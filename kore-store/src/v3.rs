//! Version-3 container: opt-in, per-column codecs, a column directory and full-coverage checksums.
//!
//! ```text
//! "KORE" | version:u16 = 3 | num_cols:u32 | num_rows:u64
//! directory, one entry per column:
//!   name_len:u16 | name | dtype:u8 | transform:u8 | stage:u8 | payload_len:u64 | payload_crc32:u32
//! dir_crc32:u32          CRC-32 of every byte from the start of the file up to here
//! payloads               concatenated in directory order (offsets are the running sum of payload_len)
//! ```
//! The file ends exactly after the last payload (no trailer; trailing bytes are an error). Every byte is
//! covered: header and directory by `dir_crc32`, each payload by its own CRC, and the length fields by the
//! exact-size check. Readers that only know versions 1 and 2 stop at the version field with
//! "unsupported version 3" and never look at the rest.

use kore_core::{Column, DataBlock, KoreError};
use crate::codec::{self, WriteOptions};
use crate::{compress, DType, MAGIC, VERSION_V3};

fn bad(m: impl Into<String>) -> KoreError { KoreError::InvalidArgument(m.into()) }

const MAX_ROWS: usize = 1 << 28;
/// name_len + dtype + transform + stage + payload_len + crc (with an empty name)
const MIN_ENTRY: usize = 2 + 1 + 1 + 1 + 8 + 4;

/// True if `data` starts with the KORE magic and format version 3.
pub fn is_v3(data: &[u8]) -> bool {
    data.len() >= 6 && &data[..4] == MAGIC && u16::from_le_bytes([data[4], data[5]]) == VERSION_V3
}

pub fn write(block: &DataBlock, opts: &WriteOptions) -> Result<Vec<u8>, KoreError> {
    use rayon::prelude::*;
    for col in &block.columns {
        if col.data.len() != block.num_rows {
            return Err(bad(format!("column '{}' has {} values, block has {} rows", col.name, col.data.len(), block.num_rows)));
        }
        if col.name.len() > u16::MAX as usize { return Err(bad("column name too long")); }
    }
    for (name, _) in &opts.columns {
        if !block.columns.iter().any(|c| &c.name == name) { return Err(bad(format!("codec given for unknown column '{name}'"))); }
    }
    let encoded: Result<Vec<(u8, u8, Vec<u8>)>, KoreError> = block.columns.par_iter()
        .map(|c| codec::encode_column(&c.data, opts.codec_for(&c.name))).collect();
    let encoded = encoded?;

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION_V3.to_le_bytes());
    out.extend_from_slice(&(block.columns.len() as u32).to_le_bytes());
    out.extend_from_slice(&(block.num_rows as u64).to_le_bytes());
    for (col, (t, s, payload)) in block.columns.iter().zip(&encoded) {
        out.extend_from_slice(&(col.name.len() as u16).to_le_bytes());
        out.extend_from_slice(col.name.as_bytes());
        out.push(codec::dtype_of(&col.data) as u8);
        out.push(*t);
        out.push(*s);
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&compress::crc32(payload).to_le_bytes());
    }
    let dir_crc = compress::crc32(&out);
    out.extend_from_slice(&dir_crc.to_le_bytes());
    for (_, _, payload) in &encoded { out.extend_from_slice(payload); }
    Ok(out)
}

struct Entry<'a> { name: String, dtype: DType, transform: u8, stage: u8, crc: u32, payload: &'a [u8] }

pub fn read(data: &[u8], wanted: Option<&[&str]>) -> Result<DataBlock, KoreError> {
    match std::panic::catch_unwind(|| read_inner(data, wanted)) {
        Ok(r) => r,
        Err(_) => Err(bad("corrupt .kore data")),
    }
}

fn read_inner(data: &[u8], wanted: Option<&[&str]>) -> Result<DataBlock, KoreError> {
    fn take<'a>(d: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], KoreError> {
        let end = pos.checked_add(n).filter(|&e| e <= d.len()).ok_or_else(|| bad("truncated .kore data"))?;
        let s = &d[*pos..end];
        *pos = end;
        Ok(s)
    }
    let mut pos = 0usize;
    if take(data, &mut pos, 4)? != MAGIC { return Err(bad("invalid KORE magic bytes")); }
    let version = u16::from_le_bytes(take(data, &mut pos, 2)?.try_into().unwrap());
    if version != VERSION_V3 { return Err(bad(format!("unsupported version {version}"))); }
    let num_cols = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap()) as usize;
    let num_rows = u64::from_le_bytes(take(data, &mut pos, 8)?.try_into().unwrap());
    if num_cols > (data.len() - pos) / MIN_ENTRY { return Err(bad("implausible column count")); }

    struct Raw { name: String, dtype: DType, transform: u8, stage: u8, len: u64, crc: u32 }
    let mut dir = Vec::with_capacity(num_cols);
    for _ in 0..num_cols {
        let name_len = u16::from_le_bytes(take(data, &mut pos, 2)?.try_into().unwrap()) as usize;
        let name = String::from_utf8(take(data, &mut pos, name_len)?.to_vec()).map_err(|_| bad("invalid UTF-8 column name"))?;
        let hdr = take(data, &mut pos, 3)?;
        let dtype = DType::try_from(hdr[0])?;
        let len = u64::from_le_bytes(take(data, &mut pos, 8)?.try_into().unwrap());
        let crc = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap());
        dir.push(Raw { name, dtype, transform: hdr[1], stage: hdr[2], len, crc });
    }
    let covered = pos;
    let dir_crc = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap());
    if compress::crc32(&data[..covered]) != dir_crc { return Err(bad("checksum mismatch in file header/directory")); }

    // header is authenticated from here on
    let num_rows = usize::try_from(num_rows).ok().filter(|&r| r <= MAX_ROWS).ok_or_else(|| bad("implausible row count"))?;
    let mut entries: Vec<Entry> = Vec::with_capacity(num_cols);
    for r in dir {
        let len = usize::try_from(r.len).map_err(|_| bad("column length overflow"))?;
        let payload = take(data, &mut pos, len)?;
        entries.push(Entry { name: r.name, dtype: r.dtype, transform: r.transform, stage: r.stage, crc: r.crc, payload });
    }
    if pos != data.len() { return Err(bad("trailing bytes after last column")); }

    if let Some(w) = wanted {
        for name in w {
            if !entries.iter().any(|c| c.name == *name) { return Err(bad(format!("column not found: {name}"))); }
        }
    }
    let keep = |name: &str| wanted.map_or(true, |w| w.iter().any(|x| *x == name));

    use rayon::prelude::*;
    let cols: Result<Vec<Column>, KoreError> = entries.into_par_iter().filter(|e| keep(&e.name)).map(|e| {
        if compress::crc32(e.payload) != e.crc { return Err(bad(format!("checksum mismatch in column '{}'", e.name))); }
        let data = codec::decode_column(e.payload, e.dtype, e.transform, e.stage, num_rows)
            .map_err(|er| bad(format!("column '{}': {er}", e.name)))?;
        if data.len() != num_rows { return Err(bad(format!("column '{}' has {} values, expected {num_rows}", e.name, data.len()))); }
        Ok(Column { name: e.name, data })
    }).collect();
    let mut columns = cols?;
    if let Some(w) = wanted {
        let mut ordered: Vec<Column> = Vec::with_capacity(w.len());
        for name in w {
            if ordered.iter().any(|c| c.name == *name) { continue; }
            if let Some(p) = columns.iter().position(|c| c.name == *name) { ordered.push(columns.swap_remove(p)); }
        }
        columns = ordered;
    }
    Ok(DataBlock { columns, num_rows })
}
