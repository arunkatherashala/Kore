//! The prediction ledger on the versioned log.
//!
//! `make` writes prediction rows (one per predictor, all for the same
//! question) BEFORE any outcome exists, using only events up to `made_at`;
//! the model version string records the log version the forecast was
//! computed from (`cutoff-v<N>`). A predictor with too little data does not
//! guess: a refusal event is logged instead. `resolve` attaches outcomes and
//! refuses an outcome that is not after its prediction. `score` is read-only;
//! it excludes and lists any pair that breaks the order (outcome dated before
//! the prediction, outcome event not after it, or written in the same commit)
//! and reports the rest with intervals and paired comparisons.
//!
//! `made_at` is supplied by the caller (default: now). The cut-off version
//! and the commit order are what the log itself vouches for.

use super::backtest::predict_at;
use super::baselines::{self, Config, Kind};
use super::score::{self, Point, Verdict, MIN_PAIRED_N};
use super::{obs_from_events, Obs, Pred, OTHER};
use crate::learn::{learnable, Ev, PREDICT_TAG};
use crate::versioned::{normalize_ts, now_iso, Batch, EventRow, LogError, NewEvent, NewOutcome, NewPrediction, OutcomeRow, PredictionRow, VersionedLog};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const MODEL_PREFIX: &str = "predict:";
pub const SOURCE: &str = "predict";
pub const MAX_CITED: usize = 20;
const BOOT_SEED: u64 = 17;

fn invalid<T>(s: impl Into<String>) -> Result<T, LogError> {
    Err(LogError::Invalid(s.into()))
}

pub struct MakeReq {
    pub context: String,
    /// Predictor ids; empty means all five baselines.
    pub predictors: Vec<String>,
    pub made_at: Option<String>,
    pub question_id: Option<String>,
    pub cfg: Config,
}

pub struct MakeResult {
    pub question_id: String,
    pub made_at: String,
    pub cutoff_version: u64,
    pub history_n: usize,
    pub written: Vec<Value>,
    pub already_present: usize,
    pub refused: Vec<Value>,
    pub version: u64,
}

pub fn make(log: &mut VersionedLog, req: &MakeReq) -> Result<MakeResult, LogError> {
    let ctx = req.context.trim();
    if ctx.is_empty() || ctx.len() > 200 {
        return invalid("context must be 1..200 characters");
    }
    let kinds: Vec<Kind> = if req.predictors.is_empty() {
        baselines::KINDS.to_vec()
    } else {
        let mut v = Vec::new();
        for p in &req.predictors {
            let k = Kind::from_id(p).ok_or_else(|| LogError::Invalid(format!("unknown predictor '{p}' (use one of: {})", baselines::KINDS.map(|k| k.id()).join(", "))))?;
            if !v.contains(&k) {
                v.push(k);
            }
        }
        v
    };
    let made_at = match &req.made_at {
        Some(s) => normalize_ts(s).map_err(LogError::Invalid)?,
        None => now_iso(),
    };
    let evs: Vec<Ev> = learnable(&log.open_latest().rows());
    let act_evs: Vec<&Ev> = evs.iter().filter(|e| e.tag_val(crate::learn::ACT_PREFIX).is_some()).collect();
    let hist: Vec<Obs> = obs_from_events(&evs);
    if let Some(late) = hist.iter().filter(|o| o.ts > made_at).map(|o| o.ts.clone()).max() {
        return invalid(format!(
            "made_at {made_at} is earlier than an event already in the log ({late}): a prediction cannot be made after its outcome could be known"
        ));
    }
    let cutoff_version = log.latest_version();
    let question_id = match &req.question_id {
        Some(q) if !q.trim().is_empty() => q.trim().to_string(),
        _ => format!("next-choice:{ctx}@{made_at}"),
    };
    let target = format!("next choice in context '{ctx}'");
    let options = super::universe(&hist);
    // events the forecast rests on: the newest ones in the context (all, if the context has none)
    let mut cite: Vec<String> = act_evs.iter().filter(|e| e.tag_val(crate::learn::CTX_PREFIX).unwrap_or(super::NO_CONTEXT) == ctx).map(|e| e.id.clone()).collect();
    if cite.is_empty() {
        cite = act_evs.iter().map(|e| e.id.clone()).collect();
    }
    if cite.len() > MAX_CITED {
        cite.drain(..cite.len() - MAX_CITED);
    }
    let have: BTreeSet<String> = log.open_latest().rows::<PredictionRow>().into_iter().filter(|r| r.question_id == question_id).map(|r| r.model_id).collect();
    let mut already_present = 0usize;
    let mut batch = Batch::default();
    let mut written = Vec::new();
    let mut refused = Vec::new();
    for k in kinds {
        if have.contains(&format!("{MODEL_PREFIX}{}", k.id())) {
            // one forecast per (question, predictor), whatever the log version now is
            already_present += 1;
            continue;
        }
        let p = baselines::make(k, &req.cfg);
        let pred = if hist.is_empty() { Pred::Insufficient { n: 0, min_n: 1 } } else { p.predict(&hist, ctx, &options) };
        match pred {
            Pred::Dist(probs) => {
                written.push(json!({"predictor": k.id(), "options": options, "probs": probs.iter().map(|x| (x * 1e6).round() / 1e6).collect::<Vec<_>>()}));
                batch.predictions.push(NewPrediction {
                    made_at: made_at.clone(),
                    question_id: question_id.clone(),
                    target: target.clone(),
                    options: options.clone(),
                    probs,
                    model_id: format!("{MODEL_PREFIX}{}", k.id()),
                    model_version: format!("{} cutoff-v{cutoff_version}", p.version()),
                    features_cite: cite.clone(),
                    resolves_by: None,
                });
            }
            Pred::Insufficient { n, min_n } => {
                refused.push(json!({"predictor": k.id(), "n": n, "min_n": min_n, "reason": "insufficient data"}));
                batch.events.push(NewEvent {
                    source: SOURCE.into(),
                    occurred_at: made_at.clone(),
                    kind: "note".into(),
                    text: json!({"refusal": "insufficient data", "predictor": k.id(), "context": ctx, "n": n, "min_n": min_n, "question_id": question_id}).to_string(),
                    tags: vec![PREDICT_TAG.into(), format!("predictor:{}", k.id())],
                    redacted: false,
                });
            }
        }
    }
    let info = log.commit_batch(batch)?;
    for (w, id) in written.iter_mut().zip(info.pred_ids.iter()) {
        w["pred_id"] = json!(id);
    }
    Ok(MakeResult {
        question_id,
        made_at,
        cutoff_version,
        history_n: hist.len(),
        already_present,
        written,
        refused,
        version: info.version,
    })
}

pub struct ResolveReq {
    pub question_id: Option<String>,
    pub pred_id: Option<String>,
    pub actual: String,
    pub resolved_at: String,
    pub outcome_event_id: Option<String>,
}

pub struct ResolveResult {
    pub resolved: Vec<String>,
    pub already_resolved: Vec<String>,
    pub actual_recorded: String,
    pub version: u64,
}

pub fn resolve(log: &mut VersionedLog, req: &ResolveReq) -> Result<ResolveResult, LogError> {
    let resolved_at = normalize_ts(&req.resolved_at).map_err(LogError::Invalid)?;
    let snap = log.open_latest();
    let preds: Vec<PredictionRow> = snap.rows();
    let done: BTreeSet<String> = snap.rows::<OutcomeRow>().into_iter().map(|o| o.pred_id).collect();
    let targets: Vec<&PredictionRow> = match (&req.pred_id, &req.question_id) {
        (Some(p), _) => preds.iter().filter(|r| &r.pred_id == p).collect(),
        (None, Some(q)) => preds.iter().filter(|r| &r.question_id == q).collect(),
        (None, None) => return invalid("give question_id or pred_id"),
    };
    if targets.is_empty() {
        return Err(LogError::NotFound("no prediction for that question_id / pred_id".into()));
    }
    let outcome_event_at = match &req.outcome_event_id {
        Some(id) => Some(
            snap.rows::<EventRow>().into_iter().find(|e| &e.event_id == id).map(|e| e.occurred_at).ok_or_else(|| LogError::NotFound(format!("outcome_event_id {id}")))?,
        ),
        None => None,
    };
    let mut batch = Batch::default();
    let (mut resolved, mut already) = (Vec::new(), Vec::new());
    let mut recorded = req.actual.clone();
    for p in targets {
        if done.contains(&p.pred_id) {
            already.push(p.pred_id.clone());
            continue;
        }
        if resolved_at < p.made_at {
            return invalid(format!("prediction {} was made at {} which is after the outcome time {resolved_at}: refused", p.pred_id, p.made_at));
        }
        if let Some(ev_at) = &outcome_event_at {
            if *ev_at <= p.made_at {
                return invalid(format!("the outcome event happened at {ev_at}, not after the prediction ({}): refused", p.made_at));
            }
        }
        let options: Vec<String> = serde_json::from_str(&p.options).unwrap_or_default();
        let mut actual = req.actual.clone();
        if !options.contains(&actual) && options.iter().any(|o| o == OTHER) {
            actual = OTHER.to_string();
        }
        recorded = actual.clone();
        resolved.push(p.pred_id.clone());
        batch.outcomes.push(NewOutcome { pred_id: p.pred_id.clone(), resolved_at: resolved_at.clone(), actual, outcome_event_id: req.outcome_event_id.clone() });
    }
    let info = log.commit_batch(batch)?;
    Ok(ResolveResult { resolved, already_resolved: already, actual_recorded: recorded, version: info.version })
}

/// First version whose cumulative row count of `table` exceeds the 0-based row index `k`.
fn commit_version(cum: &[(u64, u64)], k: usize) -> u64 {
    cum.iter().find(|(_, rows)| *rows > k as u64).map(|(v, _)| *v).unwrap_or(u64::MAX)
}

fn r4(x: f64) -> Value {
    json!((x * 1e4).round() / 1e4)
}

fn ci_json(c: &score::Ci) -> Value {
    json!({"mean": r4(c.mean), "lo": r4(c.lo), "hi": r4(c.hi)})
}

struct Scored {
    question: String,
    made_at: String,
    point: Point,
}

/// Read-only scoring report of every resolved prediction in the log.
pub fn score_report(log: &VersionedLog) -> Value {
    let snap = log.open_latest();
    let preds: Vec<PredictionRow> = snap.rows();
    let outs: Vec<OutcomeRow> = snap.rows();
    let events: Vec<EventRow> = snap.rows();
    let ev_at: HashMap<&str, &str> = events.iter().map(|e| (e.event_id.as_str(), e.occurred_at.as_str())).collect();
    let versions = log.versions();
    let cum = |t: &str| -> Vec<(u64, u64)> { versions.iter().map(|v| (v.v, v.rows.get(t).copied().unwrap_or(0))).collect() };
    let (cum_p, cum_o) = (cum("predictions"), cum("outcomes"));
    let pred_ix: HashMap<&str, usize> = preds.iter().enumerate().map(|(i, p)| (p.pred_id.as_str(), i)).collect();

    let mut by_model: BTreeMap<String, Vec<Scored>> = BTreeMap::new();
    let mut pending: BTreeMap<String, usize> = BTreeMap::new();
    let mut flagged: Vec<Value> = Vec::new();
    let resolved_ids: BTreeSet<&str> = outs.iter().map(|o| o.pred_id.as_str()).collect();
    for p in &preds {
        if !resolved_ids.contains(p.pred_id.as_str()) {
            *pending.entry(p.model_id.clone()).or_default() += 1;
        }
    }
    for (oi, o) in outs.iter().enumerate() {
        let Some(&pi) = pred_ix.get(o.pred_id.as_str()) else { continue };
        let p = &preds[pi];
        let mut why: Vec<&str> = Vec::new();
        if o.resolved_at < p.made_at {
            why.push("outcome dated before the prediction");
        }
        if let Some(e) = o.outcome_event_id.as_deref().and_then(|id| ev_at.get(id)) {
            if **e <= *p.made_at {
                why.push("outcome event not after the prediction");
            }
        }
        if commit_version(&cum_o, oi) <= commit_version(&cum_p, pi) {
            why.push("outcome written in the same commit as the prediction");
        }
        let options: Vec<String> = serde_json::from_str(&p.options).unwrap_or_default();
        let probs: Vec<f64> = serde_json::from_str(&p.probs).unwrap_or_default();
        let Some(actual) = options.iter().position(|x| *x == o.actual) else {
            why.push("actual is not one of the options");
            flagged.push(json!({"pred_id": p.pred_id, "model": p.model_id, "reasons": why}));
            continue;
        };
        if !why.is_empty() {
            flagged.push(json!({"pred_id": p.pred_id, "model": p.model_id, "reasons": why}));
            continue;
        }
        by_model.entry(p.model_id.clone()).or_default().push(Scored { question: p.question_id.clone(), made_at: p.made_at.clone(), point: Point { probs, actual } });
    }
    for v in by_model.values_mut() {
        v.sort_by(|a, b| (&a.made_at, &a.question).cmp(&(&b.made_at, &b.question)));
    }
    let mut refusals: BTreeMap<String, usize> = BTreeMap::new();
    for e in &events {
        if e.source == SOURCE && e.tags.split(',').any(|t| t == PREDICT_TAG) {
            if let Some(t) = e.tags.split(',').find_map(|t| t.strip_prefix("predictor:")) {
                *refusals.entry(t.to_string()).or_default() += 1;
            }
        }
    }
    let mut models = Vec::new();
    for (m, v) in &by_model {
        let pts: Vec<Point> = v.iter().map(|s| s.point.clone()).collect();
        let s = score::summarize_ci(&pts, BOOT_SEED);
        let bins: Vec<Value> = score::calibration(&pts, score::BINS)
            .iter()
            .filter(|b| b.n > 0)
            .map(|b| json!({"range": format!("{:.1}-{:.1}", b.lo, b.hi), "n": b.n, "mean_confidence": r4(b.mean_conf), "hit_rate": r4(b.accuracy)}))
            .collect();
        models.push(json!({
            "model": m, "scored": s.n, "pending": pending.get(m).copied().unwrap_or(0),
            "brier": ci_json(&s.brier), "log_loss": ci_json(&s.log_loss), "accuracy": ci_json(&s.accuracy),
            "ece": r4(s.ece), "calibration": bins,
            "refusals_logged": refusals.get(m.strip_prefix(MODEL_PREFIX).unwrap_or(m)).copied().unwrap_or(0),
        }));
    }
    let base_models: Vec<String> = baselines::KINDS.iter().map(|k| format!("{MODEL_PREFIX}{}", k.id())).filter(|m| by_model.contains_key(m)).collect();
    let mut comparisons = Vec::new();
    for (m, v) in &by_model {
        for b in &base_models {
            if b == m {
                continue;
            }
            let bq: HashMap<&str, &Scored> = by_model[b].iter().map(|s| (s.question.as_str(), s)).collect();
            let (mut pp, mut bp) = (Vec::new(), Vec::new());
            for s in v {
                if let Some(x) = bq.get(s.question.as_str()) {
                    pp.push(s.point.clone());
                    bp.push(x.point.clone());
                }
            }
            let c = score::paired(&pp, &bp, BOOT_SEED);
            comparisons.push(json!({
                "model": m, "versus": b, "common_questions": c.n,
                "brier_gain": ci_json(&c.d_brier), "log_loss_gain": ci_json(&c.d_log_loss), "wins": c.wins,
                "verdict": c.verdict.name(),
            }));
        }
    }
    let any_claim = comparisons.iter().any(|c| c["verdict"] == Verdict::Better.name());
    let total_scored: usize = by_model.values().map(|v| v.len()).sum();
    json!({
        "models": models,
        "paired_comparisons": comparisons,
        "flagged_excluded": flagged,
        "pending_by_model": pending,
        "refusals_logged_by_predictor": refusals,
        "min_questions_for_verdict": MIN_PAIRED_N,
        "any_significant_win": any_claim,
        "note": if total_scored == 0 {
            "no resolved predictions yet: nothing to score and no accuracy is claimed".to_string()
        } else {
            format!("gain = baseline minus predictor, so positive means the predictor is better; 95% moving-block bootstrap intervals; verdicts need at least {MIN_PAIRED_N} common questions and use the Brier interval; this is accuracy on your own log so far, not a general claim")
        },
    })
}

/// Backtest-free helper for tests and callers: what `make` would forecast for `ctx`
/// from the observations dated at or before `cutoff`, per predictor id.
pub fn forecast_from(obs: &[Obs], cutoff: &str, ctx: &str, cfg: &Config) -> Vec<(&'static str, Option<Vec<f64>>)> {
    let visible: Vec<Obs> = obs.iter().filter(|o| o.ts.as_str() <= cutoff).cloned().collect();
    let mut probe = visible.clone();
    probe.push(Obs { ts: cutoff.to_string(), ctx: ctx.to_string(), choice: OTHER.to_string() });
    let t = visible.len();
    baselines::all(cfg)
        .iter()
        .map(|p| {
            let (_, pred) = predict_at(p.as_ref(), &probe, t);
            (p.id(), if let Pred::Dist(d) = pred { Some(d) } else { None })
        })
        .collect()
}

