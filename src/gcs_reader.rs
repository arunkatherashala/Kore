//! Google Cloud Storage Connector for Kore FileFormat
//! 
//! Provides native support for reading and writing Kore files directly from/to Google Cloud Storage.
//! 
//! # Features
//! 
//! To use this module, enable the `gcs` feature in Cargo.toml:
//! ```toml
//! [dependencies]
//! kore_fileformat = { version = "1.0", features = ["gcs"] }
//! ```
//! 
//! # Examples
//! 
//! ```ignore
//! use kore_fileformat::gcs_reader::GcsReader;
//! 
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut reader = GcsReader::new("my-project")?;
//!     reader.with_cache("./cache")?;
//!     let data = reader.read_file("my-bucket", "path/to/file.kore").await?;
//!     Ok(())
//! }
//! ```

use std::error::Error;
use std::fmt;

#[cfg(feature = "gcs")]
use log;

/// Google Cloud Storage Reader Configuration and Operations
#[derive(Debug, Clone)]
pub struct GcsReader {
    project_id: String,
    cache_enabled: bool,
    cache_dir: Option<String>,
}

/// Error types for GCS operations
#[derive(Debug)]
pub enum GcsError {
    /// GCS SDK error
    GcsError(String),
    /// Invalid path format
    InvalidPath(String),
    /// Object not found
    NotFound(String),
    /// Authentication failed
    AuthenticationError(String),
    /// IO error
    IoError(String),
}

impl fmt::Display for GcsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GcsError::GcsError(e) => write!(f, "GCS Error: {}", e),
            GcsError::InvalidPath(p) => write!(f, "Invalid path: {}", p),
            GcsError::NotFound(p) => write!(f, "Object not found: {}", p),
            GcsError::AuthenticationError(e) => write!(f, "Authentication failed: {}", e),
            GcsError::IoError(e) => write!(f, "IO Error: {}", e),
        }
    }
}

impl Error for GcsError {}

/// Metadata for an object in GCS
#[derive(Debug, Clone)]
pub struct GcsObjectMetadata {
    /// Object size in bytes
    pub size: u64,
    /// Last modification time (RFC 3339 format)
    pub last_modified: String,
    /// Generation/version ID
    pub generation: String,
    /// Content type
    pub content_type: Option<String>,
}

impl GcsReader {
    /// Create reader for Google Cloud Storage
    ///
    /// # Arguments
    ///
    /// * `project_id` - Google Cloud Project ID
    ///
    /// # Returns
    ///
    /// New GcsReader instance
    ///
    /// # Errors
    ///
    /// Returns `GcsError::InvalidPath` if project_id is empty
    ///
    /// # Example
    ///
    /// ```ignore
    /// let reader = GcsReader::new("my-project")?;
    /// ```
    pub fn new(project_id: &str) -> Result<Self, GcsError> {
        if project_id.is_empty() {
            return Err(GcsError::InvalidPath(
                "Project ID cannot be empty".to_string(),
            ));
        }

        Ok(GcsReader {
            project_id: project_id.to_string(),
            cache_enabled: false,
            cache_dir: None,
        })
    }

    /// Enable local file caching
    ///
    /// # Example
    ///
    /// ```ignore
    /// reader.with_cache("./gcs_cache")?;
    /// ```
    pub fn with_cache(&mut self, cache_dir: &str) -> Result<(), GcsError> {
        if cache_dir.is_empty() {
            return Err(GcsError::InvalidPath(
                "Cache directory cannot be empty".to_string(),
            ));
        }
        self.cache_dir = Some(cache_dir.to_string());
        self.cache_enabled = true;
        Ok(())
    }

    /// Read object from GCS
    ///
    /// Checks cache first if enabled. Downloads from GCS if not cached.
    ///
    /// # Arguments
    ///
    /// * `bucket` - GCS bucket name
    /// * `object_path` - Path to object within bucket
    ///
    /// # Returns
    ///
    /// Object contents as bytes
    ///
    /// # Example
    ///
    /// ```ignore
    /// let data = reader.read_file("my-bucket", "data/file.kore").await?;
    /// ```
    pub async fn read_file(&self, bucket: &str, object_path: &str) -> Result<Vec<u8>, GcsError> {
        self.validate_bucket_path(bucket, object_path)?;

        // Check cache first
        if self.cache_enabled {
            if let Some(cached) = self.read_from_cache(bucket, object_path).await? {
                return Ok(cached);
            }
        }

        // Read from GCS
        let data = self.read_from_gcs(bucket, object_path).await?;

        // Update cache
        if self.cache_enabled {
            let _ = self.write_to_cache(bucket, object_path, &data).await;
        }

        Ok(data)
    }

    /// Write object to GCS
    pub async fn write_file(
        &self,
        bucket: &str,
        object_path: &str,
        data: &[u8],
    ) -> Result<(), GcsError> {
        self.validate_bucket_path(bucket, object_path)?;

        self.write_to_gcs(bucket, object_path, data).await?;

        if self.cache_enabled {
            let _ = self.write_to_cache(bucket, object_path, data).await;
        }

        Ok(())
    }

    /// List objects in bucket
    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, GcsError> {
        if bucket.is_empty() {
            return Err(GcsError::InvalidPath("Bucket name cannot be empty".to_string()));
        }

        self.list_gcs_objects(bucket, prefix).await
    }

    /// Get object metadata
    pub async fn get_metadata(
        &self,
        bucket: &str,
        object_path: &str,
    ) -> Result<GcsObjectMetadata, GcsError> {
        self.validate_bucket_path(bucket, object_path)?;
        self.fetch_gcs_metadata(bucket, object_path).await
    }

    /// Get project ID
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Check if caching is enabled
    pub fn cache_enabled(&self) -> bool {
        self.cache_enabled
    }

    // Private helper methods

    fn validate_bucket_path(&self, bucket: &str, object_path: &str) -> Result<(), GcsError> {
        if bucket.is_empty() {
            return Err(GcsError::InvalidPath("Bucket name cannot be empty".to_string()));
        }
        if object_path.is_empty() {
            return Err(GcsError::InvalidPath("Object path cannot be empty".to_string()));
        }
        Ok(())
    }

    async fn read_from_gcs(&self, bucket: &str, object_path: &str) -> Result<Vec<u8>, GcsError> {
        #[cfg(feature = "gcs")]
        {
            use google_cloud_storage::client::{Client, ClientConfig};
            use google_cloud_default::WithAuthExt;
            
            // Create client with default credentials
            let config = ClientConfig::default().with_auth().await
                .map_err(|e| GcsError::AuthenticationError(format!("Failed to load credentials: {}", e)))?;
            let client = Client::new(config);
            
            // Get bucket and object client
            let bucket_client = client.bucket(bucket);
            let object_client = bucket_client.object(object_path);
            
            // Download with retry logic
            let mut attempt = 0;
            const MAX_RETRIES: u32 = 3;
            
            loop {
                match object_client.download().await {
                    Ok(bytes) => {
                        log::info!("Successfully read {} bytes from gs://{}/{}", 
                            bytes.len(), bucket, object_path);
                        return Ok(bytes);
                    }
                    Err(e) if attempt < MAX_RETRIES => {
                        attempt += 1;
                        let wait_time = std::time::Duration::from_millis(100 * 2_u64.pow(attempt));
                        log::warn!("GCS read attempt {} failed for gs://{}/{}: {}. Retrying in {:?}...",
                            attempt, bucket, object_path, e, wait_time);
                        tokio::time::sleep(wait_time).await;
                    }
                    Err(e) => {
                        return Err(GcsError::GcsError(
                            format!("Failed to read object gs://{}/{} after {} attempts: {}", 
                                bucket, object_path, MAX_RETRIES, e)
                        ));
                    }
                }
            }
        }
        
        #[cfg(not(feature = "gcs"))]
        {
            Err(GcsError::GcsError(
                "GCS SDK integration not enabled - compile with 'gcs' feature".to_string(),
            ))
        }
    }

    async fn write_to_gcs(
        &self,
        bucket: &str,
        object_path: &str,
        data: &[u8],
    ) -> Result<(), GcsError> {
        #[cfg(feature = "gcs")]
        {
            use google_cloud_storage::client::{Client, ClientConfig};
            use google_cloud_default::WithAuthExt;
            
            // Create client
            let config = ClientConfig::default().with_auth().await
                .map_err(|e| GcsError::AuthenticationError(format!("Failed to load credentials: {}", e)))?;
            let client = Client::new(config);
            
            // Get bucket and object client
            let bucket_client = client.bucket(bucket);
            let object_client = bucket_client.object(object_path);
            
            // Upload with chunking for large objects
            const CHUNK_SIZE: usize = 256 * 1024 * 1024; // 256MB chunks
            
            if data.len() <= CHUNK_SIZE {
                // Single upload for small objects
                object_client
                    .upload(data, "application/octet-stream")
                    .await
                    .map_err(|e| GcsError::GcsError(format!("Upload failed: {}", e)))?;
            } else {
                // Multipart upload for large objects
                let mut offset = 0;
                while offset < data.len() {
                    let end = std::cmp::min(offset + CHUNK_SIZE, data.len());
                    let chunk = &data[offset..end];
                    
                    object_client
                        .upload(chunk, "application/octet-stream")
                        .await
                        .map_err(|e| GcsError::GcsError(format!("Chunk upload error: {}", e)))?;
                    
                    offset = end;
                }
            }
            
            log::info!("Successfully wrote {} bytes to gs://{}/{}", data.len(), bucket, object_path);
            Ok(())
        }
        
        #[cfg(not(feature = "gcs"))]
        {
            Err(GcsError::GcsError(
                "GCS SDK integration not enabled - compile with 'gcs' feature".to_string(),
            ))
        }
    }

    async fn list_gcs_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, GcsError> {
        #[cfg(feature = "gcs")]
        {
            use google_cloud_storage::client::{Client, ClientConfig};
            use google_cloud_default::WithAuthExt;
            
            // Create client
            let config = ClientConfig::default().with_auth().await
                .map_err(|e| GcsError::AuthenticationError(format!("Failed to load credentials: {}", e)))?;
            let client = Client::new(config);
            
            // Get bucket client
            let bucket_client = client.bucket(bucket);
            
            let mut query = bucket_client.list_by_prefix("", "/").await
                .map_err(|e| GcsError::GcsError(format!("List error: {}", e)))?;
            
            // Apply prefix filter if provided
            if let Some(p) = prefix {
                if !p.is_empty() {
                    query = bucket_client.list_by_prefix(p, "/").await
                        .map_err(|e| GcsError::GcsError(format!("Prefix list error: {}", e)))?;
                }
            }
            
            let mut object_names = Vec::new();
            
            // Extract object names from query results
            for (prefix_result, delimiter_result) in &query {
                for object in prefix_result {
                    object_names.push(object.name.clone());
                }
            }
            
            log::info!("Listed {} objects in bucket gs://{}", object_names.len(), bucket);
            Ok(object_names)
        }
        
        #[cfg(not(feature = "gcs"))]
        {
            Err(GcsError::GcsError(
                "GCS SDK integration not enabled - compile with 'gcs' feature".to_string(),
            ))
        }
    }

    async fn fetch_gcs_metadata(
        &self,
        bucket: &str,
        object_path: &str,
    ) -> Result<GcsObjectMetadata, GcsError> {
        #[cfg(feature = "gcs")]
        {
            use google_cloud_storage::client::{Client, ClientConfig};
            use google_cloud_default::WithAuthExt;
            
            // Create client
            let config = ClientConfig::default().with_auth().await
                .map_err(|e| GcsError::AuthenticationError(format!("Failed to load credentials: {}", e)))?;
            let client = Client::new(config);
            
            // Get bucket and object client
            let bucket_client = client.bucket(bucket);
            let object_client = bucket_client.object(object_path);
            
            // Get object metadata
            match object_client.meta().await {
                Ok(object) => {
                    Ok(GcsObjectMetadata {
                        size: object.size,
                        last_modified: object.updated
                            .map(|t| t.to_rfc3339())
                            .unwrap_or_else(|| "unknown".to_string()),
                        generation: object.generation.to_string(),
                        content_type: Some(object.content_type.clone()),
                    })
                }
                Err(e) => {
                    Err(GcsError::GcsError(
                        format!("Metadata fetch error: {}", e)
                    ))
                }
            }
        }
        
        #[cfg(not(feature = "gcs"))]
        {
            Err(GcsError::GcsError(
                "GCS SDK integration not enabled - compile with 'gcs' feature".to_string(),
            ))
        }
    }

    async fn read_from_cache(
        &self,
        bucket: &str,
        object_path: &str,
    ) -> Result<Option<Vec<u8>>, GcsError> {
        // TODO: Implement local caching
        Ok(None)
    }

    async fn write_to_cache(
        &self,
        bucket: &str,
        object_path: &str,
        data: &[u8],
    ) -> Result<(), GcsError> {
        // TODO: Implement local caching
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid_project() {
        let reader = GcsReader::new("my-project");
        assert!(reader.is_ok());
    }

    #[test]
    fn test_new_empty_project() {
        let result = GcsReader::new("");
        assert!(result.is_err());
    }

    #[test]
    fn test_with_cache() {
        let mut reader = GcsReader::new("project").unwrap();
        let result = reader.with_cache("./cache");
        assert!(result.is_ok());
        assert!(reader.cache_enabled());
    }

    #[test]
    fn test_with_cache_empty_path() {
        let mut reader = GcsReader::new("project").unwrap();
        let result = reader.with_cache("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_bucket_path() {
        let reader = GcsReader::new("project").unwrap();
        let result = reader.validate_bucket_path("bucket", "path");
        assert!(result.is_ok());

        let result = reader.validate_bucket_path("", "path");
        assert!(result.is_err());

        let result = reader.validate_bucket_path("bucket", "");
        assert!(result.is_err());
    }
}
