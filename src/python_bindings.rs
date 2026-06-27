//! Python bindings for Kore
//!
//! Exposes Kore columnar file format functionality to Python via PyO3.
//! Uses the real kore_v2 engine — actual file I/O, real compression.
//!
//! Build: `maturin build --release --features pyo3`
//! Install: `pip install .` (after building)

use pyo3::prelude::*;
use std::fs;
use std::io::Read;
use std::collections::HashMap;
use crate::kore_v2::{KoreWriter, KoreReader, KColumn, KType, KVal};
use crate::kore_pulse::FilePulse;
use crate::kore_mind::KoreMind;
use crate::kore_nerve::KoreNerve;
use crate::kore_oracle::KoreOracle;

// ── CSV parsing helpers ───────────────────────────────────────────────────────

/// Detect column types from the first data row.
/// Rules: all parseable as i64 → Int, all parseable as f64 → Float, else Str.
fn infer_ktype(samples: &[&str]) -> KType {
    if samples.iter().all(|s| s.parse::<i64>().is_ok()) {
        return KType::Int;
    }
    if samples.iter().all(|s| s.parse::<f64>().is_ok()) {
        return KType::Float;
    }
    KType::Str
}

/// Parse a CSV string into (headers, rows-as-KVal).
fn parse_csv(content: &str) -> Result<(Vec<String>, Vec<KColumn>, Vec<Vec<KVal>>), String> {
    let mut lines = content.lines().filter(|l| !l.trim().is_empty());

    let header_line = lines.next().ok_or("CSV has no header row")?;
    let headers: Vec<String> = header_line.split(',')
        .map(|h| h.trim().trim_matches('"').to_string())
        .collect();
    let ncols = headers.len();

    // Collect raw string values per row
    let mut raw_rows: Vec<Vec<String>> = Vec::new();
    for line in lines {
        let fields: Vec<String> = line.split(',')
            .map(|f| f.trim().trim_matches('"').to_string())
            .collect();
        // Pad or truncate to match header width
        let mut row = fields;
        row.resize(ncols, String::new());
        raw_rows.push(row);
    }

    if raw_rows.is_empty() {
        return Err("CSV has no data rows".to_string());
    }

    // Infer types from up to 100 sample rows
    let sample_limit = raw_rows.len().min(100);
    let mut columns: Vec<KColumn> = Vec::with_capacity(ncols);
    for ci in 0..ncols {
        let samples: Vec<&str> = raw_rows[..sample_limit]
            .iter()
            .map(|r| r[ci].as_str())
            .collect();
        let ktype = infer_ktype(&samples);
        columns.push(KColumn::new(&headers[ci], ktype));
    }

    // Convert to KVal rows
    let rows: Vec<Vec<KVal>> = raw_rows.iter().map(|raw| {
        raw.iter().enumerate().map(|(ci, s)| {
            if s.is_empty() {
                return KVal::Null;
            }
            match columns[ci].ktype {
                KType::Int   => s.parse::<i64>().map(KVal::Int).unwrap_or(KVal::Null),
                KType::Float => s.parse::<f64>().map(KVal::Float).unwrap_or(KVal::Null),
                _            => KVal::Str(s.clone()),
            }
        }).collect()
    }).collect();

    Ok((headers, columns, rows))
}

// ── Public API functions ──────────────────────────────────────────────────────

/// Compress a CSV file into KORE format.
/// Returns (csv_bytes, kore_bytes, compression_pct).
#[pyfunction]
fn compress_csv(csv_path: String, kore_path: String) -> PyResult<(u64, u64, f64)> {
    // Read CSV
    let mut f = fs::File::open(&csv_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
            format!("Failed to open CSV '{}': {}", csv_path, e)))?;
    let csv_size = f.metadata()
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
            format!("Cannot stat CSV: {}", e)))?.len();
    let mut content = String::new();
    f.read_to_string(&mut content)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
            format!("Failed to read CSV: {}", e)))?;

    // Parse CSV → KVal rows
    let (_headers, columns, rows) = parse_csv(&content)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e))?;

    // Write KORE file using the real kore_v2 engine
    let writer = KoreWriter::new(columns);
    writer.write(&kore_path, &rows)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
            format!("KORE write failed: {}", e)))?;

    // Report real file sizes
    let kore_size = fs::metadata(&kore_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
            format!("Cannot stat KORE output: {}", e)))?.len();

    let compression_pct = if csv_size > 0 {
        ((csv_size.saturating_sub(kore_size)) as f64 / csv_size as f64) * 100.0
    } else {
        0.0
    };

    Ok((csv_size, kore_size, compression_pct))
}

/// Get metadata from a KORE file.
/// Returns (file_bytes, version_string, num_columns).
#[pyfunction]
fn get_kore_info(kore_path: String) -> PyResult<(u64, String, u32)> {
    let reader = KoreReader::open(&kore_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
            format!("Failed to open KORE file '{}': {}", kore_path, e)))?;

    let file_size = fs::metadata(&kore_path)
        .map(|m| m.len()).unwrap_or(0);
    let version = "2.0".to_string();
    let num_columns = reader.ncols as u32;

    Ok((file_size, version, num_columns))
}

// ── Python class wrappers ────────────────────────────────────────────────────

/// Write CSV data to a KORE file.
#[pyclass]
pub struct PyKoreWriter {
    path: String,
}

#[pymethods]
impl PyKoreWriter {
    #[new]
    fn new(path: String) -> Self {
        PyKoreWriter { path }
    }

    /// Write a CSV file to this KORE path.
    /// Returns (csv_bytes, kore_bytes, compression_pct).
    fn write_csv(&self, csv_path: String) -> PyResult<(u64, u64, f64)> {
        compress_csv(csv_path, self.path.clone())
    }

    /// Get info about the KORE file at this path.
    fn get_info(&self) -> PyResult<(u64, String, u32)> {
        get_kore_info(self.path.clone())
    }
}

/// Read and inspect a KORE file.
#[pyclass]
pub struct PyKoreReader {
    path: String,
}

#[pymethods]
impl PyKoreReader {
    #[new]
    fn new(path: String) -> Self {
        PyKoreReader { path }
    }

    /// Returns (file_bytes, version_string).
    fn read_file(&self) -> PyResult<(u64, String)> {
        let reader = KoreReader::open(&self.path)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
                format!("Failed to open KORE file: {}", e)))?;
        let size = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        let _ = reader; // opened successfully
        Ok((size, "2.0".to_string()))
    }

    /// Returns (compression_pct, status_string).
    fn get_compression_stats(&self) -> PyResult<(f64, String)> {
        let (file_size, version) = self.read_file()?;
        Ok((file_size as f64, format!("KORE v{}", version)))
    }

    /// Returns number of rows and columns as (nrows, ncols).
    fn shape(&self) -> PyResult<(usize, usize)> {
        let reader = KoreReader::open(&self.path)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
                format!("Failed to open KORE file: {}", e)))?;
        Ok((reader.nrows, reader.ncols))
    }

    /// Returns column names as a list of strings.
    fn column_names(&self) -> PyResult<Vec<String>> {
        let reader = KoreReader::open(&self.path)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(
                format!("Failed to open KORE file: {}", e)))?;
        Ok(reader.columns.iter().map(|c| c.name.clone()).collect())
    }
}

// ── Module registration ──────────────────────────────────────────────────────

/// KoreOracle Python wrapper — KORE ∞ Layer 4
#[pyclass]
pub struct PyKoreOracle {
    inner: KoreOracle,
}

#[pymethods]
impl PyKoreOracle {
    #[new]
    fn new(path: String) -> Self {
        PyKoreOracle { inner: KoreOracle::new(&path) }
    }

    /// What drives this column? (correlation analysis)
    fn why(&self, col: String) -> PyResult<String> {
        self.inner.why(&col)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))
    }

    /// Predict target column from feature dict {col_name: value}
    fn predict(&self, target: String, features: HashMap<String, f64>) -> PyResult<String> {
        self.inner.predict(&target, features)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))
    }

    /// Conditional stats: what changes when col = val?
    fn what_if(&self, col: String, val: String) -> PyResult<String> {
        self.inner.what_if(&col, &val)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))
    }

    /// Is this column trending up or down?
    fn trend(&self, col: String) -> PyResult<String> {
        self.inner.trend(&col)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))
    }

    /// Full Pearson correlation matrix
    fn correlations(&self) -> PyResult<String> {
        self.inner.correlations()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))
    }
}

/// KoreNerve Python wrapper — KORE ∞ Layer 3
#[pyclass]
pub struct PyKoreNerve {
    inner: KoreNerve,
}

#[pymethods]
impl PyKoreNerve {
    #[new]
    fn new(path: String) -> Self {
        PyKoreNerve { inner: KoreNerve::new(&path) }
    }

    /// Run all nerve agents — returns full health report
    fn scan(&self) -> PyResult<String> {
        let result = self.inner.scan()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))?;
        Ok(result.report())
    }

    /// Run all nerve agents — returns JSON
    fn scan_json(&self) -> PyResult<String> {
        let result = self.inner.scan()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))?;
        Ok(result.to_json())
    }

    /// Detect drift between two .kore snapshots
    #[staticmethod]
    fn scan_drift(path_before: String, path_after: String) -> PyResult<String> {
        KoreNerve::scan_drift(&path_before, &path_after)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))
    }
}

/// KoreMind Python wrapper — KORE ∞ Layer 2
#[pyclass]
pub struct PyKoreMind {
    inner: KoreMind,
}

#[pymethods]
impl PyKoreMind {
    #[new]
    fn new(path: String) -> Self {
        PyKoreMind { inner: KoreMind::new(&path) }
    }

    /// Ask a natural language question about the .kore file
    fn ask(&self, question: String) -> PyResult<String> {
        let result = self.inner.ask(&question)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))?;
        Ok(result.to_table())
    }

    /// Ask a question, get result as JSON string
    fn ask_json(&self, question: String) -> PyResult<String> {
        let result = self.inner.ask(&question)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e))?;
        Ok(result.to_json())
    }

    /// Show supported query patterns
    #[staticmethod]
    fn help() -> String {
        KoreMind::help()
    }
}

/// Python wrapper for FilePulse — KORE ∞ Layer 1
#[pyclass]
pub struct PyFilePulse {
    inner: FilePulse,
}

#[pymethods]
impl PyFilePulse {
    /// Load pulse from a .kore file
    #[staticmethod]
    fn from_kore(path: String) -> PyResult<Self> {
        let inner = FilePulse::from_kore(&path)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e))?;
        Ok(PyFilePulse { inner })
    }

    /// Full stats table — all columns with types, nulls, ranges, quality scores
    fn describe(&self) -> String {
        self.inner.describe()
    }

    /// Health report — issues, grades, recommendations
    fn health(&self) -> String {
        self.inner.health()
    }

    /// Export as JSON string
    fn to_json(&self) -> String {
        self.inner.to_json()
    }

    /// Deterministic fingerprint of the data profile
    #[getter]
    fn fingerprint(&self) -> String {
        self.inner.fingerprint.clone()
    }

    /// Overall quality score 0–100
    #[getter]
    fn quality(&self) -> f64 {
        self.inner.overall_quality
    }

    /// Total row count
    #[getter]
    fn rows(&self) -> u64 {
        self.inner.total_rows
    }

    /// Column count
    #[getter]
    fn cols(&self) -> usize {
        self.inner.total_cols
    }
}

#[pymodule]
fn kore_fileformat(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", "1.2.3")?;
    m.add("__doc__", "KORE Binary Format — real kore_v2 engine, actual file I/O")?;
    m.add("__author__", "Sai Arun Kumar Ktherashala")?;

    m.add_function(wrap_pyfunction!(compress_csv, m)?)?;
    m.add_function(wrap_pyfunction!(get_kore_info, m)?)?;
    m.add_class::<PyKoreWriter>()?;
    m.add_class::<PyKoreReader>()?;
    m.add_class::<PyFilePulse>()?;
    m.add_class::<PyKoreMind>()?;
    m.add_class::<PyKoreNerve>()?;
    m.add_class::<PyKoreOracle>()?;

    Ok(())
}
