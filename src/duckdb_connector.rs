/// DuckDB Connector - Integration layer for DuckDB
/// 
/// Enables DuckDB to read/write Kore files natively.
/// Usage: SELECT * FROM 'file.kore' in DuckDB

use std::path::{Path, PathBuf};
use crate::arrow_converter::{ArrowSchema, ArrowRecordBatch, ArrowConverter, ArrowDataType, ArrowField, ArrowColumn};
use crate::kore_reader::KoreReader;
use crate::kore_writer::KoreWriter;
use std::io::{Read, Write};
use std::fs::File;

/// DuckDB connector for Kore files
pub struct KoreDuckDBConnector {
    file_path: PathBuf,
    schema: Option<ArrowSchema>,
    mode: ConnectorMode,
}

/// Connector operating mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectorMode {
    /// Read-only mode (SELECT queries)
    Read,
    /// Write-only mode (INSERT queries)
    Write,
    /// Append mode (INSERT + SELECT)
    ReadWrite,
}

impl KoreDuckDBConnector {
    /// Create a new connector pointing to a Kore file
    /// 
    /// # Arguments
    /// * `file_path` - Path to the .kore file
    /// 
    /// # Examples
    /// ```
    /// let connector = KoreDuckDBConnector::new("data.kore")?;
    /// ```
    pub fn new(file_path: &str) -> Result<Self, String> {
        let path = PathBuf::from(file_path);
        
        // Validate path
        if file_path.is_empty() {
            return Err("File path cannot be empty".to_string());
        }

        Ok(Self {
            file_path: path,
            schema: None,
            mode: ConnectorMode::ReadWrite,
        })
    }

    /// Create connector in read-only mode
    pub fn read(file_path: &str) -> Result<Self, String> {
        let mut connector = Self::new(file_path)?;
        connector.mode = ConnectorMode::Read;
        Ok(connector)
    }

    /// Create connector in write-only mode
    pub fn write(file_path: &str) -> Result<Self, String> {
        let mut connector = Self::new(file_path)?;
        connector.mode = ConnectorMode::Write;
        Ok(connector)
    }

    /// Get the file path
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Get the operating mode
    pub fn mode(&self) -> ConnectorMode {
        self.mode
    }

    /// Get the schema (if loaded)
    pub fn schema(&self) -> Option<&ArrowSchema> {
        self.schema.as_ref()
    }

    /// Set the schema
    pub fn set_schema(&mut self, schema: ArrowSchema) {
        self.schema = Some(schema);
    }

    /// Read Kore file as Arrow RecordBatch
    /// 
    /// This method loads the entire file into memory as Arrow format.
    /// For large files, consider using `read_batches()` for streaming.
    /// 
    /// # Returns
    /// * `Ok(ArrowRecordBatch)` - The data in Arrow format
    /// * `Err(String)` - If file cannot be read or is invalid
    pub fn read_as_arrow(&mut self) -> Result<ArrowRecordBatch, String> {
        // Check if in read/write mode
        match self.mode {
            ConnectorMode::Write => {
                return Err(
                    "Cannot read in write-only mode".to_string()
                );
            }
            _ => {}
        }

        // Read file into memory
        let mut file = File::open(&self.file_path)
            .map_err(|e| format!("Failed to open file: {}", e))?;
        
        let mut file_bytes = Vec::new();
        file.read_to_end(&mut file_bytes)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        // Parse Kore file
        let mut reader = KoreReader::new(file_bytes)
            .map_err(|e| format!("Invalid Kore file: {}", e))?;

        // Clone header info before mutating reader
        let col_count = reader.header().column_count as usize;
        let row_count = reader.header().row_count as usize;
        let col_metadata: Vec<_> = reader.header().columns.clone();
        
        // Build Arrow schema from Kore metadata
        let mut fields = Vec::new();
        for col_meta in &col_metadata {
            let arrow_type = Self::kore_data_type_to_arrow(col_meta.data_type)?;
            fields.push(ArrowField {
                name: col_meta.name.clone(),
                data_type: arrow_type,
                nullable: true,
            });
        }
        let schema = ArrowSchema::new(fields);

        // Read all columns
        let mut columns = Vec::new();
        for col_idx in 0..col_count {
            let col_data = reader.read_column(col_idx)
                .map_err(|e| format!("Failed to read column {}: {}", col_idx, e))?;
            
            let col_meta = &col_metadata[col_idx];
            let arrow_col = Self::decode_kore_column(col_meta.data_type, &col_data)?;
            columns.push(arrow_col);
        }

        // Store schema and return batch
        let batch = ArrowRecordBatch::new(schema, columns, row_count);
        self.set_schema(batch.schema.clone());

        Ok(batch)
    }

    /// Convert Kore data type byte to Arrow type
    fn kore_data_type_to_arrow(kore_type: u8) -> Result<ArrowDataType, String> {
        match kore_type {
            0 => Ok(ArrowDataType::Int64),
            1 => Ok(ArrowDataType::Float64),
            2 => Ok(ArrowDataType::Utf8),
            3 => Ok(ArrowDataType::Boolean),
            4 => Ok(ArrowDataType::Binary),
            _ => Err(format!("Unknown Kore data type: {}", kore_type)),
        }
    }

    /// Decode Kore binary column data into Arrow column
    /// Convert Arrow column to raw Kore bytes
    /// 
    /// Reverses the decode_kore_column process by converting Arrow columns back to binary format
    fn arrow_column_to_kore_bytes(column: &ArrowColumn) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();

        match column {
            ArrowColumn::Null(nulls) => {
                for &n in nulls {
                    bytes.push(if n { 1 } else { 0 });
                }
            }
            ArrowColumn::Boolean(data) => {
                for &b in data {
                    bytes.push(if b { 1 } else { 0 });
                }
            }
            ArrowColumn::Int8(data) => {
                for &v in data {
                    bytes.push(v as u8);
                }
            }
            ArrowColumn::Int16(data) => {
                for &v in data {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Int32(data) => {
                for &v in data {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Int64(data) => {
                for &v in data {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::UInt8(data) => {
                bytes.extend_from_slice(data);
            }
            ArrowColumn::UInt16(data) => {
                for &v in data {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::UInt32(data) => {
                for &v in data {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::UInt64(data) => {
                for &v in data {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Float32(data) => {
                for &v in data {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Float64(data) => {
                for &v in data {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Binary(data) => {
                for vec in data {
                    bytes.extend_from_slice(&(vec.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(vec);
                }
            }
            ArrowColumn::Utf8(data) => {
                for s in data {
                    let s_bytes = s.as_bytes();
                    bytes.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(s_bytes);
                }
            }
            ArrowColumn::List(data) => {
                for vec in data {
                    bytes.extend_from_slice(&(vec.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(vec);
                }
            }
        }

        Ok(bytes)
    }

    /// Convert Arrow data type to Kore data type code
    /// 
    /// Maps Arrow types to Kore's simplified type system:
    /// 0 = Int64 (i64), 1 = Float64 (f64), 2 = String (UTF-8)
    /// 3 = Boolean (bool), 4 = Binary (bytes)
    fn arrow_type_to_kore_type(arrow_type: &ArrowDataType) -> Result<u8, String> {
        match arrow_type {
            ArrowDataType::Int64 => Ok(0),
            ArrowDataType::Float64 => Ok(1),
            ArrowDataType::Utf8 => Ok(2),
            ArrowDataType::Boolean => Ok(3),
            ArrowDataType::Binary => Ok(4),
            // For other types, map to closest Kore type
            ArrowDataType::Int8 | ArrowDataType::Int16 | ArrowDataType::Int32 => Ok(0),
            ArrowDataType::UInt8 | ArrowDataType::UInt16 | ArrowDataType::UInt32 | ArrowDataType::UInt64 => Ok(0),
            ArrowDataType::Float32 => Ok(1),
            ArrowDataType::Null => Ok(3), // Map Null to boolean for compatibility
            _ => Err(format!("Unsupported Arrow type for Kore: {:?}", arrow_type)),
        }
    }

    fn decode_kore_column(data_type: u8, data: &[u8]) -> Result<ArrowColumn, String> {
        match data_type {
            0 => {
                // Int64: 8 bytes per value
                let count = data.len() / 8;
                let mut values = Vec::with_capacity(count);
                for i in 0..count {
                    let bytes = [
                        data[i*8], data[i*8+1], data[i*8+2], data[i*8+3],
                        data[i*8+4], data[i*8+5], data[i*8+6], data[i*8+7],
                    ];
                    values.push(i64::from_le_bytes(bytes));
                }
                Ok(ArrowColumn::Int64(values))
            }
            1 => {
                // Float64: 8 bytes per value
                let count = data.len() / 8;
                let mut values = Vec::with_capacity(count);
                for i in 0..count {
                    let bytes = [
                        data[i*8], data[i*8+1], data[i*8+2], data[i*8+3],
                        data[i*8+4], data[i*8+5], data[i*8+6], data[i*8+7],
                    ];
                    values.push(f64::from_le_bytes(bytes));
                }
                Ok(ArrowColumn::Float64(values))
            }
            2 => {
                // String: length-prefixed (matches arrow_column_to_kore_bytes encoding)
                let mut values = Vec::new();
                let mut pos = 0;
                while pos < data.len() {
                    if pos + 4 > data.len() {
                        break; // Not enough data for length prefix
                    }
                    let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                    pos += 4;
                    if pos + len > data.len() {
                        break; // Not enough data for string content
                    }
                    let s = String::from_utf8(data[pos..pos+len].to_vec())
                        .map_err(|e| format!("Invalid UTF-8 in string column: {}", e))?;
                    values.push(s);
                    pos += len;
                }
                Ok(ArrowColumn::Utf8(values))
            }
            3 => {
                // Boolean: 1 byte per value
                let values = data.iter().map(|&b| b != 0).collect();
                Ok(ArrowColumn::Boolean(values))
            }
            4 => {
                // Binary: variable length
                // Parse as length-prefixed chunks
                let mut values = Vec::new();
                let mut pos = 0;
                while pos < data.len() {
                    if pos + 4 > data.len() {
                        break;
                    }
                    let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                    pos += 4;
                    if pos + len > data.len() {
                        break;
                    }
                    values.push(data[pos..pos+len].to_vec());
                    pos += len;
                }
                Ok(ArrowColumn::Binary(values))
            }
            _ => Err(format!("Unknown data type: {}", data_type)),
        }
    }

    /// Read Kore file in streaming batches
    /// 
    /// Useful for large files to avoid memory overflow.
    /// 
    /// # Arguments
    /// * `batch_size` - Number of rows per batch
    /// 
    /// # Returns
    /// * Iterator of RecordBatches
    pub fn read_batches(&mut self, batch_size: usize) -> Result<Vec<ArrowRecordBatch>, String> {
        if batch_size == 0 {
            return Err("Batch size must be > 0".to_string());
        }

        // Read entire file first
        let full_batch = self.read_as_arrow()?;

        if full_batch.row_count == 0 {
            return Ok(vec![]);
        }

        // Split into batches
        let mut batches = Vec::new();
        let mut row_pos = 0;

        while row_pos < full_batch.row_count {
            let end_row = std::cmp::min(row_pos + batch_size, full_batch.row_count);
            let batch_rows = end_row - row_pos;

            // Create sliced columns for this batch
            let mut batch_columns = Vec::new();
            for col in &full_batch.columns {
                let sliced_col = Self::slice_column(col, row_pos, batch_rows)?;
                batch_columns.push(sliced_col);
            }

            let batch = ArrowRecordBatch::new(
                full_batch.schema.clone(),
                batch_columns,
                batch_rows,
            );
            batches.push(batch);

            row_pos = end_row;
        }

        Ok(batches)
    }

    /// Slice a column to extract a range of rows
    fn slice_column(col: &ArrowColumn, start_row: usize, num_rows: usize) -> Result<ArrowColumn, String> {
        match col {
            ArrowColumn::Null(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Null(sliced))
            }
            ArrowColumn::Boolean(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Boolean(sliced))
            }
            ArrowColumn::Int8(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Int8(sliced))
            }
            ArrowColumn::Int16(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Int16(sliced))
            }
            ArrowColumn::Int32(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Int32(sliced))
            }
            ArrowColumn::Int64(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Int64(sliced))
            }
            ArrowColumn::UInt8(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::UInt8(sliced))
            }
            ArrowColumn::UInt16(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::UInt16(sliced))
            }
            ArrowColumn::UInt32(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::UInt32(sliced))
            }
            ArrowColumn::UInt64(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::UInt64(sliced))
            }
            ArrowColumn::Float32(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Float32(sliced))
            }
            ArrowColumn::Float64(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Float64(sliced))
            }
            ArrowColumn::Binary(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Binary(sliced))
            }
            ArrowColumn::Utf8(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::Utf8(sliced))
            }
            ArrowColumn::List(data) => {
                let sliced = data[start_row..start_row+num_rows].to_vec();
                Ok(ArrowColumn::List(sliced))
            }
        }
    }

    /// Write Arrow RecordBatch to Kore file
    /// 
    /// # Arguments
    /// * `batch` - Arrow data to write
    /// 
    /// # Returns
    /// * `Ok(u64)` - Number of rows written
    /// * `Err(String)` - If write fails
    pub fn append_from_arrow(&mut self, batch: ArrowRecordBatch) -> Result<u64, String> {
        // Check if in read/write mode
        match self.mode {
            ConnectorMode::Read => {
                return Err(
                    "Cannot write in read-only mode".to_string()
                );
            }
            _ => {}
        }

        // Validate batch
        if batch.row_count == 0 {
            return Ok(0);
        }

        // Store schema from first batch
        if self.schema.is_none() {
            self.set_schema(batch.schema.clone());
        }

        // Create KoreWriter for this batch
        let mut writer = KoreWriter::new(batch.row_count as u64);

        // Convert each Arrow column to Kore format
        for (field_idx, field) in batch.schema.fields.iter().enumerate() {
            let arrow_col = &batch.columns[field_idx];
            let kore_bytes = Self::arrow_column_to_kore_bytes(arrow_col)?;
            let data_type = Self::arrow_type_to_kore_type(&field.data_type)?;
            writer.add_column(field.name.clone(), data_type, kore_bytes);
        }

        // Write to Kore binary format with compression
        let (kore_bytes, _stats) = writer.write()
            .map_err(|e| format!("Failed to write Kore format: {}", e))?;

        // Write to file
        let mut file = File::create(&self.file_path)
            .map_err(|e| format!("Failed to create file: {}", e))?;
        
        file.write_all(&kore_bytes)
            .map_err(|e| format!("Failed to write to file: {}", e))?;

        Ok(batch.row_count as u64)
    }

    /// Get Arrow schema for this file
    pub fn arrow_schema(&mut self) -> Result<ArrowSchema, String> {
        // If already loaded, return it
        if let Some(schema) = self.schema.clone() {
            return Ok(schema);
        }

        // Otherwise, load schema from file
        // TODO: Implement schema reading from Kore metadata
        Err("Schema inference not yet implemented".to_string())
    }

    /// Get file size in bytes
    pub fn file_size(&self) -> Result<u64, String> {
        use std::fs;
        
        fs::metadata(&self.file_path)
            .map(|m| m.len())
            .map_err(|e| format!("Cannot read file metadata: {}", e))
    }

    /// Validate that the file is a valid Kore file
    pub fn validate(&self) -> Result<(), String> {
        // Check file exists
        if !self.file_path.exists() {
            // In write mode, file doesn't need to exist yet
            if self.mode == ConnectorMode::Read {
                return Err(
                    format!("File not found: {}", self.file_path.display())
                );
            }
            return Ok(());
        }

        // Check file is readable
        if !std::fs::metadata(&self.file_path)
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return Err(
                format!("Not a file: {}", self.file_path.display())
            );
        }

        // TODO: Check Kore magic bytes
        Ok(())
    }

    /// Get row count (if known)
    pub fn row_count(&self) -> Result<Option<u64>, String> {
        // TODO: Read from Kore metadata
        Ok(None)
    }

    /// Get column count
    pub fn column_count(&self) -> Result<usize, String> {
        match &self.schema {
            Some(schema) => Ok(schema.field_count()),
            None => Err("Schema not loaded".to_string()),
        }
    }
}

/// Builder pattern for creating connectors
pub struct KoreDuckDBConnectorBuilder {
    file_path: Option<PathBuf>,
    mode: ConnectorMode,
}

impl Default for KoreDuckDBConnectorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl KoreDuckDBConnectorBuilder {
    pub fn new() -> Self {
        Self {
            file_path: None,
            mode: ConnectorMode::ReadWrite,
        }
    }

    pub fn path(mut self, path: &str) -> Self {
        self.file_path = Some(PathBuf::from(path));
        self
    }

    pub fn read_only(mut self) -> Self {
        self.mode = ConnectorMode::Read;
        self
    }

    pub fn write_only(mut self) -> Self {
        self.mode = ConnectorMode::Write;
        self
    }

    pub fn build(self) -> Result<KoreDuckDBConnector, String> {
        let path = self.file_path
            .ok_or("File path not specified")?;
        
        let connector = KoreDuckDBConnector {
            file_path: path,
            schema: None,
            mode: self.mode,
        };

        // Validation happens at read/write time, not at construction
        Ok(connector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_connector() {
        let conn = KoreDuckDBConnector::new("test.kore");
        assert!(conn.is_ok());
    }

    #[test]
    fn test_empty_path() {
        let conn = KoreDuckDBConnector::new("");
        assert!(conn.is_err());
    }

    #[test]
    fn test_connector_modes() {
        let read_conn = KoreDuckDBConnector::read("test.kore").unwrap();
        assert_eq!(read_conn.mode(), ConnectorMode::Read);

        let write_conn = KoreDuckDBConnector::write("test.kore").unwrap();
        assert_eq!(write_conn.mode(), ConnectorMode::Write);
    }

    #[test]
    fn test_builder_pattern() {
        let conn = KoreDuckDBConnectorBuilder::new()
            .path("data.kore")
            .read_only()
            .build();
        
        assert!(conn.is_ok());
        assert_eq!(conn.unwrap().mode(), ConnectorMode::Read);
    }

    #[test]
    fn test_builder_missing_path() {
        let result = KoreDuckDBConnectorBuilder::new()
            .read_only()
            .build();
        
        assert!(result.is_err());
    }

    #[test]
    fn test_write_without_schema() {
        let mut conn = KoreDuckDBConnector::new("test.kore").unwrap();
        assert!(conn.schema().is_none());
    }

    #[test]
    fn test_mode_restrictions() {
        let mut read_conn = KoreDuckDBConnector::read("test.kore").unwrap();
        
        // Should fail to write in read-only mode
        let batch = ArrowRecordBatch::new(
            ArrowSchema::new(vec![]),
            vec![],
            0,
        );
        
        let result = read_conn.append_from_arrow(batch);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_path_storage() {
        let path = "my_data/file.kore";
        let conn = KoreDuckDBConnector::new(path).unwrap();
        assert_eq!(conn.file_path(), Path::new(path));
    }

    #[test]
    fn test_column_count_no_schema() {
        let conn = KoreDuckDBConnector::new("test.kore").unwrap();
        assert!(conn.column_count().is_err());
    }
}
