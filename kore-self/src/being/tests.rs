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
