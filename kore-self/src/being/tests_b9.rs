//! Tests of Track B9 (experiments): the value of information, opt-in and off by default, the budget,
//! determinism and replay from the log, the experiment records and their citations, the kill switch,
//! controller.txt keys, source scans, and a small measured floor.

use super::controller::Controller;
use super::discover::{Example, Form, Rule};
use super::experiment::{decide, Decision, ExpBook, ExpCfg, HypCache, Opt, Probe};
use super::experiment_menu::Exp1;
use super::measure_b9::{menu_world, run_menu_b9, w2_exp_run, MenuWorld};
use super::planner::{ModelChooser, DEFAULT_BETA, DEFAULT_CURIOSITY};
use super::policy::{KillCheck, Policy, StopReason};
use super::solver::Strategy;
use super::world2::engine::{run_v2, Run2Cfg};
use super::world2::experiment::{replay_experiments2, run_v2_exp, Exp2, EXP_POLICY};
use super::world2::learner::Learner2;
use super::world2::measure::run_w2;
use super::world2::replay::{rederive_rules2, replay_decisions2};
use super::world2::world::{W2Cfg, World2, SOURCE2};
use crate::versioned::{CauseRow, EventRow, RevisionRow, VersionedLog};
use std::collections::HashSet;
use std::sync::Arc;

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn ex(id: &str, sig: u8, terms: &[i64], next: i64) -> Example {
    Example { event_id: id.into(), sig, terms: terms.to_vec(), next }
}

fn arith(a: i64, d: i64, n: usize) -> (Vec<i64>, i64) {
    let t: Vec<i64> = (0..n as i64).map(|i| a + d * i).collect();
    (t, a + d * n as i64)
}

fn learner1() -> ModelChooser {
    ModelChooser { curiosity: DEFAULT_CURIOSITY, ..ModelChooser::new(DEFAULT_BETA) }
}

// ---- the value of information ----

#[test]
fn hypotheses_split_and_support_follow_the_examples() {
    let mut c = HypCache::new();
    c.update(&[], &[]);
    let (t, next) = arith(3, 4, 8);
    let a = c.assess(1, &t, &[]);
    assert_eq!(a.live, super::discover::forms().len());
    assert!(a.eig_bits > 0.0 && a.support > 0.0 && (a.value - a.eig_bits - a.support).abs() < 1e-12);
    let mass: f64 = a.groups.iter().map(|g| g.1).sum();
    assert!((mass - 1.0).abs() < 1e-9, "{mass}");
    // every applicable hypothesis predicts the true next term of an exact arithmetic sequence
    assert!(a.groups.iter().all(|g| g.0.is_none() || g.0 == Some(next)), "{:?}", a.groups);
    // a wrong revealed answer rejects the forms that predicted the truth; the right one keeps them
    let (kept, rejected) = c.verdict(1, &t, next + 1);
    assert!(kept.is_empty() && !rejected.is_empty());
    let (kept, rejected) = c.verdict(1, &t, next);
    assert!(!kept.is_empty() && rejected.is_empty());
    // revealed examples: the support the applicable forms still need shrinks
    let exs: Vec<Example> = (0..8)
        .map(|i| {
            let (t, n) = arith(i, 2 + i % 3, 8);
            ex(&format!("e{i}"), 1, &t, n)
        })
        .collect();
    c.update(&exs, &[]);
    let b = c.assess(1, &t, &[]);
    assert!(b.support < a.support, "{} {}", b.support, a.support);
    // a wrong example removes hypotheses
    let (t2, n2) = arith(1, 1, 8);
    let mut exs2 = exs.clone();
    exs2.push(ex("bad", 1, &t2, n2 + 3));
    c.update(&exs2, &[]);
    assert!(c.assess(1, &t, &[]).live < a.live);
    // a signature whose own rule applies is resolved: value 0
    let r = Rule::new(1, Form::Poly(1), 3, 1, 0);
    let z = c.assess(1, &t, &[r.clone()]);
    assert_eq!(z.value, 0.0);
    assert_eq!(z.resolved_by, Some(r.id));
    // other signatures are not affected by these examples
    assert_eq!(c.assess(2, &t, &[]).live, super::discover::forms().len());
}

#[test]
fn incremental_cache_equals_a_fresh_one() {
    let exs: Vec<Example> = (0..30)
        .map(|i| {
            let (t, n) = arith(i - 5, (i % 4) + 1, 6 + (i as usize % 3));
            ex(&format!("e{i}"), 1 + (i % 3) as u8, &t, n + (i % 7 == 0) as i64)
        })
        .collect();
    let mut inc = HypCache::new();
    for k in 0..=exs.len() {
        inc.update(&exs[..k], &[]);
    }
    let mut fresh = HypCache::new();
    fresh.update(&exs, &[]);
    assert_eq!(inc, fresh);
    let (t, _) = arith(1, 1, 7);
    assert_eq!(inc.assess(2, &t, &[]), fresh.assess(2, &t, &[]));
}

#[test]
fn decide_respects_value_budget_and_count() {
    let mut c = HypCache::new();
    c.update(&[], &[]);
    let (t, _) = arith(0, 3, 8);
    let probes = vec![Probe { slot: 1, sig: 1, prefix: t.clone(), listed_reward: 10 }];
    let opts = vec![
        Opt { slot: 0, strategy: Strategy::Careful, p: 0.9, cost_est: 4.0, mean_score: 0.9 * 10.0 / 4.0, affordable: true, energy: 4 },
        Opt { slot: 1, strategy: Strategy::Fast, p: 0.2, cost_est: 2.0, mean_score: 0.2 * 10.0 / 2.0, affordable: true, energy: 2 },
    ];
    let go = |cfg: ExpCfg, book: &ExpBook, probes: &[Probe]| decide(&cfg, book, &c, &[], probes, &opts, 1000, 0, 0);
    // kappa 0: no value of information, the planner's choice stays
    assert!(matches!(go(ExpCfg::eig(0.0, 100, 10), &ExpBook::default(), &probes), Decision::Exploit { .. }));
    // a large kappa: an experiment on slot 1 with the cheapest strategy
    match go(ExpCfg::eig(5.0, 100, 10), &ExpBook::default(), &probes) {
        Decision::Experiment(p) => {
            assert_eq!((p.slot, p.strategy, p.energy), (1, Strategy::Fast, 2));
            assert!(p.score > p.exploit_score && p.voi > 0.0);
            assert_eq!(p.prefix, t);
        }
        _ => panic!("expected an experiment"),
    }
    // budget used up (energy share of the life, then count): refused, the planner's choice stays
    let used = ExpBook { n: 1, energy: 99, ..Default::default() };
    assert!(matches!(go(ExpCfg::eig(5.0, 100, 10), &used, &probes), Decision::Exploit { refused: Some("max_experiment_energy_pm"), .. }));
    let many = ExpBook { n: 10, ..Default::default() };
    assert!(matches!(go(ExpCfg::eig(5.0, 100, 10), &many, &probes), Decision::Exploit { refused: Some("max_experiments_per_run"), .. }));
    // zero budget (the controller default): never an experiment
    assert!(matches!(go(ExpCfg::eig(5.0, 0, 10), &ExpBook::default(), &probes), Decision::Exploit { .. }));
    assert!(matches!(go(ExpCfg::eig(5.0, 100, 0), &ExpBook::default(), &probes), Decision::Exploit { .. }));
    // nothing to probe: nothing to decide
    assert!(matches!(go(ExpCfg::eig(5.0, 100, 10), &ExpBook::default(), &[]), Decision::Exploit { considered: None, .. }));
}

#[test]
fn exp_cfg_round_trips_through_tags() {
    for c in [ExpCfg::eig(0.5, 100, 200), ExpCfg::random(7, 33, 5), ExpCfg { parts: 1, ..ExpCfg::eig(2.0, 10, 1) }] {
        assert_eq!(ExpCfg::from_tags(&c.tags().join(",")), Some(c));
    }
}

// ---- off by default, nothing changes when off ----

#[test]
fn controller_keys_default_off_and_fail_closed() {
    let d = Controller::parse("").unwrap();
    assert!(!d.experiments && d.max_experiment_energy_pm == 0 && d.max_experiments_per_run == 0);
    assert!(d.experiment_budget().is_err());
    let f = Controller::fail_closed("x");
    assert!(!f.experiments && f.max_experiment_energy_pm == 0 && f.max_experiments_per_run == 0);
    let c = Controller::parse("experiments = on\nmax_experiment_energy_pm = 100\nmax_experiments_per_run = 50\n").unwrap();
    assert_eq!(c.experiment_budget().unwrap(), (100, 50));
    let r = Controller::parse("experiments = on\n").unwrap().experiment_budget().unwrap_err();
    assert_eq!(r.cap, "max_experiment_energy_pm");
    let r = Controller::parse("max_experiment_energy_pm = 100\nmax_experiments_per_run = 50\n").unwrap().experiment_budget().unwrap_err();
    assert_eq!(r.cap, "experiments");
    for bad in ["experiments = maybe", "max_experiment_energy_pm = 501", "max_experiments_per_run = 5001", "experiments = on\nexperiments = off", "max_experiment_energy_pm = -1"] {
        assert!(Controller::parse(bad).is_err(), "{bad}");
        let f = Controller::parse_or_closed(bad);
        assert!(f.is_closed() && !f.experiments && f.max_experiment_energy_pm == 0);
    }
}

#[test]
fn zero_budget_experimenter_is_identical_to_the_current_learner_in_world_v2() {
    for seed in [1u64, 2] {
        let cfg = W2Cfg { noise_pm: 80, drift_per_10k: 20, ..W2Cfg::new(seed, 12, 1500) };
        let a = run_w2(&cfg, &mut Learner2::default(), 10, None);
        let mut ch = Exp2::new(Learner2::default(), ExpCfg::eig(10.0, 0, 1000), World2::new(cfg.clone()), cfg.budget);
        let b = run_w2(&cfg, &mut ch, 10, None);
        assert_eq!(a.recs, b.recs);
        assert_eq!(a.state, b.state);
        assert_eq!(ch.book.n, 0);
        assert!(ch.book.refused_energy > 0, "experiments were wanted and refused by the zero budget");
    }
}

#[test]
fn zero_budget_experimenter_is_identical_to_the_current_learner_in_the_menu_worlds() {
    for mw in [MenuWorld::B2, MenuWorld::B5] {
        let a = run_menu_b9(mw, 3, 1000, &mut learner1(), 10, &[]);
        let mut ch = Exp1::new(learner1(), ExpCfg::eig(10.0, 0, 1000), mw.world(3), 1000);
        ch.compose_blocks = mw == MenuWorld::B5;
        let b = run_menu_b9(mw, 3, 1000, &mut ch, 10, &[]);
        assert_eq!((a.reward, a.cost, a.steps), (b.reward, b.cost, b.steps));
        assert_eq!(a.accepted, b.accepted);
        assert_eq!(a.state.examples, b.state.examples);
        assert_eq!(ch.book.n, 0);
    }
}

#[test]
fn the_b9_runner_reproduces_the_b2_measurement_runner() {
    // the passive arm of the B9 measurement is the B2 learner: same reward, cost and rules as `run_menu_ext`
    let a = run_menu_b9(MenuWorld::B2, 5, 1200, &mut learner1(), 10, &[]);
    let b = super::measure::run_menu_ext(super::world::World::with_extended(5, true), 1200, &mut learner1(), 10, &[]);
    assert_eq!((a.reward, a.cost), (b.run.reward, b.run.cost));
    assert_eq!(a.accepted.iter().map(|x| (x.0, x.2.clone())).collect::<Vec<_>>(), b.accepted);
}

// ---- logged runs: determinism, replay, records, citations, budget, kill ----

fn exp_run(root: &std::path::Path, seed: u64, steps: u64, exp: ExpCfg) -> (VersionedLog, super::world2::engine::Run2Summary, usize) {
    let mut log = open(root);
    let cfg = Run2Cfg { world: Some(W2Cfg::new(seed, 12, 3000)), learner: None, discover_every: 10, ctl: None };
    let (s, recs) = run_v2_exp(&mut log, &cfg, &Policy::new(steps), exp).unwrap();
    (log, s, recs.len())
}

#[test]
fn logged_experiment_runs_are_deterministic_replay_and_cite_their_evidence() {
    let d1 = tempfile::tempdir().unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let exp = ExpCfg::eig(0.5, 100, 1000);
    let (l1, s1, n1) = exp_run(d1.path(), 4, 400, exp);
    let (l2, _, n2) = exp_run(d2.path(), 4, 400, exp);
    let r1: Vec<EventRow> = l1.open_latest().rows();
    let r2: Vec<EventRow> = l2.open_latest().rows();
    assert_eq!(r1.iter().map(|e| e.event_id.clone()).collect::<Vec<_>>(), r2.iter().map(|e| e.event_id.clone()).collect::<Vec<_>>());
    assert_eq!(n1, n2);
    assert!(n1 > 0, "this world yields experiments");
    // the measurement (no log) makes at least as many experiments over the whole life
    let (_, book, _) = w2_exp_run(&W2Cfg::new(4, 12, 3000), exp, None);
    assert!(book.n >= n1 as u64);
    // replay: every decision of the experimenter re-derived from the log, the learner's own decisions by the existing replay, every rule re-derived
    let rx = replay_experiments2(&r1);
    assert_eq!(rx.len(), 400);
    assert!(rx.iter().all(|r| r.1 == r.2), "{:?}", rx.iter().find(|r| r.1 != r.2));
    assert_eq!(rx.iter().filter(|r| r.1.starts_with(EXP_POLICY)).count(), n1);
    let rd = replay_decisions2(&r1);
    assert_eq!(rd.len(), 400 - n1);
    assert!(rd.iter().all(|r| r.matches()));
    let rr = rederive_rules2(&r1);
    assert!(!rr.is_empty() && rr.iter().all(|r| r.1), "{rr:?}");
    // records: one experiment note and revision per experiment, citing its decision, outcome and note
    let snap = l1.open_latest();
    let causes: Vec<CauseRow> = snap.rows();
    let revs: Vec<RevisionRow> = snap.rows();
    let ids: HashSet<String> = r1.iter().map(|e| e.event_id.clone()).collect();
    let notes: Vec<&EventRow> = r1.iter().filter(|e| e.source == SOURCE2 && e.kind == "note" && e.tags.split(',').any(|t| t == "experiment")).collect();
    assert_eq!(notes.len(), n1);
    let erevs: Vec<&RevisionRow> = revs.iter().filter(|r| r.key.starts_with("being/world2/experiment/")).collect();
    assert_eq!(erevs.len(), n1);
    let dec_ids: HashSet<String> = r1.iter().filter(|e| e.kind == "decision" && super::state::tag(&e.tags, "policy") == Some(EXP_POLICY)).map(|e| e.event_id.clone()).collect();
    let out_ids: HashSet<String> = r1.iter().filter(|e| e.kind == "outcome").map(|e| e.event_id.clone()).collect();
    let note_ids: HashSet<String> = notes.iter().map(|e| e.event_id.clone()).collect();
    for r in &erevs {
        let c: Vec<&CauseRow> = causes.iter().filter(|c| c.rev_id == r.rev_id).collect();
        assert!(c.iter().any(|c| dec_ids.contains(&c.event_id)), "cites its decision");
        assert!(c.iter().any(|c| out_ids.contains(&c.event_id)), "cites its outcome");
        assert!(c.iter().any(|c| note_ids.contains(&c.event_id)), "cites its note");
        assert!(c.iter().all(|c| ids.contains(&c.event_id)));
        assert!(r.rationale.contains("predicted_split") && r.rationale.contains("rejected") && r.rationale.contains("kept"));
    }
    // the experiment decision revisions carry the hypotheses, the predicted split and the budget
    let choice: Vec<&RevisionRow> = revs.iter().filter(|r| r.key.starts_with("being/world2/choice/") && r.author.contains(EXP_POLICY)).collect();
    assert_eq!(choice.len(), n1);
    assert!(choice.iter().all(|r| r.rationale.contains("heaviest_hypotheses") && r.rationale.contains("predicted_split") && r.rationale.contains("energy_pm_of_life")));
    // a rule that rests on an experiment's example has a credit revision citing the rule and the experiment notes
    let credits: Vec<&RevisionRow> = revs.iter().filter(|r| r.key.starts_with("being/world2/experiment_credit/")).collect();
    let rule_ids: HashSet<String> = r1.iter().filter(|e| e.kind == "note" && e.tags.split(',').any(|t| t == "world2") && e.tags.split(',').any(|t| t == "rule")).map(|e| e.event_id.clone()).collect();
    assert!(!credits.is_empty(), "some rule rests on an experiment in this world");
    for r in &credits {
        let c: Vec<&CauseRow> = causes.iter().filter(|c| c.rev_id == r.rev_id).collect();
        assert!(c.iter().any(|c| rule_ids.contains(&c.event_id)));
        assert!(c.iter().any(|c| note_ids.contains(&c.event_id)));
    }
    // budget: experiment energy within the cap of the life budget
    let spent = super::world2::experiment::experiment_energy_from_log(&r1);
    assert!(spent <= 100 * 3000 / 1000, "{spent}");
    assert_eq!(s1.state.step, 400);
}

#[test]
fn experiment_budget_refusals_are_logged_and_caps_hold() {
    let d = tempfile::tempdir().unwrap();
    let (log, s, n) = exp_run(d.path(), 6, 300, ExpCfg::eig(2.0, 30, 4));
    assert!(n <= 4);
    let rows: Vec<EventRow> = log.open_latest().rows();
    let refusals: Vec<&EventRow> = rows.iter().filter(|e| e.tags.contains("controller_refusal") && (e.tags.contains("cap:max_experiments_per_run") || e.tags.contains("cap:max_experiment_energy_pm"))).collect();
    assert!(!refusals.is_empty(), "the small budget was hit and the refusal logged");
    assert!(s.refusals.iter().any(|r| r.cap.starts_with("max_experiment")));
    assert!(super::world2::experiment::experiment_energy_from_log(&rows) <= 30 * 3000 / 1000);
}

#[test]
fn kill_switch_stops_an_experiment_run_before_the_next_step() {
    let d = tempfile::tempdir().unwrap();
    let mut log = open(d.path());
    let cfg = Run2Cfg { world: Some(W2Cfg::new(2, 12, 3000)), learner: None, discover_every: 10, ctl: None };
    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let c2 = calls.clone();
    let mut pol = Policy::new(500);
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 >= 21)));
    let (s, _) = run_v2_exp(&mut log, &cfg, &pol, ExpCfg::eig(0.5, 100, 1000)).unwrap();
    assert_eq!(s.stopped, StopReason::Killed);
    assert_eq!(s.steps_run, 20);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().any(|e| e.tags.contains("stop:kill_switch")));
    let mut pol = Policy::new(500);
    pol.kill = Some(KillCheck(Arc::new(|| true)));
    let d2 = tempfile::tempdir().unwrap();
    let mut log2 = open(d2.path());
    let (s, recs) = run_v2_exp(&mut log2, &cfg, &pol, ExpCfg::eig(0.5, 100, 1000)).unwrap();
    assert_eq!((s.steps_run, recs.len()), (0, 0));
}

#[test]
fn plain_world_v2_runs_write_no_experiment_records() {
    let d1 = tempfile::tempdir().unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let mut a = open(d1.path());
    let mut b = open(d2.path());
    let cfg = Run2Cfg { world: Some(W2Cfg::new(3, 12, 3000)), learner: None, discover_every: 10, ctl: None };
    run_v2(&mut a, &cfg, &Policy::new(120), None).unwrap();
    run_v2(&mut b, &cfg, &Policy::new(120), None).unwrap();
    let ra: Vec<EventRow> = a.open_latest().rows();
    let rb: Vec<EventRow> = b.open_latest().rows();
    assert_eq!(ra.iter().map(|e| e.event_id.clone()).collect::<Vec<_>>(), rb.iter().map(|e| e.event_id.clone()).collect::<Vec<_>>());
    assert!(!ra.iter().any(|e| e.tags.contains("experiment")));
}

// ---- source scans ----

#[test]
fn experiment_sources_read_no_hidden_parameter() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being");
    let hidden = ["regime", "Regime", ".expected(", "slip_noise", "class_of", "noise_mult", "slip_pm", "truth", "feedback(", "wall_pm", "SeqClass", "Kind::", "task_of_kind", "task_forced", "view.kind", "profile", "check("];
    for f in ["experiment.rs", "experiment_menu.rs", "world2/experiment.rs"] {
        let full = std::fs::read_to_string(dir.join(f)).unwrap();
        // the chooser part (the logged run loop below it handles the engine's measurement record as run_v2 does)
        let src = full.split("// ---- the logged run ----").next().unwrap();
        for b in hidden {
            assert!(!src.contains(b), "{f} mentions {b}");
        }
    }
    // the core does not even name a world
    let core = std::fs::read_to_string(dir.join("experiment.rs")).unwrap();
    for b in ["World", "world2", ".offers("] {
        assert!(!core.contains(b), "experiment.rs mentions {b}");
    }
}

// ---- measured floor (small) ----

#[test]
fn measured_experiments_find_rules_faster_without_wrong_rules() {
    // seeds 1..3, B2 world budget 3000 (the full report is in docs/measured.md, Track B9)
    let (mut found_p, mut found_e, mut wrong, mut frac_p, mut frac_e) = (0, 0, 0, 0.0, 0.0);
    for seed in 1..=3u64 {
        let arms = menu_world(MenuWorld::B2, seed, 3000, super::experiment::DEFAULT_KAPPA, super::measure_b9::DEFAULT_CAP_PM, 5000, false);
        let (p, e) = (&arms[0], &arms[1]);
        found_p += p.speed.found;
        found_e += e.speed.found;
        wrong += p.speed.wrong + e.speed.wrong;
        frac_p += p.speed.energy_frac;
        frac_e += e.speed.energy_frac;
        assert!(e.exp.energy as f64 <= super::measure_b9::DEFAULT_CAP_PM as f64 * 3.0 + 1e-9);
    }
    assert_eq!(wrong, 0);
    assert!(found_e >= found_p, "{found_e} {found_p}");
    assert!(frac_e <= frac_p, "{frac_e} {frac_p}");
}
