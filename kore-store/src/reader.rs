//! KoreReader — deserialize bytes / a file back into a DataBlock.

use std::io::Read;
use kore_core::{Column, ColumnData, DataBlock, KoreError};
use crate::{Compression, DType, MAGIC, compress};
use memmap2::Mmap;

const READABLE_FOOTER_PREFIX: &[u8] = b"KORE-READABLE-FOOTER trailer_len=";

/// Below this many kept column bytes a file is checksummed and decoded on the calling thread: for
/// tiny files the rayon hand-off costs more than it saves.
const PAR_MIN_BYTES: usize = 16 * 1024;

pub struct KoreReader;

impl KoreReader {
    /// Parse a DataBlock from a byte slice with zero-copy metadata parsing.
    pub fn from_bytes(data: &[u8]) -> Result<DataBlock, KoreError> {
        Self::from_bytes_columns_opt(data, None)
    }

    /// Column projection: decode only `columns` (returned in the requested order). The other
    /// columns' bytes are skipped without being checksummed or decompressed.
    pub fn from_bytes_columns(data: &[u8], columns: &[&str]) -> Result<DataBlock, KoreError> {
        Self::from_bytes_columns_opt(data, Some(columns))
    }

    fn from_bytes_columns_opt(data: &[u8], wanted: Option<&[&str]>) -> Result<DataBlock, KoreError> {
        if crate::versioned::is_version_log(data) {
            let latest = crate::versioned::select_raw(data, u64::MAX)?;
            return Self::from_bytes_columns_opt(latest, wanted);
        }
        if crate::rowgroups::is_row_group_file(data) {
            return crate::rowgroups::read(data, wanted, None);
        }
        if crate::v3::is_v3(data) {
            return crate::v3::read(data, wanted);
        }
        // Check for encryption marker
        if data.len() >= 4 && &data[0..4] == b"KENC" {
            return Err(KoreError::InvalidArgument(
                "encrypted .kore file — use from_bytes_decrypt(data, password)".into()));
        }
        let binary_data = strip_readable_trailer(data);
        Self::parse_binary(binary_data, wanted)
    }

    /// Parse an encrypted .kore file.
    pub fn from_bytes_decrypt(data: &[u8], password: &[u8]) -> Result<DataBlock, KoreError> {
        if data.len() < 4 || &data[0..4] != b"KENC" {
            return Self::from_bytes(data);
        }
        let plaintext = Self::decrypt_blob(data, password)?;
        Self::from_bytes(&plaintext)
    }

    /// Inverse of `KoreWriter::encrypt_blob`. Never panics on malformed input.
    pub fn decrypt_blob(data: &[u8], password: &[u8]) -> Result<Vec<u8>, KoreError> {
        fn bad(m: &str) -> KoreError { KoreError::InvalidArgument(m.into()) }
        fn take<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], KoreError> {
            let end = pos.checked_add(n).filter(|&e| e <= data.len()).ok_or_else(|| bad("truncated encrypted blob"))?;
            let s = &data[*pos..end];
            *pos = end;
            Ok(s)
        }
        let mut pos = 0;
        if take(data, &mut pos, 4)? != b"KENC" { return Err(bad("not an encrypted blob (missing KENC marker)")); }
        let salt_len = u16::from_le_bytes(take(data, &mut pos, 2)?.try_into().unwrap()) as usize;
        let salt = take(data, &mut pos, salt_len)?.to_vec();
        let nonce_len = u16::from_le_bytes(take(data, &mut pos, 2)?.try_into().unwrap()) as usize;
        if nonce_len != 12 { return Err(bad("invalid nonce length")); }
        let nonce = take(data, &mut pos, nonce_len)?.to_vec();
        let meta = crate::EncryptionMetadata {
            encrypted_cols: vec![],
            algorithm: "AES-256-GCM".into(),
            kdf: "PBKDF2".into(),
            salt, nonce,
        };
        crate::writer::decrypt_column(&data[pos..], password, &meta)
            .map_err(|e| KoreError::InvalidArgument(format!("decrypt failed: {e}")))
    }

    /// Read a version snapshot from the KVER footer marker.
    pub fn read_version_snapshot(data: &[u8]) -> Option<crate::VersionSnapshot> {
        let marker = b"KVER";
        // last occurrence, as `windows(4).rposition` found it, via a vectorised search
        if let Some(pos) = memchr::memmem::rfind(data, marker) {
            let vd = &data[pos + 4..];
            if vd.len() >= 28 {
                return Some(crate::VersionSnapshot {
                    version_id:   u32::from_le_bytes(vd[0..4].try_into().unwrap()),
                    timestamp:    u64::from_le_bytes(vd[4..12].try_into().unwrap()),
                    row_count:    u64::from_le_bytes(vd[12..20].try_into().unwrap()),
                    block_offset: u64::from_le_bytes(vd[20..28].try_into().unwrap()),
                    prev_version: None,
                });
            }
        }
        None
    }

    fn parse_binary(binary_data: &[u8], wanted: Option<&[&str]>) -> Result<DataBlock, KoreError> {
        // Corrupt input must yield Err, never abort the host process.
        match std::panic::catch_unwind(|| Self::parse_binary_inner(binary_data, wanted)) {
            Ok(r) => r,
            Err(_) => Err(KoreError::InvalidArgument("corrupt .kore data".into())),
        }
    }

    fn parse_binary_inner(binary_data: &[u8], wanted: Option<&[&str]>) -> Result<DataBlock, KoreError> {
        fn bad(m: &str) -> KoreError { KoreError::InvalidArgument(m.into()) }
        fn take<'a>(d: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], KoreError> {
            let end = pos.checked_add(n).filter(|&e| e <= d.len()).ok_or_else(|| bad("truncated .kore data"))?;
            let s = &d[*pos..end];
            *pos = end;
            Ok(s)
        }
        const MAX_ROWS: usize = 1 << 28;

        let mut pos = 0;

        // ── Header ────────────────────────────────────────────────────────
        if take(binary_data, &mut pos, 4)? != MAGIC {
            return Err(bad("invalid KORE magic bytes"));
        }
        let version = u16::from_le_bytes(take(binary_data, &mut pos, 2)?.try_into().unwrap());
        if version != crate::VERSION && version != 1 {
            return Err(KoreError::InvalidArgument(format!("unsupported version {version}")));
        }
        let num_cols = u32::from_le_bytes(take(binary_data, &mut pos, 4)?.try_into().unwrap()) as usize;
        let num_rows = u64::from_le_bytes(take(binary_data, &mut pos, 8)?.try_into().unwrap()) as usize;
        if num_rows > MAX_ROWS { return Err(bad("implausible row count")); }
        // every column needs at least a name length, dtype byte, codec byte and length field
        if num_cols > (binary_data.len() - pos) / 3 { return Err(bad("implausible column count")); }

        // ── Schema ────────────────────────────────────────────────────────
        let mut schema: Vec<(String, DType)> = Vec::with_capacity(num_cols);
        for _ in 0..num_cols {
            let name_len = u16::from_le_bytes(take(binary_data, &mut pos, 2)?.try_into().unwrap()) as usize;
            let name = String::from_utf8(take(binary_data, &mut pos, name_len)?.to_vec())
                .map_err(|_| bad("invalid UTF-8 column name"))?;
            let dtype = DType::try_from(take(binary_data, &mut pos, 1)?[0])?;
            schema.push((name, dtype));
        }

        // ── Column data ───────────────────────────────────────────────────
        use rayon::prelude::*;

        struct ColChunk<'a> {
            name: String,
            dtype: DType,
            comp: Compression,
            raw: &'a [u8],
        }

        let mut chunks = Vec::with_capacity(num_cols);
        for (name, dtype) in schema {
            let comp = Compression::try_from(take(binary_data, &mut pos, 1)?[0])?;
            let data_len = u64::from_le_bytes(take(binary_data, &mut pos, 8)?.try_into().unwrap());
            let data_len = usize::try_from(data_len).map_err(|_| bad("column length overflow"))?;
            let raw = take(binary_data, &mut pos, data_len)?;
            chunks.push(ColChunk { name, dtype, comp, raw });
        }

        if let Some(w) = wanted {
            for name in w {
                if !chunks.iter().any(|c| c.name == *name) {
                    return Err(KoreError::InvalidArgument(format!("column not found: {name}")));
                }
            }
        }
        let keep = |name: &str| wanted.map_or(true, |w| w.iter().any(|x| *x == name));

        // Per-column CRC32 from the stats section the writer appends after the column data. Skipped
        // when the section is absent or not exactly parseable (older files).
        let mut crcs: Option<Vec<u32>> = None;
        if let Ok(len_bytes) = take(binary_data, &mut pos, 4) {
            let sec_len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
            if let Ok(sec) = take(binary_data, &mut pos, sec_len) {
                let mut sp = 0usize;
                let mut v = Vec::with_capacity(chunks.len());
                let mut ok = true;
                for _ in 0..chunks.len() {
                    let entry = (|| -> Option<u32> {
                        let crc = u32::from_le_bytes(sec.get(sp..sp + 4)?.try_into().ok()?);
                        let has_stats = *sec.get(sp + 5)?;
                        sp += 6 + if has_stats == 1 { 16 } else { 0 };
                        (sp <= sec.len()).then_some(crc)
                    })();
                    match entry { Some(c) => v.push(c), None => { ok = false; break; } }
                }
                if ok && sp == sec.len() { crcs = Some(v); }
            }
        }

        // Kept columns: (position, chunk, expected crc).
        let work: Vec<(usize, ColChunk, Option<u32>)> = chunks.into_iter().enumerate()
            .map(|(i, c)| { let e = crcs.as_ref().map(|v| v[i]); (i, c, e) })
            .filter(|(_, c, _)| keep(&c.name))
            .collect();

        // One task per kept column: checksum, and decode only if the checksum matched. The result is
        // what checking every column before decoding any gave: if any column is damaged, the first
        // damaged one in file order is reported as a checksum mismatch (and none of the damaged
        // columns is decoded); otherwise the first decode error in file order. Columns are handled in
        // parallel only when there is enough data to pay for the thread hand-off.
        enum Res { Ok(Column), Crc(String), Dec(String) }
        let kept_bytes: usize = work.iter().map(|(_, c, _)| c.raw.len()).sum();
        let parallel = work.len() > 1 && kept_bytes >= PAR_MIN_BYTES;
        let one = |(i, chunk, expected): (usize, ColChunk, Option<u32>)| -> Res {
            if expected.map_or(false, |e| compress::crc32(chunk.raw) != e) { return Res::Crc(chunk.name); }
            match decode_column(chunk.raw, chunk.dtype, chunk.comp, num_rows) {
                Ok(data) => Res::Ok(Column { name: chunk.name, data }),
                Err(e) => Res::Dec(format!("col {}: {}", i, e)),
            }
        };
        let results: Vec<Res> = if parallel { work.into_par_iter().map(one).collect() } else { work.into_iter().map(one).collect() };
        if let Some(name) = results.iter().find_map(|r| if let Res::Crc(n) = r { Some(n) } else { None }) {
            return Err(KoreError::InvalidArgument(format!("checksum mismatch in column '{name}'")));
        }
        let columns: Result<Vec<Column>, String> = results.into_iter()
            .map(|r| match r { Res::Ok(c) => Ok(c), Res::Dec(e) => Err(e), Res::Crc(_) => unreachable!() })
            .collect();
        let mut columns = columns.map_err(|e| KoreError::InvalidArgument(e))?;

        if let Some(w) = wanted {
            // requested order; a name listed twice yields one column
            let mut ordered = Vec::with_capacity(w.len());
            for name in w {
                if ordered.iter().any(|c: &Column| c.name == *name) { continue; }
                if let Some(pos) = columns.iter().position(|c| c.name == *name) {
                    ordered.push(columns.swap_remove(pos));
                }
            }
            columns = ordered;
        }
        Ok(DataBlock { columns, num_rows })
    }

    /// Read from any `Read` source (no zero-copy slicing).
    pub fn read_from(r: &mut dyn Read) -> Result<DataBlock, KoreError> {
        let mut bytes = Vec::new();
        r.read_to_end(&mut bytes).map_err(io_err)?;
        Self::from_bytes(&bytes)
    }

    /// High Performance: Use Memory Mapped I/O for zero-copy file bridge.
    /// Files under `MMAP_MIN_BYTES` are read into memory in one call (mapping and faulting in a
    /// small file costs more than reading it); larger ones are memory-mapped so that only the parts a
    /// read touches (one version of a log, some columns) are loaded.
    pub fn read_file(path: &std::path::Path) -> Result<DataBlock, KoreError> {
        with_file_bytes(path, |d| Self::from_bytes(d))
    }

    /// File read of selected columns only (see `from_bytes_columns` and `read_file`).
    pub fn read_file_columns(path: &std::path::Path, columns: &[&str]) -> Result<DataBlock, KoreError> {
        with_file_bytes(path, |d| Self::from_bytes_columns(d, columns))
    }

    /// Time travel: newest version with timestamp <= `target_timestamp`.
    /// Works on version logs (see `versioned`). A plain .kore file is a single version
    /// stamped by its KVER footer (or timestamp 0 if none).
    pub fn read_at_version(data: &[u8], target_timestamp: u64) -> Result<DataBlock, KoreError> {
        if crate::versioned::is_version_log(data) {
            return crate::versioned::read_at(data, target_timestamp);
        }
        let ts = Self::read_version_snapshot(data).map(|v| v.timestamp).unwrap_or(0);
        if target_timestamp < ts {
            return Err(KoreError::InvalidArgument("no version at or before the requested timestamp".into()));
        }
        Self::from_bytes(data)
    }

    /// Get partition specification (for partition-aware queries).
    pub fn get_partition_spec(data: &[u8]) -> Option<crate::PartitionSpec> {
        // Extract partition spec from footer metadata
        // For now: return default (unpartitioned)
        Some(crate::PartitionSpec {
            spec_id: 0,
            columns: vec![],
            transforms: vec![],
            parent_spec_id: None,
        })
    }

    /// Get delete vector (for row-level soft deletes).
    pub fn get_delete_vector(data: &[u8]) -> Option<crate::DeleteVector> {
        // Scan footer for delete vector marker b"KDEL"
        let marker = b"KDEL";
        if let Some(pos) = memchr::memmem::rfind(data, marker) {
            let dv_data = &data[pos + 4..];
            if dv_data.len() >= 12 {
                let cardinality = u32::from_le_bytes(dv_data[0..4].try_into().unwrap());
                let timestamp = u64::from_le_bytes(dv_data[4..12].try_into().unwrap());
                let bitmap_len = dv_data.len().saturating_sub(12);
                let bitmap = dv_data[12..12 + bitmap_len].to_vec();
                return Some(crate::DeleteVector { bitmap, cardinality, timestamp });
            }
        }
        None
    }
}

/// Size from which `read_file` memory-maps instead of reading the file into a buffer.
const MMAP_MIN_BYTES: u64 = 8 * 1024 * 1024;

fn with_file_bytes<T>(path: &std::path::Path, f: impl FnOnce(&[u8]) -> Result<T, KoreError>) -> Result<T, KoreError> {
    let mut file = std::fs::File::open(path).map_err(io_err)?;
    let len = file.metadata().map_err(io_err)?.len();
    if len < MMAP_MIN_BYTES {
        let mut buf = Vec::with_capacity(len as usize + 1);
        file.read_to_end(&mut buf).map_err(io_err)?;
        f(&buf)
    } else {
        let mmap = unsafe { Mmap::map(&file).map_err(io_err)? };
        f(&mmap)
    }
}

fn strip_readable_trailer(data: &[u8]) -> &[u8] {
    match find_footer_prefix(data) {
        Some(footer_start) => {
            let digits_start = footer_start + READABLE_FOOTER_PREFIX.len();
            let digits_end = digits_start.saturating_add(20);
            if digits_end > data.len() {
                return data;
            }
            let trailer_len = std::str::from_utf8(&data[digits_start..digits_end])
                .ok()
                .and_then(|s| s.parse::<usize>().ok());
            match trailer_len.and_then(|len| footer_start.checked_sub(len)) {
                Some(binary_end) => &data[..binary_end],
                None => data,
            }
        }
        None => data,
    }
}

fn find_footer_prefix(data: &[u8]) -> Option<usize> {
    if data.len() < READABLE_FOOTER_PREFIX.len() {
        return None;
    }
    // last occurrence (same result as `windows(..).rposition(..)`), vectorised: files without a text
    // trailer used to pay a byte-by-byte scan of the whole file here
    memchr::memmem::rfind(data, READABLE_FOOTER_PREFIX)
}

fn decode_column(raw: &[u8], dtype: DType, comp: Compression, n: usize) -> Result<ColumnData, String> {
    // If LZ4-compressed: first byte is the original compression type, rest is LZ4 data
    let (raw, comp) = if comp == Compression::Lz4 {
        if raw.is_empty() { return Err("empty LZ4 block".into()); }
        let inner_comp = Compression::try_from(raw[0])
            .map_err(|e| format!("LZ4 inner comp: {e}"))?;
        let body = &raw[1..];
        if body.len() < 4 { return Err("truncated LZ4 block".into()); }
        let claimed = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        // LZ4 cannot expand more than ~255:1; reject larger claims before allocating.
        if claimed > (body.len() - 4).saturating_mul(256) + 1024 {
            return Err("LZ4 block claims implausible decompressed size".into());
        }
        let decompressed = lz4_flex::decompress_size_prepended(body)
            .map_err(|e| format!("LZ4 decompress: {e}"))?;
        return decode_column(&decompressed, dtype, inner_comp, n);
    } else if comp == Compression::ZstdShuffle {
        if raw.len() < 2 { return Err("truncated shuffled ZSTD block".into()); }
        let inner_comp = Compression::try_from(raw[0])
            .map_err(|e| format!("ZSTD-shuffle inner comp: {e}"))?;
        if matches!(inner_comp, Compression::Lz4 | Compression::Zstd | Compression::ZstdShuffle) {
            return Err("invalid nested compression".into());
        }
        let stride = raw[1] as usize;
        if !(2..=16).contains(&stride) { return Err("invalid shuffle stride".into()); }
        let planes = compress::zstd_decode_all(&raw[2..], decoded_size_hint(dtype, inner_comp, n))
            .map_err(|e| format!("ZSTD decompress: {e}"))?;
        if planes.len() % stride != 0 { return Err("shuffled block length not a multiple of stride".into()); }
        return decode_column(&compress::byte_unshuffle(&planes, stride), dtype, inner_comp, n);
    } else if comp == Compression::Zstd {
        if raw.is_empty() { return Err("empty ZSTD block".into()); }
        let inner_comp = Compression::try_from(raw[0])
            .map_err(|e| format!("ZSTD inner comp: {e}"))?;
        let decompressed = compress::zstd_decode_all(&raw[1..], decoded_size_hint(dtype, inner_comp, n))
            .map_err(|e| format!("ZSTD decompress: {e}"))?;
        return decode_column(&decompressed, dtype, inner_comp, n);
    } else {
        (raw, comp)
    };

    let col = match dtype {
        DType::I64 => {
            let vals = match comp {
                Compression::Delta  => compress::delta_decode_i64(raw, n),
                Compression::Rle    => compress::rle_decode_i64(raw, n),
                _                   => {
                    let mut out = Vec::with_capacity(n);
                    match n * 9 <= raw.len() {
                        true => {
                            // Fast path: manually unrolled loop for null-tag + i64
                            let mut i = 0;
                            while i + 9 <= raw.len() && out.len() < n {
                                let is_null = raw[i];
                                let v = i64::from_le_bytes(raw[i+1..i+9].try_into().unwrap());
                                out.push(if is_null == 1 { None } else { Some(v) });
                                i += 9;
                            }
                        }
                        false => {
                            // Fallback for smaller/malformed blocks
                            let mut i = 0;
                            while i + 9 <= raw.len() && out.len() < n {
                                let is_null = raw[i]; i += 1;
                                let v = i64::from_le_bytes(raw[i..i+8].try_into().unwrap()); i += 8;
                                out.push(if is_null == 1 { None } else { Some(v) });
                            }
                        }
                    }
                    out
                }
            };
            ColumnData::Int64(vals)
        }
        DType::F64 => ColumnData::Float64(match comp {
            Compression::Dict   => compress::dict_decode_f64(raw, n),
            Compression::NanRaw => compress::nan_decode_f64(raw, n),
            _                   => compress::raw_decode_f64(raw, n),
        }),
        DType::Bool    => ColumnData::Bool(compress::raw_decode_bool(raw, n)),
        DType::Str     => {
            // If inner compression was Dict, it's strdict-encoded data
            if comp == Compression::Dict {
                let (codes, dict) = compress::decode_strdict(raw, n);
                ColumnData::StrDict { codes, dict }
            } else if comp == Compression::StrLen {
                ColumnData::Str(compress::decode_strs_len(raw))
            } else {
                ColumnData::Str(compress::decode_strs(raw))
            }
        }
        DType::StrDict => {
            let (codes, dict) = compress::decode_strdict(raw, n);
            ColumnData::StrDict { codes, dict }
        }
        DType::Array => {
            // Placeholder: Array decoded as raw bytes (would be structured differently in full impl)
            ColumnData::Str(vec![Some(String::from_utf8_lossy(raw).into_owned())])
        }
        DType::Struct => {
            // Placeholder: Struct decoded as raw bytes (would be structured differently in full impl)
            ColumnData::Str(vec![Some(String::from_utf8_lossy(raw).into_owned())])
        }
    };
    let len = match &col {
        ColumnData::Int64(v) => v.len(),
        ColumnData::Float64(v) => v.len(),
        ColumnData::Bool(v) => v.len(),
        ColumnData::Str(v) => v.len(),
        ColumnData::StrDict { codes, .. } => codes.len(),
    };
    if len != n && !matches!(dtype, DType::Array | DType::Struct) {
        return Err(format!("column has {len} values, expected {n}"));
    }
    Ok(col)
}

/// Lower bound of a column's encoded size before the ZSTD stage, used only to pre-size the output
/// buffer (never trusted for bounds; the decoder caps it relative to the compressed size).
fn decoded_size_hint(dtype: DType, comp: Compression, n: usize) -> usize {
    match (dtype, comp) {
        (DType::I64, Compression::Delta) | (DType::I64, Compression::Raw) => n.saturating_mul(9),
        (DType::F64, Compression::NanRaw) => n.saturating_mul(8),
        (DType::F64, Compression::Raw) => n.saturating_mul(9),
        (DType::Bool, _) => n,
        (DType::Str, Compression::Raw) => n.saturating_mul(5).saturating_add(8),
        (DType::Str, Compression::StrLen) => n.saturating_mul(5).saturating_add(4),
        (DType::Str, Compression::Dict) | (DType::StrDict, _) | (DType::F64, Compression::Dict) => n.saturating_add(2),
        _ => 0,
    }
}

fn io_err<E: std::fmt::Display>(e: E) -> KoreError {
    KoreError::InvalidArgument(format!("io: {}", e))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KoreWriter;
    use kore_core::{Column, ColumnData, DataBlock};

    fn sample_block() -> DataBlock {
        DataBlock {
            num_rows: 4,
            columns: vec![
                Column { name: "id".into(),    data: ColumnData::Int64(vec![Some(1),Some(2),Some(3),Some(4)]) },
                Column { name: "score".into(), data: ColumnData::Float64(vec![Some(1.1),Some(2.2),None,Some(4.4)]) },
                Column { name: "pass".into(),  data: ColumnData::Bool(vec![Some(true),Some(false),None,Some(true)]) },
                Column { name: "name".into(),  data: ColumnData::Str(vec![Some("Alice".into()),Some("Bob".into()),None,Some("Dave".into())]) },
            ],
        }
    }

    #[test]
    fn roundtrip_bytes() {
        let orig  = sample_block();
        let bytes = KoreWriter::to_bytes(&orig);
        let back  = KoreReader::from_bytes(&bytes).unwrap();
        assert_eq!(back.num_rows, orig.num_rows);
        assert_eq!(back.columns.len(), orig.columns.len());
        for (a, b) in orig.columns.iter().zip(back.columns.iter()) {
            assert_eq!(a.name, b.name, "column name mismatch");
            match (&a.data, &b.data) {
                (ColumnData::Int64(av),   ColumnData::Int64(bv))   => assert_eq!(av, bv),
                (ColumnData::Float64(av), ColumnData::Float64(bv)) => {
                    for (x, y) in av.iter().zip(bv.iter()) {
                        match (x, y) {
                            (None, None)         => {}
                            (Some(a), Some(b))   => assert!((a - b).abs() < 1e-10),
                            _ => panic!("null mismatch"),
                        }
                    }
                }
                (ColumnData::Bool(av), ColumnData::Bool(bv)) => assert_eq!(av, bv),
                (ColumnData::Str(av),  ColumnData::Str(bv))  => assert_eq!(av, bv),
                _ => panic!("dtype mismatch"),
            }
        }
    }

    #[test]
    fn roundtrip_file() {
        let orig  = sample_block();
        let path  = std::env::temp_dir().join("kore_test.kore");
        KoreWriter::write_file(&path, &orig).unwrap();
        let back  = KoreReader::read_file(&path).unwrap();
        assert_eq!(back.num_rows, orig.num_rows);
        std::fs::remove_file(path).ok();
    }
}

