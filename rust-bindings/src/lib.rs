//! Kore Python Bindings (PyO3)
//!
//! Phase 2: PyO3 Native Bindings for Kore
//! Direct Python-Rust FFI for 2-5x performance improvement.
//!
//! Status: In Progress
//! Timeline: 2-3 weeks

use pyo3::prelude::*;
use pyo3::exceptions::PyIOError;
use std::collections::HashMap;
use rayon::prelude::*;

// Re-export Kore core types
use kore_fileformat::kore_v2::KoreReader;

/// Read Kore file - native Rust implementation
/// Returns data as Vec<Vec<String>> (columns)
#[pyfunction]
fn kore_read_native(path: &str) -> PyResult<Vec<Vec<String>>> {
    let kore_reader = KoreReader::open(path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read Kore file: {}", e)))?;
    
    // Read all data
    let data = kore_reader.read_all_columns();
    
    // Convert KVal to String
    let result: Vec<Vec<String>> = data.iter()
        .map(|column| {
            column.iter()
                .map(|kval| kval.display())
                .collect()
        })
        .collect();
    
    Ok(result)
}

/// Write Kore file - native Rust implementation
/// Takes schema and flat data, writes to .kore format
#[pyfunction]
fn kore_write_native(_path: &str, _schema: String, _data: Vec<Vec<String>>) -> PyResult<()> {
    // TODO: Implement write once KoreWriter API is fully specified
    // For now, this is a placeholder that returns success
    Err(PyIOError::new_err("Write not yet fully implemented"))
}

/// Read specific column - optimized Rust implementation
/// Returns single column as Vec<String>
#[pyfunction]
fn kore_read_column_native(path: &str, column: &str) -> PyResult<Vec<String>> {
    let kore_reader = KoreReader::open(path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read Kore file: {}", e)))?;
    
    // Find column index
    let col_idx = kore_reader.columns.iter()
        .position(|c| c.name == column)
        .ok_or_else(|| PyIOError::new_err(format!("Column '{}' not found", column)))?;
    
    // Read all columns
    let data = kore_reader.read_all_columns();
    
    let column_data: Vec<String> = data.get(col_idx)
        .ok_or_else(|| PyIOError::new_err("Column index out of bounds"))?
        .iter()
        .map(|kval| kval.display())
        .collect();
    
    Ok(column_data)
}

/// Get file statistics - metadata without full read
/// Returns HashMap with statistics
#[pyfunction]
fn kore_stats_native(path: &str) -> PyResult<HashMap<String, String>> {
    let kore_reader = KoreReader::open(path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read Kore file: {}", e)))?;
    
    let mut stats = HashMap::new();
    
    // Get metadata without reading all data
    let ncols = kore_reader.ncols;
    let nrows = kore_reader.nrows;
    
    stats.insert("column_count".to_string(), ncols.to_string());
    stats.insert("row_count".to_string(), nrows.to_string());
    stats.insert("chunk_count".to_string(), kore_reader.nchunks.to_string());
    
    // List columns with types
    for (idx, col) in kore_reader.columns.iter().enumerate() {
        stats.insert(
            format!("column_{}_name", idx),
            col.name.clone()
        );
        stats.insert(
            format!("column_{}_type", idx),
            format!("{:?}", col.ktype)
        );
    }
    
    // File size
    let metadata = std::fs::metadata(path)
        .map_err(|e| PyIOError::new_err(format!("Failed to get metadata: {}", e)))?;
    stats.insert("file_size_bytes".to_string(), metadata.len().to_string());
    
    // Estimated compression ratio
    let estimated_uncompressed = nrows * ncols * 50; // Rough estimate: 50 bytes per cell
    if estimated_uncompressed > 0 {
        let ratio = (metadata.len() as f64) / (estimated_uncompressed as f64);
        stats.insert("compression_ratio".to_string(), format!("{:.2}", ratio));
    }
    
    Ok(stats)
}

/// Batch process multiple files in parallel using Rayon
/// Operation: "read" or "stats"
#[pyfunction]
fn kore_process_batch(paths: Vec<String>, operation: &str) -> PyResult<usize> {
    let results: Vec<Result<bool, String>> = paths.par_iter()
        .map(|path| {
            match operation {
                "read" => {
                    KoreReader::open(path)
                        .map(|_| true)
                        .map_err(|e| e.to_string())
                }
                "stats" => {
                    KoreReader::open(path)
                        .map(|kr| {
                            let _ = kr.nrows;
                            true
                        })
                        .map_err(|e| e.to_string())
                }
                _ => Err(format!("Unknown operation: {}", operation))
            }
        })
        .collect();
    
    // Count successful operations
    let success_count = results.iter()
        .filter(|r| r.is_ok())
        .count();
    
    // Log errors
    for (idx, result) in results.iter().enumerate() {
        if let Err(e) = result {
            eprintln!("Error processing {}: {}", paths.get(idx).unwrap_or(&"unknown".to_string()), e);
        }
    }
    
    Ok(success_count)
}

#[pymodule]
fn kore_native(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(kore_read_native, m)?)?;
    m.add_function(wrap_pyfunction!(kore_write_native, m)?)?;
    m.add_function(wrap_pyfunction!(kore_read_column_native, m)?)?;
    m.add_function(wrap_pyfunction!(kore_stats_native, m)?)?;
    m.add_function(wrap_pyfunction!(kore_process_batch, m)?)?;
    Ok(())
}
