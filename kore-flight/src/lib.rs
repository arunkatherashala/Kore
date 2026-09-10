//! KORE Phase 4A — Arrow IPC serialization & Flight-style service interface.
//!
//! Self-contained Arrow IPC abstraction: no dependency on the real `arrow` or
//! `tonic` crates. Provides:
//! - Arrow schema/field/batch types mirroring the Arrow columnar format
//! - Binary IPC serialization (length-prefixed, column-at-a-time)
//! - Flight service trait and concrete `KoreFlightService` implementation

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use kore_core::{Column, ColumnData, DataBlock, DataType, KoreError};

// ─── Arrow IPC types ─────────────────────────────────────────────────────────

/// Arrow-compatible data type identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowDataType {
    Int64,
    Float64,
    Utf8,
    Boolean,
}

impl From<DataType> for ArrowDataType {
    fn from(dt: DataType) -> Self {
        match dt {
            DataType::Int64   => ArrowDataType::Int64,
            DataType::Float64 => ArrowDataType::Float64,
            DataType::Str     => ArrowDataType::Utf8,
            DataType::Bool    => ArrowDataType::Boolean,
        }
    }
}

impl ArrowDataType {
    pub fn to_kore_type(self) -> DataType {
        match self {
            ArrowDataType::Int64   => DataType::Int64,
            ArrowDataType::Float64 => DataType::Float64,
            ArrowDataType::Utf8    => DataType::Str,
            ArrowDataType::Boolean => DataType::Bool,
        }
    }

    fn type_tag(self) -> u8 {
        match self {
            ArrowDataType::Int64   => 0,
            ArrowDataType::Float64 => 1,
            ArrowDataType::Utf8    => 2,
            ArrowDataType::Boolean => 3,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, KoreError> {
        match tag {
            0 => Ok(ArrowDataType::Int64),
            1 => Ok(ArrowDataType::Float64),
            2 => Ok(ArrowDataType::Utf8),
            3 => Ok(ArrowDataType::Boolean),
            _ => Err(KoreError::InvalidArgument(format!("unknown arrow type tag: {tag}"))),
        }
    }
}

/// A single field in an Arrow schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrowField {
    pub name: String,
    pub data_type: ArrowDataType,
}

/// Arrow schema: ordered list of fields describing columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrowSchema {
    pub fields: Vec<ArrowField>,
}

/// Column-oriented batch of data with an associated schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArrowBatch {
    pub schema: ArrowSchema,
    pub num_rows: usize,
    pub columns: Vec<ArrowColumn>,
}

/// A single column's data in Arrow columnar layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArrowColumn {
    Int64 { validity: Vec<bool>, values: Vec<i64> },
    Float64 { validity: Vec<bool>, values: Vec<f64> },
    Utf8 { validity: Vec<bool>, values: Vec<String> },
    Boolean { validity: Vec<bool>, values: Vec<bool> },
}

// ─── Conversion: DataBlock ↔ ArrowBatch ──────────────────────────────────────

/// Convert a `DataBlock` to Arrow columnar format.
pub fn datablock_to_arrow(block: &DataBlock) -> ArrowBatch {
    let mut fields = Vec::with_capacity(block.columns.len());
    let mut columns = Vec::with_capacity(block.columns.len());

    for col in &block.columns {
        let arrow_dt = ArrowDataType::from(col.data.dtype());
        fields.push(ArrowField {
            name: col.name.clone(),
            data_type: arrow_dt,
        });

        let arrow_col = match &col.data {
            ColumnData::Int64(v) => {
                let mut validity = Vec::with_capacity(v.len());
                let mut values = Vec::with_capacity(v.len());
                for opt in v {
                    validity.push(opt.is_some());
                    values.push(opt.unwrap_or(0));
                }
                ArrowColumn::Int64 { validity, values }
            }
            ColumnData::Float64(v) => {
                let mut validity = Vec::with_capacity(v.len());
                let mut values = Vec::with_capacity(v.len());
                for opt in v {
                    validity.push(opt.is_some());
                    values.push(opt.unwrap_or(0.0));
                }
                ArrowColumn::Float64 { validity, values }
            }
            ColumnData::Bool(v) => {
                let mut validity = Vec::with_capacity(v.len());
                let mut values = Vec::with_capacity(v.len());
                for opt in v {
                    validity.push(opt.is_some());
                    values.push(opt.unwrap_or(false));
                }
                ArrowColumn::Boolean { validity, values }
            }
            ColumnData::Str(v) => {
                let mut validity = Vec::with_capacity(v.len());
                let mut values = Vec::with_capacity(v.len());
                for opt in v {
                    validity.push(opt.is_some());
                    values.push(opt.clone().unwrap_or_default());
                }
                ArrowColumn::Utf8 { validity, values }
            }
            ColumnData::StrDict { codes, dict } => {
                let mut validity = Vec::with_capacity(codes.len());
                let mut values = Vec::with_capacity(codes.len());
                for &c in codes {
                    if c == u8::MAX {
                        validity.push(false);
                        values.push(String::new());
                    } else {
                        validity.push(true);
                        values.push(dict.get(c as usize).cloned().unwrap_or_default());
                    }
                }
                ArrowColumn::Utf8 { validity, values }
            }
        };
        columns.push(arrow_col);
    }

    ArrowBatch {
        schema: ArrowSchema { fields },
        num_rows: block.num_rows,
        columns,
    }
}

/// Convert an `ArrowBatch` back to a `DataBlock`.
pub fn arrow_to_datablock(batch: &ArrowBatch) -> Result<DataBlock, KoreError> {
    if batch.columns.len() != batch.schema.fields.len() {
        return Err(KoreError::SchemaMismatch(format!(
            "schema has {} fields but batch has {} columns",
            batch.schema.fields.len(),
            batch.columns.len()
        )));
    }

    let mut cols = Vec::with_capacity(batch.columns.len());
    for (field, arrow_col) in batch.schema.fields.iter().zip(batch.columns.iter()) {
        let col_data = match arrow_col {
            ArrowColumn::Int64 { validity, values } => {
                ColumnData::Int64(
                    validity.iter().zip(values.iter())
                        .map(|(&v, &val)| if v { Some(val) } else { None })
                        .collect()
                )
            }
            ArrowColumn::Float64 { validity, values } => {
                ColumnData::Float64(
                    validity.iter().zip(values.iter())
                        .map(|(&v, &val)| if v { Some(val) } else { None })
                        .collect()
                )
            }
            ArrowColumn::Utf8 { validity, values } => {
                ColumnData::Str(
                    validity.iter().zip(values.iter())
                        .map(|(&v, val)| if v { Some(val.clone()) } else { None })
                        .collect()
                )
            }
            ArrowColumn::Boolean { validity, values } => {
                ColumnData::Bool(
                    validity.iter().zip(values.iter())
                        .map(|(&v, &val)| if v { Some(val) } else { None })
                        .collect()
                )
            }
        };
        cols.push(Column { name: field.name.clone(), data: col_data });
    }

    DataBlock::new(cols)
}

// ─── IPC Serialization ───────────────────────────────────────────────────────
//
// Binary format (self-contained, no external deps):
//   [4 bytes] magic: b"KIPC"
//   [4 bytes] num_fields (u32 LE)
//   [4 bytes] num_rows (u32 LE)
//   For each field:
//     [1 byte]  data_type tag
//     [2 bytes] name length (u16 LE)
//     [N bytes] name (UTF-8)
//   For each column (in field order):
//     [validity bitmap] ceil(num_rows / 8) bytes, bit-packed LSB-first
//     [data payload] type-specific:
//       Int64:   num_rows * 8 bytes (i64 LE)
//       Float64: num_rows * 8 bytes (f64 LE)
//       Boolean: ceil(num_rows / 8) bytes, bit-packed
//       Utf8:    [4 bytes offset_count = num_rows+1][offsets: (num_rows+1)*4 bytes u32 LE][data bytes]

const IPC_MAGIC: &[u8; 4] = b"KIPC";

/// Serialize an `ArrowBatch` to binary IPC format.
pub fn serialize_ipc(batch: &ArrowBatch) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256 + batch.num_rows * batch.columns.len() * 8);

    buf.extend_from_slice(IPC_MAGIC);
    buf.extend_from_slice(&(batch.schema.fields.len() as u32).to_le_bytes());
    buf.extend_from_slice(&(batch.num_rows as u32).to_le_bytes());

    // Schema section
    for field in &batch.schema.fields {
        buf.push(field.data_type.type_tag());
        let name_bytes = field.name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(name_bytes);
    }

    // Column data section
    for col in &batch.columns {
        match col {
            ArrowColumn::Int64 { validity, values } => {
                write_validity_bitmap(&mut buf, validity);
                for &v in values {
                    buf.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Float64 { validity, values } => {
                write_validity_bitmap(&mut buf, validity);
                for &v in values {
                    buf.extend_from_slice(&v.to_le_bytes());
                }
            }
            ArrowColumn::Boolean { validity, values } => {
                write_validity_bitmap(&mut buf, validity);
                write_bool_packed(&mut buf, values);
            }
            ArrowColumn::Utf8 { validity, values } => {
                write_validity_bitmap(&mut buf, validity);
                // Offset array: cumulative byte positions
                let mut offsets = Vec::with_capacity(values.len() + 1);
                offsets.push(0u32);
                let mut running = 0u32;
                for s in values {
                    running += s.len() as u32;
                    offsets.push(running);
                }
                buf.extend_from_slice(&(offsets.len() as u32).to_le_bytes());
                for o in &offsets {
                    buf.extend_from_slice(&o.to_le_bytes());
                }
                for s in values {
                    buf.extend_from_slice(s.as_bytes());
                }
            }
        }
    }

    buf
}

/// Deserialize an `ArrowBatch` from binary IPC bytes.
pub fn deserialize_ipc(bytes: &[u8]) -> Result<ArrowBatch, KoreError> {
    let mut pos = 0;

    let read_err = || KoreError::InvalidArgument("IPC buffer truncated".into());

    if bytes.len() < 12 {
        return Err(read_err());
    }
    if &bytes[0..4] != IPC_MAGIC {
        return Err(KoreError::InvalidArgument("invalid IPC magic".into()));
    }
    pos += 4;

    let num_fields = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;
    let num_rows = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;

    // Read schema
    let mut fields = Vec::with_capacity(num_fields);
    for _ in 0..num_fields {
        if pos >= bytes.len() { return Err(read_err()); }
        let dt = ArrowDataType::from_tag(bytes[pos])?;
        pos += 1;
        if pos + 2 > bytes.len() { return Err(read_err()); }
        let name_len = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;
        if pos + name_len > bytes.len() { return Err(read_err()); }
        let name = String::from_utf8_lossy(&bytes[pos..pos + name_len]).into_owned();
        pos += name_len;
        fields.push(ArrowField { name, data_type: dt });
    }

    // Read columns
    let validity_bytes = (num_rows + 7) / 8;
    let mut columns = Vec::with_capacity(num_fields);
    for field in &fields {
        // Validity bitmap
        if pos + validity_bytes > bytes.len() { return Err(read_err()); }
        let validity = read_validity_bitmap(&bytes[pos..pos + validity_bytes], num_rows);
        pos += validity_bytes;

        let col = match field.data_type {
            ArrowDataType::Int64 => {
                let data_len = num_rows * 8;
                if pos + data_len > bytes.len() { return Err(read_err()); }
                let mut values = Vec::with_capacity(num_rows);
                for i in 0..num_rows {
                    let start = pos + i * 8;
                    values.push(i64::from_le_bytes(bytes[start..start + 8].try_into().unwrap()));
                }
                pos += data_len;
                ArrowColumn::Int64 { validity, values }
            }
            ArrowDataType::Float64 => {
                let data_len = num_rows * 8;
                if pos + data_len > bytes.len() { return Err(read_err()); }
                let mut values = Vec::with_capacity(num_rows);
                for i in 0..num_rows {
                    let start = pos + i * 8;
                    values.push(f64::from_le_bytes(bytes[start..start + 8].try_into().unwrap()));
                }
                pos += data_len;
                ArrowColumn::Float64 { validity, values }
            }
            ArrowDataType::Boolean => {
                let packed_len = (num_rows + 7) / 8;
                if pos + packed_len > bytes.len() { return Err(read_err()); }
                let values = read_bool_packed(&bytes[pos..pos + packed_len], num_rows);
                pos += packed_len;
                ArrowColumn::Boolean { validity, values }
            }
            ArrowDataType::Utf8 => {
                if pos + 4 > bytes.len() { return Err(read_err()); }
                let offset_count = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                let offsets_len = offset_count * 4;
                if pos + offsets_len > bytes.len() { return Err(read_err()); }
                let mut offsets = Vec::with_capacity(offset_count);
                for i in 0..offset_count {
                    let start = pos + i * 4;
                    offsets.push(u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()) as usize);
                }
                pos += offsets_len;
                let total_str_bytes = offsets.last().copied().unwrap_or(0);
                if pos + total_str_bytes > bytes.len() { return Err(read_err()); }
                let mut values = Vec::with_capacity(num_rows);
                for i in 0..num_rows {
                    let s = offsets[i];
                    let e = offsets[i + 1];
                    values.push(String::from_utf8_lossy(&bytes[pos + s..pos + e]).into_owned());
                }
                pos += total_str_bytes;
                ArrowColumn::Utf8 { validity, values }
            }
        };
        columns.push(col);
    }

    Ok(ArrowBatch {
        schema: ArrowSchema { fields },
        num_rows,
        columns,
    })
}

// ─── Bitmap helpers ──────────────────────────────────────────────────────────

fn write_validity_bitmap(buf: &mut Vec<u8>, validity: &[bool]) {
    let num_bytes = (validity.len() + 7) / 8;
    for byte_idx in 0..num_bytes {
        let mut byte = 0u8;
        for bit in 0..8 {
            let idx = byte_idx * 8 + bit;
            if idx < validity.len() && validity[idx] {
                byte |= 1 << bit;
            }
        }
        buf.push(byte);
    }
}

fn read_validity_bitmap(bytes: &[u8], num_rows: usize) -> Vec<bool> {
    let mut validity = Vec::with_capacity(num_rows);
    for i in 0..num_rows {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        validity.push((bytes[byte_idx] >> bit_idx) & 1 == 1);
    }
    validity
}

fn write_bool_packed(buf: &mut Vec<u8>, values: &[bool]) {
    let num_bytes = (values.len() + 7) / 8;
    for byte_idx in 0..num_bytes {
        let mut byte = 0u8;
        for bit in 0..8 {
            let idx = byte_idx * 8 + bit;
            if idx < values.len() && values[idx] {
                byte |= 1 << bit;
            }
        }
        buf.push(byte);
    }
}

fn read_bool_packed(bytes: &[u8], count: usize) -> Vec<bool> {
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        values.push((bytes[byte_idx] >> bit_idx) & 1 == 1);
    }
    values
}

// ─── Flight service interface ────────────────────────────────────────────────

/// Identifies a dataset in the Flight service (path segments like table names).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightDescriptor {
    pub path: Vec<String>,
}

/// Metadata about an available flight (dataset).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightInfo {
    pub schema: ArrowSchema,
    pub total_records: usize,
    pub total_bytes: usize,
}

/// A ticket+location to retrieve data from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightEndpoint {
    pub ticket: Vec<u8>,
    pub location: String,
}

/// Actions the Flight service can perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlightAction {
    GetFlightInfo,
    DoGet { ticket: Vec<u8> },
    DoPut { descriptor: FlightDescriptor },
    ListFlights,
}

/// Result of a Flight data retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightResult {
    pub batches: Vec<ArrowBatch>,
}

/// Flight service trait — Arrow Flight-style data access.
pub trait FlightService: Send + Sync {
    fn get_flight_info(&self, descriptor: &FlightDescriptor) -> Result<FlightInfo, KoreError>;
    fn do_get(&self, ticket: &[u8]) -> Result<Vec<ArrowBatch>, KoreError>;
    fn do_put(&self, descriptor: &FlightDescriptor, batches: Vec<ArrowBatch>) -> Result<(), KoreError>;
    fn list_flights(&self) -> Result<Vec<FlightInfo>, KoreError>;
}

// ─── KoreFlightService ───────────────────────────────────────────────────────

/// Concrete Flight service backed by a `KqlContext`-like table registry.
///
/// Uses an internal table map to avoid depending directly on `kore-sql`.
/// Data can be registered via `do_put` and queried via `do_get` (ticket = SQL).
pub struct KoreFlightService {
    tables: Arc<RwLock<HashMap<String, DataBlock>>>,
}

impl KoreFlightService {
    /// Create a new service. Tables from the provided map are pre-registered.
    pub fn new(initial_tables: HashMap<String, DataBlock>) -> Self {
        Self {
            tables: Arc::new(RwLock::new(initial_tables)),
        }
    }

    /// Create an empty service with no initial tables.
    pub fn empty() -> Self {
        Self::new(HashMap::new())
    }

    /// Register a table directly.
    pub fn register_table(&self, name: impl Into<String>, block: DataBlock) {
        self.tables.write().unwrap().insert(name.into(), block);
    }

    /// Get a snapshot of a table.
    pub fn get_table(&self, name: &str) -> Option<DataBlock> {
        self.tables.read().unwrap().get(name).cloned()
    }

    /// List all registered table names.
    pub fn table_names(&self) -> Vec<String> {
        let guard = self.tables.read().unwrap();
        let mut names: Vec<String> = guard.keys().cloned().collect();
        names.sort();
        names
    }

    fn estimate_bytes(block: &DataBlock) -> usize {
        let mut total = 0;
        for col in &block.columns {
            total += match &col.data {
                ColumnData::Int64(v) => v.len() * 9,
                ColumnData::Float64(v) => v.len() * 9,
                ColumnData::Bool(v) => v.len() * 2,
                ColumnData::Str(v) => v.iter().map(|s| s.as_ref().map_or(1, |s| s.len() + 1)).sum(),
                ColumnData::StrDict { codes, dict } => {
                    codes.len() + dict.iter().map(|s| s.len()).sum::<usize>()
                }
            };
        }
        total
    }
}

impl FlightService for KoreFlightService {
    fn get_flight_info(&self, descriptor: &FlightDescriptor) -> Result<FlightInfo, KoreError> {
        let table_name = descriptor.path.first()
            .ok_or_else(|| KoreError::InvalidArgument("empty flight descriptor path".into()))?;

        let guard = self.tables.read().unwrap();
        let block = guard.get(table_name)
            .ok_or_else(|| KoreError::InvalidArgument(format!("table not found: {table_name}")))?;

        let schema = ArrowSchema {
            fields: block.columns.iter().map(|c| ArrowField {
                name: c.name.clone(),
                data_type: ArrowDataType::from(c.data.dtype()),
            }).collect(),
        };

        Ok(FlightInfo {
            schema,
            total_records: block.num_rows,
            total_bytes: Self::estimate_bytes(block),
        })
    }

    fn do_get(&self, ticket: &[u8]) -> Result<Vec<ArrowBatch>, KoreError> {
        let sql = std::str::from_utf8(ticket)
            .map_err(|e| KoreError::InvalidArgument(format!("ticket is not valid UTF-8: {e}")))?;

        // If the ticket is a plain table name (no spaces/keywords), return the table directly
        let trimmed = sql.trim();
        let is_plain_name = !trimmed.contains(' ') && !trimmed.contains(';');

        if is_plain_name {
            let guard = self.tables.read().unwrap();
            let block = guard.get(trimmed)
                .ok_or_else(|| KoreError::InvalidArgument(format!("table not found: {trimmed}")))?;
            return Ok(vec![datablock_to_arrow(block)]);
        }

        // For SQL queries, we do a simplified table scan (SELECT * WHERE supported via
        // direct table return; full SQL requires kore-sql dependency which is optional).
        // Here we support the pattern: "SELECT * FROM <table>"
        let upper = trimmed.to_uppercase();
        if upper.starts_with("SELECT") {
            if let Some(from_pos) = upper.find("FROM") {
                let after_from = trimmed[from_pos + 4..].trim();
                let table_name = after_from.split_whitespace().next().unwrap_or("");
                let table_name = table_name.trim_end_matches(';');
                let guard = self.tables.read().unwrap();
                let block = guard.get(table_name)
                    .ok_or_else(|| KoreError::InvalidArgument(
                        format!("table not found: {table_name}")
                    ))?;
                return Ok(vec![datablock_to_arrow(block)]);
            }
        }

        Err(KoreError::InvalidArgument(format!(
            "unsupported query in ticket: {trimmed}"
        )))
    }

    fn do_put(&self, descriptor: &FlightDescriptor, batches: Vec<ArrowBatch>) -> Result<(), KoreError> {
        let table_name = descriptor.path.first()
            .ok_or_else(|| KoreError::InvalidArgument("empty flight descriptor path".into()))?;

        // Convert all batches to DataBlocks and concatenate
        let blocks: Vec<DataBlock> = batches.iter()
            .map(|b| arrow_to_datablock(b))
            .collect::<Result<Vec<_>, _>>()?;

        let combined = if blocks.is_empty() {
            DataBlock::empty()
        } else {
            DataBlock::concat(blocks)?
        };

        self.tables.write().unwrap().insert(table_name.clone(), combined);
        Ok(())
    }

    fn list_flights(&self) -> Result<Vec<FlightInfo>, KoreError> {
        let guard = self.tables.read().unwrap();
        let mut infos = Vec::with_capacity(guard.len());
        for (_, block) in guard.iter() {
            let schema = ArrowSchema {
                fields: block.columns.iter().map(|c| ArrowField {
                    name: c.name.clone(),
                    data_type: ArrowDataType::from(c.data.dtype()),
                }).collect(),
            };
            infos.push(FlightInfo {
                schema,
                total_records: block.num_rows,
                total_bytes: Self::estimate_bytes(block),
            });
        }
        Ok(infos)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn sample_block() -> DataBlock {
        DataBlock {
            num_rows: 4,
            columns: vec![
                Column {
                    name: "id".into(),
                    data: ColumnData::Int64(vec![Some(1), Some(2), None, Some(4)]),
                },
                Column {
                    name: "score".into(),
                    data: ColumnData::Float64(vec![Some(3.14), None, Some(2.71), Some(1.0)]),
                },
                Column {
                    name: "name".into(),
                    data: ColumnData::Str(vec![
                        Some("alice".into()),
                        Some("bob".into()),
                        None,
                        Some("dave".into()),
                    ]),
                },
                Column {
                    name: "active".into(),
                    data: ColumnData::Bool(vec![Some(true), Some(false), None, Some(true)]),
                },
            ],
        }
    }

    #[test]
    fn test_datablock_to_arrow_roundtrip() {
        let block = sample_block();
        let batch = datablock_to_arrow(&block);

        assert_eq!(batch.num_rows, 4);
        assert_eq!(batch.schema.fields.len(), 4);
        assert_eq!(batch.schema.fields[0].name, "id");
        assert_eq!(batch.schema.fields[0].data_type, ArrowDataType::Int64);
        assert_eq!(batch.schema.fields[2].data_type, ArrowDataType::Utf8);

        let restored = arrow_to_datablock(&batch).unwrap();
        assert_eq!(restored.num_rows, 4);
        assert_eq!(restored.columns.len(), 4);

        // Check Int64 column with nulls
        if let ColumnData::Int64(v) = &restored.columns[0].data {
            assert_eq!(v, &vec![Some(1), Some(2), None, Some(4)]);
        } else {
            panic!("expected Int64 column");
        }

        // Check Float64 column with nulls
        if let ColumnData::Float64(v) = &restored.columns[1].data {
            assert_eq!(v, &vec![Some(3.14), None, Some(2.71), Some(1.0)]);
        } else {
            panic!("expected Float64 column");
        }

        // Check Str column with nulls
        if let ColumnData::Str(v) = &restored.columns[2].data {
            assert_eq!(v, &vec![
                Some("alice".into()),
                Some("bob".into()),
                None,
                Some("dave".into()),
            ]);
        } else {
            panic!("expected Str column");
        }

        // Check Bool column with nulls
        if let ColumnData::Bool(v) = &restored.columns[3].data {
            assert_eq!(v, &vec![Some(true), Some(false), None, Some(true)]);
        } else {
            panic!("expected Bool column");
        }
    }

    #[test]
    fn test_ipc_serialize_roundtrip() {
        let block = sample_block();
        let batch = datablock_to_arrow(&block);
        let bytes = serialize_ipc(&batch);

        assert!(!bytes.is_empty());
        assert_eq!(&bytes[0..4], b"KIPC");

        let restored = deserialize_ipc(&bytes).unwrap();
        assert_eq!(restored.num_rows, batch.num_rows);
        assert_eq!(restored.schema, batch.schema);

        // Verify data integrity through full round-trip
        let db = arrow_to_datablock(&restored).unwrap();
        assert_eq!(db.num_rows, 4);
        if let ColumnData::Int64(v) = &db.columns[0].data {
            assert_eq!(v, &vec![Some(1), Some(2), None, Some(4)]);
        } else {
            panic!("expected Int64");
        }
    }

    #[test]
    fn test_ipc_empty_batch() {
        let batch = ArrowBatch {
            schema: ArrowSchema {
                fields: vec![ArrowField { name: "x".into(), data_type: ArrowDataType::Int64 }],
            },
            num_rows: 0,
            columns: vec![ArrowColumn::Int64 { validity: vec![], values: vec![] }],
        };
        let bytes = serialize_ipc(&batch);
        let restored = deserialize_ipc(&bytes).unwrap();
        assert_eq!(restored.num_rows, 0);
        assert_eq!(restored.schema.fields.len(), 1);
    }

    #[test]
    fn test_ipc_invalid_magic() {
        let bytes = b"BAAD\x01\x00\x00\x00\x00\x00\x00\x00";
        let result = deserialize_ipc(bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_ipc_truncated() {
        let result = deserialize_ipc(&[0u8; 4]);
        assert!(result.is_err());
    }

    #[test]
    fn test_str_dict_to_arrow() {
        let block = DataBlock {
            num_rows: 3,
            columns: vec![Column {
                name: "city".into(),
                data: ColumnData::StrDict {
                    codes: vec![0, 1, u8::MAX],
                    dict: vec!["NYC".into(), "LA".into()],
                },
            }],
        };
        let batch = datablock_to_arrow(&block);
        if let ArrowColumn::Utf8 { validity, values } = &batch.columns[0] {
            assert_eq!(validity, &vec![true, true, false]);
            assert_eq!(values, &vec!["NYC".to_string(), "LA".to_string(), "".to_string()]);
        } else {
            panic!("expected Utf8 column");
        }

        let restored = arrow_to_datablock(&batch).unwrap();
        if let ColumnData::Str(v) = &restored.columns[0].data {
            assert_eq!(v, &vec![Some("NYC".into()), Some("LA".into()), None]);
        } else {
            panic!("expected Str");
        }
    }

    #[test]
    fn test_ipc_large_batch() {
        let n = 10_000;
        let block = DataBlock {
            num_rows: n,
            columns: vec![
                Column {
                    name: "idx".into(),
                    data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()),
                },
                Column {
                    name: "val".into(),
                    data: ColumnData::Float64((0..n).map(|i| {
                        if i % 7 == 0 { None } else { Some(i as f64 * 0.1) }
                    }).collect()),
                },
            ],
        };
        let batch = datablock_to_arrow(&block);
        let bytes = serialize_ipc(&batch);
        let restored = deserialize_ipc(&bytes).unwrap();
        assert_eq!(restored.num_rows, n);

        let db = arrow_to_datablock(&restored).unwrap();
        if let ColumnData::Float64(v) = &db.columns[1].data {
            assert_eq!(v[0], None);    // 0 % 7 == 0 → null
            assert_eq!(v[1], Some(0.1));
            assert_eq!(v[7], None);
        } else {
            panic!("expected Float64");
        }
    }

    // ── Flight service tests ─────────────────────────────────────────────────

    #[test]
    fn test_flight_service_register_and_list() {
        let svc = KoreFlightService::empty();
        svc.register_table("orders", sample_block());

        let flights = svc.list_flights().unwrap();
        assert_eq!(flights.len(), 1);
        assert_eq!(flights[0].total_records, 4);
    }

    #[test]
    fn test_flight_service_get_flight_info() {
        let svc = KoreFlightService::empty();
        svc.register_table("users", sample_block());

        let desc = FlightDescriptor { path: vec!["users".into()] };
        let info = svc.get_flight_info(&desc).unwrap();
        assert_eq!(info.total_records, 4);
        assert_eq!(info.schema.fields.len(), 4);
        assert_eq!(info.schema.fields[0].name, "id");
    }

    #[test]
    fn test_flight_service_get_flight_info_not_found() {
        let svc = KoreFlightService::empty();
        let desc = FlightDescriptor { path: vec!["nope".into()] };
        assert!(svc.get_flight_info(&desc).is_err());
    }

    #[test]
    fn test_flight_service_do_get_plain_name() {
        let svc = KoreFlightService::empty();
        svc.register_table("sales", sample_block());

        let batches = svc.do_get(b"sales").unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows, 4);
    }

    #[test]
    fn test_flight_service_do_get_select_star() {
        let svc = KoreFlightService::empty();
        svc.register_table("products", sample_block());

        let batches = svc.do_get(b"SELECT * FROM products").unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows, 4);
    }

    #[test]
    fn test_flight_service_do_get_not_found() {
        let svc = KoreFlightService::empty();
        let result = svc.do_get(b"missing_table");
        assert!(result.is_err());
    }

    #[test]
    fn test_flight_service_do_put() {
        let svc = KoreFlightService::empty();
        let batch = datablock_to_arrow(&sample_block());

        let desc = FlightDescriptor { path: vec!["new_table".into()] };
        svc.do_put(&desc, vec![batch]).unwrap();

        let names = svc.table_names();
        assert!(names.contains(&"new_table".to_string()));

        let block = svc.get_table("new_table").unwrap();
        assert_eq!(block.num_rows, 4);
    }

    #[test]
    fn test_flight_service_do_put_multiple_batches() {
        let svc = KoreFlightService::empty();

        let block1 = DataBlock {
            num_rows: 2,
            columns: vec![Column {
                name: "x".into(),
                data: ColumnData::Int64(vec![Some(1), Some(2)]),
            }],
        };
        let block2 = DataBlock {
            num_rows: 3,
            columns: vec![Column {
                name: "x".into(),
                data: ColumnData::Int64(vec![Some(3), Some(4), Some(5)]),
            }],
        };

        let batch1 = datablock_to_arrow(&block1);
        let batch2 = datablock_to_arrow(&block2);

        let desc = FlightDescriptor { path: vec!["combined".into()] };
        svc.do_put(&desc, vec![batch1, batch2]).unwrap();

        let stored = svc.get_table("combined").unwrap();
        assert_eq!(stored.num_rows, 5);
    }

    #[test]
    fn test_flight_service_do_put_empty_descriptor() {
        let svc = KoreFlightService::empty();
        let desc = FlightDescriptor { path: vec![] };
        let result = svc.do_put(&desc, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_flight_service_roundtrip_ipc() {
        let svc = KoreFlightService::empty();
        svc.register_table("data", sample_block());

        // Get data via Flight, serialize to IPC, deserialize, put back
        let batches = svc.do_get(b"data").unwrap();
        let ipc_bytes = serialize_ipc(&batches[0]);
        let restored_batch = deserialize_ipc(&ipc_bytes).unwrap();

        let desc = FlightDescriptor { path: vec!["data_copy".into()] };
        svc.do_put(&desc, vec![restored_batch]).unwrap();

        let copy = svc.get_table("data_copy").unwrap();
        assert_eq!(copy.num_rows, 4);
        if let ColumnData::Int64(v) = &copy.columns[0].data {
            assert_eq!(v, &vec![Some(1), Some(2), None, Some(4)]);
        }
    }

    #[test]
    fn test_arrow_data_type_conversions() {
        assert_eq!(ArrowDataType::from(DataType::Int64), ArrowDataType::Int64);
        assert_eq!(ArrowDataType::from(DataType::Float64), ArrowDataType::Float64);
        assert_eq!(ArrowDataType::from(DataType::Str), ArrowDataType::Utf8);
        assert_eq!(ArrowDataType::from(DataType::Bool), ArrowDataType::Boolean);

        assert_eq!(ArrowDataType::Int64.to_kore_type(), DataType::Int64);
        assert_eq!(ArrowDataType::Float64.to_kore_type(), DataType::Float64);
        assert_eq!(ArrowDataType::Utf8.to_kore_type(), DataType::Str);
        assert_eq!(ArrowDataType::Boolean.to_kore_type(), DataType::Bool);
    }

    #[test]
    fn test_validity_bitmap_roundtrip() {
        let validity = vec![true, false, true, true, false, false, true, false, true];
        let mut buf = Vec::new();
        write_validity_bitmap(&mut buf, &validity);
        let restored = read_validity_bitmap(&buf, validity.len());
        assert_eq!(restored, validity);
    }

    #[test]
    fn test_bool_packed_roundtrip() {
        let values = vec![true, false, false, true, true, true, false, true, false, false, true];
        let mut buf = Vec::new();
        write_bool_packed(&mut buf, &values);
        let restored = read_bool_packed(&buf, values.len());
        assert_eq!(restored, values);
    }
}
