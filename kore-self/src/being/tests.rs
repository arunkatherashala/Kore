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
    RunCfg { world_seed: Some(world), loop_seed: Some(seed), budget: Some(budget), explore_c: None, chooser: None, steps, refill_when_exhausted: false, extended: None, discover_every: None, drift: None, adapt: None, advisor: None, advisor_quality: None, trust: None }
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
    let st = [Default::default(); 5];
    let (mut exact_ok, mut exact_n, mut heur_ok, mut heur_n) = (0, 0, 0, 0);
    for step in 0..400 {
        for d in DOMAINS {
            let t = w.task(step, d);
            let ctx = Context { step, task: &t.view, noise: 999_999, stats: &st, rules: &[], degraded: false }; // noise % 1000 = 999: no slip
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

// ---- Track B2: rule discovery ----

use super::discover::{self, apply, evaluate, forms, predict, rule_id, search, search_with, Example, Form, Rule, K_TRAIN, K_VAL};
use super::measure::{control_examples, coverage, disc_row, perfect_rules, EXPRESSIBLE, UNEXPRESSIBLE};
use super::replay::rederive_rules;
use super::world::Kind;

fn ex(id: usize, sig: u8, terms: &[i64], next: i64) -> Example {
    Example { event_id: format!("e{id}"), sig, terms: terms.to_vec(), next }
}

#[test]
fn dsl_evaluator_fits_known_sequences_exactly_and_refuses_the_rest() {
    let lin = |mask: u8, constant: bool| Form::Lin { mask, constant };
    // arithmetic, geometric, affine geometric, fibonacci-like, second-order, tribonacci, lag-2 with constant, cycle, polynomials
    assert_eq!(predict(&Form::Poly(1), &[3, 7, 11, 15, 19]), Some(23));
    assert_eq!(predict(&lin(1, false), &[2, 6, 18, 54, 162]), Some(486));
    assert_eq!(predict(&lin(1, true), &[1, 3, 7, 15, 31, 63]), Some(127)); // t -> 2t + 1
    assert_eq!(predict(&lin(3, false), &[1, 1, 2, 3, 5, 8, 13]), Some(21));
    assert_eq!(predict(&lin(3, false), &[1, 2, 7, 20, 61, 182, 547]), Some(1640)); // t -> 2*t[-1] + 3*t[-2]
    assert_eq!(predict(&lin(7, false), &[1, 1, 2, 4, 7, 13, 24, 44, 81]), Some(149));
    assert_eq!(predict(&lin(2, true), &[1, 3, 4, 6, 7, 9, 10]), Some(12)); // add 2, add 1, add 2, ...
    assert_eq!(predict(&lin(4, false), &[5, 8, 2, 5, 8, 2, 5]), Some(8));
    assert_eq!(predict(&Form::Poly(3), &[0, 1, 8, 27, 64, 125, 216]), Some(343));
    // rational coefficients are fine when the next term is an integer
    assert_eq!(predict(&lin(1, false), &[64, 32, 16, 8, 4]), Some(2));
    // refusals: too short for the slack, inconsistent, not unique, non-integer next term
    assert_eq!(predict(&lin(3, false), &[1, 1, 2, 3, 5, 8]), None, "needs 3 spare equations");
    assert_eq!(predict(&lin(1, false), &[2, 6, 18, 54, 163]), None, "inconsistent prefix");
    assert_eq!(predict(&lin(1, true), &[4, 4, 4, 4, 4, 4]), None, "constant column pair is not unique");
    assert_eq!(predict(&lin(1, false), &[3, 2, 1, 3, 2]), None);
    assert_eq!(predict(&lin(1, false), &[16, 8, 4, 2, 1]), None, "next term 1/2 is not an integer");
    // codes round-trip and the grammar has exactly 17 forms, smallest first
    let all = forms();
    assert_eq!(all.len(), 17);
    assert!(all.windows(2).all(|w| w[0].size() <= w[1].size()));
    for f in &all {
        assert_eq!(Form::parse(&f.code()), Some(*f));
        assert!(f.dsl().starts_with("next = "));
    }
    assert_eq!(Form::parse("lin.3.c"), Some(lin(4, true)));
    assert_eq!(Form::parse("lin.9"), None);
    assert_eq!(Form::parse("poly.7"), None);
}

#[test]
fn search_finds_every_expressible_family_and_accepts_nothing_for_unexpressible_ones() {
    for seed in 1..=6u64 {
        let w = World::with_extended(seed, true);
        for k in EXPRESSIBLE {
            let found = search(&control_examples(seed, &[k], 40, 7_000_000));
            let book: Vec<Rule> = found.iter().map(|f| Rule::new(f.sig, f.form, f.train_ok, f.val_ok, 0)).collect();
            assert!(coverage(&w, &book, k, 100) >= 0.9, "seed {seed} family {k:?}: found {:?}", found.iter().map(|f| f.form.code()).collect::<Vec<_>>());
        }
        for n in [30u64, 150] {
            let junk = search(&control_examples(seed, &UNEXPRESSIBLE, n, 7_000_000));
            assert!(junk.is_empty(), "seed {seed}: accepted {:?} for families outside the grammar", junk.iter().map(|f| f.form.code()).collect::<Vec<_>>());
        }
    }
    // the hidden families are not solved by any fixed strategy
    let w = World::with_extended(2, true);
    let th = ScriptedThinker;
    let st = [Default::default(); 5];
    for k in EXPRESSIBLE.iter().chain(UNEXPRESSIBLE.iter()) {
        for s in [Strategy::Fast, Strategy::Careful, Strategy::Deep] {
            let hits = (0..60u64)
                .filter(|i| {
                    let t = w.task_of_kind(*i, *k);
                    let ctx = Context { step: *i, task: &t.view, noise: 999_999, stats: &st, rules: &[], degraded: false };
                    t.check(th.propose_with(&ctx, s).answer)
                })
                .count();
            assert!(hits <= 6, "{k:?} {s:?} solved {hits}/60 by a fixed solver");
        }
    }
    // with the perfect rule book the discovered strategy solves the expressible ones and not the others
    let book = perfect_rules();
    for k in EXPRESSIBLE {
        assert!(coverage(&w, &book, k, 100) >= 0.95, "{k:?}");
    }
    for k in UNEXPRESSIBLE {
        assert!(coverage(&w, &book, k, 100) <= 0.02, "{k:?}");
    }
}

#[test]
fn acceptance_needs_support_a_held_out_check_and_no_wrong_application() {
    // arithmetic examples of one signature: accepted (poly.1 is the smallest form that fits)
    let mk = |n: usize| -> Vec<Example> {
        (0..n)
            .map(|i| {
                let (s, d) = (i as i64 % 7, 1 + i as i64 % 4);
                let t: Vec<i64> = (0..5).map(|j| s + d * j).collect();
                ex(i, 1, &t, s + d * 5)
            })
            .collect()
    };
    let good = search(&mk(20));
    assert_eq!(good.len(), 1);
    assert_eq!(good[0].form, Form::Poly(1));
    assert!(good[0].train_ok >= K_TRAIN && good[0].val_ok >= K_VAL && good[0].cite.len() <= discover::MAX_CITE);
    // minimum support: too few examples, nothing is accepted
    assert!(search(&mk((K_TRAIN + K_VAL) as usize - 1)).is_empty());
    assert!(search_with(&mk(20), 30, 1).is_empty());
    // overfit guard: the rule fits every training example, but one held-out example (index 2, 5, 8, 11, ...)
    // is applicable and wrong; the rule must be rejected although the training support is large
    let mut bad = mk(20);
    let (t, n) = (bad[11].terms.clone(), bad[11].next);
    bad[11] = ex(11, 1, &t, n + 1);
    assert!(discover::is_held_out(11));
    assert_eq!(evaluate(&Form::Poly(1), &bad[..11]).1, 0, "fits all training examples");
    assert_eq!(search(&bad).len(), 0, "a rule that fails validation must not be accepted");
    // the same wrong example as a TRAINING example (index 10) is rejected too: never applicable-and-wrong
    let mut bad2 = mk(20);
    let (t, n) = (bad2[10].terms.clone(), bad2[10].next);
    bad2[10] = ex(10, 1, &t, n + 1);
    assert!(!discover::is_held_out(10));
    assert_eq!(search(&bad2).len(), 0);
    // a larger form that only repeats what a smaller accepted form already explains is not added
    assert!(search(&mk(40)).iter().all(|f| f.form == Form::Poly(1)));
    // signatures are separate: the same examples under two signatures give a rule for each
    let both: Vec<Example> = mk(20).into_iter().chain(mk(20).into_iter().map(|e| Example { sig: 2, event_id: format!("b{}", e.event_id), ..e })).collect();
    assert_eq!(search(&both).iter().map(|x| x.sig).collect::<Vec<_>>(), vec![1, 2]);
    let (r, wr, _) = evaluate(&Form::Poly(1), &mk(20));
    assert_eq!((r, wr), (20, 0));
    // determinism and ids derived from content
    assert_eq!(search(&both), search(&both));
    assert_ne!(rule_id(1, &Form::Poly(1)), rule_id(2, &Form::Poly(1)));
    assert_ne!(rule_id(1, &Form::Poly(1)), rule_id(1, &Form::Poly(2)));
    // apply: smallest applicable rule of the signature, none for another signature
    let rules = vec![Rule::new(1, Form::Poly(2), 0, 0, 0), Rule::new(1, Form::Poly(1), 0, 0, 0)];
    assert_eq!(apply(&rules, 1, &[1, 2, 3, 4, 5]).map(|(v, r)| (v, r.form)), Some((6, Form::Poly(1))));
    assert!(apply(&rules, 2, &[1, 2, 3, 4, 5]).is_none());
}

#[test]
fn discovery_source_names_no_hidden_world_fields() {
    let forbidden = ["Profile", "profile", "reward_scale", "cost_base", "diff_bias", ".expected(", "noise(", "task_forced", "task_of_kind", "truth", "World", "generate", "Kind::", "ExtAlt", "NegProd", "NegPrimes", "pattern_bonus"];
    let dir_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being");
    for f in ["discover.rs", "model.rs", "planner.rs"] {
        let src = std::fs::read_to_string(dir_src.join(f)).unwrap();
        for b in forbidden {
            assert!(!src.contains(b), "{f} mentions {b}");
        }
    }
}

#[test]
fn extended_option_is_off_by_default_and_changes_nothing_when_off() {
    let (a, b) = (World::new(5), World::with_extended(5, false));
    for step in 0..60 {
        for (x, y) in a.offers(step).iter().zip(b.offers(step).iter()) {
            assert_eq!(x.view, y.view);
        }
    }
    let ext = World::with_extended(5, true);
    // same menu domains, difficulties and costs; only pattern tasks change (families, reward bonus)
    for step in 0..60 {
        for (x, y) in a.offers(step).iter().zip(ext.offers(step).iter()) {
            assert_eq!((x.view.domain, x.view.difficulty, x.view.cost), (y.view.domain, y.view.difficulty, y.view.cost));
            if x.view.domain != Domain::Pattern {
                assert_eq!(x.view, y.view);
            } else {
                assert!((y.view.reward - 2 * x.view.reward).abs() <= 1);
                assert!(y.check(y.expected()) && !y.check(y.expected() + 1));
            }
        }
    }
    // the standard being's log carries no revealed examples, no extended tag and no rule
    let dir = tempfile::tempdir().unwrap();
    let (log, _) = run_model_logged(dir.path(), 4, 1, 60, 3000);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().all(|e| !e.tags.contains("terms:") && !e.tags.contains("pattern_families") && !e.tags.contains("rule")));
    assert!(BeingState::from_events(&rows).rules.is_empty());
    // extended needs the menu world, and discovery needs the extended world
    let d2 = tempfile::tempdir().unwrap();
    let mut l2 = open(d2.path());
    assert!(run(&mut l2, &RunCfg { extended: Some(true), ..cfg(1, 1, 5, 500) }, &ScriptedThinker, &Policy::new(5)).is_err());
    let d3 = tempfile::tempdir().unwrap();
    let mut l3 = open(d3.path());
    assert!(run(&mut l3, &RunCfg { discover_every: Some(5), ..cfg_model(1, 1, 5, 500) }, &ScriptedThinker, &Policy::new(5)).is_err());
}

fn cfg_ext(world: u64, seed: u64, steps: u64, budget: i64, every: u64) -> RunCfg {
    RunCfg { extended: Some(true), discover_every: Some(every), ..cfg_model(world, seed, steps, budget) }
}

fn run_ext(root: &std::path::Path, world: u64, steps: u64, budget: i64, every: u64) -> (VersionedLog, super::run_loop::RunSummary) {
    let mut log = open(root);
    let s = run(&mut log, &cfg_ext(world, 1, steps, budget, every), &ScriptedThinker, &Policy::new(steps)).unwrap();
    (log, s)
}

const EXT_WORLD: u64 = 6;
const EXT_BUDGET: i64 = 60_000;

#[test]
fn discovery_run_stores_rules_as_revisions_with_provenance_and_replays_from_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let (mut log, s) = run_ext(dir.path(), EXT_WORLD, 700, EXT_BUDGET, 10);
    assert_eq!(s.state.step, 700);
    assert!(!s.state.rules.is_empty(), "no rule was discovered in 700 steps");
    assert_eq!(s.new_rules.len(), s.state.rules.len());
    verify_dir(log.dir()).unwrap();
    let snap = log.open_latest();
    let rows: Vec<EventRow> = snap.rows();
    let revs: Vec<RevisionRow> = snap.rows::<RevisionRow>().into_iter().filter(|r| r.entity_type == "rule").collect();
    let causes: Vec<CauseRow> = snap.rows();
    assert_eq!(revs.len(), s.state.rules.len());
    for r in &s.state.rules {
        let rev = revs.iter().find(|v| v.key == format!("being/rule/{}", r.id)).expect("a rule revision per rule");
        assert_eq!(rev.author, "rule:being-discover");
        assert!(rev.stance.contains("next = "), "{}", rev.stance);
        let v: serde_json::Value = serde_json::from_str(&rev.rationale).unwrap();
        assert!(v["training_examples_reproduced"].as_u64().unwrap() >= K_TRAIN as u64 && v["held_out_examples_reproduced"].as_u64().unwrap() >= K_VAL as u64);
        assert_eq!(v["dsl"], r.form.dsl());
        // provenance: the rule note plus logged outcome events that carry the revealed examples
        let cited: Vec<&EventRow> = causes.iter().filter(|c| c.rev_id == rev.rev_id).filter_map(|c| rows.iter().find(|e| e.event_id == c.event_id)).collect();
        assert!(cited.iter().any(|e| e.event_id == r.event_id));
        let outs: Vec<&&EventRow> = cited.iter().filter(|e| e.kind == "outcome").collect();
        assert!(outs.len() >= (K_TRAIN + K_VAL) as usize);
        for e in outs {
            let ex = discover::example_from_event(e).expect("cited outcome carries terms and the revealed next term");
            assert_eq!(ex.sig, r.sig);
            assert_eq!(predict(&r.form, &ex.terms), Some(ex.next), "a cited example is reproduced exactly by its rule");
        }
    }
    // the rule set is a function of the log
    let refolded = BeingState::from_events(&rows);
    assert_eq!(refolded.rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), s.state.rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
    assert_eq!(refolded.model, s.state.model);
    let re = rederive_rules(&rows);
    assert_eq!(re.len(), s.state.rules.len());
    assert!(re.iter().all(|x| x.1), "every logged rule is re-derived by the search over the examples logged before it");
    assert!(replay_decisions(&rows).iter().all(|r| r.matches()));
    // idempotent: another pass over the same log adds nothing; more steps never duplicate a rule
    let mut eng = super::run_loop::Engine::new(World::with_extended(EXT_WORLD, true), refolded.clone(), &ScriptedThinker);
    assert!(eng.discover_pass().is_empty());
    let n0 = s.state.rules.len();
    let s2 = run(&mut log, &RunCfg { steps: 60, discover_every: Some(10), ..Default::default() }, &ScriptedThinker, &Policy::new(60)).unwrap();
    let mut ids: Vec<String> = s2.state.rules.iter().map(|r| r.id.clone()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "no duplicate rule ids");
    assert!(total >= n0);
    // the discovered strategy used a rule, and the logged answer was checked like any other
    let used = log.open_latest().rows::<EventRow>().iter().filter(|e| e.kind == "action" && e.tags.contains("rule_used:")).count();
    assert!(used > 0, "the being applied one of its own rules");
    // the extended being keeps its world option
    let bad = run(&mut log, &RunCfg { extended: Some(false), steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1));
    assert!(bad.is_err());
}

#[test]
fn discovery_is_deterministic_split_runs_equal_one_run_and_tamper_fails_verify() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (la, sa) = run_ext(a.path(), EXT_WORLD, 300, EXT_BUDGET, 10);
    let (lb, _) = run_ext(b.path(), EXT_WORLD, 300, EXT_BUDGET, 10);
    assert_eq!(la.open_latest().content_hash(), lb.open_latest().content_hash());
    // two calls of 150 steps equal one call of 300 (passes depend on the being's own step count)
    let c = tempfile::tempdir().unwrap();
    let (mut lc, _) = run_ext(c.path(), EXT_WORLD, 150, EXT_BUDGET, 10);
    run(&mut lc, &RunCfg { steps: 150, discover_every: Some(10), ..Default::default() }, &ScriptedThinker, &Policy::new(150)).unwrap();
    let ids = |l: &VersionedLog| l.open_latest().rows::<EventRow>().into_iter().map(|e| e.event_id).collect::<Vec<_>>();
    assert_eq!(ids(&la), ids(&lc));
    // a being that never runs a pass has no rules, a different log, and the same decisions up to the first rule
    let d = tempfile::tempdir().unwrap();
    let mut ld = open(d.path());
    let sd = run(&mut ld, &RunCfg { extended: Some(true), ..cfg_model(EXT_WORLD, 1, 300, EXT_BUDGET) }, &ScriptedThinker, &Policy::new(300)).unwrap();
    assert!(sd.state.rules.is_empty());
    assert!(!sa.state.rules.is_empty());
    assert_ne!(la.open_latest().content_hash(), ld.open_latest().content_hash());
    // tamper after a run fails verify
    verify_dir(la.dir()).unwrap();
    let seg = std::fs::read_dir(la.dir().join("events")).unwrap().flatten().map(|f| f.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).last().unwrap();
    let mut bytes = std::fs::read(&seg).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&seg, bytes).unwrap();
    assert!(verify_dir(la.dir()).is_err());
}

#[test]
fn model_treats_discovered_as_one_more_strategy_and_restarts_its_cell_when_a_rule_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = run_ext(dir.path(), EXT_WORLD, 300, EXT_BUDGET, 10);
    let snap = log.open_latest();
    let revs: Vec<RevisionRow> = snap.rows();
    let dec = revs.iter().find(|r| r.entity_type == "decision").unwrap();
    let v: serde_json::Value = serde_json::from_str(&dec.rationale).unwrap();
    assert_eq!(v["alternatives"].as_array().unwrap().len(), 16, "4 offers x 4 strategies");
    assert!(v["alternatives"].as_array().unwrap().iter().any(|a| a["strategy"] == "discovered"));
    // applying a rule restarts the estimate of the discovered strategy for that signature only
    let mut st = s.state.clone();
    let before = st.model.estimate(Domain::Pattern, 5, Strategy::Discovered).n;
    let other = st.model.estimate(Domain::Pattern, 4, Strategy::Discovered).n;
    st.apply_rule(Rule::new(5, Form::Poly(3), 3, 1, 999));
    assert_eq!(st.model.estimate(Domain::Pattern, 5, Strategy::Discovered).n, 0);
    assert_eq!(st.model.estimate(Domain::Pattern, 4, Strategy::Discovered).n, other);
    let _ = before;
    // the same rule twice changes nothing
    assert!(!st.apply_rule(Rule::new(5, Form::Poly(3), 3, 1, 1000)));
}

#[test]
fn measured_discovery_is_precise_finds_families_and_does_not_accept_junk() {
    // floors set below the measured values (docs/measured.md); seeds 1..6 of the report seeds, budget 3000
    let rows: Vec<_> = (1..=6u64).map(|s| disc_row(s, super::measure::LONG_BUDGET, DEFAULT_BETA, super::measure::DISCOVER_EVERY, super::world::EXT_PATTERN_BONUS, super::planner::DEFAULT_CURIOSITY)).collect();
    let m = |f: &dyn Fn(&super::measure::DiscRow) -> f64| mean_ci(&rows.iter().map(f).collect::<Vec<_>>()).0;
    let (disc, nodisc, random, oracle_rules, oracle_fixed) = (
        m(&|r| r.disc.run.per_budget()), m(&|r| r.nodisc.run.per_budget()), m(&|r| r.random.run.per_budget()),
        m(&|r| r.oracle_rules.run.per_budget()), m(&|r| r.oracle_fixed.run.per_budget()),
    );
    println!("disc {disc:.3} nodisc {nodisc:.3} random {random:.3} oracle_rules {oracle_rules:.3} oracle_fixed {oracle_fixed:.3}");
    let (acc, ex): (usize, usize) = rows.iter().fold((0, 0), |a, r| (a.0 + r.analysis.rules, a.1 + r.analysis.exact));
    let found: usize = rows.iter().map(|r| r.analysis.first.iter().filter(|x| x.is_some()).count()).sum();
    println!("rules {acc} exact {ex} found {found} of {}", rows.len() * 6);
    let neg: f64 = rows.iter().map(|r| r.analysis.neg[0].0 + r.analysis.neg[1].0).sum();
    println!("negative application {neg}");
    // the world gives rules something to be worth, and a being with rules can use it
    assert!(oracle_rules > oracle_fixed + 0.15, "oracle with rules {oracle_rules} fixed {oracle_fixed}");
    assert!(disc > random + 1.5 && nodisc > random + 1.5);
    // honest floor: discovery must not cost more than a small amount against the learner without it (it gained +0.07 on these seeds)
    assert!(disc > nodisc - 0.2, "discovery {disc} vs none {nodisc}");
    assert!(acc >= 10 && ex as f64 >= 0.95 * acc as f64, "precision {ex}/{acc}");
    assert!(found >= 10, "families found {found} of 36");
    assert!(neg <= 0.1, "rules were applied to families outside the grammar: {neg}");
}

// ---- Track B3: drifting world, adaptive learner ----

use super::measure::{run_drift, AdaptSpec};
use super::model::AdaptMode;

fn cfg_drift(world: u64, seed: u64, steps: u64, budget: i64, adapt: bool) -> RunCfg {
    RunCfg { drift: Some(true), adapt: Some(adapt), ..cfg_model(world, seed, steps, budget) }
}

const DRIFT_BUDGET: i64 = 6000;

fn run_drift_logged(root: &std::path::Path, world: u64, steps: u64, adapt: bool) -> (VersionedLog, super::run_loop::RunSummary) {
    let mut log = open(root);
    let s = run(&mut log, &cfg_drift(world, 1, steps, DRIFT_BUDGET, adapt), &ScriptedThinker, &Policy::new(steps)).unwrap();
    (log, s)
}

/// A world seed whose adaptive being flags at least one change within the first `steps` steps.
fn world_with_alarms() -> u64 {
    (1..=12u64)
        .find(|w| {
            let r = run_drift(*w, DRIFT_BUDGET, true, &AdaptSpec::full(), &mut ModelChooser::new(DEFAULT_BETA));
            !r.alarms.is_empty()
        })
        .expect("some world flags a change")
}

#[test]
fn drift_is_off_by_default_and_changes_nothing_when_off() {
    let a = World::new(5);
    let b = World::with_drift(5, false, None);
    assert!(b.drift.is_none() && b.regime(10_000_000) == 0 && !b.degraded(10_000_000));
    for step in 0..60 {
        for clock in [0i64, 123_456, 10_000_000] {
            for (x, y) in a.offers(step).iter().zip(b.offers_at(step, clock).iter()) {
                assert_eq!(x.view, y.view);
            }
        }
    }
    // a being without the options leaves no trace of them in its log
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = run_model_logged(dir.path(), 4, 1, 80, 3000);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().all(|e| !e.tags.contains("drift") && !e.tags.contains("adapt") && !e.tags.contains("change")));
    assert!(s.state.changes.is_empty() && s.state.model.mode() == AdaptMode::Off);
    // the B1 learner run through the drift harness (no drift, adaptation off) is the B1 run, bit for bit
    for seed in 1..=4u64 {
        let r1 = run_menu(seed, BUDGET, &mut ModelChooser::new(DEFAULT_BETA));
        let r2 = run_drift(seed, BUDGET, false, &AdaptSpec::off(), &mut ModelChooser::new(DEFAULT_BETA));
        assert_eq!((r1.reward, r1.cost, r1.recs.len()), (r2.reward, r2.cost, r2.recs.len()), "seed {seed}");
    }
}

#[test]
fn drift_schedule_is_seeded_deterministic_and_hidden_from_the_being() {
    let (w1, w2) = (World::with_drift(3, false, Some(DRIFT_BUDGET)), World::with_drift(3, false, Some(DRIFT_BUDGET)));
    assert_eq!(w1.drift, w2.drift);
    let at = w1.drift.as_ref().unwrap().at;
    assert!(at[0] < at[1] && at[1] < at[2]);
    for (i, f) in super::world::DRIFT_FRACTIONS.iter().enumerate() {
        let frac = at[i] as f64 / (DRIFT_BUDGET * super::world::SECS_PER_ENERGY) as f64;
        assert!((frac - f).abs() <= 0.0401, "change {i} at fraction {frac}");
    }
    assert_ne!(World::with_drift(4, false, Some(DRIFT_BUDGET)).drift.unwrap().at, at);
    // the new domain is offered only from its time
    let has_units = |clock: i64| (0..300).any(|s| w1.offers_at(s, clock).iter().any(|t| t.view.domain == Domain::Units));
    assert!(!has_units(at[0] - 1) && has_units(at[0]));
    // a logged being never sees units before the change, and its log does not carry the schedule
    let dir = tempfile::tempdir().unwrap();
    let (log, _) = run_drift_logged(dir.path(), 3, 1200, true);
    let rows: Vec<EventRow> = log.open_latest().rows();
    let genesis = rows.iter().find(|e| e.tags.contains("genesis")).unwrap();
    assert!(genesis.tags.contains("drift:on") && genesis.tags.contains("adapt:full"));
    for t in at {
        assert!(!genesis.tags.contains(&t.to_string()) && !genesis.text.contains(&t.to_string()));
    }
    let clock_of = |e: &EventRow| super::state::tag(&e.tags, "clock").and_then(|c| c.parse::<i64>().ok());
    let mut units_seen = 0;
    for e in rows.iter().filter(|e| e.kind == "outcome" && e.tags.contains("domain:units")) {
        units_seen += 1;
        assert!(clock_of(e).unwrap() > at[0], "units outcome before the new domain exists");
    }
    assert!(units_seen > 0, "the being tried the new domain");
}

#[test]
fn drift_has_a_new_domain_with_an_exact_checker_a_profile_shift_and_a_degrading_strategy() {
    let w = World::with_drift(7, false, Some(DRIFT_BUDGET));
    let d = w.drift.clone().unwrap();
    let st = [Default::default(); 5];
    let th = ScriptedThinker;
    for k in 1..=5u8 {
        for step in 0..60 {
            let t = w.task_forced(step, Domain::Units, k);
            assert!(t.check(t.expected()) && !t.check(t.expected() + 1));
            assert_eq!(t.view.domain, Domain::Units);
            let ctx = Context { step, task: &t.view, noise: 999_999, stats: &st, rules: &[], degraded: false };
            assert!(t.check(th.propose_with(&ctx, Strategy::Deep).answer), "deep solves every units template");
        }
    }
    // the shift: physics pays twice as much with an easier mix, arithmetic is cheap; the other domains are untouched
    assert!((d.shifted.reward_scale[Domain::Physics.idx()] - 2.0 * w.profile.reward_scale[Domain::Physics.idx()]).abs() < 0.02);
    assert_eq!(d.shifted.cost_base[Domain::Arithmetic.idx()], 1);
    for dom in [Domain::Algebra, Domain::Pattern, Domain::Units] {
        assert_eq!(d.shifted.reward_scale[dom.idx()], w.profile.reward_scale[dom.idx()]);
        assert_eq!(d.shifted.cost_base[dom.idx()], w.profile.cost_base[dom.idx()]);
    }
    assert_eq!(w.regime(d.at[1] - 1), 1);
    assert_eq!(w.regime(d.at[1]), 2);
    // the degradation: fast slips on easy exact tasks far more often; careful and deep are not affected
    let t = w.task_forced(1, Domain::Arithmetic, 1);
    let ok = |s: Strategy, degraded: bool| {
        let ctx = Context { step: 1, task: &t.view, noise: 500, stats: &st, rules: &[], degraded };
        t.check(th.propose_with(&ctx, s).answer)
    };
    assert!(ok(Strategy::Fast, false) && !ok(Strategy::Fast, true));
    assert!(ok(Strategy::Careful, true) && ok(Strategy::Deep, true));
    assert!(!w.degraded(d.at[2] - 1) && w.degraded(d.at[2]));
    // the old four domains keep their hidden profiles when the fifth exists
    assert_eq!(World::new(7).profile, w.profile);
}

#[test]
fn adaptive_run_is_deterministic_replays_exactly_and_logs_changes_with_evidence() {
    let world = world_with_alarms();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mut la, sa) = run_drift_logged(a.path(), world, 5000, true);
    let (lb, _) = run_drift_logged(b.path(), world, 5000, true);
    assert!(sa.state.step > 300, "a run with several hundred steps, got {}", sa.state.step);
    assert_eq!(la.open_latest().content_hash(), lb.open_latest().content_hash(), "same seeds, same log");
    verify_dir(la.dir()).unwrap();
    let rows: Vec<EventRow> = la.open_latest().rows();
    // replay: every decision is re-derived from the log alone, and the model and the flagged changes equal the live ones
    let rep = replay_decisions(&rows);
    assert_eq!(rep.len() as u64, sa.state.step);
    assert!(rep.iter().all(|r| r.matches()), "a decision of the adaptive learner was not re-derived");
    let refolded = BeingState::from_events(&rows);
    assert_eq!(refolded.model, sa.state.model);
    assert_eq!(refolded.changes, sa.state.changes);
    assert!(!sa.state.changes.is_empty());
    // every change is a logged note and a revision whose causes are outcome events of that group
    let snap = la.open_latest();
    let revs: Vec<RevisionRow> = snap.rows();
    let causes: Vec<CauseRow> = snap.rows();
    let notes: Vec<&EventRow> = rows.iter().filter(|e| e.kind == "note" && e.tags.split(',').any(|t| t == "change")).collect();
    assert_eq!(notes.len(), sa.state.changes.len());
    for (n, c) in notes.iter().zip(&sa.state.changes) {
        let key = super::state::tag(&n.tags, "rev_key").unwrap();
        let rev = revs.iter().find(|r| r.key == key).expect("revision of the change");
        assert_eq!((rev.entity_type.as_str(), rev.author.as_str()), ("belief", "rule:being-detector"));
        let cited: Vec<&EventRow> = causes.iter().filter(|x| x.rev_id == rev.rev_id).filter_map(|x| rows.iter().find(|e| e.event_id == x.event_id)).collect();
        let outcomes: Vec<&&EventRow> = cited.iter().filter(|e| e.kind == "outcome").collect();
        assert!(outcomes.len() >= 3 && outcomes.len() == c.cites.len(), "a change cites the outcomes it rests on");
        for o in outcomes {
            assert!(o.tags.contains(&format!("domain:{}", c.alarm.domain.name())) && o.tags.contains(&format!("strategy:{}", c.alarm.strategy.name())));
        }
        assert!(cited.iter().any(|e| e.event_id == n.event_id));
        assert!(n.text.contains("detected at step"));
    }
    // a run split over two calls equals one run, even with forgetting and detection
    let d2 = tempfile::tempdir().unwrap();
    let (mut ls, _) = run_drift_logged(d2.path(), world, 150, true);
    run(&mut ls, &RunCfg { steps: 5000, ..Default::default() }, &ScriptedThinker, &Policy::new(5000)).unwrap();
    let ids = |l: &VersionedLog| l.open_latest().rows::<EventRow>().into_iter().map(|e| e.event_id).collect::<Vec<_>>();
    assert_eq!(ids(&ls), ids(&la));
    // the options are kept once the being exists; a being with adapt needs the menu world
    assert!(run(&mut la, &RunCfg { adapt: Some(false), steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1)).is_err());
    assert!(run(&mut la, &RunCfg { drift: Some(false), steps: 1, ..Default::default() }, &ScriptedThinker, &Policy::new(1)).is_err());
    let d3 = tempfile::tempdir().unwrap();
    let mut l3 = open(d3.path());
    assert!(run(&mut l3, &RunCfg { adapt: Some(true), ..cfg(1, 1, 5, 500) }, &ScriptedThinker, &Policy::new(5)).is_err());
    assert!(run(&mut l3, &RunCfg { drift: Some(true), ..cfg(1, 1, 5, 500) }, &ScriptedThinker, &Policy::new(5)).is_err());
    assert!(BeingState::from_snapshot(&l3.open_latest()).genesis.is_none(), "a refused request creates no being");
}

#[test]
fn forgetting_without_detection_replays_and_the_detector_is_silent_without_it() {
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = run_drift_logged(dir.path(), 5, 400, false);
    assert_eq!(s.state.model.mode(), AdaptMode::Off);
    assert!(s.state.changes.is_empty());
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(replay_decisions(&rows).iter().all(|r| r.matches()));
    // the forgetting-only mode (measurement setting) never flags anything and its model is a function of the outcomes
    let r = run_drift(5, DRIFT_BUDGET, true, &AdaptSpec::forget(), &mut ModelChooser::new(DEFAULT_BETA));
    assert!(r.alarms.is_empty());
    let r2 = run_drift(5, DRIFT_BUDGET, true, &AdaptSpec::forget(), &mut ModelChooser::new(DEFAULT_BETA));
    assert_eq!((r.reward, r.cost, r.recs.len()), (r2.reward, r2.cost, r2.recs.len()));
}

#[test]
fn tamper_after_an_adaptive_drift_run_fails_verify() {
    let dir = tempfile::tempdir().unwrap();
    let (log, _) = run_drift_logged(dir.path(), world_with_alarms(), 5000, true);
    verify_dir(log.dir()).unwrap();
    let seg = std::fs::read_dir(log.dir().join("events")).unwrap().flatten().map(|f| f.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).last().unwrap();
    let mut bytes = std::fs::read(&seg).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&seg, bytes).unwrap();
    assert!(verify_dir(log.dir()).is_err());
}

#[test]
fn adaptation_code_reads_only_the_log_not_the_schedule_or_hidden_parameters() {
    let forbidden = [
        "Profile", "profile", "reward_scale", "cost_base", "diff_bias", ".expected(", "noise(", "task_forced", "task_of_kind", "truth", "World", "generate",
        "Kind::", "Drift", "regime", "FAST_DEGRADED", "degraded", "schedule", "DRIFT_FRACTIONS", "offers_at", "pattern_bonus",
    ];
    let dir_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being");
    for f in ["model.rs", "planner.rs"] {
        let src = std::fs::read_to_string(dir_src.join(f)).unwrap();
        for b in forbidden {
            assert!(!src.contains(b), "{f} mentions {b}");
        }
    }
    // the state folds outcomes and notes only; it does not know the schedule either
    let src = std::fs::read_to_string(dir_src.join("state.rs")).unwrap();
    for b in ["Drift", "regime", "FAST_DEGRADED", "DRIFT_FRACTIONS", "offers_at", "reward_scale", "diff_bias"] {
        assert!(!src.contains(b), "state.rs mentions {b}");
    }
}

#[test]
fn measured_adaptation_is_close_to_the_oracle_does_not_cost_much_and_detects_the_degradation() {
    // floors set below the measured values (docs/measured.md); report seeds 1..6 of the 20, budget 6000
    use super::measure::{schedule_energy, ScheduleOracle};
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (mut off, mut full, mut ucb, mut oracle, mut stat_off, mut stat_full) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    let (mut detected, mut stationary_alarms) = (0, 0);
    for seed in 1..=6u64 {
        let w = World::with_drift(seed, false, Some(DRIFT_BUDGET));
        let mut orc = ScheduleOracle { before: super::measure::truth_flag(&w, &[], false), after: super::measure::truth_flag(&w, &[], true), degrade_at: w.drift.as_ref().unwrap().at[2] };
        let mk = |sp: &AdaptSpec| ModelChooser { novelty: sp.novelty, ..ModelChooser::new(DEFAULT_BETA) };
        let (o, f) = (AdaptSpec::off(), AdaptSpec::full());
        let r_off = run_drift(seed, DRIFT_BUDGET, true, &o, &mut mk(&o));
        let r_full = run_drift(seed, DRIFT_BUDGET, true, &f, &mut mk(&f));
        off.push(r_off.per_budget());
        full.push(r_full.per_budget());
        ucb.push(run_drift(seed, DRIFT_BUDGET, true, &o, &mut super::chooser::UcbMenu::new(seed, super::chooser::DEFAULT_EXPLORE_C)).per_budget());
        oracle.push(run_drift(seed, DRIFT_BUDGET, true, &o, &mut orc).per_budget());
        let at = schedule_energy(seed, DRIFT_BUDGET);
        detected += r_full.alarms.iter().any(|a| a.energy >= at[2] && a.strategy == Strategy::Fast && a.drop) as u32;
        let (s_off, s_full) = (run_drift(seed, DRIFT_BUDGET, false, &o, &mut mk(&o)), run_drift(seed, DRIFT_BUDGET, false, &f, &mut mk(&f)));
        stat_off.push(s_off.per_budget());
        stat_full.push(s_full.per_budget());
        stationary_alarms += s_full.alarms.len();
    }
    println!("off {:.3} full {:.3} ucb {:.3} oracle {:.3} | stationary off {:.3} full {:.3} | detected {detected} of 6, stationary alarms {stationary_alarms}", mean(&off), mean(&full), mean(&ucb), mean(&oracle), mean(&stat_off), mean(&stat_full));
    assert!(mean(&full) > mean(&ucb) + 1.0, "adaptive {} ucb {}", mean(&full), mean(&ucb));
    assert!(mean(&off) > mean(&ucb) + 1.0);
    assert!(mean(&oracle) - mean(&full) < 0.5, "regret {}", mean(&oracle) - mean(&full));
    assert!(mean(&full) > mean(&off) - 0.3, "adaptation must not cost much in the drifting world");
    assert!(mean(&stat_full) > mean(&stat_off) - 0.3, "adaptation must not cost much in the stationary world");
    assert!(detected >= 2, "the degradation of fast is flagged in {detected} of 6 worlds");
    assert!(stationary_alarms <= 2, "false alarms: {stationary_alarms}");
}
