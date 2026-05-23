/// Multi-Cloud Storage Backends
/// Supports AWS S3, Azure Blob Storage, and Google Cloud Storage

#[cfg(feature = "azure")]
use azure_storage::StorageCredentials;
#[cfg(feature = "gcp")]
use google_cloud_storage::client::Client as GCSClient;

// ==================== AZURE BLOB STORAGE ====================

#[cfg(feature = "azure")]
pub struct AzureBlobStorageBackend {
    container_name: String,
    account_name: String,
    client: azure_storage_blobs::prelude::BlobClient,
}

#[cfg(feature = "azure")]
impl AzureBlobStorageBackend {
    pub async fn new(
        account_name: &str,
        account_key: &str,
        container_name: &str,
    ) -> Result<Self, StorageError> {
        use azure_storage::StorageCredentials;
        use azure_storage_blobs::prelude::*;

        let credentials = StorageCredentials::access_key(account_name, account_key);
        let blob_client = BlobContainerClient::new(account_name, container_name, credentials);

        Ok(AzureBlobStorageBackend {
            container_name: container_name.to_string(),
            account_name: account_name.to_string(),
            client: blob_client,
        })
    }

    pub async fn upload_blob(
        &self,
        blob_name: &str,
        data: &[u8],
    ) -> Result<StorageMetadata, StorageError> {
        let compressed = compress_data(data).map_err(|e| {
            StorageError::CompressionError(format!("Azure compression failed: {}", e))
        })?;

        self.client
            .blob_client(blob_name)
            .put_block_blob(compressed.clone())
            .await
            .map_err(|e| StorageError::UploadError(format!("Azure upload failed: {}", e)))?;

        Ok(StorageMetadata {
            file_id: blob_name.to_string(),
            filename: blob_name.to_string(),
            original_size: data.len() as u64,
            compressed_size: compressed.len() as u64,
            compression_ratio: compressed.len() as f64 / data.len() as f64,
            compression_method: "hybrid".to_string(),
            storage_backend: "azure".to_string(),
            storage_path: format!("https://{}.blob.core.windows.net/{}/{}", 
                self.account_name, self.container_name, blob_name),
            etag: Some(uuid::Uuid::new_v4().to_string()),
        })
    }
}

// ==================== GOOGLE CLOUD STORAGE ====================

#[cfg(feature = "gcp")]
pub struct GCPCloudStorageBackend {
    bucket_name: String,
    client: GCSClient,
}

#[cfg(feature = "gcp")]
impl GCPCloudStorageBackend {
    pub async fn new(bucket_name: &str, credentials_path: &str) -> Result<Self, StorageError> {
        let client = GCSClient::new(
            google_cloud_storage::client::ClientConfig::default()
                .with_credentials(
                    google_cloud_auth::credentials::CredentialsFile::new_from_file(
                        credentials_path,
                    )
                    .await
                    .map_err(|e| StorageError::ConfigError(format!("GCP auth failed: {}", e)))?,
                )
                .build()
                .await
                .map_err(|e| StorageError::ConfigError(format!("GCP client failed: {}", e)))?,
        );

        Ok(GCPCloudStorageBackend {
            bucket_name: bucket_name.to_string(),
            client,
        })
    }

    pub async fn upload_object(
        &self,
        object_name: &str,
        data: &[u8],
    ) -> Result<StorageMetadata, StorageError> {
        let compressed = compress_data(data).map_err(|e| {
            StorageError::CompressionError(format!("GCP compression failed: {}", e))
        })?;

        self.client
            .upload_object(
                &google_cloud_storage::http::objects::upload_object_req::UploadObjectRequest {
                    bucket: self.bucket_name.clone(),
                    ..Default::default()
                },
                compressed.clone(),
            )
            .await
            .map_err(|e| StorageError::UploadError(format!("GCP upload failed: {}", e)))?;

        Ok(StorageMetadata {
            file_id: object_name.to_string(),
            filename: object_name.to_string(),
            original_size: data.len() as u64,
            compressed_size: compressed.len() as u64,
            compression_ratio: compressed.len() as f64 / data.len() as f64,
            compression_method: "hybrid".to_string(),
            storage_backend: "gcp".to_string(),
            storage_path: format!(
                "https://storage.googleapis.com/{}/{}",
                self.bucket_name, object_name
            ),
            etag: Some(uuid::Uuid::new_v4().to_string()),
        })
    }
}

// ==================== UNIFIED CLOUD FACTORY ====================

pub enum CloudProvider {
    #[cfg(feature = "s3")]
    AWS,
    #[cfg(feature = "azure")]
    Azure,
    #[cfg(feature = "gcp")]
    GCP,
}

pub async fn create_cloud_storage(
    provider: CloudProvider,
) -> Result<Arc<dyn StorageBackend>, StorageError> {
    match provider {
        #[cfg(feature = "s3")]
        CloudProvider::AWS => {
            let config = S3StorageBackend::config_from_env()?;
            let backend = S3StorageBackend::new(&config).await?;
            Ok(Arc::new(backend))
        }
        #[cfg(feature = "azure")]
        CloudProvider::Azure => {
            let account_name = std::env::var("AZURE_STORAGE_ACCOUNT")
                .map_err(|_| StorageError::ConfigError("Missing AZURE_STORAGE_ACCOUNT".to_string()))?;
            let account_key = std::env::var("AZURE_STORAGE_KEY")
                .map_err(|_| StorageError::ConfigError("Missing AZURE_STORAGE_KEY".to_string()))?;
            let container = std::env::var("AZURE_STORAGE_CONTAINER")
                .unwrap_or_else(|_| "kore".to_string());

            let backend = AzureBlobStorageBackend::new(&account_name, &account_key, &container)
                .await?;
            Ok(Arc::new(backend))
        }
        #[cfg(feature = "gcp")]
        CloudProvider::GCP => {
            let bucket = std::env::var("GCP_BUCKET_NAME")
                .map_err(|_| StorageError::ConfigError("Missing GCP_BUCKET_NAME".to_string()))?;
            let creds_path = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
                .map_err(|_| StorageError::ConfigError("Missing GOOGLE_APPLICATION_CREDENTIALS".to_string()))?;

            let backend = GCPCloudStorageBackend::new(&bucket, &creds_path).await?;
            Ok(Arc::new(backend))
        }
    }
}

// ==================== MULTI-CLOUD CONFIG ====================

#[derive(Debug, Clone)]
pub struct MultiCloudConfig {
    pub primary_provider: String, // "aws", "azure", "gcp"
    pub failover_providers: Vec<String>,
    pub cross_region_replication: bool,
    pub data_residency: Option<String>,
}

impl MultiCloudConfig {
    pub fn from_env() -> Result<Self, StorageError> {
        Ok(MultiCloudConfig {
            primary_provider: std::env::var("CLOUD_PROVIDER").unwrap_or_else(|_| "aws".to_string()),
            failover_providers: std::env::var("FAILOVER_PROVIDERS")
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            cross_region_replication: std::env::var("CROSS_REGION_REPLICATION")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            data_residency: std::env::var("DATA_RESIDENCY").ok(),
        })
    }
}
