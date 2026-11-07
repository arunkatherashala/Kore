//! Tests of World O: determinism, the family schedule, the checkers, the learner's equivalence with
//! the World v2 learner, the logged loop (fold, replay, restart), caps, the kill switch, isolation, and
//! the Track B11 skip switch (off by default).

use super::engine::*;
use super::measure::{run_wo, OracleO, TruthO};
use super::world::*;
use crate::being::controller::Limits;
use crate::being::planner::SkipRule;
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::world2::learner::{Chooser2, Learner2};
use crate::being::world2::world::{bfs, knap_exact, FType, FormulaKind};
use crate::versioned::{EventRow, VersionedLog};

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn small(seed: u64) -> WOCfg {
    WOCfg { intro_every: 400, life_intros: 6, tier_every: 2, drift_pm: 500, ..WOCfg::new(seed, 40_000) }
}

#[test]
fn world_o_is_a_pure_function_of_its_config() {
    let (a, b) = (WorldO::new(small(5)), WorldO::new(small(5)));
    assert_eq!(a.fams, b.fams);
    for step in [0u64, 1, 17, 400, 3999] {
        for clock in [0i64, 30 * 1000, 30 * 9000, 30 * 30_000] {
            let (x, y) = (a.offers(step, clock), b.offers(step, clock));
            assert_eq!(x.len(), 4);
            for (p, q) in x.iter().zip(&y) {
                assert_eq!((&p.view, p.fid, p.expected()), (&q.view, q.fid, q.expected()));
            }
        }
    }
    let c = WorldO::new(small(6));
    assert_ne!(c.offers(3, 0)[0].view, a.offers(3, 0)[0].view);
}

#[test]
fn families_open_retire_and_rise_in_tier() {
    let w = WorldO::new(small(2));
    let mut seen = std::collections::BTreeSet::new();
    let mut last_newest = 0;
    for e in (0..40_000i64).step_by(100) {
        let open = w.open(e * 30);
        assert!(!open.is_empty());
        // slots of open families are distinct; a compose family's base is open and outlives it
        let slots: std::collections::BTreeSet<usize> = open.iter().map(|j| w.fams[*j as usize].slot).collect();
        assert_eq!(slots.len(), open.len());
        for j in &open {
            let f = &w.fams[*j as usize];
            assert!(f.intro <= e && e < f.retire);
            if let Some(b) = f.base {
                assert!(open.contains(&b) && w.fams[b as usize].retire >= f.retire && w.fams[b as usize].ftype == FType::Seq);
            }
            seen.insert(*j);
        }
        let newest = *open.last().unwrap();
        assert!(newest >= last_newest);
        last_newest = newest;
    }
    assert!(seen.len() > 90, "{} families seen", seen.len());
    // tiers never fall with the family index; all tiers and compose families appear
    assert!(w.fams.windows(2).all(|x| x[0].tier <= x[1].tier));
    assert!(w.fams.iter().any(|f| f.tier == MAX_TIER) && w.fams.iter().any(|f| f.ftype == FType::Compose));
    assert!(w.fams.iter().any(|f| f.drift_at.is_some()));
    // validation
    assert!(WOCfg { life_intros: 41, ..small(1) }.validate().is_err());
    assert!(WOCfg { menu: 1, ..small(1) }.validate().is_err());
    assert_eq!(WOCfg::from_tags(&small(9).tags().join(",")), Some(small(9)));
}

#[test]
fn checkers_agree_with_an_independent_statement_of_each_hidden_rule() {
    let w = WorldO::new(small(3));
    let mut n = [0u32; 5];
    for step in 0..3000u64 {
        let clock = (step as i64 * 13) * 30;
        for t in w.offers(step, clock) {
            let v = &t.view;
            let reg = w.regime(t.fid, clock);
            match v.ftype {
                FType::Grid => {
                    assert_eq!(Some(t.expected()), bfs(v.params[0] as usize, v.params[1] as usize, &v.params[2..], reg.eight));
                    n[0] += 1;
                }
                FType::Knap => {
                    let items: Vec<(i64, i64)> = v.params[1..].chunks(2).map(|c| (c[0], c[1])).collect();
                    assert_eq!(t.expected(), knap_exact(v.params[0], &items));
                    n[1] += 1;
                }
                FType::Formula => {
                    let x = v.params[0];
                    let want = if v.formula == Some(FormulaKind::Fall) { reg.g * x * x / 2 } else { x * reg.g };
                    assert_eq!(t.expected(), want);
                    n[2] += 1;
                }
                FType::Seq if w.class_of(t.fid, clock) == crate::being::world2::world::SeqClass::Arith => {
                    let p = &v.params;
                    assert_eq!(t.expected(), p[p.len() - 1] + (p[p.len() - 1] - p[p.len() - 2]));
                    n[3] += 1;
                }
                FType::Compose => {
                    assert_eq!(v.base, Some(w.fams[w.fams[t.fid as usize].base.unwrap() as usize].slot));
                    assert!(v.horizon >= 2);
                    n[4] += 1;
                }
                _ => {}
            }
            assert!(t.check(t.expected()) && !t.check(t.expected() + 1));
        }
    }
    assert!(n.iter().all(|x| *x > 20), "{n:?}");
}

#[test]
fn learner_o_with_the_default_skip_test_is_the_world_v2_learner() {
    let mut st = StateO::default();
    st.start(small(4), LearnerOCfg::default(), "g".into());
    let mut e = EngineO::new(st, None);
    let mut l = LearnerOCfg::default().learner();
    for i in 0..1500 {
        let offers = offers_of_o(&e.world.offers(e.state.s2.step, e.state.s2.clock));
        let mut plain = Learner2::default();
        let a = plain.choose(&e.state.s2, &offers);
        let b = l.clone().choose(&e.state.s2, &offers);
        assert_eq!(a, b, "step {i}");
        if e.step(&mut l).is_err() {
            break;
        }
        if i % 10 == 9 {
            e.discover_pass();
        }
    }
    assert!(e.state.closed > 0 && !e.state.s2.rules.is_empty(), "families closed {} rules {}", e.state.closed, e.state.s2.rules.len());
}

fn run_logged(root: &std::path::Path, c: WOCfg, l: LearnerOCfg, steps: u64, limits: Option<Limits>) -> (VersionedLog, RunOSummary) {
    let mut log = open_log(root);
    let s = run_o(&mut log, &RunOCfg { world: Some(c), learner: Some(l), discover_every: 10, ctl: limits, max_explore_pm: None }, &Policy::new(steps), None).unwrap();
    (log, s)
}

#[test]
fn the_state_folds_from_the_log_replays_and_survives_a_restart() {
    for l in [LearnerOCfg::default(), LearnerOCfg { skip_rule: SkipRule::Calibrated, ..Default::default() }] {
        let dir = tempfile::tempdir().unwrap();
        let (log, s) = run_logged(dir.path(), small(7), l, 2500, None);
        assert_eq!(s.steps_run, 2500);
        let rows: Vec<EventRow> = log.open_latest().rows();
        let folded = StateO::from_events(&rows);
        assert_eq!(folded, s.state, "fold of the log equals the live state");
        assert!(folded.closed > 0 && folded.opened > folded.closed);
        let (n, bad) = replay_o(&rows);
        assert_eq!((n, bad), (2500, 0));
        // one run of 2500 steps equals two of 1200 + 1300 (a cold restart in between)
        let d2 = tempfile::tempdir().unwrap();
        let (_, a) = run_logged(d2.path(), small(7), l, 1200, None);
        assert_eq!(a.steps_run, 1200);
        let mut log2 = open_log(d2.path());
        let b = run_o(&mut log2, &RunOCfg { discover_every: 10, ..Default::default() }, &Policy::new(1300), None).unwrap();
        assert_eq!(b.state, s.state);
        let tuples = |rows: &[EventRow]| rows.iter().filter(|e| e.source == SOURCE_O).map(|e| (e.kind.clone(), e.text.clone(), e.tags.clone())).collect::<Vec<_>>();
        assert_eq!(tuples(&log2.open_latest().rows()), tuples(&rows));
        // the genesis records the switch only when it is on
        let g = rows.iter().find(|e| e.tags.contains("wo_genesis")).unwrap();
        assert_eq!(g.tags.contains("wo_skip_rule:calibrated"), l.skip_rule == SkipRule::Calibrated);
    }
}

#[test]
fn world_o_caps_and_the_kill_switch_hold() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let lim = Limits { max_live_rules: 1, max_candidates: 100_000, compose: true, max_blocks: 1, ..Limits::default() };
    let (log, s) = run_logged(dir.path(), small(4), LearnerOCfg::default(), 2500, Some(lim));
    assert!(s.state.s2.live_rules() <= 1);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().any(|e| e.source == SOURCE_O && e.tags.contains("cap:max_live_rules")));
    // kill before the first step, and after 12 steps
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(|| true)));
    let s = run_o(&mut log, &RunOCfg { world: Some(small(1)), ..Default::default() }, &pol, None).unwrap();
    assert_eq!((s.steps_run, s.stopped), (0, StopReason::Killed));
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let calls = Arc::new(AtomicU64::new(0));
    let c2 = calls.clone();
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 13)));
    let s = run_o(&mut log, &RunOCfg { world: Some(small(1)), discover_every: 10, ..Default::default() }, &pol, None).unwrap();
    assert_eq!((s.steps_run, s.stopped), (12, StopReason::Killed));
    let rows: Vec<EventRow> = log.open_latest().rows();
    let stops: Vec<&EventRow> = rows.iter().filter(|e| e.tags.contains("controller_stop")).collect();
    assert!(stops.len() == 1 && stops[0].tags.contains("stop:kill_switch") && stops[0].source == SOURCE_O);
    assert_eq!(StateO::from_events(&rows).s2.step, 12);
    // energy cap
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let mut pol = Policy::new(10_000);
    pol.max_energy = Some(200);
    let s = run_o(&mut log, &RunOCfg { world: Some(small(1)), ..Default::default() }, &pol, None).unwrap();
    assert_eq!(s.stopped, StopReason::EnergyCap);
    assert!(log.open_latest().rows().iter().any(|e: &EventRow| e.tags.contains("cap:max_energy_per_run")));
    // the budget ends the life
    let c = WOCfg { budget: 300, ..small(1) };
    let dir = tempfile::tempdir().unwrap();
    let (_, s) = run_logged(dir.path(), c, LearnerOCfg::default(), 100_000, None);
    assert_eq!(s.stopped, StopReason::BudgetExhausted);
    assert!(s.cost <= 300);
}

#[test]
fn world_o_rows_are_ignored_by_world_v2_and_world_o_ignores_other_rows() {
    use crate::being::world2::engine::{run_v2, Run2Cfg, State2};
    use crate::being::world2::world::W2Cfg;
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let a = run_v2(&mut log, &Run2Cfg { world: Some(W2Cfg::new(3, 12, 3000)), discover_every: 10, ..Default::default() }, &Policy::new(300), None).unwrap();
    let o = run_o(&mut log, &RunOCfg { world: Some(small(2)), discover_every: 10, ..Default::default() }, &Policy::new(300), None).unwrap();
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert_eq!(State2::from_events(&rows), a.state);
    assert_eq!(StateO::from_events(&rows), o.state);
}

#[test]
fn references_are_deterministic_and_the_knows_rules_oracle_is_the_strongest() {
    let c = small(8);
    let run = |k: bool| {
        let mut o = OracleO { world: WorldO::new(c.clone()), truth: TruthO::default(), knows_rules: k };
        run_wo(&c, LearnerOCfg::default(), Some(&mut o), 0, None, 4000, 10_000, false)
    };
    let (a, b) = (run(true), run(true));
    assert_eq!((a.true_reward, a.cost), (b.true_reward, b.cost));
    let p = run(false);
    let me = run_wo(&c, LearnerOCfg::default(), None, 10, None, 4000, 10_000, true);
    let r = |x: &super::measure::RunWO| x.true_reward as f64 / x.cost as f64;
    assert!(r(&a) > r(&p) && r(&a) > r(&me), "knows-rules {} planner {} learner {}", r(&a), r(&p), r(&me));
    assert!(me.fams.iter().any(|f| f.competent_after.is_some()));
}
