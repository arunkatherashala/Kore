//! Evolution loop for the learner's own rules (parameters), never source code.
//!
//! propose -> backtest on the past of the log -> promote only if it wins ->
//! log the decision. Rollback is time travel: every rule set is a revision
//! (entity_type `rule`, key `learner-params`), so an earlier set is readable
//! with `self_sql ... as_of_version`, and `rollback_rules` re-instates one as
//! a new revision (library only; no tool writes rules).
//!
//! Score (defined once, used for selection, comparison and the baseline):
//! "next-period habit occurrence". At an origin day t the learner sees only
//! events with day < t and predicts a set of (context, action) pairs. The
//! truth for [t, t+H) is the set of pairs where the context had at least
//! `eval_min` occasions in that window and the action was at least
//! `eval_share` of them. Micro-averaged F1 over origins:
//! F1 = 2TP / (2TP + FP + FN) (0 when everything is empty).
//! Baseline: in every context seen so far, predict its most frequent action,
//! no minimum n, no bound.
//!
//! Rolling origin with a selection cut V = d0 + 70% of the span: candidates
//! are ranked using only events before V and truth windows that end before V
//! (origins V-H-21 .. V-H, step 7); the winner is then compared with the
//! incumbent and the baseline on origins V, V+7, ... whose windows lie inside
//! the log. Promotion needs F1(candidate) >= F1(incumbent) + margin AND
//! F1(candidate) >= F1(baseline) + margin on that second stage, and the
//! candidate must also have beaten the incumbent in selection.

use super::dates;
use super::derive::{current_params_in, MAX_CITED};
use super::params::{Params, RULE_KEY};
use super::{habits, visible_before, Ev};
use crate::versioned::{sha256_hex, AsOf, Batch, LogError, NewEvent, NewRevision, RevisionRow, VersionedLog};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const AUTHOR: &str = "rule:evolve-v1";

#[derive(Debug, Clone)]
pub struct EvolveCfg {
    pub horizon: i64,
    pub margin: f64,
    pub min_span_days: i64,
    pub eval_min: u32,
    pub eval_share: f64,
}
impl Default for EvolveCfg {
    fn default() -> Self {
        EvolveCfg { horizon: 14, margin: 0.02, min_span_days: 90, eval_min: 3, eval_share: 0.5 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Score {
    pub tp: u32,
    pub fp: u32,
    pub fn_: u32,
}
impl Score {
    pub fn f1(&self) -> f64 {
        let d = 2 * self.tp + self.fp + self.fn_;
        if d == 0 {
            0.0
        } else {
            2.0 * self.tp as f64 / d as f64
        }
    }
}

type Pairs = BTreeSet<(String, String)>;

/// What the learner predicts at `origin`, from events before it only.
pub fn predict_at(events: &[Ev], p: &Params, origin: i64) -> Pairs {
    let vis = visible_before(events, origin);
    habits::detect(&vis, p).0.into_iter().map(|h| (h.context, h.action)).collect()
}

pub fn baseline_at(events: &[Ev], origin: i64) -> Pairs {
    let vis = visible_before(events, origin);
    habits::modal_baseline(&vis, 0).into_iter().collect()
}

/// Truth for the window [origin, origin + horizon). Uses the future by design;
/// it is never given to a predictor.
pub fn truth_at(events: &[Ev], origin: i64, cfg: &EvolveCfg) -> Pairs {
    let mut by: BTreeMap<&str, (u32, BTreeMap<&str, u32>)> = BTreeMap::new();
    for e in events.iter().filter(|e| e.day >= origin && e.day < origin + cfg.horizon) {
        let Some(c) = e.tag_val(super::CTX_PREFIX) else { continue };
        let a = e.tag_val(super::ACT_PREFIX).unwrap_or(super::NO_ACTION);
        let s = by.entry(c).or_default();
        s.0 += 1;
        *s.1.entry(a).or_default() += 1;
    }
    let mut out = Pairs::new();
    for (c, (n, acts)) in by {
        if n < cfg.eval_min {
            continue;
        }
        for (a, k) in acts {
            if a != super::NO_ACTION && k as f64 / n as f64 >= cfg.eval_share {
                out.insert((c.to_string(), a.to_string()));
            }
        }
    }
    out
}

fn add(s: &mut Score, pred: &Pairs, truth: &Pairs) {
    s.tp += pred.intersection(truth).count() as u32;
    s.fp += pred.difference(truth).count() as u32;
    s.fn_ += truth.difference(pred).count() as u32;
}

pub fn score_params(events: &[Ev], p: &Params, origins: &[i64], cfg: &EvolveCfg) -> Score {
    let mut s = Score::default();
    for &o in origins {
        add(&mut s, &predict_at(events, p, o), &truth_at(events, o, cfg));
    }
    s
}

pub fn score_baseline(events: &[Ev], origins: &[i64], cfg: &EvolveCfg) -> Score {
    let mut s = Score::default();
    for &o in origins {
        add(&mut s, &baseline_at(events, o), &truth_at(events, o, cfg));
    }
    s
}

/// Single-parameter moves around the incumbent, in a fixed order.
pub fn neighbours(inc: &Params) -> Vec<Params> {
    let mut v: Vec<Params> = Vec::new();
    for dn in [-10i64, -5, -2, 2, 5, 10] {
        let n = inc.habit_min_n as i64 + dn;
        if n >= 1 {
            v.push(Params { habit_min_n: n as u32, ..inc.clone() });
        }
    }
    for dl in [-0.10f64, -0.05, 0.05, 0.10] {
        let l = ((inc.habit_lb + dl) * 100.0).round() / 100.0;
        if (0.0..=0.95).contains(&l) {
            v.push(Params { habit_lb: l, ..inc.clone() });
        }
    }
    for w in [0u32, 30, 60, 90, 120] {
        if w != inc.habit_window_days {
            v.push(Params { habit_window_days: w, ..inc.clone() });
        }
    }
    v
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Promoted,
    Rejected,
    /// Not enough history to backtest; nothing is recorded.
    Insufficient,
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub verdict: Verdict,
    pub reason: String,
    pub incumbent: Params,
    pub candidate: Option<Params>,
    pub cut_day: i64,
    pub select_origins: Vec<i64>,
    pub eval_origins: Vec<i64>,
    pub select_candidate: Score,
    pub select_incumbent: Score,
    pub eval_candidate: Score,
    pub eval_incumbent: Score,
    pub eval_baseline: Score,
    pub margin: f64,
    pub candidates_tried: usize,
}

pub fn cut_and_origins(events: &[Ev], cfg: &EvolveCfg) -> Option<(i64, Vec<i64>, Vec<i64>)> {
    let d0 = events.iter().map(|e| e.day).min()?;
    let d1 = events.iter().map(|e| e.day).max()?;
    let span = d1 - d0;
    if span < cfg.min_span_days {
        return None;
    }
    let cut = d0 + span * 7 / 10;
    let h = cfg.horizon;
    let select: Vec<i64> = (0..4).map(|i| cut - h - 21 + 7 * i).collect();
    let mut eval = Vec::new();
    let mut o = cut;
    while o + h - 1 <= d1 {
        eval.push(o);
        o += 7;
    }
    if eval.len() < 2 {
        return None;
    }
    Some((cut, select, eval))
}

/// Choose among `candidates` using only the past of the cut, then compare the
/// winner with the incumbent and the baseline after the cut.
pub fn evolve(events: &[Ev], incumbent: &Params, candidates: &[Params], cfg: &EvolveCfg) -> Decision {
    let mut d = Decision {
        verdict: Verdict::Insufficient,
        reason: String::new(),
        incumbent: incumbent.clone(),
        candidate: None,
        cut_day: 0,
        select_origins: vec![],
        eval_origins: vec![],
        select_candidate: Score::default(),
        select_incumbent: Score::default(),
        eval_candidate: Score::default(),
        eval_incumbent: Score::default(),
        eval_baseline: Score::default(),
        margin: cfg.margin,
        candidates_tried: candidates.len(),
    };
    let Some((cut, select, eval)) = cut_and_origins(events, cfg) else {
        d.reason = format!("not enough history: need a span of at least {} days and room for two evaluation origins after the cut", cfg.min_span_days);
        return d;
    };
    d.cut_day = cut;
    d.select_origins = select.clone();
    d.eval_origins = eval.clone();
    // Selection sees nothing at or after the cut.
    let past = visible_before(events, cut);
    d.select_incumbent = score_params(&past, incumbent, &select, cfg);
    let mut best: Option<(usize, Score)> = None;
    for (i, c) in candidates.iter().enumerate() {
        if c == incumbent {
            continue;
        }
        let s = score_params(&past, c, &select, cfg);
        if best.map_or(true, |(_, b)| s.f1() > b.f1() + 1e-12) {
            best = Some((i, s));
        }
    }
    let Some((bi, bs)) = best else {
        d.verdict = Verdict::Rejected;
        d.reason = "no candidate differs from the incumbent".into();
        return d;
    };
    let cand = candidates[bi].clone();
    d.select_candidate = bs;
    d.eval_candidate = score_params(events, &cand, &eval, cfg);
    d.eval_incumbent = score_params(events, incumbent, &eval, cfg);
    d.eval_baseline = score_baseline(events, &eval, cfg);
    let (fc, fi, fb) = (d.eval_candidate.f1(), d.eval_incumbent.f1(), d.eval_baseline.f1());
    d.candidate = Some(cand);
    if bs.f1() <= d.select_incumbent.f1() {
        d.verdict = Verdict::Rejected;
        d.reason = "best candidate did not beat the incumbent in selection (before the cut)".into();
    } else if fc < fi + cfg.margin {
        d.verdict = Verdict::Rejected;
        d.reason = format!("candidate F1 {fc:.3} is not at least {:.3} above incumbent F1 {fi:.3}", cfg.margin);
    } else if fc < fb + cfg.margin {
        d.verdict = Verdict::Rejected;
        d.reason = format!("candidate F1 {fc:.3} is not at least {:.3} above baseline F1 {fb:.3}", cfg.margin);
    } else {
        d.verdict = Verdict::Promoted;
        d.reason = format!("candidate F1 {fc:.3} beats incumbent {fi:.3} and baseline {fb:.3} by at least {:.3}", cfg.margin);
    }
    d
}

impl Decision {
    pub fn to_json(&self) -> Value {
        let sc = |s: &Score| json!({"tp": s.tp, "fp": s.fp, "fn": s.fn_, "f1": (s.f1() * 1000.0).round() / 1000.0});
        json!({
            "verdict": match self.verdict { Verdict::Promoted => "promoted", Verdict::Rejected => "rejected", Verdict::Insufficient => "insufficient_history" },
            "reason": self.reason,
            "incumbent": self.incumbent.to_json(),
            "candidate": self.candidate.as_ref().map(|c| c.to_json()),
            "candidates_tried": self.candidates_tried,
            "cut_date": if self.cut_day != 0 { Value::String(dates::date_of(self.cut_day)) } else { Value::Null },
            "selection": {"origins": self.select_origins.len(), "candidate": sc(&self.select_candidate), "incumbent": sc(&self.select_incumbent)},
            "evaluation": {"origins": self.eval_origins.len(), "candidate": sc(&self.eval_candidate), "incumbent": sc(&self.eval_incumbent), "baseline": sc(&self.eval_baseline)},
            "margin": self.margin,
        })
    }
    fn event_text(&self) -> String {
        let word = if self.verdict == Verdict::Promoted { "promotion" } else { "rejection" };
        format!("evolve {word}: {}", self.to_json())
    }
}

#[derive(Debug, Clone)]
pub struct Recorded {
    pub event_id: String,
    pub rev_id: String,
    pub version: u64,
}

fn last_ts(events: &[Ev]) -> Option<String> {
    events.iter().map(|e| e.ts.clone()).max()
}

/// Write the decision (event + rule revision citing it). Insufficient history writes nothing.
pub fn record(log: &mut VersionedLog, d: &Decision, events: &[Ev]) -> Result<Option<Recorded>, LogError> {
    if d.verdict == Verdict::Insufficient {
        return Ok(None);
    }
    let Some(ts) = last_ts(events) else { return Ok(None) };
    let (tag, key, params, conf) = match (&d.verdict, &d.candidate) {
        (Verdict::Promoted, Some(c)) => ("promotion", RULE_KEY.to_string(), c.clone(), d.eval_candidate.f1()),
        (_, Some(c)) => {
            let h = sha256_hex(&[c.canonical().as_bytes()]);
            ("rejection", format!("rejected:{}", &h[..12]), c.clone(), d.eval_candidate.f1())
        }
        (_, None) => ("rejection", "rejected:none".to_string(), d.incumbent.clone(), 0.0),
    };
    let ev = NewEvent { source: "evolve".into(), occurred_at: ts.clone(), kind: "decision".into(), text: d.event_text(), tags: vec!["evolve".into(), tag.into()], redacted: false };
    let eid = crate::versioned::event_id(&ev.source, &crate::versioned::normalize_ts(&ts).map_err(LogError::Invalid)?, &ev.text);
    let rev = NewRevision {
        entity_type: "rule".into(),
        key,
        stance: params.canonical(),
        confidence: conf.clamp(0.0, 1.0),
        rationale: d.reason.clone(),
        supersedes: None,
        valid_from: ts,
        author: AUTHOR.into(),
        caused_by: vec![eid.clone()],
    };
    let info = log.commit_batch(Batch { events: vec![ev], revisions: vec![rev], ..Default::default() })?;
    Ok(Some(Recorded { event_id: eid, rev_id: info.rev_ids.first().cloned().unwrap_or_default(), version: info.version }))
}

/// Full proposal step on a log: current rules, neighbours, decision, and (unless dry) the record.
pub fn run(log: &mut VersionedLog, cfg: &EvolveCfg, dry_run: bool) -> Result<(Decision, Option<Recorded>), LogError> {
    let snap = log.open_latest();
    let events = super::learnable(&snap.rows());
    let (inc, _) = current_params_in(&snap.rows::<RevisionRow>());
    let d = evolve(&events, &inc, &neighbours(&inc), cfg);
    let rec = if dry_run { None } else { record(log, &d, &events)? };
    Ok((d, rec))
}

/// Re-instate the rule set that was current at `version` as a new revision.
pub fn rollback_rules(log: &mut VersionedLog, version: u64) -> Result<Option<Recorded>, LogError> {
    let old = log.open_at(AsOf::Version(version))?;
    let (p, id) = current_params_in(&old.rows::<RevisionRow>());
    if id.is_none() {
        return Err(LogError::NotFound(format!("no rule set recorded at version {version}")));
    }
    let events = super::learnable(&log.open_latest().rows());
    let Some(ts) = last_ts(&events) else { return Ok(None) };
    let text = format!("evolve rollback: re-instated the rule set of version {version}: {}", p.canonical());
    let nts = crate::versioned::normalize_ts(&ts).map_err(LogError::Invalid)?;
    let eid = crate::versioned::event_id("evolve", &nts, &text);
    let ev = NewEvent { source: "evolve".into(), occurred_at: ts.clone(), kind: "decision".into(), text, tags: vec!["evolve".into(), "rollback".into()], redacted: false };
    let rev = NewRevision {
        entity_type: "rule".into(),
        key: RULE_KEY.into(),
        stance: p.canonical(),
        confidence: 1.0,
        rationale: format!("rollback to the rule set of version {version}"),
        supersedes: None,
        valid_from: ts,
        author: AUTHOR.into(),
        caused_by: vec![eid.clone()],
    };
    let info = log.commit_batch(Batch { events: vec![ev], revisions: vec![rev], ..Default::default() })?;
    Ok(Some(Recorded { event_id: eid, rev_id: info.rev_ids.first().cloned().unwrap_or_default(), version: info.version }))
}

#[allow(dead_code)]
const _: usize = MAX_CITED;
