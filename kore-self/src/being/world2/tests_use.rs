//! Tests of Track B7 (using discovered rules, `rules_use.rs`): with every switch off, logs, states and
//! measurements are byte-identical to the code before B7 (golden digests computed on ecf5f695); with the
//! switches on, runs are deterministic and replay from the log, every rule use is logged with its record
//! and cites the rule, the controller caps and the kill switch hold, and the robust and constant searches
//! do what they say.

use super::engine::{run_v2, LearnerCfg, Run2Cfg, Run2Summary, State2};
use super::learner::Learner2;
use super::measure::{run_w2, run_w2_with};
use super::replay::{rederive_rules2, replay_decisions2};
use super::rules_use::*;
use super::world::*;
use crate::being::controller::{Controller, Limits};
use crate::being::discover::{search, Example};
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::state::tag;
use crate::versioned::{sha256_hex, CauseRow, EventRow, RevisionRow, VersionedLog};

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn cfg(seed: u64, fams: usize, budget: i64) -> W2Cfg {
    W2Cfg { noise_pm: 80, drift_per_10k: 25, curve_pct: 50, ..W2Cfg::new(seed, fams, budget) }
}

/// Digest of everything a logged run wrote: events, revisions (with rationale) and their causes.
pub(super) fn log_digest(log: &VersionedLog) -> String {
    let snap = log.open_latest();
    let mut s = String::new();
    for e in snap.rows::<EventRow>() {
        s.push_str(&format!("E|{}|{}|{}|{}|{}|{}\n", e.event_id, e.source, e.occurred_at, e.kind, e.text, e.tags));
    }
    for r in snap.rows::<RevisionRow>() {
        s.push_str(&format!("R|{}|{}|{}|{}|{}|{}|{}|{}\n", r.rev_id, r.entity_type, r.key, r.stance, r.confidence, r.rationale, r.valid_from, r.author));
    }
    for c in snap.rows::<CauseRow>() {
        s.push_str(&format!("C|{}|{}\n", c.rev_id, c.event_id));
    }
    sha256_hex(&[s.as_bytes()])
}

fn logged_digest(c: W2Cfg, steps: u64, compose: bool) -> String {
    let d = tempfile::tempdir().unwrap();
    let mut log = open(d.path());
    let ctl = if compose {
        let mut k = Controller::defaults();
        k.composition = true;
        Some(k.limits(true, false).0)
    } else {
        None
    };
    run_v2(&mut log, &Run2Cfg { world: Some(c), learner: None, discover_every: 10, ctl }, &Policy::new(steps), None).unwrap();
    log_digest(&log)
}

fn measured_digest() -> String {
    let mut s = String::new();
    for (seed, fams, budget, noise, drift) in [(1u64, 6usize, 1500i64, 0u32, 0u32), (2, 12, 3000, 100, 20), (3, 24, 3000, 100, 0)] {
        let c = W2Cfg { noise_pm: noise, drift_per_10k: drift, ..W2Cfg::new(seed, fams, budget) };
        let r = run_w2(&c, &mut Learner2::default(), 10, None);
        s.push_str(&format!("{} {} {} {} {}|", r.true_reward, r.reported_reward, r.cost, r.steps, r.rules.len()));
        for x in &r.recs {
            s.push_str(&format!(
                "{} {} {:?} {:?} {} {:?} {} {} {} {} {} {} {} {} {} {:?};",
                x.step, x.clock, x.fam, x.ftype, x.difficulty, x.strategy, x.oracle, x.attempted, x.true_correct, x.reported_correct, x.flipped, x.true_reward, x.reported_reward, x.cost, x.rule_used, x.predicted_p
            ));
        }
        for x in &r.rules {
            let q = &x.rule;
            s.push_str(&format!("{} {} {:?} {} {} {} {} {:?} {} {} {} {:?};", q.id, q.sig, q.form, q.train_ok, q.val_ok, q.step, q.event_id, q.xf, q.nblocks, q.retired, x.wrong_at_accept, x.right_share_end));
            s.push_str(&format!("{};", x.clock));
        }
    }
    sha256_hex(&[s.as_bytes()])
}

const GOLDEN_LOG: &str = "256436c3ee35c21a6a43c7b0f165dd2cacd6e9c601806fad379f0418bbb67b85";
const GOLDEN_LOG_COMPOSE: &str = "6f94341fff1b4b23dd50fa83d2ad055dfb60b62fc81b75c127f1ecf40f0d0564";
const GOLDEN_MEASURED: &str = "ea3990f97fd5a7ae69d9270e9f52ed49748da80abb26f61e34d074cdaabbd755";

#[test]
fn with_every_switch_off_logs_and_measurements_are_byte_identical_to_before_b7() {
    let a = logged_digest(cfg(3, 12, 3000), 400, false);
    let b = logged_digest(cfg(5, 18, 4000), 300, true);
    let m = measured_digest();
    println!("GOLDEN_LOG {a}\nGOLDEN_LOG_COMPOSE {b}\nGOLDEN_MEASURED {m}");
    assert_eq!(a, GOLDEN_LOG);
    assert_eq!(b, GOLDEN_LOG_COMPOSE);
    assert_eq!(m, GOLDEN_MEASURED);
}

// ---- with the switches on ----

fn all_on() -> UseCfg {
    UseCfg { rule_est: true, rule_only: true, robust_milli: 400, fit_const: true, fit_grid: true, rule_k: DEFAULT_RULE_K }
}

fn run_on(root: &std::path::Path, c: W2Cfg, u: UseCfg, steps: u64, ctl: Option<Limits>, pol: Option<Policy>) -> (VersionedLog, Run2Summary) {
    let mut log = open(root);
    let l = LearnerCfg { use_: u, ..LearnerCfg::default() };
    let s = run_v2(&mut log, &Run2Cfg { world: Some(c), learner: Some(l), discover_every: 10, ctl }, &pol.unwrap_or_else(|| Policy::new(steps)), None).unwrap();
    (log, s)
}

#[test]
fn with_the_switches_on_runs_are_deterministic_replay_and_log_every_rule_use_with_its_record() {
    let c = cfg(3, 12, 4000);
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (l1, s1) = run_on(d1.path(), c.clone(), all_on(), 600, None, None);
    let (l2, _) = run_on(d2.path(), c.clone(), all_on(), 600, None, None);
    assert_eq!(log_digest(&l1), log_digest(&l2));
    assert_ne!(log_digest(&l1), logged_digest(c.clone(), 600, false), "the switches change the run");
    // the unlogged measurement takes the same steps
    let lc = LearnerCfg { use_: all_on(), ..LearnerCfg::default() };
    let m = run_w2_with(&c, &mut lc.learner(), 10, None, lc);
    assert_eq!(&m.recs[..600], &s1.truths[..]);
    let rows = l1.open_latest().rows::<EventRow>();
    let st = State2::from_events(&rows);
    assert_eq!(st, s1.state, "the state folded from the log equals the live state");
    assert_eq!(st.learner.use_, all_on());
    assert!(rows[0].tags.contains("w2_use:1/1/400/1/1/8"));
    let rep = replay_decisions2(&rows);
    assert_eq!(rep.len(), 600);
    assert!(rep.iter().all(|r| r.matches()), "{:?}", rep.iter().find(|r| !r.matches()));
    let rr = rederive_rules2(&rows);
    assert!(rr.iter().all(|r| r.1), "{rr:?}");
    assert!(!st.const_rules.is_empty(), "this world fits a constant within 600 steps");
    assert!(rr.iter().any(|r| st.const_rules.iter().any(|c| c.id == r.0)));
    // every rule use: the action carries the rule and its record, the decision cites the rule's note
    let snap = l1.open_latest();
    let causes: Vec<CauseRow> = snap.rows();
    let revs: Vec<RevisionRow> = snap.rows();
    let notes: std::collections::HashMap<String, String> = rows.iter().filter(|e| e.kind == "note" && e.tags.contains("rule_id:")).map(|e| (tag(&e.tags, "rule_id").unwrap().to_string(), e.event_id.clone())).collect();
    let mut uses = 0;
    for a in rows.iter().filter(|e| e.kind == "action" && e.tags.contains("rule_used:")) {
        uses += 1;
        assert!(a.tags.contains("rule_record:support"), "{}", a.tags);
        let step: u64 = tag(&a.tags, "step").unwrap().parse().unwrap();
        let id = tag(&a.tags, "rule_used").unwrap();
        let rev = revs.iter().find(|r| r.key == format!("being/world2/choice/{step:06}")).unwrap();
        assert!(causes.iter().any(|c| c.rev_id == rev.rev_id && Some(&c.event_id) == notes.get(id)), "decision {step} cites rule {id}");
        assert!(rev.rationale.contains("estimate_from_rule") && rev.rationale.contains("rules_in_use"));
    }
    assert!(uses > 20, "{uses} rule uses");
    // a continued run picks up the switches from the genesis event and still replays
    let mut l1 = l1;
    run_v2(&mut l1, &Run2Cfg { discover_every: 10, ..Default::default() }, &Policy::new(50), None).unwrap();
    let rows = l1.open_latest().rows::<EventRow>();
    assert!(replay_decisions2(&rows).iter().all(|r| r.matches()));
    assert!(rederive_rules2(&rows).iter().all(|r| r.1));
}

#[test]
fn with_the_switches_on_the_caps_and_the_kill_switch_hold() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let d = tempfile::tempdir().unwrap();
    let lim = Limits { max_live_rules: 2, ..Limits::default() };
    let (log, s) = run_on(d.path(), W2Cfg::new(3, 12, 6000), all_on(), 700, Some(lim), None);
    assert!(s.state.live_rules() <= 2, "{} live rules (constants count)", s.state.live_rules());
    let ev = log.open_latest().rows::<EventRow>();
    assert!(ev.iter().any(|e| e.tags.contains("cap:max_live_rules")));
    // kill switch: checked before every step
    let d = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicU64::new(0));
    let c2 = calls.clone();
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 41)));
    let (log, s) = run_on(d.path(), cfg(1, 12, 3000), all_on(), 100, None, Some(pol));
    assert_eq!((s.steps_run, s.stopped), (40, StopReason::Killed));
    assert_eq!(State2::from_events(&log.open_latest().rows::<EventRow>()).step, 40);
    // energy cap
    let d = tempfile::tempdir().unwrap();
    let mut pol = Policy::new(1000);
    pol.max_energy = Some(150);
    let (_, s) = run_on(d.path(), cfg(2, 12, 3000), all_on(), 1000, None, Some(pol));
    assert_eq!(s.stopped, StopReason::EnergyCap);
}

fn ex(i: usize, sig: u8, terms: Vec<i64>, next: i64) -> Example {
    Example { event_id: format!("e{i}"), sig, terms, next }
}

#[test]
fn the_robust_search_is_the_b2_search_at_threshold_zero_and_tolerates_a_wrong_revealed_answer() {
    // examples from several rules, some corrupted: at threshold 0 the robust search is the plain B2 search
    let mut r = crate::being::world::Rng::new(7);
    let mut v = vec![];
    for i in 0..400 {
        let sig = (i % 4) as u8;
        let (a, b) = (r.range(1, 4), r.range(-3, 5));
        let mut t = vec![r.range(1, 6)];
        for _ in 0..7 {
            let x = *t.last().unwrap();
            t.push(if sig == 3 { x + a } else { a * x + b });
        }
        let next = t.pop().unwrap() + if r.below(10) == 0 { 1 } else { 0 };
        v.push(ex(i, sig, t, next));
    }
    let plain: Vec<_> = search(&v).into_iter().map(|f| (f.sig, f.form, f.train_ok, f.val_ok, f.cite)).collect();
    let rob: Vec<_> = search_robust(&v, 0).into_iter().map(|f| (f.found.sig, f.found.form, f.found.train_ok, f.found.val_ok, f.found.cite)).collect();
    assert_eq!(plain, rob);
    // one wrong revealed answer among 30 arithmetic examples: B2 never accepts a form that misses it, the robust guard does
    let mut w: Vec<Example> = (0..30).map(|i| ex(i, 9, (0..6).map(|k| 3 + 2 * (k + i as i64)).collect(), 3 + 2 * (6 + i as i64))).collect();
    w[4].next += 1;
    let strict = search(&w);
    let got = search_robust(&w, 400);
    assert!(got.iter().any(|f| f.wrong == 1), "{got:?}");
    for f in got.iter().filter(|f| f.wrong > 0) {
        assert!(!strict.iter().any(|s| s.form == f.found.form), "B2 never accepts a form that missed an example");
    }
    // a form wrong on every other example is not accepted
    let mut z = w.clone();
    for e in z.iter_mut().step_by(2) {
        e.next += 3;
    }
    assert!(search_robust(&z, 400).iter().all(|f| wilson_lb(f.found.train_ok + f.found.val_ok, f.found.train_ok + f.found.val_ok + f.wrong) >= 0.4));
    assert!(wilson_lb(29, 30) > 0.8 && wilson_lb(15, 30) < 0.4 && robust_ok(5, 0, 0) && !robust_ok(29, 1, 0));
}

#[test]
fn the_constant_search_fits_g_and_the_grid_move_rule_and_rejects_what_does_not_fit() {
    let mk = |i: usize, fam: usize, kind: &str, x: Vec<i64>, rv: i64| CExample { event_id: format!("c{i}"), fam, kind: kind.into(), x, revealed: rv };
    let mut v = vec![];
    for i in 0..8 {
        let t = 1 + i as i64 % 4;
        v.push(mk(i, 3, "fall", vec![t], 12 * t * t / 2));
        v.push(mk(100 + i, 9, "weight", vec![t + 2], (t + 2) * 6));
    }
    let f = search_const(&v, 3, 1, 0);
    assert_eq!(f.iter().map(|x| (x.fam, x.g)).collect::<Vec<_>>(), vec![(3, 12), (9, 6)]);
    // one wrong revealed answer: B2's guard rejects, the robust guard accepts with one miss
    v[2].revealed += 5;
    assert!(search_const(&v, 3, 1, 0).iter().all(|x| x.fam != 3));
    assert!(search_const(&v, 3, 1, 400).iter().any(|x| x.fam == 3 && x.g == 12 && x.wrong == 1));
    // inconsistent answers: nothing
    let bad: Vec<CExample> = (0..10).map(|i| mk(i, 3, "weight", vec![2 + i as i64], 7 * (2 + i as i64) + i as i64)).collect();
    assert!(search_const(&bad, 3, 1, 400).is_empty());
    // grid: path lengths of 8-neighbour moves on open grids fit 8, not 4
    let g: Vec<CExample> = (0..8)
        .map(|i| {
            let n = 4 + (i % 3) as i64;
            let mut x = vec![n, n];
            x.extend(std::iter::repeat(0).take((n * n) as usize));
            mk(i, 1, "grid", x, n - 1)
        })
        .collect();
    assert_eq!(search_const(&g, 3, 1, 0).iter().map(|x| x.g).collect::<Vec<_>>(), vec![8]);
    assert_eq!(param_answer("grid", 4, &g[0].x), Some(2 * (g[0].x[0] - 1)));
}

#[test]
fn rules_use_reads_no_hidden_parameter() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being").join("world2");
    let hidden = ["regime", "Regime", ".expected(", "slip_noise", "class_of", "noise_mult", "slip_pm", "World2", "truth", "feedback(", "wall_pm", "SeqClass"];
    let src = std::fs::read_to_string(dir.join("rules_use.rs")).unwrap();
    for b in hidden {
        assert!(!src.contains(b), "rules_use.rs mentions {b}");
    }
}
