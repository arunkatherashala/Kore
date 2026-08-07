//! KoreReader — deserialize bytes / a file back into a DataBlock.

use std::io::Read;
use kore_core::{Column, ColumnData, DataBlock, KoreError};
use crate::{Compression, DType, MAGIC, compress};
use memmap2::Mmap;

const READABLE_FOOTER_PREFIX: &[u8] = b"KORE-READABLE-FOOTER trailer_len=";

pub struct KoreReader;

impl KoreReader {
    /// Parse a DataBlock from a byte slice with zero-copy metadata parsing.
    pub fn from_bytes(data: &[u8]) -> Result<DataBlock, KoreError> {
        let binary_data = strip_readable_trailer(data);
        if binary_data.len() < 18 { // 4+2+4+8
            return Err(KoreError::InvalidArgument("file too small".into()));
        }

        let mut pos = 0;

        // ── Header ────────────────────────────────────────────────────────
        if &binary_data[pos..pos+4] != MAGIC {
            return Err(KoreError::InvalidArgument("invalid KORE magic bytes".into()));
        }
        pos += 4;
        let version = u16::from_le_bytes(binary_data[pos..pos+2].try_into().unwrap());
        pos += 2;
        if version != crate::VERSION && version != 1 {
            return Err(KoreError::InvalidArgument(format!("unsupported version {version}")));
        }
        let num_cols = u32::from_le_bytes(binary_data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        let num_rows = u64::from_le_bytes(binary_data[pos..pos+8].try_into().unwrap()) as usize;
        pos += 8;

        // ── Schema ────────────────────────────────────────────────────────
        let mut schema: Vec<(String, DType)> = Vec::with_capacity(num_cols);
        for _ in 0..num_cols {
            let name_len = u16::from_le_bytes(binary_data[pos..pos+2].try_into().unwrap()) as usize;
            pos += 2;
            let name = String::from_utf8(binary_data[pos..pos+name_len].to_vec())
                .map_err(|_| KoreError::InvalidArgument("invalid UTF-8 column name".into()))?;
            pos += name_len;
            let dtype_byte = binary_data[pos];
            pos += 1;
            let dtype = DType::try_from(dtype_byte)?;
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
            let comp_byte = binary_data[pos];
            pos += 1;
            let comp = Compression::try_from(comp_byte)?;
            let data_len = u64::from_le_bytes(binary_data[pos..pos+8].try_into().unwrap()) as usize;
            pos += 8;
            let raw = &binary_data[pos..pos+data_len];
            pos += data_len;
            chunks.push(ColChunk { name, dtype, comp, raw });
        }

        let columns: Result<Vec<Column>, String> = chunks.into_par_iter()
            .enumerate()
            .map(|(i, chunk)| {
                let col_data = decode_column(chunk.raw, chunk.dtype, chunk.comp, num_rows)
                    .map_err(|e| format!("col {}: {}", i, e))?;
                Ok(Column { name: chunk.name, data: col_data })
            })
            .collect();

        Ok(DataBlock { 
            columns: columns.map_err(|e| KoreError::InvalidArgument(e))?, 
            num_rows 
        })
    }

    /// Read from any `Read` source (no zero-copy slicing).
    pub fn read_from(r: &mut dyn Read) -> Result<DataBlock, KoreError> {
        let mut bytes = Vec::new();
        r.read_to_end(&mut bytes).map_err(io_err)?;
        Self::from_bytes(&bytes)
    }

    /// High Performance: Use Memory Mapped I/O for zero-copy file bridge.
    pub fn read_file(path: &std::path::Path) -> Result<DataBlock, KoreError> {
        let file = std::fs::File::open(path).map_err(io_err)?;
        let mmap = unsafe { Mmap::map(&file).map_err(io_err)? };
        Self::from_bytes(&mmap)
    }

    /// Time travel: Read a specific version by timestamp (MVCC).
    pub fn read_at_version(data: &[u8], _target_timestamp: u64) -> Result<DataBlock, KoreError> {
        // Extract version snapshots from footer, find matching timestamp, read that version
        // For now: returns current version (latest)
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
        // Extract delete vector from footer metadata
        // For now: None (no deleted rows)
        None
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
    data.windows(READABLE_FOOTER_PREFIX.len())
        .rposition(|window| window == READABLE_FOOTER_PREFIX)
}

fn decode_column(raw: &[u8], dtype: DType, comp: Compression, n: usize) -> Result<ColumnData, String> {
    // If LZ4-compressed: first byte is the original compression type, rest is LZ4 data
    let (raw, comp) = if comp == Compression::Lz4 {
        if raw.is_empty() { return Err("empty LZ4 block".into()); }
        let inner_comp = Compression::try_from(raw[0])
            .map_err(|e| format!("LZ4 inner comp: {e}"))?;
        let decompressed = lz4_flex::decompress_size_prepended(&raw[1..])
            .map_err(|e| format!("LZ4 decompress: {e}"))?;
        return decode_column(&decompressed, dtype, inner_comp, n);
    } else if comp == Compression::Zstd {
        if raw.is_empty() { return Err("empty ZSTD block".into()); }
        let inner_comp = Compression::try_from(raw[0])
            .map_err(|e| format!("ZSTD inner comp: {e}"))?;
        let decompressed = compress::zstd_decode(&raw[1..], n * 16); // estimate max size
        return decode_column(&decompressed, dtype, inner_comp, n);
    } else {
        (raw, comp)
    };

    Ok(match dtype {
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
        DType::Str     => ColumnData::Str(compress::decode_strs(raw)),
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
    })
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

