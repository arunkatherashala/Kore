//! Tests for the learning module, evolution loop, ledger and tools.

use crate::learn::derive::{self, Kinds};
use crate::learn::{dates, evolve, habits, ledger, stances, visible_before, Ev, Params};
use crate::synth;
use crate::versioned::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

fn fresh(dir: &Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(dir, "t", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn filled(seed: u64, noise: f64) -> (tempfile::TempDir, VersionedLog, synth::Truth, Vec<NewEvent>) {
    let dir = tempfile::tempdir().unwrap();
    let mut log = fresh(dir.path());
    let (events, truth) = synth::generate(&synth::default_config(seed, noise));
    log.append_events(&events).unwrap();
    (dir, log, truth, events)
}

fn ev(day: i64, tags: &[&str], text: &str) -> Ev {
    Ev { id: format!("{day}{text}{tags:?}"), ts: dates::midnight_of(day), day, kind: "note".into(), source: "t".into(), text: text.into(), tags: tags.iter().map(|s| s.to_string()).collect() }
}

// ── generator ───────────────────────────────────────────────────────────────

#[test]
fn synth_is_deterministic_and_seed_sensitive() {
    let (a, ta) = synth::generate(&synth::default_config(7, 0.2));
    let (b, tb) = synth::generate(&synth::default_config(7, 0.2));
    let (c, _) = synth::generate(&synth::default_config(8, 0.2));
    let key = |v: &[NewEvent]| v.iter().map(|e| format!("{}|{}|{}", e.occurred_at, e.text, e.tags.join(","))).collect::<Vec<_>>();
    assert_eq!(key(&a), key(&b));
    assert_ne!(key(&a), key(&c));
    assert_eq!(ta.stances.len(), tb.stances.len());
    assert_eq!(ta.changes.len(), 3);
    assert_eq!(ta.habits.len(), 3);
    // every event is valid for the log
    for e in &a {
        VersionedLog::check_new_event(e).unwrap();
    }
}

// ── detectors ───────────────────────────────────────────────────────────────

#[test]
fn habits_found_and_small_n_refused() {
    let (events, truth) = synth::generate(&synth::default_config(1, 0.0));
    let evs = synth::to_evs(&events);
    let (hs, insufficient) = habits::detect(&evs, &Params::default());
    let got: BTreeSet<(String, String)> = hs.iter().map(|h| (h.context.clone(), h.action.clone())).collect();
    for (c, a, _) in &truth.habits {
        assert!(got.contains(&(c.clone(), a.clone())), "missed {c}/{a}");
    }
    for r in &truth.rare_contexts {
        assert!(insufficient.iter().any(|i| &i.context == r), "rare context {r} not reported as insufficient");
        assert!(!hs.iter().any(|h| &h.context == r));
    }
    // with min_n lowered, a 4-of-4 context would pass: the refusal is what stops it
    let p1 = Params { habit_min_n: 1, ..Params::default() };
    assert!(habits::detect(&evs, &p1).0.iter().any(|h| h.context == "retreat"));
}

#[test]
fn change_scan_on_known_series() {
    let mut s = vec![0u32; 120];
    for (i, c) in s.iter_mut().enumerate() {
        *c = if i < 60 { (i % 2) as u32 } else { 2 + (i % 3) as u32 };
    }
    let ch = crate::learn::changes::detect_series(&s, &Params::default(), 0);
    assert_eq!(ch.len(), 1);
    assert_eq!(ch[0].offset, 60);
    let flat: Vec<u32> = (0..120).map(|i| (i % 3) as u32).collect();
    assert!(crate::learn::changes::detect_series(&flat, &Params::default(), 0).is_empty());
}

#[test]
fn stance_cues_match_and_near_misses_do_not() {
    let yes = [
        ("I now prefer tea for mornings.", "mornings"),
        ("Honestly, I now think remote work is overrated!", "remote work"),
        ("I changed my mind about meetings: fewer is better.", "meetings"),
        ("I\u{2019}ve changed my mind about diet", "diet"),
        ("From now on I will write every evening.", "write every evening"),
    ];
    for (t, topic) in yes {
        let c = stances::extract(&[ev(1, &[], t)]);
        assert_eq!(c.len(), 1, "{t}");
        assert_eq!(c[0].topic, topic, "{t}");
    }
    for t in [
        "They now prefer coffee for work.",
        "I prefer tea for mornings.",
        "If I now prefer tea for mornings that would be odd.",
        "She now thinks the plan is bad.",
        "I changed my plans about lunch.",
        "I now prefer",
        "nobody knows",
    ] {
        assert!(stances::extract(&[ev(1, &[], t)]).is_empty(), "matched {t}");
    }
}

// ── writing through the library ─────────────────────────────────────────────

#[test]
fn learn_is_idempotent_and_every_row_cites_events() {
    let (_d, mut log, _t, _e) = filled(2, 0.2);
    let p = Params::default();
    let r1 = derive::learn(&mut log, &p, &Kinds::ALL, false).unwrap();
    assert!(r1.outcomes.iter().any(|o| o.is_new));
    let snap = log.open_latest();
    let revs: Vec<RevisionRow> = snap.rows();
    assert!(!revs.is_empty());
    let causes: Vec<CauseRow> = snap.rows();
    for r in &revs {
        assert!(r.author.starts_with("rule:"), "{}", r.author);
        assert!(causes.iter().any(|c| c.rev_id == r.rev_id), "{} cites nothing", r.key);
    }
    let (v, hash) = (log.latest_version(), log.snapshot_hash(log.latest_version()).unwrap());
    let r2 = derive::learn(&mut log, &p, &Kinds::ALL, false).unwrap();
    assert!(r2.outcomes.iter().all(|o| !o.is_new));
    assert_eq!(log.latest_version(), v);
    assert_eq!(log.snapshot_hash(v).unwrap(), hash);
    // and the ids are the same ones
    let ids1: BTreeSet<_> = r1.outcomes.iter().filter_map(|o| o.rev_id.clone()).collect();
    let ids2: BTreeSet<_> = r2.outcomes.iter().filter_map(|o| o.rev_id.clone()).collect();
    assert_eq!(ids1, ids2);
    log.verify().unwrap();
}

#[test]
fn dry_run_writes_nothing() {
    let (_d, mut log, _t, _e) = filled(2, 0.0);
    let v = log.latest_version();
    let r = derive::learn(&mut log, &Params::default(), &Kinds::ALL, true).unwrap();
    assert!(r.outcomes.iter().all(|o| o.is_new && o.rev_id.is_none()));
    assert_eq!(log.latest_version(), v);
}

#[test]
fn learn_is_incremental() {
    let (_d, mut log, _t, _e) = filled(3, 0.0);
    let k = Kinds { habits: true, changes: false, stances: false };
    derive::learn(&mut log, &Params::default(), &k, false).unwrap();
    // one more morning/run occasion inside the existing date range
    let day = dates::days_from_civil(2024, 1, 1) + 100;
    let e = NewEvent { source: "synth".into(), occurred_at: format!("{}T23:00:00Z", dates::date_of(day)), kind: "action".into(), text: "context morning: did run (extra)".into(), tags: vec!["ctx:morning".into(), "act:run".into()], redacted: false };
    log.append_events(&[e]).unwrap();
    let r = derive::learn(&mut log, &Params::default(), &k, false).unwrap();
    let new: Vec<&str> = r.outcomes.iter().filter(|o| o.is_new).map(|o| r.derivation.items[o.item].key.as_str()).collect();
    assert_eq!(new, vec!["ctx:morning|act:run"], "only the habit with new evidence changes");
    // the new revision supersedes the old one
    let revs: Vec<RevisionRow> = log.open_latest().rows();
    let heads: Vec<&RevisionRow> = revs.iter().filter(|r| r.key == "ctx:morning|act:run").collect();
    assert_eq!(heads.len(), 2);
    assert_eq!(heads[1].supersedes.as_deref(), Some(heads[0].rev_id.as_str()));
}

// ── provenance ablation ─────────────────────────────────────────────────────

#[test]
fn ablation_removing_cited_events_removes_the_revision_and_irrelevant_events_change_nothing() {
    let (_d, mut log, _t, events) = filled(4, 0.0);
    let p = Params::default();
    let r = derive::learn(&mut log, &p, &Kinds::ALL, false).unwrap();
    let find = |res: &derive::LearnResult, key: &str| res.derivation.items.iter().position(|i| i.key == key);
    let hk = "ctx:morning|act:run";
    let tk = "topic:gardening";
    let hi = find(&r, hk).expect("morning/run derived");
    let ti = find(&r, tk).expect("gardening change derived");
    let sig_h = r.derivation.items[hi].signature();
    let sig_t = r.derivation.items[ti].signature();
    // the cited events are exactly what the revision table says
    let cited_h: BTreeSet<String> = r.derivation.items[hi].caused_by.iter().cloned().collect();
    let cited_t: BTreeSet<String> = r.derivation.items[ti].caused_by.iter().cloned().collect();
    assert!(cited_h.len() > 50 && cited_t.len() > 20);

    // second log without the cited events
    let without = |ids: &BTreeSet<String>| -> Vec<NewEvent> {
        events.iter().filter(|e| !ids.contains(&event_id(&e.source, &normalize_ts(&e.occurred_at).unwrap(), &e.text))).cloned().collect()
    };
    for (ids, key) in [(&cited_h, hk), (&cited_t, tk)] {
        let dir = tempfile::tempdir().unwrap();
        let mut l2 = fresh(dir.path());
        l2.append_events(&without(ids)).unwrap();
        let r2 = derive::learn(&mut l2, &p, &Kinds::ALL, false).unwrap();
        assert!(find(&r2, key).is_none(), "{key} survived removal of its cited events");
        // the other derived rows are untouched by removing unrelated evidence
        let other = if key == hk { tk } else { hk };
        assert!(find(&r2, other).is_some());
    }

    // irrelevant events: other context, other topic, chatter. Same signatures, nothing new for these rows.
    let extra: Vec<NewEvent> = (0..40)
        .map(|i| NewEvent {
            source: "synth".into(),
            occurred_at: format!("2024-03-{:02}T22:{:02}:00Z", 1 + i % 28, i),
            kind: if i % 2 == 0 { "action" } else { "note" }.into(),
            text: format!("irrelevant item {i}"),
            tags: if i % 3 == 0 { vec!["ctx:zzz".into(), "act:other".into()] } else if i % 3 == 1 { vec!["topic:zzz".into()] } else { vec![] },
            redacted: false,
        })
        .collect();
    log.append_events(&extra).unwrap();
    let r3 = derive::learn(&mut log, &p, &Kinds::ALL, false).unwrap();
    let h3 = find(&r3, hk).unwrap();
    let t3 = find(&r3, tk).unwrap();
    assert_eq!(r3.derivation.items[h3].signature(), sig_h);
    assert_eq!(r3.derivation.items[t3].signature(), sig_t);
    assert!(!r3.outcomes.iter().find(|o| o.item == h3).unwrap().is_new);
    assert!(!r3.outcomes.iter().find(|o| o.item == t3).unwrap().is_new);
}

#[test]
fn forget_of_cited_events_marks_revision_unsupported_and_relearn_drops_it() {
    let (_d, mut log, _t, _e) = filled(5, 0.0);
    let p = Params::default();
    let r = derive::learn(&mut log, &p, &Kinds { habits: true, changes: false, stances: false }, false).unwrap();
    let i = r.derivation.items.iter().position(|i| i.key == "ctx:evening|act:read").unwrap();
    let ids = r.derivation.items[i].caused_by.clone();
    log.forget_events(&ids).unwrap();
    let revs: Vec<RevisionRow> = log.open_latest().rows();
    let rev = revs.iter().find(|r| r.key == "ctx:evening|act:read").unwrap();
    let causes: Vec<CauseRow> = log.open_latest().rows();
    assert!(!causes.iter().any(|c| c.rev_id == rev.rev_id), "forgotten events are no longer cited");
    let r2 = derive::learn(&mut log, &p, &Kinds { habits: true, changes: false, stances: false }, true).unwrap();
    assert!(!r2.derivation.items.iter().any(|i| i.key == "ctx:evening|act:read"));
}

// ── evolution loop ──────────────────────────────────────────────────────────

fn evs(seed: u64, noise: f64) -> Vec<Ev> {
    synth::to_evs(&synth::generate(&synth::default_config(seed, noise)).0)
}

#[test]
fn evolve_rejects_a_worse_rule() {
    let e = evs(3, 0.2);
    let good = Params::default();
    let bad = Params { habit_lb: 0.95, ..Params::default() };
    let d = evolve::evolve(&e, &good, &[bad], &evolve::EvolveCfg::default());
    assert_eq!(d.verdict, evolve::Verdict::Rejected, "{}", d.reason);
    assert!(d.select_candidate.f1() < d.select_incumbent.f1());
}

#[test]
fn evolve_promotes_a_better_rule() {
    let e = evs(3, 0.2);
    let bad = Params { habit_lb: 0.95, ..Params::default() };
    let d = evolve::evolve(&e, &bad, &[Params::default()], &evolve::EvolveCfg::default());
    assert_eq!(d.verdict, evolve::Verdict::Promoted, "{}", d.reason);
    let cfg = evolve::EvolveCfg::default();
    assert!(d.eval_candidate.f1() >= d.eval_incumbent.f1() + cfg.margin);
    assert!(d.eval_candidate.f1() >= d.eval_baseline.f1() + cfg.margin);
}

#[test]
fn evolve_needs_to_beat_the_baseline_too() {
    // A candidate that beats a useless incumbent but not a trivial baseline is rejected.
    let e = evs(3, 0.2);
    let none = Params { habit_lb: 0.95, ..Params::default() };
    // lift test off: this candidate stands for the old "every minority action" behaviour; with the lift test on
    // it filters minority actions out and may legitimately beat the baseline
    let weak = Params { habit_min_n: 1, habit_lb: 0.0, habit_lift: -1.0, ..Params::default() };
    let d = evolve::evolve(&e, &none, &[weak], &evolve::EvolveCfg::default());
    // weak behaves like the modal baseline plus every minority action, so it cannot win by a margin
    assert_ne!(d.verdict, evolve::Verdict::Promoted, "{}", d.reason);
}

#[test]
fn evolve_is_deterministic_and_insufficient_history_is_not_recorded() {
    let e = evs(6, 0.2);
    let inc = Params::default();
    let a = evolve::evolve(&e, &inc, &evolve::neighbours(&inc), &evolve::EvolveCfg::default());
    let b = evolve::evolve(&e, &inc, &evolve::neighbours(&inc), &evolve::EvolveCfg::default());
    assert_eq!(a.to_json(), b.to_json());
    let short: Vec<Ev> = e.iter().filter(|x| x.day < e[0].day + 40).cloned().collect();
    let d = evolve::evolve(&short, &inc, &evolve::neighbours(&inc), &evolve::EvolveCfg::default());
    assert_eq!(d.verdict, evolve::Verdict::Insufficient);
    let dir = tempfile::tempdir().unwrap();
    let mut log = fresh(dir.path());
    assert!(evolve::record(&mut log, &d, &short).unwrap().is_none());
}

#[test]
fn evolve_cannot_see_events_after_the_cut() {
    let cfg = evolve::EvolveCfg::default();
    let a = evs(9, 0.2);
    let (cut, _, _) = evolve::cut_and_origins(&a, &cfg).unwrap();
    // B: identical before the cut, every action at or after the cut replaced by garbage
    let b: Vec<Ev> = a
        .iter()
        .map(|e| {
            let mut e = e.clone();
            if e.day >= cut {
                e.tags = e.tags.iter().map(|t| if t.starts_with("act:") { "act:garbage".to_string() } else { t.clone() }).collect();
            }
            e
        })
        .collect();
    assert_ne!(a, b);
    let p = Params::default();
    for o in (cut - 40)..=cut {
        assert_eq!(evolve::predict_at(&a, &p, o, None), evolve::predict_at(&b, &p, o, None), "origin {o} saw the future");
    }
    assert!(visible_before(&a, cut).iter().all(|e| e.day < cut));
    let inc = Params { habit_min_n: 20, ..Params::default() };
    let cands = evolve::neighbours(&inc);
    let da = evolve::evolve(&a, &inc, &cands, &cfg);
    let db = evolve::evolve(&b, &inc, &cands, &cfg);
    // selection used only the past: same winner, same selection scores ...
    assert_eq!(da.candidate, db.candidate);
    assert_eq!(da.select_candidate, db.select_candidate);
    assert_eq!(da.select_incumbent, db.select_incumbent);
    // ... while the evaluation after the cut does react to the changed future
    assert_ne!(da.eval_candidate, db.eval_candidate);
}

fn files_outside_log(root: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, base: &Path, out: &mut BTreeSet<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().display().to_string().replace('\\', "/");
            if p.is_dir() {
                if rel == "t/log" {
                    continue;
                }
                walk(&p, base, out);
            } else {
                out.insert(rel);
            }
        }
    }
    let mut s = BTreeSet::new();
    walk(root, root, &mut s);
    s
}

#[test]
fn evolve_run_records_decisions_and_writes_only_inside_the_log() {
    let (dir, mut log, _t, _e) = filled(3, 0.2);
    let before = files_outside_log(dir.path());
    let v0 = log.latest_version();
    let (d_dry, rec) = evolve::run(&mut log, &evolve::EvolveCfg::default(), true).unwrap();
    assert!(rec.is_none());
    assert_eq!(log.latest_version(), v0, "dry run writes nothing");
    let (d, rec) = evolve::run(&mut log, &evolve::EvolveCfg::default(), false).unwrap();
    assert_eq!(d.to_json(), d_dry.to_json());
    let rec = rec.expect("decision recorded");
    assert_eq!(log.latest_version(), v0 + 1);
    let snap = log.open_latest();
    let revs: Vec<RevisionRow> = snap.rows();
    let r = revs.iter().find(|r| r.rev_id == rec.rev_id).unwrap();
    assert_eq!(r.entity_type, "rule");
    assert_eq!(r.author, "rule:evolve-v1");
    let evts: Vec<EventRow> = snap.rows();
    let e = evts.iter().find(|e| e.event_id == rec.event_id).unwrap();
    assert!(e.text.contains("\"evaluation\"") && e.tags.contains("evolve"), "scores are logged");
    // same decision again: nothing new unless the rules changed
    let (_, rec2) = evolve::run(&mut log, &evolve::EvolveCfg::default(), false).unwrap();
    let again = rec2.unwrap();
    if d.verdict != evolve::Verdict::Promoted {
        assert_eq!(again.rev_id, rec.rev_id);
        assert_eq!(log.latest_version(), v0 + 1);
    }
    log.verify().unwrap();
    assert_eq!(files_outside_log(dir.path()), before);
    assert!(!files_outside_log(dir.path()).iter().any(|f| f.ends_with(".rs")));
    // learner events are ignored by learning
    assert!(crate::learn::learnable(&log.open_latest().rows()).iter().all(|e| !e.has_tag("evolve")));
}

#[test]
fn promoted_rules_are_used_and_rollback_is_time_travel() {
    let (_d, mut log, _t, _e) = filled(3, 0.2);
    let bad = Params { habit_lb: 0.95, ..Params::default() };
    let events = crate::learn::learnable(&log.open_latest().rows());
    let d = evolve::evolve(&events, &Params::default(), &[bad.clone()], &evolve::EvolveCfg::default());
    assert_eq!(d.verdict, evolve::Verdict::Rejected);
    assert!(evolve::record(&mut log, &d, &events).unwrap().is_some());
    assert_eq!(derive::current_params(&log).0, Params::default(), "a rejection does not change the rules");
    let d2 = evolve::evolve(&events, &bad, &[Params::default()], &evolve::EvolveCfg::default());
    assert_eq!(d2.verdict, evolve::Verdict::Promoted);
    evolve::record(&mut log, &d2, &events).unwrap();
    let v_promoted = log.latest_version();
    assert_eq!(derive::current_params(&log).0, Params::default());
    // a worse set promoted by hand through the library, then rolled back
    let d3 = evolve::Decision { verdict: evolve::Verdict::Promoted, candidate: Some(bad.clone()), reason: "test".into(), ..d2.clone() };
    evolve::record(&mut log, &d3, &events).unwrap();
    assert_eq!(derive::current_params(&log).0, bad);
    // earlier rule set readable as of an older version
    let old = log.open_at(AsOf::Version(v_promoted)).unwrap();
    assert_eq!(derive::current_params_in(&old.rows::<RevisionRow>()).0, Params::default());
    evolve::rollback_rules(&mut log, v_promoted).unwrap();
    assert_eq!(derive::current_params(&log).0, Params::default());
    log.verify().unwrap();
}

// ── ledger ──────────────────────────────────────────────────────────────────

#[test]
fn ledger_snapshot_is_idempotent_before_outcome_and_uses_only_the_past() {
    let (_d, mut log, _t, _e) = filled(2, 0.0);
    let p = Params::default();
    let r1 = ledger::snapshot(&mut log, &p, Some("2024-04-01"), 7, false).unwrap();
    assert_eq!(r1.written.len(), 3, "three habits have enough data");
    assert_eq!(r1.cutoff.get(..10), Some("2024-03-31"));
    assert!(r1.ignored_future_events > 0);
    let preds: Vec<PredictionRow> = log.open_latest().rows();
    for pr in &preds {
        assert!(pr.made_at < "2024-04-01".to_string());
        assert_eq!(pr.resolves_by.as_deref(), Some("2024-04-08T00:00:00Z"));
        assert_eq!(pr.model_id, "rule:habit-frequency");
        let probs: Vec<f64> = serde_json::from_str(&pr.probs).unwrap();
        assert!((probs.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }
    assert!(log.open_latest().rows::<OutcomeRow>().is_empty(), "nothing is scored");
    let v = log.latest_version();
    let r2 = ledger::snapshot(&mut log, &p, Some("2024-04-01"), 7, false).unwrap();
    assert!(r2.written.is_empty());
    assert_eq!(r2.already_present, 3);
    assert_eq!(log.latest_version(), v);
    // a later window gets its own rows
    let r3 = ledger::snapshot(&mut log, &p, Some("2024-04-08"), 7, false).unwrap();
    assert_eq!(r3.written.len(), 3);
    // outcome can be attached later, once
    let pid = preds[0].pred_id.clone();
    log.record_outcome(NewOutcome { pred_id: pid.clone(), resolved_at: "2024-04-09".into(), actual: "yes".into(), outcome_event_id: None }).unwrap();
    assert!(log.record_outcome(NewOutcome { pred_id: pid, resolved_at: "2024-04-10".into(), actual: "no".into(), outcome_event_id: None }).is_err());
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
fn tools_learn_explain_ledger_and_refuse_rule_entity() {
    crate::safety::test_support::init();
    let owner = "lt-learn";
    let (events, _) = synth::generate(&synth::default_config(2, 0.0));
    crate::log_tools::open(owner).unwrap_or_else(|e| panic!("{e}")).append_events(&events).unwrap();
    let dry = ok("self_learn", owner, json!({"dry_run": true}));
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["version_before"], dry["version_after"]);
    assert!(dry["counts"]["habits"]["would_add"].as_u64().unwrap() >= 3);
    assert!(dry["insufficient_data"]["habit_contexts"].as_array().unwrap().iter().any(|c| c["context"] == "retreat"));
    let real = ok("self_learn", owner, json!({}));
    assert!(real["counts"]["habits"]["added"].as_u64().unwrap() >= 3);
    assert!(real["counts"]["stances"]["added"].as_u64().unwrap() == 6);
    let again = ok("self_learn", owner, json!({}));
    assert_eq!(again["counts"]["habits"]["added"], 0);
    assert_eq!(again["version_before"], again["version_after"]);
    let only = ok("self_learn", owner, json!({"kinds": ["stances"], "dry_run": true}));
    assert_eq!(only["counts"]["habits"]["derived"], 0);
    let (e, t) = call("self_learn", owner, json!({"kinds": ["nonsense"]}));
    assert!(e && t.contains("unknown kind"));
    let strict = ok("self_learn", owner, json!({"kinds": ["habits"], "min_n": 100000, "dry_run": true}));
    assert_eq!(strict["counts"]["habits"]["derived"], 0);
    assert!(!strict["insufficient_data"]["habit_contexts"].as_array().unwrap().is_empty());

    // explain
    let h = real["derived"].as_array().unwrap().iter().find(|d| d["key"] == "ctx:morning|act:run").unwrap();
    let id = h["rev_id"].as_str().unwrap();
    let ex = ok("self_explain", owner, json!({"rev_id": &id[..12]}));
    assert_eq!(ex["author"], "rule:habits-v1");
    assert_eq!(ex["unsupported"], false);
    assert!(ex["cited_event_count"].as_u64().unwrap() > 50);
    assert!(ex["cited_events"][0]["text"].as_str().unwrap().contains("morning"));
    assert!(call("self_explain", owner, json!({"rev_id": "abc"})).0);
    assert!(call("self_explain", owner, json!({"rev_id": "ffffffffffff"})).0);

    // tools cannot write rule revisions or rule authors
    let (e, t) = call("self_belief_revise", owner, json!({"key": "x", "stance": "y", "confidence": 0.5, "caused_by": [], "entity_type": "rule"}));
    assert!(e && t.contains("only by the learning"), "{t}");

    // evolve dry run then real
    let dry = ok("self_evolve_rules", owner, json!({"dry_run": true}));
    assert!(dry["recorded"].is_null());
    let real = ok("self_evolve_rules", owner, json!({}));
    assert!(!real["recorded"].is_null());
    assert_eq!(dry["verdict"], real["verdict"]);

    // ledger
    let s1 = ok("self_ledger_snapshot", owner, json!({}));
    let n = s1["written"].as_array().unwrap().len();
    assert!(n >= 3);
    let s2 = ok("self_ledger_snapshot", owner, json!({}));
    assert_eq!(s2["written"].as_array().unwrap().len(), 0);
    assert_eq!(s2["already_present"], n);
    let pid = s1["written"][0]["pred_id"].as_str().unwrap();
    let (e, _) = call("self_ledger_outcome", owner, json!({"pred_id": pid, "actual": "maybe", "resolved_at": "2024-07-01"}));
    assert!(e);
    ok("self_ledger_outcome", owner, json!({"pred_id": pid, "actual": "yes", "resolved_at": "2024-07-01"}));
    assert!(call("self_ledger_outcome", owner, json!({"pred_id": pid, "actual": "no", "resolved_at": "2024-07-02"})).0);
    assert!(call("self_ledger_outcome", owner, json!({"pred_id": "nope", "actual": "no", "resolved_at": "2024-07-02"})).0);
    assert!(call("self_verify", owner, json!({})).1.starts_with("OK"));
}

#[test]
fn no_tool_can_write_code() {
    let names: Vec<String> = crate::tool_list().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in &names {
        for bad in ["write_code", "write_source", "codegen", "generate_code", "self_modify", "self_patch", "edit_source", "apply_patch", "self_rewrite", "compile", "self_exec", "shell"] {
            assert!(!n.contains(bad), "tool {n} looks like a code-writing tool");
        }
    }
    for n in ["self_learn", "self_explain", "self_evolve_rules", "self_ledger_snapshot", "self_ledger_outcome", "self_tag_rules_check"] {
        assert!(names.contains(&n.to_string()), "{n} not registered");
    }
    // the evolution loop's output surface: only log revisions (entity_type rule)
    let src = include_str!("learn/evolve.rs");
    assert!(!src.contains("std::fs"), "the evolution loop must not touch the file system");
}

// ── measured thresholds (set after measuring; synthetic data only) ───────────

#[test]
fn measured_detector_quality_on_synthetic_data_stays_above_floor() {
    let rows = crate::measure::sweep(&crate::measure::SEEDS, &crate::measure::NOISE, &Params::default());
    for r in &rows {
        let n = r.noise;
        let min = |f: fn(&crate::measure::RunMetrics) -> f64| r.col(|m| Some(f(m))).unwrap().min;
        let med = |f: fn(&crate::measure::RunMetrics) -> f64| r.col(|m| Some(f(m))).unwrap().median;
        let max = |f: fn(&crate::measure::RunMetrics) -> f64| r.col(|m| Some(f(m))).unwrap().max;
        assert!(min(|m| m.habit_precision) >= 0.95, "habit precision at noise {n}");
        assert!(min(|m| m.stance_precision) >= 0.95 && min(|m| m.stance_recall) >= 0.95 && min(|m| m.stance_key_accuracy) >= 0.95, "stances at noise {n}");
        assert!(min(|m| m.minn_refused) >= 0.95, "min-n refusal at noise {n}");
        assert!(min(|m| m.minn1_would_derive) >= 0.4, "min-n demonstration at noise {n}");
        assert!(min(|m| m.change_precision) >= 0.45, "change precision at noise {n}");
        assert!(min(|m| m.change_recall) >= 0.6, "change recall at noise {n}");
        assert!(max(|m| m.change_fp_rate) <= 0.25, "change FP rate at noise {n}");
        assert!(max(|m| m.change_abs_error) <= 5.0, "change error at noise {n}");
        if n == 0.0 {
            assert!(min(|m| m.habit_recall) >= 0.95);
        } else if n == 0.2 {
            assert!(min(|m| m.habit_recall) >= 0.6);
        } else {
            assert!(med(|m| m.habit_recall) >= 0.3, "habit recall degrades with noise but not below the measured median");
        }
    }
}

// ── free-text journals: keyword rules (tag_rules.txt) ───────────────────────

use crate::learn::TagRules;

const DEMO_RULES: &str = "action:gym = gym, workout\nctx:morning = morning\ntopic:rust = rust, compiler\ntopic:cooking = cooking, recipes\n";

fn text_event(day: i64, text: &str) -> NewEvent {
    NewEvent { source: "journal".into(), occurred_at: dates::midnight_of(day), kind: "note".into(), text: text.into(), tags: vec![], redacted: false }
}

/// 2026-07-01 (a Wednesday) onward: weekday gym ~85%, never on weekends, topic shift at day 45.
fn free_journal(days: i64) -> Vec<NewEvent> {
    let start = dates::days_from_civil(2026, 7, 1);
    let mut rng = synth::Rng::new(7);
    let mut out = Vec::new();
    for i in 0..days {
        let d = start + i;
        let mut lines: Vec<String> = Vec::new();
        let weekday = crate::learn::tagrules::weekday_index(d) < 5;
        if weekday {
            if rng.f64() < 0.85 {
                lines.push("Morning: went to the gym before work.".into());
            } else if rng.f64() < 0.5 {
                lines.push("Morning: did not go to the gym, skipped it.".into());
            }
        }
        if rng.f64() < 0.7 {
            lines.push(if i < 45 { "Evening: spent time on the rust compiler.".into() } else { "Evening: tried new cooking recipes.".into() });
        }
        if lines.is_empty() {
            lines.push("Quiet day.".into());
        }
        out.push(text_event(d, &format!("# day {i}\n{}", lines.join("\n"))));
    }
    out
}

fn derived_keys(r: &derive::LearnResult) -> BTreeSet<String> {
    r.derivation.items.iter().map(|i| format!("{}|{}|{}", i.key, i.stance, i.rationale)).collect()
}

#[test]
fn without_rules_behaviour_is_unchanged() {
    let (_d, log, _t, _e) = filled(2, 0.2);
    let evs = crate::learn::learnable(&log.open_latest().rows());
    let p = Params::default();
    let base = derive::derive(&evs, &p, &Kinds::ALL);
    let sig = |d: &derive::Derivation| d.items.iter().map(|i| i.signature()).collect::<Vec<_>>();
    assert_eq!(sig(&base), sig(&derive::derive_with(&evs, &p, &Kinds::ALL, None)));
    // an empty rule set, or rules with no action rule and nothing matching, change nothing either
    assert_eq!(sig(&base), sig(&derive::derive_with(&evs, &p, &Kinds::ALL, Some(&TagRules::default()))));
    let inert = TagRules::parse("topic:zzzzqq = zzzzqq\nctx:qqzz = qqzz\n");
    assert_eq!(sig(&base), sig(&derive::derive_with(&evs, &p, &Kinds::ALL, Some(&inert))));
    // tool level: no file, no tag_rules key in the reply, nothing about rules written
    crate::safety::test_support::init();
    let owner = "lt-norules";
    let (events, _) = synth::generate(&synth::default_config(2, 0.0));
    crate::log_tools::open(owner).unwrap_or_else(|e| panic!("{e}")).append_events(&events).unwrap();
    let r = ok("self_learn", owner, json!({"dry_run": true}));
    assert!(r.get("tag_rules").is_none(), "{r}");
    let c = ok("self_tag_rules_check", owner, json!({}));
    assert_eq!(c["file_present"], false);
}

#[test]
fn free_text_weekday_gym_habit_and_topic_change_are_derived() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = fresh(dir.path());
    let events = free_journal(90);
    log.append_events(&events).unwrap();
    let before: Vec<EventRow> = log.open_latest().rows();
    let p = Params::default();
    // without rules: nothing (the free text has no tags)
    let none = derive::learn_with(&mut log, &p, &Kinds { habits: true, changes: true, stances: false }, true, None).unwrap();
    assert!(none.derivation.items.is_empty());
    let rules = TagRules::parse(DEMO_RULES);
    assert!(rules.warnings.is_empty(), "{:?}", rules.warnings);
    let r1 = derive::learn_with(&mut log, &p, &Kinds::ALL, false, Some(&rules)).unwrap();
    let items = &r1.derivation.items;
    let h = items.iter().find(|i| i.key == "ctx:weekday|act:gym").expect("weekday gym habit");
    assert_eq!(h.author, "rule:habits-v1");
    assert!(h.rationale.contains(&rules.fingerprint));
    assert!(items.iter().any(|i| i.key == "ctx:morning|act:gym"), "keyword context habit");
    assert!(!items.iter().any(|i| i.key == "ctx:weekend|act:gym"), "no gym on weekends");
    // cited events are exactly the events whose text matches gym without negation
    let snap = log.open_latest();
    let rows: Vec<EventRow> = snap.rows();
    let causes: Vec<CauseRow> = snap.rows();
    let revs: Vec<RevisionRow> = snap.rows();
    let rev = revs.iter().find(|r| r.key == "ctx:weekday|act:gym").unwrap();
    let cited: Vec<&CauseRow> = causes.iter().filter(|c| c.rev_id == rev.rev_id).collect();
    assert!(!cited.is_empty());
    for c in &cited {
        let e = rows.iter().find(|e| e.event_id == c.event_id).unwrap();
        assert!(e.text.contains("went to the gym"), "{}", e.text);
    }
    let k = h.detail["k"].as_u64().unwrap() as usize;
    assert_eq!(cited.len(), k, "one citing event per day here");
    // topic change: rust -> cooking, mentions found through keywords, cited
    let rust = items.iter().find(|i| i.key == "topic:rust").expect("rust change");
    let cook = items.iter().find(|i| i.key == "topic:cooking").expect("cooking change");
    assert!(rust.rationale.contains("tag_rules.txt") && cook.rationale.contains("tag_rules.txt"));
    assert!(rust.stance.contains("2026-08-1") || rust.stance.contains("2026-08-2"), "{}", rust.stance);
    for c in causes.iter().filter(|c| revs.iter().any(|r| r.rev_id == c.rev_id && r.key == "topic:rust")) {
        assert!(rows.iter().find(|e| e.event_id == c.event_id).unwrap().text.contains("rust"));
    }
    // idempotent
    let (v, hash) = (log.latest_version(), log.snapshot_hash(log.latest_version()).unwrap());
    let r2 = derive::learn_with(&mut log, &p, &Kinds::ALL, false, Some(&rules)).unwrap();
    assert!(r2.outcomes.iter().all(|o| !o.is_new));
    assert_eq!(log.snapshot_hash(v).unwrap(), hash);
    assert_eq!(log.latest_version(), v);
    // stored events are never rewritten by learning
    assert_eq!(before, log.open_latest().rows::<EventRow>());
    // incremental: new days add rows only for changed evidence
    let start = dates::days_from_civil(2026, 7, 1);
    let more: Vec<NewEvent> = (90..100).map(|i| text_event(start + i, &format!("# day {i}\nMorning: went to the gym before work."))).collect();
    log.append_events(&more).unwrap();
    let r3 = derive::learn_with(&mut log, &p, &Kinds::ALL, false, Some(&rules)).unwrap();
    assert!(r3.outcomes.iter().any(|o| o.is_new));
    let r4 = derive::learn_with(&mut log, &p, &Kinds::ALL, false, Some(&rules)).unwrap();
    assert!(r4.outcomes.iter().all(|o| !o.is_new));
    // changing the rules re-derives: new fingerprint, new rows, old rows stay
    let rules2 = TagRules::parse(&format!("{DEMO_RULES}action:gym = lifted\n"));
    assert_ne!(rules.fingerprint, rules2.fingerprint);
    let r5 = derive::learn_with(&mut log, &p, &Kinds::ALL, false, Some(&rules2)).unwrap();
    assert!(r5.outcomes.iter().any(|o| o.is_new));
    assert!(derived_keys(&r5) != derived_keys(&r4));
    log.verify().unwrap();
}

#[test]
fn negated_mentions_are_not_counted_as_the_action() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = fresh(dir.path());
    let start = dates::days_from_civil(2026, 7, 6); // a Monday
    let mut ev = Vec::new();
    for i in 0..10 {
        ev.push(text_event(start + i, &if i < 3 { "Morning: gym session.".to_string() } else { "Morning: did not go to the gym today.".to_string() }));
    }
    log.append_events(&ev).unwrap();
    let rules = TagRules::parse("action:gym = gym\nctx:morning = morning\nbuiltin_contexts = off\n");
    let r = derive::learn_with(&mut log, &Params::default(), &Kinds::ALL, true, Some(&rules)).unwrap();
    assert!(r.derivation.items.is_empty(), "3 of 10 mornings is not a habit");
    let off = TagRules::parse("negation = off\naction:gym = gym\nctx:morning = morning\nbuiltin_contexts = off\n");
    // a context that holds on every event is dropped as uninformative (share 1.0 > 0.9)
    let r = derive::learn_with(&mut log, &Params::default(), &Kinds::ALL, true, Some(&off)).unwrap();
    assert!(r.derivation.items.is_empty());
    assert_eq!(r.derivation.uninformative.len(), 1);
    // add some evenings; then, with the guard off, the negations count as gym
    let evenings: Vec<NewEvent> = (10..14).map(|i| text_event(start + i, "Evening: read a book.")).collect();
    log.append_events(&evenings).unwrap();
    let r = derive::learn_with(&mut log, &Params::default(), &Kinds::ALL, true, Some(&off)).unwrap();
    assert!(r.derivation.items.iter().any(|i| i.key == "ctx:morning|act:gym"), "with the guard off the negations count as gym");
    let r = derive::learn_with(&mut log, &Params::default(), &Kinds::ALL, true, Some(&rules)).unwrap();
    assert!(r.derivation.items.is_empty(), "guard on: still not a habit");
}

#[test]
fn tools_use_tag_rules_and_check_reports_matches() {
    crate::safety::test_support::init();
    let owner = "lt-tagrules";
    crate::log_tools::open(owner).unwrap_or_else(|e| panic!("{e}")).append_events(&free_journal(90)).unwrap();
    let dir = crate::safety::owner_dir(owner);
    std::fs::create_dir_all(&dir).unwrap();
    // a tool cannot read the rules file
    std::fs::write(dir.join("tag_rules.txt"), format!("{DEMO_RULES}bogus line\n")).unwrap();
    assert!(crate::safety::safe_path(owner, "tag_rules.txt").is_err());
    let chk = ok("self_tag_rules_check", owner, json!({}));
    assert_eq!(chk["file_present"], true);
    assert_eq!(chk["warnings"].as_array().unwrap().len(), 1);
    let gym = chk["rules"].as_array().unwrap().iter().find(|r| r["rule"] == "action:gym").unwrap();
    assert!(gym["events_matched"].as_u64().unwrap() > 40, "{gym}");
    assert!(gym["events_dropped_by_negation_guard"].as_u64().unwrap() > 0, "{gym}");
    assert!(chk["events_unmatched"].as_u64().unwrap() > 0);
    assert!(chk["unmatched_sample"].as_array().unwrap().iter().any(|e| e["text"].as_str().unwrap().contains("Quiet day")));
    let wd = chk["day_occasions"].as_array().unwrap().iter().find(|c| c["context"] == "weekday").unwrap();
    assert!(wd["days"].as_u64().unwrap() > 50);
    // read-only: no new version
    let v0 = ok("self_learn", owner, json!({"dry_run": true}));
    assert_eq!(v0["version_before"], v0["version_after"]);
    assert_eq!(v0["tag_rules"]["rules"], 4);
    assert!(v0["derived"].as_array().unwrap().iter().any(|d| d["key"] == "ctx:weekday|act:gym"));
    let real = ok("self_learn", owner, json!({}));
    let again = ok("self_learn", owner, json!({}));
    assert_eq!(again["counts"]["habits"]["added"], 0);
    assert_eq!(again["counts"]["changes"]["added"], 0);
    assert!(real["counts"]["changes"]["added"].as_u64().unwrap() >= 2, "{real}");
    // explain cites the matching events
    let h = real["derived"].as_array().unwrap().iter().find(|d| d["key"] == "ctx:weekday|act:gym").unwrap();
    let ex = ok("self_explain", owner, json!({"rev_id": &h["rev_id"].as_str().unwrap()[..12]}));
    assert!(ex["cited_events"][0]["text"].as_str().unwrap().contains("gym"));
    std::fs::remove_file(dir.join("tag_rules.txt")).unwrap();
}

#[test]
fn free_text_measured_quality_stays_above_floor() {
    let rows = crate::measure::sweep_free(&crate::measure::SEEDS, &crate::measure::NOISE, &Params::default());
    for r in &rows {
        let n = r.noise;
        let min = |f: fn(&crate::measure::RunMetrics) -> f64| r.col(|m| Some(f(m))).unwrap().min;
        let med = |f: fn(&crate::measure::RunMetrics) -> f64| r.col(|m| Some(f(m))).unwrap().median;
        let max = |f: fn(&crate::measure::RunMetrics) -> f64| r.col(|m| Some(f(m))).unwrap().max;
        assert!(min(|m| m.habit_precision_planted) >= FT_PLANTED_PRECISION, "planted-context habit precision at noise {n}");
        assert!(min(|m| m.habit_precision) >= FT_PLANTED_PRECISION, "overall habit precision at noise {n} (lift test and uninformative-context drop)");
        assert!(max(|m| m.habit_builtin_detected as f64) <= 1.0, "built-in-context habits at noise {n}");
        assert!(min(|m| m.stance_precision) >= 0.95 && min(|m| m.stance_recall) >= 0.95, "stances at noise {n}");
        assert!(min(|m| m.minn_refused) >= 0.95, "min-n refusal at noise {n}");
        assert!(min(|m| m.change_precision) >= FT_CHANGE_PRECISION, "change precision at noise {n}");
        assert!(min(|m| m.change_recall) >= FT_CHANGE_RECALL, "change recall at noise {n}");
        assert!(max(|m| m.change_fp_rate) <= 0.25, "change FP rate at noise {n}");
        assert!(max(|m| m.change_abs_error) <= 6.0, "change error at noise {n}");
        if n == 0.0 {
            assert!(min(|m| m.habit_recall) >= FT_HABIT_RECALL_0, "habit recall at 0% noise");
        } else if n == 0.2 {
            assert!(med(|m| m.habit_recall) >= FT_HABIT_RECALL_20, "habit recall median at 20% noise");
        }
    }
}

// Floors for the free-text sweep, set slightly under the measured minima (docs/measured.md).
const FT_PLANTED_PRECISION: f64 = 0.95;
const FT_CHANGE_PRECISION: f64 = 0.3;
const FT_CHANGE_RECALL: f64 = 0.3;
const FT_HABIT_RECALL_0: f64 = 0.6;
const FT_HABIT_RECALL_20: f64 = 0.5;

// ── lift test and uninformative contexts ────────────────────────────────────

#[test]
fn lift_test_refuses_an_action_that_is_just_the_base_rate() {
    // 'run' is done at 80% in both contexts: neither context explains it.
    let mut evs = Vec::new();
    for d in 0..40i64 {
        for c in ["a", "b"] {
            let act = if (d * 7 + (c == "b") as i64) % 5 != 0 { "run" } else { "nap" };
            evs.push(ev(d, &[&format!("ctx:{c}"), &format!("act:{act}")], &format!("{c}{d}")));
        }
    }
    let p = Params::default();
    let det = habits::detect_full(&evs, &p, None);
    assert!(det.habits.is_empty(), "{:?}", det.habits);
    assert!(det.lift_rejected.iter().any(|l| l.action == "run"));
    // with the lift test off both would be habits (the old behaviour)
    let off = Params { habit_lift: -1.0, ..p.clone() };
    assert_eq!(habits::detect(&evs, &off).0.iter().filter(|h| h.action == "run").count(), 2);
    // a real difference passes, and the out-of-context numbers are recorded
    let mut evs2 = Vec::new();
    for d in 0..40i64 {
        evs2.push(ev(d, &["ctx:a", "act:run"], &format!("a{d}")));
        evs2.push(ev(d, &["ctx:b", if d % 4 == 0 { "act:run" } else { "act:nap" }], &format!("b{d}")));
    }
    let det2 = habits::detect_full(&evs2, &p, None);
    let h = det2.habits.iter().find(|h| h.context == "a" && h.action == "run").expect("a/run");
    assert!(h.lift.is_some());
    // a single context has nothing to compare with: kept
    let solo: Vec<Ev> = (0..20).map(|d| ev(d, &["ctx:a", "act:run"], &format!("s{d}"))).collect();
    assert_eq!(habits::detect_full(&solo, &p, None).habits.len(), 1);
}

#[test]
fn builtin_weekday_context_needs_a_difference_from_the_weekend() {
    let rules = TagRules::parse("action:gym = gym\naction:read = read\n");
    let start = dates::days_from_civil(2026, 7, 6); // Monday
    let mut flat = Vec::new();
    let mut weekday_only = Vec::new();
    for i in 0..70 {
        let d = start + i;
        let weekday = crate::learn::tagrules::weekday_index(d) < 5;
        flat.push(ev(d, &[], if i % 3 != 0 { "went to the gym" } else { "read a book" }));
        weekday_only.push(ev(d, &[], if weekday && i % 8 != 0 { "went to the gym" } else { "read a book" }));
    }
    let p = Params::default();
    let a = habits::detect_full(&flat, &p, Some(&rules));
    assert!(!a.habits.iter().any(|h| h.context == "weekday" || h.context == "weekend"), "base rate is not a weekday habit: {:?}", a.habits);
    assert!(a.lift_rejected.iter().any(|l| l.context == "weekday" && l.action == "gym"));
    let b = habits::detect_full(&weekday_only, &p, Some(&rules));
    assert!(b.habits.iter().any(|h| h.context == "weekday" && h.action == "gym"));
    assert!(!b.habits.iter().any(|h| h.context == "weekend" && h.action == "gym"));
}

// ── evolve and ledger read free text through tag_rules.txt too ──────────────

#[test]
fn evolve_and_ledger_use_the_same_virtual_tags_as_learn() {
    crate::safety::test_support::init();
    let (events, _) = synth::generate(&synth::free_text_config(11, 0.2));
    let (with, without) = ("lt-evo-rules", "lt-evo-norules");
    for o in [with, without] {
        crate::log_tools::open(o).unwrap_or_else(|e| panic!("{e}")).append_events(&events).unwrap();
    }
    let dir = crate::safety::owner_dir(with);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tag_rules.txt"), synth::free_text_rules()).unwrap();
    // library level
    let rules = TagRules::parse(&synth::free_text_rules());
    let evs = synth::to_evs(&events);
    let origin = evs.iter().map(|e| e.day).max().unwrap() - 30;
    let p = Params::default();
    assert!(evolve::predict_at(&evs, &p, origin, Some(&rules)).contains(&("morning".to_string(), "run".to_string())));
    assert!(evolve::predict_at(&evs, &p, origin, None).is_empty(), "free text has no tags without rules");
    let cfg = evolve::EvolveCfg { rules: Some(rules.clone()), ..evolve::EvolveCfg::default() };
    assert!(evolve::truth_at(&evs, origin, &cfg).contains(&("morning".to_string(), "run".to_string())));
    assert!(evolve::truth_at(&evs, origin, &evolve::EvolveCfg::default()).is_empty());
    assert!(!evolve::baseline_at(&evs, origin, Some(&rules)).is_empty());
    assert!(evolve::baseline_at(&evs, origin, None).is_empty());
    // tool level: ledger
    let l1 = ok("self_ledger_snapshot", with, json!({"dry_run": true}));
    assert!(l1["written"].as_array().unwrap().iter().any(|w| w["context"] == "morning" && w["action"] == "run"), "{l1}");
    let l0 = ok("self_ledger_snapshot", without, json!({"dry_run": true}));
    assert!(l0["written"].as_array().unwrap().is_empty(), "{l0}");
    // tool level: evolve (dry run): with rules the baseline has something to score
    let e1 = ok("self_evolve_rules", with, json!({"dry_run": true}));
    let e0 = ok("self_evolve_rules", without, json!({"dry_run": true}));
    assert!(e1["evaluation"]["baseline"]["f1"].as_f64().unwrap() > 0.0, "{e1}");
    assert_eq!(e0["evaluation"]["baseline"]["f1"].as_f64().unwrap_or(0.0), 0.0, "{e0}");
    std::fs::remove_file(dir.join("tag_rules.txt")).unwrap();
}
