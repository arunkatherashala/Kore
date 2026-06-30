//! KORE Layer 32 — Parquet I/O
//!
//! Read and write Apache Parquet files to/from KORE DataBlocks.
//! Reader uses the Arrow columnar API for fast, allocation-efficient loading.
//! Writer uses the low-level parquet API for fine-grained control.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::schema::parser::parse_message_type;
use parquet::file::writer::SerializedFileWriter;
use parquet::file::properties::WriterProperties;
use parquet::data_type::ByteArray;
use parquet::record::Field;

use arrow_schema::DataType as ArrowType;
use arrow_array::{
    Array, RecordBatch,
    Int8Array, Int16Array, Int32Array, Int64Array,
    UInt8Array, UInt16Array, UInt32Array, UInt64Array,
    Float32Array, Float64Array, BooleanArray,
    StringArray, LargeStringArray,
    Date32Array, Date64Array,
};

use kore_core::{Column, ColumnData, DataBlock, KoreError};

// â”€â”€ Error type â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Debug, thiserror::Error)]
pub enum ParquetError {
    #[error("I/O: {0}")]     Io(#[from] std::io::Error),
    #[error("Parquet: {0}")] Parquet(#[from] parquet::errors::ParquetError),
    #[error("Arrow: {0}")]   Arrow(#[from] arrow_schema::ArrowError),
    #[error("KORE: {0}")]    Kore(#[from] KoreError),
}

// â”€â”€ Reader â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub struct ParquetReader { path: PathBuf }

impl ParquetReader {
    pub fn new(path: impl Into<PathBuf>) -> Self { Self { path: path.into() } }

    pub fn read(&self) -> Result<DataBlock, ParquetError> {
        // Arrow columnar reader: no row-by-row iteration, no String intermediates.
        // Reads entire columns in native Parquet types, converts directly to KORE types.
        let file    = File::open(&self.path)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        let schema  = builder.schema().clone();
        let reader  = builder.with_batch_size(131_072).build()?;
        let batches: Vec<RecordBatch> = reader.collect::<Result<Vec<_>, _>>()?;
        if batches.is_empty() { return Ok(DataBlock::empty()); }
        let total: usize = batches.iter().map(|b| b.num_rows()).sum();
        let mut columns: Vec<Column> = Vec::with_capacity(schema.fields().len());
        for (ci, field) in schema.fields().iter().enumerate() {
            let data = build_kore_column(&batches, ci, field.data_type(), total);
            columns.push(Column { name: field.name().clone(), data });
        }
        Ok(DataBlock { columns, num_rows: total })
    }
}

fn build_kore_column(batches: &[RecordBatch], ci: usize, dtype: &ArrowType, total: usize) -> ColumnData {
    match dtype {
        ArrowType::Int64   => { let mut out: Vec<Option<i64>> = Vec::with_capacity(total); for b in batches { let a = b.column(ci).as_any().downcast_ref::<Int64Array>().unwrap(); for i in 0..a.len() { out.push(if a.is_null(i) { None } else { Some(a.value(i)) }); } } ColumnData::Int64(out) }
        ArrowType::Int32   => { let mut out: Vec<Option<i64>> = Vec::with_capacity(total); for b in batches { let a = b.column(ci).as_any().downcast_ref::<Int32Array>().unwrap(); for i in 0..a.len() { out.push(if a.is_null(i) { None } else { Some(a.value(i) as i64) }); } } ColumnData::Int64(out) }
        ArrowType::Float64 => { let mut out: Vec<Option<f64>> = Vec::with_capacity(total); for b in batches { let a = b.column(ci).as_any().downcast_ref::<Float64Array>().unwrap(); for i in 0..a.len() { out.push(if a.is_null(i) { None } else { Some(a.value(i)) }); } } ColumnData::Float64(out) }
        ArrowType::Float32 => { let mut out: Vec<Option<f64>> = Vec::with_capacity(total); for b in batches { let a = b.column(ci).as_any().downcast_ref::<Float32Array>().unwrap(); for i in 0..a.len() { out.push(if a.is_null(i) { None } else { Some(a.value(i) as f64) }); } } ColumnData::Float64(out) }
        ArrowType::Utf8 | ArrowType::LargeUtf8 => { let mut out: Vec<Option<String>> = Vec::with_capacity(total); for b in batches { let arr = b.column(ci); if let Some(a) = arr.as_any().downcast_ref::<StringArray>() { for i in 0..a.len() { out.push(if a.is_null(i) { None } else { Some(a.value(i).to_string()) }); } } else if let Some(a) = arr.as_any().downcast_ref::<LargeStringArray>() { for i in 0..a.len() { out.push(if a.is_null(i) { None } else { Some(a.value(i).to_string()) }); } } } ColumnData::Str(out) }
        _ => ColumnData::Str(vec![None; total]),
    }
}


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
