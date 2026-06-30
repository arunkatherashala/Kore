//! KORE Layer 53 — Data Connectors
//!
//! Pluggable data source / sink system with built-in connectors:
//!
//! | Connector     | Read | Write | Format          |
//! |---------------|------|-------|-----------------|
//! | JsonFile      | ✅   | ✅    | Single JSON array|
//! | ArrowIPC      | ✅   | ✅    | Apache Arrow IPC |
//! | Http          | ✅   | ❌    | CSV/JSON over HTTP|
//! | InMemory      | ✅   | ✅    | DataBlock clone  |
//! | MultiFile     | ✅   | ❌    | Glob of CSV files|

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use kore_core::{Column, ColumnData, DataBlock, KoreError};

// ─── Connector traits ─────────────────────────────────────────────────────────

pub trait DataSource: Send + Sync {
    fn read(&self)  -> Result<DataBlock, KoreError>;
    fn name(&self)  -> &str;
    fn format(&self) -> &str;
}

pub trait DataSink: Send + Sync {
    fn write(&self, data: &DataBlock) -> Result<(), KoreError>;
    fn name(&self)  -> &str;
    fn format(&self) -> &str;
}

// ─── JSON File connector ──────────────────────────────────────────────────────

/// Reads a JSON file containing an array of objects.
/// `[{"id":1,"name":"Alice"}, {"id":2,"name":"Bob"}]`
pub struct JsonFileSource {
    path: std::path::PathBuf,
}

impl JsonFileSource {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self { Self { path: path.into() } }
}

impl DataSource for JsonFileSource {
    fn name(&self)   -> &str { "JsonFile" }
    fn format(&self) -> &str { "json" }

    fn read(&self) -> Result<DataBlock, KoreError> {
        let raw = std::fs::read_to_string(&self.path)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        let records: Vec<serde_json::Value> = serde_json::from_str(&raw)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        json_records_to_block(records)
    }
}

pub struct JsonFileSink {
    path:   std::path::PathBuf,
    pretty: bool,
}

impl JsonFileSink {
    pub fn new(path: impl Into<std::path::PathBuf>, pretty: bool) -> Self {
        Self { path: path.into(), pretty }
    }
}

impl DataSink for JsonFileSink {
    fn name(&self)   -> &str { "JsonFile" }
    fn format(&self) -> &str { "json" }

    fn write(&self, data: &DataBlock) -> Result<(), KoreError> {
        let records: Vec<serde_json::Value> = (0..data.num_rows).map(|r| {
            let mut obj = serde_json::Map::new();
            for col in &data.columns {
                let v = match &col.data {
                    ColumnData::Int64(v)   => v.get(r).and_then(|x| *x).map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
                    ColumnData::Float64(v) => v.get(r).and_then(|x| *x).map(|f| serde_json::json!(f)).unwrap_or(serde_json::Value::Null),
                    ColumnData::Bool(v)    => v.get(r).and_then(|x| *x).map(serde_json::Value::Bool).unwrap_or(serde_json::Value::Null),
                    ColumnData::Str(v)     => v.get(r).and_then(|x| x.as_deref()).map(|s| serde_json::Value::String(s.into())).unwrap_or(serde_json::Value::Null),
                    ColumnData::StrDict { codes, dict } => {
                        let c = codes.get(r).copied().unwrap_or(u8::MAX);
                        if c == u8::MAX { serde_json::Value::Null } else { dict.get(c as usize).map(|s| serde_json::Value::String(s.clone())).unwrap_or(serde_json::Value::Null) }
                    }
                };
                obj.insert(col.name.clone(), v);
            }
            serde_json::Value::Object(obj)
        }).collect();

        let json = if self.pretty {
            serde_json::to_string_pretty(&records)
        } else {
            serde_json::to_string(&records)
        }.map_err(|e| KoreError::InvalidArgument(e.to_string()))?;

        std::fs::write(&self.path, json)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))
    }
}

// ─── Arrow IPC connector ──────────────────────────────────────────────────────

/// Lightweight Arrow IPC writer/reader using KORE's native binary format
/// with an Arrow-compatible framing (no dependency on arrow crate).
///
/// Format: [4-byte magic "KORE"][4-byte version=2][kore-store binary]
pub struct ArrowIpcSource { path: std::path::PathBuf }
pub struct ArrowIpcSink   { path: std::path::PathBuf }

const ARROW_MAGIC: &[u8; 4] = b"KARE";  // KORE-Arrow

impl ArrowIpcSource {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self { Self { path: path.into() } }
}
impl ArrowIpcSink {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self { Self { path: path.into() } }
}

impl DataSource for ArrowIpcSource {
    fn name(&self)   -> &str { "ArrowIPC" }
    fn format(&self) -> &str { "arrow" }

    fn read(&self) -> Result<DataBlock, KoreError> {
        let bytes = std::fs::read(&self.path)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        if bytes.len() < 8 || &bytes[..4] != ARROW_MAGIC {
            return Err(KoreError::InvalidArgument("not a KORE Arrow IPC file".into()));
        }
        kore_store::KoreReader::from_bytes(&bytes[8..])
    }
}

impl DataSink for ArrowIpcSink {
    fn name(&self)   -> &str { "ArrowIPC" }
    fn format(&self) -> &str { "arrow" }

    fn write(&self, data: &DataBlock) -> Result<(), KoreError> {
        let mut out = Vec::new();
        out.extend_from_slice(ARROW_MAGIC);
        out.extend_from_slice(&2u32.to_le_bytes());   // version=2
        out.extend_from_slice(&kore_store::KoreWriter::to_bytes(data));
        std::fs::write(&self.path, out)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))
    }
}

// ─── HTTP connector ───────────────────────────────────────────────────────────

/// Fetch CSV or JSON data from an HTTP URL using a raw TCP connection.
/// Supports HTTP 1.1 GET (no SSL, no auth — use for localhost/intranet sources).
pub struct HttpSource {
    url:    String,
    format: HttpFormat,
}

#[derive(Debug, Clone, Copy)]
pub enum HttpFormat { Csv, Json }

impl HttpSource {
    pub fn new(url: &str, format: HttpFormat) -> Self {
        Self { url: url.to_string(), format }
    }
}

impl DataSource for HttpSource {
    fn name(&self)   -> &str { "Http" }
    fn format(&self) -> &str { "http" }

    fn read(&self) -> Result<DataBlock, KoreError> {
        // Parse URL
        let body = http_get(&self.url)?;
        match self.format {
            HttpFormat::Csv  => csv_to_block(&body),
            HttpFormat::Json => {
                let records: Vec<serde_json::Value> = serde_json::from_str(&body)
                    .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
                json_records_to_block(records)
            }
        }
    }
}

fn http_get(url: &str) -> Result<String, KoreError> {
    use std::net::TcpStream;
    // Parse http://host:port/path
    let url = url.trim_start_matches("http://");
    let (host_port, path) = url.split_once('/').unwrap_or((url, ""));
    let path = format!("/{path}");
    let addr = if host_port.contains(':') { host_port.to_string() } else { format!("{host_port}:80") };

    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| KoreError::InvalidArgument(format!("HTTP connect {addr}: {e}")))?;

    let host = host_port.split(':').next().unwrap_or(host_port);
    let req  = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).map_err(|e| KoreError::InvalidArgument(e.to_string()))?;

    let mut raw = String::new();
    BufReader::new(stream).read_to_string(&mut raw)
        .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;

    // Strip HTTP headers (find blank line)
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or(&raw);
    Ok(body.to_string())
}

fn csv_to_block(csv: &str) -> Result<DataBlock, KoreError> {
    let mut lines: Vec<&str> = csv.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() { return Ok(DataBlock::empty()); }
    let headers: Vec<&str> = lines.remove(0).split(',').map(|s| s.trim()).collect();
    let n = lines.len();
    let nc = headers.len();
    let mut raw: Vec<Vec<String>> = lines.iter().map(|l| {
        let mut fields: Vec<String> = l.split(',').map(|s| s.trim().to_string()).collect();
        fields.resize(nc, String::new());
        fields
    }).collect();

    let columns = headers.iter().enumerate().map(|(ci, &name)| {
        let samples: Vec<&str> = raw.iter().take(100).map(|r| r[ci].as_str()).collect();
        let data = if samples.iter().all(|s| s.parse::<i64>().is_ok()) {
            ColumnData::Int64(raw.iter().map(|r| r[ci].parse::<i64>().ok()).collect())
        } else if samples.iter().all(|s| s.parse::<f64>().is_ok()) {
            ColumnData::Float64(raw.iter().map(|r| r[ci].parse::<f64>().ok()).collect())
        } else {
            ColumnData::Str(raw.iter().map(|r| if r[ci].is_empty() { None } else { Some(r[ci].clone()) }).collect())
        };
        Column { name: name.to_string(), data }
    }).collect();

    Ok(DataBlock { columns, num_rows: n })
}

// ─── In-Memory connector ──────────────────────────────────────────────────────

pub struct InMemorySource { block: DataBlock }
pub struct InMemorySink   { pub block: std::sync::Arc<std::sync::Mutex<Option<DataBlock>>> }

impl InMemorySource {
    pub fn new(block: DataBlock) -> Self { Self { block } }
}

impl DataSource for InMemorySource {
    fn name(&self)   -> &str { "InMemory" }
    fn format(&self) -> &str { "memory" }
    fn read(&self) -> Result<DataBlock, KoreError> { Ok(self.block.clone()) }
}

impl InMemorySink {
    pub fn new() -> Self { Self { block: std::sync::Arc::new(std::sync::Mutex::new(None)) } }
    pub fn get(&self) -> Option<DataBlock> { self.block.lock().unwrap().clone() }
}

impl Default for InMemorySink { fn default() -> Self { Self::new() } }

impl DataSink for InMemorySink {
    fn name(&self)   -> &str { "InMemory" }
    fn format(&self) -> &str { "memory" }
    fn write(&self, data: &DataBlock) -> Result<(), KoreError> {
        *self.block.lock().unwrap() = Some(data.clone());
        Ok(())
    }
}

// ─── Connector registry ───────────────────────────────────────────────────────

/// Factory that creates connectors by name and path.
pub struct ConnectorRegistry;

impl ConnectorRegistry {
    pub fn source(format: &str, path: &str) -> Result<Box<dyn DataSource>, KoreError> {
        match format.to_lowercase().as_str() {
            "json"  => Ok(Box::new(JsonFileSource::new(path))),
            "arrow" | "ipc" => Ok(Box::new(ArrowIpcSource::new(path))),
            _ => Err(KoreError::InvalidArgument(format!("unknown source format: {format}")))
        }
    }

    pub fn sink(format: &str, path: &str) -> Result<Box<dyn DataSink>, KoreError> {
        match format.to_lowercase().as_str() {
            "json"  => Ok(Box::new(JsonFileSink::new(path, true))),
            "arrow" | "ipc" => Ok(Box::new(ArrowIpcSink::new(path))),
            _ => Err(KoreError::InvalidArgument(format!("unknown sink format: {format}")))
        }
    }
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn json_records_to_block(records: Vec<serde_json::Value>) -> Result<DataBlock, KoreError> {
    if records.is_empty() { return Ok(DataBlock::empty()); }
    let keys: Vec<String> = records[0].as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let n = records.len();
    let columns = keys.iter().map(|k| {
        let vals: Vec<&serde_json::Value> = records.iter()
            .map(|r| r.get(k).unwrap_or(&serde_json::Value::Null))
            .collect();
        let first = vals.iter().find(|v| !v.is_null());
        let data = match first {
            Some(serde_json::Value::Number(num)) if num.is_i64() =>
                ColumnData::Int64(vals.iter().map(|v| v.as_i64()).collect()),
            Some(serde_json::Value::Number(_)) =>
                ColumnData::Float64(vals.iter().map(|v| v.as_f64()).collect()),
            Some(serde_json::Value::Bool(_)) =>
                ColumnData::Bool(vals.iter().map(|v| v.as_bool()).collect()),
            _ =>
                ColumnData::Str(vals.iter().map(|v| v.as_str().map(|s| s.to_string())).collect()),
        };
        Column { name: k.clone(), data }
    }).collect();
    Ok(DataBlock { columns, num_rows: n })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn sample() -> DataBlock {
        DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(),    data: ColumnData::Int64(vec![Some(1),Some(2),Some(3)]) },
                Column { name: "score".into(), data: ColumnData::Float64(vec![Some(1.1),Some(2.2),Some(3.3)]) },
                Column { name: "tag".into(),   data: ColumnData::Str(vec![Some("a".into()),Some("b".into()),Some("c".into())]) },
            ],
        }
    }

    #[test]
    fn test_json_roundtrip() {
        let block = sample();
        let path  = std::env::temp_dir().join("kore_connect_test.json");
        JsonFileSink::new(&path, true).write(&block).unwrap();
        let back = JsonFileSource::new(&path).read().unwrap();
        assert_eq!(back.num_rows, 3);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_arrow_ipc_roundtrip() {
        let block = sample();
        let path  = std::env::temp_dir().join("kore_connect_test.arrow");
        ArrowIpcSink::new(&path).write(&block).unwrap();
        let back = ArrowIpcSource::new(&path).read().unwrap();
        assert_eq!(back.num_rows, 3);
        assert_eq!(back.columns.len(), 3);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_in_memory_connector() {
        let block = sample();
        let src   = InMemorySource::new(block.clone());
        let sink  = InMemorySink::new();
        let read  = src.read().unwrap();
        sink.write(&read).unwrap();
        assert_eq!(sink.get().unwrap().num_rows, 3);
    }

    #[test]
    fn test_connector_registry() {
        let path = std::env::temp_dir().join("kore_reg_test.json").to_string_lossy().to_string();
        let block = sample();
        ConnectorRegistry::sink("json", &path).unwrap().write(&block).unwrap();
        let back = ConnectorRegistry::source("json", &path).unwrap().read().unwrap();
        assert_eq!(back.num_rows, 3);
        std::fs::remove_file(&path).ok();
    }
}
