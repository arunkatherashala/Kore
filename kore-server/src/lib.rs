//! KORE Phase 4C — PostgreSQL wire protocol server
//!
//! Implements PG wire protocol v3 so standard `psql` / JDBC / ODBC clients
//! can connect and run SQL against the KORE engine.

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

use kore_core::{ColumnData, DataBlock, DataType};
use kore_sql::KqlContext;

// ─── PG type OIDs ───────────────────────────────────────────────────────────

const PG_TYPE_BOOL: i32 = 16;
const PG_TYPE_INT8: i32 = 20;
const PG_TYPE_TEXT: i32 = 25;
const PG_TYPE_FLOAT8: i32 = 701;

fn pg_type_oid(dt: DataType) -> i32 {
    match dt {
        DataType::Int64 => PG_TYPE_INT8,
        DataType::Float64 => PG_TYPE_FLOAT8,
        DataType::Str => PG_TYPE_TEXT,
        DataType::Bool => PG_TYPE_BOOL,
    }
}

// ─── Wire protocol messages ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct StartupParams {
    pub version_major: u16,
    pub version_minor: u16,
    pub params: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PgMessage {
    StartupMessage(StartupParams),
    Query { sql: String },
    Parse { name: String, query: String, param_types: Vec<i32> },
    Bind { portal: String, statement: String, param_values: Vec<Option<Vec<u8>>> },
    Describe { kind: u8, name: String },
    Execute { portal: String, max_rows: i32 },
    Sync,
    Terminate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDesc {
    pub name: String,
    pub table_oid: i32,
    pub col_attr: i16,
    pub type_oid: i32,
    pub type_len: i16,
    pub type_mod: i32,
    pub format: i16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PgResponse {
    AuthOk,
    ReadyForQuery { status: u8 },
    RowDescription { fields: Vec<FieldDesc> },
    DataRow { values: Vec<Option<Vec<u8>>> },
    CommandComplete { tag: String },
    ErrorResponse { severity: String, message: String, code: String },
    ParameterStatus { name: String, value: String },
}

// ─── Codec — read/write PG protocol v3 messages ────────────────────────────

pub struct PgWireCodec;

impl PgWireCodec {
    /// Read the startup message (no leading type byte).
    pub async fn read_startup(stream: &mut TcpStream) -> std::io::Result<StartupParams> {
        let len = Self::read_i32(stream).await? as usize;
        if len < 8 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "startup too short"));
        }
        let version = Self::read_i32(stream).await?;
        let version_major = (version >> 16) as u16;
        let version_minor = (version & 0xFFFF) as u16;

        let body_len = len - 8;
        let mut body = vec![0u8; body_len];
        stream.read_exact(&mut body).await?;

        let params = Self::parse_startup_params(&body);
        Ok(StartupParams { version_major, version_minor, params })
    }

    /// Read a normal message (1-byte type + 4-byte length + body).
    pub async fn read_message(stream: &mut TcpStream) -> std::io::Result<PgMessage> {
        let mut tag = [0u8; 1];
        stream.read_exact(&mut tag).await?;
        let len = Self::read_i32(stream).await? as usize;
        let body_len = if len >= 4 { len - 4 } else { 0 };
        let mut body = vec![0u8; body_len];
        if body_len > 0 {
            stream.read_exact(&mut body).await?;
        }

        match tag[0] {
            b'Q' => {
                let sql = Self::read_cstring_from(&body, 0).0;
                Ok(PgMessage::Query { sql })
            }
            b'P' => {
                let (name, off) = Self::read_cstring_from(&body, 0);
                let (query, off) = Self::read_cstring_from(&body, off);
                let n_params = if off + 2 <= body.len() {
                    i16::from_be_bytes([body[off], body[off + 1]]) as usize
                } else {
                    0
                };
                let mut param_types = Vec::with_capacity(n_params);
                let mut pos = off + 2;
                for _ in 0..n_params {
                    if pos + 4 <= body.len() {
                        param_types.push(i32::from_be_bytes([
                            body[pos], body[pos + 1], body[pos + 2], body[pos + 3],
                        ]));
                        pos += 4;
                    }
                }
                Ok(PgMessage::Parse { name, query, param_types })
            }
            b'B' => {
                let (portal, off) = Self::read_cstring_from(&body, 0);
                let (statement, off) = Self::read_cstring_from(&body, off);
                // Skip format codes
                let n_fmt = if off + 2 <= body.len() {
                    i16::from_be_bytes([body[off], body[off + 1]]) as usize
                } else {
                    0
                };
                let mut pos = off + 2 + n_fmt * 2;
                let n_params = if pos + 2 <= body.len() {
                    i16::from_be_bytes([body[pos], body[pos + 1]]) as usize
                } else {
                    0
                };
                pos += 2;
                let mut param_values = Vec::with_capacity(n_params);
                for _ in 0..n_params {
                    if pos + 4 > body.len() { break; }
                    let plen = i32::from_be_bytes([body[pos], body[pos+1], body[pos+2], body[pos+3]]);
                    pos += 4;
                    if plen == -1 {
                        param_values.push(None);
                    } else {
                        let end = pos + plen as usize;
                        param_values.push(Some(body[pos..end].to_vec()));
                        pos = end;
                    }
                }
                Ok(PgMessage::Bind { portal, statement, param_values })
            }
            b'D' => {
                let kind = if !body.is_empty() { body[0] } else { b'S' };
                let (name, _) = Self::read_cstring_from(&body, 1);
                Ok(PgMessage::Describe { kind, name })
            }
            b'E' => {
                let (portal, off) = Self::read_cstring_from(&body, 0);
                let max_rows = if off + 4 <= body.len() {
                    i32::from_be_bytes([body[off], body[off+1], body[off+2], body[off+3]])
                } else {
                    0
                };
                Ok(PgMessage::Execute { portal, max_rows })
            }
            b'S' => Ok(PgMessage::Sync),
            b'X' => Ok(PgMessage::Terminate),
            other => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown message type: 0x{:02X}", other),
            )),
        }
    }

    /// Write a response message to the stream.
    pub async fn write_response(stream: &mut TcpStream, resp: &PgResponse) -> std::io::Result<()> {
        match resp {
            PgResponse::AuthOk => {
                // 'R' + len(8) + status(0)
                let mut buf = Vec::with_capacity(9);
                buf.push(b'R');
                buf.extend_from_slice(&8i32.to_be_bytes());
                buf.extend_from_slice(&0i32.to_be_bytes());
                stream.write_all(&buf).await
            }
            PgResponse::ParameterStatus { name, value } => {
                let mut body = Vec::new();
                body.extend_from_slice(name.as_bytes());
                body.push(0);
                body.extend_from_slice(value.as_bytes());
                body.push(0);
                let len = (body.len() + 4) as i32;
                let mut buf = Vec::with_capacity(1 + 4 + body.len());
                buf.push(b'S');
                buf.extend_from_slice(&len.to_be_bytes());
                buf.extend_from_slice(&body);
                stream.write_all(&buf).await
            }
            PgResponse::ReadyForQuery { status } => {
                let mut buf = Vec::with_capacity(6);
                buf.push(b'Z');
                buf.extend_from_slice(&5i32.to_be_bytes());
                buf.push(*status);
                stream.write_all(&buf).await
            }
            PgResponse::RowDescription { fields } => {
                let mut body = Vec::new();
                body.extend_from_slice(&(fields.len() as i16).to_be_bytes());
                for f in fields {
                    body.extend_from_slice(f.name.as_bytes());
                    body.push(0);
                    body.extend_from_slice(&f.table_oid.to_be_bytes());
                    body.extend_from_slice(&f.col_attr.to_be_bytes());
                    body.extend_from_slice(&f.type_oid.to_be_bytes());
                    body.extend_from_slice(&f.type_len.to_be_bytes());
                    body.extend_from_slice(&f.type_mod.to_be_bytes());
                    body.extend_from_slice(&f.format.to_be_bytes());
                }
                let len = (body.len() + 4) as i32;
                let mut buf = Vec::with_capacity(1 + 4 + body.len());
                buf.push(b'T');
                buf.extend_from_slice(&len.to_be_bytes());
                buf.extend_from_slice(&body);
                stream.write_all(&buf).await
            }
            PgResponse::DataRow { values } => {
                let mut body = Vec::new();
                body.extend_from_slice(&(values.len() as i16).to_be_bytes());
                for v in values {
                    match v {
                        None => body.extend_from_slice(&(-1i32).to_be_bytes()),
                        Some(data) => {
                            body.extend_from_slice(&(data.len() as i32).to_be_bytes());
                            body.extend_from_slice(data);
                        }
                    }
                }
                let len = (body.len() + 4) as i32;
                let mut buf = Vec::with_capacity(1 + 4 + body.len());
                buf.push(b'D');
                buf.extend_from_slice(&len.to_be_bytes());
                buf.extend_from_slice(&body);
                stream.write_all(&buf).await
            }
            PgResponse::CommandComplete { tag } => {
                let mut body = Vec::new();
                body.extend_from_slice(tag.as_bytes());
                body.push(0);
                let len = (body.len() + 4) as i32;
                let mut buf = Vec::with_capacity(1 + 4 + body.len());
                buf.push(b'C');
                buf.extend_from_slice(&len.to_be_bytes());
                buf.extend_from_slice(&body);
                stream.write_all(&buf).await
            }
            PgResponse::ErrorResponse { severity, message, code } => {
                let mut body = Vec::new();
                body.push(b'S');
                body.extend_from_slice(severity.as_bytes());
                body.push(0);
                body.push(b'C');
                body.extend_from_slice(code.as_bytes());
                body.push(0);
                body.push(b'M');
                body.extend_from_slice(message.as_bytes());
                body.push(0);
                body.push(0); // terminator
                let len = (body.len() + 4) as i32;
                let mut buf = Vec::with_capacity(1 + 4 + body.len());
                buf.push(b'E');
                buf.extend_from_slice(&len.to_be_bytes());
                buf.extend_from_slice(&body);
                stream.write_all(&buf).await
            }
        }
    }

    // ── internal helpers ────────────────────────────────────────────────────

    async fn read_i32(stream: &mut TcpStream) -> std::io::Result<i32> {
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).await?;
        Ok(i32::from_be_bytes(buf))
    }

    fn parse_startup_params(body: &[u8]) -> Vec<(String, String)> {
        let mut params = Vec::new();
        let mut i = 0;
        while i < body.len() {
            if body[i] == 0 { break; }
            let (key, next) = Self::read_cstring_from(body, i);
            if key.is_empty() { break; }
            let (val, next) = Self::read_cstring_from(body, next);
            params.push((key, val));
            i = next;
        }
        params
    }

    fn read_cstring_from(buf: &[u8], offset: usize) -> (String, usize) {
        let start = offset;
        let mut end = start;
        while end < buf.len() && buf[end] != 0 {
            end += 1;
        }
        let s = String::from_utf8_lossy(&buf[start..end]).into_owned();
        (s, if end < buf.len() { end + 1 } else { end })
    }
}

// ─── DataBlock → PG response helpers ────────────────────────────────────────

pub fn row_description_from_block(block: &DataBlock) -> PgResponse {
    let fields = block.columns.iter().map(|col| {
        let type_oid = pg_type_oid(col.data.dtype());
        let type_len: i16 = match col.data.dtype() {
            DataType::Int64 => 8,
            DataType::Float64 => 8,
            DataType::Bool => 1,
            DataType::Str => -1,
        };
        FieldDesc {
            name: col.name.clone(),
            table_oid: 0,
            col_attr: 0,
            type_oid,
            type_len,
            type_mod: -1,
            format: 0, // text format
        }
    }).collect();
    PgResponse::RowDescription { fields }
}

pub fn data_row_from_block(block: &DataBlock, row: usize) -> PgResponse {
    let values = block.columns.iter().map(|col| {
        column_value_to_text(&col.data, row)
    }).collect();
    PgResponse::DataRow { values }
}

fn column_value_to_text(col: &ColumnData, row: usize) -> Option<Vec<u8>> {
    match col {
        ColumnData::Int64(v) => v.get(row)?.as_ref().map(|i| i.to_string().into_bytes()),
        ColumnData::Float64(v) => v.get(row)?.as_ref().map(|f| f.to_string().into_bytes()),
        ColumnData::Bool(v) => v.get(row)?.as_ref().map(|b| if *b { b"t".to_vec() } else { b"f".to_vec() }),
        ColumnData::Str(v) => v.get(row)?.as_ref().map(|s| s.as_bytes().to_vec()),
        ColumnData::StrDict { codes, dict } => {
            let c = *codes.get(row)?;
            if c == u8::MAX { None } else { dict.get(c as usize).map(|s| s.as_bytes().to_vec()) }
        }
    }
}

// ─── KoreServer ─────────────────────────────────────────────────────────────

pub struct KoreServer {
    #[allow(dead_code)]
    addr: String,
    ctx: Arc<RwLock<KqlContext>>,
}

impl KoreServer {
    pub fn new(addr: &str) -> Self {
        Self {
            addr: addr.to_string(),
            ctx: Arc::new(RwLock::new(KqlContext::new())),
        }
    }

    pub async fn register_table(&self, name: &str, block: DataBlock) {
        self.ctx.write().await.register(name, block);
    }

    pub async fn serve(&self, listener: TcpListener) {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("[kore-server] accept error: {e}");
                    continue;
                }
            };
            eprintln!("[kore-server] connection from {peer}");
            let ctx = Arc::clone(&self.ctx);
            tokio::spawn(async move {
                if let Err(e) = Self::handle_connection(stream, ctx).await {
                    eprintln!("[kore-server] connection error: {e}");
                }
            });
        }
    }

    async fn handle_connection(
        mut stream: TcpStream,
        ctx: Arc<RwLock<KqlContext>>,
    ) -> std::io::Result<()> {
        // 1. Startup handshake
        let startup = PgWireCodec::read_startup(&mut stream).await?;
        eprintln!("[kore-server] startup v{}.{} params={:?}",
            startup.version_major, startup.version_minor, startup.params);

        // SSL request (80877103) — reject with 'N' and re-read the real startup
        if startup.version_major == 1234 && startup.version_minor == 5679 {
            stream.write_all(b"N").await?;
            return Box::pin(Self::handle_connection(stream, ctx)).await;
        }

        PgWireCodec::write_response(&mut stream, &PgResponse::AuthOk).await?;

        let params = [
            ("server_version", "14.0"),
            ("server_encoding", "UTF8"),
            ("client_encoding", "UTF8"),
            ("DateStyle", "ISO, MDY"),
        ];
        for (k, v) in params {
            PgWireCodec::write_response(&mut stream, &PgResponse::ParameterStatus {
                name: k.to_string(), value: v.to_string(),
            }).await?;
        }

        PgWireCodec::write_response(&mut stream, &PgResponse::ReadyForQuery { status: b'I' }).await?;

        // 2. Query loop
        loop {
            let msg = match PgWireCodec::read_message(&mut stream).await {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            };

            match msg {
                PgMessage::Query { sql } => {
                    Self::handle_query(&mut stream, &ctx, &sql).await?;
                }
                PgMessage::Terminate => break,
                PgMessage::Sync => {
                    PgWireCodec::write_response(&mut stream,
                        &PgResponse::ReadyForQuery { status: b'I' }).await?;
                }
                _ => {
                    PgWireCodec::write_response(&mut stream, &PgResponse::ErrorResponse {
                        severity: "ERROR".into(),
                        message: "extended query protocol not yet supported".into(),
                        code: "0A000".into(),
                    }).await?;
                    PgWireCodec::write_response(&mut stream,
                        &PgResponse::ReadyForQuery { status: b'I' }).await?;
                }
            }
        }
        Ok(())
    }

    async fn handle_query(
        stream: &mut TcpStream,
        ctx: &Arc<RwLock<KqlContext>>,
        sql: &str,
    ) -> std::io::Result<()> {
        let guard = ctx.read().await;
        match guard.query(sql) {
            Ok(block) => {
                let row_desc = row_description_from_block(&block);
                PgWireCodec::write_response(stream, &row_desc).await?;

                for row in 0..block.num_rows {
                    let data_row = data_row_from_block(&block, row);
                    PgWireCodec::write_response(stream, &data_row).await?;
                }

                PgWireCodec::write_response(stream, &PgResponse::CommandComplete {
                    tag: format!("SELECT {}", block.num_rows),
                }).await?;

                PgWireCodec::write_response(stream,
                    &PgResponse::ReadyForQuery { status: b'I' }).await?;
            }
            Err(e) => {
                PgWireCodec::write_response(stream, &PgResponse::ErrorResponse {
                    severity: "ERROR".into(),
                    message: e.to_string(),
                    code: "42000".into(),
                }).await?;
                PgWireCodec::write_response(stream,
                    &PgResponse::ReadyForQuery { status: b'I' }).await?;
            }
        }
        Ok(())
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock};

    // ── Startup message parsing ─────────────────────────────────────────

    #[test]
    fn test_parse_startup_params() {
        let mut body = Vec::new();
        body.extend_from_slice(b"user\0testuser\0database\0testdb\0\0");
        let params = PgWireCodec::parse_startup_params(&body);
        assert_eq!(params, vec![
            ("user".to_string(), "testuser".to_string()),
            ("database".to_string(), "testdb".to_string()),
        ]);
    }

    #[test]
    fn test_parse_startup_params_empty() {
        let body = vec![0u8];
        let params = PgWireCodec::parse_startup_params(&body);
        assert!(params.is_empty());
    }

    #[tokio::test]
    async fn test_read_startup_message() {
        let mut body = Vec::new();
        body.extend_from_slice(b"user\0pg\0\0");
        let version: i32 = (3 << 16) | 0; // v3.0
        let total_len = (8 + body.len()) as i32;
        let mut wire = Vec::new();
        wire.extend_from_slice(&total_len.to_be_bytes());
        wire.extend_from_slice(&version.to_be_bytes());
        wire.extend_from_slice(&body);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let writer = tokio::spawn(async move {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(&wire).await.unwrap();
            s.shutdown().await.unwrap();
        });

        let (mut conn, _) = listener.accept().await.unwrap();
        let startup = PgWireCodec::read_startup(&mut conn).await.unwrap();

        assert_eq!(startup.version_major, 3);
        assert_eq!(startup.version_minor, 0);
        assert_eq!(startup.params, vec![("user".to_string(), "pg".to_string())]);

        writer.await.unwrap();
    }

    // ── Query message parsing ───────────────────────────────────────────

    #[tokio::test]
    async fn test_read_query_message() {
        let sql = "SELECT 1";
        let mut body = Vec::new();
        body.extend_from_slice(sql.as_bytes());
        body.push(0);
        let msg_len = (body.len() + 4) as i32;
        let mut wire = vec![b'Q'];
        wire.extend_from_slice(&msg_len.to_be_bytes());
        wire.extend_from_slice(&body);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let writer = tokio::spawn(async move {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(&wire).await.unwrap();
            s.shutdown().await.unwrap();
        });

        let (mut conn, _) = listener.accept().await.unwrap();
        let msg = PgWireCodec::read_message(&mut conn).await.unwrap();

        assert_eq!(msg, PgMessage::Query { sql: "SELECT 1".to_string() });
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn test_read_terminate_message() {
        let msg_len: i32 = 4;
        let mut wire = vec![b'X'];
        wire.extend_from_slice(&msg_len.to_be_bytes());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let writer = tokio::spawn(async move {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(&wire).await.unwrap();
            s.shutdown().await.unwrap();
        });

        let (mut conn, _) = listener.accept().await.unwrap();
        let msg = PgWireCodec::read_message(&mut conn).await.unwrap();

        assert_eq!(msg, PgMessage::Terminate);
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn test_read_sync_message() {
        let msg_len: i32 = 4;
        let mut wire = vec![b'S'];
        wire.extend_from_slice(&msg_len.to_be_bytes());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let writer = tokio::spawn(async move {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(&wire).await.unwrap();
            s.shutdown().await.unwrap();
        });

        let (mut conn, _) = listener.accept().await.unwrap();
        let msg = PgWireCodec::read_message(&mut conn).await.unwrap();

        assert_eq!(msg, PgMessage::Sync);
        writer.await.unwrap();
    }

    // ── RowDescription from DataBlock ───────────────────────────────────

    #[test]
    fn test_row_description_from_block() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(1)]),
            Column::float64("score", vec![Some(3.14)]),
            Column::str_col("name", vec![Some("alice".to_string())]),
            Column::bool_col("active", vec![Some(true)]),
        ]).unwrap();

        let resp = row_description_from_block(&block);
        if let PgResponse::RowDescription { fields } = resp {
            assert_eq!(fields.len(), 4);

            assert_eq!(fields[0].name, "id");
            assert_eq!(fields[0].type_oid, PG_TYPE_INT8);
            assert_eq!(fields[0].type_len, 8);

            assert_eq!(fields[1].name, "score");
            assert_eq!(fields[1].type_oid, PG_TYPE_FLOAT8);
            assert_eq!(fields[1].type_len, 8);

            assert_eq!(fields[2].name, "name");
            assert_eq!(fields[2].type_oid, PG_TYPE_TEXT);
            assert_eq!(fields[2].type_len, -1);

            assert_eq!(fields[3].name, "active");
            assert_eq!(fields[3].type_oid, PG_TYPE_BOOL);
            assert_eq!(fields[3].type_len, 1);
        } else {
            panic!("expected RowDescription");
        }
    }

    #[test]
    fn test_row_description_str_dict() {
        let block = DataBlock::new(vec![
            Column::str_dict("city",
                vec![0, 1, 0],
                vec!["NYC".to_string(), "LA".to_string()]),
        ]).unwrap();

        let resp = row_description_from_block(&block);
        if let PgResponse::RowDescription { fields } = resp {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].name, "city");
            assert_eq!(fields[0].type_oid, PG_TYPE_TEXT);
        } else {
            panic!("expected RowDescription");
        }
    }

    // ── DataRow serialization ───────────────────────────────────────────

    #[test]
    fn test_data_row_from_block() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(42), Some(99)]),
            Column::float64("score", vec![Some(3.14), None]),
            Column::str_col("name", vec![Some("alice".to_string()), Some("bob".to_string())]),
            Column::bool_col("active", vec![Some(true), Some(false)]),
        ]).unwrap();

        // Row 0
        let resp = data_row_from_block(&block, 0);
        if let PgResponse::DataRow { values } = resp {
            assert_eq!(values.len(), 4);
            assert_eq!(values[0], Some(b"42".to_vec()));
            assert_eq!(values[1], Some(b"3.14".to_vec()));
            assert_eq!(values[2], Some(b"alice".to_vec()));
            assert_eq!(values[3], Some(b"t".to_vec()));
        } else {
            panic!("expected DataRow");
        }

        // Row 1 (NULL float)
        let resp = data_row_from_block(&block, 1);
        if let PgResponse::DataRow { values } = resp {
            assert_eq!(values[0], Some(b"99".to_vec()));
            assert_eq!(values[1], None); // NULL
            assert_eq!(values[2], Some(b"bob".to_vec()));
            assert_eq!(values[3], Some(b"f".to_vec()));
        } else {
            panic!("expected DataRow");
        }
    }

    #[test]
    fn test_data_row_str_dict() {
        let block = DataBlock::new(vec![
            Column::str_dict("city",
                vec![0, u8::MAX, 1],
                vec!["NYC".to_string(), "LA".to_string()]),
        ]).unwrap();

        let r0 = data_row_from_block(&block, 0);
        if let PgResponse::DataRow { values } = r0 {
            assert_eq!(values[0], Some(b"NYC".to_vec()));
        } else { panic!("expected DataRow"); }

        let r1 = data_row_from_block(&block, 1);
        if let PgResponse::DataRow { values } = r1 {
            assert_eq!(values[0], None); // NULL
        } else { panic!("expected DataRow"); }

        let r2 = data_row_from_block(&block, 2);
        if let PgResponse::DataRow { values } = r2 {
            assert_eq!(values[0], Some(b"LA".to_vec()));
        } else { panic!("expected DataRow"); }
    }

    // ── Round-trip: write response → read bytes → verify ────────────────

    #[tokio::test]
    async fn test_write_auth_ok() {
        let (mut client, mut server) = tcp_pair().await;
        PgWireCodec::write_response(&mut server, &PgResponse::AuthOk).await.unwrap();
        drop(server);

        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf[0], b'R');
        let len = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(len, 8);
        let status = i32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]);
        assert_eq!(status, 0);
    }

    #[tokio::test]
    async fn test_write_ready_for_query() {
        let (mut client, mut server) = tcp_pair().await;
        PgWireCodec::write_response(&mut server,
            &PgResponse::ReadyForQuery { status: b'I' }).await.unwrap();
        drop(server);

        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf[0], b'Z');
        assert_eq!(buf[5], b'I');
    }

    #[tokio::test]
    async fn test_write_command_complete() {
        let (mut client, mut server) = tcp_pair().await;
        PgWireCodec::write_response(&mut server, &PgResponse::CommandComplete {
            tag: "SELECT 3".to_string(),
        }).await.unwrap();
        drop(server);

        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf[0], b'C');
        let body_start = 5;
        let body_end = buf.len() - 1; // strip null terminator
        let tag = std::str::from_utf8(&buf[body_start..body_end]).unwrap();
        assert_eq!(tag, "SELECT 3");
    }

    #[tokio::test]
    async fn test_write_error_response() {
        let (mut client, mut server) = tcp_pair().await;
        PgWireCodec::write_response(&mut server, &PgResponse::ErrorResponse {
            severity: "ERROR".to_string(),
            message: "bad query".to_string(),
            code: "42000".to_string(),
        }).await.unwrap();
        drop(server);

        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf[0], b'E');
        let body = String::from_utf8_lossy(&buf[5..]);
        assert!(body.contains("ERROR"));
        assert!(body.contains("bad query"));
        assert!(body.contains("42000"));
    }

    #[tokio::test]
    async fn test_roundtrip_row_description_and_data_rows() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(1), Some(2), Some(3)]),
            Column::str_col("name", vec![
                Some("alpha".into()), Some("beta".into()), Some("gamma".into()),
            ]),
        ]).unwrap();

        let (mut client, mut server) = tcp_pair().await;

        let row_desc = row_description_from_block(&block);
        PgWireCodec::write_response(&mut server, &row_desc).await.unwrap();
        for row in 0..block.num_rows {
            let dr = data_row_from_block(&block, row);
            PgWireCodec::write_response(&mut server, &dr).await.unwrap();
        }
        PgWireCodec::write_response(&mut server, &PgResponse::CommandComplete {
            tag: format!("SELECT {}", block.num_rows),
        }).await.unwrap();
        drop(server);

        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

        // RowDescription starts with 'T'
        assert_eq!(buf[0], b'T');

        // Verify we can find 3 DataRow messages (tag 'D') and 1 CommandComplete (tag 'C')
        let mut pos = 0;
        let mut d_count = 0;
        let mut c_count = 0;
        while pos < buf.len() {
            let tag = buf[pos];
            if pos + 5 > buf.len() { break; }
            let len = i32::from_be_bytes([buf[pos+1], buf[pos+2], buf[pos+3], buf[pos+4]]) as usize;
            match tag {
                b'D' => d_count += 1,
                b'C' => c_count += 1,
                _ => {}
            }
            pos += 1 + len;
        }
        assert_eq!(d_count, 3);
        assert_eq!(c_count, 1);
    }

    #[tokio::test]
    async fn test_roundtrip_parameter_status() {
        let (mut client, mut server) = tcp_pair().await;
        PgWireCodec::write_response(&mut server, &PgResponse::ParameterStatus {
            name: "server_encoding".into(),
            value: "UTF8".into(),
        }).await.unwrap();
        drop(server);

        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf[0], b'S');
        let body = String::from_utf8_lossy(&buf[5..]);
        assert!(body.contains("server_encoding"));
        assert!(body.contains("UTF8"));
    }

    // ── Type mapping ────────────────────────────────────────────────────

    #[test]
    fn test_pg_type_oid_mapping() {
        assert_eq!(pg_type_oid(DataType::Int64), PG_TYPE_INT8);
        assert_eq!(pg_type_oid(DataType::Float64), PG_TYPE_FLOAT8);
        assert_eq!(pg_type_oid(DataType::Str), PG_TYPE_TEXT);
        assert_eq!(pg_type_oid(DataType::Bool), PG_TYPE_BOOL);
    }

    // ── Parse message parsing ───────────────────────────────────────────

    #[tokio::test]
    async fn test_read_parse_message() {
        let mut body = Vec::new();
        body.extend_from_slice(b"stmt1\0SELECT $1\0");
        body.extend_from_slice(&1i16.to_be_bytes()); // 1 param type
        body.extend_from_slice(&PG_TYPE_INT8.to_be_bytes());

        let msg_len = (body.len() + 4) as i32;
        let mut wire = vec![b'P'];
        wire.extend_from_slice(&msg_len.to_be_bytes());
        wire.extend_from_slice(&body);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let writer = tokio::spawn(async move {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(&wire).await.unwrap();
            s.shutdown().await.unwrap();
        });

        let (mut conn, _) = listener.accept().await.unwrap();
        let msg = PgWireCodec::read_message(&mut conn).await.unwrap();

        assert_eq!(msg, PgMessage::Parse {
            name: "stmt1".to_string(),
            query: "SELECT $1".to_string(),
            param_types: vec![PG_TYPE_INT8],
        });
        writer.await.unwrap();
    }

    // ── Helper: TCP pair for write/read tests ───────────────────────────

    async fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (client, server)
    }
}
