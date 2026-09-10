//! KORE Layer 75 — HDFS Connector (WebHDFS REST API)
//!
//! Provides an HDFS client using the WebHDFS REST API (HTTP-based),
//! requiring no native libhdfs dependency. Implements the `ObjectStore` trait
//! from `kore-object-store` for seamless integration with the KORE engine.
//!
//! All HTTP communication uses raw TCP + HTTP/1.1 to keep dependencies minimal.

use std::io::{Read, Write};
use std::net::TcpStream;
use serde::{Deserialize, Serialize};
use kore_core::{DataBlock, KoreError};
use kore_object_store::{ObjectMeta, ObjectStore};

// ─── Configuration ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdfsConfig {
    pub namenode_url: String,
    pub user: String,
    pub replication: u16,
    pub block_size: usize,
}

impl Default for HdfsConfig {
    fn default() -> Self {
        Self {
            namenode_url: "hdfs://namenode:9000".to_string(),
            user: whoami(),
            replication: 3,
            block_size: 128 * 1024 * 1024,
        }
    }
}

impl HdfsConfig {
    pub fn new(namenode_url: impl Into<String>) -> Self {
        Self {
            namenode_url: namenode_url.into(),
            ..Default::default()
        }
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    pub fn with_replication(mut self, replication: u16) -> Self {
        self.replication = replication;
        self
    }

    pub fn with_block_size(mut self, block_size: usize) -> Self {
        self.block_size = block_size;
        self
    }
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "hdfs".to_string())
}

// ─── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum HdfsError {
    ConnectionFailed(String),
    HttpError { status: u16, body: String },
    IoError(std::io::Error),
    ParseError(String),
    NotFound(String),
}

impl std::fmt::Display for HdfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "HDFS connection failed: {msg}"),
            Self::HttpError { status, body } => write!(f, "HDFS HTTP {status}: {body}"),
            Self::IoError(e) => write!(f, "HDFS I/O error: {e}"),
            Self::ParseError(msg) => write!(f, "HDFS parse error: {msg}"),
            Self::NotFound(path) => write!(f, "HDFS not found: {path}"),
        }
    }
}

impl std::error::Error for HdfsError {}

impl From<std::io::Error> for HdfsError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

impl From<HdfsError> for KoreError {
    fn from(e: HdfsError) -> Self {
        KoreError::InvalidArgument(e.to_string())
    }
}

// ─── FileStatus ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub length: u64,
    pub is_dir: bool,
    pub modification_time: u64,
    pub replication: u16,
    pub block_size: u64,
}

// ─── HDFS Client ─────────────────────────────────────────────────────────────

pub struct HdfsClient {
    config: HdfsConfig,
    base_url: String,
}

impl HdfsClient {
    pub fn new(config: HdfsConfig) -> Self {
        let base_url = webhdfs_base_url(&config.namenode_url);
        Self { config, base_url }
    }

    pub fn config(&self) -> &HdfsConfig {
        &self.config
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn op_url(&self, path: &str, op: &str) -> String {
        let clean_path = path.trim_start_matches('/');
        format!(
            "{}/{}?op={}&user.name={}",
            self.base_url, clean_path, op, self.config.user
        )
    }

    fn op_url_with_params(&self, path: &str, op: &str, extra: &str) -> String {
        let clean_path = path.trim_start_matches('/');
        format!(
            "{}/{}?op={}&user.name={}&{}",
            self.base_url, clean_path, op, self.config.user, extra
        )
    }

    /// Read a file from HDFS.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, HdfsError> {
        let url = self.op_url(path, "OPEN");
        let (host, req_path) = parse_url(&url)?;
        let response = http_get(&host, &req_path)?;
        if response.status >= 400 {
            return Err(HdfsError::HttpError {
                status: response.status,
                body: String::from_utf8_lossy(&response.body).to_string(),
            });
        }
        Ok(response.body)
    }

    /// Write data to a file in HDFS.
    pub fn write(&self, path: &str, data: &[u8]) -> Result<(), HdfsError> {
        let extra = format!(
            "overwrite=true&replication={}&blocksize={}",
            self.config.replication, self.config.block_size
        );
        let url = self.op_url_with_params(path, "CREATE", &extra);
        let (host, req_path) = parse_url(&url)?;
        let response = http_put(&host, &req_path, data)?;
        if response.status >= 400 {
            return Err(HdfsError::HttpError {
                status: response.status,
                body: String::from_utf8_lossy(&response.body).to_string(),
            });
        }
        Ok(())
    }

    /// Delete a file or directory from HDFS.
    pub fn delete(&self, path: &str) -> Result<(), HdfsError> {
        let url = self.op_url_with_params(path, "DELETE", "recursive=true");
        let (host, req_path) = parse_url(&url)?;
        let response = http_delete(&host, &req_path)?;
        if response.status >= 400 {
            return Err(HdfsError::HttpError {
                status: response.status,
                body: String::from_utf8_lossy(&response.body).to_string(),
            });
        }
        Ok(())
    }

    /// List files/directories at a path.
    pub fn list(&self, path: &str) -> Result<Vec<FileStatus>, HdfsError> {
        let url = self.op_url(path, "LISTSTATUS");
        let (host, req_path) = parse_url(&url)?;
        let response = http_get(&host, &req_path)?;
        if response.status >= 400 {
            return Err(HdfsError::HttpError {
                status: response.status,
                body: String::from_utf8_lossy(&response.body).to_string(),
            });
        }
        parse_list_status_response(&response.body, path)
    }

    /// Check if a file or directory exists.
    pub fn exists(&self, path: &str) -> Result<bool, HdfsError> {
        let url = self.op_url(path, "GETFILESTATUS");
        let (host, req_path) = parse_url(&url)?;
        let response = http_get(&host, &req_path)?;
        Ok(response.status < 400)
    }

    /// Create a directory (and parents).
    pub fn mkdir(&self, path: &str) -> Result<(), HdfsError> {
        let url = self.op_url(path, "MKDIRS");
        let (host, req_path) = parse_url(&url)?;
        let response = http_put(&host, &req_path, &[])?;
        if response.status >= 400 {
            return Err(HdfsError::HttpError {
                status: response.status,
                body: String::from_utf8_lossy(&response.body).to_string(),
            });
        }
        Ok(())
    }
}

// ─── HdfsStore (ObjectStore implementation) ──────────────────────────────────

pub struct HdfsStore {
    client: HdfsClient,
    root_path: String,
}

impl HdfsStore {
    pub fn new(client: HdfsClient, root_path: impl Into<String>) -> Self {
        let root_path = root_path.into();
        let root_path = root_path.trim_end_matches('/').to_string();
        Self { client, root_path }
    }

    fn full_path(&self, key: &str) -> String {
        let clean_key = key.trim_start_matches('/');
        format!("{}/{}", self.root_path, clean_key)
    }
}

impl ObjectStore for HdfsStore {
    fn store_name(&self) -> &str {
        "hdfs"
    }

    fn put(&self, path: &str, data: &[u8]) -> Result<(), KoreError> {
        let full = self.full_path(path);
        self.client.write(&full, data)?;
        Ok(())
    }

    fn get(&self, path: &str) -> Result<Vec<u8>, KoreError> {
        let full = self.full_path(path);
        let data = self.client.read(&full)?;
        Ok(data)
    }

    fn delete(&self, path: &str) -> Result<(), KoreError> {
        let full = self.full_path(path);
        self.client.delete(&full)?;
        Ok(())
    }

    fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, KoreError> {
        let full = self.full_path(prefix);
        let entries = self.client.list(&full)?;
        let metas = entries
            .into_iter()
            .filter(|e| !e.is_dir)
            .map(|e| ObjectMeta {
                path: e.path,
                size_bytes: e.length as usize,
                modified: Some(e.modification_time),
            })
            .collect();
        Ok(metas)
    }

    fn exists(&self, path: &str) -> bool {
        let full = self.full_path(path);
        self.client.exists(&full).unwrap_or(false)
    }
}

// ─── Parquet helpers ─────────────────────────────────────────────────────────

/// Read a KORE binary file from HDFS and deserialize into a DataBlock.
pub fn read_parquet_from_hdfs(client: &HdfsClient, path: &str) -> Result<DataBlock, KoreError> {
    let bytes = client.read(path)?;
    let mem = kore_object_store::MemoryStore::new();
    mem.put(path, &bytes)?;
    kore_object_store::read_block(&mem, path)
}

/// Write a DataBlock as KORE binary to HDFS.
pub fn write_parquet_to_hdfs(
    client: &HdfsClient,
    path: &str,
    block: &DataBlock,
) -> Result<(), KoreError> {
    let mem = kore_object_store::MemoryStore::new();
    kore_object_store::write_block(&mem, path, block)?;
    let bytes = mem.get(path)?;
    client.write(path, &bytes)?;
    Ok(())
}

// ─── Store factory ───────────────────────────────────────────────────────────

/// Create an HdfsStore from a URL of the form:
/// `hdfs://namenode:9000/root/path?user=hdfs&replication=3`
pub fn from_url(url: &str) -> Result<HdfsStore, KoreError> {
    if !url.starts_with("hdfs://") {
        return Err(KoreError::InvalidArgument(format!(
            "Not an HDFS URL: {url}"
        )));
    }

    let rest = url.trim_start_matches("hdfs://");
    let (host_and_path, params) = rest.split_once('?').unwrap_or((rest, ""));

    let (host_port, root_path) = host_and_path
        .split_once('/')
        .unwrap_or((host_and_path, ""));

    let user = extract_param(params, "user").unwrap_or_else(whoami);
    let replication: u16 = extract_param(params, "replication")
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let block_size: usize = extract_param(params, "blocksize")
        .and_then(|s| s.parse().ok())
        .unwrap_or(128 * 1024 * 1024);

    let config = HdfsConfig {
        namenode_url: format!("hdfs://{host_port}"),
        user,
        replication,
        block_size,
    };

    let client = HdfsClient::new(config);
    Ok(HdfsStore::new(client, format!("/{root_path}")))
}

// ─── HTTP primitives (raw TCP, no external deps) ─────────────────────────────

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn http_get(host: &str, path: &str) -> Result<HttpResponse, HdfsError> {
    let mut stream = TcpStream::connect(host)
        .map_err(|e| HdfsError::ConnectionFailed(format!("{host}: {e}")))?;

    let host_name = host.split(':').next().unwrap_or(host);
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_name}\r\nConnection: close\r\nAccept: */*\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    read_http_response(&mut stream)
}

fn http_put(host: &str, path: &str, body: &[u8]) -> Result<HttpResponse, HdfsError> {
    let mut stream = TcpStream::connect(host)
        .map_err(|e| HdfsError::ConnectionFailed(format!("{host}: {e}")))?;

    let host_name = host.split(':').next().unwrap_or(host);
    let request = format!(
        "PUT {path} HTTP/1.1\r\nHost: {host_name}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()?;

    read_http_response(&mut stream)
}

fn http_delete(host: &str, path: &str) -> Result<HttpResponse, HdfsError> {
    let mut stream = TcpStream::connect(host)
        .map_err(|e| HdfsError::ConnectionFailed(format!("{host}: {e}")))?;

    let host_name = host.split(':').next().unwrap_or(host);
    let request = format!(
        "DELETE {path} HTTP/1.1\r\nHost: {host_name}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    read_http_response(&mut stream)
}

fn read_http_response(stream: &mut TcpStream) -> Result<HttpResponse, HdfsError> {
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;

    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());

    let header_str = String::from_utf8_lossy(&raw[..header_end]);
    let status = parse_status_code(&header_str);

    let body_start = if header_end + 4 <= raw.len() {
        header_end + 4
    } else {
        raw.len()
    };
    let body = raw[body_start..].to_vec();

    Ok(HttpResponse { status, body })
}

fn parse_status_code(header: &str) -> u16 {
    header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(500)
}

// ─── URL / JSON parsing helpers ──────────────────────────────────────────────

fn webhdfs_base_url(namenode_url: &str) -> String {
    let stripped = namenode_url
        .trim_start_matches("hdfs://")
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let host = stripped.split(':').next().unwrap_or("localhost");
    let port = if stripped.contains(":9870") || stripped.contains(":50070") {
        stripped
            .split(':')
            .nth(1)
            .and_then(|p| p.split('/').next())
            .unwrap_or("9870")
    } else {
        "9870"
    };

    format!("http://{host}:{port}/webhdfs/v1")
}

fn parse_url(url: &str) -> Result<(String, String), HdfsError> {
    let stripped = url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let (host_part, path_part) = stripped.split_once('/').unwrap_or((stripped, ""));
    let host = if host_part.contains(':') {
        host_part.to_string()
    } else {
        format!("{host_part}:80")
    };
    Ok((host, format!("/{path_part}")))
}

fn parse_list_status_response(body: &[u8], parent_path: &str) -> Result<Vec<FileStatus>, HdfsError> {
    let text = String::from_utf8_lossy(body);
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| HdfsError::ParseError(format!("JSON parse: {e}")))?;

    let statuses = json
        .pointer("/FileStatuses/FileStatus")
        .and_then(|v| v.as_array())
        .ok_or_else(|| HdfsError::ParseError("Missing FileStatuses.FileStatus array".into()))?;

    let parent = parent_path.trim_end_matches('/');
    let results = statuses
        .iter()
        .map(|entry| {
            let suffix = entry
                .get("pathSuffix")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            FileStatus {
                path: format!("{parent}/{suffix}"),
                length: entry.get("length").and_then(|v| v.as_u64()).unwrap_or(0),
                is_dir: entry
                    .get("type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == "DIRECTORY")
                    .unwrap_or(false),
                modification_time: entry
                    .get("modificationTime")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                replication: entry
                    .get("replication")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16,
                block_size: entry
                    .get("blockSize")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            }
        })
        .collect();

    Ok(results)
}

fn extract_param(params: &str, key: &str) -> Option<String> {
    params
        .split('&')
        .find(|p| p.starts_with(&format!("{key}=")))
        .and_then(|p| p.split_once('='))
        .map(|(_, v)| v.to_string())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HdfsConfig::default();
        assert_eq!(config.namenode_url, "hdfs://namenode:9000");
        assert_eq!(config.replication, 3);
        assert_eq!(config.block_size, 128 * 1024 * 1024);
    }

    #[test]
    fn test_config_builder() {
        let config = HdfsConfig::new("hdfs://mynode:9000")
            .with_user("testuser")
            .with_replication(2)
            .with_block_size(64 * 1024 * 1024);

        assert_eq!(config.namenode_url, "hdfs://mynode:9000");
        assert_eq!(config.user, "testuser");
        assert_eq!(config.replication, 2);
        assert_eq!(config.block_size, 64 * 1024 * 1024);
    }

    #[test]
    fn test_webhdfs_base_url_conversion() {
        assert_eq!(
            webhdfs_base_url("hdfs://namenode:9000"),
            "http://namenode:9870/webhdfs/v1"
        );
        assert_eq!(
            webhdfs_base_url("hdfs://myhost:9000"),
            "http://myhost:9870/webhdfs/v1"
        );
        assert_eq!(
            webhdfs_base_url("hdfs://node1:9870"),
            "http://node1:9870/webhdfs/v1"
        );
        assert_eq!(
            webhdfs_base_url("hdfs://node2:50070"),
            "http://node2:50070/webhdfs/v1"
        );
    }

    #[test]
    fn test_op_url_construction() {
        let config = HdfsConfig::new("hdfs://namenode:9000").with_user("alice");
        let client = HdfsClient::new(config);

        let url = client.op_url("/data/file.txt", "OPEN");
        assert_eq!(url, "http://namenode:9870/webhdfs/v1/data/file.txt?op=OPEN&user.name=alice");

        let url = client.op_url("data/file.txt", "OPEN");
        assert_eq!(url, "http://namenode:9870/webhdfs/v1/data/file.txt?op=OPEN&user.name=alice");
    }

    #[test]
    fn test_op_url_with_params() {
        let config = HdfsConfig::new("hdfs://namenode:9000").with_user("bob");
        let client = HdfsClient::new(config);

        let url = client.op_url_with_params("/tmp/out.parquet", "CREATE", "overwrite=true&replication=2");
        assert_eq!(
            url,
            "http://namenode:9870/webhdfs/v1/tmp/out.parquet?op=CREATE&user.name=bob&overwrite=true&replication=2"
        );
    }

    #[test]
    fn test_op_url_delete() {
        let config = HdfsConfig::new("hdfs://nn:9000").with_user("hdfs");
        let client = HdfsClient::new(config);

        let url = client.op_url_with_params("/old/data", "DELETE", "recursive=true");
        assert!(url.contains("op=DELETE"));
        assert!(url.contains("recursive=true"));
        assert!(url.contains("user.name=hdfs"));
    }

    #[test]
    fn test_op_url_liststatus() {
        let config = HdfsConfig::new("hdfs://nn:9000").with_user("hdfs");
        let client = HdfsClient::new(config);

        let url = client.op_url("/data/warehouse", "LISTSTATUS");
        assert!(url.contains("op=LISTSTATUS"));
        assert!(url.contains("/data/warehouse"));
    }

    #[test]
    fn test_op_url_mkdirs() {
        let config = HdfsConfig::new("hdfs://nn:9000").with_user("admin");
        let client = HdfsClient::new(config);

        let url = client.op_url("/new/directory/path", "MKDIRS");
        assert!(url.contains("op=MKDIRS"));
        assert!(url.contains("user.name=admin"));
        assert!(url.contains("/new/directory/path"));
    }

    #[test]
    fn test_parse_list_status_json() {
        let json = br#"{
            "FileStatuses": {
                "FileStatus": [
                    {
                        "pathSuffix": "part-00000.parquet",
                        "type": "FILE",
                        "length": 1048576,
                        "modificationTime": 1700000000000,
                        "replication": 3,
                        "blockSize": 134217728
                    },
                    {
                        "pathSuffix": "part-00001.parquet",
                        "type": "FILE",
                        "length": 2097152,
                        "modificationTime": 1700000001000,
                        "replication": 3,
                        "blockSize": 134217728
                    },
                    {
                        "pathSuffix": "_metadata",
                        "type": "DIRECTORY",
                        "length": 0,
                        "modificationTime": 1700000002000,
                        "replication": 0,
                        "blockSize": 0
                    }
                ]
            }
        }"#;

        let entries = parse_list_status_response(json, "/data/table").unwrap();
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].path, "/data/table/part-00000.parquet");
        assert_eq!(entries[0].length, 1048576);
        assert!(!entries[0].is_dir);
        assert_eq!(entries[0].replication, 3);
        assert_eq!(entries[0].block_size, 134217728);

        assert_eq!(entries[1].path, "/data/table/part-00001.parquet");
        assert_eq!(entries[1].length, 2097152);

        assert_eq!(entries[2].path, "/data/table/_metadata");
        assert!(entries[2].is_dir);
        assert_eq!(entries[2].replication, 0);
    }

    #[test]
    fn test_parse_list_status_empty() {
        let json = br#"{"FileStatuses": {"FileStatus": []}}"#;
        let entries = parse_list_status_response(json, "/empty").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_list_status_invalid_json() {
        let bad = b"not json at all";
        let result = parse_list_status_response(bad, "/x");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_status_code() {
        assert_eq!(parse_status_code("HTTP/1.1 200 OK"), 200);
        assert_eq!(parse_status_code("HTTP/1.1 404 Not Found"), 404);
        assert_eq!(parse_status_code("HTTP/1.1 201 Created"), 201);
        assert_eq!(parse_status_code("HTTP/1.1 500 Internal Server Error"), 500);
        assert_eq!(parse_status_code("garbage"), 500);
    }

    #[test]
    fn test_parse_url() {
        let (host, path) = parse_url("http://namenode:9870/webhdfs/v1/data?op=OPEN").unwrap();
        assert_eq!(host, "namenode:9870");
        assert_eq!(path, "/webhdfs/v1/data?op=OPEN");
    }

    #[test]
    fn test_parse_url_no_port() {
        let (host, path) = parse_url("http://myhost/path/to/file").unwrap();
        assert_eq!(host, "myhost:80");
        assert_eq!(path, "/path/to/file");
    }

    #[test]
    fn test_hdfs_store_full_path() {
        let config = HdfsConfig::new("hdfs://nn:9000").with_user("test");
        let client = HdfsClient::new(config);
        let store = HdfsStore::new(client, "/warehouse/data");

        assert_eq!(store.full_path("table/part0.parquet"), "/warehouse/data/table/part0.parquet");
        assert_eq!(store.full_path("/table/part0.parquet"), "/warehouse/data/table/part0.parquet");
    }

    #[test]
    fn test_hdfs_store_name() {
        let config = HdfsConfig::new("hdfs://nn:9000").with_user("test");
        let client = HdfsClient::new(config);
        let store = HdfsStore::new(client, "/data");
        assert_eq!(store.store_name(), "hdfs");
    }

    #[test]
    fn test_from_url_basic() {
        let store = from_url("hdfs://namenode:9000/warehouse/tables?user=alice&replication=2").unwrap();
        assert_eq!(store.client.config().user, "alice");
        assert_eq!(store.client.config().replication, 2);
        assert_eq!(store.root_path, "/warehouse/tables");
        assert_eq!(store.client.base_url(), "http://namenode:9870/webhdfs/v1");
    }

    #[test]
    fn test_from_url_defaults() {
        let store = from_url("hdfs://myhost:9000/data").unwrap();
        assert_eq!(store.client.config().replication, 3);
        assert_eq!(store.client.config().block_size, 128 * 1024 * 1024);
        assert_eq!(store.root_path, "/data");
    }

    #[test]
    fn test_from_url_invalid_scheme() {
        let result = from_url("s3://bucket/key");
        assert!(result.is_err());
    }

    #[test]
    fn test_error_display() {
        let e = HdfsError::ConnectionFailed("timeout".into());
        assert!(e.to_string().contains("connection failed"));
        assert!(e.to_string().contains("timeout"));

        let e = HdfsError::NotFound("/missing/file".into());
        assert!(e.to_string().contains("not found"));

        let e = HdfsError::HttpError { status: 403, body: "Forbidden".into() };
        assert!(e.to_string().contains("403"));
    }

    #[test]
    fn test_extract_param() {
        assert_eq!(extract_param("user=alice&replication=2", "user"), Some("alice".into()));
        assert_eq!(extract_param("user=alice&replication=2", "replication"), Some("2".into()));
        assert_eq!(extract_param("user=alice&replication=2", "missing"), None);
        assert_eq!(extract_param("", "key"), None);
    }

    #[test]
    fn test_hdfs_error_to_kore_error() {
        let hdfs_err = HdfsError::NotFound("/gone".into());
        let kore_err: KoreError = hdfs_err.into();
        match kore_err {
            KoreError::InvalidArgument(msg) => assert!(msg.contains("not found")),
            _ => panic!("Expected InvalidArgument"),
        }
    }

    #[test]
    fn test_file_status_serde() {
        let fs = FileStatus {
            path: "/data/file.parquet".to_string(),
            length: 4096,
            is_dir: false,
            modification_time: 1700000000,
            replication: 3,
            block_size: 134217728,
        };
        let json = serde_json::to_string(&fs).unwrap();
        let back: FileStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.path, "/data/file.parquet");
        assert_eq!(back.length, 4096);
        assert!(!back.is_dir);
    }
}
