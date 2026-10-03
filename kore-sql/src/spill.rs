//! Spill-to-disk support for the KQL executor.
//!
//! * a small, exact (bit-preserving) binary serializer for `DataBlock`
//! * a self-cleaning temp directory
//! * a size estimator used to decide when an operator must spill
//! * counters so callers/tests can observe that a spill really happened

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use kore_core::{Column, ColumnData, DataBlock, KoreError};

/// Counters describing how often operators spilled (shared by all clones of a context).
#[derive(Debug, Default)]
pub struct SpillStats {
    pub sort_spills:  AtomicUsize,
    pub group_spills: AtomicUsize,
    pub join_spills:  AtomicUsize,
    pub bytes_written: AtomicU64,
    pub files_created: AtomicUsize,
}

/// Plain-value snapshot of [`SpillStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpillStatsSnapshot {
    pub sort_spills: usize,
    pub group_spills: usize,
    pub join_spills: usize,
    pub bytes_written: u64,
    pub files_created: usize,
}

impl SpillStats {
    pub fn snapshot(&self) -> SpillStatsSnapshot {
        SpillStatsSnapshot {
            sort_spills:   self.sort_spills.load(Ordering::Relaxed),
            group_spills:  self.group_spills.load(Ordering::Relaxed),
            join_spills:   self.join_spills.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            files_created: self.files_created.load(Ordering::Relaxed),
        }
    }
}

/// Estimated in-memory footprint of a block, in bytes (approximate, deterministic).
pub fn estimate_bytes(b: &DataBlock) -> usize {
    let mut total = 0usize;
    for c in &b.columns {
        total += c.name.len() + 24;
        total += match &c.data {
            ColumnData::Int64(v)   => v.len() * std::mem::size_of::<Option<i64>>(),
            ColumnData::Float64(v) => v.len() * std::mem::size_of::<Option<f64>>(),
            ColumnData::Bool(v)    => v.len(),
            ColumnData::Str(v)     => v.iter()
                .map(|s| std::mem::size_of::<Option<String>>() + s.as_ref().map_or(0, |s| s.len()))
                .sum::<usize>(),
            ColumnData::StrDict { codes, dict } =>
                codes.len() + dict.iter().map(|s| s.len() + 24).sum::<usize>(),
        };
    }
    total
}

/// Temp directory that removes itself (and everything in it) on drop.
pub struct SpillDir {
    path: PathBuf,
    next: usize,
    stats: Arc<SpillStats>,
}

static DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl SpillDir {
    pub fn create(base: Option<&Path>, stats: Arc<SpillStats>) -> Result<Self, KoreError> {
        let base = base.map(|p| p.to_path_buf()).unwrap_or_else(std::env::temp_dir);
        let unique = format!(
            "kore-sql-spill-{}-{}-{}",
            std::process::id(),
            DIR_COUNTER.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
        );
        let path = base.join(unique);
        fs::create_dir_all(&path)?;
        Ok(Self { path, next: 0, stats })
    }

    pub fn path(&self) -> &Path { &self.path }

    /// Create a new, empty spill file and return a counting writer for it.
    pub fn new_file(&mut self) -> Result<SpillWriter, KoreError> {
        let p = self.path.join(format!("part-{}.spill", self.next));
        self.next += 1;
        let f = File::create(&p)?;
        self.stats.files_created.fetch_add(1, Ordering::Relaxed);
        Ok(SpillWriter { path: p, w: BufWriter::new(f), stats: self.stats.clone(), pages: 0 })
    }
}

impl Drop for SpillDir {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.path); }
}

/// Appends length-delimited pages (DataBlocks) to a spill file.
pub struct SpillWriter {
    path: PathBuf,
    w: BufWriter<File>,
    stats: Arc<SpillStats>,
    pages: usize,
}

impl SpillWriter {
    pub fn write_page(&mut self, b: &DataBlock) -> Result<(), KoreError> {
        let mut buf = Vec::with_capacity(estimate_bytes(b) / 2 + 64);
        encode_block(&mut buf, b);
        self.w.write_all(&(buf.len() as u64).to_le_bytes())?;
        self.w.write_all(&buf)?;
        self.stats.bytes_written.fetch_add(buf.len() as u64 + 8, Ordering::Relaxed);
        self.pages += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<SpillFile, KoreError> {
        self.w.flush()?;
        Ok(SpillFile { path: self.path, pages: self.pages })
    }
}

/// A completed spill file.
pub struct SpillFile {
    path: PathBuf,
    pub pages: usize,
}

impl SpillFile {
    pub fn reader(&self) -> Result<SpillReader, KoreError> {
        Ok(SpillReader { r: BufReader::new(File::open(&self.path)?) })
    }

    /// Read every page and concatenate into one block; `template` supplies the schema
    /// when the file has no pages.
    pub fn read_all(&self, template: &DataBlock) -> Result<DataBlock, KoreError> {
        let mut rd = self.reader()?;
        let mut blocks = Vec::new();
        while let Some(b) = rd.next_page()? { blocks.push(b); }
        if blocks.is_empty() { return Ok(template.select_rows(&[])); }
        if blocks.len() == 1 { return Ok(blocks.pop().unwrap()); }
        DataBlock::concat(blocks)
    }
}

pub struct SpillReader { r: BufReader<File> }

impl SpillReader {
    pub fn next_page(&mut self) -> Result<Option<DataBlock>, KoreError> {
        let mut len = [0u8; 8];
        match self.r.read_exact(&mut len) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let n = u64::from_le_bytes(len) as usize;
        let mut buf = vec![0u8; n];
        self.r.read_exact(&mut buf)?;
        decode_block(&buf).map(Some)
    }
}

// ─── Binary codec ────────────────────────────────────────────────────────────

fn put_u32(b: &mut Vec<u8>, v: u32) { b.extend_from_slice(&v.to_le_bytes()); }
fn put_u64(b: &mut Vec<u8>, v: u64) { b.extend_from_slice(&v.to_le_bytes()); }
fn put_str(b: &mut Vec<u8>, s: &str) { put_u32(b, s.len() as u32); b.extend_from_slice(s.as_bytes()); }

pub fn encode_block(out: &mut Vec<u8>, blk: &DataBlock) {
    put_u32(out, blk.columns.len() as u32);
    put_u64(out, blk.num_rows as u64);
    for c in &blk.columns {
        put_str(out, &c.name);
        match &c.data {
            ColumnData::Int64(v) => {
                out.push(0);
                put_u64(out, v.len() as u64);
                for x in v {
                    match x { Some(i) => { out.push(1); out.extend_from_slice(&i.to_le_bytes()); }
                              None    => { out.push(0); out.extend_from_slice(&0i64.to_le_bytes()); } }
                }
            }
            ColumnData::Float64(v) => {
                out.push(1);
                put_u64(out, v.len() as u64);
                for x in v {
                    match x { Some(f) => { out.push(1); out.extend_from_slice(&f.to_bits().to_le_bytes()); }
                              None    => { out.push(0); out.extend_from_slice(&0u64.to_le_bytes()); } }
                }
            }
            ColumnData::Bool(v) => {
                out.push(2);
                put_u64(out, v.len() as u64);
                for x in v { out.push(match x { None => 2, Some(false) => 0, Some(true) => 1 }); }
            }
            ColumnData::Str(v) => {
                out.push(3);
                put_u64(out, v.len() as u64);
                for x in v {
                    match x { Some(s) => { out.push(1); put_str(out, s); }
                              None    => out.push(0) }
                }
            }
            ColumnData::StrDict { codes, dict } => {
                out.push(4);
                put_u64(out, codes.len() as u64);
                put_u32(out, dict.len() as u32);
                for s in dict { put_str(out, s); }
                out.extend_from_slice(codes);
            }
        }
    }
}

struct Cur<'a> { b: &'a [u8], p: usize }

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], KoreError> {
        if self.p + n > self.b.len() {
            return Err(KoreError::InvalidArgument("corrupt spill page".into()));
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, KoreError> { Ok(self.take(1)?[0]) }
    fn u32(&mut self) -> Result<u32, KoreError> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn u64(&mut self) -> Result<u64, KoreError> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    fn i64(&mut self) -> Result<i64, KoreError> { Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    fn string(&mut self) -> Result<String, KoreError> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| KoreError::InvalidArgument("corrupt spill string".into()))
    }
}

pub fn decode_block(buf: &[u8]) -> Result<DataBlock, KoreError> {
    let mut c = Cur { b: buf, p: 0 };
    let ncols = c.u32()? as usize;
    let num_rows = c.u64()? as usize;
    let mut columns = Vec::with_capacity(ncols);
    for _ in 0..ncols {
        let name = c.string()?;
        let tag = c.u8()?;
        let n = c.u64()? as usize;
        let data = match tag {
            0 => {
                let mut v = Vec::with_capacity(n);
                for _ in 0..n { let ok = c.u8()?; let x = c.i64()?; v.push(if ok == 1 { Some(x) } else { None }); }
                ColumnData::Int64(v)
            }
            1 => {
                let mut v = Vec::with_capacity(n);
                for _ in 0..n { let ok = c.u8()?; let x = f64::from_bits(c.u64()?); v.push(if ok == 1 { Some(x) } else { None }); }
                ColumnData::Float64(v)
            }
            2 => {
                let mut v = Vec::with_capacity(n);
                for _ in 0..n { v.push(match c.u8()? { 0 => Some(false), 1 => Some(true), _ => None }); }
                ColumnData::Bool(v)
            }
            3 => {
                let mut v = Vec::with_capacity(n);
                for _ in 0..n { v.push(if c.u8()? == 1 { Some(c.string()?) } else { None }); }
                ColumnData::Str(v)
            }
            4 => {
                let nd = c.u32()? as usize;
                let mut dict = Vec::with_capacity(nd);
                for _ in 0..nd { dict.push(c.string()?); }
                let codes = c.take(n)?.to_vec();
                ColumnData::StrDict { codes, dict }
            }
            t => return Err(KoreError::InvalidArgument(format!("bad spill column tag {t}"))),
        };
        columns.push(Column { name, data });
    }
    Ok(DataBlock { columns, num_rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_roundtrip_all_types() {
        let b = DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "i".into(), data: ColumnData::Int64(vec![Some(i64::MIN), None, Some(7)]) },
                Column { name: "f".into(), data: ColumnData::Float64(vec![Some(f64::NAN), None, Some(-0.0)]) },
                Column { name: "b".into(), data: ColumnData::Bool(vec![Some(true), None, Some(false)]) },
                Column { name: "s".into(), data: ColumnData::Str(vec![Some("".into()), None, Some("h\u{e9}llo".into())]) },
                Column { name: "d".into(), data: ColumnData::StrDict { codes: vec![0, u8::MAX, 1], dict: vec!["a".into(), "b".into()] } },
            ],
        };
        let mut buf = Vec::new();
        encode_block(&mut buf, &b);
        let r = decode_block(&buf).unwrap();
        assert_eq!(r.num_rows, 3);
        // NaN != NaN under PartialEq, so compare float column by bits.
        for (x, y) in b.columns.iter().zip(&r.columns) {
            assert_eq!(x.name, y.name);
            match (&x.data, &y.data) {
                (ColumnData::Float64(p), ColumnData::Float64(q)) => {
                    let pb: Vec<_> = p.iter().map(|v| v.map(f64::to_bits)).collect();
                    let qb: Vec<_> = q.iter().map(|v| v.map(f64::to_bits)).collect();
                    assert_eq!(pb, qb);
                }
                (p, q) => assert_eq!(p, q),
            }
        }
    }
}
