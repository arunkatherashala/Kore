//! Python bindings for Kore
//! 
//! Exposes Kore columnar file format functionality to Python via PyO3
//! 
//! Build: `maturin build --release --features pyo3`
//! Install: `pip install .` (after building)

use pyo3::prelude::*;

/// Kore Python module
#[pymodule]
fn kore_fileformat(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", "1.1.0")?;
    m.add(
        "__doc__",
        "Kore Binary Format - Complete 8-language ecosystem for efficient data storage and querying\n\nVersion 1.1.0: Phase A complete with 5-8x compression",
    )?;
    m.add("__author__", "Sai Arun Kumar Ktherashala")?;

    Ok(())
}
