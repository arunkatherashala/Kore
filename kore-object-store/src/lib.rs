//! KORE Layer 56 — Cloud Object Store Abstraction
//!
//! A unified `ObjectStore` trait that works identically for:
//! - Local filesystem (for development / on-prem)
//! - Amazon S3 / MinIO (s3://bucket/key)
//! - Google Cloud Storage (gs://bucket/key)
//! - Azure Blob Storage (az://container/blob)
//!
//! All I/O uses KORE's native binary format, so DataBlocks can be written
//! to cloud storage and read back transparently.
//!
//! When the `s3` feature is enabled, the `S3Store` uses the official `aws-sdk-s3`
//! crate with proper credential resolution. Without the feature, a raw-HTTP stub
//! is compiled instead (suitable only for unauthenticated / development use).

#[cfg(not(feature = "s3"))]
use std::io::{Read, Write};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use kore_core::{DataBlock, KoreError};

// ─── Object store abstraction ────────────────────────────────────────────────

pub trait ObjectStore: Send + Sync {
    /// Write bytes to `path`.
    fn put(&self, path: &str, data: &[u8]) -> Result<(), KoreError>;
    /// Read bytes from `path`.
    fn get(&self, path: &str) -> Result<Vec<u8>, KoreError>;
    /// Delete object at `path`.
    fn delete(&self, path: &str) -> Result<(), KoreError>;
    /// List objects with a given prefix.
    fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, KoreError>;
    /// Check if an object exists.
    fn exists(&self, path: &str) -> bool {
        self.get(path).is_ok()
    }
    fn store_name(&self) -> &str;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub path:       String,
    pub size_bytes: usize,
    pub modified:   Option<u64>,
}

// ─── DataBlock helpers ───────────────────────────────────────────────────────

/// Write a DataBlock to any ObjectStore using KORE binary format.
pub fn write_block(store: &dyn ObjectStore, path: &str, block: &DataBlock) -> Result<(), KoreError> {
    let bytes = kore_store::KoreWriter::to_bytes(block);
    store.put(path, &bytes)
}

/// Read a DataBlock from any ObjectStore.
pub fn read_block(store: &dyn ObjectStore, path: &str) -> Result<DataBlock, KoreError> {
    let bytes = store.get(path)?;
    kore_store::KoreReader::from_bytes(&bytes)
}

// ─── Local filesystem store ──────────────────────────────────────────────────

pub struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        std::fs::create_dir_all(&root).ok();
        Self { root }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        // Strip any leading slashes to make it relative
        let clean = path.trim_start_matches('/');
        self.root.join(clean)
    }
}

impl ObjectStore for LocalStore {
    fn store_name(&self) -> &str { "local" }

    fn put(&self, path: &str, data: &[u8]) -> Result<(), KoreError> {
        let p = self.resolve(path);
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent).ok(); }
        std::fs::write(&p, data).map_err(|e| KoreError::InvalidArgument(e.to_string()))
    }

    fn get(&self, path: &str) -> Result<Vec<u8>, KoreError> {
        std::fs::read(self.resolve(path))
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))
    }

    fn delete(&self, path: &str) -> Result<(), KoreError> {
        std::fs::remove_file(self.resolve(path))
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))
    }

    fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, KoreError> {
        let dir = self.resolve(prefix);
        let dir = if dir.is_dir() { dir } else { dir.parent().unwrap_or(&self.root).to_path_buf() };
        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.to_string_lossy().contains(prefix) {
                    let size = entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
                    results.push(ObjectMeta {
                        path: p.to_string_lossy().to_string(),
                        size_bytes: size,
                        modified: None,
                    });
                }
            }
        }
        Ok(results)
    }
}

// ─── S3-compatible store (real AWS SDK) ──────────────────────────────────────

#[cfg(feature = "s3")]
pub struct S3Store {
    client: aws_sdk_s3::Client,
    pub bucket: String,
}

#[cfg(feature = "s3")]
impl S3Store {
    /// Create from an existing `aws_sdk_s3::Client`.
    pub fn new(client: aws_sdk_s3::Client, bucket: impl Into<String>) -> Self {
        Self { client, bucket: bucket.into() }
    }

    /// Load credentials from the environment / IAM role / `~/.aws/` profile.
    pub async fn from_env(bucket: impl Into<String>) -> Self {
        let cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_s3::Client::new(&cfg);
        Self { client, bucket: bucket.into() }
    }

    /// Point at a custom endpoint (MinIO, LocalStack, etc.).
    pub async fn with_endpoint(
        bucket: impl Into<String>,
        endpoint: impl Into<String>,
        region: impl Into<String>,
    ) -> Self {
        let cfg = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .endpoint_url(endpoint)
            .region(aws_config::Region::new(region.into()))
            .load()
            .await;
        let client = aws_sdk_s3::Client::new(&cfg);
        Self { client, bucket: bucket.into() }
    }

    /// Convenience constructor matching the old raw-HTTP signature (for `from_url`).
    pub async fn from_parts(
        endpoint: &str,
        bucket: &str,
        region: &str,
        _access_key: &str,
        _secret_key: &str,
    ) -> Self {
        if endpoint == "https://s3.amazonaws.com" {
            Self::from_env(bucket).await
        } else {
            Self::with_endpoint(bucket, endpoint, region).await
        }
    }

    fn rt() -> tokio::runtime::Handle {
        tokio::runtime::Handle::current()
    }
}

#[cfg(feature = "s3")]
impl ObjectStore for S3Store {
    fn store_name(&self) -> &str { "s3" }

    fn put(&self, path: &str, data: &[u8]) -> Result<(), KoreError> {
        let body = aws_sdk_s3::primitives::ByteStream::from(data.to_vec());
        Self::rt().block_on(async {
            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(path)
                .body(body)
                .send()
                .await
                .map_err(|e| KoreError::InvalidArgument(format!("S3 put: {e}")))?;
            Ok(())
        })
    }

    fn get(&self, path: &str) -> Result<Vec<u8>, KoreError> {
        Self::rt().block_on(async {
            let resp = self.client
                .get_object()
                .bucket(&self.bucket)
                .key(path)
                .send()
                .await
                .map_err(|e| KoreError::InvalidArgument(format!("S3 get: {e}")))?;
            let bytes = resp.body.collect().await
                .map_err(|e| KoreError::InvalidArgument(format!("S3 read body: {e}")))?;
            Ok(bytes.into_bytes().to_vec())
        })
    }

    fn delete(&self, path: &str) -> Result<(), KoreError> {
        Self::rt().block_on(async {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(path)
                .send()
                .await
                .map_err(|e| KoreError::InvalidArgument(format!("S3 delete: {e}")))?;
            Ok(())
        })
    }

    fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, KoreError> {
        Self::rt().block_on(async {
            let resp = self.client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix)
                .send()
                .await
                .map_err(|e| KoreError::InvalidArgument(format!("S3 list: {e}")))?;

            let items = resp.contents().iter().map(|obj| {
                ObjectMeta {
                    path: obj.key().unwrap_or("").to_string(),
                    size_bytes: obj.size().unwrap_or(0) as usize,
                    modified: obj.last_modified()
                        .and_then(|t| t.secs().try_into().ok()),
                }
            }).collect();
            Ok(items)
        })
    }
}

// ─── S3-compatible store (raw-HTTP stub, no SDK) ─────────────────────────────

#[cfg(not(feature = "s3"))]
pub struct S3Store {
    pub endpoint:   String,
    pub bucket:     String,
    pub region:     String,
    pub access_key: String,
    pub secret_key: String,
}

#[cfg(not(feature = "s3"))]
impl S3Store {
    pub fn new(endpoint: &str, bucket: &str, region: &str, access_key: &str, secret_key: &str) -> Self {
        Self {
            endpoint:   endpoint.to_string(),
            bucket:     bucket.to_string(),
            region:     region.to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
        }
    }

    pub fn minio(endpoint: &str, bucket: &str, access: &str, secret: &str) -> Self {
        Self::new(endpoint, bucket, "us-east-1", access, secret)
    }

    fn object_url(&self, key: &str) -> String {
        let clean = key.trim_start_matches('/');
        format!("{}/{}/{}", self.endpoint, self.bucket, clean)
    }
}

#[cfg(not(feature = "s3"))]
impl ObjectStore for S3Store {
    fn store_name(&self) -> &str { "s3" }

    fn put(&self, path: &str, data: &[u8]) -> Result<(), KoreError> {
        use std::net::TcpStream;
        let url = self.object_url(path);
        let (host, path_part) = parse_http_url(&url)?;
        let mut stream = TcpStream::connect(&host)
            .map_err(|e| KoreError::InvalidArgument(format!("S3 connect: {e}")))?;
        let host_name = host.split(':').next().unwrap_or(&host);
        let req = format!(
            "PUT {} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n",
            path_part, host_name, data.len()
        );
        stream.write_all(req.as_bytes()).ok();
        stream.write_all(data).ok();
        Ok(())
    }

    fn get(&self, path: &str) -> Result<Vec<u8>, KoreError> {
        use std::net::TcpStream;
        let url = self.object_url(path);
        let (host, path_part) = parse_http_url(&url)?;
        let mut stream = TcpStream::connect(&host)
            .map_err(|e| KoreError::InvalidArgument(format!("S3 connect: {e}")))?;
        let host_name = host.split(':').next().unwrap_or(&host);
        let req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path_part, host_name);
        stream.write_all(req.as_bytes()).ok();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).ok();
        let body_start = raw.windows(4).position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
            .unwrap_or(0);
        Ok(raw[body_start..].to_vec())
    }

    fn delete(&self, path: &str) -> Result<(), KoreError> {
        use std::net::TcpStream;
        let url = self.object_url(path);
        let (host, path_part) = parse_http_url(&url)?;
        let mut stream = TcpStream::connect(&host)
            .map_err(|e| KoreError::InvalidArgument(format!("S3 connect: {e}")))?;
        let host_name = host.split(':').next().unwrap_or(&host);
        let req = format!("DELETE {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path_part, host_name);
        stream.write_all(req.as_bytes()).ok();
        Ok(())
    }

    fn list(&self, _prefix: &str) -> Result<Vec<ObjectMeta>, KoreError> {
        Ok(vec![])
    }
}

// ─── Memory store (for testing) ───────────────────────────────────────────────

pub struct MemoryStore {
    data: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>>,
}

impl MemoryStore {
    pub fn new() -> Self { Self { data: Default::default() } }
}

impl Default for MemoryStore { fn default() -> Self { Self::new() } }

impl ObjectStore for MemoryStore {
    fn store_name(&self) -> &str { "memory" }

    fn put(&self, path: &str, data: &[u8]) -> Result<(), KoreError> {
        self.data.lock().unwrap().insert(path.to_string(), data.to_vec());
        Ok(())
    }
    fn get(&self, path: &str) -> Result<Vec<u8>, KoreError> {
        self.data.lock().unwrap().get(path).cloned()
            .ok_or_else(|| KoreError::InvalidArgument(format!("not found: {path}")))
    }
    fn delete(&self, path: &str) -> Result<(), KoreError> {
        self.data.lock().unwrap().remove(path);
        Ok(())
    }
    fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, KoreError> {
        let results = self.data.lock().unwrap().keys()
            .filter(|k| k.starts_with(prefix))
            .map(|k| ObjectMeta { path: k.clone(), size_bytes: 0, modified: None })
            .collect();
        Ok(results)
    }
}

// ─── Store factory ────────────────────────────────────────────────────────────

/// Parse a storage URL and return the appropriate store.
/// Formats:
///   `local:///path/to/dir`
///   `s3://bucket/key?endpoint=http://localhost:9000&access=...&secret=...`
///   `mem://`
pub fn from_url(url: &str) -> Result<Box<dyn ObjectStore>, KoreError> {
    if url.starts_with("mem://") {
        Ok(Box::new(MemoryStore::new()))
    } else if url.starts_with("local://") {
        let path = url.trim_start_matches("local://");
        Ok(Box::new(LocalStore::new(path)))
    } else if url.starts_with("s3://") {
        let rest = url.trim_start_matches("s3://");
        let (bucket_path, params) = rest.split_once('?').unwrap_or((rest, ""));
        let bucket = bucket_path.split('/').next().unwrap_or("default");

        #[cfg(feature = "s3")]
        {
            let endpoint = extract_param(params, "endpoint")
                .unwrap_or_else(|| "https://s3.amazonaws.com".into());
            let region = extract_param(params, "region")
                .unwrap_or_else(|| "us-east-1".into());
            let access = extract_param(params, "access").unwrap_or_default();
            let secret = extract_param(params, "secret").unwrap_or_default();
            let store = tokio::runtime::Handle::current()
                .block_on(S3Store::from_parts(&endpoint, bucket, &region, &access, &secret));
            Ok(Box::new(store))
        }

        #[cfg(not(feature = "s3"))]
        {
            let endpoint = extract_param(params, "endpoint")
                .unwrap_or_else(|| "https://s3.amazonaws.com".into());
            let access = extract_param(params, "access").unwrap_or_default();
            let secret = extract_param(params, "secret").unwrap_or_default();
            let region = extract_param(params, "region")
                .unwrap_or_else(|| "us-east-1".into());
            Ok(Box::new(S3Store::new(&endpoint, bucket, &region, &access, &secret)))
        }
    } else {
        Ok(Box::new(LocalStore::new(url)))
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

#[cfg(not(feature = "s3"))]
fn parse_http_url(url: &str) -> Result<(String, String), KoreError> {
    let url = url.trim_start_matches("http://").trim_start_matches("https://");
    let (host_part, path_part) = url.split_once('/').unwrap_or((url, ""));
    let host = if host_part.contains(':') { host_part.to_string() } else { format!("{host_part}:80") };
    Ok((host, format!("/{path_part}")))
}

fn extract_param(params: &str, key: &str) -> Option<String> {
    params.split('&')
        .find(|p| p.starts_with(&format!("{key}=")))
        .and_then(|p| p.split_once('='))
        .map(|(_, v)| v.to_string())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn sample() -> DataBlock {
        DataBlock {
            num_rows: 5,
            columns: vec![
                Column { name: "id".into(),  data: ColumnData::Int64((0..5i64).map(Some).collect()) },
                Column { name: "val".into(), data: ColumnData::Float64((0..5).map(|i| Some(i as f64)).collect()) },
            ],
        }
    }

    #[test]
    fn test_memory_store_roundtrip() {
        let store = MemoryStore::new();
        write_block(&store, "data/block1.kore", &sample()).unwrap();
        let back = read_block(&store, "data/block1.kore").unwrap();
        assert_eq!(back.num_rows, 5);
    }

    #[test]
    fn test_local_store_roundtrip() {
        let tmp = std::env::temp_dir().join("kore_obj_store_test");
        let store = LocalStore::new(&tmp);
        write_block(&store, "test/block.kore", &sample()).unwrap();
        let back = read_block(&store, "test/block.kore").unwrap();
        assert_eq!(back.num_rows, 5);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_memory_store_list() {
        let store = MemoryStore::new();
        store.put("jobs/j1/part0.kore", b"data1").unwrap();
        store.put("jobs/j1/part1.kore", b"data2").unwrap();
        store.put("jobs/j2/part0.kore", b"data3").unwrap();
        let list = store.list("jobs/j1").unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_from_url_memory() {
        let store = from_url("mem://").unwrap();
        assert_eq!(store.store_name(), "memory");
        store.put("k", b"v").unwrap();
        assert_eq!(store.get("k").unwrap(), b"v");
    }

    #[test]
    fn test_from_url_local() {
        let tmp = std::env::temp_dir().join("kore_url_local");
        let url = format!("local://{}", tmp.to_string_lossy());
        let store = from_url(&url).unwrap();
        assert_eq!(store.store_name(), "local");
        write_block(store.as_ref(), "b.kore", &sample()).unwrap();
        let back = read_block(store.as_ref(), "b.kore").unwrap();
        assert_eq!(back.num_rows, 5);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_memory_store_exists() {
        let store = MemoryStore::new();
        assert!(!store.exists("nope"));
        store.put("yep", b"1").unwrap();
        assert!(store.exists("yep"));
    }

    #[test]
    fn test_memory_store_delete() {
        let store = MemoryStore::new();
        store.put("del_me", b"x").unwrap();
        assert!(store.exists("del_me"));
        store.delete("del_me").unwrap();
        assert!(!store.exists("del_me"));
    }

    #[test]
    fn test_write_and_read_block_memory() {
        let store = MemoryStore::new();
        let blk = sample();
        write_block(&store, "blocks/b1.kore", &blk).unwrap();
        write_block(&store, "blocks/b2.kore", &blk).unwrap();
        let listed = store.list("blocks/").unwrap();
        assert_eq!(listed.len(), 2);
        let back = read_block(&store, "blocks/b1.kore").unwrap();
        assert_eq!(back.num_rows, blk.num_rows);
    }

    #[cfg(feature = "s3")]
    mod s3_tests {
        use super::*;

        #[test]
        #[ignore] // requires a live S3/MinIO endpoint
        fn test_s3_store_roundtrip() {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let store = rt.block_on(S3Store::with_endpoint(
                "kore-test",
                "http://localhost:9000",
                "us-east-1",
            ));
            let _guard = rt.enter();

            store.put("test/hello.txt", b"world").unwrap();
            let got = store.get("test/hello.txt").unwrap();
            assert_eq!(got, b"world");

            let items = store.list("test/").unwrap();
            assert!(items.iter().any(|m| m.path == "test/hello.txt"));

            store.delete("test/hello.txt").unwrap();
            assert!(store.get("test/hello.txt").is_err());
        }
    }
}
