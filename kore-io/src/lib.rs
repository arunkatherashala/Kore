//! KORE Layer 28 — File I/O
//!
//! Read/write DataBlock from/to CSV, NDJSON, and the native .kore format.
//!
//! # Examples
//! ```no_run
//! use kore_io::{CsvReader, CsvWriter, NdJsonReader};
//!
//! let block = CsvReader::new("sales.csv").read().unwrap();
//! CsvWriter::write_file(&block, "output.csv").unwrap();
//!
//! let block2 = NdJsonReader::new("events.ndjson").read().unwrap();
//! ```

use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::fs::File;
use std::path::{Path, PathBuf};
use kore_core::{Column, ColumnData, DataBlock, KoreError};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("KORE engine error: {0}")]
    Kore(#[from] KoreError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

// ── Type inference ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum InferredType { Int64, Float64, Bool, Str }

fn infer_type(samples: &[&str]) -> InferredType {
    let non_empty: Vec<&str> = samples.iter().copied().filter(|s| !s.is_empty()).collect();
    if non_empty.is_empty() { return InferredType::Str; }

    if non_empty.iter().all(|s| s.parse::<i64>().is_ok()) { return InferredType::Int64; }
    if non_empty.iter().all(|s| s.parse::<f64>().is_ok()) { return InferredType::Float64; }

    let bool_vals = ["true","false","1","0","yes","no","t","f"];
    if non_empty.iter().all(|s| bool_vals.contains(&s.to_lowercase().as_str())) {
        return InferredType::Bool;
    }
    InferredType::Str
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_lowercase().as_str() {
        "true"|"1"|"yes"|"t" => Some(true),
        "false"|"0"|"no"|"f" => Some(false),
        _ => None,
    }
}

// ── CSV Reader ────────────────────────────────────────────────────────────────

pub struct CsvReader {
    path:        PathBuf,
    delimiter:   u8,
    has_header:  bool,
    sample_rows: usize,   // rows to sample for type inference
}

impl CsvReader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), delimiter: b',', has_header: true, sample_rows: 1000 }
    }

    pub fn delimiter(mut self, d: u8) -> Self { self.delimiter = d; self }
    pub fn no_header(mut self) -> Self { self.has_header = false; self }

    pub fn read(&self) -> Result<DataBlock, IoError> {
        let file   = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut lines: Vec<String> = reader.lines().collect::<io::Result<_>>()?;

        if lines.is_empty() {
            return Ok(DataBlock::empty());
        }

        let delimiter = self.delimiter as char;

        // Headers
        let headers: Vec<String> = if self.has_header {
            let h = lines.remove(0);
            h.split(delimiter).map(|s| s.trim().to_string()).collect()
        } else {
            let first_count = lines[0].split(delimiter).count();
            (0..first_count).map(|i| format!("col{i}")).collect()
        };

        let n_cols = headers.len();
        let n_rows = lines.len();

        // Parse all rows
        let mut raw: Vec<Vec<String>> = lines.iter()
            .map(|line| {
                let mut fields: Vec<String> = line.split(delimiter)
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .collect();
                fields.resize(n_cols, String::new());
                fields
            })
            .collect();

        // Infer types from sample
        let sample_n = self.sample_rows.min(n_rows);
        let mut columns: Vec<Column> = Vec::with_capacity(n_cols);

        for (ci, name) in headers.iter().enumerate() {
            let samples: Vec<&str> = raw[..sample_n].iter()
                .map(|row| row[ci].as_str())
                .collect();
            let typ = infer_type(&samples);

            let data = match typ {
                InferredType::Int64 => ColumnData::Int64(
                    raw.iter().map(|row| row[ci].parse::<i64>().ok()).collect()
                ),
                InferredType::Float64 => ColumnData::Float64(
                    raw.iter().map(|row| row[ci].parse::<f64>().ok()).collect()
                ),
                InferredType::Bool => ColumnData::Bool(
                    raw.iter().map(|row| parse_bool(&row[ci])).collect()
                ),
                InferredType::Str => ColumnData::Str(
                    raw.iter().map(|row| {
                        if row[ci].is_empty() { None } else { Some(row[ci].clone()) }
                    }).collect()
                ),
            };
            columns.push(Column { name: name.clone(), data });
        }

        let _ = raw; // drop early
        Ok(DataBlock { columns, num_rows: n_rows })
    }
}

// ── CSV Writer ────────────────────────────────────────────────────────────────

pub struct CsvWriter;

impl CsvWriter {
    pub fn write_file(block: &DataBlock, path: impl AsRef<Path>) -> io::Result<()> {
        let file   = File::create(path)?;
        let mut w  = BufWriter::new(file);
        w.write_all(Self::to_string(block).as_bytes())
    }

    pub fn to_string(block: &DataBlock) -> String {
        let mut out = String::new();
        // Header
        let headers: Vec<&str> = block.columns.iter().map(|c| c.name.as_str()).collect();
        out.push_str(&headers.join(","));
        out.push('\n');
        // Rows
        for i in 0..block.num_rows {
            let row: Vec<String> = block.columns.iter().map(|c| {
                match &c.data {
                    ColumnData::Int64(v)   => v.get(i).and_then(|x| *x).map(|n| n.to_string()).unwrap_or_default(),
                    ColumnData::Float64(v) => v.get(i).and_then(|x| *x).map(|f| format!("{f}")).unwrap_or_default(),
                    ColumnData::Bool(v)    => v.get(i).and_then(|x| *x).map(|b| b.to_string()).unwrap_or_default(),
                    ColumnData::Str(v)     => v.get(i).and_then(|x| x.as_deref())
                        .map(|s| if s.contains(',') { format!("\"{s}\"") } else { s.to_string() })
                        .unwrap_or_default(),
                    ColumnData::StrDict { codes, dict } => {
                        let c = codes.get(i).copied().unwrap_or(u8::MAX);
                        if c == u8::MAX { String::new() } else {
                            let s = dict.get(c as usize).map(|s| s.as_str()).unwrap_or("");
                            if s.contains(',') { format!("\"{}\"", s) } else { s.to_string() }
                        }
                    }
                }
            }).collect();
            out.push_str(&row.join(","));
            out.push('\n');
        }
        out
    }
}

// ── NDJSON Reader ─────────────────────────────────────────────────────────────

pub struct NdJsonReader {
    path: PathBuf,
}

impl NdJsonReader {
    pub fn new(path: impl Into<PathBuf>) -> Self { Self { path: path.into() } }

    pub fn read(&self) -> Result<DataBlock, IoError> {
        let file   = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let records: Vec<serde_json::Value> = reader.lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l))
            .collect::<serde_json::Result<_>>()?;

        if records.is_empty() { return Ok(DataBlock::empty()); }

        // Collect all keys
        let keys: Vec<String> = {
            let mut seen = indexmap_like(&records);
            seen
        };

        let n = records.len();
        let mut columns: Vec<Column> = Vec::new();

        for key in &keys {
            let vals: Vec<&serde_json::Value> = records.iter()
                .map(|r| r.get(key).unwrap_or(&serde_json::Value::Null))
                .collect();

            // Infer type from first non-null
            let first_non_null = vals.iter().find(|v| !v.is_null());

            let data = match first_non_null {
                Some(serde_json::Value::Bool(_)) => ColumnData::Bool(
                    vals.iter().map(|v| v.as_bool()).collect()
                ),
                Some(serde_json::Value::Number(n)) if n.is_i64() => ColumnData::Int64(
                    vals.iter().map(|v| v.as_i64()).collect()
                ),
                Some(serde_json::Value::Number(_)) => ColumnData::Float64(
                    vals.iter().map(|v| v.as_f64()).collect()
                ),
                _ => ColumnData::Str(
                    vals.iter().map(|v| v.as_str().map(|s| s.to_string())).collect()
                ),
            };
            columns.push(Column { name: key.clone(), data });
        }

        Ok(DataBlock { columns, num_rows: n })
    }
}

fn indexmap_like(records: &[serde_json::Value]) -> Vec<String> {
    let mut seen = std::collections::LinkedList::new();
    let mut set  = std::collections::HashSet::new();
    for rec in records {
        if let Some(obj) = rec.as_object() {
            for k in obj.keys() {
                if set.insert(k.clone()) {
                    seen.push_back(k.clone());
                }
            }
        }
    }
    seen.into_iter().collect()
}

// ── NDJSON Writer ─────────────────────────────────────────────────────────────

pub struct NdJsonWriter;

impl NdJsonWriter {
    pub fn write_file(block: &DataBlock, path: impl AsRef<Path>) -> io::Result<()> {
        let file = File::create(path)?;
        let mut w = BufWriter::new(file);
        for line in Self::to_lines(block) {
            w.write_all(line.as_bytes())?;
            w.write_all(b"\n")?;
        }
        Ok(())
    }

    pub fn to_lines(block: &DataBlock) -> Vec<String> {
        (0..block.num_rows).map(|i| {
            let obj: serde_json::Map<String, serde_json::Value> = block.columns.iter()
                .map(|c| {
                    let val = match &c.data {
                        ColumnData::Int64(v)   => v.get(i).and_then(|x| *x).map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
                        ColumnData::Float64(v) => v.get(i).and_then(|x| *x).map(|f| serde_json::json!(f)).unwrap_or(serde_json::Value::Null),
                        ColumnData::Bool(v)    => v.get(i).and_then(|x| *x).map(serde_json::Value::Bool).unwrap_or(serde_json::Value::Null),
                        ColumnData::Str(v)     => v.get(i).and_then(|x| x.as_deref()).map(|s| serde_json::Value::String(s.to_string())).unwrap_or(serde_json::Value::Null),
                        ColumnData::StrDict { codes, dict } => {
                            let c = codes.get(i).copied().unwrap_or(u8::MAX);
                            if c == u8::MAX { serde_json::Value::Null } else { dict.get(c as usize).map(|s| serde_json::Value::String(s.clone())).unwrap_or(serde_json::Value::Null) }
                        }
                    };
                    (c.name.clone(), val)
                })
                .collect();
            serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or_default()
        }).collect()
    }
}

// ── Re-export kore-store read/write for convenience ───────────────────────────
pub use kore_store::{KoreReader, KoreWriter};

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_block() -> DataBlock {
        DataBlock::new(vec![
            Column::int64("id",   vec![Some(1), Some(2), Some(3)]),
            Column::float64("val", vec![Some(1.5), Some(2.5), Some(3.5)]),
            Column::str_col("name", vec![Some("Alice".into()), Some("Bob".into()), Some("Carol".into())]),
        ]).unwrap()
    }

    #[test]
    fn test_csv_roundtrip() {
        use std::io::Write;
        let block = simple_block();
        let csv = CsvWriter::to_string(&block);
        assert!(csv.contains("id,val,name"));
        assert!(csv.contains("Alice"));
        // Write to temp file and read back
        let dir  = std::env::temp_dir();
        let path = dir.join("kore_io_test.csv");
        std::fs::write(&path, &csv).unwrap();
        let block2 = CsvReader::new(&path).read().unwrap();
        assert_eq!(block2.num_rows, 3);
        assert_eq!(block2.columns.len(), 3);
    }

    #[test]
    fn test_ndjson_roundtrip() {
        let block = simple_block();
        let lines = NdJsonWriter::to_lines(&block);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("Alice"));

        let dir  = std::env::temp_dir();
        let path = dir.join("kore_io_test.ndjson");
        std::fs::write(&path, lines.join("\n")).unwrap();
        let block2 = NdJsonReader::new(&path).read().unwrap();
        assert_eq!(block2.num_rows, 3);
    }
}
