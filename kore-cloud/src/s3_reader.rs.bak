//! Amazon S3 reader with range request support

use async_trait::async_trait;
use bytes::Bytes;
use log::*;
use rusoto_core::Region;
use rusoto_s3::{GetObjectRequest, HeadObjectRequest, S3Client, S3};
use tokio_util::io::StreamReader;
use tokio::io::AsyncReadExt;

use crate::{CloudReader, CloudReaderConfig, ObjectMetadata, RangeRequest, Result, CloudError};

/// AWS S3 reader implementation
pub struct S3Reader {
    client: S3Client,
    bucket: String,
    key: String,
    region: String,
}

impl S3Reader {
    /// Create new S3 reader
    pub fn new(config: CloudReaderConfig) -> Result<Self> {
        let region = config
            .region
            .clone()
            .unwrap_or_else(|| "us-east-1".to_string());

        let parsed_region = region.parse::<Region>()
            .map_err(|e| CloudError::S3Error(format!("Invalid region: {}", e)))?;

        let client = S3Client::new(parsed_region);

        info!("Created S3 reader for s3://{}/{}", config.bucket, config.key);

        Ok(S3Reader {
            client,
            bucket: config.bucket,
            key: config.key,
            region,
        })
    }
}

#[async_trait]
impl CloudReader for S3Reader {
    async fn size(&self) -> Result<u64> {
        debug!("Getting size for s3://{}/{}", self.bucket, self.key);

        let req = HeadObjectRequest {
            bucket: self.bucket.clone(),
            key: self.key.clone(),
            ..Default::default()
        };

        let output = self.client
            .head_object(req)
            .await
            .map_err(|e| CloudError::S3Error(format!("HeadObject failed: {}", e)))?;

        let size = output
            .content_length
            .ok_or_else(|| CloudError::S3Error("No content-length header".to_string()))?;

        info!("S3 object size: {} bytes", size);
        Ok(size as u64)
    }

    async fn read_all(&self) -> Result<Bytes> {
        debug!("Reading entire object from s3://{}/{}", self.bucket, self.key);

        let req = GetObjectRequest {
            bucket: self.bucket.clone(),
            key: self.key.clone(),
            ..Default::default()
        };

        let output = self.client
            .get_object(req)
            .await
            .map_err(|e| CloudError::S3Error(format!("GetObject failed: {}", e)))?;

        let body = output
            .body
            .ok_or_else(|| CloudError::S3Error("No body in response".to_string()))?;

        let mut reader = StreamReader::new(body.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("S3 error: {}", e))
        }));

        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)
            .await
            .map_err(|e| CloudError::IoError(e))?;

        info!("Read {} bytes from S3", buffer.len());
        Ok(Bytes::from(buffer))
    }

    async fn read_range(&self, range: RangeRequest) -> Result<Bytes> {
        debug!(
            "Reading range {}-{} from s3://{}/{}",
            range.start, range.end, self.bucket, self.key
        );

        let req = GetObjectRequest {
            bucket: self.bucket.clone(),
            key: self.key.clone(),
            range: Some(range.to_header()),
            ..Default::default()
        };

        let output = self.client
            .get_object(req)
            .await
            .map_err(|e| CloudError::S3Error(format!("GetObject with range failed: {}", e)))?;

        let body = output
            .body
            .ok_or_else(|| CloudError::S3Error("No body in response".to_string()))?;

        let mut reader = StreamReader::new(body.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("S3 error: {}", e))
        }));

        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)
            .await
            .map_err(|e| CloudError::IoError(e))?;

        info!(
            "Read {} bytes from S3 range {}-{}",
            buffer.len(),
            range.start,
            range.end
        );
        Ok(Bytes::from(buffer))
    }

    async fn read_ranges(&self, ranges: Vec<RangeRequest>) -> Result<Vec<Bytes>> {
        debug!(
            "Reading {} ranges from s3://{}/{}",
            ranges.len(),
            self.bucket,
            self.key
        );

        let futures = ranges
            .into_iter()
            .map(|range| self.read_range(range));

        let results = futures::future::try_join_all(futures).await?;

        info!("Read {} ranges from S3", results.len());
        Ok(results)
    }

    async fn metadata(&self) -> Result<ObjectMetadata> {
        debug!("Getting metadata for s3://{}/{}", self.bucket, self.key);

        let req = HeadObjectRequest {
            bucket: self.bucket.clone(),
            key: self.key.clone(),
            ..Default::default()
        };

        let output = self.client
            .head_object(req)
            .await
            .map_err(|e| CloudError::S3Error(format!("HeadObject failed: {}", e)))?;

        let metadata = ObjectMetadata {
            size: output.content_length.unwrap_or(0) as u64,
            last_modified: output.last_modified.unwrap_or_default(),
            etag: output.e_tag.unwrap_or_default(),
            content_type: output.content_type.unwrap_or_default(),
        };

        Ok(metadata)
    }

    async fn exists(&self) -> Result<bool> {
        debug!("Checking if exists: s3://{}/{}", self.bucket, self.key);

        let req = HeadObjectRequest {
            bucket: self.bucket.clone(),
            key: self.key.clone(),
            ..Default::default()
        };

        match self.client.head_object(req).await {
            Ok(_) => {
                info!("Object exists: s3://{}/{}", self.bucket, self.key);
                Ok(true)
            }
            Err(e) => {
                if e.to_string().contains("404") || e.to_string().contains("NoSuchKey") {
                    info!("Object not found: s3://{}/{}", self.bucket, self.key);
                    Ok(false)
                } else {
                    Err(CloudError::S3Error(format!("HeadObject failed: {}", e)))
                }
            }
        }
    }

    fn provider(&self) -> &'static str {
        "s3"
    }

    fn path(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_reader_path() {
        let config = CloudReaderConfig::s3("mybucket", "data.kore");
        let reader = S3Reader::new(config);
        if let Ok(r) = reader {
            assert_eq!(r.path(), "s3://mybucket/data.kore");
        }
    }

    #[test]
    fn test_s3_provider() {
        let config = CloudReaderConfig::s3("mybucket", "data.kore");
        let reader = S3Reader::new(config);
        if let Ok(r) = reader {
            assert_eq!(r.provider(), "s3");
        }
    }
}
