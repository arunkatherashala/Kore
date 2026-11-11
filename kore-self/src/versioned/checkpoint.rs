//! Track D1b: state checkpoint files next to the log (`<log>/checkpoints/`).
//!
//! A checkpoint is an opaque payload (the serialized fold state of one kind of reader, e.g. the
//! being) taken at log version `v`. This file only stores and validates it; what the payload means and
//! how it is anchored in the log is the caller's (`being/ckpt.rs`). Nothing here changes the log:
//! checkpoint files are not segments, are not listed in the MANIFEST, and `verify` / tamper detection
//! of the log are unchanged (the `checkpoints/` directory is not a table directory, so it is not a
//! stray segment either). Deleting every checkpoint is always safe: the reader folds the whole log.
//!
//! File: `checkpoints/<kind>-<v, 10 digits>.kore`, a kore-store file (written by `KoreWriter`) with one
//! row and the columns `ckpt_format` (snapshot format version, `CKPT_FORMAT`), `kind`, `version`,
//! `chain_hash` (the MANIFEST hash of entry `v`), `step`, `content_sha256` and `payload_hex`. Extra
//! columns are ignored (additive); an unknown `ckpt_format` makes the file invalid (ignored).
//!
//! `content_sha256` = sha256 of `kore-ckpt|<format>|<kind>|<v>|<chain_hash>|<step>|` + payload, so the
//! content hash binds the payload to the version and the chain. A file is *valid for this log* only if
//! its content hash verifies, `v` <= the latest version, and the log's own MANIFEST hash at `v` equals
//! `chain_hash` (a `forget` that re-seals entry `v` or earlier, a replaced or rolled-back log, all make
//! it invalid). Being valid is necessary, not sufficient: the content hash is also anchored in the log
//! (see `being/ckpt.rs`), so a file forged with a self-consistent hash is still not used.
//!
//! Writes are atomic: temp file, fsync, rename, fsync of the directory. A crash before the rename
//! leaves a `.tmp` file that no reader looks at (removed by the next write); the previous checkpoint
//! files are untouched until a newer one is complete.

use super::*;
use kore_core::{Column, DataBlock, Value};

pub const CKPT_DIR: &str = "checkpoints";
/// Snapshot format version written now. A reader ignores any other value.
pub const CKPT_FORMAT: i64 = 1;
// Track W2: a reader whose payload layout changed writes and expects its own format version through
// `write_checkpoint_fmt` / `read_checkpoint_fmt` (additive; `write_checkpoint` / `read_checkpoint` are these
// with `CKPT_FORMAT`). A file of another format version is then not used, never misread.

/// A checkpoint file as read from disk (not yet known to be anchored in the log).
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointFile {
    pub kind: String,
    pub version: u64,
    pub chain_hash: String,
    pub step: u64,
    pub content_hash: String,
    pub payload: Vec<u8>,
    /// Path relative to the log directory.
    pub file: String,
    /// sha256 of the whole file as written / read (also anchored in the log, so a change of any byte of
    /// the file, including bytes the reader never decodes, makes it unusable).
    pub file_sha256: String,
}

pub fn content_hash(format: i64, kind: &str, version: u64, chain_hash: &str, step: u64, payload: &[u8]) -> String {
    let head = format!("kore-ckpt|{format}|{kind}|{version}|{chain_hash}|{step}|");
    sha256_hex(&[head.as_bytes(), payload])
}

fn valid_kind(kind: &str) -> bool {
    !kind.is_empty() && kind.len() <= 32 && kind.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn file_name(kind: &str, v: u64) -> String {
    format!("{kind}-{v:010}.kore")
}

fn to_hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    let d = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    };
    b.chunks(2).map(|p| Some(d(p[0])? << 4 | d(p[1])?)).collect()
}

/// Where a simulated crash stops a checkpoint write (tests).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CkptCrash {
    None,
    /// The temp file is written (possibly partly) but never renamed.
    BeforeRename,
}


/// The file bytes of `c` with `ckpt_format` = `format` (tests write unknown versions with it).
pub(crate) fn encode_with_format(c: &CheckpointFile, format: i64) -> Vec<u8> {
    let s = |name: &str, v: &str| Column::str_col(name, vec![Some(v.to_string())]);
    let i = |name: &str, v: i64| Column::int64(name, vec![Some(v)]);
    let block = DataBlock::new(vec![
        i("ckpt_format", format),
        s("kind", &c.kind),
        i("version", c.version as i64),
        s("chain_hash", &c.chain_hash),
        i("step", c.step as i64),
        s("content_sha256", &c.content_hash),
        s("payload_hex", &to_hex(&c.payload)),
    ])
    .expect("one row per column");
    kore_store::KoreWriter::to_bytes(&block)
}

fn decode(bytes: &[u8], file: &str, format: i64) -> Result<CheckpointFile, String> {
    let block = kore_store::KoreReader::from_bytes(bytes).map_err(|e| format!("not a readable kore file: {e}"))?;
    if block.num_rows != 1 {
        return Err(format!("{} rows (expected 1)", block.num_rows));
    }
    let get = |name: &str| block.column(name).map(|c| c.data.get_value(0)).ok_or_else(|| format!("column {name} missing"));
    let int = |name: &str| match get(name)? {
        Value::Int(x) => Ok(x),
        v => Err(format!("column {name}: {v:?}")),
    };
    let st = |name: &str| match get(name)? {
        Value::Str(x) => Ok(x),
        v => Err(format!("column {name}: {v:?}")),
    };
    let fmt = int("ckpt_format")?;
    if fmt != format {
        return Err(format!("unknown snapshot format version {fmt}"));
    }
    let (version, step) = (int("version")?, int("step")?);
    if version < 0 || step < 0 {
        return Err("negative version or step".into());
    }
    let payload = from_hex(&st("payload_hex")?).ok_or("payload is not hex")?;
    Ok(CheckpointFile { kind: st("kind")?, version: version as u64, chain_hash: st("chain_hash")?, step: step as u64, content_hash: st("content_sha256")?, payload, file: file.to_string(), file_sha256: sha256_hex(&[bytes]) })
}

impl VersionedLog {
    /// The MANIFEST hash of entry `v` (the head of the chain at version `v`).
    pub fn chain_hash_at(&self, v: u64) -> Option<&str> {
        self.entries.get(v as usize).filter(|e| e.v == v).map(|e| e.hash.as_str())
    }

    /// Write a checkpoint of `kind` at the latest version (atomic). Returns what was written.
    pub fn write_checkpoint(&self, kind: &str, step: u64, payload: Vec<u8>) -> Result<CheckpointFile, LogError> {
        self.write_checkpoint_at(kind, step, payload, CkptCrash::None)
    }

    /// As `write_checkpoint`, with the snapshot format version `format` (Track W2).
    pub fn write_checkpoint_fmt(&self, kind: &str, step: u64, payload: Vec<u8>, format: i64) -> Result<CheckpointFile, LogError> {
        self.write_checkpoint_impl(kind, step, payload, CkptCrash::None, format)
    }

    pub(crate) fn write_checkpoint_at(&self, kind: &str, step: u64, payload: Vec<u8>, crash: CkptCrash) -> Result<CheckpointFile, LogError> {
        self.write_checkpoint_impl(kind, step, payload, crash, CKPT_FORMAT)
    }

    fn write_checkpoint_impl(&self, kind: &str, step: u64, payload: Vec<u8>, crash: CkptCrash, format: i64) -> Result<CheckpointFile, LogError> {
        if !valid_kind(kind) {
            return inv("checkpoint kind must be 1-32 chars of [a-z0-9_]");
        }
        let v = self.latest_version();
        let chain_hash = self.chain_hash_at(v).ok_or_else(|| LogError::Invalid("no version to checkpoint".into()))?.to_string();
        let name = file_name(kind, v);
        let mut c = CheckpointFile { kind: kind.to_string(), version: v, content_hash: content_hash(format, kind, v, &chain_hash, step, &payload), chain_hash, step, payload, file: format!("{CKPT_DIR}/{name}"), file_sha256: String::new() };
        let dir = self.dir.join(CKPT_DIR);
        fs::create_dir_all(&dir)?;
        // leftovers of an earlier interrupted write of this kind
        for f in fs::read_dir(&dir)?.flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            if n.starts_with(&format!("{kind}-")) && n.ends_with(".tmp") {
                let _ = fs::remove_file(f.path());
            }
        }
        let bytes = encode_with_format(&c, format);
        c.file_sha256 = sha256_hex(&[&bytes]);
        let tmp = dir.join(format!("{name}.tmp"));
        if crash == CkptCrash::BeforeRename {
            // a torn temp file: half the bytes, never renamed
            write_synced(&tmp, &bytes[..bytes.len() / 2])?;
            return Err(LogError::Io("simulated crash".into()));
        }
        write_synced(&tmp, &bytes)?;
        fs::rename(&tmp, dir.join(&name))?;
        sync_dir(&dir);
        Ok(c)
    }

    /// Checkpoint files of `kind` on disk, newest version first (names only; nothing is validated).
    pub fn checkpoint_files(&self, kind: &str) -> Vec<(u64, String)> {
        let mut out = vec![];
        if !valid_kind(kind) {
            return out;
        }
        if let Ok(rd) = fs::read_dir(self.dir.join(CKPT_DIR)) {
            let pre = format!("{kind}-");
            for f in rd.flatten() {
                let n = f.file_name().to_string_lossy().to_string();
                if let Some(num) = n.strip_prefix(&pre).and_then(|r| r.strip_suffix(".kore")) {
                    if num.len() == 10 && num.bytes().all(|c| c.is_ascii_digit()) {
                        if let Ok(v) = num.parse::<u64>() {
                            out.push((v, format!("{CKPT_DIR}/{n}")));
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| b.cmp(a));
        out
    }

    /// Read one checkpoint file and check it against this log: snapshot format, kind, content hash,
    /// `v` <= latest version, and the MANIFEST hash at `v`. Any failure is the reason, as text.
    pub fn read_checkpoint(&self, kind: &str, file: &str) -> Result<CheckpointFile, String> {
        self.read_checkpoint_fmt(kind, file, CKPT_FORMAT)
    }

    /// As `read_checkpoint`, for a reader whose snapshot format version is `format` (Track W2): a file of
    /// any other format version is refused.
    pub fn read_checkpoint_fmt(&self, kind: &str, file: &str, format: i64) -> Result<CheckpointFile, String> {
        let bytes = fs::read(self.dir.join(file)).map_err(|e| format!("{file}: {e}"))?;
        let c = decode(&bytes, file, format).map_err(|e| format!("{file}: {e}"))?;
        if c.kind != kind {
            return Err(format!("{file}: kind {} (expected {kind})", c.kind));
        }
        if content_hash(format, &c.kind, c.version, &c.chain_hash, c.step, &c.payload) != c.content_hash {
            return Err(format!("{file}: content hash does not verify"));
        }
        if c.version > self.latest_version() {
            return Err(format!("{file}: version {} is after the latest version {} of the log", c.version, self.latest_version()));
        }
        if self.chain_hash_at(c.version) != Some(c.chain_hash.as_str()) {
            return Err(format!("{file}: the log's chain hash at version {} differs (re-sealed by forget, or another log)", c.version));
        }
        Ok(c)
    }

    /// Keep the newest `keep` checkpoint files of `kind`; delete the others (best effort).
    pub fn prune_checkpoints(&self, kind: &str, keep: usize) {
        for (_, f) in self.checkpoint_files(kind).into_iter().skip(keep) {
            let _ = fs::remove_file(self.dir.join(f));
        }
    }

    /// Every event row of the versions after `v`, in log order (as `for_each_event`, which is the
    /// case `v` = none). Only the events segments of those versions are read (checksums verified).
    pub fn for_each_event_after(&self, v: u64, f: impl FnMut(EventRow) -> bool) -> Result<(), LogError> {
        let first = self.ev_segs.partition_point(|s| (s.1 as u64) <= v);
        self.each_event_from(first, f)
    }

    /// Every event row of the versions `0..=v`, in log order (the events the log had at version `v`,
    /// as far as `forget` has not removed them since).
    pub fn for_each_event_upto(&self, v: u64, f: impl FnMut(EventRow) -> bool) -> Result<(), LogError> {
        let end = self.ev_segs.partition_point(|s| (s.1 as u64) <= v);
        self.each_event_range(0, end, f)
    }
}
