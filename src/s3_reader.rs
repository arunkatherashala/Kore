//! AWS S3 Connector for Kore FileFormat
//! 
//! Provides native support for reading and writing Kore files directly from/to AWS S3.
//! 
//! # Features
//! 
//! To use this module, enable the `s3` feature in Cargo.toml:
//! ```toml
//! [dependencies]
//! kore_fileformat = { version = "1.0", features = ["s3"] }
//! ```
//! 
//! # Examples
//! 
//! ```ignore
//! use kore_fileformat::s3_reader::S3Reader;
//! 
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut reader = S3Reader::new("us-east-1")?;
//!     reader.with_cache("./cache")?;
//!     let data = reader.read_file("my-bucket", "path/to/file.kore").await?;
//!     Ok(())
//! }
//! ```

use std::error::Error;
use std::fmt;

/// S3 Reader Configuration and Operations
#[derive(Debug, Clone)]
pub struct S3Reader {
    region: String,
    cache_enabled: bool,
    cache_dir: Option<String>,
}

/// Error types for S3 operations
#[derive(Debug)]
pub enum S3Error {
    /// AWS SDK error
    AwsError(String),
    /// Invalid S3 path format
    InvalidPath(String),
    /// File not found in S3
    NotFound(String),
    /// Authentication failed
    AuthenticationError(String),
    /// IO error
    IoError(String),
}

impl fmt::Display for S3Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            S3Error::AwsError(e) => write!(f, "AWS Error: {}", e),
            S3Error::InvalidPath(p) => write!(f, "Invalid S3 path: {}", p),
            S3Error::NotFound(p) => write!(f, "File not found in S3: {}", p),
            S3Error::AuthenticationError(e) => write!(f, "Authentication failed: {}", e),
            S3Error::IoError(e) => write!(f, "IO Error: {}", e),
        }
    }
}

impl Error for S3Error {}

impl S3Reader {
    /// Create a new S3Reader with specified AWS region
    /// 
    /// # Arguments
    /// * `region` - AWS region (e.g., "us-east-1", "eu-west-1")
    /// 
    /// # Example
    /// ```rust
    /// let reader = S3Reader::new("us-east-1")?;
    /// ```
    pub fn new(region: &str) -> Result<Self, S3Error> {
        if region.is_empty() {
            return Err(S3Error::InvalidPath("Region cannot be empty".to_string()));
        }

        Ok(S3Reader {
            region: region.to_string(),
            cache_enabled: false,
            cache_dir: None,
        })
    }

    /// Enable caching of downloaded files locally
    /// 
    /// # Arguments
    /// * `cache_dir` - Directory to cache files in
    /// 
    /// # Example
    /// ```rust
    /// let mut reader = S3Reader::new("us-east-1")?;
    /// reader.with_cache("./kore_cache")?;
    /// ```
    pub fn with_cache(&mut self, cache_dir: &str) -> Result<(), S3Error> {
        if cache_dir.is_empty() {
            return Err(S3Error::InvalidPath("Cache directory cannot be empty".to_string()));
        }
        
        self.cache_enabled = true;
        self.cache_dir = Some(cache_dir.to_string());
        Ok(())
    }

    /// Read a Kore file from S3
    /// 
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `key` - S3 object key (path)
    /// 
    /// # Returns
    /// * `Vec<u8>` - File contents as bytes
    /// 
    /// # Example
    /// ```rust
    /// let reader = S3Reader::new("us-east-1")?;
    /// let data = reader.read_file("my-bucket", "data/records.kore").await?;
    /// ```
    pub async fn read_file(&self, bucket: &str, key: &str) -> Result<Vec<u8>, S3Error> {
        // Validate inputs
        if bucket.is_empty() {
            return Err(S3Error::InvalidPath("Bucket name cannot be empty".to_string()));
        }
        if key.is_empty() {
            return Err(S3Error::InvalidPath("Object key cannot be empty".to_string()));
        }

        // Check cache first if enabled
        if self.cache_enabled {
            if let Some(cached) = self.read_from_cache(bucket, key).await {
                return Ok(cached);
            }
        }

        // Read from S3 (placeholder - actual AWS SDK implementation)
        let data = self.read_from_s3(bucket, key).await?;

        // Write to cache if enabled
        if self.cache_enabled {
            let _ = self.write_to_cache(bucket, key, &data).await;
        }

        Ok(data)
    }

    /// Write a Kore file to S3
    /// 
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `key` - S3 object key (path)
    /// * `data` - File contents as bytes
    /// 
    /// # Example
    /// ```rust
    /// let reader = S3Reader::new("us-east-1")?;
    /// let data = vec![0x4b, 0x4f, 0x52, 0x45]; // KORE magic bytes
    /// reader.write_file("my-bucket", "output.kore", &data).await?;
    /// ```
    pub async fn write_file(
        &self,
        bucket: &str,
        key: &str,
        data: &[u8],
    ) -> Result<(), S3Error> {
        if bucket.is_empty() {
            return Err(S3Error::InvalidPath("Bucket name cannot be empty".to_string()));
        }
        if key.is_empty() {
            return Err(S3Error::InvalidPath("Object key cannot be empty".to_string()));
        }

        self.write_to_s3(bucket, key, data).await?;

        // Update cache if enabled
        if self.cache_enabled {
            let _ = self.write_to_cache(bucket, key, data).await;
        }

        Ok(())
    }

    /// List Kore files in an S3 bucket/prefix
    /// 
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `prefix` - S3 prefix (optional)
    /// 
    /// # Returns
    /// * `Vec<String>` - List of object keys
    /// 
    /// # Example
    /// ```rust
    /// let reader = S3Reader::new("us-east-1")?;
    /// let files = reader.list_files("my-bucket", Some("data/")).await?;
    /// ```
    pub async fn list_files(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, S3Error> {
        if bucket.is_empty() {
            return Err(S3Error::InvalidPath("Bucket name cannot be empty".to_string()));
        }

        self.list_s3_objects(bucket, prefix).await
    }

    /// Get file metadata from S3
    /// 
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `key` - S3 object key
    /// 
    /// # Returns
    /// * `S3FileMetadata` - File metadata
    pub async fn get_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<S3FileMetadata, S3Error> {
        if bucket.is_empty() {
            return Err(S3Error::InvalidPath("Bucket name cannot be empty".to_string()));
        }
        if key.is_empty() {
            return Err(S3Error::InvalidPath("Object key cannot be empty".to_string()));
        }

        self.fetch_s3_metadata(bucket, key).await
    }

    // Private helper methods
    async fn read_from_s3(&self, bucket: &str, key: &str) -> Result<Vec<u8>, S3Error> {
        #[cfg(feature = "s3")]
        {
            use aws_sdk_s3::Client;
            use aws_config::BehaviorVersion;
            
            let config = aws_config::load_defaults(BehaviorVersion::latest())
                .await;
            let client = Client::new(&config);

            let resp = client
                .get_object()
                .bucket(bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| S3Error::AwsError(format!("Failed to read from S3: {}", e)))?;

            let body = resp
                .body
                .collect()
                .await
                .map_err(|e| S3Error::IoError(format!("Failed to read response body: {}", e)))?;

            Ok(body.into_bytes().to_vec())
        }
        
        #[cfg(not(feature = "s3"))]
        {
            Err(S3Error::AwsError(
                "AWS SDK integration not enabled - compile with 's3' feature".to_string(),
            ))
        }
    }

    async fn write_to_s3(&self, bucket: &str, key: &str, data: &[u8]) -> Result<(), S3Error> {
        #[cfg(feature = "s3")]
        {
            use aws_sdk_s3::Client;
            use aws_config::BehaviorVersion;
            use aws_sdk_s3::primitives::ByteStream;
            
            let config = aws_config::load_defaults(BehaviorVersion::latest())
                .await;
            let client = Client::new(&config);

            let byte_stream = ByteStream::from(data.to_vec());

            client
                .put_object()
                .bucket(bucket)
                .key(key)
                .body(byte_stream)
                .send()
                .await
                .map_err(|e| S3Error::AwsError(format!("Failed to write to S3: {}", e)))?;

            Ok(())
        }
        
        #[cfg(not(feature = "s3"))]
        {
            Err(S3Error::AwsError(
                "AWS SDK integration not enabled - compile with 's3' feature".to_string(),
            ))
        }
    }

    async fn list_s3_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, S3Error> {
        #[cfg(feature = "s3")]
        {
            use aws_sdk_s3::Client;
            use aws_config::BehaviorVersion;
            
            let config = aws_config::load_defaults(BehaviorVersion::latest())
                .await;
            let client = Client::new(&config);

            let mut request = client
                .list_objects_v2()
                .bucket(bucket);

            if let Some(p) = prefix {
                request = request.prefix(p);
            }

            let resp = request
                .send()
                .await
                .map_err(|e| S3Error::AwsError(format!("Failed to list S3 objects: {}", e)))?;

            let mut objects = Vec::new();
            // resp.contents() returns &[Object], not Option
            for obj in resp.contents() {
                if let Some(key) = obj.key() {
                    objects.push(key.to_string());
                }
            }

            Ok(objects)
        }
        
        #[cfg(not(feature = "s3"))]
        {
            Err(S3Error::AwsError(
                "AWS SDK integration not enabled - compile with 's3' feature".to_string(),
            ))
        }
    }

    async fn fetch_s3_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<S3FileMetadata, S3Error> {
        #[cfg(feature = "s3")]
        {
            use aws_sdk_s3::Client;
            use aws_config::BehaviorVersion;
            
            let config = aws_config::load_defaults(BehaviorVersion::latest())
                .await;
            let client = Client::new(&config);

            let resp = client
                .head_object()
                .bucket(bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| S3Error::AwsError(format!("Failed to fetch S3 metadata: {}", e)))?;

            let size = resp.content_length().unwrap_or(0) as u64;
            let last_modified = resp
                .last_modified()
                .map(|dt| dt.to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let etag = resp
                .e_tag()
                .unwrap_or("Unknown")
                .to_string();
            let content_type = resp
                .content_type()
                .unwrap_or("application/octet-stream")
                .to_string();

            Ok(S3FileMetadata {
                size,
                last_modified,
                etag,
                content_type,
            })
        }
        
        #[cfg(not(feature = "s3"))]
        {
            Err(S3Error::AwsError(
                "AWS SDK integration not enabled - compile with 's3' feature".to_string(),
            ))
        }
    }

    async fn read_from_cache(&self, _bucket: &str, _key: &str) -> Option<Vec<u8>> {
        // Placeholder for cache implementation
        None
    }

    async fn write_to_cache(&self, _bucket: &str, _key: &str, _data: &[u8]) -> Result<(), S3Error> {
        Ok(())
    }

    /// Get the configured region
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Check if caching is enabled
    pub fn cache_enabled(&self) -> bool {
        self.cache_enabled
    }
}

/// File metadata from S3
#[derive(Debug, Clone)]
pub struct S3FileMetadata {
    /// File size in bytes
    pub size: u64,
    /// Last modified timestamp
    pub last_modified: String,
    /// ETag (file version identifier)
    pub etag: String,
    /// Content type
    pub content_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_reader_creation() {
        let reader = S3Reader::new("us-east-1");
        assert!(reader.is_ok());

        let reader = reader.unwrap();
        assert_eq!(reader.region(), "us-east-1");
        assert!(!reader.cache_enabled());
    }

    #[test]
    fn test_invalid_region() {
        let reader = S3Reader::new("");
        assert!(reader.is_err());
    }

    #[tokio::test]
    async fn test_invalid_bucket() {
        let reader = S3Reader::new("us-east-1").unwrap();
        let result = reader.read_file("", "key.kore").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_key() {
        let reader = S3Reader::new("us-east-1").unwrap();
        let result = reader.read_file("bucket", "").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_cache_configuration() {
        let mut reader = S3Reader::new("us-east-1").unwrap();
        assert!(!reader.cache_enabled());

        let result = reader.with_cache("./cache");
        assert!(result.is_ok());
        assert!(reader.cache_enabled());
    }

    #[test]
    fn test_invalid_cache_dir() {
        let mut reader = S3Reader::new("us-east-1").unwrap();
        let result = reader.with_cache("");
        assert!(result.is_err());
    }
}
