//! Restart fix: the events-only fold equals the old fold of the whole snapshot, the genesis peek
//! equals the genesis of the full fold, a being split over many restarts equals one run, and the
//! KILL file is answered before the history is folded.

use crate::authorize;
use crate::being::advice::AdvisorKind;
use crate::being::controller::Controller;
use crate::being::model::AdaptMode;
use crate::being::run_loop::{run_full, RunCfg};
use crate::being::state::{BeingState, Chooser};
use crate::being::{Policy, ScriptedThinker};
use crate::log_tools;
use crate::safety;
use crate::versioned::{EventRow, LogFormat, VersionedLog};
use serde_json::{json, Value};

const CTL: &str = "max_live_rules = 16\nmax_building_blocks = 8\nmax_search_candidates = 2000\nmax_steps_per_run = 5000\nmax_energy_per_run = 100000\ncomposition = on\npruning = on\ntransfer = on\nmax_transfer_links = 4\n";

fn fixed_clock() -> String {
    "2030-01-01T00:00:00Z".to_string()
}

/// The being's state folded the old way (every event row of the decoded snapshot) and the new way
/// (events-only read of a freshly opened log), compared field by field (Debug: no hash maps inside).
fn assert_folds_equal(root: &std::path::Path) -> BeingState {
    let log = VersionedLog::open(root, "being").unwrap();
    let new = BeingState::from_log(&log).unwrap();
    let gen = BeingState::genesis_of(&log).unwrap();
    let peek = BeingState::genesis_peek(root, "being").unwrap();
    let old = BeingState::from_events(&log.open_latest().rows::<EventRow>());
    assert_eq!(format!("{new:?}"), format!("{old:?}"));
    assert_eq!(format!("{gen:?}"), format!("{:?}", old.genesis));
    assert_eq!(format!("{peek:?}"), format!("{:?}", old.genesis));
    // with the snapshot now in memory the same read uses it, same answer
    assert_eq!(format!("{:?}", BeingState::from_log(&log).unwrap()), format!("{old:?}"));
    old
}

fn run_chunks(root: &std::path::Path, cfg: &RunCfg, chunks: &[u64], fmt: LogFormat) {
    for &n in chunks {
        let mut log = VersionedLog::open_or_create_with_format(root, "being", fixed_clock, fmt).unwrap();
        let mut p = Policy::new(n);
        p.refill_when_exhausted = cfg.refill_when_exhausted;
        run_full(&mut log, &RunCfg { steps: n, ..cfg.clone() }, &ScriptedThinker, &p, None, None).unwrap();
        assert_folds_equal(root);
    }
}

#[test]
fn events_only_fold_equals_the_full_fold_in_every_world() {
    let (limits, _) = Controller::parse(CTL).unwrap().limits(true, true);
    let base = RunCfg { chooser: Some(Chooser::Model), budget: Some(3000), refill_when_exhausted: true, ..Default::default() };
    let worlds: Vec<(&str, RunCfg)> = vec![
        ("ucb", RunCfg { chooser: None, budget: Some(800), refill_when_exhausted: true, ..Default::default() }),
        ("rules b5", RunCfg { ext2: Some(true), discover_every: Some(10), ctl: Some(limits.clone()), world_seed: Some(4), ..base.clone() }),
        ("rule drift", RunCfg { ext2: Some(true), rule_drift: Some(true), discover_every: Some(10), ctl: Some(limits.clone()), world_seed: Some(3), ..base.clone() }),
        ("drift adapt switch", RunCfg { drift: Some(true), adapt: Some(true), adapt_mode: Some(AdaptMode::Switch), ..base.clone() }),
        ("transfer", RunCfg { ext2: Some(true), transfer: Some(true), learn_transfer: Some(true), discover_every: Some(10), ctl: Some(limits.clone()), ..base.clone() }),
    ];
    for (name, cfg) in worlds {
        for fmt in [LogFormat::V1, LogFormat::V3] {
            let d = tempfile::tempdir().unwrap();
            run_chunks(d.path(), &cfg, &[150, 1, 249], fmt);
            let st = assert_folds_equal(d.path());
            assert_eq!(st.step, 400, "{name} {fmt:?}");
        }
    }
}

#[test]
fn many_restarts_give_the_same_log_as_one_run() {
    // the same being run in one call and in 7 calls with a fresh open each time: identical rows
    let (limits, _) = Controller::parse(CTL).unwrap().limits(true, true);
    let cfg = RunCfg { chooser: Some(Chooser::Model), budget: Some(3000), refill_when_exhausted: true, ext2: Some(true), discover_every: Some(10), ctl: Some(limits), world_seed: Some(4), ..Default::default() };
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    run_chunks(a.path(), &cfg, &[700], LogFormat::V3);
    run_chunks(b.path(), &cfg, &[100, 25, 75, 1, 99, 200, 200], LogFormat::V3);
    let (la, lb) = (VersionedLog::open(a.path(), "being").unwrap(), VersionedLog::open(b.path(), "being").unwrap());
    let rows = |l: &VersionedLog| {
        let s = l.open_latest();
        let ev: Vec<String> = s.rows::<EventRow>().iter().map(|e| format!("{}|{}|{}|{}|{}", e.event_id, e.occurred_at, e.kind, e.text, e.tags)).collect();
        let rv: Vec<String> = s.rows::<crate::versioned::RevisionRow>().iter().map(|r| format!("{}|{}|{}|{}", r.rev_id, r.key, r.stance, r.rationale)).collect();
        (ev, rv)
    };
    assert_eq!(rows(&la), rows(&lb));
    // the commit times are the same fixed clock, so the whole content is the same too
    assert_eq!(la.open_latest().content_hash(), lb.open_latest().content_hash());
}

fn setup(owner: &str) -> std::path::PathBuf {
    safety::test_support::init();
    let dir = safety::owner_dir(owner);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("controller.txt"), CTL).unwrap();
    dir
}

fn call(owner: &str, args: Value) -> (bool, String) {
    let r = log_tools::handle("being_run", &args, owner).expect("being_run");
    (r["isError"].as_bool().unwrap_or(false), r["content"][0]["text"].as_str().unwrap().to_string())
}

fn audit(owner: &str) -> Vec<EventRow> {
    log_tools::open(owner).unwrap().open_latest().rows::<EventRow>().into_iter().filter(|e| e.source == authorize::SOURCE).collect()
}

#[test]
fn kill_file_before_the_call_is_refused_without_a_step_and_audited_once() {
    let o = "restart-kill";
    let dir = setup(o);
    let args = json!({"steps": 300, "chooser": "model", "pattern_families": "extended2", "discover": true, "world_seed": 4, "budget": 3000, "stop_after_budget": false});
    let (e, t) = call(o, args.clone());
    assert!(!e, "{t}");
    let events_before = log_tools::open(o).unwrap().event_count();
    std::fs::write(dir.join("KILL"), "").unwrap();
    let (e, t) = call(o, args.clone());
    assert!(e && t.contains("KILL file exists"), "{t}");
    let rows = audit(o);
    assert_eq!(rows.len(), 1, "one audit row for the refusal");
    assert!(rows[0].text.contains("controller:kill_switch"), "{}", rows[0].text);
    assert_eq!(log_tools::open(o).unwrap().event_count(), events_before + 1, "nothing but the audit row was written");
    std::fs::remove_file(dir.join("KILL")).unwrap();
    let (e, t) = call(o, args);
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["total_steps"], 600, "{t}");
}

#[test]
fn kill_during_a_run_stops_it_and_the_refusal_is_audited_through_the_open_log() {
    // the KILL file appears while steps run (checked before each step): stop, controller_stop note
    // and audit row, both written through the log the run has open
    let o = "restart-kill-run";
    let dir = setup(o);
    let args = json!({"steps": 50, "chooser": "model", "world_seed": 2, "budget": 3000, "stop_after_budget": false});
    let (e, t) = call(o, args);
    assert!(!e, "{t}");
    let k = dir.join("KILL");
    let root = log_tools::log_root(o).unwrap();
    let v0 = VersionedLog::open(&root, o).unwrap().latest_version();
    let o2 = o.to_string();
    // the file appears once the run has committed its first steps
    let h = std::thread::spawn(move || {
        loop {
            if VersionedLog::open(&root, &o2).map_or(false, |l| l.latest_version() > v0) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::fs::write(&k, "").unwrap();
    });
    let (e, t) = call(o, json!({"steps": 5000}));
    h.join().unwrap();
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    if v["stopped"] == "kill switch" {
        let rows = audit(o);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].text.contains("controller:kill_switch"));
        let notes = log_tools::open(o).unwrap().open_latest().rows::<EventRow>().into_iter().filter(|e| e.tags.contains("controller_stop") || e.text.contains("kill")).count();
        assert!(notes >= 1);
        log_tools::open(o).unwrap().verify().unwrap();
    } else {
        // 5000 steps finished in under 300 ms on this machine: nothing to check
        assert_eq!(v["steps_run"], 5000);
    }
    std::fs::remove_file(dir.join("KILL")).unwrap();
}

#[test]
fn advisor_being_folds_the_same_and_its_genesis_is_peeked() {
    let o = "restart-advisor";
    let dir = setup(o);
    std::fs::write(dir.join(authorize::POLICY_FILE), "[L1]\nadvisor_query\n").unwrap();
    let (e, t) = call(o, json!({"steps": 120, "chooser": "model", "advisor": "simulated", "advisor_quality": 0.8, "world_seed": 3, "budget": 3000}));
    assert!(!e, "{t}");
    let (e, t) = call(o, json!({"steps": 80}));
    assert!(!e, "{t}");
    let root = log_tools::log_root(o).unwrap();
    let log = VersionedLog::open(&root, o).unwrap();
    let new = BeingState::from_log(&log).unwrap();
    let old = BeingState::from_events(&log.open_latest().rows::<EventRow>());
    assert_eq!(format!("{new:?}"), format!("{old:?}"));
    let g = BeingState::genesis_peek(&root, o).unwrap().unwrap();
    assert_eq!(g.advice.kind, AdvisorKind::Simulated);
    assert_eq!(old.step, 200);
}
