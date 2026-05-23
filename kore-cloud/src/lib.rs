//! Kore Cloud Storage Integration
//! 
//! Provides unified API for reading Kore files from cloud storage (S3, GCS, Azure)
//! with efficient range requests for streaming data without full downloads.

pub mod cloud_traits;
pub mod s3_reader;
pub mod gcs_reader;
pub mod azure_reader;
pub mod range_request;
pub mod error;

pub use cloud_traits::{CloudReader, CloudReaderConfig};
pub use error::{CloudError, Result};
pub use range_request::RangeRequest;

/// Cloud storage provider types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    S3,
    GCS,
    Azure,
}

/// Unified cloud reader builder
pub struct CloudReaderBuilder {
    provider: Provider,
    config: CloudReaderConfig,
}

impl CloudReaderBuilder {
    /// Create builder for S3
    pub fn s3(bucket: &str, key: &str) -> Self {
        Self {
            provider: Provider::S3,
            config: CloudReaderConfig::s3(bucket, key),
        }
    }

    /// Create builder for GCS
    pub fn gcs(bucket: &str, object: &str) -> Self {
        Self {
            provider: Provider::GCS,
            config: CloudReaderConfig::gcs(bucket, object),
        }
    }

    /// Create builder for Azure
    pub fn azure(container: &str, blob: &str) -> Self {
        Self {
            provider: Provider::Azure,
            config: CloudReaderConfig::azure(container, blob),
        }
    }

    /// Set region for S3
    pub fn with_region(mut self, region: &str) -> Self {
        self.config.region = Some(region.to_string());
        self
    }

    /// Set endpoint URL (for S3-compatible services)
    pub fn with_endpoint(mut self, endpoint: &str) -> Self {
        self.config.endpoint = Some(endpoint.to_string());
        self
    }

    /// Build the cloud reader
    pub fn build(self) -> Result<Box<dyn CloudReader>> {
        match self.provider {
            Provider::S3 => {
                #[cfg(feature = "s3")]
                {
                    Ok(Box::new(s3_reader::S3Reader::new(self.config)?))
                }
                #[cfg(not(feature = "s3"))]
                Err(CloudError::FeatureNotEnabled("S3 support not enabled".to_string()))
            }
            Provider::GCS => {
                #[cfg(feature = "gcs")]
                {
                    Ok(Box::new(gcs_reader::GCSReader::new(self.config)?))
                }
                #[cfg(not(feature = "gcs"))]
                Err(CloudError::FeatureNotEnabled("GCS support not enabled".to_string()))
            }
            Provider::Azure => {
                #[cfg(feature = "azure")]
                {
                    Ok(Box::new(azure_reader::AzureReader::new(self.config)?))
                }
                #[cfg(not(feature = "azure"))]
                Err(CloudError::FeatureNotEnabled("Azure support not enabled".to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloud_reader_builder_s3() {
        let builder = CloudReaderBuilder::s3("mybucket", "data.kore");
        assert_eq!(builder.provider, Provider::S3);
    }

    #[test]
    fn test_cloud_reader_builder_gcs() {
        let builder = CloudReaderBuilder::gcs("mybucket", "data.kore");
        assert_eq!(builder.provider, Provider::GCS);
    }

    #[test]
    fn test_cloud_reader_builder_azure() {
        let builder = CloudReaderBuilder::azure("mycontainer", "data.kore");
        assert_eq!(builder.provider, Provider::Azure);
    }

    #[test]
    fn test_cloud_reader_builder_with_region() {
        let builder = CloudReaderBuilder::s3("mybucket", "data.kore")
            .with_region("us-east-1");
        assert_eq!(builder.config.region, Some("us-east-1".to_string()));
    }

    #[test]
    fn test_cloud_reader_builder_with_endpoint() {
        let builder = CloudReaderBuilder::s3("mybucket", "data.kore")
            .with_endpoint("https://minio.example.com");
        assert_eq!(builder.config.endpoint, Some("https://minio.example.com".to_string()));
    }
}
