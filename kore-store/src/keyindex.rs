//! Key index: a separate, immutable, content-addressed object (`<sha256>.kidx`) that maps the values of
//! one key column (e.g. `event_id`) to the data file that holds them, so a point lookup reads one file
//! instead of scanning the key column of every file. Opt-in; data files are not changed (version 2, 3 and 4
//! files can all be indexed), so every existing file and reader keeps working.
//!
//! ```text
//! "KIDX" | version:u16 = 1 | hash:u8 = 1 (KH64) | flags:u8 = 0
//! column_len:u16 | column                        name of the key column (UTF-8)
//! num_files:u32, per file:
//!   name_len:u16 | name | file_len:u64 | file_sha256:[32] | rows:u32
//! num_keys:u64 | q:u8 | k:u8 | file_bits:u8 | block_keys:u16
//! num_blocks:u32, per block: first:u64 | bit_offset:u64
//! files_len:u64 | file ids                       num_keys values of file_bits bits, LSB first
//! rice_len:u64  | rice stream                    per block: block_keys-1 Rice-coded deltas (parameter k)
//! sha256:[32]                                    of every byte before it = the index id
//! ```
//!
//! Every key is hashed with KH64 (below) and the top `q` bits are kept (`q = ceil(log2(num_keys)) + 8`).
//! The hashes are sorted (ties by file number) and stored as a Golomb-Rice coded set in blocks of
//! `block_keys`: the first hash of each block in the fence table, the following ones as deltas
//! (`unary(d >> k)` as ones then a zero, then the low `k` bits LSB first). Entry `i` has the file number
//! `file ids[i]`. So the index costs about `k + 2 + file_bits` bits per key (about 2.5 bytes per key
//! with 1000 files), not the key itself.
//!
//! Lookup: hash the key, find its block by binary search over the fences, decode that block (and the
//! next while the hash repeats), collect the file numbers of equal hashes. Only those files are read; in
//! each one the key column is decoded and compared with the key, so a hash collision never returns a wrong
//! row. A key that is not in the index finds no equal hash with probability about 1 - 2^-8 (no file is
//! read); otherwise one file is read and the key is not found in it.
//!
//! The rows are not stored: the row is found by scanning the decoded key column of that one file.
//!
//! Integrity: `from_bytes` verifies the whole object (sha256 = id, exact length, all structure) before
//! use. Every data file is pinned by length and sha256; `Verify::Sha256` (default) hashes the file
//! before using it, so a changed, replaced or truncated file is refused ("file changed since the index
//! was built"); `Verify::Length` only checks the length and relies on the file's own checksums.
//!
//! Versioning / time travel: an index object is never modified. A new set of files (a new version of a
//! store) gets a new index object with a new id; older index objects keep pinning the exact file bytes of
//! their version. Names are opaque to the index: the caller resolves them (a path in a directory, or an
//! entry of a version log, see the tests).
//!
//! KH64 (64-bit, not cryptographic; collisions only cost an extra file read):
//! ```text
//! h = 0x9E3779B97F4A7C15 ^ (len * 0xC2B2AE3D27D4EB4F)        (wrapping)
//! for each 8-byte little-endian word w of the key (last word zero-padded):
//!     h = rotl((h ^ w) * 0xFF51AFD7ED558CCD, 29)
//! h ^= h >> 30; h *= 0xBF58476D1CE4E5B9; h ^= h >> 27; h *= 0x94D049BB133111EB; h ^= h >> 31
//! ```

use kore_core::{ColumnData, DataBlock, KoreError};
use sha2::{Digest, Sha256};

use crate::dict::DictSet;
use crate::KoreReader;

pub const INDEX_MAGIC: &[u8; 4] = b"KIDX";
pub const INDEX_VERSION: u16 = 1;
pub const HASH_KH64: u8 = 1;
/// Fingerprint bits beyond log2(num_keys): a missing key reads a file with probability about 2^-8.
pub const FP_BITS: u32 = 8;
pub const BLOCK_KEYS: u16 = 128;
const MAX_KEYS: u64 = 1 << 32;
const MAX_FILES: usize = 1 << 24;

fn bad(m: impl Into<String>) -> KoreError { KoreError::InvalidArgument(m.into()) }

/// The KH64 hash of a key (see the module documentation).
pub fn kh64(key: &[u8]) -> u64 {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15 ^ (key.len() as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let mut chunks = key.chunks_exact(8);
    for c in &mut chunks {
        let w = u64::from_le_bytes(c.try_into().unwrap());
        h = (h ^ w).wrapping_mul(0xFF51_AFD7_ED55_8CCD).rotate_left(29);
    }
    let r = chunks.remainder();
    if !r.is_empty() {
        let mut b = [0u8; 8];
        b[..r.len()].copy_from_slice(r);
        h = (h ^ u64::from_le_bytes(b)).wrapping_mul(0xFF51_AFD7_ED55_8CCD).rotate_left(29);
    }
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

/// A data file covered by an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub name: String,
    pub len: u64,
    pub sha256: [u8; 32],
    pub rows: u32,
}

/// How much of a data file is checked against the index before it is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// length and sha256 of the whole file (default)
    Sha256,
    /// length only (the file's own CRCs still cover what is decoded)
    Length,
}

/// One row found by `KeyIndex::lookup`.
#[derive(Debug, Clone)]
pub struct Hit {
    /// position of the file in `KeyIndex::files()`
    pub file: usize,
    /// row within that file
    pub row: usize,
    /// the requested columns, one row
    pub values: DataBlock,
}

/// A loaded, verified key index (owns its bytes; share it by reference).
#[derive(Clone)]
pub struct KeyIndex {
    bytes: Vec<u8>,
    id: [u8; 32],
    column: String,
    files: Vec<IndexedFile>,
    num_keys: u64,
    q: u32,
    k: u32,
    file_bits: u32,
    block_keys: usize,
    fences: Vec<(u64, u64)>,
    ids_at: usize,
    ids_len: usize,
    rice_at: usize,
    rice_len: usize,
}

impl std::fmt::Debug for KeyIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyIndex(id={}, column={}, {} files, {} keys, {} bytes)", self.id_hex(), self.column, self.files.len(), self.num_keys, self.bytes.len())
    }
}

/// True if `data` starts with the key index magic.
pub fn is_key_index(data: &[u8]) -> bool { data.len() >= 4 && &data[..4] == INDEX_MAGIC }

// ── bit I/O (LSB first) ──────────────────────────────────────────────────────

struct BitW { out: Vec<u8>, acc: u64, n: u32 }
impl BitW {
    fn new() -> Self { BitW { out: vec![], acc: 0, n: 0 } }
    fn bits(&self) -> u64 { self.out.len() as u64 * 8 + self.n as u64 }
    fn put(&mut self, v: u64, nbits: u32) {
        if nbits == 0 { return; }
        let mut v = if nbits == 64 { v } else { v & ((1u64 << nbits) - 1) };
        let mut nb = nbits;
        while nb > 0 {
            let room = 64 - self.n;
            let take = room.min(nb);
            let part = if take == 64 { v } else { v & ((1u64 << take) - 1) };
            self.acc |= part << self.n;
            self.n += take;
            nb -= take;
            v = if take == 64 { 0 } else { v >> take };
            while self.n >= 8 { self.out.push(self.acc as u8); self.acc >>= 8; self.n -= 8; }
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 { self.out.push(self.acc as u8); }
        self.out
    }
}

#[inline]
fn bit(d: &[u8], pos: u64) -> Option<bool> { d.get((pos >> 3) as usize).map(|b| (b >> (pos & 7)) & 1 == 1) }

fn get_bits(d: &[u8], pos: u64, nbits: u32) -> Option<u64> {
    if nbits == 0 { return Some(0); }
    let end = pos.checked_add(nbits as u64)?;
    if end > d.len() as u64 * 8 { return None; }
    let mut v = 0u64;
    let mut got = 0u32;
    let mut p = pos;
    while got < nbits {
        let byte = d[(p >> 3) as usize] as u64;
        let off = (p & 7) as u32;
        let take = (8 - off).min(nbits - got);
        v |= ((byte >> off) & ((1u64 << take) - 1)) << got;
        got += take;
        p += take as u64;
    }
    Some(v)
}

// ── builder ──────────────────────────────────────────────────────────────────

/// Collects the keys of a set of data files and builds a `KeyIndex`.
pub struct KeyIndexBuilder {
    column: String,
    files: Vec<IndexedFile>,
    hashes: Vec<(u64, u32)>,
}

impl KeyIndexBuilder {
    pub fn new(column: &str) -> Self { KeyIndexBuilder { column: column.to_string(), files: vec![], hashes: vec![] } }

    /// Add a data file with its keys as written (the caller has them, e.g. from the block it just wrote).
    /// Null keys are not indexed. `bytes` is the exact file content (pinned by length and sha256).
    pub fn add_file_keys<'a>(&mut self, name: &str, bytes: &[u8], keys: impl IntoIterator<Item = Option<&'a str>>) -> Result<(), KoreError> {
        if name.len() > u16::MAX as usize { return Err(bad("file name too long")); }
        if self.files.iter().any(|f| f.name == name) { return Err(bad(format!("file '{name}' added twice"))); }
        if self.files.len() >= MAX_FILES { return Err(bad("too many files in one index")); }
        let fno = self.files.len() as u32;
        let mut rows: u64 = 0;
        for k in keys {
            rows += 1;
            if let Some(k) = k { self.hashes.push((kh64(k.as_bytes()), fno)); }
        }
        let rows = u32::try_from(rows).map_err(|_| bad("too many rows in one file"))?;
        let sha256: [u8; 32] = Sha256::digest(bytes).into();
        self.files.push(IndexedFile { name: name.to_string(), len: bytes.len() as u64, sha256, rows });
        Ok(())
    }

    /// Add a data file by decoding its key column (any version the reader knows; `dicts` for version 4).
    pub fn add_file_bytes(&mut self, name: &str, bytes: &[u8], dicts: &DictSet) -> Result<(), KoreError> {
        let b = KoreReader::from_bytes_columns_with_dicts(bytes, &[self.column.as_str()], dicts)?;
        let c = &b.columns[0].data;
        if !matches!(c, ColumnData::Str(_) | ColumnData::StrDict { .. }) { return Err(bad(format!("key column '{}' is not a string column", self.column))); }
        let keys: Vec<Option<&str>> = (0..b.num_rows).map(|r| c.get_str(r)).collect();
        self.add_file_keys(name, bytes, keys)
    }

    pub fn build(self) -> Result<KeyIndex, KoreError> {
        let KeyIndexBuilder { column, files, mut hashes } = self;
        if column.len() > u16::MAX as usize { return Err(bad("column name too long")); }
        let n = hashes.len() as u64;
        if n >= MAX_KEYS { return Err(bad("too many keys in one index")); }
        let lg = if n <= 1 { 0 } else { 64 - (n - 1).leading_zeros() };
        let q = (lg + FP_BITS).min(64);
        let shift = 64 - q;
        for h in hashes.iter_mut() { h.0 = if q == 0 { 0 } else { h.0 >> shift }; }
        hashes.sort_unstable();
        let file_bits = if files.len() <= 1 { 0 } else { 32 - ((files.len() - 1) as u32).leading_zeros() };
        let bk = BLOCK_KEYS as usize;
        // Rice parameter minimising the stream size (exact, over all deltas)
        let deltas = |f: &mut dyn FnMut(u64)| {
            for blk in hashes.chunks(bk) { for w in blk.windows(2) { f(w[1].0 - w[0].0); } }
        };
        let mut best_k = 0u32;
        let mut best_bits = u128::MAX;
        for k in 0..=q.min(63) {
            let mut bits: u128 = 0;
            deltas(&mut |d| bits += (d >> k) as u128 + 1 + k as u128);
            if bits < best_bits { best_bits = bits; best_k = k; }
        }
        let k = best_k;
        let mut rice = BitW::new();
        let mut fences = Vec::with_capacity(hashes.len().div_ceil(bk));
        for blk in hashes.chunks(bk) {
            fences.push((blk[0].0, rice.bits()));
            for w in blk.windows(2) {
                let d = w[1].0 - w[0].0;
                let mut u = d >> k;
                while u >= 32 { rice.put(u32::MAX as u64, 32); u -= 32; }
                rice.put((1u64 << u) - 1, u as u32);
                rice.put(0, 1);
                rice.put(d, k);
            }
        }
        let rice = rice.finish();
        let mut ids = BitW::new();
        for h in &hashes { ids.put(h.1 as u64, file_bits); }
        let ids = ids.finish();

        let mut out = Vec::new();
        out.extend_from_slice(INDEX_MAGIC);
        out.extend_from_slice(&INDEX_VERSION.to_le_bytes());
        out.push(HASH_KH64);
        out.push(0);
        out.extend_from_slice(&(column.len() as u16).to_le_bytes());
        out.extend_from_slice(column.as_bytes());
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        for f in &files {
            out.extend_from_slice(&(f.name.len() as u16).to_le_bytes());
            out.extend_from_slice(f.name.as_bytes());
            out.extend_from_slice(&f.len.to_le_bytes());
            out.extend_from_slice(&f.sha256);
            out.extend_from_slice(&f.rows.to_le_bytes());
        }
        out.extend_from_slice(&n.to_le_bytes());
        out.push(q as u8);
        out.push(k as u8);
        out.push(file_bits as u8);
        out.extend_from_slice(&BLOCK_KEYS.to_le_bytes());
        out.extend_from_slice(&(fences.len() as u32).to_le_bytes());
        for (first, off) in &fences { out.extend_from_slice(&first.to_le_bytes()); out.extend_from_slice(&off.to_le_bytes()); }
        out.extend_from_slice(&(ids.len() as u64).to_le_bytes());
        out.extend_from_slice(&ids);
        out.extend_from_slice(&(rice.len() as u64).to_le_bytes());
        out.extend_from_slice(&rice);
        let id: [u8; 32] = Sha256::digest(&out).into();
        out.extend_from_slice(&id);
        KeyIndex::from_bytes(&out)
    }
}

// ── reader ───────────────────────────────────────────────────────────────────

fn take<'a>(d: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], KoreError> {
    let end = pos.checked_add(n).filter(|&e| e <= d.len()).ok_or_else(|| bad("truncated key index"))?;
    let s = &d[*pos..end];
    *pos = end;
    Ok(s)
}
fn u16_at(d: &[u8], p: &mut usize) -> Result<u16, KoreError> { Ok(u16::from_le_bytes(take(d, p, 2)?.try_into().unwrap())) }
fn u32_at(d: &[u8], p: &mut usize) -> Result<u32, KoreError> { Ok(u32::from_le_bytes(take(d, p, 4)?.try_into().unwrap())) }
fn u64_at(d: &[u8], p: &mut usize) -> Result<u64, KoreError> { Ok(u64::from_le_bytes(take(d, p, 8)?.try_into().unwrap())) }

impl KeyIndex {
    /// Parse and verify a serialized index object: magic, version, sha256 over the whole object, exact
    /// length, and every structural rule (fences increasing, file numbers in range, Rice stream decodes
    /// to exactly `num_keys` hashes in order).
    pub fn from_bytes(data: &[u8]) -> Result<KeyIndex, KoreError> {
        if data.len() < 8 || &data[..4] != INDEX_MAGIC { return Err(bad("not a KORE key index object")); }
        let ver = u16::from_le_bytes([data[4], data[5]]);
        if ver != INDEX_VERSION { return Err(bad(format!("unsupported key index version {ver}"))); }
        if data.len() < 8 + 32 { return Err(bad("truncated key index")); }
        let body = &data[..data.len() - 32];
        let want: [u8; 32] = Sha256::digest(body).into();
        if want[..] != data[data.len() - 32..] { return Err(bad("key index checksum (sha256) mismatch")); }
        let mut p = 6usize;
        let hdr = take(body, &mut p, 2)?;
        if hdr[0] != HASH_KH64 { return Err(bad(format!("unknown key index hash {}", hdr[0]))); }
        if hdr[1] != 0 { return Err(bad(format!("unknown key index flags {}", hdr[1]))); }
        let cl = u16_at(body, &mut p)? as usize;
        let column = String::from_utf8(take(body, &mut p, cl)?.to_vec()).map_err(|_| bad("invalid UTF-8 column name"))?;
        let nf = u32_at(body, &mut p)? as usize;
        if nf > MAX_FILES || nf > (body.len() - p) / (2 + 8 + 32 + 4) { return Err(bad("implausible file count")); }
        let mut files = Vec::with_capacity(nf);
        for _ in 0..nf {
            let nl = u16_at(body, &mut p)? as usize;
            let name = String::from_utf8(take(body, &mut p, nl)?.to_vec()).map_err(|_| bad("invalid UTF-8 file name"))?;
            let len = u64_at(body, &mut p)?;
            let sha256: [u8; 32] = take(body, &mut p, 32)?.try_into().unwrap();
            let rows = u32_at(body, &mut p)?;
            if files.iter().any(|f: &IndexedFile| f.name == name) { return Err(bad("duplicate file name in key index")); }
            files.push(IndexedFile { name, len, sha256, rows });
        }
        let num_keys = u64_at(body, &mut p)?;
        let pr = take(body, &mut p, 3)?;
        let (q, k, file_bits) = (pr[0] as u32, pr[1] as u32, pr[2] as u32);
        let block_keys = u16_at(body, &mut p)? as usize;
        if num_keys >= MAX_KEYS { return Err(bad("implausible key count")); }
        if q > 64 || k > 63 || k > q || file_bits > 24 || block_keys < 2 { return Err(bad("invalid key index parameters")); }
        let total_rows: u64 = files.iter().map(|f| f.rows as u64).sum();
        if num_keys > total_rows { return Err(bad("more keys than rows")); }
        let lg = if num_keys <= 1 { 0 } else { 64 - (num_keys - 1).leading_zeros() };
        if q != (lg + FP_BITS).min(64) { return Err(bad("key index hash width does not match the key count")); }
        let need_fb = if nf <= 1 { 0 } else { 32 - ((nf - 1) as u32).leading_zeros() };
        if file_bits != need_fb { return Err(bad("key index file-number width does not match the file count")); }
        let nb = u32_at(body, &mut p)? as usize;
        if nb as u64 != num_keys.div_ceil(block_keys as u64) { return Err(bad("key index block count mismatch")); }
        let mut fences = Vec::with_capacity(nb);
        for _ in 0..nb {
            let first = u64_at(body, &mut p)?;
            let off = u64_at(body, &mut p)?;
            fences.push((first, off));
        }
        let ids_len = usize::try_from(u64_at(body, &mut p)?).map_err(|_| bad("overflow"))?;
        let ids_at = p;
        take(body, &mut p, ids_len)?;
        if ids_len as u64 != (num_keys * file_bits as u64).div_ceil(8) { return Err(bad("key index file-number table length mismatch")); }
        let rice_len = usize::try_from(u64_at(body, &mut p)?).map_err(|_| bad("overflow"))?;
        let rice_at = p;
        take(body, &mut p, rice_len)?;
        if p != body.len() { return Err(bad("trailing bytes in key index")); }
        let idx = KeyIndex {
            id: want, bytes: data.to_vec(), column, files, num_keys, q, k, file_bits, block_keys,
            fences, ids_at, ids_len, rice_at, rice_len,
        };
        idx.validate()?;
        Ok(idx)
    }

    /// Full structural check: decode every block (offsets must follow each other exactly, hashes must be
    /// sorted and below 2^q), every file number in range.
    fn validate(&self) -> Result<(), KoreError> {
        let limit = if self.q == 64 { u64::MAX } else { (1u64 << self.q) - 1 };
        let rice = self.rice();
        let mut prev: Option<u64> = None;
        let mut expect_off = 0u64;
        for b in 0..self.fences.len() {
            if self.fences[b].1 != expect_off { return Err(bad("key index block offset mismatch")); }
            let mut last = None;
            let mut err = None;
            let end = self.decode_block(b, rice, |v, _| {
                if v > limit || prev.map_or(false, |p| v < p) { err = Some(bad("key index hashes out of order")); return false; }
                prev = Some(v);
                last = Some(v);
                true
            })?;
            if let Some(e) = err { return Err(e); }
            let _ = last;
            expect_off = end;
        }
        if expect_off.div_ceil(8) != rice.len() as u64 { return Err(bad("key index Rice stream length mismatch")); }
        // padding bits must be zero (every byte is significant)
        if expect_off % 8 != 0 && (rice[rice.len() - 1] >> (expect_off % 8)) != 0 { return Err(bad("key index padding is not zero")); }
        let ids = self.ids();
        let nbits = self.num_keys * self.file_bits as u64;
        if nbits % 8 != 0 && (ids[ids.len() - 1] >> (nbits % 8)) != 0 { return Err(bad("key index padding is not zero")); }
        for i in 0..self.num_keys {
            if self.file_of(i)? as usize >= self.files.len().max(1) { return Err(bad("key index file number out of range")); }
        }
        if self.num_keys > 0 && self.files.is_empty() { return Err(bad("keys without files")); }
        Ok(())
    }

    fn rice(&self) -> &[u8] { &self.bytes[self.rice_at..self.rice_at + self.rice_len] }
    fn ids(&self) -> &[u8] { &self.bytes[self.ids_at..self.ids_at + self.ids_len] }

    fn file_of(&self, i: u64) -> Result<u32, KoreError> {
        get_bits(self.ids(), i * self.file_bits as u64, self.file_bits).map(|v| v as u32).ok_or_else(|| bad("key index file number out of bounds"))
    }

    /// Decode block `b`, calling `f(hash, entry number)` until it returns false. Returns the bit offset
    /// after the block (when fully decoded).
    fn decode_block(&self, b: usize, rice: &[u8], mut f: impl FnMut(u64, u64) -> bool) -> Result<u64, KoreError> {
        let first_entry = (b * self.block_keys) as u64;
        let count = (self.num_keys - first_entry).min(self.block_keys as u64);
        let (mut v, mut pos) = self.fences[b];
        if !f(v, first_entry) { return Ok(pos); }
        for j in 1..count {
            let mut u = 0u64;
            loop {
                match bit(rice, pos) {
                    Some(true) => { u += 1; pos += 1; }
                    Some(false) => { pos += 1; break; }
                    None => return Err(bad("truncated key index Rice stream")),
                }
                if u > 1 << 40 { return Err(bad("corrupt key index Rice stream")); }
            }
            let low = get_bits(rice, pos, self.k).ok_or_else(|| bad("truncated key index Rice stream"))?;
            pos += self.k as u64;
            let d = u.checked_shl(self.k).filter(|x| x >> self.k == u).ok_or_else(|| bad("corrupt key index Rice stream"))? | low;
            v = v.checked_add(d).ok_or_else(|| bad("corrupt key index Rice stream"))?;
            if !f(v, first_entry + j) { return Ok(pos); }
        }
        Ok(pos)
    }

    pub fn to_bytes(&self) -> &[u8] { &self.bytes }
    pub fn id(&self) -> [u8; 32] { self.id }
    pub fn id_hex(&self) -> String { crate::dict::hex(&self.id) }
    /// File name convention: `<id>.kidx`.
    pub fn file_name(&self) -> String { format!("{}.kidx", self.id_hex()) }
    pub fn column(&self) -> &str { &self.column }
    pub fn files(&self) -> &[IndexedFile] { &self.files }
    pub fn num_keys(&self) -> u64 { self.num_keys }

    pub fn read_file(path: &std::path::Path) -> Result<KeyIndex, KoreError> {
        let b = std::fs::read(path).map_err(|e| bad(format!("io: {e}")))?;
        Self::from_bytes(&b)
    }

    /// Positions (into `files()`) of the files that may contain `key`, ascending, without duplicates.
    /// A file that contains the key is always listed; a listed file may not contain it.
    pub fn candidates(&self, key: &str) -> Vec<usize> {
        if self.num_keys == 0 { return vec![]; }
        let h = kh64(key.as_bytes());
        let h = if self.q == 0 { 0 } else { h >> (64 - self.q) };
        // first block whose first hash is >= h; equal hashes may start in the block before it
        let b = self.fences.partition_point(|f| f.0 < h);
        let mut b = b.saturating_sub(1);
        let rice = self.rice();
        let mut out = vec![];
        loop {
            if b >= self.fences.len() || self.fences[b].0 > h { break; }
            let mut past = false;
            // validated at load: decoding cannot fail
            let _ = self.decode_block(b, rice, |v, i| {
                if v == h { if let Ok(f) = self.file_of(i) { out.push(f as usize); } }
                if v > h { past = true; return false; }
                true
            });
            if past { break; }
            b += 1;
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Find every row whose key column equals `key` and return the requested `columns` of it.
    /// `read` returns the bytes of an indexed file (by its entry); each file is checked per `verify`
    /// before it is decoded. Version-4 files may need `dicts`.
    pub fn lookup(&self, key: &str, columns: &[&str], dicts: &DictSet, verify: Verify,
                  mut read: impl FnMut(&IndexedFile) -> Result<Vec<u8>, KoreError>) -> Result<Vec<Hit>, KoreError> {
        let mut hits = vec![];
        for fno in self.candidates(key) {
            let f = &self.files[fno];
            let bytes = read(f)?;
            self.check_file(f, &bytes, verify)?;
            hits.extend(self.find_in(fno, &bytes, key, columns, dicts)?);
        }
        Ok(hits)
    }

    /// Check `bytes` against the pin of `f`.
    pub fn check_file(&self, f: &IndexedFile, bytes: &[u8], verify: Verify) -> Result<(), KoreError> {
        if bytes.len() as u64 != f.len { return Err(bad(format!("file '{}' changed since the index was built (length)", f.name))); }
        if verify == Verify::Sha256 {
            let h: [u8; 32] = Sha256::digest(bytes).into();
            if h != f.sha256 { return Err(bad(format!("file '{}' changed since the index was built (sha256)", f.name))); }
        }
        Ok(())
    }

    /// Rows of file `fno` (already checked) whose key equals `key`.
    pub fn find_in(&self, fno: usize, bytes: &[u8], key: &str, columns: &[&str], dicts: &DictSet) -> Result<Vec<Hit>, KoreError> {
        // the key column first: a file without the key (hash collision, missing key) costs only that
        // column, and the value columns are decoded only when there is a row to return
        let kb = KoreReader::from_bytes_columns_with_dicts(bytes, &[self.column.as_str()], dicts)?;
        let kc = &kb.columns[0].data;
        let rows: Vec<usize> = (0..kb.num_rows).filter(|&r| kc.get_str(r) == Some(key)).collect();
        if rows.is_empty() { return Ok(vec![]); }
        let vb = if columns.is_empty() { None } else { Some(KoreReader::from_bytes_columns_with_dicts(bytes, columns, dicts)?) };
        Ok(rows.into_iter().map(|r| {
            let cols = columns.iter().map(|c| {
                let col = vb.as_ref().unwrap().columns.iter().find(|x| x.name == *c).expect("projected");
                kore_core::Column { name: col.name.clone(), data: col.data.take_rows(&[r]) }
            }).collect();
            Hit { file: fno, row: r, values: DataBlock { columns: cols, num_rows: 1 } }
        }).collect())
    }
}
