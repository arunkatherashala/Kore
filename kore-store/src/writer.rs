//! KoreWriter — serialize a DataBlock to bytes / a file.

use std::io::{self, Write};
use kore_core::{ColumnData, DataBlock};
use crate::{Compression, DType, MAGIC, VERSION, compress};

/// Try LZ4 on top of already-encoded bytes.
/// Returns (Compression::Lz4, compressed) if LZ4 shrinks data, else original.
fn try_lz4(comp: Compression, data: Vec<u8>) -> (Compression, Vec<u8>) {
    if data.len() < 64 { return (comp, data); }  // not worth it for tiny cols
    let compressed = lz4_flex::compress_prepend_size(&data);
    if compressed.len() < data.len() {
        // Encode the original comp type in first byte so reader can round-trip
        let mut out = vec![comp as u8];
        out.extend_from_slice(&compressed);
        (Compression::Lz4, out)
    } else {
        (comp, data)
    }
}

pub struct KoreWriter;

impl KoreWriter {
    /// Serialize a DataBlock to a byte buffer.
    pub fn to_bytes(block: &DataBlock) -> Vec<u8> {
        let mut buf = Vec::new();
        KoreWriter::write_to(&mut buf, block).expect("in-memory write never fails");
        buf
    }

    /// Serialize to any `Write` target.
    pub fn write_to<W: Write>(w: &mut W, block: &DataBlock) -> io::Result<()> {
        let num_cols = block.columns.len() as u32;
        let num_rows = block.num_rows as u64;

        // ── Header ────────────────────────────────────────────────────────
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&num_cols.to_le_bytes())?;
        w.write_all(&num_rows.to_le_bytes())?;

        // ── Schema ────────────────────────────────────────────────────────
        for col in &block.columns {
            let name_bytes = col.name.as_bytes();
            w.write_all(&(name_bytes.len() as u16).to_le_bytes())?;
            w.write_all(name_bytes)?;
            let dtype: u8 = match &col.data {
                ColumnData::Int64(_)       => DType::I64     as u8,
                ColumnData::Float64(_)     => DType::F64     as u8,
                ColumnData::Bool(_)        => DType::Bool    as u8,
                ColumnData::Str(_)         => DType::Str     as u8,
                ColumnData::StrDict { .. } => DType::StrDict as u8,
            };
            w.write_all(&[dtype])?;;
        }

        // ── Column data ───────────────────────────────────────────────────
        for col in &block.columns {
            let (comp, data) = encode_column(&col.data);
            let (final_comp, final_data) = try_lz4(comp, data);
            w.write_all(&[final_comp as u8])?;
            w.write_all(&(final_data.len() as u64).to_le_bytes())?;
            w.write_all(&final_data)?;
        }
        Ok(())
    }

    /// Convenience: write to a file path.
    pub fn write_file(path: &std::path::Path, block: &DataBlock) -> io::Result<()> {
        let mut f = std::fs::File::create(path)?;
        Self::write_to(&mut f, block)
    }
}

fn encode_column(data: &ColumnData) -> (Compression, Vec<u8>) {
    match data {
        ColumnData::Int64(v) => {
            let is_sorted = v.windows(2).all(|w| match (w[0], w[1]) {
                (Some(a), Some(b)) => a <= b,
                _ => true,
            });
            if is_sorted {
                (Compression::Delta, compress::delta_encode_i64(v))
            } else {
                (Compression::Rle, compress::rle_encode_i64(v))
            }
        }
        // Float64: try dictionary first (huge win for low-cardinality, e.g. l_discount).
        // Fall back to NaN-sentinel (8 bytes/el) which is still better than raw (9 bytes/el).
        ColumnData::Float64(v) => {
            if let Some(dict_bytes) = compress::dict_encode_f64(v) {
                (Compression::Dict, dict_bytes)
            } else {
                (Compression::NanRaw, compress::nan_encode_f64(v))
            }
        }
        ColumnData::Bool(v)    => (Compression::Raw, compress::raw_encode_bool(v)),
        ColumnData::Str(v)     => (Compression::Raw, compress::encode_strs(v)),
        // StrDict: store codes directly (1 byte/row) + tiny dict — no string explosion.
        ColumnData::StrDict { codes, dict } => {
            (Compression::Raw, compress::encode_strdict(codes, dict))
        }
    }
}
