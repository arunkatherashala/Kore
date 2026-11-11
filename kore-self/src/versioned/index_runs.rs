//! Track D1c (design C, `docs/d1c_design.md`): on-disk index runs, so that the open of a long
//! format-5 log reads the MANIFEST, the listed runs and the tail, not every segment.
//!
//! A run is an immutable file `index/<from>-<to>.krun` holding the index delta of versions
//! `from..=to` as seven sorted tables of fixed-width records (32-byte key, u64, u32) plus a blob area
//! per table. The complete set of current runs is listed (field `index_runs`, hashed) by the commit
//! that wrote them; each listing names the file's sha256, its length and the MANIFEST hash of entry
//! `to` (`chain`). Lookups binary-search a run with one positioned read of at most two 4 KB pages,
//! after an in-memory bloom filter and fence keys; the tail since the newest run is the ordinary
//! in-memory `Index`, overlaid on the runs.
//!
//! Trust rule (verified prefix, owner decision D-1): a run is used only if its listing is in the
//! hash chain, its `chain` equals the current hash of entry `to` (a `forget` that re-seals `to`
//! makes it stale), and the whole file hashes to the listed sha256 at open. Every 4 KB page of a run
//! is hashed at open and each later page read is checked against that digest. Segments of versions
//! <= the newest used run are not all re-hashed at open: 64 of them, chosen by a PRNG seeded from
//! the head hash, are; every read of a segment and `verify` check all of them as before.
//!
//! Crash safety: a run is written to `<file>.tmp`, fsynced, renamed, the directory synced; it is
//! used only once a later MANIFEST line lists it. An unlisted file is ignored and removed by
//! `recover` (and by the next `maintain_index_runs`).

use super::manifest::{Entry, RunRef};
use super::*;
use sha2::{Digest, Sha256};
use std::io::{BufWriter, Read, Seek, SeekFrom};

pub const RUN_DIR: &str = "index";
pub const RUN_MAGIC: &[u8; 8] = b"KORERUN1";
/// Version of the run file layout. An unknown version is never used (full open instead).
pub const RUN_FORMAT: u32 = 1;
/// D-3: at most this many runs; when there are more, the two oldest are merged.
pub const MAX_RUNS: usize = 8;
/// D-3: a new run when this many versions ...
pub const EVERY_VERSIONS: u64 = 2_000;
/// ... or this many events have passed since the newest run.
pub const EVERY_EVENTS: u64 = 100_000;
/// D-1: old segments hashed at every fast open.
pub const SAMPLE_SEGMENTS: usize = 64;

const PAGE: usize = 4096;
const REC: usize = 44;
const PER_FENCE: u64 = (PAGE / REC) as u64;
const NT: usize = 7;
const HDR: usize = 8 + 4 + 4 + 8 + 8 + 8 + 32 + NT * 32;
pub(super) const T_EV: usize = 0;
pub(super) const T_EVO: usize = 1;
pub(super) const T_REV: usize = 2;
pub(super) const T_HEAD: usize = 3;
pub(super) const T_CONT: usize = 4;
pub(super) const T_PRED: usize = 5;
pub(super) const T_OUT: usize = 6;
/// On the same key in two runs the newer one wins (heads, revisions, predictions), else the older one
/// (event ordinals and "first revision with this content" keep the first).
const NEWER_WINS: [bool; NT] = [false, false, true, true, false, true, false];
/// Every table but `T_EV` keeps the original key strings (and the value) in its blob.
const HAS_BLOB: [bool; NT] = [false, true, true, true, true, true, true];

pub fn run_file(from: u64, to: u64) -> String {
    format!("{RUN_DIR}/{from:010}-{to:010}.krun")
}

type Rec = ([u8; 32], u64, Vec<u8>);

fn sha_raw(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}
fn key_str(s: &str) -> [u8; 32] {
    codec::hex64_to_bytes(s).unwrap_or_else(|| sha_raw(&[b"k1|", s.as_bytes()]))
}
fn key_odd(s: &str) -> [u8; 32] {
    sha_raw(&[b"odd1|", s.as_bytes()])
}
fn key_head(t: &str, k: &str) -> [u8; 32] {
    sha_raw(&[b"head1|", t.as_bytes(), &[0x1f], k.as_bytes()])
}
fn enc(parts: &[&str]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&(p.len() as u32).to_le_bytes());
        v.extend_from_slice(p.as_bytes());
    }
    v
}
fn dec(b: &[u8]) -> Option<Vec<String>> {
    let mut out = vec![];
    let mut i = 0usize;
    while i < b.len() {
        let n = u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize;
        i += 4;
        out.push(String::from_utf8(b.get(i..i + n)?.to_vec()).ok()?);
        i += n;
    }
    Some(out)
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().expect("8 bytes"))
}
fn page_digest(b: &[u8]) -> [u8; 16] {
    let d = sha_raw(&[b]);
    d[..16].try_into().expect("16")
}

fn pread(f: &fs::File, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, off)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0usize;
        while done < buf.len() {
            let n = f.seek_read(&mut buf[done..], off + done as u64)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            done += n;
        }
        Ok(())
    }
}

struct Bloom {
    bits: Vec<u64>,
    m: u64,
}
impl Bloom {
    fn new(n: u64) -> Bloom {
        let m = (n.saturating_mul(10)).max(64).div_ceil(64) * 64;
        Bloom { bits: vec![0; (m / 64) as usize], m }
    }
    fn probes(&self, k: &[u8; 32]) -> impl Iterator<Item = u64> + '_ {
        let h1 = u64::from_le_bytes(k[0..8].try_into().expect("8"));
        let h2 = u64::from_le_bytes(k[8..16].try_into().expect("8")) | 1;
        (0..7u64).map(move |i| h1.wrapping_add(i.wrapping_mul(h2)) % self.m)
    }
    fn insert(&mut self, k: &[u8; 32]) {
        let p: Vec<u64> = self.probes(k).collect();
        for b in p {
            self.bits[(b / 64) as usize] |= 1 << (b % 64);
        }
    }
    fn maybe(&self, k: &[u8; 32]) -> bool {
        self.probes(k).all(|b| self.bits[(b / 64) as usize] & (1 << (b % 64)) != 0)
    }
}

struct Tab {
    count: u64,
    rec_off: u64,
    blob_off: u64,
    blob_len: u64,
    fence: Vec<[u8; 32]>,
    bloom: Bloom,
}

/// One verified, opened run.
pub(super) struct Run {
    pub r: RunRef,
    /// Event rows in versions `0..=to`.
    pub next_ord: u64,
    file: fs::File,
    len: u64,
    pages: Vec<[u8; 16]>,
    tabs: Vec<Tab>,
}

/// The runs in use, oldest first (contiguous from version 0).
#[derive(Default)]
pub(super) struct RunSet {
    pub runs: Vec<Arc<Run>>,
}

fn bad(r: &RunRef, d: impl Into<String>) -> VerifyError {
    VerifyError::SegmentUnreadable { file: r.file.clone(), detail: d.into() }
}

/// sha256 (hex), length and 4 KB page digests of a file, read once, sequentially.
fn hash_file(path: &Path, file: &str) -> Result<(String, u64, Vec<[u8; 16]>), VerifyError> {
    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(VerifyError::MissingSegment { file: file.to_string() }),
        Err(e) => return Err(VerifyError::Io(format!("{file}: {e}"))),
    };
    let mut h = Sha256::new();
    let mut pages = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut len = 0u64;
    let mut carry: Vec<u8> = Vec::with_capacity(PAGE);
    loop {
        let n = f.read(&mut buf).map_err(|e| VerifyError::Io(format!("{file}: {e}")))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        len += n as u64;
        let mut s = &buf[..n];
        while !s.is_empty() {
            let take = (PAGE - carry.len()).min(s.len());
            carry.extend_from_slice(&s[..take]);
            s = &s[take..];
            if carry.len() == PAGE {
                pages.push(page_digest(&carry));
                carry.clear();
            }
        }
    }
    if !carry.is_empty() {
        pages.push(page_digest(&carry));
    }
    count_io(len);
    Ok((hex(&h.finalize()), len, pages))
}

impl Run {
    /// Open a listed run: the whole file must hash to the listing, the header must agree with it and
    /// every table must be strictly sorted with in-range values. Builds fences and bloom filters.
    pub fn load(dir: &Path, r: &RunRef) -> Result<Run, VerifyError> {
        let path = dir.join(&r.file);
        let (sha, len, pages) = hash_file(&path, &r.file)?;
        if sha != r.sha256 || len != r.bytes {
            return Err(VerifyError::ChecksumMismatch { file: r.file.clone() });
        }
        let file = fs::File::open(&path).map_err(|e| VerifyError::Io(format!("{}: {e}", r.file)))?;
        let mut run = Run { r: r.clone(), next_ord: 0, file, len, pages, tabs: vec![] };
        if (len as usize) < HDR {
            return Err(bad(r, "shorter than the run header"));
        }
        let h = run.read_at(0, HDR)?;
        if &h[0..8] != RUN_MAGIC {
            return Err(bad(r, "not an index run (magic)"));
        }
        let fmt = u32::from_le_bytes(h[8..12].try_into().expect("4"));
        if fmt != RUN_FORMAT {
            return Err(bad(r, format!("unknown run format {fmt}")));
        }
        if u32::from_le_bytes(h[12..16].try_into().expect("4")) as usize != NT {
            return Err(bad(r, "wrong table count"));
        }
        let (from, to, next_ord) = (u64_at(&h, 16), u64_at(&h, 24), u64_at(&h, 32));
        if from != r.from || to != r.to || hex(&h[40..72]) != r.chain {
            return Err(bad(r, "header does not match its listing"));
        }
        run.next_ord = next_ord;
        let mut at = HDR as u64;
        for t in 0..NT {
            let b = 72 + t * 32;
            let (count, rec_off, blob_off, blob_len) = (u64_at(&h, b), u64_at(&h, b + 8), u64_at(&h, b + 16), u64_at(&h, b + 24));
            let rec_end = count.checked_mul(REC as u64).and_then(|x| x.checked_add(rec_off));
            if rec_off != at || rec_end != Some(blob_off) || blob_off.checked_add(blob_len).map_or(true, |e| e > len) {
                return Err(bad(r, format!("table {t}: bad layout")));
            }
            at = blob_off + blob_len;
            run.tabs.push(Tab { count, rec_off, blob_off, blob_len, fence: vec![], bloom: Bloom::new(count) });
        }
        if at != len {
            return Err(bad(r, "trailing bytes after the last table"));
        }
        // pass 2: structure, fences, bloom filters
        for t in 0..NT {
            let (count, rec_off, blob_len) = (run.tabs[t].count, run.tabs[t].rec_off, run.tabs[t].blob_len);
            let mut prev: Option<[u8; 32]> = None;
            let mut want_off = 0u64;
            let mut fence = Vec::with_capacity((count / PER_FENCE + 1) as usize);
            let mut bloom = Bloom::new(count);
            let mut i = 0u64;
            while i < count {
                let n = (count - i).min(16_384);
                let recs = run.read_at(rec_off + i * REC as u64, (n as usize) * REC)?;
                for j in 0..n as usize {
                    let rb = &recs[j * REC..(j + 1) * REC];
                    let k: [u8; 32] = rb[..32].try_into().expect("32");
                    let a = u64_at(rb, 32);
                    let b = u32::from_le_bytes(rb[40..44].try_into().expect("4")) as u64;
                    if prev.map_or(false, |p| p >= k) {
                        return Err(bad(r, format!("table {t}: keys not strictly ascending")));
                    }
                    if HAS_BLOB[t] {
                        if a != want_off || a + b > blob_len {
                            return Err(bad(r, format!("table {t}: blob reference out of order or range")));
                        }
                        want_off = a + b;
                    } else if b != 0 || a >= next_ord {
                        return Err(bad(r, "event ordinal out of range"));
                    }
                    if (i + j as u64) % PER_FENCE == 0 {
                        fence.push(k);
                    }
                    bloom.insert(&k);
                    prev = Some(k);
                }
                i += n;
            }
            if HAS_BLOB[t] && want_off != blob_len {
                return Err(bad(r, format!("table {t}: blob area not fully referenced")));
            }
            run.tabs[t].fence = fence;
            run.tabs[t].bloom = bloom;
        }
        Ok(run)
    }

    /// Bytes `off..off+n` of the file; every 4 KB page touched is checked against its digest from the open.
    fn read_at(&self, off: u64, n: usize) -> Result<Vec<u8>, VerifyError> {
        if n == 0 {
            return Ok(vec![]);
        }
        let end = off + n as u64;
        if end > self.len {
            return Err(bad(&self.r, "read past the end"));
        }
        let p0 = off / PAGE as u64;
        let p1 = (end - 1) / PAGE as u64;
        let start = p0 * PAGE as u64;
        let stop = ((p1 + 1) * PAGE as u64).min(self.len);
        let mut buf = vec![0u8; (stop - start) as usize];
        pread(&self.file, start, &mut buf).map_err(|e| VerifyError::Io(format!("{}: {e}", self.r.file)))?;
        count_io(buf.len() as u64);
        for (k, p) in (p0..=p1).enumerate() {
            let s = k * PAGE;
            let e = (s + PAGE).min(buf.len());
            if page_digest(&buf[s..e]) != self.pages[p as usize] {
                return Err(VerifyError::ChecksumMismatch { file: self.r.file.clone() });
            }
        }
        let a = (off - start) as usize;
        Ok(buf[a..a + n].to_vec())
    }

    /// (a, b) of the record with this key in table `t`, if any.
    fn get(&self, t: usize, key: &[u8; 32]) -> Result<Option<(u64, u64)>, VerifyError> {
        let tb = &self.tabs[t];
        if tb.count == 0 || !tb.bloom.maybe(key) {
            return Ok(None);
        }
        let idx = tb.fence.partition_point(|f| f <= key);
        if idx == 0 {
            return Ok(None);
        }
        let i0 = (idx as u64 - 1) * PER_FENCE;
        let n = (tb.count - i0).min(PER_FENCE) as usize;
        let recs = self.read_at(tb.rec_off + i0 * REC as u64, n * REC)?;
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let k = &recs[mid * REC..mid * REC + 32];
            match k.cmp(&key[..]) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let rb = &recs[mid * REC..(mid + 1) * REC];
                    return Ok(Some((u64_at(rb, 32), u32::from_le_bytes(rb[40..44].try_into().expect("4")) as u64)));
                }
            }
        }
        Ok(None)
    }

    fn blob(&self, t: usize, a: u64, b: u64) -> Result<Vec<String>, VerifyError> {
        let bytes = self.read_at(self.tabs[t].blob_off + a, b as usize)?;
        dec(&bytes).ok_or_else(|| bad(&self.r, "blob does not decode"))
    }

    fn scan(&self, t: usize) -> Scan<'_> {
        Scan { run: self, t, i: 0, recs: vec![], rec_i0: 0, blob: vec![], blob0: 0 }
    }
}

/// Sequential read of one table of a run, in key order (merges, `materialize`).
struct Scan<'a> {
    run: &'a Run,
    t: usize,
    i: u64,
    recs: Vec<u8>,
    rec_i0: u64,
    blob: Vec<u8>,
    blob0: u64,
}
impl<'a> Iterator for Scan<'a> {
    type Item = Result<Rec, VerifyError>;
    fn next(&mut self) -> Option<Self::Item> {
        let tb = &self.run.tabs[self.t];
        if self.i >= tb.count {
            return None;
        }
        let r = (|| {
            let mut local = (self.i - self.rec_i0) as usize;
            if self.recs.is_empty() || local * REC >= self.recs.len() {
                let n = (tb.count - self.i).min(16_384) as usize;
                self.recs = self.run.read_at(tb.rec_off + self.i * REC as u64, n * REC)?;
                self.rec_i0 = self.i;
                local = 0;
            }
            let rb = &self.recs[local * REC..(local + 1) * REC];
            let k: [u8; 32] = rb[..32].try_into().expect("32");
            let a = u64_at(rb, 32);
            let b = u32::from_le_bytes(rb[40..44].try_into().expect("4")) as u64;
            let mut bytes = vec![];
            if HAS_BLOB[self.t] {
                if a < self.blob0 || a + b > self.blob0 + self.blob.len() as u64 {
                    let end = (a + b).max(a + (4 << 20)).min(tb.blob_len);
                    self.blob = self.run.read_at(tb.blob_off + a, (end - a) as usize)?;
                    self.blob0 = a;
                }
                let s = (a - self.blob0) as usize;
                bytes = self.blob[s..s + b as usize].to_vec();
            }
            Ok((k, if HAS_BLOB[self.t] { 0 } else { a }, bytes))
        })();
        self.i = if r.is_ok() { self.i + 1 } else { tb.count };
        Some(r)
    }
}

/// Two sorted streams (older, newer) merged by the rule of the table.
struct Merge<'a> {
    a: std::iter::Peekable<Scan<'a>>,
    b: std::iter::Peekable<Scan<'a>>,
    newer_wins: bool,
}
impl<'a> Iterator for Merge<'a> {
    type Item = Result<Rec, VerifyError>;
    fn next(&mut self) -> Option<Self::Item> {
        let ka = match self.a.peek() {
            Some(Ok(r)) => Some(r.0),
            Some(Err(_)) => return self.a.next(),
            None => None,
        };
        let kb = match self.b.peek() {
            Some(Ok(r)) => Some(r.0),
            Some(Err(_)) => return self.b.next(),
            None => None,
        };
        match (ka, kb) {
            (None, None) => None,
            (Some(_), None) => self.a.next(),
            (None, Some(_)) => self.b.next(),
            (Some(x), Some(y)) if x < y => self.a.next(),
            (Some(x), Some(y)) if x > y => self.b.next(),
            _ => {
                let (ra, rb) = (self.a.next(), self.b.next());
                if self.newer_wins {
                    rb
                } else {
                    ra
                }
            }
        }
    }
}

fn hdr_bytes(from: u64, to: u64, chain: &str, next_ord: u64, tabs: &[(u64, u64, u64, u64)]) -> Vec<u8> {
    let mut h = Vec::with_capacity(HDR);
    h.extend_from_slice(RUN_MAGIC);
    h.extend_from_slice(&RUN_FORMAT.to_le_bytes());
    h.extend_from_slice(&(NT as u32).to_le_bytes());
    for x in [from, to, next_ord] {
        h.extend_from_slice(&x.to_le_bytes());
    }
    h.extend_from_slice(&codec::hex64_to_bytes(chain).unwrap_or([0; 32]));
    for t in 0..NT {
        let x = tabs.get(t).copied().unwrap_or_default();
        for v in [x.0, x.1, x.2, x.3] {
            h.extend_from_slice(&v.to_le_bytes());
        }
    }
    debug_assert_eq!(h.len(), HDR);
    h
}

/// Test hook: where a run write "crashes".
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RunCrash {
    /// The temp file is written but not renamed.
    BeforeRename,
    /// The run file is in place but no MANIFEST line lists it.
    AfterRename,
    /// The listing commit is written, the old (merged) runs are not deleted.
    AfterCommit,
}

fn io_err(e: std::io::Error) -> LogError {
    LogError::Io(e.to_string())
}

/// Write one run (temp + fsync + rename + directory sync). Returns its listing.
fn write_run(dir: &Path, from: u64, to: u64, chain: &str, next_ord: u64, tables: Vec<Box<dyn Iterator<Item = Result<Rec, VerifyError>> + '_>>, crash: Option<RunCrash>) -> Result<RunRef, LogError> {
    let rdir = dir.join(RUN_DIR);
    fs::create_dir_all(&rdir)?;
    let rel = run_file(from, to);
    let tmp = dir.join(format!("{rel}.tmp"));
    let side = dir.join(format!("{rel}.blob.tmp"));
    let f = fs::File::create(&tmp)?;
    let mut w = BufWriter::with_capacity(1 << 20, f);
    w.write_all(&[0u8; HDR])?;
    let mut pos = HDR as u64;
    let mut metas = vec![];
    for (t, it) in tables.into_iter().enumerate() {
        let mut sw = BufWriter::with_capacity(1 << 20, fs::File::create(&side)?);
        let (rec_off, mut count, mut blob_len) = (pos, 0u64, 0u64);
        let mut prev: Option<[u8; 32]> = None;
        for item in it {
            let (k, a, blob) = item?;
            if prev.map_or(false, |p| p >= k) {
                return Err(LogError::Io(format!("internal: index run table {t} not strictly sorted")));
            }
            prev = Some(k);
            w.write_all(&k)?;
            if HAS_BLOB[t] {
                w.write_all(&blob_len.to_le_bytes())?;
                w.write_all(&(blob.len() as u32).to_le_bytes())?;
                sw.write_all(&blob)?;
                blob_len += blob.len() as u64;
            } else {
                w.write_all(&a.to_le_bytes())?;
                w.write_all(&0u32.to_le_bytes())?;
            }
            count += 1;
        }
        sw.flush()?;
        drop(sw);
        let mut sf = fs::File::open(&side)?;
        std::io::copy(&mut sf, &mut w)?;
        drop(sf);
        let blob_off = rec_off + count * REC as u64;
        metas.push((count, rec_off, blob_off, blob_len));
        pos = blob_off + blob_len;
    }
    let _ = fs::remove_file(&side);
    let mut f = w.into_inner().map_err(|e| io_err(e.into_error()))?;
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&hdr_bytes(from, to, chain, next_ord, &metas))?;
    f.sync_all()?;
    drop(f);
    let (sha, bytes, _) = hash_file(&tmp, &rel)?;
    if crash == Some(RunCrash::BeforeRename) {
        return Err(LogError::Io("simulated crash".into()));
    }
    fs::rename(&tmp, dir.join(&rel))?;
    sync_dir(&rdir);
    Ok(RunRef { from, to, file: rel, sha256: sha, bytes, chain: chain.to_string() })
}

/// The tail index as sorted run records, one Vec per table.
fn tail_records(ix: &Index) -> Vec<Vec<Rec>> {
    let mut t: Vec<Vec<Rec>> = (0..NT).map(|_| vec![]).collect();
    t[T_EV] = ix.events.iter().map(|(k, &o)| (*k, o as u64, vec![])).collect();
    t[T_EVO] = ix.events_odd.iter().map(|(s, &o)| (key_odd(s), 0, enc(&[s, &o.to_string()]))).collect();
    t[T_REV] = ix.revs.iter().map(|(id, (et, k))| (key_str(id), 0, enc(&[id, et, k]))).collect();
    t[T_HEAD] = ix.heads.iter().map(|((et, k), (r, d))| (key_head(et, k), 0, enc(&[et, k, r, d]))).collect();
    t[T_CONT] = ix.content_ids.iter().map(|(d, r)| (key_str(d), 0, enc(&[d, r]))).collect();
    t[T_PRED] = ix.preds.iter().map(|(id, o)| (key_str(id), 0, enc(&[id, &serde_json::to_string(o).expect("json")]))).collect();
    t[T_OUT] = ix.outcomes.iter().map(|id| (key_str(id), 0, enc(&[id]))).collect();
    for v in t.iter_mut() {
        v.sort_by(|a, b| a.0.cmp(&b.0));
    }
    t
}

impl RunSet {
    /// The decoded fields (and `a`) of the winning record of table `t` whose leading fields equal `want`.
    fn find(&self, t: usize, key: &[u8; 32], want: &[&str]) -> Result<Option<(u64, Vec<String>)>, VerifyError> {
        let order: Vec<&Arc<Run>> = if NEWER_WINS[t] { self.runs.iter().rev().collect() } else { self.runs.iter().collect() };
        for r in order {
            if let Some((a, b)) = r.get(t, key)? {
                if !HAS_BLOB[t] {
                    return Ok(Some((a, vec![])));
                }
                let f = r.blob(t, a, b)?;
                if f.len() >= want.len() && f.iter().zip(want).all(|(x, y)| x == y) {
                    return Ok(Some((a, f)));
                }
            }
        }
        Ok(None)
    }
    pub fn last(&self) -> Option<&Arc<Run>> {
        self.runs.last()
    }
}

// ─── The overlay: tail `Index` on top of the runs ────────────────────────────

impl Index {
    fn base_ord(&self, id: &str) -> Result<Option<u32>, VerifyError> {
        let Some(b) = &self.base else { return Ok(None) };
        let (t, k) = match codec::hex64_to_bytes(id) {
            Some(raw) => (T_EV, raw),
            None => (T_EVO, key_odd(id)),
        };
        if t == T_EV {
            return Ok(b.find(T_EV, &k, &[])?.map(|x| x.0 as u32));
        }
        // EVO: record a = blob offset; the ordinal is the second field of a scan only, so look it up
        for r in &b.runs {
            if let Some((a, len)) = r.get(T_EVO, &k)? {
                let f = r.blob(T_EVO, a, len)?;
                if f.first().map(|s| s.as_str()) == Some(id) {
                    if let Some(o) = f.get(1).and_then(|s| s.parse::<u32>().ok()) {
                        return Ok(Some(o));
                    }
                }
            }
        }
        Ok(None)
    }
    pub(super) fn ev_ord(&self, id: &str) -> Result<Option<u32>, VerifyError> {
        let tail = match codec::hex64_to_bytes(id) {
            Some(b) => self.events.get(&b).copied(),
            None => self.events_odd.get(id).copied(),
        };
        match tail {
            Some(o) => Ok(Some(o)),
            None => self.base_ord(id),
        }
    }
    pub(super) fn ev_has(&self, id: &str) -> Result<bool, VerifyError> {
        Ok(self.ev_ord(id)?.is_some())
    }
    /// `ev_add` that also treats an id stored in a run as present (keeps the first ordinal).
    pub(super) fn ev_add_o(&mut self, k: IdKey) -> Result<bool, VerifyError> {
        if self.base.is_some() {
            let s = match &k {
                IdKey::Hex(b) => hex(b),
                IdKey::Other(s) => s.clone(),
            };
            if self.base_ord(&s)?.is_some() {
                self.next_ord += 1;
                return Ok(false);
            }
        }
        Ok(self.ev_add(k))
    }
    pub(super) fn rev_get(&self, id: &str) -> Result<Option<(String, String)>, VerifyError> {
        if let Some(v) = self.revs.get(id) {
            return Ok(Some(v.clone()));
        }
        let Some(b) = &self.base else { return Ok(None) };
        Ok(b.find(T_REV, &key_str(id), &[id])?.and_then(|(_, f)| (f.len() == 3).then(|| (f[1].clone(), f[2].clone()))))
    }
    pub(super) fn rev_has(&self, id: &str) -> Result<bool, VerifyError> {
        Ok(self.rev_get(id)?.is_some())
    }
    pub(super) fn head_get(&self, hk: &(String, String)) -> Result<Option<(String, String)>, VerifyError> {
        if let Some(v) = self.heads.get(hk) {
            return Ok(Some(v.clone()));
        }
        let Some(b) = &self.base else { return Ok(None) };
        Ok(b.find(T_HEAD, &key_head(&hk.0, &hk.1), &[&hk.0, &hk.1])?.and_then(|(_, f)| (f.len() == 4).then(|| (f[2].clone(), f[3].clone()))))
    }
    pub(super) fn content_get(&self, d: &str) -> Result<Option<String>, VerifyError> {
        if let Some(v) = self.content_ids.get(d) {
            return Ok(Some(v.clone()));
        }
        let Some(b) = &self.base else { return Ok(None) };
        Ok(b.find(T_CONT, &key_str(d), &[d])?.and_then(|(_, f)| f.get(1).cloned()))
    }
    pub(super) fn pred_get(&self, id: &str) -> Result<Option<Vec<String>>, VerifyError> {
        if let Some(v) = self.preds.get(id) {
            return Ok(Some(v.clone()));
        }
        let Some(b) = &self.base else { return Ok(None) };
        Ok(b.find(T_PRED, &key_str(id), &[id])?.and_then(|(_, f)| f.get(1).map(|j| serde_json::from_str(j).unwrap_or_default())))
    }
    pub(super) fn pred_has(&self, id: &str) -> Result<bool, VerifyError> {
        Ok(self.pred_get(id)?.is_some())
    }
    pub(super) fn outcome_has(&self, id: &str) -> Result<bool, VerifyError> {
        if self.outcomes.contains(id) {
            return Ok(true);
        }
        let Some(b) = &self.base else { return Ok(false) };
        Ok(b.find(T_OUT, &key_str(id), &[id])?.is_some())
    }
    /// `add_rev` against the overlay (content digest: the first revision keeps it).
    fn add_rev_o(&mut self, rv: RevisionRow, mut c: Vec<String>) -> Result<(), VerifyError> {
        c.sort();
        let digest = revision_content(&rv.entity_type, &rv.key, &rv.stance, &format!("{:?}", rv.confidence), &rv.rationale, &rv.valid_from, &rv.author, &c);
        let fresh = self.content_get(&digest)?.is_none();
        self.revs.insert(rv.rev_id.clone(), (rv.entity_type.clone(), rv.key.clone()));
        if fresh {
            self.content_ids.insert(digest.clone(), rv.rev_id.clone());
        }
        self.heads.insert((rv.entity_type, rv.key), (rv.rev_id, digest));
        Ok(())
    }

    /// The whole index (runs merged, tail overlaid) as plain maps: `verify` and tests.
    pub(super) fn materialize(&self) -> Result<Index, VerifyError> {
        let mut ix = Index { next_ord: self.next_ord, content_idem: self.content_idem, forgotten: self.forgotten.clone(), ..Index::default() };
        if let Some(b) = &self.base {
            for r in &b.runs {
                for t in 0..NT {
                    for rec in r.scan(t) {
                        let (k, a, blob) = rec?;
                        let f = if HAS_BLOB[t] { dec(&blob).ok_or_else(|| bad(&r.r, "blob does not decode"))? } else { vec![] };
                        let need = [0, 2, 3, 4, 2, 2, 1][t];
                        if f.len() < need {
                            return Err(bad(&r.r, format!("table {t}: short record")));
                        }
                        match t {
                            T_EV => {
                                ix.events.entry(k).or_insert(a as u32);
                            }
                            T_EVO => {
                                let o = f[1].parse::<u32>().map_err(|_| bad(&r.r, "odd event ordinal"))?;
                                ix.events_odd.entry(f[0].clone()).or_insert(o);
                            }
                            T_REV => {
                                ix.revs.insert(f[0].clone(), (f[1].clone(), f[2].clone()));
                            }
                            T_HEAD => {
                                ix.heads.insert((f[0].clone(), f[1].clone()), (f[2].clone(), f[3].clone()));
                            }
                            T_CONT => {
                                ix.content_ids.entry(f[0].clone()).or_insert(f[1].clone());
                            }
                            T_PRED => {
                                ix.preds.insert(f[0].clone(), serde_json::from_str(&f[1]).unwrap_or_default());
                            }
                            _ => {
                                ix.outcomes.insert(f[0].clone());
                            }
                        }
                    }
                }
            }
        }
        for (k, v) in &self.events {
            ix.events.entry(*k).or_insert(*v);
        }
        for (k, v) in &self.events_odd {
            ix.events_odd.entry(k.clone()).or_insert(*v);
        }
        for (k, v) in &self.content_ids {
            ix.content_ids.entry(k.clone()).or_insert(v.clone());
        }
        ix.revs.extend(self.revs.clone());
        ix.heads.extend(self.heads.clone());
        ix.preds.extend(self.preds.clone());
        ix.outcomes.extend(self.outcomes.iter().cloned());
        Ok(ix)
    }
}

// ─── Fast open ────────────────────────────────────────────────────────────────

/// How the log was opened (Track D1c counters for the measurement).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpenReport {
    /// The caller asked for the fast open (`fast_open = on`).
    pub fast_requested: bool,
    /// The fast path was used (false: the full open, as before D1c).
    pub fast_used: bool,
    /// Why the fast path was not used (None when it was, or when it was not requested).
    pub fallback_reason: Option<String>,
    pub runs_used: usize,
    /// Listed runs not used because a `forget` re-sealed their last version.
    pub runs_stale: usize,
    /// Versions after the newest used run (read and verified at open).
    pub tail_versions: u64,
    /// Old segments hashed by the D-1 sample.
    pub sampled_segments: usize,
    /// Files and bytes read by this open (process-wide counters; exact when nothing else reads logs meanwhile).
    pub files_read: u64,
    pub bytes_read: u64,
}

/// Options of `VersionedLog::open_with` (all off = the open as before D1c).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OpenOptions {
    /// Use the listed index runs of a format-5 log (falls back to the full open on any problem).
    pub fast_open: bool,
}

fn splitmix(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// D-1: the (entry, segment) pairs of `entries[..=upto]` hashed at a fast open, seeded from `head`.
pub(super) fn sample_segments(entries: &[Entry], upto: usize, head: &str) -> Vec<(usize, usize)> {
    let all = all_segs(&entries[..=upto]);
    if all.len() <= SAMPLE_SEGMENTS {
        return all;
    }
    let mut seed = codec::hex64_to_bytes(head).map(|b| u64::from_le_bytes(b[..8].try_into().expect("8"))).unwrap_or(0);
    let mut picked = BTreeMap::new();
    while picked.len() < SAMPLE_SEGMENTS {
        let i = (splitmix(&mut seed) % all.len() as u64) as usize;
        picked.insert(i, all[i]);
    }
    picked.into_values().collect()
}

/// The active listing: the run set of the newest entry that lists runs.
pub(super) fn active_listing(entries: &[Entry]) -> &[RunRef] {
    entries.iter().rev().find(|e| !e.index_runs.is_empty()).map(|e| e.index_runs.as_slice()).unwrap_or(&[])
}

fn events_through(entries: &[Entry], upto: usize) -> u64 {
    entries[..=upto].iter().flat_map(|e| e.segments.iter()).filter(|g| g.table == "events").map(|g| g.rows).sum()
}

/// The chain-valid prefix of the active listing (runs whose `to` still has the recorded hash), loaded.
/// Err: a chain-valid run failed to load (tampered, missing, unknown layout).
pub(super) fn usable_runs(dir: &Path, entries: &[Entry]) -> Result<(Vec<Arc<Run>>, usize), VerifyError> {
    let listing = active_listing(entries);
    let mut out = vec![];
    for (i, r) in listing.iter().enumerate() {
        if r.to as usize >= entries.len() || entries[r.to as usize].hash != r.chain {
            return Ok((out, listing.len() - i));
        }
        let run = Run::load(dir, r)?;
        if run.next_ord != events_through(entries, r.to as usize) {
            return Err(bad(r, "event count does not match the MANIFEST"));
        }
        out.push(Arc::new(run));
    }
    Ok((out, 0))
}

/// The fast open of a format-5 log (design C). Err(reason): the caller does the full open instead.
pub(super) fn fast_state(dir: &Path, entries: &[Entry], level: u8) -> Result<(State, OpenReport), String> {
    if level != 4 {
        return Err(format!("not a format-5 log (MANIFEST header level {level})"));
    }
    if active_listing(entries).is_empty() {
        return Err("no index run is listed yet".into());
    }
    let (runs, stale) = usable_runs(dir, entries).map_err(|e| format!("index run rejected: {e}"))?;
    let Some(last) = runs.last().cloned() else {
        return Err(format!("no listed index run matches the current hash chain ({stale} stale, re-sealed by forget)"));
    };
    let t_end = last.r.to as usize;
    let head = entries.last().map(|e| e.hash.clone()).unwrap_or_default();
    let sample = sample_segments(entries, t_end, &head);
    let checked: Vec<Result<Vec<u8>, VerifyError>> = sample.par_iter().map(|&(ei, si)| read_verified(dir, &entries[ei].segments[si])).collect();
    for c in checked {
        c.map_err(|e| format!("sampled old segment failed: {e}"))?;
    }
    let dicts = v4log::load_dicts(dir, entries).map_err(|e| format!("dictionary: {e}"))?;
    let mut meta: Vec<Vec<Cell>> = vec![];
    for e in &entries[..=t_end] {
        for g in e.segments.iter().filter(|g| g.table == "schema_meta") {
            meta.extend(load_seg(dir, g, &dicts).map_err(|e| e.to_string())?);
        }
    }
    let mut ix = Index { base: Some(Arc::new(RunSet { runs: runs.clone() })), next_ord: last.next_ord as u32, ..Index::default() };
    // the tail, folded as the streamed open does (a revision's causes are in its own commit)
    let segs: Vec<(usize, usize)> = all_segs(entries).into_iter().filter(|&(ei, _)| ei > t_end).collect();
    let mut cur = usize::MAX;
    let mut revs: Vec<RevisionRow> = vec![];
    let mut causes: HashMap<String, Vec<String>> = HashMap::new();
    fn flush(ix: &mut Index, revs: &mut Vec<RevisionRow>, causes: &mut HashMap<String, Vec<String>>) -> Result<bool, VerifyError> {
        for rv in revs.drain(..) {
            if ix.rev_has(&rv.rev_id)? {
                return Ok(false);
            }
            let c = causes.remove(&rv.rev_id).unwrap_or_default();
            ix.add_rev_o(rv, c)?;
        }
        Ok(causes.is_empty())
    }
    let irregular = || "irregular tail (a revision's causes outside its commit, or a revision stored twice)".to_string();
    for chunk in segs.chunks(READ_CHUNK) {
        let parts: Vec<Result<Part, VerifyError>> = chunk
            .par_iter()
            .map(|&(ei, si)| {
                let g = &entries[ei].segments[si];
                if g.table == "events" && ids_only(g) {
                    load_ids(dir, g).map(Part::Ids)
                } else {
                    load_seg(dir, g, &dicts).map(Part::Rows)
                }
            })
            .collect();
        for (&(ei, si), r) in chunk.iter().zip(parts) {
            if ei != cur {
                if !flush(&mut ix, &mut revs, &mut causes).map_err(|e| e.to_string())? {
                    return Err(irregular());
                }
                cur = ei;
            }
            let t = seg_table(&entries[ei].segments[si]).map_err(|e| e.to_string())?;
            let ids: Vec<IdKey> = match (r.map_err(|e| e.to_string())?, t) {
                (Part::Ids(v), _) => v,
                (Part::Rows(rows), Table::Events) => rows.iter().map(|r| IdKey::from_str(&r[0].s())).collect(),
                (Part::Rows(rows), Table::Revisions) => {
                    revs.extend(rows.iter().map(|r| RevisionRow::from_cells(r)));
                    continue;
                }
                (Part::Rows(rows), Table::RevisionCauses) => {
                    for r in &rows {
                        causes.entry(r[0].s()).or_default().push(r[1].s());
                    }
                    continue;
                }
                (Part::Rows(rows), Table::Predictions) => {
                    for r in &rows {
                        ix.add_pred(r);
                    }
                    continue;
                }
                (Part::Rows(rows), Table::Outcomes) => {
                    for r in &rows {
                        ix.outcomes.insert(r[0].s());
                    }
                    continue;
                }
                (Part::Rows(rows), Table::SchemaMeta) => {
                    meta.extend(rows);
                    continue;
                }
            };
            for k in ids {
                ix.ev_add_o(k).map_err(|e| e.to_string())?;
            }
        }
    }
    if !flush(&mut ix, &mut revs, &mut causes).map_err(|e| e.to_string())? {
        return Err(irregular());
    }
    let format = meta.iter().map(|r| LogFormat::from_number(MetaRow::from_cells(r).format_version)).fold(LogFormat::V1, |a, b| if b.number() > a.number() { b } else { a });
    if format != LogFormat::V5 {
        return Err(format!("schema_meta says format {}, not 5", format.number()));
    }
    let mut ev_segs = vec![];
    let mut cum = 0u64;
    for (ei, e) in entries.iter().enumerate() {
        for (si, g) in e.segments.iter().enumerate() {
            if g.table == "events" {
                ev_segs.push((cum as u32, ei, si));
                cum += g.rows;
            }
        }
    }
    if cum != ix.next_ord as u64 {
        return Err("event count of the tail does not match the MANIFEST".into());
    }
    ix.forgotten = forget::forgotten_set(entries);
    ix.content_idem = true;
    let rep = OpenReport { fast_used: true, runs_used: runs.len(), runs_stale: stale, tail_versions: (entries.len() - 1 - t_end) as u64, sampled_segments: sample.len(), ..Default::default() };
    Ok((State { head: None, index: ix, ev_segs, format, dicts }, rep))
}

/// `verify` part for runs: every chain-valid listed run verifies and, merged, equals the index rebuilt
/// from the segments up to the newest of them. Stale runs (re-sealed by forget) are not an error.
pub(super) fn verify_runs(dir: &Path, entries: &[Entry]) -> Result<(), VerifyError> {
    let listing = active_listing(entries);
    if listing.is_empty() {
        return Ok(());
    }
    let (runs, _) = usable_runs(dir, entries)?;
    let Some(last) = runs.last().cloned() else { return Ok(()) };
    let upto = last.r.to as usize;
    let mut st = load_state(dir, &entries[..=upto])?;
    let merged = Index { base: Some(Arc::new(RunSet { runs })), next_ord: last.next_ord as u32, ..Index::default() }.materialize()?;
    st.index.forgotten.clear();
    st.index.content_idem = false;
    if !merged.same_plain(&st.index) {
        return Err(bad(&last.r, "index runs differ from the index rebuilt from the segments"));
    }
    Ok(())
}

/// Run files on disk (in `index/`) that the active listing does not name (and temp files).
pub(super) fn stray_runs(dir: &Path, entries: &[Entry]) -> Vec<String> {
    let known: HashSet<&str> = active_listing(entries).iter().map(|r| r.file.as_str()).collect();
    let mut out = vec![];
    if let Ok(rd) = fs::read_dir(dir.join(RUN_DIR)) {
        for f in rd.flatten() {
            let rel = format!("{RUN_DIR}/{}", f.file_name().to_string_lossy());
            if !known.contains(rel.as_str()) {
                out.push(rel);
            }
        }
    }
    out
}

/// Chain-level rules for run listings (canonical names, ranges, contiguity, hex hashes).
pub(super) fn check_run_listings(entries: &[Entry]) -> Result<(), VerifyError> {
    let is_hex = |s: &str| codec::hex64_to_bytes(s).is_some();
    for e in entries.iter().filter(|e| !e.index_runs.is_empty()) {
        let mut next = 0u64;
        for r in &e.index_runs {
            let ok = r.from == next && r.from <= r.to && r.to < e.v && r.file == run_file(r.from, r.to) && is_hex(&r.sha256) && is_hex(&r.chain);
            if !ok {
                return Err(VerifyError::ChainBroken { version: e.v, detail: format!("bad index run listing {}", r.file) });
            }
            next = r.to + 1;
        }
    }
    Ok(())
}

impl Index {
    /// Equality of the plain maps (no runs), ignoring `forgotten` / `content_idem`.
    pub(super) fn same_plain(&self, o: &Index) -> bool {
        self.events == o.events && self.events_odd == o.events_odd && self.next_ord == o.next_ord && self.revs == o.revs && self.heads == o.heads && self.content_ids == o.content_ids && self.preds == o.preds && self.outcomes == o.outcomes
    }
}

/// What `maintain_index_runs` did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunsReport {
    /// Why nothing was written (not due, not format 5).
    pub skipped: Option<String>,
    pub written: Option<RunRef>,
    pub merged: Option<RunRef>,
    /// Version of the commit that lists the new run set.
    pub listed_at: Option<u64>,
    pub runs: usize,
    pub deleted: usize,
}

impl VersionedLog {
    /// How this handle was opened (Track D1c).
    pub fn open_report(&self) -> &OpenReport {
        &self.open_info
    }

    /// The run set listed by the newest listing entry (oldest first).
    pub fn index_runs(&self) -> Vec<RunRef> {
        active_listing(&self.entries).to_vec()
    }

    /// Track D1c: write a new index run when one is due (D-3: no usable run in this handle, or
    /// `EVERY_VERSIONS` versions / `EVERY_EVENTS` events since the newest), merge the two oldest when
    /// there are more than `MAX_RUNS`, and list the new set in one commit without rows. Format 5 only.
    /// Unlisted run files are deleted afterwards. A failure leaves the log valid (no listing = not used).
    pub fn maintain_index_runs(&mut self) -> Result<RunsReport, LogError> {
        self.maintain_runs_inner(None, EVERY_VERSIONS, EVERY_EVENTS)
    }

    pub(crate) fn maintain_runs_inner(&mut self, crash: Option<RunCrash>, every_versions: u64, every_events: u64) -> Result<RunsReport, LogError> {
        let mut rep = RunsReport::default();
        if self.format != LogFormat::V5 {
            rep.skipped = Some(format!("format {} (index runs need format 5, see upgrade_to_v5)", self.format.number()));
            return Ok(rep);
        }
        let latest = self.latest_version();
        let base: Vec<Arc<Run>> = self.index.base.as_ref().map(|b| b.runs.clone()).unwrap_or_default();
        let (from, base_next) = match base.last() {
            Some(r) => (r.r.to + 1, r.next_ord),
            None => (0, 0),
        };
        let due = base.is_empty() || latest + 1 - from >= every_versions || (self.index.next_ord as u64).saturating_sub(base_next) >= every_events;
        if !due || from > latest {
            rep.skipped = Some(format!("not due ({} versions and {} events since the newest run)", latest + 1 - from, self.index.next_ord as u64 - base_next));
            rep.runs = base.len();
            return Ok(rep);
        }
        self.check_manifest_unchanged()?;
        let chain = self.entries[latest as usize].hash.clone();
        let recs = tail_records(&self.index);
        let tables: Vec<Box<dyn Iterator<Item = Result<Rec, VerifyError>>>> = recs.into_iter().map(|v| Box::new(v.into_iter().map(Ok)) as Box<dyn Iterator<Item = Result<Rec, VerifyError>>>).collect();
        let r = write_run(&self.dir, from, latest, &chain, self.index.next_ord as u64, tables, crash)?;
        rep.written = Some(r.clone());
        let mut set: Vec<Arc<Run>> = base.clone();
        set.push(Arc::new(Run::load(&self.dir, &r)?));
        if set.len() > MAX_RUNS {
            let (a, b) = (set[0].clone(), set[1].clone());
            let tables: Vec<Box<dyn Iterator<Item = Result<Rec, VerifyError>> + '_>> = (0..NT).map(|t| Box::new(Merge { a: a.scan(t).peekable(), b: b.scan(t).peekable(), newer_wins: NEWER_WINS[t] }) as Box<dyn Iterator<Item = Result<Rec, VerifyError>>>).collect();
            let m = write_run(&self.dir, a.r.from, b.r.to, &b.r.chain, b.next_ord, tables, None)?;
            rep.merged = Some(m.clone());
            let mr = Arc::new(Run::load(&self.dir, &m)?);
            set.splice(0..2, [mr]);
        }
        if crash == Some(RunCrash::AfterRename) {
            return Err(LogError::Io("simulated crash".into()));
        }
        let refs: Vec<RunRef> = set.iter().map(|r| r.r.clone()).collect();
        let last_at = self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
        let mut now = (self.clock)();
        if now < last_at {
            now = last_at;
        }
        self.write_commit_runs(now, refs)?;
        rep.listed_at = Some(self.latest_version());
        rep.runs = set.len();
        // committed: the tail is now in the runs
        let ix = &mut self.index;
        ix.base = Some(Arc::new(RunSet { runs: set }));
        ix.events.clear();
        ix.events_odd.clear();
        ix.revs.clear();
        ix.heads.clear();
        ix.content_ids.clear();
        ix.preds.clear();
        ix.outcomes.clear();
        drop(base);
        if crash == Some(RunCrash::AfterCommit) {
            return Err(LogError::Io("simulated crash".into()));
        }
        for f in stray_runs(&self.dir, &self.entries) {
            if fs::remove_file(self.dir.join(&f)).is_ok() {
                rep.deleted += 1;
            }
        }
        Ok(rep)
    }

    /// Explicit, one-way upgrade to format 5 (from 1, 2 or 3; Track D1c, owner decision D-2): one new
    /// commit records `schema_meta.format_version = 5` and, in the same atomic MANIFEST replacement,
    /// switches the header to `KORE-VLOG-MANIFEST 4` (every earlier entry line byte for byte, so every
    /// old version hash is unchanged). Later commits append and write format-2 segments, and may list
    /// index runs. Older binaries refuse the result. False if already format 5. A format-4 log is
    /// refused (dictionary logs are not covered by D1c).
    pub fn upgrade_to_v5(&mut self) -> Result<bool, LogError> {
        match self.format {
            LogFormat::V5 => return Ok(false),
            LogFormat::V4 => return inv("format-4 logs cannot be upgraded to format 5 (not covered by Track D1c)"),
            _ => {}
        }
        self.check_manifest_unchanged()?;
        let last_at = self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
        let mut now = (self.clock)();
        if now < last_at {
            now = last_at;
        }
        let meta = MetaRow { format_version: FORMAT_V5, created_at: now.clone() };
        self.write_commit(vec![(Table::SchemaMeta, vec![meta.to_cells()])], now, LogFormat::V5, None)?;
        self.format = LogFormat::V5;
        self.index.content_idem = true;
        Ok(true)
    }
}
