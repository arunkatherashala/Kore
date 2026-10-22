//! `forget`: the one documented exception to append-only.
//!
//! What it does, in order:
//! 1. Finds every segment (of any version) that holds a row for a forgotten
//!    event: `events` rows are removed, `revision_causes` rows that cite the
//!    event are dropped (the revisions stay and show as `unsupported` if they
//!    lost all citations), `predictions.features_cite` loses the id and
//!    `outcomes.outcome_event_id` becomes null.
//! 2. Rewrites those segments (a segment that ends up empty is deleted), so
//!    old versions no longer contain the text either: time travel to any
//!    earlier version does not bring it back.
//! 3. Re-seals the hash chain from the first rewritten version onward (their
//!    manifest hashes therefore change; this is the visible price of forgetting)
//!    and appends one new commit whose manifest entry carries a `forget`
//!    record: the sha256 of each forgotten event id, the head hash from before
//!    the re-seal, and the first re-sealed version. Content is never recorded.
//! 4. Remembers the id hashes so that re-ingesting the same source text is
//!    refused (counted as `suppressed`) instead of silently bringing it back.
//!
//! Crash safety: new segments and the new MANIFEST are written to
//! `forget.staging/` and fsynced; then a `FORGET_PENDING` marker (listing the
//! renames and deletions) is created atomically. Applying is idempotent. On the
//! next open, a log with a marker is rolled forward; a staging directory without
//! a marker is discarded, leaving the log exactly as it was.
//!
//! Limits: copies of the text outside this log (backups, filesystem snapshots,
//! earlier exports, disk blocks not overwritten) are not touched.

use super::manifest::{Entry, ForgetRecord, SegRef, ZERO_HASH};
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const STAGING: &str = "forget.staging";
const PENDING: &str = "FORGET_PENDING";

/// Digest stored in place of a forgotten event id.
pub fn forget_hash(event_id: &str) -> String {
    sha256_hex(&[b"forget1|", event_id.as_bytes()])
}

pub(super) fn forgotten_set(entries: &[Entry]) -> HashSet<String> {
    entries.iter().filter_map(|e| e.forget.as_ref()).flat_map(|f| f.removed.iter().cloned()).collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForgetReport {
    /// False when nothing was removed (all ids were already forgotten).
    pub created: bool,
    pub version: u64,
    pub forgotten: usize,
    pub already_forgotten: usize,
    pub causes_dropped: usize,
    /// Revisions that lost their last citation (they now count as unsupported).
    pub revisions_orphaned: Vec<String>,
    pub segments_rewritten: usize,
    pub segments_removed: usize,
    pub predictions_edited: usize,
    pub outcomes_edited: usize,
}

#[derive(Serialize, Deserialize)]
struct Pending {
    rename: Vec<String>,
    delete: Vec<String>,
}

/// Simulated crash points (tests).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Crash {
    BeforeMarker,
    AfterMarker,
    /// Stop after this many apply operations.
    DuringApply(usize),
}

fn crash_err() -> LogError {
    LogError::Io("simulated crash".into())
}

fn apply(dir: &Path, p: &Pending, stop_after: Option<usize>) -> Result<(), LogError> {
    let staging = dir.join(STAGING);
    let mut ops = 0usize;
    let mut stop = |ops: &mut usize| -> bool {
        if stop_after == Some(*ops) {
            return true;
        }
        *ops += 1;
        false
    };
    for rel in &p.rename {
        if stop(&mut ops) {
            return Err(crash_err());
        }
        let src = staging.join(rel);
        if src.exists() {
            let dst = dir.join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&src, &dst)?;
        }
    }
    for rel in &p.delete {
        if stop(&mut ops) {
            return Err(crash_err());
        }
        match fs::remove_file(dir.join(rel)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    // The manifest goes last: it is what makes the new state the committed one.
    if stop(&mut ops) {
        return Err(crash_err());
    }
    let m = staging.join("MANIFEST");
    if m.exists() {
        fs::rename(&m, dir.join("MANIFEST"))?;
    }
    sync_dir(dir);
    let _ = fs::remove_file(dir.join(PENDING));
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

/// Roll an interrupted forget forward (marker present) or discard its
/// staging area (no marker). Returns true if a forget was completed.
pub fn recover_pending(dir: &Path) -> Result<bool, LogError> {
    let marker = dir.join(PENDING);
    if marker.exists() {
        let raw = fs::read(&marker)?;
        let p: Pending = serde_json::from_slice(&raw).map_err(|e| LogError::Io(format!("FORGET_PENDING unreadable: {e}")))?;
        apply(dir, &p, None)?;
        return Ok(true);
    }
    let staging = dir.join(STAGING);
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    Ok(false)
}

impl VersionedLog {
    /// Ids of all stored events whose source starts with `prefix`.
    pub fn event_ids_by_source_prefix(&self, prefix: &str) -> Vec<String> {
        let mut v: Vec<String> = self.head.raw(Table::Events).iter().filter(|r| r[3].s().starts_with(prefix)).map(|r| r[0].s()).collect();
        v.sort();
        v
    }

    /// Remove events (and what depends on them, see the module docs).
    /// Unknown ids are an error and nothing is changed; ids that were already
    /// forgotten are counted and ignored.
    pub fn forget_events(&mut self, ids: &[String]) -> Result<ForgetReport, LogError> {
        self.forget_inner(ids, None)
    }

    pub(crate) fn forget_inner(&mut self, ids: &[String], crash: Option<Crash>) -> Result<ForgetReport, LogError> {
        if ids.is_empty() {
            return inv("no event ids to forget");
        }
        match read_manifest(&self.dir) {
            Ok(on_disk) => {
                if on_disk.last().map(|e| &e.hash) != self.entries.last().map(|e| &e.hash) || on_disk.len() != self.entries.len() {
                    return Err(LogError::Conflict("MANIFEST changed on disk since this log was opened".into()));
                }
            }
            Err(e) => return Err(e.into()),
        }
        let mut set: BTreeSet<String> = BTreeSet::new();
        let mut already = 0usize;
        for id in ids {
            if self.index.events.contains(id) {
                set.insert(id.clone());
            } else if self.index.forgotten.contains(&forget_hash(id)) {
                already += 1;
            } else {
                return Err(LogError::NotFound(format!("event {id}")));
            }
        }
        let mut report = ForgetReport { version: self.latest_version(), already_forgotten: already, ..Default::default() };
        if set.is_empty() {
            return Ok(report);
        }

        // Pass 1: compute new row sets for every affected segment.
        let mut changed: Vec<(usize, usize, Vec<Vec<Cell>>)> = Vec::new();
        let mut cause_total: HashMap<String, usize> = HashMap::new();
        let mut cause_dropped: HashMap<String, usize> = HashMap::new();
        let mut events_removed = 0usize;
        for (ei, e) in self.entries.iter().enumerate() {
            for (si, g) in e.segments.iter().enumerate() {
                let rows = load_segment(&self.dir, g)?;
                let table = Table::from_name(&g.table).expect("checked by load_segment");
                let mut touched = false;
                let mut out: Vec<Vec<Cell>> = Vec::with_capacity(rows.len());
                for mut r in rows {
                    match table {
                        Table::Events => {
                            if set.contains(&r[0].s()) {
                                events_removed += 1;
                                touched = true;
                                continue;
                            }
                        }
                        Table::RevisionCauses => {
                            let rev = r[0].s();
                            *cause_total.entry(rev.clone()).or_default() += 1;
                            if set.contains(&r[1].s()) {
                                *cause_dropped.entry(rev).or_default() += 1;
                                report.causes_dropped += 1;
                                touched = true;
                                continue;
                            }
                        }
                        Table::Predictions => {
                            let cited: Vec<String> = serde_json::from_str(&r[8].s()).unwrap_or_default();
                            if cited.iter().any(|c| set.contains(c)) {
                                let keep: Vec<String> = cited.into_iter().filter(|c| !set.contains(c)).collect();
                                r[8] = Cell::S(serde_json::to_string(&keep).expect("json"));
                                report.predictions_edited += 1;
                                touched = true;
                            }
                        }
                        Table::Outcomes => {
                            if matches!(&r[3], Cell::S(x) if set.contains(x)) {
                                r[3] = Cell::Null;
                                report.outcomes_edited += 1;
                                touched = true;
                            }
                        }
                        _ => {}
                    }
                    out.push(r);
                }
                if touched {
                    changed.push((ei, si, out));
                }
            }
        }
        if events_removed != set.len() {
            return Err(LogError::Io(format!("internal: expected to remove {} event rows, found {events_removed}", set.len())));
        }
        report.revisions_orphaned = cause_dropped.iter().filter(|(rev, n)| cause_total.get(*rev) == Some(*n)).map(|(r, _)| r.clone()).collect();
        report.revisions_orphaned.sort();
        report.forgotten = set.len();
        report.created = true;

        // Pass 2: new segment bytes, re-sealed entries, forget entry.
        let mut entries = self.entries.clone();
        let first = changed.iter().map(|c| c.0).min().expect("events were found in some segment");
        let mut staged: Vec<(String, Vec<u8>)> = Vec::new();
        let mut deletes: Vec<String> = Vec::new();
        for (ei, si, rows) in &changed {
            let g: SegRef = entries[*ei].segments[*si].clone();
            let table = Table::from_name(&g.table).expect("known");
            if rows.is_empty() {
                deletes.push(g.file.clone());
            } else {
                let bytes = kore_store::KoreWriter::to_bytes(&rows_to_block(table, rows));
                let seg = &mut entries[*ei].segments[*si];
                seg.sha256 = sha256_hex(&[&bytes]);
                seg.rows = rows.len() as u64;
                staged.push((g.file.clone(), bytes));
            }
        }
        report.segments_removed = deletes.len();
        report.segments_rewritten = staged.len();
        for e in entries.iter_mut() {
            e.segments.retain(|g| !deletes.contains(&g.file));
        }
        let mut prev = if first == 0 { ZERO_HASH.to_string() } else { entries[first - 1].hash.clone() };
        for e in entries.iter_mut().skip(first) {
            e.reseal(&prev);
            prev = e.hash.clone();
        }
        let prior_head = self.entries.last().map(|e| e.hash.clone()).unwrap_or_default();
        let last_at = self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
        let mut now = (self.clock)();
        if now < last_at {
            now = last_at;
        }
        let v = entries.len() as u64;
        let mut fe = Entry::new(v, now, vec![], &prev);
        fe.forget = Some(ForgetRecord { removed: set.iter().map(|i| forget_hash(i)).collect(), prior_head, resealed_from: first as u64 });
        fe.reseal(&prev);
        entries.push(fe);

        // Stage, mark, apply.
        let staging = self.dir.join(STAGING);
        let _ = fs::remove_dir_all(&staging);
        for (rel, bytes) in &staged {
            let p = staging.join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent)?;
            }
            write_synced(&p, bytes)?;
        }
        fs::create_dir_all(&staging)?;
        write_synced(&staging.join("MANIFEST"), manifest::render(&entries).as_bytes())?;
        if matches!(crash, Some(Crash::BeforeMarker)) {
            return Err(crash_err());
        }
        let pending = Pending { rename: staged.iter().map(|s| s.0.clone()).collect(), delete: deletes };
        let tmp = self.dir.join(format!("{PENDING}.tmp"));
        write_synced(&tmp, &serde_json::to_vec(&pending).expect("json"))?;
        fs::rename(&tmp, self.dir.join(PENDING))?;
        sync_dir(&self.dir);
        if matches!(crash, Some(Crash::AfterMarker)) {
            return Err(crash_err());
        }
        let stop = if let Some(Crash::DuringApply(n)) = crash { Some(n) } else { None };
        apply(&self.dir, &pending, stop)?;

        // Committed: refresh memory from what is now on disk.
        self.head = snapshot_from(&self.dir, &entries, entries.len() - 1)?;
        let mut index = Index::build(&self.head.tables);
        index.forgotten = forgotten_set(&entries);
        self.index = index;
        self.entries = entries;
        report.version = self.latest_version();
        Ok(report)
    }
}
