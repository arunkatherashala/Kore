//! Versioned, append-only, evidence-linked log (milestone M1 core).
//!
//! See `kore-self/docs/versioned_log.md` for the data model, file layout,
//! invariants, and what is and is not guaranteed. This module is
//! self-contained: it only uses `super::`/`self::` paths and external crates,
//! so the example can include it with `#[path]`.
//!
//! Commit semantics: every mutating call (`append_events`, `revise`,
//! `record_prediction`, `record_outcome`, `commit_batch`) is exactly one
//! commit and creates at most one new version. A call that adds no new rows
//! (all duplicates) creates no version. `commit_batch` groups several kinds of
//! rows into a single atomic version.
#![allow(dead_code)]

pub mod checkpoint;
pub mod codec;
pub mod forget;
pub mod hz;
pub mod manifest;
pub mod model;
pub mod sql;
pub use checkpoint::{CheckpointFile, CKPT_DIR, CKPT_FORMAT};
pub use forget::{forget_hash, recover_pending, ForgetReport};
pub mod upgrade;
pub mod v4log;
pub use v4log::DictPolicy;
pub use sql::{register_into, run_readonly_sql};
pub use upgrade::{migrate_to_v2, MigrateReport};
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_v2;
#[cfg(test)]
mod tests_restart;
#[cfg(test)]
mod tests_v4;

use kore_store::{DictSet, SharedDict};
use manifest::{Entry, SegRef, ZERO_HASH};
pub use model::*;
use codec::IdKey;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Log format 1: the original segment encoding (kore-store columns + LZ4).
/// The default for newly created logs until Track D1 (now `DEFAULT_FORMAT`, format 3); format-1
/// logs still open, read and append exactly as before (the format is read from `schema_meta`).
pub const FORMAT_VERSION: i64 = 1;
/// Log format 2: `codec.rs` segments (raw 32-byte ids, delta timestamps,
/// dictionaries, hz entropy stage) and content-hash idempotent revisions.
/// Reached only by creating a log with `LogFormat::V2` or by an explicit
/// `upgrade_to_v2` / `migrate_to_v2`; never silently.
pub const FORMAT_V2: i64 = 2;
/// Log format 3: format 2 segments plus the journal MANIFEST (header `KORE-VLOG-MANIFEST 2`):
/// a commit appends one line and fsyncs instead of rewriting the whole file, so commit cost no
/// longer grows with the number of versions. Reached only by creating a log with `LogFormat::V3`
/// or by the explicit `upgrade_to_v3`; never silently. Older binaries refuse the header.
pub const FORMAT_V3: i64 = 3;
/// Log format 4: segments are kore-store version-4 ("compact") files (`"enc":4`); the event text may
/// be coded with a shared dictionary, a content-addressed object in `dicts/` listed in the MANIFEST
/// by the commit that first uses it. Journal MANIFEST with header `KORE-VLOG-MANIFEST 3`, which
/// older binaries refuse. Reached only by creating a log with `LogFormat::V4` or by the explicit
/// `upgrade_to_v4`; never silently.
pub const FORMAT_V4: i64 = 4;
/// Track D1a: the format of a log created by `open_or_create` / `open_or_create_with_clock`
/// (format 3: format-2 segments and the journal MANIFEST, 611 B/step against 2,582 for format 1 in
/// `docs/longrun.md`). Applies only when the log is created; an existing log keeps its format and is
/// never upgraded automatically. `open_or_create_with_format(.., LogFormat::V1)` still creates format 1.
pub const DEFAULT_FORMAT: LogFormat = LogFormat::V3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    V1,
    V2,
    V3,
    V4,
}
impl LogFormat {
    pub fn number(self) -> i64 {
        match self {
            LogFormat::V1 => FORMAT_VERSION,
            LogFormat::V2 => FORMAT_V2,
            LogFormat::V3 => FORMAT_V3,
            LogFormat::V4 => FORMAT_V4,
        }
    }
    fn enc(self) -> Option<u8> {
        match self {
            LogFormat::V1 => None,
            LogFormat::V2 | LogFormat::V3 => Some(2),
            LogFormat::V4 => Some(v4log::ENC_V4),
        }
    }
    /// MANIFEST header level this format writes (see `manifest::header_for`).
    fn manifest_level(self) -> u8 {
        match self {
            LogFormat::V4 => 3,
            LogFormat::V3 => 2,
            _ => 1,
        }
    }
    fn from_number(n: i64) -> LogFormat {
        if n >= FORMAT_V4 {
            LogFormat::V4
        } else if n >= FORMAT_V3 {
            LogFormat::V3
        } else if n >= FORMAT_V2 {
            LogFormat::V2
        } else {
            LogFormat::V1
        }
    }
    /// Revision replay is content-idempotent from format 2 on.
    fn content_idem(self) -> bool {
        self != LogFormat::V1
    }
}

/// `dict` is used only by format-4 `events` segments.
fn encode_seg(enc: Option<u8>, t: Table, rows: &[Vec<Cell>], dict: Option<&SharedDict>) -> Vec<u8> {
    match enc {
        Some(2) => codec::encode_segment(t, rows),
        Some(v4log::ENC_V4) => v4log::encode(t, rows, dict),
        _ => kore_store::KoreWriter::to_bytes(&rows_to_block(t, rows)),
    }
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum VerifyError {
    ManifestMissing,
    ManifestCorrupt(String),
    /// Chain or ordering broken at this version.
    ChainBroken { version: u64, detail: String },
    MissingSegment { file: String },
    ChecksumMismatch { file: String },
    RowCountMismatch { file: String },
    SegmentUnreadable { file: String, detail: String },
    /// A segment file on disk that no manifest entry lists (crash leftover or
    /// a manifest truncated at a line boundary).
    ExtraSegment { file: String },
    HeadMismatch { expected: String, found: String },
    Io(String),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::ManifestMissing => write!(f, "MANIFEST is missing"),
            VerifyError::ManifestCorrupt(d) => write!(f, "MANIFEST is corrupt: {d}"),
            VerifyError::ChainBroken { version, detail } => write!(f, "hash chain broken at version {version}: {detail}"),
            VerifyError::MissingSegment { file } => write!(f, "segment missing: {file}"),
            VerifyError::ChecksumMismatch { file } => write!(f, "checksum mismatch in segment: {file}"),
            VerifyError::RowCountMismatch { file } => write!(f, "row count mismatch in segment: {file}"),
            VerifyError::SegmentUnreadable { file, detail } => write!(f, "segment unreadable: {file}: {detail}"),
            VerifyError::ExtraSegment { file } => write!(f, "segment not listed in MANIFEST (uncommitted leftover or truncated manifest): {file}"),
            VerifyError::HeadMismatch { expected, found } => write!(f, "head hash mismatch: expected {expected}, found {found}"),
            VerifyError::Io(d) => write!(f, "io error: {d}"),
        }
    }
}
impl std::error::Error for VerifyError {}

#[derive(Debug, Clone, PartialEq)]
pub enum LogError {
    Invalid(String),
    NotFound(String),
    Conflict(String),
    Io(String),
    Verify(VerifyError),
}
impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::Invalid(s) => write!(f, "invalid: {s}"),
            LogError::NotFound(s) => write!(f, "not found: {s}"),
            LogError::Conflict(s) => write!(f, "conflict: {s}"),
            LogError::Io(s) => write!(f, "io: {s}"),
            LogError::Verify(v) => write!(f, "verify: {v}"),
        }
    }
}
impl std::error::Error for LogError {}
impl From<std::io::Error> for LogError {
    fn from(e: std::io::Error) -> Self {
        LogError::Io(e.to_string())
    }
}
impl From<VerifyError> for LogError {
    fn from(e: VerifyError) -> Self {
        LogError::Verify(e)
    }
}
fn inv<T>(s: impl Into<String>) -> Result<T, LogError> {
    Err(LogError::Invalid(s.into()))
}

// ─── Public value types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct VersionInfo {
    pub v: u64,
    pub recorded_at: String,
    /// Cumulative rows per table as of this version.
    pub rows: BTreeMap<String, u64>,
    pub manifest_hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommitInfo {
    pub version: u64,
    /// False when nothing new was added (no version was created).
    pub created: bool,
    pub recorded_at: String,
    pub new_rows: BTreeMap<String, usize>,
    pub event_ids: Vec<String>,
    pub rev_ids: Vec<String>,
    pub pred_ids: Vec<String>,
    /// Events skipped because they were forgotten earlier (re-ingest guard).
    pub suppressed: usize,
}

#[derive(Debug, Clone)]
pub enum AsOf {
    Version(u64),
    /// Latest version whose `recorded_at` is <= this timestamp (transaction time).
    Time(String),
}

#[derive(Debug, Clone, Default)]
pub struct Diff {
    pub from: u64,
    pub to: u64,
    pub events: Vec<EventRow>,
    pub revisions: Vec<RevisionRow>,
    pub causes: Vec<CauseRow>,
    pub predictions: Vec<PredictionRow>,
    pub outcomes: Vec<OutcomeRow>,
}

/// Immutable view of the log as of one version.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub version: u64,
    pub recorded_at: String,
    pub manifest_hash: String,
    /// Valid-time cut-off used by the `beliefs_asof` view. Default: `recorded_at`.
    pub valid_at: String,
    tables: Arc<Vec<Vec<Vec<Cell>>>>,
}

impl Snapshot {
    pub fn rows<R: Row>(&self) -> Vec<R> {
        self.tables[R::TABLE.idx()].iter().map(|c| R::from_cells(c)).collect()
    }
    pub fn raw(&self, t: Table) -> &[Vec<Cell>] {
        &self.tables[t.idx()]
    }
    pub fn block(&self, t: Table) -> kore_core::DataBlock {
        rows_to_block(t, &self.tables[t.idx()])
    }
    pub fn with_valid_at(mut self, ts: &str) -> Result<Snapshot, LogError> {
        self.valid_at = normalize_ts(ts).map_err(LogError::Invalid)?;
        Ok(self)
    }
    /// Digest of the row contents of every table (independent of the manifest).
    pub fn content_hash(&self) -> String {
        let mut buf = Vec::new();
        for t in ALL_TABLES {
            buf.extend(t.name().as_bytes());
            buf.extend(canon_rows(&self.tables[t.idx()]));
        }
        sha256_hex(&[&buf])
    }
}

// ─── The log ─────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct Index {
    /// event id (32 raw bytes) -> ordinal of its row among all event rows, in log order
    events: HashMap<[u8; 32], u32>,
    /// ids that are not canonical 64-char lowercase hex (cannot be produced by the API)
    events_odd: HashMap<String, u32>,
    /// number of event rows in the latest version
    next_ord: u32,
    revs: HashMap<String, (String, String)>,
    /// (entity_type, key) -> (head rev_id, content digest of that revision)
    heads: HashMap<(String, String), (String, String)>,
    /// content digest -> rev_id of the first revision with that content
    content_ids: HashMap<String, String>,
    /// Log format 2: a revision whose content digest already exists is a no-op.
    content_idem: bool,
    preds: HashMap<String, Vec<String>>,
    outcomes: HashSet<String>,
    /// `forget_hash` of every event id removed by `forget`.
    forgotten: HashSet<String>,
}

enum Undo {
    Event(IdKey),
    Rev { rev_id: String, hk: (String, String), prev_head: Option<(String, String)>, digest: String, added_content: bool },
    Pred(String),
    Outcome(String),
}

impl Index {
    fn ev_has(&self, id: &str) -> bool {
        self.ev_ord(id).is_some()
    }
    fn ev_ord(&self, id: &str) -> Option<u32> {
        match codec::hex64_to_bytes(id) {
            Some(b) => self.events.get(&b).copied(),
            None => self.events_odd.get(id).copied(),
        }
    }
    /// Register the next event row. Returns false (and keeps the first ordinal) if the id exists.
    fn ev_add(&mut self, k: IdKey) -> bool {
        let ord = self.next_ord;
        use std::collections::hash_map::Entry as E;
        let fresh = match k {
            IdKey::Hex(b) => match self.events.entry(b) {
                E::Vacant(v) => {
                    v.insert(ord);
                    true
                }
                E::Occupied(_) => false,
            },
            IdKey::Other(s) => match self.events_odd.entry(s) {
                E::Vacant(v) => {
                    v.insert(ord);
                    true
                }
                E::Occupied(_) => false,
            },
        };
        self.next_ord += 1;
        fresh
    }
    fn ev_remove_last(&mut self, k: &IdKey) {
        match k {
            IdKey::Hex(b) => {
                self.events.remove(b);
            }
            IdKey::Other(s) => {
                self.events_odd.remove(s);
            }
        }
        self.next_ord -= 1;
    }

    /// `event_ids`: ids of all event rows in log order. `tables`: every other table (Events is ignored).
    fn build_with_ids(event_ids: Vec<IdKey>, tables: &[Vec<Vec<Cell>>]) -> Index {
        let mut ix = Index::default();
        ix.events.reserve(event_ids.len());
        for k in event_ids {
            ix.ev_add(k);
        }
        let mut causes: HashMap<String, Vec<String>> = HashMap::new();
        for r in &tables[Table::RevisionCauses.idx()] {
            causes.entry(r[0].s()).or_default().push(r[1].s());
        }
        for r in &tables[Table::Revisions.idx()] {
            let rv = RevisionRow::from_cells(r);
            let c = causes.remove(&rv.rev_id).unwrap_or_default();
            ix.add_rev(rv, c);
        }
        for r in &tables[Table::Predictions.idx()] {
            ix.add_pred(r);
        }
        for r in &tables[Table::Outcomes.idx()] {
            ix.outcomes.insert(r[0].s());
        }
        ix
    }

    /// Register a stored revision with all its cause event ids (any order).
    fn add_rev(&mut self, rv: RevisionRow, mut c: Vec<String>) {
        c.sort();
        let digest = revision_content(&rv.entity_type, &rv.key, &rv.stance, &format!("{:?}", rv.confidence), &rv.rationale, &rv.valid_from, &rv.author, &c);
        self.revs.insert(rv.rev_id.clone(), (rv.entity_type.clone(), rv.key.clone()));
        self.content_ids.entry(digest.clone()).or_insert_with(|| rv.rev_id.clone());
        self.heads.insert((rv.entity_type, rv.key), (rv.rev_id, digest));
    }

    fn add_pred(&mut self, r: &[Cell]) {
        let p = PredictionRow::from_cells(r);
        let opts: Vec<String> = serde_json::from_str(&p.options).unwrap_or_default();
        self.preds.insert(p.pred_id, opts);
    }

    /// Order-independent comparison of two indexes (tests: the streamed open equals the full one).
    #[cfg(test)]
    pub(crate) fn same_as(&self, o: &Index) -> bool {
        self.events == o.events && self.events_odd == o.events_odd && self.next_ord == o.next_ord && self.revs == o.revs && self.heads == o.heads && self.content_ids == o.content_ids && self.content_idem == o.content_idem && self.preds == o.preds && self.outcomes == o.outcomes && self.forgotten == o.forgotten
    }

    fn build(tables: &[Vec<Vec<Cell>>]) -> Index {
        let ids = tables[Table::Events.idx()].iter().map(|r| IdKey::from_str(&r[0].s())).collect();
        Self::build_with_ids(ids, tables)
    }

    fn rollback(&mut self, undo: Vec<Undo>) {
        for u in undo.into_iter().rev() {
            match u {
                Undo::Event(k) => self.ev_remove_last(&k),
                Undo::Rev { rev_id, hk, prev_head, digest, added_content } => {
                    self.revs.remove(&rev_id);
                    match prev_head {
                        Some(h) => {
                            self.heads.insert(hk, h);
                        }
                        None => {
                            self.heads.remove(&hk);
                        }
                    }
                    if added_content {
                        self.content_ids.remove(&digest);
                    }
                }
                Undo::Pred(id) => {
                    self.preds.remove(&id);
                }
                Undo::Outcome(id) => {
                    self.outcomes.remove(&id);
                }
            }
        }
    }
}

pub struct VersionedLog {
    dir: PathBuf,
    entries: Vec<Entry>,
    /// Exact bytes of the MANIFEST as last written or read (header + one line per entry).
    manifest_raw: Vec<u8>,
    /// Journal manifest (header 2 or 3): commits append. False for format 1/2 logs written before format 3.
    journal: bool,
    /// MANIFEST header level (1, 2, or 3 = format 4), see `manifest::header_for`.
    level: u8,
    /// Uncommitted partial tail found on disk at open (journal only); removed before the next append.
    torn: Vec<u8>,
    /// Latest snapshot, decoded lazily for all-v2 logs (see `open_latest`).
    head: Mutex<Option<Snapshot>>,
    index: Index,
    /// (first event ordinal, entry index, segment index) of every events segment, in log order.
    ev_segs: Vec<(u32, usize, usize)>,
    format: LogFormat,
    /// Every dictionary listed in the MANIFEST (verified when loaded).
    dicts: DictSet,
    /// Format 4: dictionary used for new events segments, and the event count when it was listed.
    cur_dict: Option<SharedDict>,
    events_at_dict: u64,
    dict_policy: DictPolicy,
    clock: Box<dyn Fn() -> String + Send>,
}

fn valid_owner(owner: &str) -> bool {
    !owner.is_empty() && owner.len() <= 64 && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn sync_dir(p: &Path) {
    // Best effort: directories cannot be opened for sync on every platform.
    if let Ok(f) = fs::File::open(p) {
        let _ = f.sync_all();
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Read a segment file and check its sha256 against the manifest entry.
fn read_verified(dir: &Path, seg: &SegRef) -> Result<Vec<u8>, VerifyError> {
    let path = dir.join(&seg.file);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(VerifyError::MissingSegment { file: seg.file.clone() }),
        Err(e) => return Err(VerifyError::Io(format!("{}: {e}", seg.file))),
    };
    if sha256_hex(&[&bytes]) != seg.sha256 {
        return Err(VerifyError::ChecksumMismatch { file: seg.file.clone() });
    }
    Ok(bytes)
}

fn seg_table(seg: &SegRef) -> Result<Table, VerifyError> {
    Table::from_name(&seg.table).ok_or_else(|| VerifyError::SegmentUnreadable { file: seg.file.clone(), detail: format!("unknown table {}", seg.table) })
}

fn unreadable(seg: &SegRef, detail: impl Into<String>) -> VerifyError {
    VerifyError::SegmentUnreadable { file: seg.file.clone(), detail: detail.into() }
}

fn decode_seg_bytes(seg: &SegRef, table: Table, bytes: &[u8], dicts: &DictSet) -> Result<Vec<Vec<Cell>>, VerifyError> {
    let rows = match seg.enc {
        Some(2) => codec::decode_segment(table, bytes).map_err(|d| unreadable(seg, d))?,
        Some(v4log::ENC_V4) => v4log::decode(table, bytes, dicts).map_err(|d| unreadable(seg, d))?,
        None => {
            let block = kore_store::KoreReader::from_bytes(bytes).map_err(|e| unreadable(seg, e.to_string()))?;
            block_to_rows(table, &block).map_err(|d| unreadable(seg, d))?
        }
        Some(x) => return Err(unreadable(seg, format!("unknown segment encoding {x}"))),
    };
    if rows.len() as u64 != seg.rows {
        return Err(VerifyError::RowCountMismatch { file: seg.file.clone() });
    }
    Ok(rows)
}

/// Verified read + decode of a segment that uses no shared dictionary (formats 1-3; tests).
fn load_segment(dir: &Path, seg: &SegRef) -> Result<Vec<Vec<Cell>>, VerifyError> {
    load_seg(dir, seg, &DictSet::new())
}

/// Verified read + decode of any segment; `dicts` = the log's listed dictionaries.
fn load_seg(dir: &Path, seg: &SegRef, dicts: &DictSet) -> Result<Vec<Vec<Cell>>, VerifyError> {
    let bytes = read_verified(dir, seg)?;
    let table = seg_table(seg)?;
    decode_seg_bytes(seg, table, &bytes, dicts)
}

/// Id-only read is available (v2 codec or format 4).
fn ids_only(g: &SegRef) -> bool {
    matches!(g.enc, Some(2) | Some(v4log::ENC_V4))
}

/// Verified read of just the id column of a v2 events segment.
fn load_ids(dir: &Path, seg: &SegRef) -> Result<Vec<IdKey>, VerifyError> {
    let bytes = read_verified(dir, seg)?;
    let table = seg_table(seg)?;
    let ids = match seg.enc {
        Some(2) => codec::decode_first_col_ids(table, &bytes).map_err(|d| unreadable(seg, d))?,
        Some(v4log::ENC_V4) => v4log::decode_ids(table, &bytes).map_err(|d| unreadable(seg, d))?,
        _ => return Err(unreadable(seg, "id-only read needs a v2 or format-4 segment")),
    };
    if ids.len() as u64 != seg.rows {
        return Err(VerifyError::RowCountMismatch { file: seg.file.clone() });
    }
    Ok(ids)
}

fn read_manifest_raw(dir: &Path) -> Result<Vec<u8>, VerifyError> {
    match fs::read(dir.join("MANIFEST")) {
        Ok(r) => Ok(r),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(VerifyError::ManifestMissing),
        Err(e) => Err(VerifyError::Io(e.to_string())),
    }
}

fn read_manifest(dir: &Path) -> Result<Vec<Entry>, VerifyError> {
    manifest::parse(&read_manifest_raw(dir)?).map_err(VerifyError::ManifestCorrupt)
}

fn check_chain(entries: &[Entry]) -> Result<(), VerifyError> {
    let mut prev = ZERO_HASH.to_string();
    for (i, e) in entries.iter().enumerate() {
        if e.v != i as u64 {
            return Err(VerifyError::ChainBroken { version: i as u64, detail: format!("entry numbered {} at position {i}", e.v) });
        }
        if e.prev != prev {
            return Err(VerifyError::ChainBroken { version: e.v, detail: "previous-hash link does not match".into() });
        }
        if e.compute_hash() != e.hash {
            return Err(VerifyError::ChainBroken { version: e.v, detail: "entry hash does not match its contents".into() });
        }
        for g in &e.segments {
            let want = format!("{}/{:010}.kore", g.table, e.v);
            if g.file != want {
                return Err(VerifyError::ChainBroken { version: e.v, detail: format!("segment path {} (expected {want})", g.file) });
            }
        }
        prev = e.hash.clone();
    }
    v4log::check_listings(entries)
}

/// List `<table>/<file>` entries on disk that are not in `entries`.
fn stray_files(dir: &Path, entries: &[Entry]) -> Vec<String> {
    let known: HashSet<&str> = entries.iter().flat_map(|e| e.segments.iter().map(|s| s.file.as_str())).collect();
    let mut out = Vec::new();
    for t in ALL_TABLES {
        if let Ok(rd) = fs::read_dir(dir.join(t.name())) {
            for f in rd.flatten() {
                let rel = format!("{}/{}", t.name(), f.file_name().to_string_lossy());
                if !known.contains(rel.as_str()) {
                    out.push(rel);
                }
            }
        }
    }
    out.extend(stray_dicts(dir, entries));
    out.sort();
    out
}

/// Files in `dicts/` that no manifest entry lists.
fn stray_dicts(dir: &Path, entries: &[Entry]) -> Vec<String> {
    let known: HashSet<&str> = entries.iter().flat_map(|e| e.dicts.iter().map(|d| d.file.as_str())).collect();
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir.join(v4log::DICT_DIR)) {
        for f in rd.flatten() {
            let rel = format!("{}/{}", v4log::DICT_DIR, f.file_name().to_string_lossy());
            if !known.contains(rel.as_str()) {
                out.push(rel);
            }
        }
    }
    out
}

fn all_segs(entries: &[Entry]) -> Vec<(usize, usize)> {
    entries.iter().enumerate().flat_map(|(ei, e)| (0..e.segments.len()).map(move |si| (ei, si))).collect()
}

/// Full integrity check of a log directory, straight from disk. Every segment is
/// read, checksummed and decoded (in parallel); the first failure in log order is reported.
pub fn verify_dir(dir: &Path) -> Result<(), VerifyError> {
    let entries = read_manifest(dir)?;
    check_chain(&entries)?;
    // dictionary objects: each listed one present, sha256 = id, length as listed
    let dicts = v4log::load_dicts(dir, &entries)?;
    let listed = v4log::listed_at(&entries);
    let results: Vec<Result<(), VerifyError>> = all_segs(&entries)
        .par_iter()
        .map(|&(ei, si)| {
            let g = &entries[ei].segments[si];
            let bytes = read_verified(dir, g)?;
            let table = seg_table(g)?;
            if g.enc == Some(v4log::ENC_V4) {
                // every dictionary the segment references is listed at or before its version
                for id in v4log::seg_dict_ids(&bytes).map_err(|d| unreadable(g, d))? {
                    if listed.get(&id).map_or(true, |&at| at > ei) {
                        return Err(unreadable(g, format!("references dictionary {id} that is not listed at or before version {}", entries[ei].v)));
                    }
                }
            }
            decode_seg_bytes(g, table, &bytes, &dicts).map(|_| ())
        })
        .collect();
    for r in results {
        r?;
    }
    if let Some(f) = stray_files(dir, &entries).into_iter().next() {
        return Err(VerifyError::ExtraSegment { file: f });
    }
    Ok(())
}

fn assemble(entries: &[Entry], upto: usize, parts: Vec<Vec<Vec<Cell>>>, skip_events: bool) -> Vec<Vec<Vec<Cell>>> {
    let _ = skip_events;
    let mut tables: Vec<Vec<Vec<Cell>>> = vec![Vec::new(); ALL_TABLES.len()];
    let mut it = parts.into_iter();
    for e in &entries[..=upto] {
        for g in &e.segments {
            let rows = it.next().expect("one part per segment");
            let t = Table::from_name(&g.table).expect("checked by load");
            tables[t.idx()].extend(rows);
        }
    }
    tables
}

fn snapshot_from(dir: &Path, entries: &[Entry], upto: usize, dicts: &DictSet) -> Result<Snapshot, VerifyError> {
    let segs = all_segs(&entries[..=upto]);
    let results: Vec<Result<Vec<Vec<Cell>>, VerifyError>> = segs.par_iter().map(|&(ei, si)| load_seg(dir, &entries[ei].segments[si], dicts)).collect();
    let mut parts = Vec::with_capacity(results.len());
    for r in results {
        parts.push(r?);
    }
    let tables = assemble(entries, upto, parts, false);
    let e = &entries[upto];
    Ok(Snapshot { version: e.v, recorded_at: e.recorded_at.clone(), manifest_hash: e.hash.clone(), valid_at: e.recorded_at.clone(), tables: Arc::new(tables) })
}

/// Everything `open` derives from the segments.
struct State {
    head: Option<Snapshot>,
    index: Index,
    ev_segs: Vec<(u32, usize, usize)>,
    format: LogFormat,
    dicts: DictSet,
}

enum Part {
    Rows(Vec<Vec<Cell>>),
    Ids(Vec<IdKey>),
}

/// Read and verify every segment. If all events segments are v2, only their id
/// column is decoded (the text etc. is decoded later, on first use of the
/// snapshot) and the other tables are read a chunk of segments at a time and
/// folded into the index, so open never holds the whole log in memory
/// (`load_state_streamed`); otherwise everything is decoded now, as before.
fn load_state(dir: &Path, entries: &[Entry]) -> Result<State, VerifyError> {
    let lazy = entries.iter().flat_map(|e| e.segments.iter()).filter(|g| g.table == "events").all(ids_only);
    if lazy {
        if let Some(st) = load_state_streamed(dir, entries)? {
            return Ok(st);
        }
    }
    load_state_full(dir, entries)
}

/// Segments read and decoded at once (in parallel) by the streamed open and the events-only read.
const READ_CHUNK: usize = 512;

/// The streamed open of an all-v2 log. Gives exactly the index of `load_state_full`, provided every
/// revision_causes row is in the same commit as its revision and no rev_id is stored twice (true for
/// every log written through `commit_batch`); otherwise returns `None` and the caller falls back to
/// the full read, so an irregular log still opens with the same index as before.
fn load_state_streamed(dir: &Path, entries: &[Entry]) -> Result<Option<State>, VerifyError> {
    let dicts = v4log::load_dicts(dir, entries)?;
    let segs = all_segs(entries);
    let mut ix = Index::default();
    let mut ev_segs = Vec::new();
    let mut meta: Vec<Vec<Cell>> = Vec::new();
    // revisions and cause rows of the commit being read (a revision's causes are in its own commit)
    let mut cur = usize::MAX;
    let mut revs: Vec<RevisionRow> = Vec::new();
    let mut causes: HashMap<String, Vec<String>> = HashMap::new();
    fn flush(ix: &mut Index, revs: &mut Vec<RevisionRow>, causes: &mut HashMap<String, Vec<String>>) -> bool {
        for rv in revs.drain(..) {
            if ix.revs.contains_key(&rv.rev_id) {
                return false;
            }
            let c = causes.remove(&rv.rev_id).unwrap_or_default();
            ix.add_rev(rv, c);
        }
        causes.is_empty()
    }
    for chunk in segs.chunks(READ_CHUNK) {
        let parts: Vec<Result<Part, VerifyError>> = chunk
            .par_iter()
            .map(|&(ei, si)| {
                let g = &entries[ei].segments[si];
                if g.table == "events" {
                    load_ids(dir, g).map(Part::Ids)
                } else {
                    load_seg(dir, g, &dicts).map(Part::Rows)
                }
            })
            .collect();
        for (&(ei, si), r) in chunk.iter().zip(parts) {
            if ei != cur {
                if !flush(&mut ix, &mut revs, &mut causes) {
                    return Ok(None);
                }
                cur = ei;
            }
            let t = seg_table(&entries[ei].segments[si])?;
            match (r?, t) {
                (Part::Ids(v), _) => {
                    ev_segs.push((ix.next_ord, ei, si));
                    ix.events.reserve(v.len());
                    for k in v {
                        ix.ev_add(k);
                    }
                }
                (Part::Rows(_), Table::Events) => return Ok(None),
                (Part::Rows(rows), Table::Revisions) => revs.extend(rows.iter().map(|r| RevisionRow::from_cells(r))),
                (Part::Rows(rows), Table::RevisionCauses) => {
                    for r in &rows {
                        causes.entry(r[0].s()).or_default().push(r[1].s());
                    }
                }
                (Part::Rows(rows), Table::Predictions) => {
                    for r in &rows {
                        ix.add_pred(r);
                    }
                }
                (Part::Rows(rows), Table::Outcomes) => {
                    for r in &rows {
                        ix.outcomes.insert(r[0].s());
                    }
                }
                (Part::Rows(rows), Table::SchemaMeta) => meta.extend(rows),
            }
        }
    }
    if !flush(&mut ix, &mut revs, &mut causes) {
        return Ok(None);
    }
    let format = meta.iter().map(|r| LogFormat::from_number(MetaRow::from_cells(r).format_version)).fold(LogFormat::V1, |a, b| if b.number() > a.number() { b } else { a });
    ix.forgotten = forget::forgotten_set(entries);
    ix.content_idem = format.content_idem();
    Ok(Some(State { head: None, index: ix, ev_segs, format, dicts }))
}

/// Read and decode every segment at once (format 1 events, irregular logs).
fn load_state_full(dir: &Path, entries: &[Entry]) -> Result<State, VerifyError> {
    let dicts = v4log::load_dicts(dir, entries)?;
    let segs = all_segs(entries);
    let lazy = entries.iter().flat_map(|e| e.segments.iter()).filter(|g| g.table == "events").all(ids_only);
    let results: Vec<Result<Part, VerifyError>> = segs
        .par_iter()
        .map(|&(ei, si)| {
            let g = &entries[ei].segments[si];
            if lazy && g.table == "events" {
                load_ids(dir, g).map(Part::Ids)
            } else {
                load_seg(dir, g, &dicts).map(Part::Rows)
            }
        })
        .collect();
    let mut tables: Vec<Vec<Vec<Cell>>> = vec![Vec::new(); ALL_TABLES.len()];
    let mut ids: Vec<IdKey> = Vec::new();
    let mut ev_segs = Vec::new();
    for (&(ei, si), r) in segs.iter().zip(results) {
        let g = &entries[ei].segments[si];
        let t = seg_table(g)?;
        match r? {
            Part::Rows(rows) => {
                if t == Table::Events {
                    ev_segs.push((tables[Table::Events.idx()].len() as u32, ei, si));
                }
                tables[t.idx()].extend(rows);
            }
            Part::Ids(v) => {
                ev_segs.push((ids.len() as u32, ei, si));
                ids.extend(v);
            }
        }
    }
    let format = tables[Table::SchemaMeta.idx()].iter().map(|r| LogFormat::from_number(MetaRow::from_cells(r).format_version)).fold(LogFormat::V1, |a, b| if b.number() > a.number() { b } else { a });
    let last = entries.len() - 1;
    let mut index = if lazy { Index::build_with_ids(ids, &tables) } else { Index::build(&tables) };
    index.forgotten = forget::forgotten_set(entries);
    index.content_idem = format.content_idem();
    let e = &entries[last];
    let head = if lazy {
        None
    } else {
        Some(Snapshot { version: e.v, recorded_at: e.recorded_at.clone(), manifest_hash: e.hash.clone(), valid_at: e.recorded_at.clone(), tables: Arc::new(tables) })
    };
    Ok(State { head, index, ev_segs, format, dicts })
}

impl VersionedLog {
    /// Open `<root>/<owner>/log`, creating it (version 0 = genesis with
    /// `schema_meta`) if no MANIFEST exists. Does not delete anything when the
    /// log already exists; call `recover()` or just commit to clean leftovers.
    /// New logs use `DEFAULT_FORMAT` (format 3); see `open_or_create_with_format` for any other.
    pub fn open_or_create(root: &Path, owner: &str) -> Result<VersionedLog, LogError> {
        Self::open_or_create_with_clock(root, owner, now_iso)
    }

    /// Like `open_or_create` but with an explicit time source, which also
    /// stamps the genesis version (tests, replays, imports).
    pub fn open_or_create_with_clock(root: &Path, owner: &str, clock: impl Fn() -> String + Send + 'static) -> Result<VersionedLog, LogError> {
        Self::open_or_create_with_format(root, owner, clock, DEFAULT_FORMAT)
    }

    /// Like `open_or_create_with_clock`; `format` applies only when the log is
    /// created here. An existing log keeps the format it has (upgrade is explicit:
    /// `upgrade_to_v2`).
    pub fn open_or_create_with_format(root: &Path, owner: &str, clock: impl Fn() -> String + Send + 'static, format: LogFormat) -> Result<VersionedLog, LogError> {
        if !valid_owner(owner) {
            return inv("owner must be 1-64 chars of [A-Za-z0-9_-]");
        }
        let dir = root.join(owner).join("log");
        fs::create_dir_all(&dir)?;
        if !dir.join("MANIFEST").exists() {
            for t in ALL_TABLES {
                fs::create_dir_all(dir.join(t.name()))?;
            }
            let mut log = VersionedLog {
                dir: dir.clone(),
                entries: vec![],
                manifest_raw: format!("{}\n", manifest::header_for(format.manifest_level())).into_bytes(),
                journal: format.manifest_level() >= 2,
                level: format.manifest_level(),
                torn: vec![],
                head: Mutex::new(Some(Snapshot { version: 0, recorded_at: String::new(), manifest_hash: String::new(), valid_at: String::new(), tables: Arc::new(vec![Vec::new(); ALL_TABLES.len()]) })),
                index: Index { content_idem: format.content_idem(), ..Index::default() },
                ev_segs: vec![],
                format,
                dicts: DictSet::new(),
                cur_dict: None,
                events_at_dict: 0,
                dict_policy: DictPolicy::default(),
                clock: Box::new(clock),
            };
            log.recover()?;
            let now = (log.clock)();
            let meta = MetaRow { format_version: format.number(), created_at: now.clone() };
            log.write_commit(vec![(Table::SchemaMeta, vec![meta.to_cells()])], now, format, None)?;
            return Ok(log);
        }
        let mut log = Self::open_existing(dir)?;
        log.clock = Box::new(clock);
        Ok(log)
    }

    /// Open an existing log; fails if the MANIFEST is missing or invalid.
    pub fn open(root: &Path, owner: &str) -> Result<VersionedLog, LogError> {
        if !valid_owner(owner) {
            return inv("owner must be 1-64 chars of [A-Za-z0-9_-]");
        }
        Self::open_existing(root.join(owner).join("log"))
    }

    fn open_existing(dir: PathBuf) -> Result<VersionedLog, LogError> {
        // Finish (or discard) a forget that was interrupted by a crash.
        recover_pending(&dir)?;
        let full = read_manifest_raw(&dir)?;
        let parsed = manifest::parse_ex(&full).map_err(VerifyError::ManifestCorrupt)?;
        let (entries, journal, level) = (parsed.entries, parsed.journal, parsed.level);
        let torn = full[parsed.valid_len..].to_vec();
        let mut raw = full;
        raw.truncate(parsed.valid_len);
        check_chain(&entries)?;
        if entries.is_empty() {
            return Err(VerifyError::ManifestCorrupt("no entries".into()).into());
        }
        let st = load_state(&dir, &entries)?;
        let (cur_dict, events_at_dict) = v4log::current(&entries, &st.dicts);
        Ok(VersionedLog {
            dir, entries, manifest_raw: raw, journal, level, torn, head: Mutex::new(st.head), index: st.index, ev_segs: st.ev_segs, format: st.format,
            dicts: st.dicts, cur_dict, events_at_dict, dict_policy: DictPolicy::default(), clock: Box::new(now_iso),
        })
    }

    /// Read the first events of an existing log without opening it: the MANIFEST hash chain is
    /// checked and the events segments are read in log order (each checksum verified) until `f`
    /// returns false; nothing else is read and no index is built, so the cost does not grow with the
    /// log when `f` stops early. For a quick look before a full `open` (the being's genesis when the
    /// KILL file is present). An interrupted forget is finished first, as `open` does.
    pub fn peek_events(root: &Path, owner: &str, mut f: impl FnMut(EventRow) -> bool) -> Result<(), LogError> {
        if !valid_owner(owner) {
            return inv("owner must be 1-64 chars of [A-Za-z0-9_-]");
        }
        let dir = root.join(owner).join("log");
        recover_pending(&dir)?;
        let full = read_manifest_raw(&dir)?;
        let parsed = manifest::parse_ex(&full).map_err(VerifyError::ManifestCorrupt)?;
        check_chain(&parsed.entries)?;
        let dicts = v4log::load_dicts(&dir, &parsed.entries)?;
        for e in &parsed.entries {
            for g in e.segments.iter().filter(|g| g.table == "events") {
                for r in load_seg(&dir, g, &dicts)? {
                    if !f(EventRow::from_cells(&r)) {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    /// Re-derive all in-memory state from disk (used after `forget`).
    fn reload(&mut self) -> Result<(), LogError> {
        let full = read_manifest_raw(&self.dir)?;
        let parsed = manifest::parse_ex(&full).map_err(VerifyError::ManifestCorrupt)?;
        let entries = parsed.entries;
        self.torn = full[parsed.valid_len..].to_vec();
        self.journal = parsed.journal;
        self.level = parsed.level;
        let mut raw = full;
        raw.truncate(parsed.valid_len);
        check_chain(&entries)?;
        let st = load_state(&self.dir, &entries)?;
        self.entries = entries;
        self.manifest_raw = raw;
        *self.head.get_mut().unwrap_or_else(|e| e.into_inner()) = st.head;
        self.index = st.index;
        self.ev_segs = st.ev_segs;
        self.format = st.format;
        let (cur_dict, events_at_dict) = v4log::current(&self.entries, &st.dicts);
        self.dicts = st.dicts;
        self.cur_dict = cur_dict;
        self.events_at_dict = events_at_dict;
        Ok(())
    }

    /// Replace the time source (tests / replays). `recorded_at` is still forced
    /// to be non-decreasing.
    pub fn set_clock(&mut self, f: impl Fn() -> String + Send + 'static) {
        self.clock = Box::new(f);
    }

    /// Check an event against the same rules a commit applies (without writing).
    pub fn check_new_event(e: &NewEvent) -> Result<(), LogError> {
        check_event(e).map(|_| ())
    }

    /// True if an event with this id is stored (latest version).
    pub fn has_event(&self, id: &str) -> bool {
        self.index.ev_has(id)
    }

    /// Number of event rows in the latest version.
    pub fn event_count(&self) -> usize {
        self.index.next_ord as usize
    }

    /// The stored row for this event id (latest version), if any. Uses the id
    /// index: at most one segment is read (and its checksum verified) when the
    /// snapshot is not already in memory; no scan over all events.
    pub fn get_event(&self, id: &str) -> Result<Option<EventRow>, LogError> {
        let ord = match self.index.ev_ord(id) {
            Some(o) => o,
            None => return Ok(None),
        };
        {
            let g = self.head.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(s) = g.as_ref() {
                return Ok(s.tables[Table::Events.idx()].get(ord as usize).map(|r| EventRow::from_cells(r)));
            }
        }
        let k = self.ev_segs.partition_point(|s| s.0 <= ord) - 1;
        let (start, ei, si) = self.ev_segs[k];
        let seg = &self.entries[ei].segments[si];
        let at = (ord - start) as usize;
        if seg.enc == Some(2) {
            // Verified read, then materialise only this row (not the whole segment).
            let bytes = read_verified(&self.dir, seg)?;
            let cells = codec::decode_row(seg_table(seg)?, &bytes, at).map_err(|d| unreadable(seg, d))?;
            return Ok(Some(EventRow::from_cells(&cells)));
        }
        let rows = load_seg(&self.dir, seg, &self.dicts)?;
        Ok(rows.get(at).map(|r| EventRow::from_cells(r)))
    }

    /// Every event row of the latest version, in log order, without decoding any other table and
    /// without keeping the rows: the events segments are read (checksums verified) a chunk at a time.
    /// If the latest snapshot is already in memory its rows are used. `f` returns false to stop.
    /// Gives the same rows, in the same order, as `open_latest().rows::<EventRow>()`.
    pub fn for_each_event(&self, f: impl FnMut(EventRow) -> bool) -> Result<(), LogError> {
        self.each_event_from(0, f)
    }

    /// The events of `ev_segs[first..]` (all events segments from that one on), in log order.
    fn each_event_from(&self, first: usize, f: impl FnMut(EventRow) -> bool) -> Result<(), LogError> {
        self.each_event_range(first, self.ev_segs.len(), f)
    }

    /// The events of `ev_segs[first..end]`, in log order.
    pub(crate) fn each_event_range(&self, first: usize, end: usize, mut f: impl FnMut(EventRow) -> bool) -> Result<(), LogError> {
        let end = end.min(self.ev_segs.len());
        let first = first.min(end);
        let head = self.head.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(s) = head {
            let rows = &s.tables[Table::Events.idx()];
            let ord = |k: usize| self.ev_segs.get(k).map_or(rows.len(), |x| x.0 as usize);
            let (a, b) = (ord(first), ord(end));
            for r in rows[a.min(b)..b.min(rows.len())].iter() {
                if !f(EventRow::from_cells(r)) {
                    break;
                }
            }
            return Ok(());
        }
        // small first chunks, so that a read that stops early (the genesis) touches few segments
        let mut at = first;
        let mut n = 8usize;
        while at < end {
            let chunk = &self.ev_segs[at..(at + n).min(end)];
            at += chunk.len();
            n = (n * 2).min(READ_CHUNK);
            let (dir, entries, dicts) = (&self.dir, &self.entries, &self.dicts);
            let parts: Vec<Result<Vec<Vec<Cell>>, VerifyError>> = chunk.par_iter().map(|&(_, ei, si)| load_seg(dir, &entries[ei].segments[si], dicts)).collect();
            for p in parts {
                for r in p? {
                    if !f(EventRow::from_cells(&r)) {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    /// True if this event id was removed by `forget` (ingest of it is refused).
    pub fn is_forgotten(&self, id: &str) -> bool {
        self.index.forgotten.contains(&forget_hash(id))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The write format of this log (the newest `schema_meta.format_version`).
    pub fn format(&self) -> LogFormat {
        self.format
    }

    pub fn latest_version(&self) -> u64 {
        self.entries.last().map(|e| e.v).unwrap_or(0)
    }

    /// Delete segment files (and temp files) that no manifest entry lists.
    pub fn recover(&mut self) -> Result<Vec<String>, LogError> {
        let strays = stray_files(&self.dir, &self.entries);
        for f in &strays {
            let _ = fs::remove_file(self.dir.join(f));
        }
        let _ = fs::remove_file(self.dir.join("MANIFEST.tmp"));
        self.drop_torn_tail()?;
        Ok(strays)
    }

    /// Journal manifest: cut an uncommitted partial last line (a crashed append) off the file.
    fn drop_torn_tail(&mut self) -> Result<(), LogError> {
        if self.journal && !self.torn.is_empty() {
            let f = fs::OpenOptions::new().write(true).open(self.dir.join("MANIFEST"))?;
            f.set_len(self.manifest_raw.len() as u64)?;
            f.sync_all()?;
            self.torn.clear();
        }
        Ok(())
    }

    /// Cheap per-commit cleanup: only the file names a crashed commit could have
    /// left for the next two versions (full scan: `recover`).
    fn recover_next(&self, v: u64) {
        for t in ALL_TABLES {
            for k in [v, v + 1] {
                let p = self.dir.join(t.name()).join(format!("{k:010}.kore"));
                let _ = fs::remove_file(&p);
                let _ = fs::remove_file(p.with_extension("kore.tmp"));
            }
        }
        let _ = fs::remove_file(self.dir.join("MANIFEST.tmp"));
        if self.format == LogFormat::V4 {
            // a crashed commit may have left a dictionary object that no entry lists (few files)
            for f in stray_dicts(&self.dir, &self.entries) {
                let _ = fs::remove_file(self.dir.join(f));
            }
        }
    }

    pub fn verify(&self) -> Result<(), VerifyError> {
        verify_dir(&self.dir)
    }

    /// Verify and additionally require the on-disk head hash to equal a value
    /// recorded elsewhere (detects rollback by manifest truncation).
    pub fn verify_against(&self, expected_head: &str) -> Result<(), VerifyError> {
        verify_dir(&self.dir)?;
        let entries = read_manifest(&self.dir)?;
        let found = entries.last().map(|e| e.hash.clone()).unwrap_or_default();
        if found != expected_head {
            return Err(VerifyError::HeadMismatch { expected: expected_head.into(), found });
        }
        Ok(())
    }

    pub fn versions(&self) -> Vec<VersionInfo> {
        let mut cum: BTreeMap<String, u64> = ALL_TABLES.iter().map(|t| (t.name().to_string(), 0)).collect();
        self.entries
            .iter()
            .map(|e| {
                for g in &e.segments {
                    *cum.entry(g.table.clone()).or_insert(0) += g.rows;
                }
                VersionInfo { v: e.v, recorded_at: e.recorded_at.clone(), rows: cum.clone(), manifest_hash: e.hash.clone() }
            })
            .collect()
    }

    pub fn snapshot_hash(&self, v: u64) -> Result<String, LogError> {
        self.entries.get(v as usize).map(|e| e.hash.clone()).ok_or_else(|| LogError::NotFound(format!("version {v}")))
    }

    /// The latest snapshot. For an all-v2 log the rows are decoded on the first
    /// call (open only indexes ids) and then kept; a clone is cheap (shared rows).
    /// Fails only if the files changed on disk since `open`.
    pub fn try_open_latest(&self) -> Result<Snapshot, LogError> {
        let mut g = self.head.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            *g = Some(snapshot_from(&self.dir, &self.entries, self.entries.len() - 1, &self.dicts)?);
        }
        Ok(g.as_ref().expect("just set").clone())
    }

    /// Like `try_open_latest`; panics if the log files were modified after `open`
    /// (the old code could not observe that; use `try_open_latest` to handle it).
    pub fn open_latest(&self) -> Snapshot {
        self.try_open_latest().unwrap_or_else(|e| panic!("versioned log changed on disk after open: {e}"))
    }

    pub fn open_at(&self, at: AsOf) -> Result<Snapshot, LogError> {
        let upto = match at {
            AsOf::Version(v) => {
                if v as usize >= self.entries.len() {
                    return Err(LogError::NotFound(format!("version {v} (latest is {})", self.latest_version())));
                }
                v as usize
            }
            AsOf::Time(ts) => {
                let ts = normalize_ts(&ts).map_err(LogError::Invalid)?;
                match self.entries.iter().rposition(|e| e.recorded_at <= ts) {
                    Some(i) => i,
                    None => return Err(LogError::NotFound(format!("no version recorded at or before {ts}"))),
                }
            }
        };
        Ok(snapshot_from(&self.dir, &self.entries, upto, &self.dicts)?)
    }

    /// Rows added by commits `v1+1 ..= v2`.
    pub fn diff(&self, v1: u64, v2: u64) -> Result<Diff, LogError> {
        if v1 > v2 || v2 as usize >= self.entries.len() {
            return Err(LogError::NotFound(format!("diff range {v1}..{v2}")));
        }
        let mut d = Diff { from: v1, to: v2, ..Default::default() };
        for e in &self.entries[(v1 as usize + 1)..=(v2 as usize)] {
            for g in &e.segments {
                let rows = load_seg(&self.dir, g, &self.dicts)?;
                match Table::from_name(&g.table) {
                    Some(Table::Events) => d.events.extend(rows.iter().map(|r| EventRow::from_cells(r))),
                    Some(Table::Revisions) => d.revisions.extend(rows.iter().map(|r| RevisionRow::from_cells(r))),
                    Some(Table::RevisionCauses) => d.causes.extend(rows.iter().map(|r| CauseRow::from_cells(r))),
                    Some(Table::Predictions) => d.predictions.extend(rows.iter().map(|r| PredictionRow::from_cells(r))),
                    Some(Table::Outcomes) => d.outcomes.extend(rows.iter().map(|r| OutcomeRow::from_cells(r))),
                    _ => {}
                }
            }
        }
        Ok(d)
    }

    // ── mutating API ─────────────────────────────────────────────────────────

    pub fn append_events(&mut self, events: &[NewEvent]) -> Result<CommitInfo, LogError> {
        self.commit_batch(Batch { events: events.to_vec(), ..Default::default() })
    }

    /// One revision = one commit. Returns the rev_id (existing id if identical).
    pub fn revise(&mut self, r: NewRevision) -> Result<String, LogError> {
        let info = self.commit_batch(Batch { revisions: vec![r], ..Default::default() })?;
        Ok(info.rev_ids.into_iter().next().expect("one revision requested"))
    }

    pub fn record_prediction(&mut self, p: NewPrediction) -> Result<CommitInfo, LogError> {
        self.commit_batch(Batch { predictions: vec![p], ..Default::default() })
    }

    pub fn record_outcome(&mut self, o: NewOutcome) -> Result<CommitInfo, LogError> {
        self.commit_batch(Batch { outcomes: vec![o], ..Default::default() })
    }

    /// Detect a concurrent writer / on-disk tampering of the MANIFEST: the file
    /// must still be byte-identical to what this handle last wrote or read.
    fn check_manifest_unchanged(&self) -> Result<(), LogError> {
        let conflict = || LogError::Conflict("MANIFEST changed on disk since this log was opened".into());
        if !self.journal || !self.torn.is_empty() {
            let on_disk = read_manifest_raw(&self.dir)?;
            let mut want = self.manifest_raw.clone();
            want.extend_from_slice(&self.torn);
            return if on_disk == want { Ok(()) } else { Err(conflict()) };
        }
        // Journal: O(1) check, length plus the last bytes (a concurrent append changes both).
        use std::io::{Read, Seek, SeekFrom};
        let path = self.dir.join("MANIFEST");
        let mut f = fs::File::open(&path).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { LogError::from(VerifyError::ManifestMissing) } else { LogError::from(VerifyError::Io(e.to_string())) })?;
        let len = f.metadata()?.len();
        if len != self.manifest_raw.len() as u64 {
            return Err(conflict());
        }
        let k = (len as usize).min(512);
        f.seek(SeekFrom::Start(len - k as u64))?;
        let mut tail = vec![0u8; k];
        f.read_exact(&mut tail)?;
        if tail != self.manifest_raw[self.manifest_raw.len() - k..] {
            return Err(conflict());
        }
        Ok(())
    }

    /// Explicit, one-way upgrade of this log to format 2: one new commit records
    /// `schema_meta.format_version = 2`; every later segment is written in the
    /// v2 encoding and revisions become content-hash idempotent. Existing
    /// segments, versions and hashes are untouched (a mixed-encoding log). Old
    /// binaries cannot read the new segments. Returns false if already format 2.
    pub fn upgrade_to_v2(&mut self) -> Result<bool, LogError> {
        if self.format == LogFormat::V2 || self.format == LogFormat::V4 {
            return Ok(false);
        }
        self.check_manifest_unchanged()?;
        let last_at = self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
        let mut now = (self.clock)();
        if now < last_at {
            now = last_at;
        }
        let meta = MetaRow { format_version: FORMAT_V2, created_at: now.clone() };
        self.write_commit(vec![(Table::SchemaMeta, vec![meta.to_cells()])], now, LogFormat::V2, None)?;
        self.format = LogFormat::V2;
        self.index.content_idem = true;
        Ok(true)
    }

    /// Explicit, one-way upgrade to format 3 (from 1 or 2): one new commit records
    /// `schema_meta.format_version = 3` and, in the same atomic MANIFEST replacement, switches the
    /// MANIFEST to the journal header (all earlier entry lines are kept byte for byte, so the hash
    /// chain and every old version hash are unchanged). Later commits append. Segments written from
    /// here on use the format 2 encoding. Old binaries cannot read the result. False if already 3.
    pub fn upgrade_to_v3(&mut self) -> Result<bool, LogError> {
        if self.format == LogFormat::V3 || self.format == LogFormat::V4 {
            return Ok(false);
        }
        self.check_manifest_unchanged()?;
        let last_at = self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
        let mut now = (self.clock)();
        if now < last_at {
            now = last_at;
        }
        let meta = MetaRow { format_version: FORMAT_V3, created_at: now.clone() };
        self.write_commit(vec![(Table::SchemaMeta, vec![meta.to_cells()])], now, LogFormat::V3, None)?;
        self.format = LogFormat::V3;
        self.index.content_idem = true;
        Ok(true)
    }

    /// Validate everything, then write at most one new version atomically.
    /// Within a batch, revisions may cite events of the same batch, and
    /// outcomes may reference predictions of the same batch.
    pub fn commit_batch(&mut self, batch: Batch) -> Result<CommitInfo, LogError> {
        let last_at = self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
        let mut now = (self.clock)();
        if now < last_at {
            now = last_at;
        }
        let mut undo: Vec<Undo> = Vec::new();
        let staged = self.stage_batch(&batch, &now, &mut undo);
        let (new, event_ids, rev_ids, pred_ids, suppressed) = match staged {
            Ok(x) => x,
            Err(e) => {
                self.index.rollback(undo);
                return Err(e);
            }
        };
        if new.is_empty() {
            self.index.rollback(undo);
            return Ok(CommitInfo {
                version: self.latest_version(),
                created: false,
                recorded_at: self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default(),
                new_rows: BTreeMap::new(),
                event_ids,
                rev_ids,
                pred_ids,
                suppressed,
            });
        }
        // Conflict check only when something will be written (a no-op re-submission reads nothing from
        // disk; the log is append-only, so "already stored" cannot become false behind our back).
        if let Err(e) = self.check_manifest_unchanged() {
            self.index.rollback(undo);
            return Err(e);
        }
        let new_rows: BTreeMap<String, usize> = new.iter().map(|(t, r)| (t.name().to_string(), r.len())).collect();
        let ev_rows = new.iter().find(|(t, _)| *t == Table::Events).map(|(_, r)| r.len()).unwrap_or(0);
        let ev_seg_pos = new.iter().position(|(t, _)| *t == Table::Events);
        let ev_start = self.index.next_ord - ev_rows as u32;
        let fmt = self.format;
        let v = self.entries.len();
        // Stamp recorded_at into rows that carry it (done in validate_*), then write.
        if let Err(e) = self.write_commit(new, now.clone(), fmt, None) {
            self.index.rollback(undo);
            return Err(e);
        }
        if let Some(si) = ev_seg_pos {
            self.ev_segs.push((ev_start, v, si));
        }
        Ok(CommitInfo { version: self.latest_version(), created: true, recorded_at: now, new_rows, event_ids, rev_ids, pred_ids, suppressed })
    }

    /// Validate the batch and register its ids in the index (recorded in `undo`).
    #[allow(clippy::type_complexity)]
    fn stage_batch(&mut self, batch: &Batch, now: &str, undo: &mut Vec<Undo>) -> Result<(Vec<(Table, Vec<Vec<Cell>>)>, Vec<String>, Vec<String>, Vec<String>, usize), LogError> {
        let mut new: Vec<(Table, Vec<Vec<Cell>>)> = Vec::new();
        let (mut ev, mut rv, mut ca, mut pr, mut oc) = (vec![], vec![], vec![], vec![], vec![]);
        let (mut event_ids, mut rev_ids, mut pred_ids) = (vec![], vec![], vec![]);
        let mut suppressed = 0usize;
        let ix = &mut self.index;

        for e in &batch.events {
            // Cheap path first: a duplicate (re-ingest) never builds a row.
            let (occurred_at, id) = check_event(e)?;
            if ix.forgotten.contains(&forget_hash(&id)) {
                suppressed += 1;
                continue;
            }
            let key = IdKey::from_str(&id);
            event_ids.push(id.clone());
            if ix.ev_add(key.clone()) {
                undo.push(Undo::Event(key));
                ev.push(build_event_row(e, occurred_at, id, now).to_cells());
            } else {
                // duplicate: the ordinal counter was bumped by ev_add; undo that
                ix.next_ord -= 1;
            }
        }
        for r in &batch.revisions {
            let (row, causes, digest) = validate_revision(r, ix, now)?;
            rev_ids.push(row.rev_id.clone());
            if ix.revs.contains_key(&row.rev_id) {
                continue;
            }
            let hk = (row.entity_type.clone(), row.key.clone());
            let prev_head = ix.heads.get(&hk).cloned();
            ix.revs.insert(row.rev_id.clone(), hk.clone());
            ix.heads.insert(hk.clone(), (row.rev_id.clone(), digest.clone()));
            let added_content = !ix.content_ids.contains_key(&digest);
            if added_content {
                ix.content_ids.insert(digest.clone(), row.rev_id.clone());
            }
            undo.push(Undo::Rev { rev_id: row.rev_id.clone(), hk, prev_head, digest, added_content });
            for c in causes {
                ca.push(CauseRow { rev_id: row.rev_id.clone(), event_id: c }.to_cells());
            }
            rv.push(row.to_cells());
        }
        for p in &batch.predictions {
            let row = validate_prediction(p, ix)?;
            pred_ids.push(row.pred_id.clone());
            if !ix.preds.contains_key(&row.pred_id) {
                ix.preds.insert(row.pred_id.clone(), p.options.clone());
                undo.push(Undo::Pred(row.pred_id.clone()));
                pr.push(row.to_cells());
            }
        }
        for o in &batch.outcomes {
            let row = validate_outcome(o, ix)?;
            if ix.outcomes.insert(row.pred_id.clone()) {
                undo.push(Undo::Outcome(row.pred_id.clone()));
                oc.push(row.to_cells());
            }
        }
        for (t, rows) in [(Table::Events, ev), (Table::Revisions, rv), (Table::RevisionCauses, ca), (Table::Predictions, pr), (Table::Outcomes, oc)] {
            if !rows.is_empty() {
                new.push((t, rows));
            }
        }
        Ok((new, event_ids, rev_ids, pred_ids, suppressed))
    }

    /// Write segments, then atomically replace the MANIFEST (temp file, fsync,
    /// rename). The commit exists only once the rename succeeds. The new MANIFEST
    /// is the previous bytes plus one line; nothing is re-parsed or re-rendered.
    fn write_commit(&mut self, new: Vec<(Table, Vec<Vec<Cell>>)>, recorded_at: String, fmt: LogFormat, forget: Option<manifest::ForgetRecord>) -> Result<(), LogError> {
        let v = self.entries.len() as u64;
        if v == 0 {
            self.recover()?;
        } else {
            self.recover_next(v);
        }
        // format 4: the dictionary for the events segment (maybe a new object, listed by this commit)
        let (dict, new_dict) = match (fmt, new.iter().find(|(t, _)| *t == Table::Events)) {
            (LogFormat::V4, Some((_, rows))) => self.choose_dict(rows)?,
            _ => (None, false),
        };
        let mut dict_refs = Vec::new();
        if let (Some(d), true) = (&dict, new_dict) {
            let r = v4log::dict_ref(d);
            let ddir = self.dir.join(v4log::DICT_DIR);
            fs::create_dir_all(&ddir)?;
            let tmp = self.dir.join(format!("{}.tmp", r.file));
            write_synced(&tmp, d.to_bytes())?;
            fs::rename(&tmp, self.dir.join(&r.file))?;
            sync_dir(&ddir);
            dict_refs.push(r);
        }
        let mut segs = Vec::new();
        for (t, rows) in &new {
            let bytes = encode_seg(fmt.enc(), *t, rows, dict.as_ref());
            let rel = format!("{}/{:010}.kore", t.name(), v);
            let tdir = self.dir.join(t.name());
            fs::create_dir_all(&tdir)?;
            let tmp = self.dir.join(format!("{rel}.tmp"));
            write_synced(&tmp, &bytes)?;
            fs::rename(&tmp, self.dir.join(&rel))?;
            sync_dir(&tdir);
            segs.push(SegRef { table: t.name().to_string(), file: rel, sha256: sha256_hex(&[&bytes]), rows: rows.len() as u64, enc: fmt.enc() });
        }
        let prev = self.entries.last().map(|e| e.hash.clone()).unwrap_or_else(|| ZERO_HASH.to_string());
        let mut entry = Entry::new(v, recorded_at, segs, &prev);
        if forget.is_some() || !dict_refs.is_empty() {
            entry.forget = forget;
            entry.dicts = dict_refs;
            entry.reseal(&prev);
        }
        let line = serde_json::to_string(&entry).expect("entry serialises");
        let want_level = fmt.manifest_level();
        let path = self.dir.join("MANIFEST");
        if self.journal && v > 0 && self.level >= want_level {
            // Journal append: the segments are already durable; the commit exists once this line
            // (with its newline) is fsynced. A crash in between leaves an unterminated tail that
            // `open` ignores. No rename, no directory entry changes, nothing is rewritten.
            self.drop_torn_tail()?;
            let mut rec = Vec::with_capacity(line.len() + 1);
            rec.extend_from_slice(line.as_bytes());
            rec.push(b'\n');
            let mut f = fs::OpenOptions::new().append(true).open(&path)?;
            f.write_all(&rec)?;
            f.sync_all()?;
            self.manifest_raw.extend_from_slice(&rec);
        } else {
            // Atomic whole-file replacement (format 1/2 logs, genesis, or the switch to the journal header).
            let mut raw = Vec::with_capacity(self.manifest_raw.len() + line.len() + 1);
            if want_level > self.level {
                raw.extend_from_slice(manifest::header_for(want_level).as_bytes());
                raw.push(b'\n');
                let old_hdr = self.manifest_raw.iter().position(|&b| b == b'\n').map(|p| p + 1).unwrap_or(self.manifest_raw.len());
                raw.extend_from_slice(&self.manifest_raw[old_hdr..]);
            } else {
                raw.extend_from_slice(&self.manifest_raw);
            }
            raw.extend_from_slice(line.as_bytes());
            raw.push(b'\n');
            let tmp = self.dir.join("MANIFEST.tmp");
            write_synced(&tmp, &raw)?;
            fs::rename(&tmp, &path)?;
            sync_dir(&self.dir);
            self.manifest_raw = raw;
            if want_level > self.level {
                self.level = want_level;
                self.journal = want_level >= 2;
            }
            self.torn.clear();
        }
        // Commit point passed: update memory.
        if let (Some(d), true) = (dict.as_ref(), new_dict) {
            self.dicts.add(d.clone());
            self.cur_dict = Some(d.clone());
            self.events_at_dict = self.index.next_ord as u64;
        }
        self.entries.push(entry);
        let e = self.entries.last().expect("just pushed");
        if let Some(h) = self.head.get_mut().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let t = Arc::make_mut(&mut h.tables);
            for (tb, rows) in new {
                t[tb.idx()].extend(rows);
            }
            h.version = e.v;
            h.recorded_at = e.recorded_at.clone();
            h.manifest_hash = e.hash.clone();
            h.valid_at = e.recorded_at.clone();
        }
        Ok(())
    }

    /// Test hook: write segments for a would-be commit but "crash" before the
    /// MANIFEST rename. The log must stay valid at the previous version.
    #[cfg(test)]
    pub(crate) fn simulate_crash_after_segments(&mut self, batch: Batch) -> Result<(), LogError> {
        let v = self.entries.len() as u64;
        let now = (self.clock)();
        let ev: Vec<Vec<Cell>> = batch.events.iter().map(|e| validate_event(e, &now).map(|r| r.to_cells())).collect::<Result<_, _>>()?;
        let bytes = encode_seg(self.format.enc(), Table::Events, &ev, None);
        let tdir = self.dir.join("events");
        write_synced(&tdir.join(format!("{v:010}.kore")), &bytes)?;
        write_synced(&tdir.join(format!("{:010}.kore.tmp", v + 1)), b"partial")?;
        write_synced(&self.dir.join("MANIFEST.tmp"), b"partial manifest")?;
        Ok(())
    }
}

// ─── Validation ──────────────────────────────────────────────────────────────

fn check_text(name: &str, s: &str, max: usize) -> Result<(), LogError> {
    if s.trim().is_empty() {
        return inv(format!("{name} must not be empty"));
    }
    if s.len() > max {
        return inv(format!("{name} too long ({} > {max} bytes)", s.len()));
    }
    Ok(())
}

/// Field checks; returns the normalised `occurred_at` and the event id (no row is built yet).
fn check_event(e: &NewEvent) -> Result<(String, String), LogError> {
    check_text("source", &e.source, 256)?;
    if e.source.contains('|') {
        return inv("source must not contain '|'");
    }
    check_text("text", &e.text, 1 << 20)?;
    if !EVENT_KINDS.contains(&e.kind.as_str()) {
        return inv(format!("kind must be one of {EVENT_KINDS:?}"));
    }
    let occurred_at = normalize_ts(&e.occurred_at).map_err(LogError::Invalid)?;
    for t in &e.tags {
        if t.is_empty() || t.contains(',') {
            return inv("tags must be non-empty and contain no commas");
        }
    }
    let id = event_id(&e.source, &occurred_at, &e.text);
    Ok((occurred_at, id))
}

fn build_event_row(e: &NewEvent, occurred_at: String, event_id: String, now: &str) -> EventRow {
    EventRow { event_id, occurred_at, recorded_at: now.to_string(), source: e.source.clone(), kind: e.kind.clone(), text: e.text.clone(), tags: e.tags.join(","), redacted: e.redacted }
}

fn validate_event(e: &NewEvent, now: &str) -> Result<EventRow, LogError> {
    let (occurred_at, id) = check_event(e)?;
    Ok(build_event_row(e, occurred_at, id, now))
}

#[allow(clippy::too_many_arguments)]
fn revision_content(t: &str, key: &str, stance: &str, conf: &str, rationale: &str, valid_from: &str, author: &str, causes: &[String]) -> String {
    sha256_hex(&[
        t.as_bytes(), &[0x1f], key.as_bytes(), &[0x1f], stance.as_bytes(), &[0x1f], conf.as_bytes(), &[0x1f], rationale.as_bytes(),
        &[0x1f], valid_from.as_bytes(), &[0x1f], author.as_bytes(), &[0x1f], causes.join(",").as_bytes(),
    ])
}

/// Returns (row, sorted causes, content digest). If `supersedes` is not given
/// and the current head already has identical content, the row returned
/// carries the head's rev_id so the caller treats it as an existing revision.
fn validate_revision(r: &NewRevision, ix: &Index, now: &str) -> Result<(RevisionRow, Vec<String>, String), LogError> {
    if !ENTITY_TYPES.contains(&r.entity_type.as_str()) {
        return inv(format!("entity_type must be one of {ENTITY_TYPES:?}"));
    }
    check_text("key", &r.key, 512)?;
    check_text("stance", &r.stance, 1 << 16)?;
    if !r.confidence.is_finite() || !(0.0..=1.0).contains(&r.confidence) {
        return inv("confidence must be within 0..=1");
    }
    let valid_from = normalize_ts(&r.valid_from).map_err(LogError::Invalid)?;
    let rule_ok = r.author.strip_prefix("rule:").map(|id| !id.trim().is_empty()).unwrap_or(false);
    if r.author != "user" && r.author != "legacy" && !rule_ok {
        return inv("author must be 'user', 'legacy' or 'rule:<id>'");
    }
    let mut causes: Vec<String> = r.caused_by.clone();
    causes.sort();
    causes.dedup();
    if rule_ok && causes.is_empty() {
        return inv("rule-authored revisions require at least one cited event (caused_by)");
    }
    for c in &causes {
        if !ix.ev_has(c) {
            return Err(LogError::NotFound(format!("caused_by event {c}")));
        }
    }
    let k = (r.entity_type.clone(), r.key.clone());
    let supersedes = match &r.supersedes {
        Some(s) => {
            match ix.revs.get(s) {
                None => return Err(LogError::NotFound(format!("superseded revision {s}"))),
                Some(t) if *t != k => return inv("superseded revision has a different entity_type/key"),
                _ => {}
            }
            Some(s.clone())
        }
        None => ix.heads.get(&k).map(|h| h.0.clone()),
    };
    let conf = format!("{:?}", r.confidence);
    let digest = revision_content(&r.entity_type, &r.key, &r.stance, &conf, &r.rationale, &valid_from, &r.author, &causes);
    if r.supersedes.is_none() {
        if let Some((head_id, head_digest)) = ix.heads.get(&k) {
            if *head_digest == digest {
                let row = RevisionRow {
                    rev_id: head_id.clone(), entity_type: r.entity_type.clone(), key: r.key.clone(), stance: r.stance.clone(),
                    confidence: r.confidence, rationale: r.rationale.clone(), supersedes: None, valid_from, author: r.author.clone(),
                    recorded_at: now.to_string(),
                };
                return Ok((row, causes, digest));
            }
        }
        // Format 2: identical content anywhere in the log is a no-op (replays are idempotent).
        if ix.content_idem {
            if let Some(existing) = ix.content_ids.get(&digest) {
                let row = RevisionRow {
                    rev_id: existing.clone(), entity_type: r.entity_type.clone(), key: r.key.clone(), stance: r.stance.clone(),
                    confidence: r.confidence, rationale: r.rationale.clone(), supersedes: None, valid_from, author: r.author.clone(),
                    recorded_at: now.to_string(),
                };
                return Ok((row, causes, digest));
            }
        }
    }
    let rev_id = sha256_hex(&[
        b"rev1", &[0x1f], r.entity_type.as_bytes(), &[0x1f], r.key.as_bytes(), &[0x1f], r.stance.as_bytes(), &[0x1f],
        conf.as_bytes(), &[0x1f], r.rationale.as_bytes(), &[0x1f], supersedes.clone().unwrap_or_default().as_bytes(), &[0x1f],
        valid_from.as_bytes(), &[0x1f], r.author.as_bytes(), &[0x1f], causes.join(",").as_bytes(),
    ]);
    Ok((
        RevisionRow {
            rev_id, entity_type: r.entity_type.clone(), key: r.key.clone(), stance: r.stance.clone(), confidence: r.confidence,
            rationale: r.rationale.clone(), supersedes, valid_from, author: r.author.clone(), recorded_at: now.to_string(),
        },
        causes,
        digest,
    ))
}

fn validate_prediction(p: &NewPrediction, ix: &Index) -> Result<PredictionRow, LogError> {
    for c in &p.features_cite {
        if !ix.ev_has(c) {
            return Err(LogError::NotFound(format!("features_cite event {c}")));
        }
    }
    build_prediction(p)
}

/// The id a prediction will get, so a caller can attach an outcome to it in
/// the same batch. Same checks as a commit except the cited events are not
/// looked up.
pub fn prediction_id(p: &NewPrediction) -> Result<String, LogError> {
    build_prediction(p).map(|r| r.pred_id)
}

fn build_prediction(p: &NewPrediction) -> Result<PredictionRow, LogError> {
    let made_at = normalize_ts(&p.made_at).map_err(LogError::Invalid)?;
    check_text("question_id", &p.question_id, 256)?;
    check_text("target", &p.target, 512)?;
    check_text("model_id", &p.model_id, 256)?;
    check_text("model_version", &p.model_version, 256)?;
    if p.options.is_empty() || p.options.len() != p.probs.len() {
        return inv("options and probs must be non-empty and equally long");
    }
    let uniq: HashSet<&String> = p.options.iter().collect();
    if uniq.len() != p.options.len() {
        return inv("options must be unique");
    }
    if p.probs.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) || (p.probs.iter().sum::<f64>() - 1.0).abs() > 1e-6 {
        return inv("probs must each be within 0..=1 and sum to 1");
    }
    let resolves_by = match &p.resolves_by {
        Some(s) => Some(normalize_ts(s).map_err(LogError::Invalid)?),
        None => None,
    };
    let options = serde_json::to_string(&p.options).expect("json");
    let probs = serde_json::to_string(&p.probs).expect("json");
    let features_cite = serde_json::to_string(&p.features_cite).expect("json");
    let pred_id = sha256_hex(&[
        b"pred1", &[0x1f], made_at.as_bytes(), &[0x1f], p.question_id.as_bytes(), &[0x1f], p.target.as_bytes(), &[0x1f],
        options.as_bytes(), &[0x1f], probs.as_bytes(), &[0x1f], p.model_id.as_bytes(), &[0x1f], p.model_version.as_bytes(),
        &[0x1f], features_cite.as_bytes(), &[0x1f], resolves_by.clone().unwrap_or_default().as_bytes(),
    ]);
    Ok(PredictionRow {
        pred_id, made_at, question_id: p.question_id.clone(), target: p.target.clone(), options, probs,
        model_id: p.model_id.clone(), model_version: p.model_version.clone(), features_cite, resolves_by,
    })
}

fn validate_outcome(o: &NewOutcome, ix: &Index) -> Result<OutcomeRow, LogError> {
    let opts = ix.preds.get(&o.pred_id).ok_or_else(|| LogError::NotFound(format!("prediction {}", o.pred_id)))?;
    let resolved_at = normalize_ts(&o.resolved_at).map_err(LogError::Invalid)?;
    if !opts.contains(&o.actual) {
        return inv("actual must be one of the prediction's options");
    }
    if let Some(e) = &o.outcome_event_id {
        if !ix.ev_has(e) {
            return Err(LogError::NotFound(format!("outcome_event_id {e}")));
        }
    }
    if ix.outcomes.contains(&o.pred_id) {
        return Err(LogError::Conflict(format!("prediction {} already has an outcome", o.pred_id)));
    }
    Ok(OutcomeRow { pred_id: o.pred_id.clone(), resolved_at, actual: o.actual.clone(), outcome_event_id: o.outcome_event_id.clone() })
}
