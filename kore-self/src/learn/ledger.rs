//! Early prediction ledger: a frequency baseline written before any outcome.
//!
//! For each habit that meets the sufficiency rules (n >= min_n and the
//! Wilson bound), one row: "action X in context C within the next 7 days",
//! options [yes, no], probability yes = Wilson-smoothed frequency
//! (k + z^2/2) / (n + z^2) of the action at past occasions of the context.
//! Only events strictly before the window start are used, `made_at` is the
//! newest such event's time (the cutoff), and the row is written once per
//! (habit, window start): a second call, even with more events, writes
//! nothing for the same window. Nothing is scored here; no accuracy is
//! claimed. An outcome can be attached later with `record_outcome`.

use super::dates;
use super::derive::MAX_CITED;
use super::params::{wilson_smoothed, Params};
use super::tagrules::TagRules;
use super::{habits, visible_before};
use crate::versioned::{Batch, LogError, NewPrediction, PredictionRow, VersionedLog};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const MODEL_ID: &str = "rule:habit-frequency";
pub const MODEL_VERSION: &str = "wilson-smoothed-v1";

pub struct SnapshotResult {
    pub window_start: String,
    pub resolves_by: String,
    pub cutoff: String,
    pub written: Vec<Value>,
    pub already_present: usize,
    pub insufficient: Vec<Value>,
    pub ignored_future_events: usize,
    pub version: u64,
}

pub fn snapshot(log: &mut VersionedLog, p: &Params, window_start: Option<&str>, window_days: i64, dry_run: bool) -> Result<SnapshotResult, LogError> {
    snapshot_with(log, p, window_start, window_days, dry_run, None)
}

/// `snapshot` reading free text through keyword rules (as `self_learn` does).
pub fn snapshot_with(log: &mut VersionedLog, p: &Params, window_start: Option<&str>, window_days: i64, dry_run: bool, rules: Option<&TagRules>) -> Result<SnapshotResult, LogError> {
    let events = super::learnable(&log.open_latest().rows());
    let newest = events.iter().map(|e| e.day).max().ok_or_else(|| LogError::Invalid("the log has no events to learn from".into()))?;
    let ws = match window_start {
        Some(s) => dates::day_of(&crate::versioned::normalize_ts(s).map_err(LogError::Invalid)?).ok_or_else(|| LogError::Invalid("bad window_start".into()))?,
        None => newest + 1,
    };
    let vis = visible_before(&events, ws);
    if vis.is_empty() {
        return Err(LogError::Invalid("no events before the window start".into()));
    }
    let ignored = events.len() - vis.len();
    let cutoff = vis.iter().map(|e| e.ts.clone()).max().unwrap_or_default();
    let date = dates::date_of(ws);
    let resolves_by = dates::midnight_of(ws + window_days);
    let have: HashSet<String> = log.open_latest().rows::<PredictionRow>().into_iter().map(|r| r.question_id).collect();
    let (hs, insuff) = habits::detect_with(&vis, p, rules);
    let mut batch = Batch::default();
    let (mut written, mut present) = (Vec::new(), 0usize);
    for h in hs {
        let qid = format!("habit-window:ctx:{}|act:{}@{date}", h.context, h.action);
        if have.contains(&qid) {
            present += 1;
            continue;
        }
        let prob = wilson_smoothed(h.k, h.n);
        let mut cite = h.cited.clone();
        if cite.len() > MAX_CITED {
            cite.drain(..cite.len() - MAX_CITED);
        }
        written.push(json!({"question_id": qid, "context": h.context, "action": h.action, "k": h.k, "n": h.n, "probability_yes": (prob * 1e4).round() / 1e4}));
        batch.predictions.push(NewPrediction {
            made_at: cutoff.clone(),
            question_id: qid,
            target: format!("action '{}' in context '{}' within the {window_days} days starting {date}", h.action, h.context),
            options: vec!["yes".into(), "no".into()],
            probs: vec![prob, 1.0 - prob],
            model_id: MODEL_ID.into(),
            model_version: MODEL_VERSION.into(),
            features_cite: cite,
            resolves_by: Some(resolves_by.clone()),
        });
    }
    let mut version = log.latest_version();
    if !dry_run && !batch.predictions.is_empty() {
        let info = log.commit_batch(batch)?;
        for (w, id) in written.iter_mut().zip(info.pred_ids.iter()) {
            w["pred_id"] = json!(id);
        }
        version = info.version;
    }
    Ok(SnapshotResult {
        window_start: date,
        resolves_by,
        cutoff,
        written,
        already_present: present,
        insufficient: insuff.iter().map(|i| json!({"context": i.context, "n": i.n, "min_n": i.min_n})).collect(),
        ignored_future_events: ignored,
        version,
    })
}
