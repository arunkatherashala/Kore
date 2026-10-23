//! Turn detector output into revisions and write them through the versioned
//! library as `author = rule:<id>`, every one citing the events it came from.
//!
//! Idempotence: a derived row is identified by a content signature (type,
//! key, stance, confidence, rationale, valid_from, author, sorted causes). A
//! row whose signature already exists in the log is not written again, so
//! running learn twice on the same log adds nothing, and running it after new
//! events only adds rows whose content changed (new evidence, new stance).
//! Nothing here reads the clock; `valid_from` is the newest cited event's time.

use super::params::Params;
use super::{changes, habits, stances, Ev};
use crate::versioned::{sha256_hex, Batch, CauseRow, LogError, NewRevision, RevisionRow, VersionedLog};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const MAX_CITED: usize = 2000;
pub const AUTHOR_HABITS: &str = "rule:habits-v1";
pub const AUTHOR_CHANGES: &str = "rule:changes-v1";
pub const AUTHOR_STANCES: &str = "rule:stances-v1";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kinds {
    pub habits: bool,
    pub changes: bool,
    pub stances: bool,
}
impl Kinds {
    pub const ALL: Kinds = Kinds { habits: true, changes: true, stances: true };
    pub fn parse(names: &[String]) -> Result<Kinds, String> {
        if names.is_empty() {
            return Ok(Kinds::ALL);
        }
        let mut k = Kinds { habits: false, changes: false, stances: false };
        for n in names {
            match n.as_str() {
                "habits" => k.habits = true,
                "changes" => k.changes = true,
                "stances" => k.stances = true,
                other => return Err(format!("unknown kind '{other}' (use habits, changes, stances)")),
            }
        }
        Ok(k)
    }
}

#[derive(Debug, Clone)]
pub struct Derived {
    /// habits | changes | stances
    pub kind: &'static str,
    pub entity_type: &'static str,
    pub key: String,
    pub stance: String,
    pub confidence: f64,
    pub rationale: String,
    pub valid_from: String,
    pub author: &'static str,
    pub caused_by: Vec<String>,
    /// Numbers and evidence for the tool reply.
    pub detail: Value,
}

impl Derived {
    pub fn signature(&self) -> String {
        signature(self.entity_type, &self.key, &self.stance, self.confidence, &self.rationale, &self.valid_from, self.author, &self.caused_by)
    }
    fn to_new(&self) -> NewRevision {
        NewRevision {
            entity_type: self.entity_type.into(),
            key: self.key.clone(),
            stance: self.stance.clone(),
            confidence: self.confidence,
            rationale: self.rationale.clone(),
            supersedes: None,
            valid_from: self.valid_from.clone(),
            author: self.author.into(),
            caused_by: self.caused_by.clone(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn signature(t: &str, key: &str, stance: &str, conf: f64, rationale: &str, valid_from: &str, author: &str, causes: &[String]) -> String {
    let mut c: Vec<&String> = causes.iter().collect();
    c.sort();
    c.dedup();
    let joined = c.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
    let conf = format!("{conf:?}");
    sha256_hex(&[
        t.as_bytes(), &[0x1f], key.as_bytes(), &[0x1f], stance.as_bytes(), &[0x1f], conf.as_bytes(), &[0x1f], rationale.as_bytes(),
        &[0x1f], valid_from.as_bytes(), &[0x1f], author.as_bytes(), &[0x1f], joined.as_bytes(),
    ])
}

fn cap(mut ids: Vec<String>) -> (Vec<String>, usize) {
    let total = ids.len();
    if total > MAX_CITED {
        ids.drain(..total - MAX_CITED);
    }
    (ids, total)
}

fn evidence_sample(ids: &[String]) -> Vec<String> {
    ids.iter().take(10).cloned().collect()
}

#[derive(Debug, Clone, Default)]
pub struct Derivation {
    pub items: Vec<Derived>,
    pub insufficient_habits: Vec<habits::Insufficient>,
    pub insufficient_topics: Vec<changes::InsufficientTopic>,
}

/// Run the selected detectors over `events` (already filtered to what may be
/// seen) and describe the revisions they imply. Pure: no log access.
pub fn derive(events: &[Ev], p: &Params, kinds: &Kinds) -> Derivation {
    let mut d = Derivation::default();
    if kinds.habits {
        let (hs, insuff) = habits::detect(events, p);
        d.insufficient_habits = insuff;
        for h in hs {
            let (cited, total) = cap(h.cited.clone());
            let note = if total > cited.len() { format!("; cites the newest {} of {total}", cited.len()) } else { String::new() };
            d.items.push(Derived {
                kind: "habits",
                entity_type: "habit",
                key: format!("ctx:{}|act:{}", h.context, h.action),
                stance: format!(
                    "in context '{}' does '{}' ({} of {} occasions, rate {:.3}, Wilson 95% lower bound {:.3})",
                    h.context, h.action, h.k, h.n, h.rate, h.lower
                ),
                confidence: h.lower,
                rationale: format!("rule habits-v1 ({}): counts occasions tagged ctx:{} and their act: tag{note}", p.label(), h.context),
                valid_from: h.last_ts.clone(),
                author: AUTHOR_HABITS,
                detail: json!({
                    "context": h.context, "action": h.action, "k": h.k, "n": h.n, "rate": round3(h.rate), "wilson_lower": round3(h.lower),
                    "evidence_count": total, "evidence": evidence_sample(&cited),
                }),
                caused_by: cited,
            });
        }
    }
    if kinds.changes {
        let (tcs, insuff) = changes::detect(events, p);
        d.insufficient_topics = insuff;
        for t in tcs {
            let parts: Vec<String> = t
                .changes
                .iter()
                .map(|c| format!("{:.2}/day to {:.2}/day from {} (statistic {:.1})", c.rate_before, c.rate_after, c.date, c.stat))
                .collect();
            let last = t.changes.last().expect("non-empty");
            let (cited, total) = cap(t.cited.clone());
            let valid_from = crate::learn::dates::midnight_of(last.day);
            d.items.push(Derived {
                kind: "changes",
                entity_type: "belief",
                key: format!("topic:{}", t.topic),
                stance: format!("mentions of topic '{}' changed: {}", t.topic, parts.join("; ")),
                confidence: (1.0 - (-(last.stat) / 2.0).exp()).clamp(0.0, 1.0),
                rationale: format!(
                    "rule changes-v1 (statistic>={:.1}, min_seg={}d, ratio>={:.1}): Poisson likelihood-ratio scan over daily mention counts; cites all {} mentions of the topic",
                    p.change_llr, p.change_min_seg, p.change_min_ratio, total
                ),
                valid_from,
                author: AUTHOR_CHANGES,
                detail: json!({
                    "topic": t.topic, "changes": t.changes.iter().map(|c| json!({"date": c.date, "rate_before": round3(c.rate_before), "rate_after": round3(c.rate_after), "statistic": round3(c.stat)})).collect::<Vec<_>>(),
                    "evidence_count": total, "evidence": evidence_sample(&cited),
                }),
                caused_by: cited,
            });
        }
    }
    if kinds.stances {
        for c in stances::extract(events) {
            d.items.push(Derived {
                kind: "stances",
                entity_type: "belief",
                key: format!("stance:{}", c.topic),
                stance: c.stance.clone(),
                confidence: stances::STANCE_CONF,
                rationale: format!("rule stances-v1: explicit '{}' cue in the cited event: \"{}\"", c.cue, c.phrase),
                valid_from: c.ts.clone(),
                author: AUTHOR_STANCES,
                detail: json!({ "topic": c.topic, "cue": c.cue, "stance": c.stance, "evidence_count": 1, "evidence": [c.event_id.clone()] }),
                caused_by: vec![c.event_id.clone()],
            });
        }
    }
    // Chronological per key, then stable: supersedes chains follow valid time.
    d.items.sort_by(|a, b| (&a.valid_from, &a.key, &a.stance).cmp(&(&b.valid_from, &b.key, &b.stance)));
    d
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// signature -> rev_id of every revision already in the log.
pub fn existing_signatures(log: &VersionedLog) -> HashMap<String, String> {
    let snap = log.open_latest();
    let mut causes: HashMap<String, Vec<String>> = HashMap::new();
    for c in snap.rows::<CauseRow>() {
        causes.entry(c.rev_id).or_default().push(c.event_id);
    }
    let mut m = HashMap::new();
    for r in snap.rows::<RevisionRow>() {
        let c = causes.remove(&r.rev_id).unwrap_or_default();
        m.insert(signature(&r.entity_type, &r.key, &r.stance, r.confidence, &r.rationale, &r.valid_from, &r.author, &c), r.rev_id);
    }
    m
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub item: usize,
    /// None in a dry run for rows that would be added.
    pub rev_id: Option<String>,
    pub is_new: bool,
}

#[derive(Debug, Clone)]
pub struct LearnResult {
    pub derivation: Derivation,
    pub outcomes: Vec<Outcome>,
    pub version_before: u64,
    pub version_after: u64,
    pub dry_run: bool,
}

/// Derive from the whole log and (unless `dry_run`) write the new rows in one commit.
pub fn learn(log: &mut VersionedLog, p: &Params, kinds: &Kinds, dry_run: bool) -> Result<LearnResult, LogError> {
    let version_before = log.latest_version();
    let events = super::learnable(&log.open_latest().rows());
    let derivation = derive(&events, p, kinds);
    let existing = existing_signatures(log);
    let mut outcomes = Vec::new();
    let mut batch = Batch::default();
    let mut pending: Vec<usize> = Vec::new();
    for (i, it) in derivation.items.iter().enumerate() {
        match existing.get(&it.signature()) {
            Some(id) => outcomes.push(Outcome { item: i, rev_id: Some(id.clone()), is_new: false }),
            None => {
                pending.push(outcomes.len());
                outcomes.push(Outcome { item: i, rev_id: None, is_new: true });
                batch.revisions.push(it.to_new());
            }
        }
    }
    let mut version_after = version_before;
    if !dry_run && !batch.revisions.is_empty() {
        let info = log.commit_batch(batch)?;
        version_after = info.version;
        for (slot, id) in pending.iter().zip(info.rev_ids.iter()) {
            outcomes[*slot].rev_id = Some(id.clone());
        }
    }
    Ok(LearnResult { derivation, outcomes, version_before, version_after, dry_run })
}

/// Current rule set: the newest `rule`/`learner-params` revision, else defaults.
pub fn current_params(log: &VersionedLog) -> (Params, Option<String>) {
    current_params_in(&log.open_latest().rows::<RevisionRow>())
}

pub fn current_params_in(revs: &[RevisionRow]) -> (Params, Option<String>) {
    let head = revs.iter().rev().find(|r| r.entity_type == "rule" && r.key == super::params::RULE_KEY && r.author.starts_with("rule:"));
    match head {
        Some(r) => match Params::from_canonical(&r.stance) {
            Some(p) => (p, Some(r.rev_id.clone())),
            None => (Params::default(), None),
        },
        None => (Params::default(), None),
    }
}
