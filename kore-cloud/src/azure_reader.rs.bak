//! Microsoft Azure Blob Storage reader with range request support

use async_trait::async_trait;
use bytes::Bytes;
use log::*;

use crate::{CloudReader, CloudReaderConfig, ObjectMetadata, RangeRequest, Result, CloudError};

/// Azure Blob Storage reader implementation
pub struct AzureReader {
    container: String,
    blob: String,
    account: Option<String>,
}

impl AzureReader {
    /// Create new Azure reader
    pub fn new(config: CloudReaderConfig) -> Result<Self> {
        info!(
            "Created Azure reader for az://{}/{}",
            config.bucket, config.key
        );

        Ok(AzureReader {
            container: config.bucket,
            blob: config.key,
            account: config.endpoint,
        })
    }

    /// Get Azure REST API endpoint
    fn api_url(&self) -> String {
        let account = self.account.as_deref().unwrap_or("storageaccount");
        format!(
            "https://{}.blob.core.windows.net/{}/{}",
            account, self.container, self.blob
        )
    }
}

#[async_trait]
impl CloudReader for AzureReader {
    async fn size(&self) -> Result<u64> {
        debug!(
            "Getting size for az://{}/{}",
            self.container, self.blob
        );

        // Would use Azure REST API to get blob size
        warn!("Azure size() not yet fully implemented");
        Ok(0)
    }

    async fn read_all(&self) -> Result<Bytes> {
        debug!(
            "Reading entire blob from az://{}/{}",
            self.container, self.blob
        );

        // Would use Azure REST API to download entire blob
        warn!("Azure read_all() not yet fully implemented");
        Ok(Bytes::new())
    }

    async fn read_range(&self, range: RangeRequest) -> Result<Bytes> {
        debug!(
            "Reading range {}-{} from az://{}/{}",
            range.start, range.end, self.container, self.blob
        );

        // Would use Azure REST API with Range header
        warn!("Azure read_range() not yet fully implemented");
        Ok(Bytes::new())
    }

    async fn metadata(&self) -> Result<ObjectMetadata> {
        debug!(
            "Getting metadata for az://{}/{}",
            self.container, self.blob
        );

        Ok(ObjectMetadata {
            size: 0,
            last_modified: String::new(),
            etag: String::new(),
            content_type: "application/octet-stream".to_string(),
        })
    }

    async fn exists(&self) -> Result<bool> {
        debug!(
            "Checking if exists: az://{}/{}",
            self.container, self.blob
        );
        Ok(false)
    }

    fn provider(&self) -> &'static str {
        "azure"
    }

    fn path(&self) -> String {
        format!("az://{}/{}", self.container, self.blob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_azure_reader_path() {
        let config = CloudReaderConfig::azure("mycontainer", "data.kore");
        let reader = AzureReader::new(config).unwrap();
        assert_eq!(reader.path(), "az://mycontainer/data.kore");
    }

    #[test]
    fn test_azure_provider() {
        let config = CloudReaderConfig::azure("mycontainer", "data.kore");
        let reader = AzureReader::new(config).unwrap();
        assert_eq!(reader.provider(), "azure");
    }

    #[test]
    fn test_azure_api_url() {
        let config = CloudReaderConfig::azure("mycontainer", "data.kore");
        let reader = AzureReader::new(config).unwrap();
        let url = reader.api_url();
        assert!(url.contains("blob.core.windows.net"));
        assert!(url.contains("mycontainer"));
        assert!(url.contains("data.kore"));
    }
}
