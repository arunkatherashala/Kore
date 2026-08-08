//! Wire codec — binary (MessagePack + optional LZ4) with JSON auto-fallback.
//!
//! # Frame body layout (inside the length-prefixed frame)
//!
//! Two formats are supported on the wire; the reader auto-detects by peeking
//! the first byte:
//!
//! ```text
//! JSON  (backward compat):    { "type": "..." , ... }        first byte = b'{'
//! Binary (default from v8+):  [ 'K' 'R' 'B' codec_byte  payload... ]
//!                               │              │
//!                               │              └─ 0 = MessagePack raw
//!                               │                 1 = MessagePack + LZ4
//!                               └─ 3-byte magic (Kore Rust Binary)
//! ```
//!
//! Because JSON output for `KoreMsg` always starts with `{`, and our binary
//! magic starts with `K`, the two are unambiguously distinguishable on read.
//!
//! # Rationale
//!
//! JSON serialization of a `DataBlock` with millions of rows is dominated by
//! `serde_json::to_vec` (numeric → decimal string conversion) and inflates
//! payloads 3–5×. MessagePack + LZ4 collapses both costs, which is the
//! single largest win on the shuffle / bulk-data path.
//!
//! MessagePack is chosen over bincode because it fully supports serde
//! internally-tagged enums (`#[serde(tag = "type")]`) — bincode 1.x does not.

use crate::KoreMsg;

/// Binary frame magic: 'K' 'R' 'B' (Kore Rust Binary).
pub const BINARY_MAGIC: [u8; 3] = *b"KRB";

/// Codec byte written right after the magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireCodec {
    /// MessagePack-serialized payload (no compression).
    MsgPack = 0,
    /// MessagePack-serialized payload compressed with LZ4.
    MsgPackLz4 = 1,
}

impl WireCodec {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::MsgPack),
            1 => Some(Self::MsgPackLz4),
            _ => None,
        }
    }
}

/// Chosen wire format for outbound messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    Json,
    Binary(WireCodec),
}

impl WireFormat {
    /// LZ4-compressed MessagePack — the default for bulk DataBlock traffic.
    pub const BINARY_FAST: WireFormat = WireFormat::Binary(WireCodec::MsgPackLz4);
    /// Uncompressed MessagePack — useful for tiny control messages.
    pub const BINARY_RAW: WireFormat = WireFormat::Binary(WireCodec::MsgPack);

    /// The default format chosen from the `KORE_WIRE` env var.
    /// Accepts:  `binary` (default), `binary-raw`, `json`.
    pub fn from_env() -> Self {
        match std::env::var("KORE_WIRE").ok().as_deref() {
            Some("json")       => Self::Json,
            Some("binary-raw") => Self::BINARY_RAW,
            _                  => Self::BINARY_FAST,
        }
    }
}

// ─── Encode ───────────────────────────────────────────────────────────────────

/// Serialize a `KoreMsg` into a frame body according to `fmt`.
pub fn encode(msg: &KoreMsg, fmt: WireFormat) -> std::io::Result<Vec<u8>> {
    match fmt {
        WireFormat::Json => encode_json(msg),
        WireFormat::Binary(codec) => encode_binary(msg, codec),
    }
}

fn encode_json(msg: &KoreMsg) -> std::io::Result<Vec<u8>> {
    serde_json::to_vec(msg).map_err(io_err)
}

fn encode_binary(msg: &KoreMsg, codec: WireCodec) -> std::io::Result<Vec<u8>> {
    // Named-field mode for KoreMsg's internally tagged enum. Both
    // `rmp_serde::to_vec_named` and `to_vec` work for internally-tagged
    // enums; `_named` also preserves struct field names so future field
    // additions on either side are tolerated.
    let raw = rmp_serde::to_vec_named(msg)
        .map_err(|e| io_err_str(format!("msgpack encode: {e}")))?;
    let payload = match codec {
        WireCodec::MsgPack => raw,
        WireCodec::MsgPackLz4 => lz4_flex::compress_prepend_size(&raw),
    };
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&BINARY_MAGIC);
    out.push(codec as u8);
    out.extend_from_slice(&payload);
    Ok(out)
}

// ─── Decode ───────────────────────────────────────────────────────────────────

/// Deserialize a frame body into a `KoreMsg`, auto-detecting the format.
pub fn decode(body: &[u8]) -> std::io::Result<KoreMsg> {
    if body.len() >= 4 && body[..3] == BINARY_MAGIC {
        let codec = WireCodec::from_byte(body[3])
            .ok_or_else(|| io_err_str(format!("unknown wire codec byte: {}", body[3])))?;
        return decode_binary(&body[4..], codec);
    }
    // Fallback: JSON (starts with '{' or whitespace).
    serde_json::from_slice(body).map_err(io_err)
}

fn decode_binary(payload: &[u8], codec: WireCodec) -> std::io::Result<KoreMsg> {
    let raw_owned;
    let raw: &[u8] = match codec {
        WireCodec::MsgPack => payload,
        WireCodec::MsgPackLz4 => {
            raw_owned = lz4_flex::decompress_size_prepended(payload)
                .map_err(|e| io_err_str(format!("lz4 decompress: {e}")))?;
            &raw_owned[..]
        }
    };
    rmp_serde::from_slice::<KoreMsg>(raw)
        .map_err(|e| io_err_str(format!("msgpack decode: {e}")))
}

// ─── Small helpers ────────────────────────────────────────────────────────────

fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}
fn io_err_str(s: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, s)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn sample_msg(rows: usize) -> KoreMsg {
        KoreMsg::AssignTask {
            task_id: "t-1".into(),
            stage_id: 0,
            partition_id: 3,
            sql: "SELECT * FROM t WHERE v > 100".into(),
            table_name: "t".into(),
            data: DataBlock {
                num_rows: rows,
                columns: vec![
                    Column { name: "id".into(),
                        data: ColumnData::Int64((0..rows).map(|i| Some(i as i64)).collect()) },
                    Column { name: "v".into(),
                        data: ColumnData::Float64((0..rows).map(|i| Some(i as f64 * 1.25)).collect()) },
                ],
            },
        }
    }

    #[test]
    fn json_roundtrip_still_works() {
        let msg = sample_msg(100);
        let body = encode(&msg, WireFormat::Json).unwrap();
        assert_eq!(body[0], b'{'); // JSON prefix
        let back = decode(&body).unwrap();
        assert!(matches!(back, KoreMsg::AssignTask { .. }));
    }

    #[test]
    fn binary_raw_roundtrip() {
        let msg = sample_msg(500);
        let body = encode(&msg, WireFormat::BINARY_RAW).unwrap();
        assert_eq!(&body[..3], b"KRB");
        assert_eq!(body[3], WireCodec::MsgPack as u8);
        let back = decode(&body).unwrap();
        if let KoreMsg::AssignTask { data, .. } = back {
            assert_eq!(data.num_rows, 500);
        } else { panic!("wrong variant"); }
    }

    #[test]
    fn binary_lz4_roundtrip() {
        let msg = sample_msg(5_000);
        let body = encode(&msg, WireFormat::BINARY_FAST).unwrap();
        assert_eq!(&body[..3], b"KRB");
        assert_eq!(body[3], WireCodec::MsgPackLz4 as u8);
        let back = decode(&body).unwrap();
        if let KoreMsg::AssignTask { data, .. } = back {
            assert_eq!(data.num_rows, 5_000);
        } else { panic!("wrong variant"); }
    }

    #[test]
    fn binary_is_smaller_than_json_for_bulk_data() {
        let msg = sample_msg(10_000);
        let json = encode(&msg, WireFormat::Json).unwrap();
        let bin  = encode(&msg, WireFormat::BINARY_FAST).unwrap();
        // For 10k rows of numeric data, LZ4 MessagePack should be strictly
        // smaller than JSON. Concrete ratio depends on data; assert only the
        // basic property that binary wins.
        assert!(bin.len() < json.len(),
            "expected binary ({}) < JSON ({})", bin.len(), json.len());
    }

    #[test]
    fn auto_detect_reads_both_formats() {
        let msg = sample_msg(50);
        let json_body = encode(&msg, WireFormat::Json).unwrap();
        let bin_body  = encode(&msg, WireFormat::BINARY_FAST).unwrap();
        // Same decode entry point handles both.
        assert!(matches!(decode(&json_body).unwrap(), KoreMsg::AssignTask { .. }));
        assert!(matches!(decode(&bin_body).unwrap(),  KoreMsg::AssignTask { .. }));
    }
}
