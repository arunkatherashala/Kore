use super::chooser::{DomainPicker, FixedPicker};
use super::measure::{self, mean_ci, seed_row, BUDGET};
use super::replay::{next_pick, replay_decisions};
use super::run_loop::{run, run_with, RunCfg};
use super::solver::{scripted_is_exact, Context, ScriptedThinker, Thinker, SLIP_PER_MILLE};
use super::state::{BeingState, SOURCE};
use super::world::{Domain, World, DOMAINS};
use super::{Policy, StopReason};
use crate::versioned::{verify_dir, CauseRow, EventRow, OutcomeRow, PredictionRow, RevisionRow, VersionedLog};

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn cfg(world: u64, seed: u64, steps: u64, budget: i64) -> RunCfg {
    RunCfg { world_seed: Some(world), loop_seed: Some(seed), budget: Some(budget), explore_c: None, chooser: None, steps, refill_when_exhausted: false }
}

fn run_logged(root: &std::path::Path, world: u64, seed: u64, steps: u64, budget: i64) -> VersionedLog {
    let mut log = open(root);
    run(&mut log, &cfg(world, seed, steps, budget), &ScriptedThinker, &Policy::new(steps)).unwrap();
    log
}

#[test]
fn world_is_deterministic_and_checker_is_exact() {
    let (a, b) = (World::new(7), World::new(7));
    for step in 0..50 {
        for d in DOMAINS {
            let (ta, tb) = (a.task(step, d), b.task(step, d));
            assert_eq!(ta.view, tb.view);
            assert!(ta.check(ta.expected()));
            assert!(!ta.check(ta.expected() + 1));
            assert!((1..=5).contains(&ta.view.difficulty));
            assert!(ta.view.cost > 0 && ta.view.reward > 0);
        }
    }
    assert_ne!(World::new(7).profile, World::new(8).profile);
}

#[test]
fn scripted_thinker_is_exact_where_documented_and_imperfect_elsewhere() {
    let w = World::new(3);
    let th = ScriptedThinker;
    let st = [Default::default(); 4];
    let (mut exact_ok, mut exact_n, mut heur_ok, mut heur_n) = (0, 0, 0, 0);
    for step in 0..400 {
        for d in DOMAINS {
            let t = w.task(step, d);
            let ctx = Context { step, task: &t.view, noise: 999_999, stats: &st }; // noise % 1000 = 999: no slip
            let ok = t.check(th.propose(&ctx).answer);
            if scripted_is_exact(&t.view) {
                exact_n += 1;
                exact_ok += ok as u32;
            } else {
                heur_n += 1;
                heur_ok += ok as u32;
            }
        }
    }
    assert_eq!(exact_ok, exact_n, "an exact strategy without slip must be right");
    assert!(heur_n > 100 && (heur_ok as f64) < 0.5 * heur_n as f64, "heuristics are imperfect: {heur_ok}/{heur_n}");
    assert!(SLIP_PER_MILLE.iter().all(|s| *s < 200));
}

#[test]
fn run_writes_events_revisions_predictions_and_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let log = run_logged(dir.path(), 5, 1, 60, 5000);
    verify_dir(log.dir()).unwrap();
    let snap = log.open_latest();
    let ev: Vec<EventRow> = snap.rows();
    let loop_ev: Vec<&EventRow> = ev.iter().filter(|e| e.source == SOURCE).collect();
    let count = |k: &str| loop_ev.iter().filter(|e| e.kind == k).count();
    assert_eq!((count("decision"), count("action"), count("outcome"), count("note")), (60, 60, 60, 1));
    let revs: Vec<RevisionRow> = snap.rows();
    assert_eq!(revs.len(), 60);
    assert!(revs.iter().all(|r| r.entity_type == "decision" && r.author.starts_with("rule:being-")));
    let causes: Vec<CauseRow> = snap.rows();
    let genesis = loop_ev.iter().find(|e| e.kind == "note").unwrap();
    for r in &revs {
        assert!(causes.iter().any(|c| c.rev_id == r.rev_id && c.event_id == genesis.event_id));
    }
    assert_eq!(snap.rows::<PredictionRow>().len(), 60);
    assert_eq!(snap.rows::<OutcomeRow>().len(), 60);
    // occurred_at comes from the world clock (2030), not the wall clock
    assert!(loop_ev.iter().all(|e| e.occurred_at.starts_with("2030-")));
    let st = BeingState::from_snapshot(&snap);
    assert_eq!(st.step, 60);
    assert_eq!(st.total_cost, 5000 - st.budget_left);
}

#[test]
fn replay_from_the_log_reproduces_every_decision_and_the_next_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = run_logged(dir.path(), 9, 4, 130, 5000);
    let rows: Vec<EventRow> = log.open_latest().rows();
    let rep = replay_decisions(&rows);
    assert_eq!(rep.len(), 130);
    assert!(rep.iter().all(|r| r.matches()), "a decision was not reproduced");
    let predicted = next_pick(&rows).unwrap().domain;
    run(&mut log, &RunCfg { steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1)).unwrap();
    let rows: Vec<EventRow> = log.open_latest().rows();
    let last = rows.iter().rev().find(|e| e.kind == "decision").unwrap();
    assert_eq!(super::state::tag(&last.tags, "domain"), Some(predicted.name()));
}

#[test]
fn a_run_split_over_two_calls_equals_one_run() {
    let dir = tempfile::tempdir().unwrap();
    {
        let _ = run_logged(dir.path(), 2, 2, 40, 5000);
    }
    let mut log = VersionedLog::open(dir.path(), "being").unwrap();
    run(&mut log, &RunCfg { steps: 40, ..Default::default() }, &ScriptedThinker, &Policy::new(40)).unwrap();
    let rows: Vec<EventRow> = log.open_latest().rows();
    let rep = replay_decisions(&rows);
    assert_eq!(rep.len(), 80);
    assert!(rep.iter().all(|r| r.matches()));
    let d2 = tempfile::tempdir().unwrap();
    let one = run_logged(d2.path(), 2, 2, 80, 5000);
    let ids = |l: &VersionedLog| l.open_latest().rows::<EventRow>().into_iter().map(|e| e.event_id).collect::<Vec<_>>();
    assert_eq!(ids(&log), ids(&one));
}

#[test]
fn same_seeds_give_identical_log_hash_and_other_seeds_do_not() {
    let (a, b, c) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (la, lb) = (run_logged(a.path(), 3, 3, 80, 3000), run_logged(b.path(), 3, 3, 80, 3000));
    assert_eq!(la.open_latest().content_hash(), lb.open_latest().content_hash());
    assert_eq!(la.snapshot_hash(la.latest_version()).unwrap(), lb.snapshot_hash(lb.latest_version()).unwrap());
    let lc = run_logged(c.path(), 4, 3, 80, 3000);
    assert_ne!(la.open_latest().content_hash(), lc.open_latest().content_hash());
}

fn selections(log: &VersionedLog) -> ([u64; 4], [f64; 4]) {
    let st = BeingState::from_snapshot(&log.open_latest());
    let (mut n, mut e) = ([0; 4], [0.0; 4]);
    for d in DOMAINS {
        n[d.idx()] = st.stats[d.idx()].n;
        e[d.idx()] = st.stats[d.idx()].efficiency();
    }
    (n, e)
}

#[test]
fn different_worlds_lead_to_different_preferences_explained_by_evidence() {
    // worlds where the learned preference equals the hidden best domain, for two different best domains
    let pref_of = |w: u64| {
        let r = measure::run_policy(w, 3000, &mut super::chooser::Ucb1::new(1, super::chooser::DEFAULT_EXPLORE_C));
        let mut n = [0usize; 4];
        r.picks.iter().for_each(|d| n[d.idx()] += 1);
        (0..4).max_by_key(|i| n[*i]).unwrap()
    };
    let good: Vec<(u64, usize)> = (1..=30u64).filter(|w| pref_of(*w) == measure::oracle_domain(*w).idx()).map(|w| (w, pref_of(w))).collect();
    let (w1, p1) = good[0];
    let (w2, _) = *good.iter().find(|(_, p)| *p != p1).expect("two worlds with different learned preferences");
    let other = w2;
    let first = w1;
    let mut preferred = vec![];
    for w in [first, other] {
        let dir = tempfile::tempdir().unwrap();
        let log = run_logged(dir.path(), w, 1, 300, 3000);
        let (n, e) = selections(&log);
        let pref = (0..4).max_by_key(|i| n[*i]).unwrap();
        let best_logged = (0..4).filter(|i| n[*i] > 0).max_by(|a, b| e[*a].partial_cmp(&e[*b]).unwrap()).unwrap();
        // the preference is the domain with (about) the best reward per energy in the being's own log
        assert!(e[pref] >= 0.85 * e[best_logged], "world {w}: preferred {pref} {e:?} {n:?}");
        preferred.push(pref);
    }
    assert_ne!(preferred[0], preferred[1], "preferences should differ between these worlds");
}

#[test]
fn budget_exhaustion_stop_flag_and_refill() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run(&mut log, &cfg(1, 1, 10_000, 100), &ScriptedThinker, &Policy::new(10_000)).unwrap();
    assert_eq!(s.stopped, StopReason::BudgetExhausted);
    assert!(s.state.budget_left < 12 && s.state.budget_left >= 0 && s.steps_run > 5);
    let s2 = run(&mut log, &RunCfg { steps: 5, ..Default::default() }, &ScriptedThinker, &Policy::new(5)).unwrap();
    assert_eq!((s2.steps_run, s2.stopped), (0, StopReason::BudgetExhausted));
    let mut p = Policy::new(30);
    p.refill_when_exhausted = true;
    let s3 = run(&mut log, &RunCfg { steps: 30, ..Default::default() }, &ScriptedThinker, &p).unwrap();
    assert_eq!((s3.steps_run, s3.stopped), (30, StopReason::MaxSteps));
    assert!(s3.state.refills >= 1);
    assert!(log.open_latest().rows::<EventRow>().iter().any(|e| e.tags.contains("refill")));
    assert!(replay_decisions(&log.open_latest().rows::<EventRow>()).iter().all(|r| r.matches()));
    let p = Policy::new(100);
    p.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let s4 = run(&mut log, &RunCfg { steps: 100, ..Default::default() }, &ScriptedThinker, &p).unwrap();
    assert_eq!((s4.steps_run, s4.stopped), (0, StopReason::StopFlag));
}

#[test]
fn a_being_keeps_its_world_and_seed() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = run_logged(dir.path(), 1, 1, 5, 500);
    let r = run(&mut log, &RunCfg { world_seed: Some(2), steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1));
    assert!(r.is_err());
}

#[test]
fn tamper_after_a_run_fails_verify() {
    let dir = tempfile::tempdir().unwrap();
    let log = run_logged(dir.path(), 6, 6, 60, 3000);
    verify_dir(log.dir()).unwrap();
    let seg = std::fs::read_dir(log.dir().join("events")).unwrap().flatten().map(|f| f.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).last().unwrap();
    let mut bytes = std::fs::read(&seg).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&seg, bytes).unwrap();
    assert!(verify_dir(log.dir()).is_err());
}

#[test]
fn being_sources_have_no_file_network_or_process_access() {
    let banned = [
        ["std", "::fs"].concat(), ["std", "::net"].concat(), ["std", "::process"].concat(), ["std", "::env"].concat(),
        ["File", "::open"].concat(), ["File", "::create"].concat(), ["Tcp", "Stream"].concat(), ["Udp", "Socket"].concat(),
        ["Command", "::new"].concat(), ["tokio", "::net"].concat(), ["safety", "::"].concat(), ["log_tools", "::"].concat(),
        ["read_", "to_string"].concat(), ["std", "::thread"].concat(),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being");
    let mut n = 0;
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = f.file_name().to_string_lossy().to_string();
        if name == "tests.rs" || !name.ends_with(".rs") {
            continue;
        }
        n += 1;
        let src = std::fs::read_to_string(f.path()).unwrap();
        for b in &banned {
            assert!(!src.contains(b.as_str()), "{name} uses {b}");
        }
    }
    assert!(n >= 8);
}

#[test]
fn baseline_pickers_run_through_the_logged_loop_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut p = FixedPicker::always(Domain::Pattern);
    run_with(&mut log, &cfg(1, 1, 20, 2000), &ScriptedThinker, &Policy::new(20), Some(&mut p as &mut dyn DomainPicker)).unwrap();
    let st = BeingState::from_snapshot(&log.open_latest());
    assert_eq!(st.stats[Domain::Pattern.idx()].n, 20);
}

#[test]
fn measured_chooser_beats_random_and_regret_stays_below_floor() {
    let rows: Vec<_> = (1..=20).map(|s| seed_row(s, super::chooser::DEFAULT_EXPLORE_C, BUDGET)).collect();
    let l: Vec<f64> = rows.iter().map(|r| r.learned.per_budget()).collect();
    let r: Vec<f64> = rows.iter().map(|r| r.random.per_budget()).collect();
    let o: Vec<f64> = rows.iter().map(|r| r.oracle.per_budget()).collect();
    let (ml, _, _) = mean_ci(&l);
    let (mr, _, _) = mean_ci(&r);
    let (mo, _, _) = mean_ci(&o);
    assert!(ml > mr + 0.2, "learned {ml} vs random {mr}");
    assert!(mo - ml < 0.45, "regret {}", mo - ml);
    assert!(l.iter().zip(&r).filter(|(a, b)| a > b).count() >= 15);
}

// ---- Track B1: menu world, learned model, planner ----

use super::measure::{calibration, menu_row, run_menu, truth, LONG_BUDGET};
use super::planner::{ModelChooser, DEFAULT_BETA, DEFAULT_SKIP_FRAC};
use super::solver::{Strategy, STRATEGIES};
use super::state::Chooser;

fn cfg_model(world: u64, seed: u64, steps: u64, budget: i64) -> RunCfg {
    RunCfg { chooser: Some(Chooser::Model), ..cfg(world, seed, steps, budget) }
}

fn run_model_logged(root: &std::path::Path, world: u64, seed: u64, steps: u64, budget: i64) -> (VersionedLog, super::run_loop::RunSummary) {
    let mut log = open(root);
    let s = run(&mut log, &cfg_model(world, seed, steps, budget), &ScriptedThinker, &Policy::new(steps)).unwrap();
    (log, s)
}

#[test]
fn menu_world_is_deterministic_and_single_mode_is_untouched() {
    let (a, b) = (World::new(11), World::new(11));
    for step in 0..40 {
        let (oa, ob) = (a.offers(step), b.offers(step));
        assert_eq!(oa.len(), 4);
        for (x, y) in oa.iter().zip(&ob) {
            assert_eq!(x.view, y.view);
            assert!(x.check(x.expected()) && !x.check(x.expected() + 1));
        }
        for d in DOMAINS {
            assert_eq!(a.task(step, d).view.id, format!("{step}-{}", d.name()), "single-task ids are unchanged");
        }
    }
    let domains: std::collections::HashSet<_> = (0..40).flat_map(|s| a.offers(s)).map(|t| t.view.domain).collect();
    assert_eq!(domains.len(), 4, "all domains appear in menus");
    // the same menu run twice gives the same decisions and totals
    let r1 = run_menu(5, 600, &mut ModelChooser::new(DEFAULT_BETA));
    let r2 = run_menu(5, 600, &mut ModelChooser::new(DEFAULT_BETA));
    assert_eq!((r1.reward, r1.cost, r1.recs.len()), (r2.reward, r2.cost, r2.recs.len()));
    assert!(r1.recs.iter().zip(&r2.recs).all(|(x, y)| x.correct == y.correct && x.cost == y.cost));
}

#[test]
fn strategies_trade_cost_against_reliability_in_every_domain() {
    for s in STRATEGIES {
        assert!(Strategy::parse(s.name()) == Some(s));
    }
    assert!(Strategy::Fast.energy(7) < Strategy::Careful.energy(7) && Strategy::Careful.energy(7) < Strategy::Deep.energy(7));
    for w in [1u64, 2, 3] {
        let t = truth(w);
        for d in DOMAINS {
            let avg = |s: Strategy, lo: usize| (lo..5).map(|k| t.p[d.idx()][k][s.idx()]).sum::<f64>() / (5 - lo) as f64;
            // deep is the most reliable on the hard templates, fast the least; fast is good on easy ones
            assert!(avg(Strategy::Deep, 2) > avg(Strategy::Careful, 2) - 0.05, "{d:?}");
            assert!(avg(Strategy::Deep, 2) > avg(Strategy::Fast, 2) + 0.15, "{d:?} deep {} fast {}", avg(Strategy::Deep, 2), avg(Strategy::Fast, 2));
            assert!(t.p[d.idx()][0][Strategy::Fast.idx()] > 0.6, "{d:?} easy tasks are solved by the fast strategy");
        }
    }
}

#[test]
fn model_is_derived_from_the_log_and_the_planner_cannot_see_hidden_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = run_model_logged(dir.path(), 7, 3, 150, 4000);
    let refolded = BeingState::from_snapshot(&log.open_latest());
    assert_eq!(refolded.model, s.state.model, "the model folded from the log equals the live model");
    assert!(refolded.model.attempts() > 100);
    // the model has learned that the fast strategy is cheaper than the listed cost
    let e = refolded.model.estimate(Domain::Arithmetic, 1, Strategy::Fast);
    assert!(e.n == 0 || e.cost_ratio < 1.0);
    // source scan: model.rs and planner.rs name none of the hidden world parameters
    let forbidden = ["Profile", "profile", "reward_scale", "cost_base", "diff_bias", ".expected(", "noise(", "task_forced", "truth", "World", "generate"];
    let dir_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being");
    for f in ["model.rs", "planner.rs"] {
        let src = std::fs::read_to_string(dir_src.join(f)).unwrap();
        for b in forbidden {
            assert!(!src.contains(b), "{f} mentions {b}");
        }
    }
}

#[test]
fn model_decisions_replay_exactly_from_the_log_with_reasons_logged() {
    let dir = tempfile::tempdir().unwrap();
    let (mut log, s) = run_model_logged(dir.path(), 8, 2, 200, 6000);
    assert!(s.state.step == 200);
    let rows: Vec<EventRow> = log.open_latest().rows();
    let rep = replay_decisions(&rows);
    assert_eq!(rep.len(), 200);
    assert!(rep.iter().all(|r| r.matches()), "a menu decision was not re-derived");
    // every decision carries its reasons: 12 alternatives (4 offers x 3 strategies) with estimates
    let snap = log.open_latest();
    let revs: Vec<RevisionRow> = snap.rows();
    assert_eq!(revs.len(), 200);
    for r in &revs {
        let v: serde_json::Value = serde_json::from_str(&r.rationale).unwrap();
        let alts = v["alternatives"].as_array().unwrap();
        assert_eq!(alts.len(), 12);
        assert!(alts[0]["p_success"].is_number() && alts[0]["uncertainty_sd"].is_number() && alts[0]["score"].is_number());
    }
    // next decision equals the replayed prediction, and a split run equals one run
    let predicted = super::replay::next_action(&rows).unwrap();
    run(&mut log, &RunCfg { steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1)).unwrap();
    let rows2: Vec<EventRow> = log.open_latest().rows();
    let last = rows2.iter().rev().find(|e| e.kind == "decision").unwrap();
    assert_eq!(super::state::tag(&last.tags, "action"), Some(predicted.as_str()));
    let d2 = tempfile::tempdir().unwrap();
    let (one, _) = run_model_logged(d2.path(), 8, 2, 201, 6000);
    let ids = |l: &VersionedLog| l.open_latest().rows::<EventRow>().into_iter().map(|e| e.event_id).collect::<Vec<_>>();
    assert_eq!(ids(&log), ids(&one));
}

#[test]
fn menu_run_logs_skips_without_predictions_and_keeps_its_chooser() {
    let dir = tempfile::tempdir().unwrap();
    let (mut log, s) = run_model_logged(dir.path(), 9, 1, 400, 20_000);
    let rows: Vec<EventRow> = log.open_latest().rows();
    let skips = rows.iter().filter(|e| e.kind == "outcome" && e.tags.contains("result:skip")).count();
    assert!(skips > 0, "the planner skips some menus in this run");
    let preds = log.open_latest().rows::<PredictionRow>().len();
    assert_eq!(preds, 400 - skips);
    assert_eq!(s.state.total_cost, 20_000 - s.state.budget_left);
    verify_dir(log.dir()).unwrap();
    // chooser is fixed once the being exists
    let bad = run(&mut log, &RunCfg { chooser: Some(Chooser::Ucb), steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1));
    assert!(bad.is_err());
    let ok = run(&mut log, &RunCfg { chooser: Some(Chooser::Model), steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1));
    assert!(ok.is_ok());
    assert!(replay_decisions(&log.open_latest().rows::<EventRow>()).iter().all(|r| r.matches()));
}

#[test]
fn tamper_after_a_model_run_fails_verify() {
    let dir = tempfile::tempdir().unwrap();
    let (log, _) = run_model_logged(dir.path(), 6, 6, 80, 3000);
    verify_dir(log.dir()).unwrap();
    let seg = std::fs::read_dir(log.dir().join("events")).unwrap().flatten().map(|f| f.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).last().unwrap();
    let mut bytes = std::fs::read(&seg).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&seg, bytes).unwrap();
    assert!(verify_dir(log.dir()).is_err());
}

#[test]
fn menu_budget_exhaustion_and_refill_replay() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run(&mut log, &cfg_model(1, 1, 10_000, 120), &ScriptedThinker, &Policy::new(10_000)).unwrap();
    assert_eq!(s.stopped, StopReason::BudgetExhausted);
    let mut p = Policy::new(40);
    p.refill_when_exhausted = true;
    let s3 = run(&mut log, &RunCfg { steps: 40, ..Default::default() }, &ScriptedThinker, &p).unwrap();
    assert!(s3.steps_run > 0);
    assert!(replay_decisions(&log.open_latest().rows::<EventRow>()).iter().all(|r| r.matches()));
}

#[test]
fn measured_model_learner_beats_baselines_and_is_better_calibrated_than_the_base_rate() {
    // floors set below the measured minima (docs/measured.md); report seeds 1..20, budget 600
    let rows: Vec<_> = (1..=20).map(|s| menu_row(s, BUDGET, DEFAULT_BETA, DEFAULT_SKIP_FRAC)).collect();
    let m = |f: &dyn Fn(&super::measure::MenuRow) -> f64| mean_ci(&rows.iter().map(f).collect::<Vec<_>>()).0;
    let (model, ucb, greedy, fast, random, oracle) = (
        m(&|r| r.model.per_budget()), m(&|r| r.ucb.per_budget()), m(&|r| r.greedy.per_budget()), m(&|r| r.greedy_fast.per_budget()),
        m(&|r| r.random.per_budget()), m(&|r| r.oracle.per_budget()),
    );
    assert!(model > ucb + 1.0, "model {model} ucb {ucb}");
    assert!(model > greedy + 1.0 && model > random + 1.0);
    assert!(model > fast + 0.6, "model {model} greedy-fast {fast}");
    assert!(oracle - model < 1.0, "regret {}", oracle - model);
    assert!(rows.iter().filter(|r| r.model.per_budget() > r.ucb.per_budget()).count() >= 18);
    let runs: Vec<_> = rows.iter().map(|r| &r.model).collect();
    let cal = calibration(&runs, 10);
    assert!(cal.n > 1000 && cal.brier < cal.brier_base_rate, "brier {} base {}", cal.brier, cal.brier_base_rate);
    let long = menu_row(3, LONG_BUDGET, DEFAULT_BETA, DEFAULT_SKIP_FRAC);
    assert!(long.model.per_budget() > long.ucb.per_budget() + 0.8);
}
