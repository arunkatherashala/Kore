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
    m.add_function(wrap_pyfunction!(read_kore, m)?)?;
    m.add_function(wrap_pyfunction!(to_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(from_bytes, m)?)?;
    Ok(())
}
