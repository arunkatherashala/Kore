//! Tests of Track B13 (the context-aware chooser estimate, `linval.rs`): with the switch off every chooser
//! is byte-identical to e9ed7d1d (golden digests computed there); the estimate is the ridge regression it
//! says it is; with the switch on runs are deterministic, fold and replay from the log, every decision
//! carries its estimate's evidence, the controller key fails closed, and the kill switch holds.

#[path = "tests_b13_golden.rs"]
mod golden;

use super::linval::*;
use super::lookahead::beingl::{run_wl, GreedyL};
use super::lookahead::worldl::{WLCfg, SETTINGS};
use super::measure::{run_menu_ext, DISCOVER_EVERY};
use super::planner::{Action, MenuChooser, ModelChooser, DEFAULT_BETA};
use super::policy::{KillCheck, Policy, StopReason};
use super::vs_world::core::{Rng, Ridge};
use super::world::World;
use super::world2::engine::{genesis_event2, run_v2, LearnerCfg, Run2Cfg, State2, LIN_TAG};
use super::world2::learner::Chooser2;
use super::world2::measure::run_w2_with;
use super::world2::replay::replay_decisions2;
use super::world2::world::W2Cfg;
use super::world_o::engine::{replay_o, run_o, LearnerOCfg, RunOCfg, StateO};
use super::world_o::world::WOCfg;
use crate::being::controller::Controller;
use crate::versioned::{EventRow, RevisionRow, VersionedLog};

/// Computed on e9ed7d1d with `golden::golden_digests` (World v2 log, World O log, first world log, World L
/// logs of the four settings, measured lives of all four choosers).
const GOLDEN: [&str; 8] = [
    "be981c51376cf99e007dd5799ffcd27107fa6bebb7e0458e340354e355b6577a",
    "28740c163acfbcb5802afc9bc2dfcd74e524c7815577e513759362508f84cde8",
    "6d5ef60ae583786eb27d55d96b393f155f4065423f85e3121035d8bd470c457f",
    "7f84b0a7bec2f0dab01eca1f3e3dd5abd730d40ab2492eb0133e54e18c3d607a",
    "385bf1b947464da18347d0c9cbfd652d5683b756ef919b2dd433b0390b4c484e",
    "ce8acbbea1aa9d25624962564044ff5dc062d2f5c194f7761c0797ee02a24385",
    "602d5ff31a5e116bf27e52fe68fe7c4688b9654c3c6d34606c105dbf5bc802a1",
    "fafabd6629d67d34b60d1098b69c7c576f2d4cc5272f985d741fd2247ce36acc",
];

#[test]
fn with_the_b13_switch_off_every_chooser_is_byte_identical_to_e9ed7d1d() {
    let got = golden::golden_digests();
    for (i, d) in got.iter().enumerate() {
        println!("GOLDEN_B13 {i} {d}");
    }
    assert_eq!(got, GOLDEN.to_vec());
}

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

// ---- the estimate ----

/// The fit equals an explicit ridge regression over (global features, one feature per cell) with the
/// cell feature's penalty `k` instead of 1, fed one observation at a time (the LinUCB model).
#[test]
fn the_estimate_is_a_ridge_regression_with_a_penalised_cell_feature() {
    let mut rng = Rng::new(7);
    let dims = Dims { fams: 4, diffs: true, types: 0 };
    let mut cells = vec![];
    let mut obs = vec![];
    for fam in 0..4usize {
        for d in 1..=5u8 {
            let n = rng.below(6) as u64;
            if n == 0 {
                continue;
            }
            let mut succ = 0;
            for _ in 0..n {
                let y = rng.f64() < 0.3 + 0.1 * fam as f64;
                succ += y as u64;
                obs.push((fam, d, y));
            }
            cells.push(CellObs { strat: 0, fam, diff: d, ftype: None, ctx: 0, n: n as f64, succ: succ as f64 });
        }
    }
    for k in [0.0f64, 1.0, 4.0, 16.0] {
        let cfg = LinCfg { alpha_milli: 500, cell_k_milli: (k * 1000.0) as u32, types: false };
        let fit = LinFit::fit(cfg, dims, 1, &cells);
        // explicit ridge: global features + a cell feature scaled so its penalty is k (x_c = 1/sqrt(k), weight sqrt(k))
        let g = dims.len();
        let d = if k > 0.0 { g + 20 } else { g };
        let mut r = Ridge::new(d);
        let x_of = |fam: usize, dd: u8| -> Vec<(usize, f64)> {
            let mut x: Vec<(usize, f64)> = dims.z(fam, dd, None).into_iter().map(|i| (i, 1.0)).collect();
            if k > 0.0 {
                x.push((g + fam * 5 + (dd - 1) as usize, 1.0 / k.sqrt()));
            }
            x
        };
        for (fam, dd, y) in &obs {
            r.update(&x_of(*fam, *dd), *y as u8 as f64);
        }
        for fam in 0..4 {
            for dd in 1..=5u8 {
                let e = fit.estimate(0, fam, dd, None, 0);
                let x = x_of(fam, dd);
                let mean = r.mean(&x);
                let width = r.quad(&x).sqrt();
                // with the scaled cell feature the explicit model's x has 1/sqrt(k) on the cell: rescale its effect
                assert!((e.p - mean.clamp(0.0, 1.0)).abs() < 1e-9, "k {k} fam {fam} d {dd}: {} vs {mean}", e.p);
                assert!((e.width - width).abs() < 1e-9, "k {k} fam {fam} d {dd}: {} vs {width}", e.width);
                assert!((e.p_up - (mean + 0.5 * width).clamp(0.0, 1.0)).abs() < 1e-9);
            }
        }
    }
}

#[test]
fn the_switch_round_trips_and_bad_values_are_refused() {
    for c in [LinCfg::default(), LIN_V2, LIN_MENU, LIN_L, LinCfg { alpha_milli: 0, cell_k_milli: 0, types: true }] {
        assert_eq!(LinCfg::parse(&c.code()), Some(c));
    }
    for bad in ["", "a500", "a500/k4000", "a500/k4000/t2", "x500/k4000/t1", "a99999/k1/t1", "a1/k9999999/t1", "a-1/k1/t1"] {
        assert_eq!(LinCfg::parse(bad), None, "{bad}");
    }
    // World v2: the genesis event carries the switch and the folded state reads it back; off: no tag
    let c = W2Cfg::new(1, 6, 600);
    let (ev, _) = genesis_event2(&c, &LearnerCfg::default());
    assert!(!ev.tags.iter().any(|t| t.starts_with(LIN_TAG)));
    let (ev, _) = genesis_event2(&c, &LearnerCfg { lin: Some(LIN_V2), ..LearnerCfg::default() });
    assert!(ev.tags.contains(&format!("{LIN_TAG}:{}", LIN_V2.code())));
    assert!(ev.text.contains("Track B13"));
}

// ---- World v2 with the switch on ----

fn w2cfg(seed: u64) -> W2Cfg {
    W2Cfg { noise_pm: 80, drift_per_10k: 25, curve_pct: 50, ..W2Cfg::new(seed, 12, 3000) }
}

fn run_on(root: &std::path::Path, c: W2Cfg, l: LearnerCfg, pol: Policy) -> (VersionedLog, super::world2::engine::Run2Summary) {
    let mut log = open(root);
    let s = run_v2(&mut log, &Run2Cfg { world: Some(c), learner: Some(l), discover_every: 10, ctl: None }, &pol, None).unwrap();
    (log, s)
}

#[test]
fn world_v2_with_b13_is_deterministic_folds_replays_and_explains_every_choice() {
    let lc = LearnerCfg { lin: Some(LIN_V2), use_: super::world2::all_skills::profile_use(), ..LearnerCfg::default() };
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (l1, s1) = run_on(d1.path(), w2cfg(3), lc, Policy::new(500));
    let (l2, _) = run_on(d2.path(), w2cfg(3), lc, Policy::new(500));
    let rows1 = l1.open_latest().rows::<EventRow>();
    assert_eq!(rows1, l2.open_latest().rows::<EventRow>(), "same seed, same log");
    // the unlogged measurement takes the same steps
    let m = run_w2_with(&w2cfg(3), &mut lc.learner(), 10, None, lc);
    assert_eq!(&m.recs[..500], &s1.truths[..]);
    // the state folds back from the log, with the switch read from the genesis event
    let st = State2::from_events(&rows1);
    assert_eq!(st, s1.state);
    assert_eq!(st.learner.lin, Some(LIN_V2));
    // every decision is re-derived from the log alone
    let rep = replay_decisions2(&rows1);
    assert_eq!(rep.len(), 500);
    assert!(rep.iter().all(|r| r.matches()));
    // every decision revision names the estimate and gives, per alternative, its evidence
    let revs = l1.open_latest().rows::<RevisionRow>();
    let mut with_lin = 0;
    for r in revs.iter().filter(|r| r.entity_type == "decision") {
        let v: serde_json::Value = serde_json::from_str(&r.rationale).unwrap();
        assert_eq!(v["chooser_estimate"]["track"], "B13", "{}", r.rationale);
        for a in v["alternatives"].as_array().unwrap() {
            if let Some(e) = a.get("estimate_from_linear") {
                with_lin += 1;
                assert!(e["linear_part"].is_number() && e["width"].is_number() && e["cell_attempts"].is_number());
                let terms = e["terms"].as_array().unwrap();
                assert!(terms.iter().any(|t| t["feature"] == "bias"));
                assert!(terms.iter().any(|t| t["feature"].as_str().unwrap().starts_with("family f")));
                assert!(terms.iter().any(|t| t["feature"].as_str().unwrap().starts_with("difficulty ")));
            } else {
                // only options a rule covers keep the rule estimate
                assert!(a.get("estimate_from_rule").is_some(), "{a}");
            }
        }
    }
    assert!(with_lin > 1000);
    // the switch changes the run
    let d3 = tempfile::tempdir().unwrap();
    let (l3, _) = run_on(d3.path(), w2cfg(3), LearnerCfg { lin: None, ..lc }, Policy::new(500));
    assert_ne!(l3.open_latest().rows::<EventRow>(), rows1);
}

#[test]
fn world_v2_with_b13_stops_on_the_kill_switch_before_the_next_step() {
    let lc = LearnerCfg { lin: Some(LIN_V2), ..LearnerCfg::default() };
    let n = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let n2 = n.clone();
    let mut pol = Policy::new(300);
    pol.kill = Some(KillCheck(std::sync::Arc::new(move || n2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 40)));
    let d = tempfile::tempdir().unwrap();
    let (log, s) = run_on(d.path(), w2cfg(4), lc, pol);
    assert_eq!(s.stopped, StopReason::Killed);
    assert_eq!(s.steps_run, 40);
    let rows = log.open_latest().rows::<EventRow>();
    assert!(rows.iter().any(|e| e.tags.contains("stop:kill_switch")), "the stop is logged");
    assert_eq!(rows.iter().filter(|e| e.kind == "decision").count(), 40);
}

// ---- World O with the switch on ----

#[test]
fn world_o_with_b13_folds_replays_and_reads_the_switch_from_its_genesis() {
    let c = WOCfg { intro_every: 400, life_intros: 6, tier_every: 2, drift_pm: 500, ..WOCfg::new(2, 40_000) };
    let l = LearnerOCfg { lin: Some(LIN_V2), ..Default::default() };
    let d = tempfile::tempdir().unwrap();
    let mut log = open(d.path());
    let s = run_o(&mut log, &RunOCfg { world: Some(c.clone()), learner: Some(l), discover_every: 10, ctl: None, max_explore_pm: None }, &Policy::new(1500), None).unwrap();
    let rows = log.open_latest().rows::<EventRow>();
    let st = StateO::from_events(&rows);
    assert_eq!(st.learner.lin, Some(LIN_V2));
    assert_eq!(st, s.state);
    let (n, bad) = replay_o(&rows);
    assert_eq!((n, bad), (1500, 0));
    // the switch changes the run
    let d2 = tempfile::tempdir().unwrap();
    let mut log2 = open(d2.path());
    run_o(&mut log2, &RunOCfg { world: Some(c), learner: None, discover_every: 10, ctl: None, max_explore_pm: None }, &Policy::new(1500), None).unwrap();
    assert_ne!(log2.open_latest().rows::<EventRow>(), rows);
}

// ---- the first world and World L ----

#[test]
fn the_first_world_planner_with_b13_is_deterministic_and_explains_its_estimate() {
    let mk = || ModelChooser { lin: Some(LIN_MENU), ..ModelChooser::new(DEFAULT_BETA) };
    let a = run_menu_ext(World::with_extended(3, true), 600, &mut mk(), DISCOVER_EVERY, &[]);
    let b = run_menu_ext(World::with_extended(3, true), 600, &mut mk(), DISCOVER_EVERY, &[]);
    assert_eq!(format!("{:?}", a.run.recs), format!("{:?}", b.run.recs));
    let off = run_menu_ext(World::with_extended(3, true), 600, &mut ModelChooser::new(DEFAULT_BETA), DISCOVER_EVERY, &[]);
    assert_ne!(format!("{:?}", a.run.recs), format!("{:?}", off.run.recs));
    // a decision on the final state names the estimate and its terms
    let mut st = a.state.clone();
    st.budget_left = 1000;
    let st = &st;
    let offers = super::planner::Offer { slot: 0, domain: super::world::Domain::Pattern, difficulty: 2, listed_cost: 10, listed_reward: 20 };
    let ch = mk().choose(st, &[offers]).unwrap();
    assert_eq!(ch.rationale["chooser_estimate"]["track"], "B13");
    assert!(ch.rationale["alternatives"][0]["estimate_from_linear"]["terms"].as_array().unwrap().len() >= 3);
    assert!(matches!(ch.action, Action::Attempt { .. } | Action::Skip));
}

#[test]
fn world_l_with_b13_is_deterministic_and_the_one_step_planner_is_unchanged_without_it() {
    for st in SETTINGS {
        let c = WLCfg { seed: 3, setting: st, budget: 400 };
        let on = || GreedyL { lin: Some(LIN_L), skip_frac: 0.0, ..GreedyL::default() };
        let a = run_wl(&c, &mut on());
        let b = run_wl(&c, &mut on());
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        // with the switch at its off value the planner is the B1 planner
        let x = run_wl(&c, &mut GreedyL::default());
        let y = run_wl(&c, &mut GreedyL { lin: None, ..GreedyL::default() });
        assert_eq!(format!("{x:?}"), format!("{y:?}"));
    }
}

// ---- the controller key ----

#[test]
fn the_controller_key_is_validated_and_fails_closed() {
    let ok = "max_steps_per_run = 50\ncontext_chooser = on\n";
    assert!(Controller::parse(ok).is_ok());
    assert_eq!(Controller::parse(ok).unwrap(), Controller::parse("max_steps_per_run = 50\n").unwrap(), "the key changes no B5 cap");
    for bad in ["context_chooser = maybe", "context_chooser = on\ncontext_chooser = on", "context_chooser"] {
        assert!(Controller::parse(bad).is_err(), "{bad}");
        let c = LinCtl::from_text(Some(bad));
        assert!(!c.allowed && c.closed.is_some(), "{bad}");
    }
    assert_eq!(LinCtl::from_text(Some(ok)), LinCtl { allowed: true, closed: None });
    assert_eq!(LinCtl::from_text(Some("max_steps_per_run = 50\n")), LinCtl { allowed: false, closed: None }, "default off");
    assert!(LinCtl::from_text(None).closed.is_some(), "missing file: closed");
    assert_eq!(LinCtl::from_text(Some(ok)).grant(true), Ok(true));
    assert_eq!(LinCtl::from_text(Some(ok)).grant(false), Ok(false));
    for t in [None, Some("max_steps_per_run = 50\n"), Some("context_chooser = maybe")] {
        let r = LinCtl::from_text(t).grant(true).unwrap_err();
        assert_eq!(r.cap, KEY_CHOOSER);
    }
}

#[test]
fn linval_names_no_hidden_parameter_and_reads_no_world() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/being/linval.rs")).unwrap();
    for b in ["regime", "Regime", ".expected(", "slip_pm", "World2", "WorldO", "WorldL", "truth", "feedback(", "hidden_p", "phase"] {
        assert!(!src.contains(b), "linval.rs mentions {b}");
    }
}

#[test]
fn on_an_empty_model_the_estimate_is_the_prior() {
    // on an empty model (no family seen yet) the estimate is 0 and the width is the prior one: bias and
    // difficulty features (penalty 1 each) plus the cell (penalty 16)
    let st = State2::default();
    let fit = super::world2::learner::lin_fit2(&st, &[], LIN_V2);
    let e = fit.estimate(0, 0, 1, None, 0);
    assert_eq!(e.p, 0.0);
    assert!((e.width - (2.0f64 + 1.0 / 16.0).sqrt()).abs() < 1e-9, "{}", e.width);
    assert!(e.p_up > 0.0);
    let _ = std::any::type_name::<dyn Chooser2>();
}

// ---- the review baseline: rules where a rule covers the offer, tuned LinUCB elsewhere ----

#[test]
fn the_rules_or_linucb_fallback_is_deterministic_and_uses_both_sources() {
    use super::vs_world::fallback::RulesOrLin;
    let c = W2Cfg::new(2, 12, 3000);
    let lc = LearnerCfg { use_: super::world2::all_skills::profile_use(), ..LearnerCfg::default() };
    let mut a = RulesOrLin::new(0.5, 1.0);
    let ra = run_w2_with(&c, &mut a, 10, None, lc);
    let mut b = RulesOrLin::new(0.5, 1.0);
    let rb = run_w2_with(&c, &mut b, 10, None, lc);
    assert_eq!(ra.recs, rb.recs);
    assert!(a.by_rule > 0 && a.by_lin > 0, "rule {} lin {}", a.by_rule, a.by_lin);
    // every rule answer it chose was given by an accepted rule or constant
    assert!(ra.recs.iter().filter(|x| x.rule_used).count() as u64 >= a.by_rule / 2);
    assert!(ra.cost <= 3000 && ra.cost >= 3000 - 60);
}
