//! Track D1b: state checkpoints of a folded being (first world `BeingState`, World T `StateT`,
//! World O `StateO`), so that a restart folds only the tail of the log.
//!
//! What a checkpoint is: the state a reader folds from all events of versions `0..=v`, encoded with
//! `binser` (exact, floats bit for bit), stored by the log (`versioned/checkpoint.rs`: file under
//! `<log>/checkpoints/`, atomic write, content hash binding payload, `v` and the MANIFEST hash at `v`).
//!
//! **Anchor in the hash chain.** Right after the file is written, one note is appended to the log
//! (source `being:checkpoint`, tags `checkpoint`, `ckpt_kind:<kind>`, `ckpt_v:<v>`, `ckpt_step:<n>`,
//! `ckpt_sha:<content hash>`, `ckpt_file:<sha256 of the whole file>`), so both hashes are covered by
//! the MANIFEST chain and a change of any byte of the file makes it unusable. No reader
//! folds this source (each reader filters on its own source), so decisions do not change.
//!
//! **Restore** (`restore`): the newest file first. It is used only if (1) the log accepts it
//! (`read_checkpoint`: snapshot format version known, content hash verifies, `v` <= latest version,
//! the log's chain hash at `v` equals the recorded one), (2) the payload decodes to the reader's type
//! and its step count equals the recorded one, and (3) while the tail (versions after `v`) is folded
//! onto it, the anchor note with exactly that kind, version, content hash and file hash is found in the tail.
//! Otherwise the next older file is tried, and in the end the whole log is folded (as before D1). A
//! file forged with a self-consistent content hash fails (3): forging an anchor means writing into the
//! hash-chained log, which `verify_against(expected head)` detects like any other change of the log.
//! A `forget` of an event at or before `v` re-seals entry `v`, so (1) fails; a forget of the anchor
//! itself makes (3) fail. Either way: full fold, never a wrong state.
//!
//! **Write** (`write_after_run`): at the end of a run, when the steps since the restored checkpoint
//! reach `Policy::checkpoint_every`. The state written is not the live engine state but, by
//! construction, the restored start state of the run with every event of this run folded onto it
//! (exactly what a full fold of the log gives). Before writing, the payload is decoded again and must
//! give the same state (`Debug` text equal); otherwise nothing is written. A failed write never fails
//! the run (the checkpoint is only a shortcut). The newest `KEEP` files are kept.
//!
//! **Cadence / controller.** Off unless the owner writes `checkpoint_every = N` (N in
//! `MIN_EVERY..=HARD_EVERY` steps; 0 = off) in `controller.txt` (fail closed: missing or invalid
//! file = off, the repo convention for new capabilities). With it off the restart is exactly the old
//! full fold and existing checkpoint files are not even read.
//!
//! The full fold stays available and is what `audit` compares against: `audit` folds the whole log up
//! to each checkpoint's version and compares it with the decoded checkpoint (the `self_verify` tool
//! reports it).

use super::binser;
use super::state::tag;
use super::world::iso_at;
use crate::versioned::{CheckpointFile, EventRow, LogError, NewEvent, VersionedLog};
use serde::{de::DeserializeOwned, Serialize};

/// controller.txt key (Track D1b): steps between checkpoints; 0 or missing = off.
pub const KEY: &str = "checkpoint_every";
pub const MIN_EVERY: u64 = 100;
pub const HARD_EVERY: u64 = 1_000_000;
/// Checkpoint files kept per kind (the newest; an older one is the fallback if the newest is bad).
pub const KEEP: usize = 2;
/// Source of the anchor notes.
pub const ANCHOR_SOURCE: &str = "being:checkpoint";
pub const ANCHOR_TAG: &str = "checkpoint";

pub fn is_key(k: &str) -> bool {
    k == KEY
}

/// Check the value (called by `Controller::parse`; an error makes the whole file invalid).
pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    let x: u64 = v.parse().map_err(|_| format!("'{k}' needs a non-negative integer"))?;
    if x != 0 && !(MIN_EVERY..=HARD_EVERY).contains(&x) {
        return Err(format!("'{k}' must be 0 (off) or {MIN_EVERY}..={HARD_EVERY} steps"));
    }
    Ok(())
}

/// The interval from the text of controller.txt (None: missing). Missing, invalid or absent key: 0 (off).
pub fn every_from_text(text: Option<&str>) -> u64 {
    let Some(text) = text else { return 0 };
    if super::controller::Controller::parse(text).is_err() {
        return 0;
    }
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == KEY {
                return v.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

/// A state that is folded from the log, event by event, and can be checkpointed.
pub trait Foldable: Serialize + DeserializeOwned + Default + Clone + std::fmt::Debug {
    /// Checkpoint kind (file name prefix and anchor tag): `[a-z0-9_]`, unique per reader.
    const KIND: &'static str;
    fn fold(&mut self, e: &EventRow);
    /// Completed steps (cadence of the checkpoints).
    fn steps(&self) -> u64;
    /// World clock in seconds (time stamp of the anchor note).
    fn clock_secs(&self) -> i64;
    /// Field-by-field equality (default: the `Debug` text; types with hash maps inside use `PartialEq`).
    fn same(&self, other: &Self) -> bool {
        format!("{self:?}") == format!("{other:?}")
    }
}

/// What a restore did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Restored {
    /// Version of the checkpoint used (None: the whole log was folded).
    pub from_version: Option<u64>,
    /// Step count recorded in that checkpoint (0 if none).
    pub from_step: u64,
    /// Checkpoint files looked at and not used, with the reason.
    pub rejected: Vec<String>,
}

/// What a run did with checkpoints (for the tool output).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CkptReport {
    pub every: u64,
    pub restored: Restored,
    /// Version of the checkpoint written at the end of the run, if any.
    pub written: Option<u64>,
    /// Why a due checkpoint was not written.
    pub write_error: Option<String>,
}

/// The fold of the whole log (the reference, and the fallback).
pub fn full_fold<S: Foldable>(log: &VersionedLog) -> Result<S, LogError> {
    let mut st = S::default();
    log.for_each_event(|e| {
        st.fold(&e);
        true
    })?;
    Ok(st)
}

/// The anchor of exactly this file: kind, version, content hash and the sha256 of the whole file.
fn is_anchor(e: &EventRow, c: &CheckpointFile) -> bool {
    e.source == ANCHOR_SOURCE
        && tag(&e.tags, "ckpt_kind") == Some(c.kind.as_str())
        && tag(&e.tags, "ckpt_v") == Some(c.version.to_string().as_str())
        && tag(&e.tags, "ckpt_sha") == Some(c.content_hash.as_str())
        && tag(&e.tags, "ckpt_file") == Some(c.file_sha256.as_str())
}

/// Decode a file's payload and check its step count.
fn decode<S: Foldable>(c: &CheckpointFile) -> Result<S, String> {
    let st: S = binser::from_bytes(&c.payload).map_err(|e| format!("{}: payload does not decode: {e}", c.file))?;
    if st.steps() != c.step {
        return Err(format!("{}: payload has {} steps, the file says {}", c.file, st.steps(), c.step));
    }
    Ok(st)
}

/// The state of the whole log: the newest usable checkpoint plus the tail, or the full fold.
pub fn restore<S: Foldable>(log: &VersionedLog) -> Result<(S, Restored), LogError> {
    let mut rejected = vec![];
    for (_, file) in log.checkpoint_files(S::KIND) {
        let c = match log.read_checkpoint(S::KIND, &file) {
            Ok(c) => c,
            Err(e) => {
                rejected.push(e);
                continue;
            }
        };
        let mut st: S = match decode(&c) {
            Ok(s) => s,
            Err(e) => {
                rejected.push(e);
                continue;
            }
        };
        let mut anchored = false;
        log.for_each_event_after(c.version, |e| {
            if !anchored && is_anchor(&e, &c) {
                anchored = true;
            }
            st.fold(&e);
            true
        })?;
        if !anchored {
            rejected.push(format!("{}: no anchor note for its content and file hash in the log after version {}", c.file, c.version));
            continue;
        }
        return Ok((st, Restored { from_version: Some(c.version), from_step: c.step, rejected }));
    }
    Ok((full_fold(log)?, Restored { from_version: None, from_step: 0, rejected }))
}

/// The start state of a run: `restore` when checkpoints are on, the plain full fold when off.
pub fn start<S: Foldable>(log: &VersionedLog, every: u64) -> Result<(S, Restored), LogError> {
    if every == 0 {
        return Ok((full_fold(log)?, Restored::default()));
    }
    restore(log)
}

/// The anchor note of a written checkpoint.
pub fn anchor_event(c: &CheckpointFile, clock: i64) -> NewEvent {
    NewEvent {
        source: ANCHOR_SOURCE.into(),
        occurred_at: iso_at(clock),
        kind: "note".into(),
        text: format!("state checkpoint {} at log version {}, step {}, content sha256 {} ({})", c.kind, c.version, c.step, c.content_hash, c.file),
        tags: vec![ANCHOR_TAG.into(), format!("ckpt_kind:{}", c.kind), format!("ckpt_v:{}", c.version), format!("ckpt_step:{}", c.step), format!("ckpt_sha:{}", c.content_hash), format!("ckpt_file:{}", c.file_sha256)],
        redacted: false,
    }
}

/// Write `st` (the fold of the whole log at its latest version) as a checkpoint and anchor it.
pub fn write_now<S: Foldable>(log: &mut VersionedLog, st: &S) -> Result<CheckpointFile, String> {
    let payload = binser::to_bytes(st).map_err(|e| format!("encode: {e}"))?;
    let back: S = binser::from_bytes(&payload).map_err(|e| format!("decode check: {e}"))?;
    if !back.same(st) {
        return Err("the decoded payload differs from the state; not written".into());
    }
    let c = log.write_checkpoint(S::KIND, st.steps(), payload).map_err(|e| e.to_string())?;
    log.append_events(&[anchor_event(&c, st.clock_secs())]).map_err(|e| e.to_string())?;
    log.prune_checkpoints(S::KIND, KEEP);
    Ok(c)
}

/// End of a run: if due, fold this run's events onto the run's start state `base` (the fold of the
/// log at `base_v`) and write that as the checkpoint. Never fails the run.
pub fn write_after_run<S: Foldable>(log: &mut VersionedLog, every: u64, base: Option<(S, u64)>, restored: Restored) -> CkptReport {
    let mut rep = CkptReport { every, restored, ..Default::default() };
    let Some((mut st, base_v)) = base else { return rep };
    if every == 0 {
        return rep;
    }
    if let Err(e) = log.for_each_event_after(base_v, |e| {
        st.fold(&e);
        true
    }) {
        rep.write_error = Some(e.to_string());
        return rep;
    }
    if st.steps() < rep.restored.from_step.saturating_add(every) {
        return rep;
    }
    match write_now(log, &st) {
        Ok(c) => rep.written = Some(c.version),
        Err(e) => rep.write_error = Some(e),
    }
    rep
}

/// One line of an audit of the checkpoints of `kind`.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditLine {
    pub file: String,
    pub version: Option<u64>,
    /// None: the file is not usable (reason in `problem`); Some(b): its state equals the full fold at its version.
    pub equal_to_full_fold: Option<bool>,
    pub problem: Option<String>,
}

/// Full verification of the checkpoints: each file is read and checked as `restore` does, and its
/// state is compared with the fold of the whole log up to its version (the expensive reference).
pub fn audit<S: Foldable>(log: &VersionedLog) -> Result<Vec<AuditLine>, LogError> {
    let mut out = vec![];
    for (_, file) in log.checkpoint_files(S::KIND) {
        let c = match log.read_checkpoint(S::KIND, &file) {
            Ok(c) => c,
            Err(e) => {
                out.push(AuditLine { file, version: None, equal_to_full_fold: None, problem: Some(e) });
                continue;
            }
        };
        let st: S = match decode(&c) {
            Ok(s) => s,
            Err(e) => {
                out.push(AuditLine { file, version: Some(c.version), equal_to_full_fold: None, problem: Some(e) });
                continue;
            }
        };
        let mut anchored = false;
        log.for_each_event_after(c.version, |e| {
            anchored |= is_anchor(&e, &c);
            !anchored
        })?;
        // the reference: fold of every event of versions 0..=v
        let mut full = S::default();
        log.for_each_event_upto(c.version, |e| {
            full.fold(&e);
            true
        })?;
        let eq = full.same(&st);
        let problem = if !anchored { Some("no anchor note in the log".to_string()) } else if !eq { Some("state differs from the full fold".to_string()) } else { None };
        out.push(AuditLine { file, version: Some(c.version), equal_to_full_fold: Some(eq), problem });
    }
    Ok(out)
}

impl Foldable for super::state::BeingState {
    const KIND: &'static str = "being";
    fn fold(&mut self, e: &EventRow) {
        self.fold_event(e);
    }
    fn steps(&self) -> u64 {
        self.step
    }
    fn clock_secs(&self) -> i64 {
        self.clock
    }
}

impl Foldable for super::world_t::engine::StateT {
    const KIND: &'static str = "world_t";
    fn fold(&mut self, e: &EventRow) {
        self.fold_event(e);
    }
    fn steps(&self) -> u64 {
        self.step
    }
    fn clock_secs(&self) -> i64 {
        self.clock
    }
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

impl Foldable for super::world_o::engine::StateO {
    const KIND: &'static str = "world_o";
    fn fold(&mut self, e: &EventRow) {
        self.fold_event(e);
    }
    fn steps(&self) -> u64 {
        self.s2.step
    }
    fn clock_secs(&self) -> i64 {
        self.s2.clock
    }
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

/// The tool output of a run's checkpoint report.
pub fn report_json(r: &CkptReport) -> serde_json::Value {
    serde_json::json!({
        "checkpoint_every": r.every,
        "restored_from_version": r.restored.from_version,
        "rejected": r.restored.rejected,
        "written_at_version": r.written,
        "write_error": r.write_error,
    })
}

/// Audit of every checkpoint kind of this log, as one line of text (None: no checkpoint files).
/// Used by the `self_verify` tool: "full verify" covers the checkpoints too.
pub fn audit_summary(log: &VersionedLog) -> Option<String> {
    let mut lines = vec![];
    let r = (|| -> Result<(), LogError> {
        lines.extend(audit::<super::state::BeingState>(log)?);
        lines.extend(audit::<super::world_t::engine::StateT>(log)?);
        lines.extend(audit::<super::world_o::engine::StateO>(log)?);
        Ok(())
    })();
    if let Err(e) = r {
        return Some(format!("checkpoints: audit failed: {e}"));
    }
    if lines.is_empty() {
        return None;
    }
    let bad: Vec<String> = lines.iter().filter_map(|l| l.problem.as_ref().map(|p| format!("{}: {p}", l.file))).collect();
    Some(if bad.is_empty() {
        format!("checkpoints: {} file(s), each anchored in the log and equal to the full fold at its version", lines.len())
    } else {
        format!("checkpoints: {} file(s), {} NOT usable (a restart ignores them and folds the whole log): {}", lines.len(), bad.len(), bad.join("; "))
    })
}
