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

pub mod forget;
pub mod manifest;
pub mod model;
pub mod sql;
pub use forget::{forget_hash, recover_pending, ForgetReport};
pub use sql::{register_into, run_readonly_sql};
#[cfg(test)]
mod tests;

use manifest::{Entry, SegRef, ZERO_HASH};
pub use model::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FORMAT_VERSION: i64 = 1;

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
    tables: Vec<Vec<Vec<Cell>>>,
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
    events: HashSet<String>,
    revs: HashMap<String, (String, String)>,
    /// (entity_type, key) -> (head rev_id, content digest of that revision)
    heads: HashMap<(String, String), (String, String)>,
    preds: HashMap<String, Vec<String>>,
    outcomes: HashSet<String>,
    /// `forget_hash` of every event id removed by `forget`.
    forgotten: HashSet<String>,
}

impl Index {
    fn build(tables: &[Vec<Vec<Cell>>]) -> Index {
        let mut ix = Index::default();
        for r in &tables[Table::Events.idx()] {
            ix.events.insert(r[0].s());
        }
        let mut causes: HashMap<String, Vec<String>> = HashMap::new();
        for r in &tables[Table::RevisionCauses.idx()] {
            causes.entry(r[0].s()).or_default().push(r[1].s());
        }
        for r in &tables[Table::Revisions.idx()] {
            let rv = RevisionRow::from_cells(r);
            let mut c = causes.remove(&rv.rev_id).unwrap_or_default();
            c.sort();
            let digest = revision_content(&rv.entity_type, &rv.key, &rv.stance, &format!("{:?}", rv.confidence), &rv.rationale, &rv.valid_from, &rv.author, &c);
            ix.revs.insert(rv.rev_id.clone(), (rv.entity_type.clone(), rv.key.clone()));
            ix.heads.insert((rv.entity_type, rv.key), (rv.rev_id, digest));
        }
        for r in &tables[Table::Predictions.idx()] {
            let p = PredictionRow::from_cells(r);
            let opts: Vec<String> = serde_json::from_str(&p.options).unwrap_or_default();
            ix.preds.insert(p.pred_id, opts);
        }
        for r in &tables[Table::Outcomes.idx()] {
            ix.outcomes.insert(r[0].s());
        }
        ix
    }
}

pub struct VersionedLog {
    dir: PathBuf,
    entries: Vec<Entry>,
    head: Snapshot,
    index: Index,
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

fn load_segment(dir: &Path, seg: &SegRef) -> Result<Vec<Vec<Cell>>, VerifyError> {
    let path = dir.join(&seg.file);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(VerifyError::MissingSegment { file: seg.file.clone() }),
        Err(e) => return Err(VerifyError::Io(format!("{}: {e}", seg.file))),
    };
    if sha256_hex(&[&bytes]) != seg.sha256 {
        return Err(VerifyError::ChecksumMismatch { file: seg.file.clone() });
    }
    let table = Table::from_name(&seg.table)
        .ok_or_else(|| VerifyError::SegmentUnreadable { file: seg.file.clone(), detail: format!("unknown table {}", seg.table) })?;
    let block = kore_store::KoreReader::from_bytes(&bytes)
        .map_err(|e| VerifyError::SegmentUnreadable { file: seg.file.clone(), detail: e.to_string() })?;
    let rows = block_to_rows(table, &block).map_err(|d| VerifyError::SegmentUnreadable { file: seg.file.clone(), detail: d })?;
    if rows.len() as u64 != seg.rows {
        return Err(VerifyError::RowCountMismatch { file: seg.file.clone() });
    }
    Ok(rows)
}

fn read_manifest(dir: &Path) -> Result<Vec<Entry>, VerifyError> {
    let raw = match fs::read(dir.join("MANIFEST")) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(VerifyError::ManifestMissing),
        Err(e) => return Err(VerifyError::Io(e.to_string())),
    };
    manifest::parse(&raw).map_err(VerifyError::ManifestCorrupt)
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
    Ok(())
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
    out.sort();
    out
}

/// Full integrity check of a log directory, straight from disk.
pub fn verify_dir(dir: &Path) -> Result<(), VerifyError> {
    let entries = read_manifest(dir)?;
    check_chain(&entries)?;
    for e in &entries {
        for g in &e.segments {
            load_segment(dir, g)?;
        }
    }
    if let Some(f) = stray_files(dir, &entries).into_iter().next() {
        return Err(VerifyError::ExtraSegment { file: f });
    }
    Ok(())
}

fn snapshot_from(dir: &Path, entries: &[Entry], upto: usize) -> Result<Snapshot, VerifyError> {
    let mut tables: Vec<Vec<Vec<Cell>>> = vec![Vec::new(); ALL_TABLES.len()];
    for e in &entries[..=upto] {
        for g in &e.segments {
            let rows = load_segment(dir, g)?;
            let t = Table::from_name(&g.table).expect("checked by load_segment");
            tables[t.idx()].extend(rows);
        }
    }
    let e = &entries[upto];
    Ok(Snapshot { version: e.v, recorded_at: e.recorded_at.clone(), manifest_hash: e.hash.clone(), valid_at: e.recorded_at.clone(), tables })
}

impl VersionedLog {
    /// Open `<root>/<owner>/log`, creating it (version 0 = genesis with
    /// `schema_meta`) if no MANIFEST exists. Does not delete anything when the
    /// log already exists; call `recover()` or just commit to clean leftovers.
    pub fn open_or_create(root: &Path, owner: &str) -> Result<VersionedLog, LogError> {
        Self::open_or_create_with_clock(root, owner, now_iso)
    }

    /// Like `open_or_create` but with an explicit time source, which also
    /// stamps the genesis version (tests, replays, imports).
    pub fn open_or_create_with_clock(root: &Path, owner: &str, clock: impl Fn() -> String + Send + 'static) -> Result<VersionedLog, LogError> {
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
                head: Snapshot { version: 0, recorded_at: String::new(), manifest_hash: String::new(), valid_at: String::new(), tables: vec![Vec::new(); ALL_TABLES.len()] },
                index: Index::default(),
                clock: Box::new(clock),
            };
            log.recover()?;
            let now = (log.clock)();
            let meta = MetaRow { format_version: FORMAT_VERSION, created_at: now.clone() };
            log.write_commit(vec![(Table::SchemaMeta, vec![meta.to_cells()])], now)?;
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
        let entries = read_manifest(&dir)?;
        check_chain(&entries)?;
        if entries.is_empty() {
            return Err(VerifyError::ManifestCorrupt("no entries".into()).into());
        }
        let head = snapshot_from(&dir, &entries, entries.len() - 1)?;
        let mut index = Index::build(&head.tables);
        index.forgotten = forget::forgotten_set(&entries);
        Ok(VersionedLog { dir, entries, head, index, clock: Box::new(now_iso) })
    }

    /// Replace the time source (tests / replays). `recorded_at` is still forced
    /// to be non-decreasing.
    pub fn set_clock(&mut self, f: impl Fn() -> String + Send + 'static) {
        self.clock = Box::new(f);
    }

    /// Check an event against the same rules a commit applies (without writing).
    pub fn check_new_event(e: &NewEvent) -> Result<(), LogError> {
        validate_event(e, "1970-01-01T00:00:00Z").map(|_| ())
    }

    /// True if an event with this id is stored (latest version).
    pub fn has_event(&self, id: &str) -> bool {
        self.index.events.contains(id)
    }

    /// True if this event id was removed by `forget` (ingest of it is refused).
    pub fn is_forgotten(&self, id: &str) -> bool {
        self.index.forgotten.contains(&forget_hash(id))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
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
        Ok(strays)
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

    pub fn open_latest(&self) -> Snapshot {
        self.head.clone()
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
        Ok(snapshot_from(&self.dir, &self.entries, upto)?)
    }

    /// Rows added by commits `v1+1 ..= v2`.
    pub fn diff(&self, v1: u64, v2: u64) -> Result<Diff, LogError> {
        if v1 > v2 || v2 as usize >= self.entries.len() {
            return Err(LogError::NotFound(format!("diff range {v1}..{v2}")));
        }
        let mut d = Diff { from: v1, to: v2, ..Default::default() };
        for e in &self.entries[(v1 as usize + 1)..=(v2 as usize)] {
            for g in &e.segments {
                let rows = load_segment(&self.dir, g)?;
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

    /// Validate everything, then write at most one new version atomically.
    /// Within a batch, revisions may cite events of the same batch, and
    /// outcomes may reference predictions of the same batch.
    pub fn commit_batch(&mut self, batch: Batch) -> Result<CommitInfo, LogError> {
        // Detect a concurrent writer / on-disk tampering of the head.
        match read_manifest(&self.dir) {
            Ok(on_disk) => {
                if on_disk.last().map(|e| &e.hash) != self.entries.last().map(|e| &e.hash) || on_disk.len() != self.entries.len() {
                    return Err(LogError::Conflict("MANIFEST changed on disk since this log was opened".into()));
                }
            }
            Err(e) => return Err(e.into()),
        }
        let last_at = self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
        let mut now = (self.clock)();
        if now < last_at {
            now = last_at;
        }
        let mut ix = self.index.clone();
        let mut new: Vec<(Table, Vec<Vec<Cell>>)> = Vec::new();
        let (mut ev, mut rv, mut ca, mut pr, mut oc) = (vec![], vec![], vec![], vec![], vec![]);
        let (mut event_ids, mut rev_ids, mut pred_ids) = (vec![], vec![], vec![]);
        let mut suppressed = 0usize;

        for e in &batch.events {
            let row = validate_event(e, &now)?;
            if ix.forgotten.contains(&forget_hash(&row.event_id)) {
                suppressed += 1;
                continue;
            }
            event_ids.push(row.event_id.clone());
            if ix.events.insert(row.event_id.clone()) {
                ev.push(row.to_cells());
            }
        }
        for r in &batch.revisions {
            let (row, causes, digest) = validate_revision(r, &ix, &now)?;
            rev_ids.push(row.rev_id.clone());
            if ix.revs.contains_key(&row.rev_id) {
                continue;
            }
            ix.revs.insert(row.rev_id.clone(), (row.entity_type.clone(), row.key.clone()));
            ix.heads.insert((row.entity_type.clone(), row.key.clone()), (row.rev_id.clone(), digest));
            for c in causes {
                ca.push(CauseRow { rev_id: row.rev_id.clone(), event_id: c }.to_cells());
            }
            rv.push(row.to_cells());
        }
        for p in &batch.predictions {
            let row = validate_prediction(p, &ix)?;
            pred_ids.push(row.pred_id.clone());
            if !ix.preds.contains_key(&row.pred_id) {
                ix.preds.insert(row.pred_id.clone(), p.options.clone());
                pr.push(row.to_cells());
            }
        }
        for o in &batch.outcomes {
            let row = validate_outcome(o, &ix)?;
            if ix.outcomes.insert(row.pred_id.clone()) {
                oc.push(row.to_cells());
            }
        }
        for (t, rows) in [(Table::Events, ev), (Table::Revisions, rv), (Table::RevisionCauses, ca), (Table::Predictions, pr), (Table::Outcomes, oc)] {
            if !rows.is_empty() {
                new.push((t, rows));
            }
        }
        if new.is_empty() {
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
        let new_rows: BTreeMap<String, usize> = new.iter().map(|(t, r)| (t.name().to_string(), r.len())).collect();
        // Stamp recorded_at into rows that carry it (done in validate_*), then write.
        self.write_commit(new, now.clone())?;
        self.index = ix;
        Ok(CommitInfo { version: self.latest_version(), created: true, recorded_at: now, new_rows, event_ids, rev_ids, pred_ids, suppressed })
    }

    /// Write segments, then atomically replace the MANIFEST. The commit exists
    /// only once the rename succeeds.
    fn write_commit(&mut self, new: Vec<(Table, Vec<Vec<Cell>>)>, recorded_at: String) -> Result<(), LogError> {
        self.recover()?;
        let v = self.entries.len() as u64;
        let mut segs = Vec::new();
        for (t, rows) in &new {
            let bytes = kore_store::KoreWriter::to_bytes(&rows_to_block(*t, rows));
            let rel = format!("{}/{:010}.kore", t.name(), v);
            let tdir = self.dir.join(t.name());
            fs::create_dir_all(&tdir)?;
            let tmp = self.dir.join(format!("{rel}.tmp"));
            write_synced(&tmp, &bytes)?;
            fs::rename(&tmp, self.dir.join(&rel))?;
            sync_dir(&tdir);
            segs.push(SegRef { table: t.name().to_string(), file: rel, sha256: sha256_hex(&[&bytes]), rows: rows.len() as u64 });
        }
        let prev = self.entries.last().map(|e| e.hash.clone()).unwrap_or_else(|| ZERO_HASH.to_string());
        let entry = Entry::new(v, recorded_at, segs, &prev);
        let mut all = self.entries.clone();
        all.push(entry);
        let tmp = self.dir.join("MANIFEST.tmp");
        write_synced(&tmp, manifest::render(&all).as_bytes())?;
        fs::rename(&tmp, self.dir.join("MANIFEST"))?;
        sync_dir(&self.dir);
        // Commit point passed: update memory.
        for (t, rows) in new {
            self.head.tables[t.idx()].extend(rows);
        }
        self.entries = all;
        let e = self.entries.last().expect("just pushed");
        self.head.version = e.v;
        self.head.recorded_at = e.recorded_at.clone();
        self.head.manifest_hash = e.hash.clone();
        self.head.valid_at = e.recorded_at.clone();
        if v == 0 {
            self.index = Index::build(&self.head.tables);
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
        let bytes = kore_store::KoreWriter::to_bytes(&rows_to_block(Table::Events, &ev));
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

fn validate_event(e: &NewEvent, now: &str) -> Result<EventRow, LogError> {
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
    Ok(EventRow {
        event_id: event_id(&e.source, &occurred_at, &e.text),
        occurred_at,
        recorded_at: now.to_string(),
        source: e.source.clone(),
        kind: e.kind.clone(),
        text: e.text.clone(),
        tags: e.tags.join(","),
        redacted: e.redacted,
    })
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
        if !ix.events.contains(c) {
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
        if !ix.events.contains(c) {
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
        if !ix.events.contains(e) {
            return Err(LogError::NotFound(format!("outcome_event_id {e}")));
        }
    }
    if ix.outcomes.contains(&o.pred_id) {
        return Err(LogError::Conflict(format!("prediction {} already has an outcome", o.pred_id)));
    }
    Ok(OutcomeRow { pred_id: o.pred_id.clone(), resolved_at, actual: o.actual.clone(), outcome_event_id: o.outcome_event_id.clone() })
}
