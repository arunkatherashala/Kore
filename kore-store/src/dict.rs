//! Shared compression dictionary: a separate, immutable, content-addressed object that version-4 files
//! (see `v4`) reference by id instead of repeating the same vocabulary in every file.
//!
//! ```text
//! "KDIC" | version:u16 = 1 | kind:u8 | len:u32 | content[len] | sha256:[32]
//! ```
//! `sha256` covers every byte before it and is the dictionary **id**. A version-4 file stores the id and
//! the length of every dictionary it uses in its checksummed header, so a reader can only decode it with
//! exactly the dictionary object it was written with: a changed byte in the object fails the sha256,
//! a different (valid) object has a different id and is refused.
//!
//! `kind` 1 = raw content (any bytes; used by zstd as history), 2 = trained zstd dictionary. Both are
//! handed to zstd unchanged; `kind` is informational and must be 1 or 2.
//!
//! Versioning: a dictionary is never modified or replaced. A store that wants a better dictionary writes a
//! new object (new id); older files keep referencing the old one, so every historical version stays
//! readable as long as the objects are kept (time travel needs the dictionaries of the versions read).

use kore_core::KoreError;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex, OnceLock};

pub const DICT_MAGIC: &[u8; 4] = b"KDIC";
pub const DICT_VERSION: u16 = 1;
/// Upper bound on dictionary content (zstd itself is fine with more; this bounds reader memory).
pub const MAX_DICT: usize = 16 << 20;

pub const KIND_RAW: u8 = 1;
pub const KIND_TRAINED: u8 = 2;

const ZSTD_DICT_MAGIC: [u8; 4] = [0x37, 0xA4, 0x30, 0xEC];

fn bad(m: impl Into<String>) -> KoreError { KoreError::InvalidArgument(m.into()) }

/// A loaded, verified shared dictionary. Cheap to clone.
#[derive(Clone)]
pub struct SharedDict {
    inner: Arc<Inner>,
}

struct Inner {
    kind: u8,
    content: Vec<u8>,
    id: [u8; 32],
    bytes: Vec<u8>,
    ddict: OnceLock<zstd::dict::DecoderDictionary<'static>>,
    cdicts: Mutex<Vec<(i32, Arc<zstd::dict::EncoderDictionary<'static>>)>>,
}

impl std::fmt::Debug for SharedDict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SharedDict(id={}, kind={}, {} bytes)", self.id_hex(), self.inner.kind, self.inner.content.len())
    }
}

impl SharedDict {
    fn build(kind: u8, content: Vec<u8>) -> Result<SharedDict, KoreError> {
        if content.is_empty() { return Err(bad("empty dictionary")); }
        if content.len() > MAX_DICT { return Err(bad("dictionary too large")); }
        let mut bytes = Vec::with_capacity(content.len() + 43);
        bytes.extend_from_slice(DICT_MAGIC);
        bytes.extend_from_slice(&DICT_VERSION.to_le_bytes());
        bytes.push(kind);
        bytes.extend_from_slice(&(content.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&content);
        let id: [u8; 32] = Sha256::digest(&bytes).into();
        bytes.extend_from_slice(&id);
        Ok(SharedDict { inner: Arc::new(Inner { kind, content, id, bytes, ddict: OnceLock::new(), cdicts: Mutex::new(Vec::new()) }) })
    }

    /// Raw-content dictionary (e.g. the text of a first segment). If the bytes happen to start with the
    /// zstd dictionary magic a zero byte is put in front, so zstd always treats them as raw content.
    pub fn from_content(content: &[u8]) -> Result<SharedDict, KoreError> {
        let mut c = Vec::with_capacity(content.len() + 1);
        if content.len() >= 4 && content[..4] == ZSTD_DICT_MAGIC { c.push(0); }
        c.extend_from_slice(content);
        Self::build(KIND_RAW, c)
    }

    /// zstd-trained dictionary of at most `max_size` bytes from `samples`.
    pub fn train<S: AsRef<[u8]>>(samples: &[S], max_size: usize) -> Result<SharedDict, KoreError> {
        let d = zstd::dict::from_samples(samples, max_size).map_err(|e| bad(format!("dictionary training failed: {e}")))?;
        Self::build(KIND_TRAINED, d)
    }

    /// Parse and verify a serialized dictionary object (strict: exact length, known version and kind,
    /// sha256 must match).
    pub fn from_bytes(data: &[u8]) -> Result<SharedDict, KoreError> {
        if data.len() < 4 + 2 + 1 + 4 + 32 || &data[..4] != DICT_MAGIC { return Err(bad("not a KORE dictionary object")); }
        let ver = u16::from_le_bytes([data[4], data[5]]);
        if ver != DICT_VERSION { return Err(bad(format!("unsupported dictionary version {ver}"))); }
        let kind = data[6];
        if kind != KIND_RAW && kind != KIND_TRAINED { return Err(bad(format!("unknown dictionary kind {kind}"))); }
        let len = u32::from_le_bytes(data[7..11].try_into().unwrap()) as usize;
        if len == 0 || len > MAX_DICT || data.len() != 11 + len + 32 { return Err(bad("dictionary object length mismatch")); }
        let want: [u8; 32] = Sha256::digest(&data[..11 + len]).into();
        if want[..] != data[11 + len..] { return Err(bad("dictionary checksum (sha256) mismatch")); }
        Self::build(kind, data[11..11 + len].to_vec())
    }

    pub fn read_file(path: &std::path::Path) -> Result<SharedDict, KoreError> {
        let b = std::fs::read(path).map_err(|e| bad(format!("io: {e}")))?;
        Self::from_bytes(&b)
    }

    /// The serialized object (what `from_bytes` accepts).
    pub fn to_bytes(&self) -> &[u8] { &self.inner.bytes }
    pub fn id(&self) -> [u8; 32] { self.inner.id }
    pub fn id_hex(&self) -> String { hex(&self.inner.id) }
    pub fn kind(&self) -> u8 { self.inner.kind }
    pub fn content(&self) -> &[u8] { &self.inner.content }
    /// File name convention for stores that keep dictionaries next to their segments.
    pub fn file_name(&self) -> String { format!("{}.kdic", self.id_hex()) }

    pub(crate) fn compress(&self, raw: &[u8], level: i32) -> Vec<u8> {
        let cd = {
            let mut g = self.inner.cdicts.lock().unwrap_or_else(|e| e.into_inner());
            match g.iter().find(|(l, _)| *l == level) {
                Some((_, d)) => d.clone(),
                None => {
                    let d = Arc::new(zstd::dict::EncoderDictionary::copy(&self.inner.content, level));
                    g.push((level, d.clone()));
                    d
                }
            }
        };
        let mut c = zstd::bulk::Compressor::with_prepared_dictionary(&cd).expect("zstd dictionary");
        c.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true)).expect("zstd parameter");
        c.compress(raw).expect("zstd in-memory compress")
    }

    pub(crate) fn decompress(&self, body: &[u8], raw_len: usize) -> Result<Vec<u8>, KoreError> {
        let dd = self.inner.ddict.get_or_init(|| zstd::dict::DecoderDictionary::copy(&self.inner.content));
        let mut d = zstd::bulk::Decompressor::with_prepared_dictionary(dd).map_err(|e| bad(format!("ZSTD: {e}")))?;
        d.decompress(body, raw_len).map_err(|e| bad(format!("ZSTD (dictionary {}): {e}", self.id_hex())))
    }
}

pub(crate) fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b { s.push(H[(x >> 4) as usize] as char); s.push(H[(x & 15) as usize] as char); }
    s
}

/// The dictionaries available to a reader, looked up by id.
#[derive(Clone, Debug, Default)]
pub struct DictSet {
    dicts: Vec<SharedDict>,
}

impl DictSet {
    pub fn new() -> Self { DictSet { dicts: vec![] } }
    pub fn with(mut self, d: SharedDict) -> Self { self.add(d); self }
    pub fn add(&mut self, d: SharedDict) { if self.get(&d.id()).is_none() { self.dicts.push(d); } }
    pub fn get(&self, id: &[u8; 32]) -> Option<&SharedDict> { self.dicts.iter().find(|d| &d.id() == id) }
    pub fn len(&self) -> usize { self.dicts.len() }
    pub fn is_empty(&self) -> bool { self.dicts.is_empty() }
    /// Load every `*.kdic` object in `dir` (each one verified).
    pub fn load_dir(dir: &std::path::Path) -> Result<DictSet, KoreError> {
        let mut s = DictSet::new();
        let rd = match std::fs::read_dir(dir) { Ok(r) => r, Err(_) => return Ok(s) };
        let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |e| e == "kdic")).collect();
        paths.sort();
        for p in paths {
            let d = SharedDict::read_file(&p)?;
            if p.file_stem().map_or(true, |st| st.to_string_lossy() != d.id_hex()) {
                return Err(bad(format!("dictionary file {} does not match its id {}", p.display(), d.id_hex())));
            }
            s.add(d);
        }
        Ok(s)
    }
}
