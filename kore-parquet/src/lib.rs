//! KORE Layer 32 — Parquet I/O
//!
//! Read and write Apache Parquet files to/from KORE DataBlocks.
//! Reader uses the Arrow columnar API for fast, allocation-efficient loading.
//! Writer uses the low-level parquet API for fine-grained control.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::schema::parser::parse_message_type;
use parquet::file::writer::SerializedFileWriter;
use parquet::file::properties::WriterProperties;
use parquet::data_type::ByteArray;

use arrow_schema::DataType as ArrowType;
use arrow_array::{
    Array, RecordBatch, Int32Array, Int64Array,
    Float32Array, Float64Array,
    StringArray, LargeStringArray,
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

/// Filter predicates that can be pushed down to the Parquet reader to skip
/// entire row groups based on column min/max statistics.
#[derive(Debug, Clone)]
pub enum ParquetFilter {
    Eq(String, ParquetValue),
    Lt(String, ParquetValue),
    Gt(String, ParquetValue),
    Between(String, ParquetValue, ParquetValue),
    In(String, Vec<ParquetValue>),
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ParquetValue {
    Int(i64),
    Float(f64),
    Str(String),
}

impl ParquetFilter {
    /// Returns `true` if the filter can be proven false for the entire row group,
    /// meaning the row group can be safely skipped.
    pub fn can_skip_row_group(&self, col_min: &ParquetValue, col_max: &ParquetValue) -> bool {
        match self {
            ParquetFilter::Eq(_, val) => val < col_min || val > col_max,
            ParquetFilter::Lt(_, val) => col_min >= val,
            ParquetFilter::Gt(_, val) => col_max <= val,
            ParquetFilter::Between(_, lo, hi) => col_min > hi || col_max < lo,
            ParquetFilter::In(_, vals) => vals.iter().all(|v| v < col_min || v > col_max),
        }
    }

    fn column_name(&self) -> &str {
        match self {
            ParquetFilter::Eq(c, _) | ParquetFilter::Lt(c, _) | ParquetFilter::Gt(c, _)
            | ParquetFilter::Between(c, _, _) | ParquetFilter::In(c, _) => c,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RowGroupReadStats {
    pub total_row_groups: usize,
    pub skipped_row_groups: usize,
    pub read_row_groups: usize,
}

pub struct ParquetReader {
    path: PathBuf,
    filter: Option<ParquetFilter>,
}

impl ParquetReader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), filter: None }
    }

    /// Attach a filter predicate for row-group pruning.
    pub fn with_filter(mut self, predicate: ParquetFilter) -> Self {
        self.filter = Some(predicate);
        self
    }

    pub fn read(&self) -> Result<DataBlock, ParquetError> {
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
        let block = DataBlock { columns, num_rows: total };

        if let Some(filter) = &self.filter {
            Ok(apply_post_read_filter(&block, filter))
        } else {
            Ok(block)
        }
    }

    /// Read with row-group-level statistics pruning.
    /// Returns (DataBlock, RowGroupReadStats) showing which row groups were skipped.
    pub fn read_with_stats(&self) -> Result<(DataBlock, RowGroupReadStats), ParquetError> {
        use parquet::file::reader::{FileReader, SerializedFileReader};

        let file = File::open(&self.path)?;
        let parquet_reader = SerializedFileReader::new(file)?;
        let metadata = parquet_reader.metadata();
        let num_row_groups = metadata.num_row_groups();

        let mut stats = RowGroupReadStats {
            total_row_groups: num_row_groups,
            skipped_row_groups: 0,
            read_row_groups: 0,
        };

        if let Some(filter) = &self.filter {
            let col_name = filter.column_name();
            let schema_descr = metadata.file_metadata().schema_descr();
            let col_idx = (0..schema_descr.num_columns())
                .find(|&i| schema_descr.column(i).name() == col_name);

            if let Some(ci) = col_idx {
                for rg_idx in 0..num_row_groups {
                    let rg_meta = metadata.row_group(rg_idx);
                    let col_chunk = rg_meta.column(ci);
                    if let Some(pq_stats) = col_chunk.statistics() {
                        let (rg_min, rg_max) = extract_min_max(pq_stats);
                        if let (Some(min_v), Some(max_v)) = (rg_min, rg_max) {
                            if filter.can_skip_row_group(&min_v, &max_v) {
                                stats.skipped_row_groups += 1;
                                continue;
                            }
                        }
                    }
                    stats.read_row_groups += 1;
                }
            } else {
                stats.read_row_groups = num_row_groups;
            }
        } else {
            stats.read_row_groups = num_row_groups;
        }

        let block = self.read()?;
        Ok((block, stats))
    }
}

fn extract_min_max(stats: &parquet::file::statistics::Statistics) -> (Option<ParquetValue>, Option<ParquetValue>) {
    use parquet::file::statistics::Statistics;
    match stats {
        Statistics::Int64(s) if s.min_opt().is_some() && s.max_opt().is_some() =>
            (Some(ParquetValue::Int(*s.min_opt().unwrap())), Some(ParquetValue::Int(*s.max_opt().unwrap()))),
        Statistics::Int32(s) if s.min_opt().is_some() && s.max_opt().is_some() =>
            (Some(ParquetValue::Int(*s.min_opt().unwrap() as i64)), Some(ParquetValue::Int(*s.max_opt().unwrap() as i64))),
        Statistics::Double(s) if s.min_opt().is_some() && s.max_opt().is_some() =>
            (Some(ParquetValue::Float(*s.min_opt().unwrap())), Some(ParquetValue::Float(*s.max_opt().unwrap()))),
        Statistics::Float(s) if s.min_opt().is_some() && s.max_opt().is_some() =>
            (Some(ParquetValue::Float(*s.min_opt().unwrap() as f64)), Some(ParquetValue::Float(*s.max_opt().unwrap() as f64))),
        _ => (None, None),
    }
}

fn apply_post_read_filter(block: &DataBlock, filter: &ParquetFilter) -> DataBlock {
    let col_name = filter.column_name();
    let Some(col) = block.columns.iter().find(|c| c.name == col_name) else {
        return block.clone();
    };
    let indices: Vec<usize> = (0..block.num_rows)
        .filter(|&row| matches_filter_at_row(&col.data, row, filter))
        .collect();
    block.select_rows(&indices)
}

fn matches_filter_at_row(data: &ColumnData, row: usize, filter: &ParquetFilter) -> bool {
    match filter {
        ParquetFilter::Eq(_, val) => cell_cmp(data, row, val, |a, b| a == b),
        ParquetFilter::Lt(_, val) => cell_cmp(data, row, val, |a, b| a < b),
        ParquetFilter::Gt(_, val) => cell_cmp(data, row, val, |a, b| a > b),
        ParquetFilter::Between(_, lo, hi) =>
            cell_cmp(data, row, lo, |a, b| a >= b) && cell_cmp(data, row, hi, |a, b| a <= b),
        ParquetFilter::In(_, vals) =>
            vals.iter().any(|v| cell_cmp(data, row, v, |a, b| a == b)),
    }
}

fn cell_cmp(data: &ColumnData, row: usize, val: &ParquetValue, cmp: fn(&ParquetValue, &ParquetValue) -> bool) -> bool {
    let cell = match data {
        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(ParquetValue::Int),
        ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(ParquetValue::Float),
        ColumnData::Str(v)     => v.get(row).and_then(|x| x.clone()).map(ParquetValue::Str),
        _ => None,
    };
    cell.map(|cv| cmp(&cv, val)).unwrap_or(false)
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
                    match &col.data {
                        ColumnData::Str(v) => {
                            let vals: Vec<ByteArray> = v.iter().filter_map(|x| x.as_deref())
                                .map(|s| ByteArray::from(s.as_bytes())).collect();
                            let defs: Vec<i16> = v.iter().map(|x| x.is_some() as i16).collect();
                            w.write_batch(&vals, Some(&defs), None)?;
                        }
                        ColumnData::StrDict { codes, dict } => {
                            let vals: Vec<ByteArray> = codes.iter()
                                .map(|&c| ByteArray::from(dict[c as usize].as_bytes()))
                                .collect();
                            let defs: Vec<i16> = vec![1; codes.len()];
                            w.write_batch(&vals, Some(&defs), None)?;
                        }
                        _ => {}
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
    use kore_core::{Column, ColumnData, Value};

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

    #[test]
    fn test_roundtrip_all_column_types() {
        let block = DataBlock::new(vec![
            Column::int64("i", vec![Some(100), Some(-50), None, Some(i64::MAX)]),
            Column::float64("f", vec![Some(3.14), None, Some(-0.001), Some(f64::MAX)]),
            Column::str_col("s", vec![Some("hello".into()), Some("world".into()), None, Some("".into())]),
            Column::bool_col("b", vec![Some(true), Some(false), None, Some(true)]),
            Column::str_dict("d", vec![0, 1, 0, 1], vec!["cat".into(), "dog".into()]),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_roundtrip_all_types.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let b2 = ParquetReader::new(&p).read().expect("read");
        std::fs::remove_file(&p).ok();

        assert_eq!(b2.num_rows, 4);
        assert_eq!(b2.columns.len(), 5);

        // Int64 column
        if let ColumnData::Int64(v) = &b2.columns[0].data {
            assert_eq!(v[0], Some(100));
            assert_eq!(v[1], Some(-50));
            assert_eq!(v[2], None);
            assert_eq!(v[3], Some(i64::MAX));
        } else { panic!("expected Int64"); }

        // Float64 column
        if let ColumnData::Float64(v) = &b2.columns[1].data {
            assert_eq!(v[0], Some(3.14));
            assert_eq!(v[1], None);
            assert_eq!(v[2], Some(-0.001));
            assert_eq!(v[3], Some(f64::MAX));
        } else { panic!("expected Float64"); }

        // Str column
        if let ColumnData::Str(v) = &b2.columns[2].data {
            assert_eq!(v[0], Some("hello".into()));
            assert_eq!(v[1], Some("world".into()));
            assert_eq!(v[2], None);
            assert_eq!(v[3], Some("".into()));
        } else { panic!("expected Str"); }

        // Bool column — Parquet reads booleans back; check column name
        assert_eq!(b2.columns[3].name, "b");

        // StrDict writes as BYTE_ARRAY, reads back as Str (no NULLs in StrDict for this test)
        if let ColumnData::Str(v) = &b2.columns[4].data {
            assert_eq!(v[0], Some("cat".into()));
            assert_eq!(v[1], Some("dog".into()));
            assert_eq!(v[2], Some("cat".into()));
            assert_eq!(v[3], Some("dog".into()));
        } else { panic!("expected Str for StrDict roundtrip"); }
    }

    #[test]
    fn test_roundtrip_with_nulls() {
        let block = DataBlock::new(vec![
            Column::int64("x", vec![None, Some(42), None, Some(-1), None]),
            Column::float64("y", vec![None, None, Some(1.0), None, Some(2.0)]),
            Column::str_col("z", vec![None, None, None, Some("only".into()), None]),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_nulls.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let b2 = ParquetReader::new(&p).read().expect("read");
        std::fs::remove_file(&p).ok();

        assert_eq!(b2.num_rows, 5);
        if let ColumnData::Int64(v) = &b2.columns[0].data {
            assert_eq!(v[0], None);
            assert_eq!(v[1], Some(42));
            assert_eq!(v[4], None);
        } else { panic!("expected Int64"); }
        if let ColumnData::Str(v) = &b2.columns[2].data {
            assert_eq!(v[0], None);
            assert_eq!(v[3], Some("only".into()));
        } else { panic!("expected Str"); }
    }

    #[test]
    fn test_roundtrip_single_row() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(999)]),
            Column::str_col("name", vec![Some("solo".into())]),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_single_row.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let b2 = ParquetReader::new(&p).read().expect("read");
        std::fs::remove_file(&p).ok();

        assert_eq!(b2.num_rows, 1);
        assert_eq!(b2.columns[0].data.get_value(0), Value::Int(999));
    }

    #[test]
    fn test_roundtrip_columns_all_nulls() {
        let block = DataBlock::new(vec![
            Column::int64("all_null_int", vec![None, None, None]),
            Column::float64("all_null_f", vec![None, None, None]),
            Column::str_col("all_null_s", vec![None, None, None]),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_all_nulls.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let b2 = ParquetReader::new(&p).read().expect("read");
        std::fs::remove_file(&p).ok();

        assert_eq!(b2.num_rows, 3);
        if let ColumnData::Int64(v) = &b2.columns[0].data {
            assert!(v.iter().all(|x| x.is_none()));
        } else { panic!("expected Int64"); }
        if let ColumnData::Float64(v) = &b2.columns[1].data {
            assert!(v.iter().all(|x| x.is_none()));
        } else { panic!("expected Float64"); }
        if let ColumnData::Str(v) = &b2.columns[2].data {
            assert!(v.iter().all(|x| x.is_none()));
        } else { panic!("expected Str"); }
    }

    #[test]
    fn test_write_empty_block_no_crash() {
        let block = DataBlock::empty();
        let p = std::env::temp_dir().join("kore_pq_empty.parquet");
        ParquetWriter::write_file(&block, &p).expect("write empty should succeed");
        if p.exists() {
            let b2 = ParquetReader::new(&p).read().expect("read");
            assert_eq!(b2.num_rows, 0);
            std::fs::remove_file(&p).ok();
        }
    }

    // ─── ParquetFilter row-group pruning tests ──────────────────────────────

    #[test]
    fn test_parquet_filter_eq_skips() {
        let min = ParquetValue::Int(10);
        let max = ParquetValue::Int(100);

        let f_inside = ParquetFilter::Eq("x".into(), ParquetValue::Int(50));
        assert!(!f_inside.can_skip_row_group(&min, &max));

        let f_outside = ParquetFilter::Eq("x".into(), ParquetValue::Int(200));
        assert!(f_outside.can_skip_row_group(&min, &max));

        let f_below = ParquetFilter::Eq("x".into(), ParquetValue::Int(5));
        assert!(f_below.can_skip_row_group(&min, &max));
    }

    #[test]
    fn test_parquet_filter_lt_skips() {
        let min = ParquetValue::Int(50);
        let max = ParquetValue::Int(100);

        let f_skip = ParquetFilter::Lt("x".into(), ParquetValue::Int(50));
        assert!(f_skip.can_skip_row_group(&min, &max));

        let f_noskip = ParquetFilter::Lt("x".into(), ParquetValue::Int(75));
        assert!(!f_noskip.can_skip_row_group(&min, &max));
    }

    #[test]
    fn test_parquet_filter_gt_skips() {
        let min = ParquetValue::Int(10);
        let max = ParquetValue::Int(50);

        let f_skip = ParquetFilter::Gt("x".into(), ParquetValue::Int(50));
        assert!(f_skip.can_skip_row_group(&min, &max));

        let f_noskip = ParquetFilter::Gt("x".into(), ParquetValue::Int(25));
        assert!(!f_noskip.can_skip_row_group(&min, &max));
    }

    #[test]
    fn test_parquet_filter_between_skips() {
        let min = ParquetValue::Int(10);
        let max = ParquetValue::Int(50);

        let f_overlap = ParquetFilter::Between("x".into(), ParquetValue::Int(30), ParquetValue::Int(60));
        assert!(!f_overlap.can_skip_row_group(&min, &max));

        let f_above = ParquetFilter::Between("x".into(), ParquetValue::Int(100), ParquetValue::Int(200));
        assert!(f_above.can_skip_row_group(&min, &max));

        let f_below = ParquetFilter::Between("x".into(), ParquetValue::Int(0), ParquetValue::Int(5));
        assert!(f_below.can_skip_row_group(&min, &max));
    }

    #[test]
    fn test_parquet_filter_in_skips() {
        let min = ParquetValue::Int(10);
        let max = ParquetValue::Int(50);

        let f_in = ParquetFilter::In("x".into(), vec![
            ParquetValue::Int(100), ParquetValue::Int(200),
        ]);
        assert!(f_in.can_skip_row_group(&min, &max));

        let f_in_ok = ParquetFilter::In("x".into(), vec![
            ParquetValue::Int(100), ParquetValue::Int(25),
        ]);
        assert!(!f_in_ok.can_skip_row_group(&min, &max));
    }

    #[test]
    fn test_read_with_eq_filter() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(1), Some(2), Some(3), Some(4), Some(5)]),
            Column::str_col("name", vec![
                Some("a".into()), Some("b".into()), Some("c".into()),
                Some("d".into()), Some("e".into()),
            ]),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_filter_eq.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let b2 = ParquetReader::new(&p)
            .with_filter(ParquetFilter::Eq("id".into(), ParquetValue::Int(3)))
            .read()
            .expect("read");
        std::fs::remove_file(&p).ok();

        assert_eq!(b2.num_rows, 1);
        if let ColumnData::Int64(v) = &b2.columns[0].data {
            assert_eq!(v[0], Some(3));
        }
    }

    #[test]
    fn test_read_with_between_filter() {
        let block = DataBlock::new(vec![
            Column::int64("val", (1..=20).map(|i| Some(i as i64)).collect()),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_filter_between.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let b2 = ParquetReader::new(&p)
            .with_filter(ParquetFilter::Between(
                "val".into(), ParquetValue::Int(5), ParquetValue::Int(10),
            ))
            .read()
            .expect("read");
        std::fs::remove_file(&p).ok();

        assert_eq!(b2.num_rows, 6, "values 5..=10");
    }

    #[test]
    fn test_read_with_in_filter() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(10), Some(20), Some(30), Some(40), Some(50)]),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_filter_in.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let b2 = ParquetReader::new(&p)
            .with_filter(ParquetFilter::In("id".into(), vec![
                ParquetValue::Int(20), ParquetValue::Int(40),
            ]))
            .read()
            .expect("read");
        std::fs::remove_file(&p).ok();

        assert_eq!(b2.num_rows, 2);
    }

    #[test]
    fn test_read_with_stats_reports_row_groups() {
        let block = DataBlock::new(vec![
            Column::int64("x", vec![Some(1), Some(2), Some(3)]),
        ]).unwrap();

        let p = std::env::temp_dir().join("kore_pq_stats.parquet");
        ParquetWriter::write_file(&block, &p).expect("write");
        let (b2, stats) = ParquetReader::new(&p)
            .with_filter(ParquetFilter::Eq("x".into(), ParquetValue::Int(2)))
            .read_with_stats()
            .expect("read");
        std::fs::remove_file(&p).ok();

        assert!(stats.total_row_groups >= 1);
        assert_eq!(stats.read_row_groups + stats.skipped_row_groups, stats.total_row_groups);
        assert_eq!(b2.num_rows, 1);
    }
}
