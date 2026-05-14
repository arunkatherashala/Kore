//! Python bindings for Kore Cloud Connectors
//! 
//! Exposes S3Reader, AzureBlobReader, and GcsReader to Python via PyO3
//! 
//! Build: `maturin develop` or `maturin build --release`
//! Install: `pip install .` (after building)

use pyo3::prelude::*;
use pyo3::exceptions::PyException;

#[cfg(feature = "s3")]
use crate::s3_reader::{S3Reader, S3Error, S3FileMetadata};

#[cfg(feature = "azure")]
use crate::azure_reader::{AzureBlobReader, AzureError, AzureBlobMetadata};

#[cfg(feature = "gcs")]
use crate::gcs_reader::{GcsReader, GcsError, GcsObjectMetadata};

/// Python wrapper for S3Reader
#[cfg(feature = "s3")]
#[pyclass]
pub struct PyS3Reader {
    inner: S3Reader,
}

#[cfg(feature = "s3")]
#[pymethods]
impl PyS3Reader {
    /// Create new S3Reader
    #[new]
    fn new(region: &str) -> PyResult<Self> {
        let reader = S3Reader::new(region)
            .map_err(|e| PyException::new_err(e.to_string()))?;
        Ok(PyS3Reader { inner: reader })
    }

    /// Enable caching
    fn with_cache(&mut self, cache_dir: &str) -> PyResult<()> {
        self.inner
            .with_cache(cache_dir)
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Read file from S3
    fn read_file(&self, bucket: &str, key: &str) -> PyResult<Vec<u8>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.read_file(bucket, key))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Write file to S3
    fn write_file(&self, bucket: &str, key: &str, data: &[u8]) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.write_file(bucket, key, data))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// List files in S3 bucket
    fn list_files(&self, bucket: &str, prefix: Option<&str>) -> PyResult<Vec<String>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.list_files(bucket, prefix))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Get metadata for S3 object
    fn get_metadata(&self, bucket: &str, key: &str) -> PyResult<(u64, String, String, String)> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        let meta = rt.block_on(self.inner.get_metadata(bucket, key))
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        Ok((meta.size, meta.last_modified, meta.etag, meta.content_type))
    }

    fn __repr__(&self) -> String {
        format!("PyS3Reader(region={})", self.inner.region())
    }
}

/// Python wrapper for AzureBlobReader
#[cfg(feature = "azure")]
#[pyclass]
pub struct PyAzureBlobReader {
    inner: AzureBlobReader,
}

#[cfg(feature = "azure")]
#[pymethods]
impl PyAzureBlobReader {
    /// Create new AzureBlobReader
    #[new]
    fn new(storage_account: &str, account_key: &str) -> PyResult<Self> {
        let reader = AzureBlobReader::new(storage_account, account_key)
            .map_err(|e| PyException::new_err(e.to_string()))?;
        Ok(PyAzureBlobReader { inner: reader })
    }

    /// Enable caching
    fn with_cache(&mut self, cache_dir: &str) -> PyResult<()> {
        self.inner
            .with_cache(cache_dir)
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Read blob from Azure
    fn read_file(&self, container: &str, blob_path: &str) -> PyResult<Vec<u8>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.read_file(container, blob_path))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Write blob to Azure
    fn write_file(&self, container: &str, blob_path: &str, data: &[u8]) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.write_file(container, blob_path, data))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// List blobs in container
    fn list_blobs(&self, container: &str, prefix: Option<&str>) -> PyResult<Vec<String>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.list_blobs(container, prefix))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Get metadata for Azure blob
    fn get_metadata(&self, container: &str, blob_path: &str) -> PyResult<(u64, String, String, Option<String>)> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        let meta = rt.block_on(self.inner.get_metadata(container, blob_path))
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        Ok((meta.size, meta.last_modified, meta.etag, meta.content_type))
    }

    fn __repr__(&self) -> String {
        format!("PyAzureBlobReader(account={})", self.inner.storage_account())
    }
}

/// Python wrapper for GcsReader
#[cfg(feature = "gcs")]
#[pyclass]
pub struct PyGcsReader {
    inner: GcsReader,
}

#[cfg(feature = "gcs")]
#[pymethods]
impl PyGcsReader {
    /// Create new GcsReader
    #[new]
    fn new(project_id: &str) -> PyResult<Self> {
        let reader = GcsReader::new(project_id)
            .map_err(|e| PyException::new_err(e.to_string()))?;
        Ok(PyGcsReader { inner: reader })
    }

    /// Enable caching
    fn with_cache(&mut self, cache_dir: &str) -> PyResult<()> {
        self.inner
            .with_cache(cache_dir)
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Read object from GCS
    fn read_file(&self, bucket: &str, object_path: &str) -> PyResult<Vec<u8>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.read_file(bucket, object_path))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Write object to GCS
    fn write_file(&self, bucket: &str, object_path: &str, data: &[u8]) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.write_file(bucket, object_path, data))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// List objects in GCS bucket
    fn list_objects(&self, bucket: &str, prefix: Option<&str>) -> PyResult<Vec<String>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        rt.block_on(self.inner.list_objects(bucket, prefix))
            .map_err(|e| PyException::new_err(e.to_string()))
    }

    /// Get metadata for GCS object
    fn get_metadata(&self, bucket: &str, object_path: &str) -> PyResult<(u64, String, String, Option<String>)> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        let meta = rt.block_on(self.inner.get_metadata(bucket, object_path))
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        Ok((meta.size, meta.last_modified, meta.generation, meta.content_type))
    }

    fn __repr__(&self) -> String {
        format!("PyGcsReader(project={})", self.inner.project_id())
    }
}

/// Python module for Kore Cloud Connectors
#[pymodule]
fn kore_cloud(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add("__version__", "1.0.0")?;
    m.add("__doc__", "Kore Cloud Connectors - AWS S3, Azure Blob, and Google Cloud Storage")?;

    #[cfg(feature = "s3")]
    m.add_class::<PyS3Reader>()?;

    #[cfg(feature = "azure")]
    m.add_class::<PyAzureBlobReader>()?;

    #[cfg(feature = "gcs")]
    m.add_class::<PyGcsReader>()?;

    Ok(())
}
