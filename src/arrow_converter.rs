/// Arrow Type Conversion and Serialization Layer
/// 
/// Bridges between Kore's native format and Apache Arrow for DuckDB integration.
/// This module handles all conversions between Kore data types and Arrow data types.

use std::sync::Arc;

/// Represents Arrow data types supported by Kore
#[derive(Debug, Clone, PartialEq)]
pub enum ArrowDataType {
    Null,
    Boolean,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Binary,
    Utf8,
    Date32,
    Date64,
    Timestamp(TimeUnit),
    List(Box<ArrowDataType>),
    Struct(Vec<(String, ArrowDataType)>),
}

/// Arrow timestamp precision options
#[derive(Debug, Clone, PartialEq)]
pub enum TimeUnit {
    Second,
    Millisecond,
    Microsecond,
    Nanosecond,
}

/// Arrow schema field definition
#[derive(Debug, Clone)]
pub struct ArrowField {
    pub name: String,
    pub data_type: ArrowDataType,
    pub nullable: bool,
}

/// Arrow schema definition
#[derive(Debug, Clone)]
pub struct ArrowSchema {
    pub fields: Vec<ArrowField>,
}

impl ArrowSchema {
    pub fn new(fields: Vec<ArrowField>) -> Self {
        Self { fields }
    }

    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    pub fn find_field(&self, name: &str) -> Option<&ArrowField> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// Arrow RecordBatch - columnar data container
#[derive(Debug, Clone)]
pub struct ArrowRecordBatch {
    pub schema: ArrowSchema,
    pub columns: Vec<ArrowColumn>,
    pub row_count: usize,
}

impl ArrowRecordBatch {
    pub fn new(schema: ArrowSchema, columns: Vec<ArrowColumn>, row_count: usize) -> Self {
        Self {
            schema,
            columns,
            row_count,
        }
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn column(&self, index: usize) -> Option<&ArrowColumn> {
        self.columns.get(index)
    }
}

/// Arrow array data container
#[derive(Debug, Clone)]
pub enum ArrowColumn {
    Null(Vec<bool>), // nullability bitmap
    Boolean(Vec<bool>),
    Int8(Vec<i8>),
    Int16(Vec<i16>),
    Int32(Vec<i32>),
    Int64(Vec<i64>),
    UInt8(Vec<u8>),
    UInt16(Vec<u16>),
    UInt32(Vec<u32>),
    UInt64(Vec<u64>),
    Float32(Vec<f32>),
    Float64(Vec<f64>),
    Binary(Vec<Vec<u8>>),
    Utf8(Vec<String>),
    List(Vec<Vec<u8>>), // Variable length data
}

/// Arrow Type Converter - Maps between Kore and Arrow types
pub struct ArrowConverter;

impl ArrowConverter {
    /// Convert Kore data type string to Arrow data type
    /// 
    /// # Examples
    /// ```
    /// let arrow_type = ArrowConverter::kore_type_to_arrow("i32");
    /// assert_eq!(arrow_type, Ok(ArrowDataType::Int32));
    /// ```
    pub fn kore_type_to_arrow(kore_type: &str) -> Result<ArrowDataType, String> {
        match kore_type.to_lowercase().as_str() {
            "bool" | "boolean" => Ok(ArrowDataType::Boolean),
            "i8" | "int8" => Ok(ArrowDataType::Int8),
            "i16" | "int16" => Ok(ArrowDataType::Int16),
            "i32" | "int32" => Ok(ArrowDataType::Int32),
            "i64" | "int64" => Ok(ArrowDataType::Int64),
            "u8" | "uint8" => Ok(ArrowDataType::UInt8),
            "u16" | "uint16" => Ok(ArrowDataType::UInt16),
            "u32" | "uint32" => Ok(ArrowDataType::UInt32),
            "u64" | "uint64" => Ok(ArrowDataType::UInt64),
            "f32" | "float32" | "float" => Ok(ArrowDataType::Float32),
            "f64" | "float64" | "double" => Ok(ArrowDataType::Float64),
            "bytes" | "binary" => Ok(ArrowDataType::Binary),
            "string" | "utf8" | "str" => Ok(ArrowDataType::Utf8),
            "date32" => Ok(ArrowDataType::Date32),
            "date64" => Ok(ArrowDataType::Date64),
            "timestamp_ms" => Ok(ArrowDataType::Timestamp(TimeUnit::Millisecond)),
            "timestamp_us" => Ok(ArrowDataType::Timestamp(TimeUnit::Microsecond)),
            "timestamp_ns" => Ok(ArrowDataType::Timestamp(TimeUnit::Nanosecond)),
            _ => Err(format!("Unsupported Kore type: {}", kore_type)),
        }
    }

    /// Convert Arrow data type to Kore data type string
    pub fn arrow_type_to_kore(arrow_type: &ArrowDataType) -> Result<String, String> {
        match arrow_type {
            ArrowDataType::Null => Ok("null".to_string()),
            ArrowDataType::Boolean => Ok("bool".to_string()),
            ArrowDataType::Int8 => Ok("i8".to_string()),
            ArrowDataType::Int16 => Ok("i16".to_string()),
            ArrowDataType::Int32 => Ok("i32".to_string()),
            ArrowDataType::Int64 => Ok("i64".to_string()),
            ArrowDataType::UInt8 => Ok("u8".to_string()),
            ArrowDataType::UInt16 => Ok("u16".to_string()),
            ArrowDataType::UInt32 => Ok("u32".to_string()),
            ArrowDataType::UInt64 => Ok("u64".to_string()),
            ArrowDataType::Float32 => Ok("f32".to_string()),
            ArrowDataType::Float64 => Ok("f64".to_string()),
            ArrowDataType::Binary => Ok("bytes".to_string()),
            ArrowDataType::Utf8 => Ok("string".to_string()),
            ArrowDataType::Date32 => Ok("date32".to_string()),
            ArrowDataType::Date64 => Ok("date64".to_string()),
            ArrowDataType::Timestamp(unit) => {
                let unit_str = match unit {
                    TimeUnit::Second => "s",
                    TimeUnit::Millisecond => "ms",
                    TimeUnit::Microsecond => "us",
                    TimeUnit::Nanosecond => "ns",
                };
                Ok(format!("timestamp_{}", unit_str))
            }
            _ => Err(format!("Unsupported Arrow type: {:?}", arrow_type)),
        }
    }

    /// Infer Arrow schema from data
    /// 
    /// Used when reading Kore files to generate Arrow-compatible schema
    pub fn infer_schema_from_columns(
        column_names: &[String],
        column_types: &[String],
    ) -> Result<ArrowSchema, String> {
        if column_names.len() != column_types.len() {
            return Err(
                "Column names and types must have the same length".to_string()
            );
        }

        let fields = column_names
            .iter()
            .zip(column_types.iter())
            .map(|(name, type_str)| {
                let data_type = Self::kore_type_to_arrow(type_str)?;
                Ok(ArrowField {
                    name: name.clone(),
                    data_type,
                    nullable: true, // Assume nullable by default
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        Ok(ArrowSchema::new(fields))
    }

    /// Convert Arrow RecordBatch to bytes for storage
    /// 
    /// Uses a custom binary format:
    /// - Magic bytes: "ARRB"
    /// - Version: 1
    /// - Row count, column count, schema metadata, column data
    pub fn serialize_batch(batch: &ArrowRecordBatch) -> Result<Vec<u8>, String> {
        let mut buffer = Vec::new();

        // Write magic bytes
        buffer.extend_from_slice(b"ARRB");

        // Write version
        buffer.extend_from_slice(&1u32.to_le_bytes());

        // Write dimensions
        buffer.extend_from_slice(&(batch.row_count as u32).to_le_bytes());
        buffer.extend_from_slice(&(batch.column_count() as u32).to_le_bytes());

        // Write schema metadata
        for field in &batch.schema.fields {
            // Field name
            let name_bytes = field.name.as_bytes();
            buffer.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
            buffer.extend_from_slice(name_bytes);

            // Data type enum (simplified: convert to string and encode)
            let type_str = Self::arrow_type_to_kore(&field.data_type)?;
            let type_bytes = type_str.as_bytes();
            buffer.extend_from_slice(&(type_bytes.len() as u32).to_le_bytes());
            buffer.extend_from_slice(type_bytes);

            // Nullable flag
            buffer.push(if field.nullable { 1 } else { 0 });
        }

        // Write column data
        for column in &batch.columns {
            let column_bytes = Self::serialize_column(column)?;
            buffer.extend_from_slice(&(column_bytes.len() as u32).to_le_bytes());
            buffer.extend_from_slice(&column_bytes);
        }

        Ok(buffer)
    }

    /// Serialize a single Arrow column to bytes
    fn serialize_column(column: &ArrowColumn) -> Result<Vec<u8>, String> {
        let mut buffer = Vec::new();

        match column {
            ArrowColumn::Null(nulls) => {
                buffer.push(0u8); // Type byte for Null
                buffer.extend_from_slice(&(nulls.len() as u32).to_le_bytes());
                for &n in nulls {
                    buffer.push(if n { 1 } else { 0 });
                }
            }
            ArrowColumn::Boolean(data) => {
                buffer.push(1u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &b in data {
                    buffer.push(if b { 1 } else { 0 });
                }
            }
            ArrowColumn::Int8(data) => {
                buffer.push(2u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.push(v as u8);
                }
            }
            ArrowColumn::Int16(data) => {
                buffer.push(3u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Int32(data) => {
                buffer.push(4u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Int64(data) => {
                buffer.push(5u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::UInt8(data) => {
                buffer.push(6u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                buffer.extend_from_slice(data);
            }
            ArrowColumn::UInt16(data) => {
                buffer.push(7u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::UInt32(data) => {
                buffer.push(8u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::UInt64(data) => {
                buffer.push(9u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Float32(data) => {
                buffer.push(10u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Float64(data) => {
                buffer.push(11u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for &v in data {
                    buffer.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Binary(data) => {
                buffer.push(12u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for bytes in data {
                    buffer.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    buffer.extend_from_slice(bytes);
                }
            }
            ArrowColumn::Utf8(data) => {
                buffer.push(13u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for s in data {
                    let str_bytes = s.as_bytes();
                    buffer.extend_from_slice(&(str_bytes.len() as u32).to_le_bytes());
                    buffer.extend_from_slice(str_bytes);
                }
            }
            ArrowColumn::List(data) => {
                buffer.push(14u8);
                buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
                for bytes in data {
                    buffer.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    buffer.extend_from_slice(bytes);
                }
            }
        }

        Ok(buffer)
    }

    /// Convert bytes back to Arrow RecordBatch
    pub fn deserialize_batch(
        schema: &ArrowSchema,
        data: &[u8],
    ) -> Result<ArrowRecordBatch, String> {
        if data.len() < 12 {
            return Err("Data too short for Arrow batch header".to_string());
        }

        let mut pos = 0;

        // Check magic bytes
        if &data[pos..pos+4] != b"ARRB" {
            return Err("Invalid Arrow batch magic bytes".to_string());
        }
        pos += 4;

        // Check version
        let version = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
        pos += 4;
        if version != 1 {
            return Err(format!("Unsupported Arrow batch version: {}", version));
        }

        // Read dimensions
        let row_count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        pos += 4;
        let col_count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        pos += 4;

        if col_count != schema.fields.len() {
            return Err(format!("Column count mismatch: expected {}, got {}", schema.fields.len(), col_count));
        }

        // Read schema metadata (skip, we have the schema already)
        for _ in 0..col_count {
            // Skip field name
            let name_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            pos += 4 + name_len;

            // Skip type string
            let type_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            pos += 4 + type_len;

            // Skip nullable flag
            pos += 1;
        }

        // Read column data
        let mut columns = Vec::new();
        for _ in 0..col_count {
            let col_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            pos += 4;

            let col_data = &data[pos..pos+col_len];
            pos += col_len;

            let column = Self::deserialize_column(col_data)?;
            columns.push(column);
        }

        Ok(ArrowRecordBatch::new(schema.clone(), columns, row_count))
    }

    /// Deserialize a single Arrow column from bytes
    fn deserialize_column(data: &[u8]) -> Result<ArrowColumn, String> {
        if data.is_empty() {
            return Err("Column data is empty".to_string());
        }

        let type_byte = data[0];
        let mut pos = 1;

        if data.len() < 5 {
            return Err("Column data too short for count".to_string());
        }

        let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        pos += 4;

        match type_byte {
            0 => {
                let nulls = data[pos..pos+count].iter().map(|&b| b != 0).collect();
                Ok(ArrowColumn::Null(nulls))
            }
            1 => {
                let bools = data[pos..pos+count].iter().map(|&b| b != 0).collect();
                Ok(ArrowColumn::Boolean(bools))
            }
            2 => {
                let vals = data[pos..pos+count].iter().map(|&b| b as i8).collect();
                Ok(ArrowColumn::Int8(vals))
            }
            3 => {
                let mut vals = Vec::new();
                for i in 0..count {
                    let p = pos + i * 2;
                    vals.push(i16::from_le_bytes([data[p], data[p+1]]));
                }
                Ok(ArrowColumn::Int16(vals))
            }
            4 => {
                let mut vals = Vec::new();
                for i in 0..count {
                    let p = pos + i * 4;
                    vals.push(i32::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3]]));
                }
                Ok(ArrowColumn::Int32(vals))
            }
            5 => {
                let mut vals = Vec::new();
                for i in 0..count {
                    let p = pos + i * 8;
                    vals.push(i64::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3], data[p+4], data[p+5], data[p+6], data[p+7]]));
                }
                Ok(ArrowColumn::Int64(vals))
            }
            6 => {
                let vals = data[pos..pos+count].to_vec();
                Ok(ArrowColumn::UInt8(vals))
            }
            7 => {
                let mut vals = Vec::new();
                for i in 0..count {
                    let p = pos + i * 2;
                    vals.push(u16::from_le_bytes([data[p], data[p+1]]));
                }
                Ok(ArrowColumn::UInt16(vals))
            }
            8 => {
                let mut vals = Vec::new();
                for i in 0..count {
                    let p = pos + i * 4;
                    vals.push(u32::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3]]));
                }
                Ok(ArrowColumn::UInt32(vals))
            }
            9 => {
                let mut vals = Vec::new();
                for i in 0..count {
                    let p = pos + i * 8;
                    vals.push(u64::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3], data[p+4], data[p+5], data[p+6], data[p+7]]));
                }
                Ok(ArrowColumn::UInt64(vals))
            }
            10 => {
                let mut vals = Vec::new();
                for i in 0..count {
                    let p = pos + i * 4;
                    vals.push(f32::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3]]));
                }
                Ok(ArrowColumn::Float32(vals))
            }
            11 => {
                let mut vals = Vec::new();
                for i in 0..count {
                    let p = pos + i * 8;
                    vals.push(f64::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3], data[p+4], data[p+5], data[p+6], data[p+7]]));
                }
                Ok(ArrowColumn::Float64(vals))
            }
            12 => {
                let mut vals = Vec::new();
                let mut p = pos;
                for _ in 0..count {
                    let len = u32::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3]]) as usize;
                    p += 4;
                    vals.push(data[p..p+len].to_vec());
                    p += len;
                }
                Ok(ArrowColumn::Binary(vals))
            }
            13 => {
                let mut vals = Vec::new();
                let mut p = pos;
                for _ in 0..count {
                    let len = u32::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3]]) as usize;
                    p += 4;
                    let s = String::from_utf8(data[p..p+len].to_vec())
                        .map_err(|e| format!("UTF-8 decode error: {}", e))?;
                    vals.push(s);
                    p += len;
                }
                Ok(ArrowColumn::Utf8(vals))
            }
            14 => {
                let mut vals = Vec::new();
                let mut p = pos;
                for _ in 0..count {
                    let len = u32::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3]]) as usize;
                    p += 4;
                    vals.push(data[p..p+len].to_vec());
                    p += len;
                }
                Ok(ArrowColumn::List(vals))
            }
            _ => Err(format!("Unknown column type byte: {}", type_byte)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kore_type_to_arrow() {
        assert_eq!(
            ArrowConverter::kore_type_to_arrow("i32").unwrap(),
            ArrowDataType::Int32
        );
        assert_eq!(
            ArrowConverter::kore_type_to_arrow("string").unwrap(),
            ArrowDataType::Utf8
        );
        assert_eq!(
            ArrowConverter::kore_type_to_arrow("bool").unwrap(),
            ArrowDataType::Boolean
        );
    }

    #[test]
    fn test_arrow_type_to_kore() {
        assert_eq!(
            ArrowConverter::arrow_type_to_kore(&ArrowDataType::Int32).unwrap(),
            "i32"
        );
        assert_eq!(
            ArrowConverter::arrow_type_to_kore(&ArrowDataType::Utf8).unwrap(),
            "string"
        );
    }

    #[test]
    fn test_infer_schema() {
        let names = vec!["id".to_string(), "name".to_string()];
        let types = vec!["i64".to_string(), "string".to_string()];

        let schema = ArrowConverter::infer_schema_from_columns(&names, &types).unwrap();

        assert_eq!(schema.field_count(), 2);
        assert_eq!(schema.fields[0].name, "id");
        assert_eq!(schema.fields[0].data_type, ArrowDataType::Int64);
        assert_eq!(schema.fields[1].name, "name");
        assert_eq!(schema.fields[1].data_type, ArrowDataType::Utf8);
    }

    #[test]
    fn test_schema_field_lookup() {
        let field1 = ArrowField {
            name: "id".to_string(),
            data_type: ArrowDataType::Int64,
            nullable: false,
        };
        let field2 = ArrowField {
            name: "value".to_string(),
            data_type: ArrowDataType::Float64,
            nullable: true,
        };
        let schema = ArrowSchema::new(vec![field1, field2]);

        assert!(schema.find_field("id").is_some());
        assert!(schema.find_field("value").is_some());
        assert!(schema.find_field("notfound").is_none());
    }

    #[test]
    fn test_record_batch() {
        let schema = ArrowSchema::new(vec![ArrowField {
            name: "values".to_string(),
            data_type: ArrowDataType::Int32,
            nullable: false,
        }]);

        let col = ArrowColumn::Int32(vec![1, 2, 3, 4, 5]);
        let batch = ArrowRecordBatch::new(schema, vec![col], 5);

        assert_eq!(batch.row_count, 5);
        assert_eq!(batch.column_count(), 1);
    }

    #[test]
    fn test_type_conversion_roundtrip() {
        let types = vec!["i8", "i32", "i64", "f32", "f64", "bool", "string"];

        for type_str in types {
            let arrow_type = ArrowConverter::kore_type_to_arrow(type_str)
                .expect(&format!("Failed to convert {}", type_str));
            let kore_type = ArrowConverter::arrow_type_to_kore(&arrow_type)
                .expect(&format!("Failed to convert back {:?}", arrow_type));

            // Allow some flexibility in naming (e.g., "string" vs "utf8")
            assert_eq!(kore_type.as_str(), type_str);
        }
    }

    #[test]
    fn test_unsupported_type_error() {
        let result = ArrowConverter::kore_type_to_arrow("unknown_type");
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_deserialize_int32_batch() {
        let schema = ArrowSchema::new(vec![
            ArrowField {
                name: "id".to_string(),
                data_type: ArrowDataType::Int32,
                nullable: false,
            },
        ]);

        let col = ArrowColumn::Int32(vec![10, 20, 30, 40, 50]);
        let original_batch = ArrowRecordBatch::new(schema.clone(), vec![col], 5);

        // Serialize
        let serialized = ArrowConverter::serialize_batch(&original_batch).unwrap();
        assert!(!serialized.is_empty());
        assert_eq!(&serialized[0..4], b"ARRB"); // Check magic bytes

        // Deserialize
        let deserialized = ArrowConverter::deserialize_batch(&schema, &serialized).unwrap();
        assert_eq!(deserialized.row_count, 5);
        assert_eq!(deserialized.column_count(), 1);

        // Verify data
        if let ArrowColumn::Int32(vals) = &deserialized.columns[0] {
            assert_eq!(vals, &vec![10, 20, 30, 40, 50]);
        } else {
            panic!("Expected Int32 column");
        }
    }

    #[test]
    fn test_serialize_deserialize_string_batch() {
        let schema = ArrowSchema::new(vec![
            ArrowField {
                name: "name".to_string(),
                data_type: ArrowDataType::Utf8,
                nullable: true,
            },
        ]);

        let col = ArrowColumn::Utf8(vec!["alice".to_string(), "bob".to_string(), "charlie".to_string()]);
        let original_batch = ArrowRecordBatch::new(schema.clone(), vec![col], 3);

        // Serialize and deserialize
        let serialized = ArrowConverter::serialize_batch(&original_batch).unwrap();
        let deserialized = ArrowConverter::deserialize_batch(&schema, &serialized).unwrap();

        assert_eq!(deserialized.row_count, 3);

        if let ArrowColumn::Utf8(vals) = &deserialized.columns[0] {
            assert_eq!(vals[0], "alice");
            assert_eq!(vals[1], "bob");
            assert_eq!(vals[2], "charlie");
        } else {
            panic!("Expected Utf8 column");
        }
    }

    #[test]
    fn test_serialize_deserialize_multiple_columns() {
        let schema = ArrowSchema::new(vec![
            ArrowField {
                name: "id".to_string(),
                data_type: ArrowDataType::Int32,
                nullable: false,
            },
            ArrowField {
                name: "value".to_string(),
                data_type: ArrowDataType::Float64,
                nullable: true,
            },
            ArrowField {
                name: "active".to_string(),
                data_type: ArrowDataType::Boolean,
                nullable: false,
            },
        ]);

        let col1 = ArrowColumn::Int32(vec![1, 2, 3]);
        let col2 = ArrowColumn::Float64(vec![1.5, 2.5, 3.5]);
        let col3 = ArrowColumn::Boolean(vec![true, false, true]);

        let batch = ArrowRecordBatch::new(schema.clone(), vec![col1, col2, col3], 3);

        // Serialize and deserialize
        let serialized = ArrowConverter::serialize_batch(&batch).unwrap();
        let deserialized = ArrowConverter::deserialize_batch(&schema, &serialized).unwrap();

        assert_eq!(deserialized.row_count, 3);
        assert_eq!(deserialized.column_count(), 3);

        // Verify each column
        if let ArrowColumn::Int32(vals) = &deserialized.columns[0] {
            assert_eq!(vals, &vec![1, 2, 3]);
        } else {
            panic!("Expected Int32 column");
        }

        if let ArrowColumn::Float64(vals) = &deserialized.columns[1] {
            assert_eq!(vals, &vec![1.5, 2.5, 3.5]);
        } else {
            panic!("Expected Float64 column");
        }

        if let ArrowColumn::Boolean(vals) = &deserialized.columns[2] {
            assert_eq!(vals, &vec![true, false, true]);
        } else {
            panic!("Expected Boolean column");
        }
    }

    #[test]
    fn test_serialize_deserialize_binary_column() {
        let schema = ArrowSchema::new(vec![
            ArrowField {
                name: "data".to_string(),
                data_type: ArrowDataType::Binary,
                nullable: true,
            },
        ]);

        let binary_data = vec![
            vec![1, 2, 3],
            vec![4, 5],
            vec![6, 7, 8, 9],
        ];
        let col = ArrowColumn::Binary(binary_data.clone());
        let batch = ArrowRecordBatch::new(schema.clone(), vec![col], 3);

        // Serialize and deserialize
        let serialized = ArrowConverter::serialize_batch(&batch).unwrap();
        let deserialized = ArrowConverter::deserialize_batch(&schema, &serialized).unwrap();

        if let ArrowColumn::Binary(vals) = &deserialized.columns[0] {
            assert_eq!(vals, &binary_data);
        } else {
            panic!("Expected Binary column");
        }
    }

    #[test]
    fn test_serialize_empty_batch() {
        let schema = ArrowSchema::new(vec![
            ArrowField {
                name: "id".to_string(),
                data_type: ArrowDataType::Int32,
                nullable: false,
            },
        ]);

        let col = ArrowColumn::Int32(vec![]);
        let batch = ArrowRecordBatch::new(schema.clone(), vec![col], 0);

        let serialized = ArrowConverter::serialize_batch(&batch).unwrap();
        let deserialized = ArrowConverter::deserialize_batch(&schema, &serialized).unwrap();

        assert_eq!(deserialized.row_count, 0);
        assert_eq!(deserialized.column_count(), 1);
    }

    #[test]
    fn test_deserialize_invalid_magic_bytes() {
        let schema = ArrowSchema::new(vec![]);
        let invalid_data = b"XXXX";

        let result = ArrowConverter::deserialize_batch(&schema, invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_deserialize_all_integer_types() {
        let schema = ArrowSchema::new(vec![
            ArrowField { name: "i8_col".to_string(), data_type: ArrowDataType::Int8, nullable: false },
            ArrowField { name: "i16_col".to_string(), data_type: ArrowDataType::Int16, nullable: false },
            ArrowField { name: "i64_col".to_string(), data_type: ArrowDataType::Int64, nullable: false },
            ArrowField { name: "u8_col".to_string(), data_type: ArrowDataType::UInt8, nullable: false },
            ArrowField { name: "u32_col".to_string(), data_type: ArrowDataType::UInt32, nullable: false },
        ]);

        let columns = vec![
            ArrowColumn::Int8(vec![-1, -2, -3]),
            ArrowColumn::Int16(vec![-100, -200, -300]),
            ArrowColumn::Int64(vec![-1000, -2000, -3000]),
            ArrowColumn::UInt8(vec![1, 2, 3]),
            ArrowColumn::UInt32(vec![100, 200, 300]),
        ];

        let batch = ArrowRecordBatch::new(schema.clone(), columns, 3);
        let serialized = ArrowConverter::serialize_batch(&batch).unwrap();
        let deserialized = ArrowConverter::deserialize_batch(&schema, &serialized).unwrap();

        assert_eq!(deserialized.row_count, 3);
        assert_eq!(deserialized.column_count(), 5);
    }
}
