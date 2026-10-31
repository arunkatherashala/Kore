//! Tests for prediction, scoring, backtest leakage and the ledger.

use super::backtest::{leakage_check, predict_at, predictor_is_leak_free, rolling_origin};
use super::baselines::{self, Config, Kind};
use super::ledger::{self, MakeReq, ResolveReq};
use super::scenarios::{self, Scenario};
use super::score::{self, Point, Verdict};
use super::*;
use crate::versioned::*;
use serde_json::{json, Value};

fn obs(ts: &str, ctx: &str, choice: &str) -> Obs {
    Obs { ts: ts.into(), ctx: ctx.into(), choice: choice.into() }
}

fn fresh(dir: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(dir, "t", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

/// Events "in ctx C the choice was X", one per hour from 2024-03-01.
fn events(n: usize, f: impl Fn(usize) -> (&'static str, &'static str)) -> Vec<NewEvent> {
    (0..n)
        .map(|i| {
            let (c, a) = f(i);
            NewEvent {
                source: "t".into(),
                occurred_at: format!("2024-03-{:02}T{:02}:00:00Z", 1 + i / 24, i % 24),
                kind: "action".into(),
                text: format!("did {a} in {c} #{i}"),
                tags: vec![format!("ctx:{c}"), format!("act:{a}")],
                redacted: false,
            }
        })
        .collect()
}

fn mk(ctx: &str, made_at: &str) -> MakeReq {
    MakeReq { context: ctx.into(), predictors: vec![], made_at: Some(made_at.into()), question_id: None, cfg: Config::default() }
}

// ── baselines ───────────────────────────────────────────────────────────────

#[test]
fn every_baseline_gives_a_valid_distribution_or_refuses() {
    let hist: Vec<Obs> = scenarios::generate(Scenario::Context, 1, 60);
    let opts = universe(&hist);
    for p in baselines::all(&Config::default()) {
        match p.predict(&hist, "c1", &opts) {
            Pred::Dist(d) => {
                assert_eq!(d.len(), opts.len(), "{}", p.id());
                assert!(d.iter().all(|x| *x > 0.0 && *x <= 1.0), "{}: {d:?}", p.id());
                assert!((d.iter().sum::<f64>() - 1.0).abs() < 1e-9);
            }
            Pred::Insufficient { .. } => panic!("{} refused with 60 observations", p.id()),
        }
    }
}

#[test]
fn minimum_n_refusal_not_a_guess() {
    let hist = scenarios::generate(Scenario::Context, 1, 6);
    let opts = universe(&hist);
    for k in [Kind::Frequency, Kind::LastChoice, Kind::Recency, Kind::ContextCount] {
        let p = baselines::make(k, &Config::default());
        assert!(matches!(p.predict(&hist, "c0", &opts), Pred::Insufficient { .. }), "{} must refuse with 6 observations", k.id());
    }
    // the context-count predictor counts only observations in the context
    let mut h = scenarios::generate(Scenario::Context, 2, 200);
    h.retain(|o| o.ctx != "c3");
    h.push(obs("2024-01-01T00:00:00Z", "c3", "a"));
    let p = baselines::make(Kind::ContextCount, &Config::default());
    assert_eq!(p.predict(&h, "c3", &universe(&h)), Pred::Insufficient { n: 1, min_n: 5 });
}

#[test]
fn frequency_and_context_count_learn_the_obvious() {
    let mut h = Vec::new();
    for i in 0..40 {
        h.push(obs("t", "x", if i % 10 == 0 { "b" } else { "a" }));
        h.push(obs("t", "y", "b"));
    }
    let o = universe(&h);
    let ia = o.iter().position(|c| c == "a").unwrap();
    let ib = o.iter().position(|c| c == "b").unwrap();
    let cc = baselines::make(Kind::ContextCount, &Config::default());
    let Pred::Dist(dx) = cc.predict(&h, "x", &o) else { panic!() };
    let Pred::Dist(dy) = cc.predict(&h, "y", &o) else { panic!() };
    assert!(dx[ia] > 0.8 && dy[ib] > 0.9, "{dx:?} {dy:?}");
    let last = baselines::make(Kind::LastChoice, &Config::default());
    let Pred::Dist(dl) = last.predict(&h, "x", &o) else { panic!() };
    assert!(dl[ib] > dl[ia], "last choice was b");
}

// ── scoring ─────────────────────────────────────────────────────────────────

#[test]
fn score_arithmetic() {
    let p = Point { probs: vec![0.7, 0.2, 0.1], actual: 0 };
    assert!((score::brier(&p) - (0.09 + 0.04 + 0.01)).abs() < 1e-12);
    assert!((score::log_loss(&p) + 0.7f64.ln()).abs() < 1e-12);
    assert!(score::correct(&p));
    let q = Point { probs: vec![0.7, 0.2, 0.1], actual: 2 };
    assert!(!score::correct(&q));
    // zero probability on the actual outcome is finite (floored), not infinite
    assert!(score::log_loss(&Point { probs: vec![1.0, 0.0], actual: 1 }).is_finite());
    // perfect calibration at confidence 0.8 with 80% hits: ECE 0
    let mut pts = Vec::new();
    for i in 0..100 {
        pts.push(Point { probs: vec![0.8, 0.2], actual: if i < 80 { 0 } else { 1 } });
    }
    let s = score::summarize(&pts);
    assert!(s.ece < 1e-9, "{}", s.ece);
    assert!((s.accuracy - 0.8).abs() < 1e-12);
    // always-confident and wrong half the time: ECE 0.45 (confidence 0.95, hit rate 0.5)
    let bad: Vec<Point> = (0..100).map(|i| Point { probs: vec![0.95, 0.05], actual: (i % 2) as usize }).collect();
    assert!((score::summarize(&bad).ece - 0.45).abs() < 1e-9);
    let bins = score::calibration(&bad, 10);
    assert_eq!(bins[9].n, 100);
}

#[test]
fn bootstrap_is_deterministic_and_brackets_the_mean() {
    let v: Vec<f64> = (0..200).map(|i| ((i * 37) % 11) as f64).collect();
    let a = score::bootstrap_mean(&v, 500, 10, 3);
    let b = score::bootstrap_mean(&v, 500, 10, 3);
    assert_eq!(a, b);
    assert!(a.lo < a.mean && a.mean < a.hi);
    let one = score::bootstrap_mean(&[1.0], 500, 10, 3);
    assert_eq!((one.lo, one.hi), (1.0, 1.0));
}

#[test]
fn paired_comparison_refuses_small_samples_and_detects_a_clear_win() {
    let good = |n: usize| -> Vec<Point> { (0..n).map(|_| Point { probs: vec![0.9, 0.1], actual: 0 }).collect() };
    let poor = |n: usize| -> Vec<Point> { (0..n).map(|_| Point { probs: vec![0.5, 0.5], actual: 0 }).collect() };
    let small = score::paired(&good(10), &poor(10), 1);
    assert_eq!(small.verdict, Verdict::InsufficientData);
    // constant differences have a zero-width interval: a clear win
    let big = score::paired(&good(100), &poor(100), 1);
    assert_eq!(big.verdict, Verdict::Better);
    assert!(big.d_brier.mean > 0.0 && big.wins == 100);
    assert_eq!(score::paired(&poor(100), &good(100), 1).verdict, Verdict::Worse);
    assert_eq!(score::paired(&good(100), &good(100), 1).verdict, Verdict::NoDifference);
}

// ── leakage ─────────────────────────────────────────────────────────────────

#[test]
fn shuffling_future_events_does_not_change_past_predictions() {
    for scenario in [Scenario::Context, Scenario::Drift, Scenario::Noise] {
        let seq = scenarios::generate(scenario, 5, 300);
        for p in baselines::all(&Config::default()) {
            for t in [0usize, 9, 10, 50, 120, 299] {
                assert!(predictor_is_leak_free(p.as_ref(), &seq, t, 99), "{} leaked at t={t} in {scenario:?}", p.id());
            }
        }
    }
}

#[test]
fn the_leakage_check_catches_a_predictor_that_peeks() {
    let seq = scenarios::generate(Scenario::Noise, 6, 200);
    // "predicts" from the NEXT observation in the sequence it was handed
    let peek = |s: &[Obs], u: usize| -> Option<Vec<f64>> {
        let ahead = s.get(u + 1..(u + 21).min(s.len()))?;
        Some(vec![ahead.iter().filter(|o| o.choice == "a").count() as f64])
    };
    assert!(!leakage_check(&seq, 50, 1, &peek), "a predictor reading the future must be flagged");
    // and a predictor that only reads obs[..u] passes
    let honest = |s: &[Obs], u: usize| -> Option<Vec<f64>> { Some(vec![s[..u].iter().filter(|o| o.choice == "a").count() as f64]) };
    assert!(leakage_check(&seq, 50, 1, &honest));
}

#[test]
fn rolling_origin_uses_only_the_past_and_matches_predict_at() {
    let seq = scenarios::generate(Scenario::Context, 7, 120);
    let preds = baselines::all(&Config::default());
    let bt = rolling_origin(&preds, &seq, 20);
    let fi = bt.index("frequency").unwrap();
    assert_eq!(bt.points[fi].len(), 100);
    let (opts, pred) = predict_at(preds[fi].as_ref(), &seq, 57);
    let Pred::Dist(d) = pred else { panic!() };
    assert_eq!(bt.points[fi][57 - 20].as_ref().unwrap().probs, d);
    assert_eq!(opts, universe(&seq[..57]));
    // changing the LAST observation changes no earlier forecast
    let mut altered = seq.clone();
    altered[119].choice = "zzz".into();
    let bt2 = rolling_origin(&preds, &altered, 20);
    for i in 0..preds.len() {
        assert_eq!(bt.points[i][..99], bt2.points[i][..99]);
    }
    // a choice never seen before the origin is scored as (other)
    let ctx0: Vec<Obs> = (0..30).map(|i| obs("t", "x", if i % 2 == 0 { "a" } else { "b" })).chain([obs("t", "x", "never-seen")]).collect();
    let bt3 = rolling_origin(&[baselines::make(Kind::Frequency, &Config::default())], &ctx0, 30);
    let pt = bt3.points[0][0].as_ref().unwrap();
    assert_eq!(pt.actual, pt.probs.len() - 1);
}

#[test]
fn forecast_from_ignores_events_after_the_cutoff() {
    let seq = scenarios::generate(Scenario::Context, 8, 200);
    let cut = &seq[99].ts;
    let a = ledger::forecast_from(&seq, cut, "c2", &Config::default());
    let mut future_changed = seq.clone();
    for o in future_changed.iter_mut().skip(100) {
        o.choice = "x".into();
        o.ctx = "c2".into();
    }
    let b = ledger::forecast_from(&future_changed, cut, "c2", &Config::default());
    assert_eq!(a, b);
}

#[test]
fn scenarios_are_deterministic() {
    assert_eq!(scenarios::generate(Scenario::Drift, 3, 50), scenarios::generate(Scenario::Drift, 3, 50));
    assert_ne!(scenarios::generate(Scenario::Drift, 3, 50), scenarios::generate(Scenario::Drift, 4, 50));
}

// ── ledger ──────────────────────────────────────────────────────────────────

fn rows_of(log: &VersionedLog) -> (Vec<PredictionRow>, Vec<OutcomeRow>) {
    let s = log.open_latest();
    (s.rows(), s.rows())
}

#[test]
fn make_writes_before_outcome_and_records_the_cutoff_version() {
    let d = tempfile::tempdir().unwrap();
    let mut log = fresh(d.path());
    log.append_events(&events(60, |i| (["m", "n"][i % 2], if i % 2 == 0 { "tea" } else { "coffee" }))).unwrap();
    let v = log.latest_version();
    let r = ledger::make(&mut log, &mk("m", "2024-04-01")).unwrap();
    assert_eq!(r.cutoff_version, v);
    assert_eq!(r.written.len(), 5);
    assert!(r.refused.is_empty());
    let (preds, outs) = rows_of(&log);
    assert!(outs.is_empty(), "no outcome exists when forecasts are written");
    assert_eq!(preds.len(), 5);
    for p in &preds {
        assert!(p.model_id.starts_with("predict:"));
        assert!(p.model_version.ends_with(&format!("cutoff-v{v}")), "{}", p.model_version);
        assert_eq!(p.made_at, "2024-04-01T00:00:00Z");
        assert!(p.question_id.starts_with("next-choice:m@"));
        let probs: Vec<f64> = serde_json::from_str(&p.probs).unwrap();
        assert!((probs.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }
    // context-count has learned that "m" means tea
    let cc = preds.iter().find(|p| p.model_id == "predict:context-count").unwrap();
    let (o, pr): (Vec<String>, Vec<f64>) = (serde_json::from_str(&cc.options).unwrap(), serde_json::from_str(&cc.probs).unwrap());
    assert!(pr[o.iter().position(|x| x == "tea").unwrap()] > 0.9);
    // same question again: nothing new
    let r2 = ledger::make(&mut log, &mk("m", "2024-04-01")).unwrap();
    assert_eq!(r2.already_present, 5);
    assert_eq!(rows_of(&log).0.len(), 5);
}

#[test]
fn insufficient_data_is_refused_and_logged_not_guessed() {
    let d = tempfile::tempdir().unwrap();
    let mut log = fresh(d.path());
    // only 6 observations in total, 3 in context "m"
    log.append_events(&events(6, |i| (["m", "n"][i % 2], "tea"))).unwrap();
    let r = ledger::make(&mut log, &mk("m", "2024-04-01")).unwrap();
    let names: Vec<String> = r.refused.iter().map(|x| x["predictor"].as_str().unwrap().to_string()).collect();
    for n in ["frequency", "last-choice", "recency", "context-count"] {
        assert!(names.contains(&n.to_string()), "{n} should have refused: {names:?}");
    }
    assert_eq!(rows_of(&log).0.len(), 1, "only the uniform forecast (it needs no data) is written");
    let evs: Vec<EventRow> = log.open_latest().rows();
    let logged = evs.iter().filter(|e| e.source == "predict" && e.tags.contains("predict_refusal")).count();
    assert_eq!(logged, 4, "every refusal is a logged event");
    // the refusal events are invisible to the learners
    assert!(crate::learn::learnable(&evs).iter().all(|e| e.source != "predict"));
    // an empty log refuses everything
    let d2 = tempfile::tempdir().unwrap();
    let mut empty = fresh(d2.path());
    let r = ledger::make(&mut empty, &mk("m", "2024-04-01")).unwrap();
    assert_eq!(r.refused.len(), 5);
    assert!(rows_of(&empty).0.is_empty());
}

#[test]
fn a_prediction_cannot_be_made_after_its_outcome_is_in_the_log() {
    let d = tempfile::tempdir().unwrap();
    let mut log = fresh(d.path());
    log.append_events(&events(60, |i| (["m", "n"][i % 2], "tea"))).unwrap();
    // events run to 2024-03-03T11:00; a forecast stamped before that is backdated
    let e = ledger::make(&mut log, &mk("m", "2024-03-02")).err().unwrap();
    assert!(e.to_string().contains("cannot be made after its outcome"), "{e}");
    assert!(rows_of(&log).0.is_empty());
}

#[test]
fn resolve_rejects_an_outcome_that_is_not_after_the_prediction() {
    let d = tempfile::tempdir().unwrap();
    let mut log = fresh(d.path());
    log.append_events(&events(60, |i| (["m", "n"][i % 2], "tea"))).unwrap();
    let r = ledger::make(&mut log, &mk("m", "2024-04-01T12:00:00Z")).unwrap();
    let rq = |at: &str, ev: Option<String>| ResolveReq { question_id: Some(r.question_id.clone()), pred_id: None, actual: "tea".into(), resolved_at: at.into(), outcome_event_id: ev };
    // outcome time before the forecast
    let e = ledger::resolve(&mut log, &rq("2024-04-01T11:59:59Z", None)).err().unwrap();
    assert!(e.to_string().contains("after the outcome time"), "{e}");
    // outcome event older than the forecast (an event already in the log)
    let old = log.open_latest().rows::<EventRow>()[0].event_id.clone();
    let e = ledger::resolve(&mut log, &rq("2024-04-02", Some(old))).err().unwrap();
    assert!(e.to_string().contains("not after the prediction"), "{e}");
    assert!(rows_of(&log).1.is_empty(), "nothing was written by the refused calls");
    // a proper outcome: later time; an unseen choice is recorded as (other)
    let ok = ledger::resolve(&mut log, &ResolveReq { question_id: Some(r.question_id.clone()), pred_id: None, actual: "cocoa".into(), resolved_at: "2024-04-02".into(), outcome_event_id: None }).unwrap();
    assert_eq!(ok.resolved.len(), 5);
    assert_eq!(ok.actual_recorded, "(other)");
    let again = ledger::resolve(&mut log, &rq("2024-04-03", None)).unwrap();
    assert_eq!(again.already_resolved.len(), 5);
    assert!(again.resolved.is_empty());
}

#[test]
fn score_excludes_and_lists_pairs_that_break_the_time_order() {
    let d = tempfile::tempdir().unwrap();
    let mut log = fresh(d.path());
    log.append_events(&events(60, |i| (["m", "n"][i % 2], "tea"))).unwrap();
    let r = ledger::make(&mut log, &mk("m", "2024-04-01")).unwrap();
    let pid = r.written[0]["pred_id"].as_str().unwrap().to_string();
    // written around the guard, straight through the library: outcome dated before the forecast
    log.record_outcome(NewOutcome { pred_id: pid, resolved_at: "2024-03-30".into(), actual: "tea".into(), outcome_event_id: None }).unwrap();
    // and one written in the same commit as its forecast
    let p2 = NewPrediction {
        made_at: "2024-04-03".into(),
        question_id: "same-commit".into(),
        target: "x".into(),
        options: vec!["a".into(), "(other)".into()],
        probs: vec![0.5, 0.5],
        model_id: "predict:frequency".into(),
        model_version: "v1".into(),
        features_cite: vec![],
        resolves_by: None,
    };
    let id2 = prediction_id(&p2).unwrap();
    log.commit_batch(Batch { predictions: vec![p2], outcomes: vec![NewOutcome { pred_id: id2, resolved_at: "2024-04-04".into(), actual: "a".into(), outcome_event_id: None }], ..Default::default() }).unwrap();
    let rep = ledger::score_report(&log);
    let flagged = rep["flagged_excluded"].as_array().unwrap();
    assert_eq!(flagged.len(), 2, "{flagged:?}");
    let reasons = flagged.iter().map(|f| f["reasons"].to_string()).collect::<String>();
    assert!(reasons.contains("before the prediction") && reasons.contains("same commit"), "{reasons}");
    assert!(rep["models"].as_array().unwrap().is_empty(), "flagged pairs are not scored");
    assert!(rep["note"].as_str().unwrap().contains("nothing to score"));
}

#[test]
fn score_with_few_questions_gives_no_verdict() {
    let d = tempfile::tempdir().unwrap();
    let mut log = fresh(d.path());
    log.append_events(&events(60, |i| (["m", "n"][i % 2], if i % 2 == 0 { "tea" } else { "coffee" }))).unwrap();
    for day in 1..=5 {
        let r = ledger::make(&mut log, &mk("m", &format!("2024-04-{day:02}"))).unwrap();
        ledger::resolve(&mut log, &ResolveReq { question_id: Some(r.question_id), pred_id: None, actual: "tea".into(), resolved_at: format!("2024-04-{day:02}T12:00:00Z"), outcome_event_id: None }).unwrap();
    }
    let rep = ledger::score_report(&log);
    let models = rep["models"].as_array().unwrap();
    assert_eq!(models.len(), 5);
    assert_eq!(models[0]["scored"], 5);
    let cmp = rep["paired_comparisons"].as_array().unwrap();
    assert!(!cmp.is_empty());
    assert!(cmp.iter().all(|c| c["verdict"] == "insufficient_data"), "5 questions are not enough for any verdict");
    assert_eq!(rep["any_significant_win"], false);
}

// ── tools ───────────────────────────────────────────────────────────────────

fn call(name: &str, owner: &str, a: Value) -> (bool, String) {
    crate::safety::test_support::init();
    let r = crate::log_tools::handle(name, &a, owner).expect("log tool");
    (r["isError"].as_bool().unwrap_or(false), r["content"][0]["text"].as_str().unwrap().to_string())
}
fn ok(name: &str, owner: &str, a: Value) -> Value {
    let (e, t) = call(name, owner, a);
    assert!(!e, "{name}: {t}");
    serde_json::from_str(&t).unwrap()
}

#[test]
fn tools_make_resolve_score_round_trip_in_a_temp_root() {
    crate::safety::test_support::init();
    let owner = "pt-predict";
    for i in 0..40 {
        let (c, a) = (["am", "pm"][i % 2], if i % 2 == 0 { "tea" } else { "coffee" });
        ok("self_event_add", owner, json!({"text": format!("drank {a} {i}"), "occurred_at": format!("2024-05-{:02}T{:02}:00:00Z", 1 + i / 24, i % 24), "kind": "action", "tags": [format!("ctx:{c}"), format!("act:{a}")]}));
    }
    // a stricter minimum than the data allows: refused and logged
    let strict = ok("predict_make", owner, json!({"context": "am", "made_at": "2024-06-01", "min_n": 500, "question_id": "strict"}));
    assert_eq!(strict["refused_insufficient_data"].as_array().unwrap().len(), 4);
    assert_eq!(strict["written"].as_array().unwrap().len(), 1);
    // bad input
    assert!(call("predict_make", owner, json!({})).0);
    assert!(call("predict_make", owner, json!({"context": "am", "predictors": ["oracle"], "made_at": "2024-06-01"})).0);
    assert!(call("predict_make", owner, json!({"context": "am", "made_at": "2024-05-01"})).0, "backdated");
    assert!(call("predict_resolve", owner, json!({"actual": "tea", "resolved_at": "2024-06-02"})).0);
    // the real thing
    let m = ok("predict_make", owner, json!({"context": "am", "made_at": "2024-06-01", "question_id": "q1", "predictors": ["frequency", "context-count"]}));
    assert_eq!(m["written"].as_array().unwrap().len(), 2);
    assert_eq!(m["cutoff_version"].as_u64().unwrap() + 1, m["version"].as_u64().unwrap());
    assert!(call("predict_resolve", owner, json!({"question_id": "q1", "actual": "tea", "resolved_at": "2024-05-31"})).0, "outcome before forecast");
    let r = ok("predict_resolve", owner, json!({"question_id": "q1", "actual": "tea", "resolved_at": "2024-06-02"}));
    assert_eq!(r["resolved"].as_array().unwrap().len(), 2);
    let s = ok("predict_score", owner, json!({}));
    let models = s["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["scored"], 1);
    assert!(s["paired_comparisons"].as_array().unwrap().iter().all(|c| c["verdict"] == "insufficient_data"));
    // read-only: scoring writes nothing
    let v = call("self_versions", owner, json!({})).1;
    ok("predict_score", owner, json!({}));
    assert_eq!(v, call("self_versions", owner, json!({})).1);
    assert!(call("self_verify", owner, json!({})).1.starts_with("OK"));
}

#[test]
fn tools_are_registered_and_not_chat_tools() {
    let names: Vec<String> = crate::tool_list().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in crate::predict_tools::TOOLS {
        assert!(names.contains(&n.to_string()), "{n} not registered");
        assert!(crate::log_tools::is_log_tool(n));
        // authorize(): default deny for the chat thinker; not in CHAT_TOOLS and not grantable in the policy
        assert!(!crate::authorize::CHAT_TOOLS.iter().any(|(t, _)| *t == n));
        assert!(crate::authorize::parse_policy(&format!("[L3]\n{n}\n")).is_err(), "{n} must not be grantable to the chat");
    }
}

// ── measured floors (synthetic data only; set after measuring, see docs/measured.md) ──

fn bt_seed(s: Scenario, seed: u64) -> backtest::Backtest {
    rolling_origin(&baselines::all(&Config::default()), &scenarios::generate(s, seed, 400), 20)
}

fn verdict(bt: &backtest::Backtest, x: &str, base: &str, seed: u64) -> Verdict {
    let (pp, bp) = bt.common(bt.index(x).unwrap(), bt.index(base).unwrap());
    score::paired(&pp, &bp, seed).verdict
}

#[test]
fn measured_context_count_beats_frequency_where_context_matters() {
    for seed in 1..=8 {
        let bt = bt_seed(Scenario::Context, seed);
        assert_eq!(verdict(&bt, "context-count", "frequency", seed), Verdict::Better, "seed {seed}");
        assert_eq!(verdict(&bt, "context-count", "uniform", seed), Verdict::Better, "seed {seed}");
        let s = score::summarize(&bt.answered(bt.index("context-count").unwrap()));
        assert!(s.accuracy > 0.70 && s.brier < 0.5, "seed {seed}: {s:?}");
    }
}

#[test]
fn measured_no_predictor_wins_on_pure_noise() {
    for seed in 1..=8 {
        let bt = bt_seed(Scenario::Noise, seed);
        for x in ["frequency", "last-choice", "recency", "context-count"] {
            for base in ["uniform", "frequency"] {
                if x != base {
                    assert_ne!(verdict(&bt, x, base, seed), Verdict::Better, "{x} claimed a win over {base} on noise, seed {seed}");
                }
            }
        }
        // the frequency forecasts stay close to the diagonal
        let s = score::summarize(&bt.answered(bt.index("frequency").unwrap()));
        assert!(s.ece < 0.12, "seed {seed}: ece {}", s.ece);
    }
}

#[test]
fn measured_recency_beats_frequency_under_drift() {
    for seed in 1..=8 {
        let bt = bt_seed(Scenario::Drift, seed);
        assert_eq!(verdict(&bt, "recency", "frequency", seed), Verdict::Better, "seed {seed}");
        assert_eq!(verdict(&bt, "recency", "uniform", seed), Verdict::Better, "seed {seed}");
    }
}
