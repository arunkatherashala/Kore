//! Node.js/JavaScript bindings using NAPI
//! 
//! Exposes S3Reader, AzureBlobReader, and GcsReader to JavaScript
//! 
//! Build: cargo build --release --features napi --target x86_64-unknown-linux-gnu
//! Or use: npm install && npm run build
//! 
//! Usage in JavaScript:
//! ```javascript
//! const { S3Reader } = require('./index.node');
//! const reader = new S3Reader('us-east-1');
//! const data = await reader.readFile('bucket', 'key');
//! ```

#![cfg(feature = "napi")]

use napi::{
    bindgen_prelude::*,
    JsObject, JsString, JsBuffer, JsPromise,
};

#[cfg(feature = "s3")]
use crate::s3_reader::S3Reader;

#[cfg(feature = "azure")]
use crate::azure_reader::AzureBlobReader;

#[cfg(feature = "gcs")]
use crate::gcs_reader::GcsReader;

// ============================================================================
// S3Reader NAPI Binding
// ============================================================================

#[cfg(feature = "s3")]
#[napi]
pub struct JsS3Reader {
    inner: S3Reader,
}

#[cfg(feature = "s3")]
#[napi]
impl JsS3Reader {
    /// Create a new S3Reader
    #[napi(constructor)]
    pub fn new(region: String) -> napi::Result<Self> {
        let inner = S3Reader::new(&region)
            .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
        Ok(JsS3Reader { inner })
    }

    /// Enable local caching
    #[napi]
    pub fn with_cache(mut self, cache_dir: String) -> napi::Result<Self> {
        self.inner = self.inner.with_cache(cache_dir);
        Ok(self)
    }

    /// Read file from S3 (async)
    #[napi]
    pub async fn read_file(
        &self,
        bucket: String,
        key: String,
    ) -> napi::Result<Vec<u8>> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.read_file(&bucket, &key)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// Write file to S3 (async)
    #[napi]
    pub async fn write_file(
        &self,
        bucket: String,
        key: String,
        data: Vec<u8>,
    ) -> napi::Result<()> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.write_file(&bucket, &key, data)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// List objects in bucket (async)
    #[napi]
    pub async fn list_files(
        &self,
        bucket: String,
        prefix: Option<String>,
    ) -> napi::Result<Vec<String>> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.list_files(&bucket, prefix.as_deref())
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// Get object metadata (async)
    #[napi(object)]
    pub struct S3Metadata {
        pub size: u64,
        pub last_modified: String,
        pub etag: String,
        pub content_type: String,
    }

    #[napi]
    pub async fn get_metadata(
        &self,
        bucket: String,
        key: String,
    ) -> napi::Result<S3Metadata> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            let (size, modified, etag, content_type) = reader.get_metadata(&bucket, &key)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
            Ok(S3Metadata {
                size,
                last_modified: modified,
                etag,
                content_type,
            })
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }
}

// ============================================================================
// AzureBlobReader NAPI Binding
// ============================================================================

#[cfg(feature = "azure")]
#[napi]
pub struct JsAzureBlobReader {
    inner: AzureBlobReader,
}

#[cfg(feature = "azure")]
#[napi]
impl JsAzureBlobReader {
    /// Create a new AzureBlobReader
    #[napi(constructor)]
    pub fn new(storage_account: String, account_key: String) -> napi::Result<Self> {
        let inner = AzureBlobReader::new(&storage_account, &account_key)
            .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
        Ok(JsAzureBlobReader { inner })
    }

    /// Enable local caching
    #[napi]
    pub fn with_cache(mut self, cache_dir: String) -> napi::Result<Self> {
        self.inner = self.inner.with_cache(cache_dir);
        Ok(self)
    }

    /// Read blob from Azure (async)
    #[napi]
    pub async fn read_file(
        &self,
        container: String,
        blob_path: String,
    ) -> napi::Result<Vec<u8>> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.read_file(&container, &blob_path)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// Write blob to Azure (async)
    #[napi]
    pub async fn write_file(
        &self,
        container: String,
        blob_path: String,
        data: Vec<u8>,
    ) -> napi::Result<()> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.write_file(&container, &blob_path, data)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// List blobs in container (async)
    #[napi]
    pub async fn list_blobs(
        &self,
        container: String,
        prefix: Option<String>,
    ) -> napi::Result<Vec<String>> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.list_blobs(&container, prefix.as_deref())
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// Get blob metadata (async)
    #[napi(object)]
    pub struct AzureMetadata {
        pub size: u64,
        pub last_modified: String,
        pub etag: String,
        pub content_type: String,
    }

    #[napi]
    pub async fn get_metadata(
        &self,
        container: String,
        blob_path: String,
    ) -> napi::Result<AzureMetadata> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            let (size, modified, etag, content_type) = reader.get_metadata(&container, &blob_path)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
            Ok(AzureMetadata {
                size,
                last_modified: modified,
                etag,
                content_type,
            })
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }
}

// ============================================================================
// GcsReader NAPI Binding
// ============================================================================

#[cfg(feature = "gcs")]
#[napi]
pub struct JsGcsReader {
    inner: GcsReader,
}

#[cfg(feature = "gcs")]
#[napi]
impl JsGcsReader {
    /// Create a new GcsReader
    #[napi(constructor)]
    pub fn new(project_id: String) -> napi::Result<Self> {
        let inner = GcsReader::new(&project_id)
            .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
        Ok(JsGcsReader { inner })
    }

    /// Enable local caching
    #[napi]
    pub fn with_cache(mut self, cache_dir: String) -> napi::Result<Self> {
        self.inner = self.inner.with_cache(cache_dir);
        Ok(self)
    }

    /// Read object from GCS (async)
    #[napi]
    pub async fn read_file(
        &self,
        bucket: String,
        object_path: String,
    ) -> napi::Result<Vec<u8>> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.read_file(&bucket, &object_path)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// Write object to GCS (async)
    #[napi]
    pub async fn write_file(
        &self,
        bucket: String,
        object_path: String,
        data: Vec<u8>,
    ) -> napi::Result<()> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.write_file(&bucket, &object_path, data)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// List objects in bucket (async)
    #[napi]
    pub async fn list_objects(
        &self,
        bucket: String,
        prefix: Option<String>,
    ) -> napi::Result<Vec<String>> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            reader.list_objects(&bucket, prefix.as_deref())
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }

    /// Get object metadata (async)
    #[napi(object)]
    pub struct GcsMetadata {
        pub size: u64,
        pub updated: String,
        pub generation: String,
        pub content_type: String,
    }

    #[napi]
    pub async fn get_metadata(
        &self,
        bucket: String,
        object_path: String,
    ) -> napi::Result<GcsMetadata> {
        let reader = self.inner.clone();
        tokio::spawn(async move {
            let (size, updated, generation, content_type) = reader.get_metadata(&bucket, &object_path)
                .await
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
            Ok(GcsMetadata {
                size,
                updated,
                generation,
                content_type,
            })
        })
        .await
        .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?
    }
}
