//! Python bindings for Kore
//! 
//! Exposes Kore columnar file format functionality to Python via PyO3
//! 
//! Build: `maturin build --release --features pyo3`
//! Install: `pip install .` (after building)

use pyo3::prelude::*;

/// Kore Python module
#[pymodule]
fn kore_cloud(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", "1.0.0")?;
    m.add(
        "__doc__",
        "Kore Columnar File Format - Python Bindings\n\nVersion 1.0.0: Core functionality with PyO3 support",
    )?;
    m.add("__author__", "Katherashala Sai Arun Kumar")?;

    Ok(())
}
