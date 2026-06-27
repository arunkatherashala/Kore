//! Cloud storage reader traits and configuration

use async_trait::async_trait;
use bytes::Bytes;
use crate::{Result, RangeRequest};

/// Configuration for cloud readers
#[derive(Debug, Clone)]
pub struct CloudReaderConfig {
    pub bucket: String,
    pub key: String,
    pub region: Option<String>,
    pub endpoint: Option<String>,
}

impl CloudReaderConfig {
    /// Create S3 config
    pub fn s3(bucket: &str, key: &str) -> Self {
        Self {
            bucket: bucket.to_string(),
            key: key.to_string(),
            region: None,
            endpoint: None,
        }
    }

    /// Create GCS config
    pub fn gcs(bucket: &str, object: &str) -> Self {
        Self {
            bucket: bucket.to_string(),
            key: object.to_string(),
            region: None,
            endpoint: None,
        }
    }

    /// Create Azure config
    pub fn azure(container: &str, blob: &str) -> Self {
        Self {
            bucket: container.to_string(),
            key: blob.to_string(),
            region: None,
            endpoint: None,
        }
    }
}

/// Cloud storage reader trait
#[async_trait]
pub trait CloudReader: Send + Sync {
    /// Get the total size of the object in bytes
    async fn size(&self) -> Result<u64>;

    /// Read the entire object
    async fn read_all(&self) -> Result<Bytes>;

    /// Read a range of bytes from the object (efficient with HTTP Range)
    async fn read_range(&self, range: RangeRequest) -> Result<Bytes>;

    /// Read multiple ranges in parallel
    async fn read_ranges(&self, ranges: Vec<RangeRequest>) -> Result<Vec<Bytes>> {
        // Default implementation: read sequentially
        let mut results = Vec::with_capacity(ranges.len());
        for range in ranges {
            results.push(self.read_range(range).await?);
        }
        Ok(results)
    }

    /// Get metadata about the object
    async fn metadata(&self) -> Result<ObjectMetadata>;

    /// Check if object exists
    async fn exists(&self) -> Result<bool>;

    /// Provider name (s3, gcs, azure)
    fn provider(&self) -> &'static str;

    /// Path for logging/debugging
    fn path(&self) -> String;
}

/// Object metadata
#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub size: u64,
    pub last_modified: String,
    pub etag: String,
    pub content_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_config() {
        let config = CloudReaderConfig::s3("mybucket", "data.kore");
        assert_eq!(config.bucket, "mybucket");
        assert_eq!(config.key, "data.kore");
        assert_eq!(config.region, None);
    }

    #[test]
    fn test_gcs_config() {
        let config = CloudReaderConfig::gcs("mybucket", "data.kore");
        assert_eq!(config.bucket, "mybucket");
        assert_eq!(config.key, "data.kore");
    }

    #[test]
    fn test_azure_config() {
        let config = CloudReaderConfig::azure("mycontainer", "data.kore");
        assert_eq!(config.bucket, "mycontainer");
        assert_eq!(config.key, "data.kore");
    }
}
