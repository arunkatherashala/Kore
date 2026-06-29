//! KoreReader — deserialize bytes / a file back into a DataBlock.

use std::io::{self, Read};
use kore_core::{Column, ColumnData, DataBlock, KoreError};
use crate::{Compression, DType, MAGIC, compress};

pub struct KoreReader;

impl KoreReader {
    /// Parse a DataBlock from a byte slice.
    pub fn from_bytes(data: &[u8]) -> Result<DataBlock, KoreError> {
        let mut r = Cursor::new(data);
        Self::read_from(&mut r)
    }

    /// Read from any `Read` source.
    pub fn read_from(r: &mut dyn Read) -> Result<DataBlock, KoreError> {
        // ── Header ────────────────────────────────────────────────────────
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic).map_err(io_err)?;
        if &magic != MAGIC {
            return Err(KoreError::InvalidArgument("invalid KORE magic bytes".into()));
        }
        let version = read_u16(r)?;
        if version != crate::VERSION {
            return Err(KoreError::InvalidArgument(format!("unsupported version {version}")));
        }
        let num_cols = read_u32(r)? as usize;
        let num_rows = read_u64(r)? as usize;

        // ── Schema ────────────────────────────────────────────────────────
        let mut schema: Vec<(String, DType)> = Vec::with_capacity(num_cols);
        for _ in 0..num_cols {
            let name_len = read_u16(r)? as usize;
            let mut name_bytes = vec![0u8; name_len];
            r.read_exact(&mut name_bytes).map_err(io_err)?;
            let name = String::from_utf8(name_bytes)
                .map_err(|_| KoreError::InvalidArgument("invalid UTF-8 column name".into()))?;
            let dtype_byte = read_u8(r)?;
            let dtype = DType::try_from(dtype_byte)?;
            schema.push((name, dtype));
        }

        // ── Column data ───────────────────────────────────────────────────
        let mut columns: Vec<Column> = Vec::with_capacity(num_cols);
        for (i, (name, dtype)) in schema.into_iter().enumerate() {
            let comp_byte  = read_u8(r)?;
            let comp       = Compression::try_from(comp_byte)?;
            let data_len   = read_u64(r)? as usize;
            let mut raw    = vec![0u8; data_len];
            r.read_exact(&mut raw).map_err(io_err)?;
            let col_data = decode_column(&raw, dtype, comp, num_rows)
                .map_err(|e| KoreError::InvalidArgument(format!("col {}: {}", i, e)))?;
            columns.push(Column { name, data: col_data });
        }

        Ok(DataBlock { columns, num_rows })
    }

    /// Convenience: read from a file path.
    pub fn read_file(path: &std::path::Path) -> Result<DataBlock, KoreError> {
        let bytes = std::fs::read(path).map_err(io_err)?;
        Self::from_bytes(&bytes)
    }
}

fn decode_column(raw: &[u8], dtype: DType, comp: Compression, n: usize) -> Result<ColumnData, String> {
    Ok(match dtype {
        DType::I64 => {
            let vals = match comp {
                Compression::Delta => compress::delta_decode_i64(raw, n),
                Compression::Rle   => compress::rle_decode_i64(raw, n),
                Compression::Raw   => {
                    // raw i64: (null:1)(val:8) × n
                    let mut out = Vec::with_capacity(n);
                    let mut i = 0;
                    while i + 9 <= raw.len() && out.len() < n {
                        let is_null = raw[i]; i += 1;
                        let v = i64::from_le_bytes(raw[i..i+8].try_into().unwrap()); i += 8;
                        out.push(if is_null == 1 { None } else { Some(v) });
                    }
                    out
                }
            };
            ColumnData::Int64(vals)
        }
        DType::F64  => ColumnData::Float64(compress::raw_decode_f64(raw, n)),
        DType::Bool => ColumnData::Bool(compress::raw_decode_bool(raw, n)),
        DType::Str  => ColumnData::Str(compress::decode_strs(raw)),
    })
}

// ─── Cursor + read helpers ────────────────────────────────────────────────────

struct Cursor<'a> { data: &'a [u8], pos: usize }
impl<'a> Cursor<'a> { fn new(data: &'a [u8]) -> Self { Self { data, pos: 0 } } }
impl<'a> Read for Cursor<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

fn io_err<E: std::fmt::Display>(e: E) -> KoreError {
    KoreError::InvalidArgument(format!("io: {}", e))
}
fn read_u8(r: &mut dyn Read)  -> Result<u8,  KoreError> { let mut b = [0u8;1]; r.read_exact(&mut b).map_err(io_err)?; Ok(b[0]) }
fn read_u16(r: &mut dyn Read) -> Result<u16, KoreError> { let mut b = [0u8;2]; r.read_exact(&mut b).map_err(io_err)?; Ok(u16::from_le_bytes(b)) }
fn read_u32(r: &mut dyn Read) -> Result<u32, KoreError> { let mut b = [0u8;4]; r.read_exact(&mut b).map_err(io_err)?; Ok(u32::from_le_bytes(b)) }
fn read_u64(r: &mut dyn Read) -> Result<u64, KoreError> { let mut b = [0u8;8]; r.read_exact(&mut b).map_err(io_err)?; Ok(u64::from_le_bytes(b)) }

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

