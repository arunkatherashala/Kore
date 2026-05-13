//! AWS S3 Connector for Kore FileFormat
//! 
//! Enables reading/writing Kore files directly from/to AWS S3 buckets
//! with streaming support, connection pooling, and automatic retry logic.

use crate::errors::{CloudError, CloudResult};
use aws_sdk_s3::Client;
use aws_config::SdkConfig;
use bytes::Bytes;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// S3 Configuration
#[derive(Debug, Clone)]
pub struct S3Config {
    /// AWS Region (e.g., "us-east-1")
    pub region: String,
    /// S3 Bucket name
    pub bucket: String,
    /// Optional endpoint URL (for S3-compatible services)
    pub endpoint: Option<String>,
    /// Enable path-style URLs (e.g., for MinIO)
    pub force_path_style: bool,
}

impl S3Config {
    /// Create new S3 configuration
    pub fn new(region: impl Into<String>, bucket: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            bucket: bucket.into(),
            endpoint: None,
            force_path_style: false,
        }
    }

    /// Set custom endpoint (e.g., for MinIO or LocalStack)
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Enable path-style URLs
    pub fn with_path_style(mut self, enabled: bool) -> Self {
        self.force_path_style = enabled;
        self
    }
}

/// S3 Reader for Kore files
pub struct S3Reader {
    client: Arc<Client>,
    config: S3Config,
}

impl S3Reader {
    /// Create new S3 reader with AWS SDK auto-configuration
    pub async fn new(config: S3Config) -> CloudResult<Self> {
        info!("Initializing S3Reader for bucket: {}", config.bucket);
        
        let sdk_config = aws_config::load_from_env().await;
        let client = Self::build_client(&sdk_config, &config).await?;
        
        Ok(Self {
            client: Arc::new(client),
            config,
        })
    }

    /// Create with explicit AWS SDK configuration
    pub async fn with_config(aws_config: &SdkConfig, s3_config: S3Config) -> CloudResult<Self> {
        info!("Initializing S3Reader with explicit config for bucket: {}", s3_config.bucket);
        
        let client = Self::build_client(aws_config, &s3_config).await?;
        
        Ok(Self {
            client: Arc::new(client),
            config: s3_config,
        })
    }

    /// Build S3 client with configuration
    async fn build_client(sdk_config: &SdkConfig, config: &S3Config) -> CloudResult<Client> {
        let mut s3_config = aws_sdk_s3::config::Builder::from(sdk_config);
        
        if let Some(endpoint) = &config.endpoint {
            s3_config = s3_config.endpoint_url(endpoint.clone());
        }
        
        if config.force_path_style {
            s3_config = s3_config.force_path_style(true);
        }
        
        Ok(Client::from_conf(s3_config.build()))
    }

    /// Read entire Kore file from S3
    pub async fn read_file(&self, key: &str) -> CloudResult<Vec<u8>> {
        debug!("Reading Kore file from S3: s3://{}/{}", self.config.bucket, key);
        
        match self
            .client
            .get_object()
            .bucket(&self.config.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(response) => {
                let bytes = response.body.collect().await
                    .map_err(|e| CloudError::S3Error(format!("Failed to read body: {}", e)))?;
                info!("Successfully read {} bytes from S3", bytes.len());
                Ok(bytes.into_bytes().to_vec())
            }
            Err(e) => {
                if e.as_service_error()
                    .map(|se| se.is_no_such_key())
                    .unwrap_or(false)
                {
                    warn!("File not found in S3: {}/{}", self.config.bucket, key);
                    Err(CloudError::FileNotFound(format!("{}/{}", self.config.bucket, key)))
                } else {
                    Err(CloudError::S3Error(e.to_string()))
                }
            }
        }
    }

    /// Read Kore file as stream (for large files)
    pub async fn read_file_streaming(&self, key: &str) -> CloudResult<aws_sdk_s3::primitives::ByteStream> {
        debug!("Reading Kore file stream from S3: s3://{}/{}", self.config.bucket, key);
        
        match self
            .client
            .get_object()
            .bucket(&self.config.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(response) => {
                info!("Opened streaming read from S3");
                Ok(response.body)
            }
            Err(e) => {
                if e.as_service_error()
                    .map(|se| se.is_no_such_key())
                    .unwrap_or(false)
                {
                    Err(CloudError::FileNotFound(format!("{}/{}", self.config.bucket, key)))
                } else {
                    Err(CloudError::S3Error(e.to_string()))
                }
            }
        }
    }

    /// Write Kore file to S3
    pub async fn write_file(&self, key: &str, data: &[u8]) -> CloudResult<()> {
        debug!("Writing Kore file to S3: s3://{}/{} ({} bytes)", 
               self.config.bucket, key, data.len());
        
        self.client
            .put_object()
            .bucket(&self.config.bucket)
            .key(key)
            .body(aws_sdk_s3::primitives::ByteStream::from(Bytes::copy_from_slice(data)))
            .send()
            .await
            .map_err(|e| CloudError::S3Error(e.to_string()))?;
        
        info!("Successfully wrote Kore file to S3");
        Ok(())
    }

    /// List Kore files in S3 bucket prefix
    pub async fn list_files(&self, prefix: &str) -> CloudResult<Vec<String>> {
        debug!("Listing Kore files in S3 with prefix: {}", prefix);
        
        let response = self
            .client
            .list_objects_v2()
            .bucket(&self.config.bucket)
            .prefix(prefix)
            .send()
            .await
            .map_err(|e| CloudError::S3Error(e.to_string()))?;
        
        let files = response
            .contents()
            .iter()
            .filter_map(|obj| obj.key().map(|k| k.to_string()))
            .collect();
        
        info!("Found {} files in S3 prefix", files.len());
        Ok(files)
    }

    /// Check if Kore file exists in S3
    pub async fn exists(&self, key: &str) -> CloudResult<bool> {
        debug!("Checking if file exists in S3: {}", key);
        
        match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => {
                debug!("File exists in S3: {}", key);
                Ok(true)
            }
            Err(e) => {
                if e.as_service_error()
                    .map(|se| se.is_not_found())
                    .unwrap_or(false)
                {
                    debug!("File does not exist in S3: {}", key);
                    Ok(false)
                } else {
                    Err(CloudError::S3Error(e.to_string()))
                }
            }
        }
    }

    /// Get file metadata from S3
    pub async fn get_metadata(&self, key: &str) -> CloudResult<FileMetadata> {
        debug!("Getting metadata for S3 file: {}", key);
        
        let response = self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| CloudError::S3Error(e.to_string()))?;
        
        Ok(FileMetadata {
            size: response.content_length().unwrap_or(0) as u64,
            last_modified: response.last_modified().map(|dt| dt.to_string()),
            content_type: response.content_type().map(|s| s.to_string()),
        })
    }
}

/// File metadata
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_config_builder() {
        let config = S3Config::new("us-east-1", "my-bucket")
            .with_endpoint("http://localhost:9000")
            .with_path_style(true);
        
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.bucket, "my-bucket");
        assert_eq!(config.endpoint, Some("http://localhost:9000".to_string()));
        assert!(config.force_path_style);
    }

    #[test]
    fn test_file_metadata() {
        let meta = FileMetadata {
            size: 1024,
            last_modified: Some("2024-01-01T00:00:00Z".to_string()),
            content_type: Some("application/kore".to_string()),
        };
        
        assert_eq!(meta.size, 1024);
        assert!(meta.last_modified.is_some());
    }
}
