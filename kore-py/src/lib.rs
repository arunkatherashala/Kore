use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{KoreWriter, KoreReader};

#[pyclass]
struct PyDataBlock {
    inner: DataBlock,
}

#[pymethods]
impl PyDataBlock {
    #[new]
    fn new() -> Self {
        PyDataBlock { inner: DataBlock::empty() }
    }

    fn add_i64_column(&mut self, name: &str, data: Vec<i64>) {
        let vals: Vec<Option<i64>> = data.into_iter().map(Some).collect();
        self.inner.columns.push(Column { name: name.to_string(), data: ColumnData::Int64(vals) });
        self.inner.num_rows = self.inner.columns[0].data.len();
    }

    fn add_f64_column(&mut self, name: &str, data: Vec<f64>) {
        let vals: Vec<Option<f64>> = data.into_iter().map(Some).collect();
        self.inner.columns.push(Column { name: name.to_string(), data: ColumnData::Float64(vals) });
        self.inner.num_rows = self.inner.columns[0].data.len();
    }

    fn add_str_column(&mut self, name: &str, data: Vec<String>) {
        let vals: Vec<Option<String>> = data.into_iter().map(Some).collect();
        self.inner.columns.push(Column { name: name.to_string(), data: ColumnData::Str(vals) });
        self.inner.num_rows = self.inner.columns[0].data.len();
    }

    fn num_rows(&self) -> usize { self.inner.num_rows }
    fn num_columns(&self) -> usize { self.inner.columns.len() }

    fn column_names(&self) -> Vec<String> {
        self.inner.columns.iter().map(|c| c.name.clone()).collect()
    }

    fn get_f64_column(&self, name: &str) -> Option<Vec<f64>> {
        self.inner.columns.iter().find(|c| c.name == name).and_then(|c| {
            if let ColumnData::Float64(v) = &c.data {
                Some(v.iter().map(|x| x.unwrap_or(f64::NAN)).collect())
            } else { None }
        })
    }

    fn get_i64_column(&self, name: &str) -> Option<Vec<i64>> {
        self.inner.columns.iter().find(|c| c.name == name).and_then(|c| {
            if let ColumnData::Int64(v) = &c.data {
                Some(v.iter().map(|x| x.unwrap_or(0)).collect())
            } else { None }
        })
    }

    fn get_str_column(&self, name: &str) -> Option<Vec<String>> {
        self.inner.columns.iter().find(|c| c.name == name).and_then(|c| {
            match &c.data {
                ColumnData::Str(v) => Some(v.iter().map(|x| x.clone().unwrap_or_default()).collect()),
                ColumnData::StrDict { codes, dict } => {
                    Some(codes.iter().map(|&c| if c == u8::MAX { String::new() } else { dict.get(c as usize).cloned().unwrap_or_default() }).collect())
                }
                _ => None,
            }
        })
    }
}

/// Write a DataBlock to a .kore file (Rust native — full compression).
#[pyfunction]
fn write_kore(path: &str, block: &PyDataBlock) -> PyResult<()> {
    KoreWriter::write_file(std::path::Path::new(path), &block.inner)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
}

/// Read a .kore file into a DataBlock (Rust native — parallel decompression).
#[pyfunction]
fn read_kore(path: &str) -> PyResult<PyDataBlock> {
    let inner = KoreReader::read_file(std::path::Path::new(path))
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(PyDataBlock { inner })
}

/// Write .hkore v3 — full human-readable text + binary fast (Rust speed).
#[pyfunction]
fn write_hkore(path: &str, block: &PyDataBlock, preview_rows: Option<usize>) -> PyResult<()> {
    use std::io::{BufWriter, Write};
    let b = &block.inner;
    let nrows = b.num_rows;
    let ncols = b.columns.len();
    let n_prev = preview_rows.unwrap_or(nrows).min(nrows);

    // Build text section in memory
    let mut text = Vec::with_capacity(nrows * 30);
    let version = if preview_rows.is_none() { "v3.0" } else { "v2.0" };
    writeln!(text, "# KORE Hybrid Format {version}").unwrap();
    writeln!(text, "# Rows: {nrows}  Columns: {ncols}").unwrap();
    writeln!(text, "# Schema:").unwrap();
    for col in &b.columns {
        let dt = match &col.data {
            ColumnData::Float64(_) => "F64",
            ColumnData::Int64(_) => "I64",
            ColumnData::Bool(_) => "Bool",
            ColumnData::Str(_) => "STR",
            ColumnData::StrDict { .. } => "STR",
        };
        writeln!(text, "#   {:<20} {dt}", col.name).unwrap();
    }
    if preview_rows.is_none() {
        writeln!(text, "# Data (ALL {nrows} rows):").unwrap();
    } else {
        writeln!(text, "# Preview (first {n_prev} rows):").unwrap();
    }
    for i in 0..n_prev {
        write!(text, "#   [").unwrap();
        for (j, col) in b.columns.iter().enumerate() {
            if j > 0 { write!(text, " | ").unwrap(); }
            write!(text, "{}=", col.name).unwrap();
            match &col.data {
                ColumnData::Float64(v) => write!(text, "{}", v[i].unwrap_or(f64::NAN)).unwrap(),
                ColumnData::Int64(v) => write!(text, "{}", v[i].unwrap_or(0)).unwrap(),
                ColumnData::Bool(v) => write!(text, "{}", v[i].unwrap_or(false)).unwrap(),
                ColumnData::Str(v) => write!(text, "{}", v[i].as_deref().unwrap_or("NULL")).unwrap(),
                ColumnData::StrDict { codes, dict } => {
                    let c = codes[i];
                    if c == u8::MAX { write!(text, "NULL").unwrap(); }
                    else { write!(text, "{}", dict.get(c as usize).map(|s| s.as_str()).unwrap_or("?")).unwrap(); }
                }
            }
        }
        writeln!(text, "]").unwrap();
    }
    writeln!(text).unwrap();

    // Build binary section
    let bin_data = KoreWriter::to_bytes(b);

    // Compute offset and write file
    let marker = b"\x00KORE_RAW_V2\x00";
    let offset_line_len = 24; // "KORE2 offset=OOOOOOOOOO\n"
    let binary_start = offset_line_len + text.len() + marker.len();

    let file = std::fs::File::create(path)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    let mut w = BufWriter::new(file);
    write!(w, "KORE2 offset={:010}\n", binary_start).unwrap();
    w.write_all(&text).unwrap();
    w.write_all(marker).unwrap();
    w.write_all(&bin_data).unwrap();
    w.flush().unwrap();
    Ok(())
}

/// Write DataBlock to bytes in memory.
#[pyfunction]
fn to_bytes(block: &PyDataBlock) -> Vec<u8> {
    KoreWriter::to_bytes(&block.inner)
}

/// Read DataBlock from bytes.
#[pyfunction]
fn from_bytes(data: &[u8]) -> PyResult<PyDataBlock> {
    let inner = KoreReader::from_bytes(data)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(PyDataBlock { inner })
}

#[pymodule]
fn kore_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDataBlock>()?;
    m.add_function(wrap_pyfunction!(write_kore, m)?)?;
    m.add_function(wrap_pyfunction!(write_hkore, m)?)?;
    m.add_function(wrap_pyfunction!(read_kore, m)?)?;
    m.add_function(wrap_pyfunction!(to_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(from_bytes, m)?)?;
    Ok(())
}
