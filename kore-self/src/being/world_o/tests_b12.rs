//! Tests of Track B12 (learning to learn in World O): off by default and then exactly the B11 being; each
//! mechanism (hierarchical prior, new-family exploration, family-wide rules and the rule-form prior); the
//! exploration budget and the controller's cap; caps and the kill switch with B12 on; fold, replay and restart
//! from the log.

use super::b12::*;
use super::engine::*;
use super::measure::run_wo;
use super::measure_b12::run_b12;
use super::world::*;
use crate::being::controller::Limits;
use crate::being::discover::{Example, Form};
use crate::being::model::Cell;
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::solver::Strategy;
use crate::being::world2::solve::sig2;
use crate::being::world2::world::FType;
use crate::versioned::{EventRow, VersionedLog};

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn small(seed: u64) -> WOCfg {
    WOCfg { intro_every: 400, life_intros: 6, tier_every: 2, drift_pm: 500, ..WOCfg::new(seed, 40_000) }
}

fn all_on() -> LearnerOCfg {
    LearnerOCfg { l2l: L2LCfg { prior_k: 8, explore_n: 10, explore_w_milli: 500, explore_pm: 50, form_prior: 2 }, ..LearnerOCfg::default() }
}

fn lim() -> Limits {
    Limits { max_live_rules: 200, max_candidates: 100_000, ..Limits::default() }
}

fn run_logged(root: &std::path::Path, c: WOCfg, l: LearnerOCfg, steps: u64, cap: Option<u32>) -> (VersionedLog, RunOSummary) {
    let mut log = open_log(root);
    let s = run_o(&mut log, &RunOCfg { world: Some(c), learner: Some(l), discover_every: 10, ctl: Some(lim()), max_explore_pm: cap }, &Policy::new(steps), None).unwrap();
    (log, s)
}

#[test]
fn b12_is_off_by_default_and_then_the_b11_being_exactly() {
    assert!(!LearnerOCfg::default().l2l.any());
    // the B12 measurement run with the switches off is the B11 run, window for window
    for seed in [3u64, 9] {
        let c = WOCfg { budget: 20_000, ..small(seed) };
        let a = run_wo(&c, LearnerOCfg::default(), None, 10, Some(lim()), u64::MAX, 2000, true);
        let b = run_b12(&c, LearnerOCfg::default(), None, 10, Some(lim()), u64::MAX, 2000, true);
        assert_eq!(a.wins.len(), b.run.wins.len());
        for (x, y) in a.wins.iter().zip(&b.run.wins) {
            assert_eq!((x.steps, x.true_reward, x.cost, x.attempts, x.rules_accepted, x.rule_uses), (y.steps, y.true_reward, y.cost, y.attempts, y.rules_accepted, y.rule_uses));
        }
        assert_eq!((b.explore_energy, b.explore_attempts, b.reduced_rules), (0, 0, 0));
    }
    // a default being's log carries no B12 tag, and its state holds nothing of B12
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = run_logged(dir.path(), small(7), LearnerOCfg::default(), 1500, None);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(!rows.iter().any(|e| e.tags.contains("wo_l2l") || e.tags.contains("b12_")));
    assert_eq!(s.state.b12, B12State::default());
    // pinned digest of a default run (B11 being, the same before and after B12; see docs/measured.md, Track B12)
    let r = run_wo(&WOCfg { budget: 30_000, ..small(5) }, LearnerOCfg::default(), None, 10, Some(lim()), u64::MAX, 3000, false);
    let digest: Vec<(u64, i64, i64)> = r.wins.iter().map(|w| (w.steps, w.true_reward, w.cost)).collect();
    let h = crate::versioned::sha256_hex(&[format!("{digest:?}").as_bytes()]);
    assert_eq!(&h[..16], GOLDEN_B11, "the default World O run changed: {digest:?}");
}

/// First 16 hex digits of the digest above (switches off). Before pinning it, the outputs of `measure_b11 worldo`
/// and `measure_b11 b1` from the binary built before Track B12 and from the one built after were compared: byte-identical.
const GOLDEN_B11: &str = "26b0c6859490b918";

#[test]
fn the_l2l_tag_round_trips_and_bad_tags_are_refused() {
    let c = all_on().l2l;
    assert_eq!(L2LCfg::parse(c.tag().trim_start_matches("wo_l2l:")), Some(c));
    assert_eq!(L2LCfg::parse("8/10/500/50/3"), None);
    assert_eq!(L2LCfg::parse("8/10/500"), None);
    let (g, _) = genesis_event_o(&small(1), &all_on());
    assert!(g.tags.iter().any(|t| t.starts_with("wo_l2l:")));
}

fn hist(fid: u64, ftype: FType, s: Strategy, n: u64, succ: u64) -> FamHist {
    let mut cells = [[Cell::default(); 4]; 5];
    for k in 0..5 {
        cells[k][s.idx()] = Cell { n, succ, spent: n as i64 * 4, listed: n as i64 * 4 };
    }
    FamHist { fid, ftype, cells }
}

#[test]
fn the_hierarchical_prior_reads_only_earlier_families_of_the_same_type() {
    // too few families: no prior
    assert!(eb_prior(&[(1, 10), (2, 10)]).is_none());
    // consistent families: mean near their share, a strong weight; spread-out families: a weak weight
    let tight = eb_prior(&[(2, 20), (2, 20), (3, 20), (2, 20)]).unwrap();
    let loose = eb_prior(&[(0, 20), (20, 20), (1, 20), (19, 20)]).unwrap();
    assert!((tight.mean - 0.12).abs() < 0.03 && tight.k > loose.k && loose.k == K_MIN, "{tight:?} {loose:?}");
    let before = vec![
        hist(1, FType::Seq, Strategy::Careful, 20, 2),
        hist(2, FType::Grid, Strategy::Careful, 20, 19),
        hist(3, FType::Seq, Strategy::Careful, 20, 3),
        hist(4, FType::Seq, Strategy::Careful, 20, 2),
        hist(9, FType::Seq, Strategy::Careful, 20, 20),
    ];
    let p = form_slot_prior(5, FType::Seq, &before, 8);
    assert_eq!(p.fams, 3, "families 1, 3, 4 (same type, earlier); not 2 (grid) nor 9 (later)");
    let c = p.cells[2][Strategy::Careful.idx()].unwrap();
    assert!(c.mean < 0.25, "{c:?}");
    assert!(p.cells[2][Strategy::Deep.idx()].is_none(), "no strategy data, no prior");
    // the estimate of an empty cell is the prior's
    let m = crate::being::world2::model::FamModel::new(4);
    let e = estimate_h(&m, 2, 3, Strategy::Careful, &p);
    assert!((e.p - c.mean).abs() < 1e-9 && e.n == 0);
    let b1 = m.estimate(2, 3, Strategy::Careful);
    assert!((b1.p - 0.5).abs() < 1e-9, "the B1 estimate of an empty model is 1/2");
}

fn ex(slot: usize, d: u8, terms: Vec<i64>, next: i64, i: usize) -> Example {
    Example { event_id: format!("ev{slot}-{d}-{i}"), sig: sig2(slot, d), terms, next }
}

/// An engine with one open sequence family in slot 3 and the given examples.
fn engine_with(examples: Vec<Example>, form_prior: u8) -> EngineO {
    let mut l = LearnerOCfg::default();
    l.l2l.form_prior = form_prior;
    let mut st = StateO::default();
    st.start(small(1), l, "genesis".into());
    st.open_family(&Open { fid: 3, slot: 3, ftype: FType::Seq, base_slot: None });
    st.s2.examples = examples;
    EngineO::new(st, Some(lim()))
}

fn arith(a: i64, d: i64, len: usize) -> (Vec<i64>, i64) {
    ((0..len as i64).map(|i| a + d * i).collect(), a + d * len as i64)
}

#[test]
fn family_wide_rules_pool_the_examples_of_all_difficulties() {
    // four examples, one per difficulty: no signature has the B2 support (3 + 1), the family has
    let xs: Vec<Example> = (1..=4u8).map(|d| {
        let (t, n) = arith(d as i64, 3 + d as i64, 8);
        ex(3, d, t, n, d as usize)
    }).collect();
    let mut e0 = engine_with(xs.clone(), 0);
    assert!(e0.discover_pass().is_empty(), "B2 alone accepts nothing");
    let mut e = engine_with(xs.clone(), 1);
    let new = e.discover_pass();
    assert_eq!(new.len(), 5, "one rule per difficulty of the family, including difficulty 5 without examples");
    assert!(new.iter().all(|r| r.event.tags.iter().any(|t| t == "b12_family") && !r.event.tags.iter().any(|t| t == "b12_reduced")));
    assert!(new.iter().all(|r| !r.revision.caused_by.is_empty()));
    // a second pass adds nothing (each difficulty has its rule)
    assert!(e.discover_pass().is_empty());
    // one example the form gets wrong (and that no other form explains with the rest) stops it
    let mut bad = xs.clone();
    bad.push(ex(3, 2, vec![1, 2, 4, 8, 16, 32, 64, 128], 255, 9));
    let mut e2 = engine_with(bad, 1);
    assert!(!e2.discover_pass().iter().any(|r| r.rule.form == Form::Lin { mask: 1, constant: true }));
}

#[test]
fn the_rule_form_prior_lowers_the_support_only_for_forms_seen_in_earlier_families() {
    // three examples: 2 training + 1 held out (below B2's 3 + 1 even pooled)
    let xs: Vec<Example> = (1..=3u8).map(|d| {
        let (t, n) = arith(2 * d as i64, 5, 8);
        ex(3, d, t, n, d as usize)
    }).collect();
    let form = Form::Lin { mask: 1, constant: true };
    let mut e = engine_with(xs.clone(), 2);
    assert!(e.discover_pass().is_empty(), "no prior over forms yet: B2 support needed");
    // the form was accepted in two earlier families
    let mut e = engine_with(xs.clone(), 2);
    e.state.b12.form_fams.entry(form.code()).or_default().extend([1u64, 2]);
    e.state.b12.form_version += 1;
    let new = e.discover_pass();
    assert_eq!(new.len(), 5);
    assert!(new.iter().all(|r| r.rule.form == form && r.event.tags.iter().any(|t| t == "b12_reduced")));
    assert_eq!(e.state.b12.reduced_rules, 5);
    // level 1 ignores the form prior
    let mut e = engine_with(xs, 1);
    e.state.b12.form_fams.entry(form.code()).or_default().extend([1u64, 2]);
    assert!(e.discover_pass().is_empty());
}

#[test]
fn exploration_tries_new_families_within_its_budget_and_the_controller_cap() {
    let c = WOCfg { budget: 30_000, ..small(2) };
    let l = LearnerOCfg { l2l: L2LCfg { explore_n: 10, explore_w_milli: 2000, explore_pm: 100, ..Default::default() }, ..Default::default() };
    let r = run_b12(&c, l, None, 10, Some(lim()), u64::MAX, 3000, false);
    assert!(r.explore_attempts > 0);
    // the budget holds (one step may cross it)
    assert!(r.explore_energy as f64 <= 0.100 * r.run.cost as f64 + 40.0, "{} of {}", r.explore_energy, r.run.cost);
    // logged, with a controller cap of 10 per mille: the refusal is in the log before the first decision
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = run_logged(dir.path(), c.clone(), l, 4000, Some(10));
    let rows: Vec<EventRow> = log.open_latest().rows();
    let ref_i = rows.iter().position(|e| e.tags.contains("cap:max_explore_pm")).expect("refusal logged");
    let dec_i = rows.iter().position(|e| e.kind == "decision").unwrap();
    assert!(ref_i < dec_i);
    assert_eq!(s.state.b12.explore_cap_pm, Some(10));
    assert!(s.state.b12.explore_energy as f64 <= 0.010 * s.cost as f64 + 40.0);
    assert!(rows.iter().any(|e| e.kind == "decision" && e.tags.contains("policy:wo-explore")));
    assert_eq!(StateO::from_events(&rows), s.state);
    assert_eq!(replay_o(&rows).1, 0);
}

#[test]
fn with_b12_on_the_state_folds_from_the_log_replays_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = run_logged(dir.path(), small(7), all_on(), 2500, None);
    assert_eq!(s.steps_run, 2500);
    let rows: Vec<EventRow> = log.open_latest().rows();
    let folded = StateO::from_events(&rows);
    assert_eq!(folded, s.state, "fold of the log equals the live state");
    assert!(!folded.b12.hist.is_empty() && !folded.b12.priors.is_empty(), "priors were formed and families closed");
    let (n, bad) = replay_o(&rows);
    assert_eq!((n, bad), (2500, 0));
    let d2 = tempfile::tempdir().unwrap();
    let (_, a) = run_logged(d2.path(), small(7), all_on(), 1200, None);
    assert_eq!(a.steps_run, 1200);
    let mut log2 = open_log(d2.path());
    let b = run_o(&mut log2, &RunOCfg { discover_every: 10, ctl: Some(lim()), ..Default::default() }, &Policy::new(1300), None).unwrap();
    assert_eq!(b.state, s.state);
    let tuples = |rows: &[EventRow]| rows.iter().filter(|e| e.source == SOURCE_O).map(|e| (e.kind.clone(), e.text.clone(), e.tags.clone())).collect::<Vec<_>>();
    assert_eq!(tuples(&log2.open_latest().rows()), tuples(&rows));
}

#[test]
fn with_b12_on_caps_and_the_kill_switch_hold() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    // live-rule cap (family-wide rules count like any rule)
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let l1 = Limits { max_live_rules: 2, max_candidates: 100_000, ..Limits::default() };
    let s = run_o(&mut log, &RunOCfg { world: Some(small(4)), learner: Some(all_on()), discover_every: 10, ctl: Some(l1), max_explore_pm: None }, &Policy::new(3000), None).unwrap();
    assert!(s.state.s2.live_rules() <= 2);
    // kill before the first step, and after 12 steps
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(|| true)));
    let s = run_o(&mut log, &RunOCfg { world: Some(small(1)), learner: Some(all_on()), ..Default::default() }, &pol, None).unwrap();
    assert_eq!((s.steps_run, s.stopped), (0, StopReason::Killed));
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let calls = Arc::new(AtomicU64::new(0));
    let c2 = calls.clone();
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 13)));
    let s = run_o(&mut log, &RunOCfg { world: Some(small(1)), learner: Some(all_on()), discover_every: 10, ..Default::default() }, &pol, None).unwrap();
    assert_eq!((s.steps_run, s.stopped), (12, StopReason::Killed));
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().any(|e| e.tags.contains("stop:kill_switch")));
    assert_eq!(StateO::from_events(&rows).s2.step, 12);
}
