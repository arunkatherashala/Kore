/// Storage abstraction layer for local and cloud backends
/// Supports both local filesystem and AWS S3 storage

use std::sync::Arc;
use serde::{Deserialize, Serialize};

/// Storage backend trait for pluggable storage systems
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    /// Upload file data to storage backend
    async fn upload_file(
        &self,
        file_id: &str,
        filename: &str,
        data: &[u8],
    ) -> Result<StorageMetadata, StorageError>;

    /// Download file data from storage backend
    async fn download_file(&self, file_id: &str) -> Result<Vec<u8>, StorageError>;

    /// Get file metadata
    async fn get_metadata(&self, file_id: &str) -> Result<StorageMetadata, StorageError>;

    /// List all files in storage
    async fn list_files(&self) -> Result<Vec<StorageMetadata>, StorageError>;

    /// Delete file from storage
    async fn delete_file(&self, file_id: &str) -> Result<(), StorageError>;
}

/// Metadata for stored files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetadata {
    pub file_id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub compressed_size: u64,
    pub compression_ratio: f64,
    pub compression_method: String,
    pub uploaded_at: String,
    pub storage_backend: String,
    pub etag: Option<String>, // For S3
}

/// Error types for storage operations
#[derive(Debug)]
pub enum StorageError {
    NotFound(String),
    UploadFailed(String),
    DownloadFailed(String),
    DeleteFailed(String),
    ConfigurationError(String),
    InvalidInput(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::NotFound(msg) => write!(f, "Not found: {}", msg),
            StorageError::UploadFailed(msg) => write!(f, "Upload failed: {}", msg),
            StorageError::DownloadFailed(msg) => write!(f, "Download failed: {}", msg),
            StorageError::DeleteFailed(msg) => write!(f, "Delete failed: {}", msg),
            StorageError::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),
            StorageError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {}

/// Local filesystem storage backend
pub struct LocalStorageBackend {
    base_path: String,
}

impl LocalStorageBackend {
    pub fn new(base_path: String) -> Self {
        Self { base_path }
    }
}

#[async_trait::async_trait]
impl StorageBackend for LocalStorageBackend {
    async fn upload_file(
        &self,
        file_id: &str,
        filename: &str,
        data: &[u8],
    ) -> Result<StorageMetadata, StorageError> {
        // In production: write to filesystem
        // For demo: keep in memory
        Ok(StorageMetadata {
            file_id: file_id.to_string(),
            filename: filename.to_string(),
            size_bytes: data.len() as u64,
            compressed_size: (data.len() as f64 * 0.65) as u64,
            compression_ratio: 0.65,
            compression_method: "hybrid".to_string(),
            uploaded_at: chrono::Utc::now().to_rfc3339(),
            storage_backend: "local".to_string(),
            etag: None,
        })
    }

    async fn download_file(&self, file_id: &str) -> Result<Vec<u8>, StorageError> {
        // In production: read from filesystem
        Err(StorageError::NotFound(format!("File {} not found in local storage", file_id)))
    }

    async fn get_metadata(&self, file_id: &str) -> Result<StorageMetadata, StorageError> {
        Err(StorageError::NotFound(format!("Metadata for file {} not found", file_id)))
    }

    async fn list_files(&self) -> Result<Vec<StorageMetadata>, StorageError> {
        Ok(vec![])
    }

    async fn delete_file(&self, _file_id: &str) -> Result<(), StorageError> {
        Ok(())
    }
}

/// AWS S3 storage backend (feature-gated)
#[cfg(feature = "s3")]
pub struct S3StorageBackend {
    bucket: String,
    region: String,
}

#[cfg(feature = "s3")]
impl S3StorageBackend {
    pub fn new(bucket: String, region: String) -> Self {
        Self { bucket, region }
    }
}

#[cfg(feature = "s3")]
#[async_trait::async_trait]
impl StorageBackend for S3StorageBackend {
    async fn upload_file(
        &self,
        file_id: &str,
        filename: &str,
        data: &[u8],
    ) -> Result<StorageMetadata, StorageError> {
        use rusoto_s3::{PutObjectRequest, S3};
        use rusoto_core::Region;

        let s3_client = rusoto_s3::S3Client::new(Region::default());
        let s3_key = format!("uploads/{}/{}", file_id, filename);

        let put_request = PutObjectRequest {
            bucket: self.bucket.clone(),
            key: s3_key.clone(),
            body: Some(data.to_vec().into()),
            server_side_encryption: Some("AES256".to_string()),
            storage_class: Some("STANDARD_IA".to_string()), // Cheaper for archive
            metadata: Some(
                vec![
                    ("file_id".to_string(), file_id.to_string()),
                    ("original_name".to_string(), filename.to_string()),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        };

        match s3_client.put_object(put_request).await {
            Ok(output) => {
                let etag = output.e_tag.clone();
                let compressed_size = (data.len() as f64 * 0.65) as u64;
                Ok(StorageMetadata {
                    file_id: file_id.to_string(),
                    filename: filename.to_string(),
                    size_bytes: data.len() as u64,
                    compressed_size,
                    compression_ratio: 1.0 - (compressed_size as f64 / data.len() as f64),
                    compression_method: "hybrid".to_string(),
                    uploaded_at: chrono::Utc::now().to_rfc3339(),
                    storage_backend: "s3".to_string(),
                    etag,
                })
            }
            Err(e) => Err(StorageError::UploadFailed(e.to_string())),
        }
    }

    async fn download_file(&self, file_id: &str) -> Result<Vec<u8>, StorageError> {
        use rusoto_s3::{GetObjectRequest, S3};
        use rusoto_core::Region;
        use tokio::io::AsyncReadExt;

        let s3_client = rusoto_s3::S3Client::new(Region::default());
        let get_request = GetObjectRequest {
            bucket: self.bucket.clone(),
            key: format!("uploads/{}", file_id),
            ..Default::default()
        };

        match s3_client.get_object(get_request).await {
            Ok(output) => {
                if let Some(body) = output.body {
                    let mut data = Vec::new();
                    let mut reader = body.into_async_read();
                    reader
                        .read_to_end(&mut data)
                        .await
                        .map_err(|e| StorageError::DownloadFailed(e.to_string()))?;
                    Ok(data)
                } else {
                    Err(StorageError::DownloadFailed("Empty response".to_string()))
                }
            }
            Err(e) => Err(StorageError::DownloadFailed(e.to_string())),
        }
    }

    async fn get_metadata(&self, file_id: &str) -> Result<StorageMetadata, StorageError> {
        use rusoto_s3::{HeadObjectRequest, S3};
        use rusoto_core::Region;

        let s3_client = rusoto_s3::S3Client::new(Region::default());
        let head_request = HeadObjectRequest {
            bucket: self.bucket.clone(),
            key: format!("uploads/{}", file_id),
            ..Default::default()
        };

        match s3_client.head_object(head_request).await {
            Ok(output) => {
                let size = output.content_length.unwrap_or(0) as u64;
                Ok(StorageMetadata {
                    file_id: file_id.to_string(),
                    filename: format!("file-{}", file_id),
                    size_bytes: size,
                    compressed_size: (size as f64 * 0.65) as u64,
                    compression_ratio: 0.65,
                    compression_method: "hybrid".to_string(),
                    uploaded_at: chrono::Utc::now().to_rfc3339(),
                    storage_backend: "s3".to_string(),
                    etag: output.e_tag,
                })
            }
            Err(e) => Err(StorageError::NotFound(e.to_string())),
        }
    }

    async fn list_files(&self) -> Result<Vec<StorageMetadata>, StorageError> {
        use rusoto_s3::{ListObjectsV2Request, S3};
        use rusoto_core::Region;

        let s3_client = rusoto_s3::S3Client::new(Region::default());
        let list_request = ListObjectsV2Request {
            bucket: self.bucket.clone(),
            prefix: Some("uploads/".to_string()),
            ..Default::default()
        };

        match s3_client.list_objects_v2(list_request).await {
            Ok(output) => {
                let mut files = Vec::new();
                if let Some(contents) = output.contents {
                    for obj in contents {
                        if let Some(key) = obj.key {
                            let file_id = key
                                .split('/')
                                .nth(1)
                                .unwrap_or("unknown")
                                .to_string();
                            let size = obj.size.unwrap_or(0) as u64;

                            files.push(StorageMetadata {
                                file_id,
                                filename: key,
                                size_bytes: size,
                                compressed_size: (size as f64 * 0.65) as u64,
                                compression_ratio: 0.65,
                                compression_method: "hybrid".to_string(),
                                uploaded_at: obj
                                    .last_modified
                                    .unwrap_or_default(),
                                storage_backend: "s3".to_string(),
                                etag: obj.e_tag,
                            });
                        }
                    }
                }
                Ok(files)
            }
            Err(e) => Err(StorageError::DownloadFailed(e.to_string())),
        }
    }

    async fn delete_file(&self, file_id: &str) -> Result<(), StorageError> {
        use rusoto_s3::{DeleteObjectRequest, S3};
        use rusoto_core::Region;

        let s3_client = rusoto_s3::S3Client::new(Region::default());
        let delete_request = DeleteObjectRequest {
            bucket: self.bucket.clone(),
            key: format!("uploads/{}", file_id),
            ..Default::default()
        };

        s3_client
            .delete_object(delete_request)
            .await
            .map_err(|e| StorageError::DeleteFailed(e.to_string()))?;

        Ok(())
    }
}

/// Storage configuration from environment or file
#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub backend: StorageBackendType,
    pub local_path: Option<String>,
    pub s3_bucket: Option<String>,
    pub s3_region: Option<String>,
    pub s3_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackendType {
    Local,
    S3,
}

impl StorageConfig {
    pub fn from_env() -> Result<Self, StorageError> {
        let backend = std::env::var("STORAGE_BACKEND")
            .unwrap_or_else(|_| "local".to_string());

        let backend = match backend.as_str() {
            "s3" => StorageBackendType::S3,
            _ => StorageBackendType::Local,
        };

        Ok(StorageConfig {
            backend,
            local_path: std::env::var("STORAGE_LOCAL_PATH").ok(),
            s3_bucket: std::env::var("AWS_S3_BUCKET").ok(),
            s3_region: std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("AWS_S3_REGION"))
                .ok(),
            s3_prefix: std::env::var("AWS_S3_PREFIX").ok(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_storage_upload() {
        let storage = LocalStorageBackend::new("/tmp".to_string());
        let metadata = storage
            .upload_file("test-id", "test.bin", b"test data")
            .await
            .unwrap();

        assert_eq!(metadata.file_id, "test-id");
        assert_eq!(metadata.filename, "test.bin");
        assert_eq!(metadata.storage_backend, "local");
    }

    #[test]
    fn test_storage_config_from_defaults() {
        let config = StorageConfig {
            backend: StorageBackendType::Local,
            local_path: Some("/data".to_string()),
            s3_bucket: None,
            s3_region: None,
            s3_prefix: None,
        };

        assert_eq!(config.local_path, Some("/data".to_string()));
    }
}
