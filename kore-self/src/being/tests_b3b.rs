//! Tests of Track B3b: the per-cell adaptive learner (mode `switch`), its logging, replay and limits.

use super::adapt_b3b::{SwitchCfg, DEFAULT_SWITCH};
use super::controller::Limits;
use super::measure_b3b::{run_arm, Arm};
use super::model::{AdaptMode, Detector, Model, Obs};
use super::policy::KillCheck;
use super::replay::replay_decisions;
use super::run_loop::{run, RunCfg};
use super::solver::{ScriptedThinker, Strategy};
use super::state::{tag, BeingState, Chooser};
use super::world::Domain;
use super::{Policy, StopReason};
use crate::versioned::{CauseRow, EventRow, RevisionRow, VersionedLog};

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

const BUDGET: i64 = 2000;

fn cfg(world: u64, steps: u64, mode: Option<AdaptMode>) -> RunCfg {
    RunCfg {
        world_seed: Some(world),
        loop_seed: Some(1),
        budget: Some(BUDGET),
        chooser: Some(Chooser::Model),
        steps,
        drift: Some(true),
        adapt: Some(mode.is_some()),
        adapt_mode: mode,
        ..Default::default()
    }
}

fn logged(root: &std::path::Path, world: u64, steps: u64) -> (VersionedLog, super::run_loop::RunSummary) {
    let mut log = open(root);
    let s = run(&mut log, &cfg(world, steps, Some(AdaptMode::Switch)), &ScriptedThinker, &Policy::new(steps)).unwrap();
    (log, s)
}

#[test]
fn switch_mode_with_its_mechanisms_off_is_the_b1_learner_bit_for_bit() {
    let none = SwitchCfg { detect: false, prior_check: false, ..DEFAULT_SWITCH };
    for seed in [3u64, 7] {
        let a = run_arm(seed, 1500, true, &Arm::b1());
        let b = run_arm(seed, 1500, true, &Arm::switch(none, 0.0));
        assert_eq!((a.run.reward, a.run.cost, a.run.recs.len()), (b.run.reward, b.run.cost, b.run.recs.len()), "seed {seed}");
        for (x, y) in a.run.recs.iter().zip(&b.run.recs) {
            assert_eq!((x.domain, x.strategy, x.correct, x.cost), (y.domain, y.strategy, y.correct, y.cost));
        }
        assert!(b.flags.is_empty());
    }
}

#[test]
fn a_cell_switch_replaces_old_evidence_by_the_window_and_a_rejected_prior_loses_weight() {
    let mut m = Model::new(AdaptMode::Switch, 1e9);
    m.sw_cfg = DEFAULT_SWITCH;
    let o = |ok: bool| Obs { domain: Domain::Algebra, difficulty: 1, strategy: Strategy::Fast, correct: ok, listed_cost: 4, spent: 2 };
    let mut alarms = vec![];
    for i in 0..60 {
        alarms.extend(m.observe(&o(i % 10 != 0)));
        alarms.extend(m.take_extra_alarms());
    }
    assert!(alarms.is_empty(), "a steady cell at 90% flags nothing: {alarms:?}");
    let before = m.estimate(Domain::Algebra, 1, Strategy::Fast);
    assert!(before.p > 0.8 && before.n == 60);
    let mut hit = None;
    for i in 0..12 {
        if let Some(a) = m.observe(&o(false)) {
            hit = Some((i, a));
            break;
        }
    }
    let (i, a) = hit.expect("the drop is flagged");
    assert!(i < 12 && a.kind == Detector::CellSwitch && a.drop && a.cell == Some(1));
    let after = m.estimate(Domain::Algebra, 1, Strategy::Fast);
    assert_eq!(after.n, DEFAULT_SWITCH.window as u64, "the cell now holds only its recent window");
    assert!(after.p < 0.5, "estimate follows the recent window: {}", after.p);
    // a fresh cell whose outcomes contradict an optimistic prior: the prior is rejected and loses weight
    let mut m = Model::new(AdaptMode::Switch, 1e9);
    for _ in 0..40 {
        m.observe(&Obs { domain: Domain::Units, difficulty: 1, strategy: Strategy::Fast, correct: true, listed_cost: 4, spent: 2 });
    }
    let fresh = |m: &Model| m.estimate(Domain::Units, 5, Strategy::Fast).p;
    let p0 = fresh(&m);
    assert!(p0 > 0.8, "the prior of an untried hard cell comes from the easy cells of its domain: {p0}");
    let mut kinds = vec![];
    for _ in 0..3 {
        kinds.extend(m.observe(&Obs { domain: Domain::Units, difficulty: 5, strategy: Strategy::Fast, correct: false, listed_cost: 8, spent: 4 }).map(|a| a.kind));
    }
    assert!(kinds.contains(&Detector::PriorConflict) && m.prior_rejected(Domain::Units, 5, Strategy::Fast));
    let mut b1 = Model::new(AdaptMode::Off, 1e9);
    for _ in 0..40 {
        b1.observe(&Obs { domain: Domain::Units, difficulty: 1, strategy: Strategy::Fast, correct: true, listed_cost: 4, spent: 2 });
    }
    for _ in 0..3 {
        b1.observe(&Obs { domain: Domain::Units, difficulty: 5, strategy: Strategy::Fast, correct: false, listed_cost: 8, spent: 4 });
    }
    assert!(fresh(&m) + 0.2 < fresh(&b1), "after 3 failures B3b believes {:.3}, B1 still {:.3}", fresh(&m), fresh(&b1));
}

/// A world whose logged B3b being flags changes of both kinds within its life.
fn world_with_flags() -> u64 {
    (1..=12u64)
        .find(|w| {
            let r = run_arm(*w, BUDGET, true, &Arm::switch(DEFAULT_SWITCH, 0.0));
            r.flags.iter().any(|f| f.1 == Detector::PriorConflict) && r.flags.iter().any(|f| f.1 == Detector::CellSwitch)
        })
        .expect("some world flags both kinds")
}

#[test]
fn b3b_run_is_deterministic_replays_exactly_and_logs_its_changes_with_evidence() {
    let world = world_with_flags();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (la, sa) = logged(a.path(), world, 3000);
    let (lb, _) = logged(b.path(), world, 3000);
    assert_eq!(sa.state.model.mode(), AdaptMode::Switch);
    assert_eq!(la.open_latest().content_hash(), lb.open_latest().content_hash(), "same seeds, same log");
    let rows: Vec<EventRow> = la.open_latest().rows();
    let g = rows.iter().find(|e| e.tags.contains("genesis")).expect("genesis");
    assert!(g.tags.contains("adapt:switch") && g.text.contains("recent window"));
    let rep = replay_decisions(&rows);
    assert_eq!(rep.len() as u64, sa.state.step);
    assert!(rep.iter().all(|r| r.matches()), "a decision of the B3b learner was not re-derived from the log");
    let refolded = BeingState::from_events(&rows);
    assert_eq!(refolded.model, sa.state.model);
    assert_eq!(refolded.changes, sa.state.changes);
    let kinds: Vec<Detector> = sa.state.changes.iter().map(|c| c.alarm.kind).collect();
    assert!(kinds.contains(&Detector::PriorConflict) && kinds.contains(&Detector::CellSwitch), "{kinds:?}");
    assert!(!kinds.contains(&Detector::Group));
    let snap = la.open_latest();
    let revs: Vec<RevisionRow> = snap.rows();
    let causes: Vec<CauseRow> = snap.rows();
    let notes: Vec<&EventRow> = rows.iter().filter(|e| e.kind == "note" && e.tags.split(',').any(|t| t == "change")).collect();
    assert_eq!(notes.len(), sa.state.changes.len());
    for (n, c) in notes.iter().zip(&sa.state.changes) {
        assert_eq!(tag(&n.tags, "change_kind"), Some(c.alarm.kind.name()));
        assert_eq!(tag(&n.tags, "difficulty"), c.alarm.cell.map(|k| k.to_string()).as_deref());
        let key = tag(&n.tags, "rev_key").unwrap();
        let rev = revs.iter().find(|r| r.key == key).expect("revision of the change");
        assert_eq!((rev.entity_type.as_str(), rev.author.as_str()), ("belief", "rule:being-detector"));
        let cited: Vec<&EventRow> = causes.iter().filter(|x| x.rev_id == rev.rev_id).filter_map(|x| rows.iter().find(|e| e.event_id == x.event_id)).collect();
        let outcomes: Vec<&&EventRow> = cited.iter().filter(|e| e.kind == "outcome").collect();
        assert!(!outcomes.is_empty() && outcomes.len() == c.cites.len(), "a change cites the outcomes it rests on");
        for o in outcomes {
            assert!(o.tags.contains(&format!("domain:{}", c.alarm.domain.name())) && o.tags.contains(&format!("strategy:{}", c.alarm.strategy.name())));
        }
        assert!(cited.iter().any(|e| e.event_id == n.event_id));
    }
    // a run split over two calls equals one run
    let d2 = tempfile::tempdir().unwrap();
    let (mut ls, _) = logged(d2.path(), world, 200);
    run(&mut ls, &RunCfg { steps: 3000, ..Default::default() }, &ScriptedThinker, &Policy::new(3000)).unwrap();
    let ids = |l: &VersionedLog| l.open_latest().rows::<EventRow>().into_iter().map(|e| e.event_id).collect::<Vec<_>>();
    assert_eq!(ids(&ls), ids(&la));
}

#[test]
fn adapt_mode_is_fixed_at_creation_and_needs_adapt() {
    let d = tempfile::tempdir().unwrap();
    let mut log = open(d.path());
    let bad = RunCfg { adapt: Some(false), ..cfg(1, 5, Some(AdaptMode::Switch)) };
    assert!(run(&mut log, &bad, &ScriptedThinker, &Policy::new(5)).is_err(), "adapt_mode without adapt is refused");
    let bad = cfg(1, 5, Some(AdaptMode::Forget));
    assert!(run(&mut log, &bad, &ScriptedThinker, &Policy::new(5)).is_err(), "only full or switch");
    assert!(BeingState::from_snapshot(&log.open_latest()).genesis.is_none(), "a refused request creates no being");
    run(&mut log, &cfg(1, 5, Some(AdaptMode::Switch)), &ScriptedThinker, &Policy::new(5)).unwrap();
    let other = RunCfg { adapt_mode: Some(AdaptMode::Full), steps: 1, ..Default::default() };
    assert!(run(&mut log, &other, &ScriptedThinker, &Policy::new(1)).is_err(), "the mode is kept once the being exists");
    assert!(run(&mut log, &RunCfg { adapt_mode: Some(AdaptMode::Switch), steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1)).is_ok());
    // without adapt_mode, adapt: true is still the B3 learner
    let d = tempfile::tempdir().unwrap();
    let mut log = open(d.path());
    let s = run(&mut log, &RunCfg { adapt: Some(true), adapt_mode: None, ..cfg(1, 5, None) }, &ScriptedThinker, &Policy::new(5)).unwrap();
    assert_eq!(s.state.model.mode(), AdaptMode::Full);
}

#[test]
fn kill_switch_and_energy_cap_hold_for_the_b3b_learner() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let d = tempfile::tempdir().unwrap();
    let mut log = open(d.path());
    let calls = Arc::new(AtomicU64::new(0));
    let c2 = calls.clone();
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 13)));
    let c = RunCfg { ctl: Some(Limits::default()), ..cfg(2, 100, Some(AdaptMode::Switch)) };
    let s = run(&mut log, &c, &ScriptedThinker, &pol).unwrap();
    assert_eq!((s.steps_run, s.stopped), (12, StopReason::Killed), "the kill switch is checked before every step");
    let ev: Vec<EventRow> = log.open_latest().rows();
    assert!(ev.iter().any(|e| e.tags.contains("controller_stop") && e.tags.contains("stop:kill_switch")));
    let d = tempfile::tempdir().unwrap();
    let mut log = open(d.path());
    let mut pol = Policy::new(1000);
    pol.max_energy = Some(120);
    let c = RunCfg { ctl: Some(Limits::default()), ..cfg(2, 1000, Some(AdaptMode::Switch)) };
    let s = run(&mut log, &c, &ScriptedThinker, &pol).unwrap();
    assert_eq!(s.stopped, StopReason::EnergyCap);
    assert!(s.cost >= 120 && s.cost < 150, "cost {}", s.cost);
    let ev: Vec<EventRow> = log.open_latest().rows();
    assert!(ev.iter().any(|e| e.tags.contains("cap:max_energy_per_run") && e.tags.contains("controller_refusal")));
}

#[test]
fn b3b_sources_read_only_the_log() {
    let forbidden = [
        "Profile", "profile", "reward_scale", "cost_base", "diff_bias", ".expected(", "noise(", "task_forced", "task_of_kind", "truth", "World", "generate",
        "Kind::", "Drift", "regime", "FAST_DEGRADED", "degraded", "schedule", "DRIFT_FRACTIONS", "offers_at", "pattern_bonus", "std::fs", "std::net", "File::", "TcpStream", "Command",
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being");
    for f in ["adapt_b3b.rs", "model.rs"] {
        let src = std::fs::read_to_string(dir.join(f)).unwrap();
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for b in forbidden {
            assert!(!code.contains(b), "{f} mentions {b}");
        }
    }
}

#[test]
fn measured_b3b_floors_on_report_seeds_1_to_4() {
    // floors set below the values measured on these seeds (docs/measured.md, Track B3b); budget 6000
    use super::measure_b3b::window;
    let (mut dw, mut dp, mut ds, mut switched) = (vec![], vec![], vec![], 0);
    for seed in 1..=4u64 {
        let at = super::measure_b3b::at(seed, 6000);
        let b1 = run_arm(seed, 6000, true, &Arm::b1());
        let b3b = run_arm(seed, 6000, true, &Arm::switch(DEFAULT_SWITCH, 0.0));
        let post = |x: &super::measure_b3b::XRun| (0..3).map(|i| window(&x.run, at[i], at[i] + 300)).sum::<f64>() / 3.0;
        dw.push(b3b.per_budget() - b1.per_budget());
        dp.push(post(&b3b) - post(&b1));
        let s1 = run_arm(seed, 6000, false, &Arm::b1());
        let s2 = run_arm(seed, 6000, false, &Arm::switch(DEFAULT_SWITCH, 0.0));
        ds.push(s2.per_budget() - s1.per_budget());
        assert!(s2.flags.iter().all(|f| f.1 != Detector::Group));
        switched += b3b.flags.iter().any(|f| f.1 == Detector::CellSwitch && f.3 == Strategy::Fast && f.0 >= at[2]) as usize;
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    eprintln!("B3b floors: whole {:?} post {:?} stationary {:?} switched {switched}", dw, dp, ds);
    // measured on seeds 1..4: whole life mean -0.009, post-change windows mean +0.013 (these four seeds are below the 20-seed means)
    assert!(mean(&dw) > -0.05, "whole life {:?}", dw);
    assert!(mean(&dp) > -0.05, "post-change windows {:?}", dp);
    assert!(mean(&ds) > -0.03, "stationary {:?}", ds);
    assert!(switched >= 2, "the degradation of fast is flagged by a cell switch in at least 2 of 4 worlds, got {switched}");
}
