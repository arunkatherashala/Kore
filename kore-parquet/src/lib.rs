//! KORE Layer 32 â€” Parquet I/O
//!
//! Read and write Apache Parquet files to/from KORE DataBlocks.
//! Uses the official `parquet` crate (v59) from Apache Arrow-rs.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use parquet::schema::parser::parse_message_type;
use parquet::file::writer::SerializedFileWriter;
use parquet::file::properties::WriterProperties;
use parquet::data_type::ByteArray;

use kore_core::{Column, ColumnData, DataBlock, KoreError};

// â”€â”€ Error type â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Debug, thiserror::Error)]
pub enum ParquetError {
    #[error("I/O: {0}")]     Io(#[from] std::io::Error),
    #[error("Parquet: {0}")] Parquet(#[from] parquet::errors::ParquetError),
    #[error("KORE: {0}")]    Kore(#[from] KoreError),
}

// â”€â”€ Reader â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub struct ParquetReader { path: PathBuf }

impl ParquetReader {
    pub fn new(path: impl Into<PathBuf>) -> Self { Self { path: path.into() } }

    pub fn read(&self) -> Result<DataBlock, ParquetError> {
        let file   = File::open(&self.path)?;
        let reader = SerializedFileReader::new(file)?;
        let row_iter = reader.get_row_iter(None)?;
        let mut col_data: HashMap<String, Vec<Option<String>>> = HashMap::new();
        let mut col_order: Vec<String> = Vec::new();
        let mut n = 0usize;

        for row_result in row_iter {
            let row = row_result?;
            for (name, field) in row.get_column_iter() {
                if !col_data.contains_key(name) {
                    col_order.push(name.to_string());
                    col_data.insert(name.to_string(), Vec::new());
                }
                col_data.get_mut(name).unwrap().push(field_to_string(field));
            }
            n += 1;
        }

        if n == 0 { return Ok(DataBlock::empty()); }
        let columns: Vec<Column> = col_order.iter()
            .map(|c| infer_column(c, &col_data[c]))
            .collect();
        Ok(DataBlock { columns, num_rows: n })
    }
}

fn field_to_string(f: &Field) -> Option<String> {
    match f {
        Field::Null      => None,
        Field::Bool(b)   => Some(b.to_string()),
        Field::Byte(i)   => Some(i.to_string()),
        Field::Short(i)  => Some(i.to_string()),
        Field::Int(i)    => Some(i.to_string()),
        Field::Long(i)   => Some(i.to_string()),
        Field::Float(f)  => Some(f.to_string()),
        Field::Double(f) => Some(f.to_string()),
        Field::Str(s)    => Some(s.clone()),
        Field::Bytes(b)  => Some(String::from_utf8_lossy(b.data()).to_string()),
        other            => Some(format!("{other:?}")),
    }
}

fn infer_column(name: &str, raw: &[Option<String>]) -> Column {
    let non_null: Vec<&str> = raw.iter().filter_map(|x| x.as_deref()).collect();
    if non_null.is_empty() {
        return Column { name: name.into(), data: ColumnData::Str(vec![None; raw.len()]) };
    }
    if non_null.iter().all(|s| s.parse::<i64>().is_ok()) {
        return Column { name: name.into(), data: ColumnData::Int64(
            raw.iter().map(|x| x.as_deref().and_then(|s| s.parse().ok())).collect()
        )};
    }
    if non_null.iter().all(|s| s.parse::<f64>().is_ok()) {
        return Column { name: name.into(), data: ColumnData::Float64(
            raw.iter().map(|x| x.as_deref().and_then(|s| s.parse().ok())).collect()
        )};
    }
    Column { name: name.into(), data: ColumnData::Str(raw.iter().cloned().collect()) }
}

// â”€â”€ Writer â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub struct ParquetWriter;

impl ParquetWriter {
    pub fn write_file(block: &DataBlock, path: impl AsRef<Path>) -> Result<(), ParquetError> {
        if block.num_rows == 0 { return Ok(()); }

        let fields: Vec<String> = block.columns.iter().map(|c| {
            let safe = c.name.replace('.', "_").replace(' ', "_");
            match &c.data {
                ColumnData::Int64(_)   => format!("OPTIONAL INT64 {};", safe),
                ColumnData::Float64(_) => format!("OPTIONAL DOUBLE {};", safe),
                ColumnData::Bool(_)    => format!("OPTIONAL BOOLEAN {};", safe),
                ColumnData::Str(_)      => format!("OPTIONAL BYTE_ARRAY {} (UTF8);", safe),
                ColumnData::StrDict { .. } => format!("OPTIONAL BYTE_ARRAY {} (UTF8);", safe),
            }
        }).collect();

        let schema_str = format!("message schema {{\n  {}\n}}", fields.join("\n  "));
        let schema = Arc::new(parse_message_type(&schema_str)?);
        let props  = Arc::new(WriterProperties::builder().build());
        let file   = File::create(path)?;
        let mut fw = SerializedFileWriter::new(file, schema, props)?;
        let mut rg = fw.next_row_group()?;

        for col in &block.columns {
            let Some(mut cw) = rg.next_column()? else { continue };
            use parquet::column::writer::ColumnWriter;
            match cw.untyped() {
                ColumnWriter::Int64ColumnWriter(w) => {
                    if let ColumnData::Int64(v) = &col.data {
                        let vals: Vec<i64> = v.iter().filter_map(|x| *x).collect();
                        let defs: Vec<i16> = v.iter().map(|x| x.is_some() as i16).collect();
                        w.write_batch(&vals, Some(&defs), None)?;
                    }
                }
                ColumnWriter::DoubleColumnWriter(w) => {
                    if let ColumnData::Float64(v) = &col.data {
                        let vals: Vec<f64> = v.iter().filter_map(|x| *x).collect();
                        let defs: Vec<i16> = v.iter().map(|x| x.is_some() as i16).collect();
                        w.write_batch(&vals, Some(&defs), None)?;
                    }
                }
                ColumnWriter::BoolColumnWriter(w) => {
                    if let ColumnData::Bool(v) = &col.data {
                        let vals: Vec<bool> = v.iter().filter_map(|x| *x).collect();
                        let defs: Vec<i16>  = v.iter().map(|x| x.is_some() as i16).collect();
                        w.write_batch(&vals, Some(&defs), None)?;
                    }
                }
                ColumnWriter::ByteArrayColumnWriter(w) => {
                    if let ColumnData::Str(v) = &col.data {
                        let vals: Vec<ByteArray> = v.iter().filter_map(|x| x.as_deref())
                            .map(|s| ByteArray::from(s.as_bytes())).collect();
                        let defs: Vec<i16> = v.iter().map(|x| x.is_some() as i16).collect();
                        w.write_batch(&vals, Some(&defs), None)?;
                    }
                }
                _ => {}
            }
            cw.close()?;
        }
        rg.close()?;
        fw.close()?;
        Ok(())
    }
}

// â”€â”€ Tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::Column;

    #[test]
    fn test_roundtrip() {
        let block = DataBlock::new(vec![
            Column::int64("id",    vec![Some(1), Some(2), Some(3)]),
            Column::float64("val", vec![Some(1.5), Some(2.5), Some(3.5)]),
            Column::str_col("tag", vec![Some("a".into()), Some("b".into()), Some("c".into())]),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_test.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let b2 = ParquetReader::new(&p).read().expect("read");
        std::fs::remove_file(&p).ok();
        assert_eq!(b2.num_rows, 3);
    }
}
