//! Azure Blob Storage Connector for Kore FileFormat
//! 
//! Provides native support for reading and writing Kore files directly from/to Azure Blob Storage.
//! 
//! # Features
//! 
//! To use this module, enable the `azure` feature in Cargo.toml:
//! ```toml
//! [dependencies]
//! kore_fileformat = { version = "1.0", features = ["azure"] }
//! ```
//! 
//! # Examples
//! 
//! ```ignore
//! use kore_fileformat::azure_reader::AzureBlobReader;
//! 
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut reader = AzureBlobReader::new(
//!         "storage_account",
//!         "account_key"
//!     )?;
//!     reader.with_cache("./cache")?;
//!     let data = reader.read_file("container", "path/to/file.kore").await?;
//!     Ok(())
//! }
//! ```

use std::error::Error;
use std::fmt;

#[cfg(feature = "azure")]
use futures_util::stream::StreamExt;
#[cfg(feature = "azure")]
use log;

/// Azure Blob Storage Reader Configuration and Operations
#[derive(Debug, Clone)]
pub struct AzureBlobReader {
    storage_account: String,
    account_key: String,
    cache_enabled: bool,
    cache_dir: Option<String>,
}

/// Error types for Azure operations
#[derive(Debug)]
pub enum AzureError {
    /// Azure SDK error
    AzureError(String),
    /// Invalid path format
    InvalidPath(String),
    /// Blob not found
    NotFound(String),
    /// Authentication failed
    AuthenticationError(String),
    /// IO error
    IoError(String),
}

impl fmt::Display for AzureError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AzureError::AzureError(e) => write!(f, "Azure Error: {}", e),
            AzureError::InvalidPath(p) => write!(f, "Invalid path: {}", p),
            AzureError::NotFound(p) => write!(f, "Blob not found: {}", p),
            AzureError::AuthenticationError(e) => write!(f, "Authentication failed: {}", e),
            AzureError::IoError(e) => write!(f, "IO Error: {}", e),
        }
    }
}

impl Error for AzureError {}

/// Metadata for a blob in Azure Storage
#[derive(Debug, Clone)]
pub struct AzureBlobMetadata {
    /// Blob size in bytes
    pub size: u64,
    /// Last modification time
    pub last_modified: String,
    /// Azure blob ETag
    pub etag: String,
    /// Content type
    pub content_type: Option<String>,
}

impl AzureBlobReader {
    /// Create reader for Azure Blob Storage
    ///
    /// # Arguments
    ///
    /// * `storage_account` - Azure storage account name
    /// * `account_key` - Storage account access key
    ///
    /// # Returns
    ///
    /// New AzureBlobReader instance
    ///
    /// # Errors
    ///
    /// Returns `AzureError::AuthenticationError` if credentials are invalid
    ///
    /// # Example
    ///
    /// ```ignore
    /// let reader = AzureBlobReader::new("myaccount", "mykey")?;
    /// ```
    pub fn new(storage_account: &str, account_key: &str) -> Result<Self, AzureError> {
        if storage_account.is_empty() {
            return Err(AzureError::InvalidPath(
                "Storage account name cannot be empty".to_string(),
            ));
        }
        if account_key.is_empty() {
            return Err(AzureError::AuthenticationError(
                "Account key cannot be empty".to_string(),
            ));
        }

        Ok(AzureBlobReader {
            storage_account: storage_account.to_string(),
            account_key: account_key.to_string(),
            cache_enabled: false,
            cache_dir: None,
        })
    }

    /// Enable local file caching
    ///
    /// # Example
    ///
    /// ```ignore
    /// reader.with_cache("./blob_cache")?;
    /// ```
    pub fn with_cache(&mut self, cache_dir: &str) -> Result<(), AzureError> {
        if cache_dir.is_empty() {
            return Err(AzureError::InvalidPath(
                "Cache directory cannot be empty".to_string(),
            ));
        }
        self.cache_dir = Some(cache_dir.to_string());
        self.cache_enabled = true;
        Ok(())
    }

    /// Read blob from Azure Storage
    ///
    /// Checks cache first if enabled. Downloads from Azure if not cached.
    ///
    /// # Arguments
    ///
    /// * `container` - Container name
    /// * `blob_path` - Path to blob within container
    ///
    /// # Returns
    ///
    /// Blob contents as bytes
    ///
    /// # Example
    ///
    /// ```ignore
    /// let data = reader.read_file("container", "data/file.kore").await?;
    /// ```
    pub async fn read_file(
        &self,
        container: &str,
        blob_path: &str,
    ) -> Result<Vec<u8>, AzureError> {
        self.validate_container_path(container, blob_path)?;

        // Check cache first
        if self.cache_enabled {
            if let Some(cached) = self.read_from_cache(container, blob_path).await? {
                return Ok(cached);
            }
        }

        // Read from Azure
        let data = self.read_from_azure(container, blob_path).await?;

        // Update cache
        if self.cache_enabled {
            let _ = self.write_to_cache(container, blob_path, &data).await;
        }

        Ok(data)
    }

    /// Write blob to Azure Storage
    pub async fn write_file(
        &self,
        container: &str,
        blob_path: &str,
        data: &[u8],
    ) -> Result<(), AzureError> {
        self.validate_container_path(container, blob_path)?;

        self.write_to_azure(container, blob_path, data).await?;

        if self.cache_enabled {
            let _ = self.write_to_cache(container, blob_path, data).await;
        }

        Ok(())
    }

    /// List blobs in container
    pub async fn list_blobs(
        &self,
        container: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, AzureError> {
        if container.is_empty() {
            return Err(AzureError::InvalidPath("Container name cannot be empty".to_string()));
        }

        self.list_azure_blobs(container, prefix).await
    }

    /// Get blob metadata
    pub async fn get_metadata(
        &self,
        container: &str,
        blob_path: &str,
    ) -> Result<AzureBlobMetadata, AzureError> {
        self.validate_container_path(container, blob_path)?;
        self.fetch_azure_metadata(container, blob_path).await
    }

    /// Get storage account name
    pub fn storage_account(&self) -> &str {
        &self.storage_account
    }

    /// Check if caching is enabled
    pub fn cache_enabled(&self) -> bool {
        self.cache_enabled
    }

    // Private helper methods

    fn validate_container_path(&self, container: &str, blob_path: &str) -> Result<(), AzureError> {
        if container.is_empty() {
            return Err(AzureError::InvalidPath("Container name cannot be empty".to_string()));
        }
        if blob_path.is_empty() {
            return Err(AzureError::InvalidPath("Blob path cannot be empty".to_string()));
        }
        Ok(())
    }

    async fn read_from_azure(&self, container: &str, blob_path: &str) -> Result<Vec<u8>, AzureError> {
        #[cfg(feature = "azure")]
        {
            use azure_storage::prelude::*;
            use azure_storage_blobs::prelude::*;
            
            // Get credentials from environment variables
            let account_key = std::env::var("AZURE_STORAGE_KEY")
                .or_else(|_| std::env::var("AZURE_STORAGE_ACCOUNT_KEY"))
                .map_err(|_| AzureError::AuthenticationError(
                    "AZURE_STORAGE_KEY or AZURE_STORAGE_ACCOUNT_KEY not set".to_string()
                ))?;
            
            let client = BlobServiceClient::new(
                &self.storage_account,
                StorageCredentials::access_key(&self.storage_account, &account_key),
            );
            
            let container_client = client.container_client(container);
            let blob_client = container_client.blob_client(blob_path);
            
            // Retry logic with exponential backoff
            let mut attempt = 0;
            const MAX_RETRIES: u32 = 3;
            
            loop {
                match blob_client.get().await {
                    Ok(response) => {
                        let bytes = response.blob.data.into();
                        log::info!("Successfully read {} bytes from {}/{}", 
                            std::mem::size_of_val(&bytes), container, blob_path);
                        return Ok(bytes);
                    }
                    Err(e) if attempt < MAX_RETRIES => {
                        attempt += 1;
                        let wait_time = std::time::Duration::from_millis(100 * 2_u64.pow(attempt));
                        log::warn!("Azure read attempt {} failed for {}/{}: {}. Retrying in {:?}...",
                            attempt, container, blob_path, e, wait_time);
                        tokio::time::sleep(wait_time).await;
                    }
                    Err(e) => {
                        return Err(AzureError::AzureError(
                            format!("Failed to read blob {}/{} after {} attempts: {}", 
                                container, blob_path, MAX_RETRIES, e)
                        ));
                    }
                }
            }
        }
        
        #[cfg(not(feature = "azure"))]
        {
            Err(AzureError::AzureError(
                "Azure SDK integration not enabled - compile with 'azure' feature".to_string()
            ))
        }
    }

    async fn write_to_azure(
        &self,
        container: &str,
        blob_path: &str,
        data: &[u8],
    ) -> Result<(), AzureError> {
        #[cfg(feature = "azure")]
        {
            use azure_storage::prelude::*;
            use azure_storage_blobs::prelude::*;
            
            // Get credentials
            let account_key = std::env::var("AZURE_STORAGE_KEY")
                .or_else(|_| std::env::var("AZURE_STORAGE_ACCOUNT_KEY"))
                .map_err(|_| AzureError::AuthenticationError(
                    "AZURE_STORAGE_KEY or AZURE_STORAGE_ACCOUNT_KEY not set".to_string()
                ))?;
            
            let client = BlobServiceClient::new(
                &self.storage_account,
                StorageCredentials::access_key(&self.storage_account, &account_key),
            );
            
            let container_client = client.container_client(container);
            let blob_client = container_client.blob_client(blob_path);
            
            // Upload with chunking for large files (4MB chunks)
            const CHUNK_SIZE: usize = 4 * 1024 * 1024;
            
            if data.len() <= CHUNK_SIZE {
                // Single upload for small files
                blob_client
                    .put_block_blob(data.to_vec())
                    .await
                    .map_err(|e| AzureError::AzureError(format!("Upload failed: {}", e)))?;
            } else {
                // Block blob upload for large files
                let mut block_ids = Vec::new();
                
                for (i, chunk) in data.chunks(CHUNK_SIZE).enumerate() {
                    let block_id = format!("{:08x}", i);
                    blob_client
                        .put_block(block_id.clone(), chunk.to_vec())
                        .await
                        .map_err(|e| AzureError::AzureError(format!("Put block error: {}", e)))?;
                    block_ids.push(block_id);
                }
                
                // Finalize the block blob
                blob_client
                    .put_block_list(block_ids)
                    .await
                    .map_err(|e| AzureError::AzureError(format!("Finalize error: {}", e)))?;
            }
            
            log::info!("Successfully wrote {} bytes to {}/{}", data.len(), container, blob_path);
            Ok(())
        }
        
        #[cfg(not(feature = "azure"))]
        {
            Err(AzureError::AzureError(
                "Azure SDK integration not enabled - compile with 'azure' feature".to_string(),
            ))
        }
    }

    async fn list_azure_blobs(
        &self,
        container: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, AzureError> {
        #[cfg(feature = "azure")]
        {
            use azure_storage::prelude::*;
            use azure_storage_blobs::prelude::*;
            
            // Get credentials
            let account_key = std::env::var("AZURE_STORAGE_KEY")
                .or_else(|_| std::env::var("AZURE_STORAGE_ACCOUNT_KEY"))
                .map_err(|_| AzureError::AuthenticationError(
                    "AZURE_STORAGE_KEY or AZURE_STORAGE_ACCOUNT_KEY not set".to_string()
                ))?;
            
            let client = BlobServiceClient::new(
                &self.storage_account,
                StorageCredentials::access_key(&self.storage_account, &account_key),
            );
            
            let container_client = client.container_client(container);
            let mut list_blobs = container_client.list_blobs();
            
            // Apply prefix filter if provided
            if let Some(p) = prefix {
                if !p.is_empty() {
                    list_blobs = list_blobs.prefix(p);
                }
            }
            
            let mut blob_names = Vec::new();
            
            // Paginate through results
            let mut stream = list_blobs.into_stream();
            while let Some(result) = stream.next().await {
                match result {
                    Ok(page) => {
                        for blob in page.blobs {
                            blob_names.push(blob.name);
                        }
                    }
                    Err(e) => {
                        return Err(AzureError::AzureError(
                            format!("List blobs error: {}", e)
                        ));
                    }
                }
            }
            
            log::info!("Listed {} blobs in container {}", blob_names.len(), container);
            Ok(blob_names)
        }
        
        #[cfg(not(feature = "azure"))]
        {
            Err(AzureError::AzureError(
                "Azure SDK integration not enabled - compile with 'azure' feature".to_string(),
            ))
        }
    }

    async fn fetch_azure_metadata(
        &self,
        container: &str,
        blob_path: &str,
    ) -> Result<AzureBlobMetadata, AzureError> {
        #[cfg(feature = "azure")]
        {
            use azure_storage::prelude::*;
            use azure_storage_blobs::prelude::*;
            
            // Get credentials
            let account_key = std::env::var("AZURE_STORAGE_KEY")
                .or_else(|_| std::env::var("AZURE_STORAGE_ACCOUNT_KEY"))
                .map_err(|_| AzureError::AuthenticationError(
                    "AZURE_STORAGE_KEY or AZURE_STORAGE_ACCOUNT_KEY not set".to_string()
                ))?;
            
            let client = BlobServiceClient::new(
                &self.storage_account,
                StorageCredentials::access_key(&self.storage_account, &account_key),
            );
            
            let container_client = client.container_client(container);
            let blob_client = container_client.blob_client(blob_path);
            
            match blob_client.get_properties().await {
                Ok(response) => {
                    Ok(AzureBlobMetadata {
                        size: response.blob.properties.content_length,
                        last_modified: response.blob.properties.last_modified.to_rfc3339(),
                        etag: response.blob.properties.etag.to_string(),
                        content_type: response.blob.properties.content_type.clone(),
                    })
                }
                Err(e) => {
                    Err(AzureError::AzureError(
                        format!("Metadata fetch error: {}", e)
                    ))
                }
            }
        }
        
        #[cfg(not(feature = "azure"))]
        {
            Err(AzureError::AzureError(
                "Azure SDK integration not enabled - compile with 'azure' feature".to_string(),
            ))
        }
    }

    async fn read_from_cache(
        &self,
        container: &str,
        blob_path: &str,
    ) -> Result<Option<Vec<u8>>, AzureError> {
        // TODO: Implement local caching
        Ok(None)
    }

    async fn write_to_cache(
        &self,
        container: &str,
        blob_path: &str,
        data: &[u8],
    ) -> Result<(), AzureError> {
        // TODO: Implement local caching
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid_credentials() {
        let reader = AzureBlobReader::new("myaccount", "mykey");
        assert!(reader.is_ok());
    }

    #[test]
    fn test_new_empty_account() {
        let result = AzureBlobReader::new("", "key");
        assert!(result.is_err());
    }

    #[test]
    fn test_new_empty_key() {
        let result = AzureBlobReader::new("account", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_with_cache() {
        let mut reader = AzureBlobReader::new("account", "key").unwrap();
        let result = reader.with_cache("./cache");
        assert!(result.is_ok());
        assert!(reader.cache_enabled());
    }

    #[test]
    fn test_with_cache_empty_path() {
        let mut reader = AzureBlobReader::new("account", "key").unwrap();
        let result = reader.with_cache("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_container_path() {
        let reader = AzureBlobReader::new("account", "key").unwrap();
        let result = reader.validate_container_path("container", "path");
        assert!(result.is_ok());

        let result = reader.validate_container_path("", "path");
        assert!(result.is_err());

        let result = reader.validate_container_path("container", "");
        assert!(result.is_err());
    }
}
