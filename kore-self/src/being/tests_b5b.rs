//! Tests of Track B5, part 2: logged runs, controller caps, kill switch, pruning.

use super::controller::{Controller, Limits, Origin, PruneCfg};
use super::discover::{Example, Form, Rule};
use super::run_loop::{run, Engine, RunCfg};
use super::solver::ScriptedThinker;
use super::state::{tag, BeingState, Chooser, Genesis, SOURCE};
use super::world::World;
use super::{Policy, StopReason};
use crate::versioned::{CauseRow, EventRow, RevisionRow, VersionedLog};

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn cfg2(world: u64, steps: u64, budget: i64, ctl: Limits, drift: bool) -> RunCfg {
    RunCfg {
        world_seed: Some(world),
        loop_seed: Some(1),
        budget: Some(budget),
        chooser: Some(Chooser::Model),
        steps,
        ext2: Some(true),
        rule_drift: if drift { Some(true) } else { None },
        discover_every: Some(10),
        ctl: Some(ctl),
        ..Default::default()
    }
}

fn compose_limits() -> Limits {
    Limits { compose: true, ..Default::default() }
}

fn events(log: &VersionedLog) -> Vec<EventRow> {
    log.open_latest().rows()
}

#[test]
fn controller_parses_defaults_and_fails_closed() {
    let d = Controller::parse("").unwrap();
    assert_eq!(d, Controller::defaults());
    assert!(!d.composition && !d.pruning && !d.is_closed());
    let c = Controller::parse("# owner\nmax_live_rules = 5 # few\nmax_building_blocks=3\nmax_search_candidates = 100\nmax_steps_per_run = 50\nmax_energy_per_run = 400\ncomposition = on\npruning = true\n").unwrap();
    assert_eq!((c.max_live_rules, c.max_building_blocks, c.max_search_candidates, c.max_steps_per_run, c.max_energy_per_run, c.composition, c.pruning), (5, 3, 100, 50, 400, true, true));
    for bad in ["bogus = 1", "max_live_rules = 5\nmax_live_rules = 6", "max_live_rules = 5000", "max_steps_per_run = -1", "max_live_rules 5", "composition = maybe", "max_energy_per_run = 99999999"] {
        assert!(Controller::parse(bad).is_err(), "{bad}");
        let f = Controller::parse_or_closed(bad);
        assert!(f.is_closed(), "{bad}");
        assert_eq!((f.max_live_rules, f.max_building_blocks, f.max_search_candidates, f.max_steps_per_run, f.max_energy_per_run, f.composition, f.pruning), (0, 0, 0, 0, 0, false, false));
    }
    assert!(matches!(Controller::fail_closed("x").origin, Origin::FailClosed(_)));
    // a request the controller does not allow is refused, not honoured
    let (l, r) = Controller::defaults().limits(true, true);
    assert!(!l.compose && !l.prune && r.len() == 2);
    let (l, r) = c.limits(true, true);
    assert!(l.compose && l.prune && r.is_empty());
}

#[test]
fn kill_switch_stops_within_one_step_before_and_during_a_run() {
    use super::policy::KillCheck;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    // already on: nothing runs
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(|| true)));
    let s = run(&mut log, &cfg2(1, 100, 3000, Limits::default(), false), &ScriptedThinker, &pol).unwrap();
    assert_eq!((s.steps_run, s.stopped), (0, StopReason::Killed));
    // turns on during the 13th check: exactly 12 steps ran, and the stop is in the log
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let calls = Arc::new(AtomicU64::new(0));
    let c2 = calls.clone();
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 13)));
    let s = run(&mut log, &cfg2(1, 100, 3000, Limits::default(), false), &ScriptedThinker, &pol).unwrap();
    assert_eq!((s.steps_run, s.stopped), (12, StopReason::Killed));
    let stops: Vec<EventRow> = events(&log).into_iter().filter(|e| e.tags.split(',').any(|t| t == "controller_stop")).collect();
    assert_eq!(stops.len(), 1);
    assert!(stops[0].tags.contains("stop:kill_switch"));
    assert_eq!(BeingState::from_events(&events(&log)).step, 12);
}

#[test]
fn energy_cap_stops_the_run_and_is_logged() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut pol = Policy::new(1000);
    pol.max_energy = Some(120);
    let s = run(&mut log, &cfg2(2, 1000, 3000, Limits::default(), false), &ScriptedThinker, &pol).unwrap();
    assert_eq!(s.stopped, StopReason::EnergyCap);
    assert!(s.cost >= 120 && s.cost < 120 + 30, "cost {}", s.cost);
    let ev = events(&log);
    assert!(ev.iter().any(|e| e.tags.contains("cap:max_energy_per_run") && e.tags.contains("controller_refusal")));
    assert!(ev.iter().any(|e| e.tags.contains("controller_stop")));
    // a cap of 0 (fail closed) runs nothing
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut pol = Policy::new(1000);
    pol.max_energy = Some(0);
    let s = run(&mut log, &cfg2(2, 1000, 3000, Limits::default(), false), &ScriptedThinker, &pol).unwrap();
    assert_eq!((s.steps_run, s.stopped), (0, StopReason::EnergyCap));
}

#[test]
fn live_rule_cap_refusal_is_logged_never_silent() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let lim = Limits { compose: true, max_live_rules: 2, ..Default::default() };
    let s = run(&mut log, &cfg2(3, 600, 4000, lim, false), &ScriptedThinker, &Policy::new(600)).unwrap();
    assert!(s.state.live_rules() <= 2, "{} live", s.state.live_rules());
    assert!(!s.refusals.is_empty(), "this world finds more than 2 rules without a cap");
    let logged: Vec<EventRow> = events(&log).into_iter().filter(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:max_live_rules")).collect();
    assert_eq!(logged.len(), s.refusals.iter().filter(|r| r.cap == "max_live_rules").count());
    assert!(logged.iter().all(|e| e.tags.contains("limit:2") && e.source == SOURCE));
    // replaying the log gives the same book
    assert_eq!(BeingState::from_events(&events(&log)).rules.len(), s.state.rules.len());
}

#[test]
fn block_and_candidate_caps_are_refusals_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let lim = Limits { compose: true, max_blocks: 1, max_candidates: 30, ..Default::default() };
    let s = run(&mut log, &cfg2(3, 400, 4000, lim, false), &ScriptedThinker, &Policy::new(400)).unwrap();
    let caps: Vec<&str> = s.refusals.iter().map(|r| r.cap.as_str()).collect();
    assert!(caps.contains(&"max_search_candidates"), "{caps:?}");
    let logged = events(&log).into_iter().filter(|e| e.tags.contains("controller_refusal")).count();
    assert_eq!(logged, s.refusals.len());
}

#[test]
fn composed_rules_are_logged_revisions_citing_examples_and_rederive_from_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run(&mut log, &cfg2(3, 600, 4000, compose_limits(), false), &ScriptedThinker, &Policy::new(600)).unwrap();
    let composed: Vec<&Rule> = s.state.rules.iter().filter(|r| !r.xf.is_id()).collect();
    assert!(!composed.is_empty(), "world 3 composes a rule within 600 steps");
    let snap = log.open_latest();
    let evs: Vec<EventRow> = snap.rows();
    let revs: Vec<RevisionRow> = snap.rows();
    let causes: Vec<CauseRow> = snap.rows();
    for r in &composed {
        let note = evs.iter().find(|e| e.event_id == r.event_id).expect("rule note in the log");
        assert_eq!(tag(&note.tags, "xf"), Some(r.xf.code()));
        let rev = revs.iter().find(|v| v.key == format!("being/rule/{}", r.id)).expect("revision");
        assert!(rev.rationale.contains("composition"));
        let cited: Vec<&EventRow> = causes.iter().filter(|c| c.rev_id == rev.rev_id).filter_map(|c| evs.iter().find(|e| e.event_id == c.event_id)).filter(|e| e.kind == "outcome").collect();
        assert!(cited.len() >= 4, "{} cited examples", cited.len());
        assert!(cited.iter().all(|e| tag(&e.tags, "terms").is_some() && tag(&e.tags, "next").is_some()));
    }
    let re = super::replay::rederive_rules(&evs);
    assert!(re.len() >= composed.len() && re.iter().all(|x| x.1), "every rule, composed or not, is re-derived from the log: {re:?}");
    let st = BeingState::from_events(&evs);
    assert_eq!(st.rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), s.state.rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
}

#[test]
fn composition_is_off_unless_asked_and_allowed() {
    // without compose the book has plain rules only, and a run is identical to the B2 grammar search
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run(&mut log, &cfg2(3, 300, 4000, Limits::default(), false), &ScriptedThinker, &Policy::new(300)).unwrap();
    assert!(s.state.rules.iter().all(|r| r.xf.is_id()));
}

#[test]
fn merge_retires_a_rule_a_simpler_one_always_explains() {
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed: 1, loop_seed: 0, budget: 100, explore_c: 0.0, chooser: Chooser::Model, extended: true, ext2: true, rule_drift: false, drift: false, adapt: Default::default(), advice: Default::default(), transfer: false, learn_transfer: false }, String::new());
    for i in 0..9i64 {
        let terms: Vec<i64> = (0..6).map(|n| 2 + (i + 1) * n).collect();
        st.examples.push(Example { event_id: format!("e{i}"), sig: 1, terms, next: 2 + (i + 1) * 6 });
    }
    st.apply_rule(Rule::new(1, Form::Poly(2), 3, 1, 0));
    st.apply_rule(Rule::new(1, Form::Poly(1), 3, 1, 0));
    // pruning off: nothing happens
    let mut off = Engine::new(World::with_ext2(1, None), st.clone(), &th).with_limits(Some(Limits::default()));
    assert!(off.housekeeping().is_empty());
    let mut e = Engine::new(World::with_ext2(1, None), st, &th).with_limits(Some(Limits { prune: true, prune_cfg: PruneCfg::default(), ..Default::default() }));
    let out = e.housekeeping();
    assert_eq!(out.len(), 1);
    assert!(out[0].0.tags.contains(&"reason:merged".to_string()) && out[0].0.tags.iter().any(|t| t.starts_with("into:")));
    assert!(out[0].1.caused_by.len() >= 5 && out[0].1.stance.starts_with("merged"));
    let live: Vec<_> = e.state.rules.iter().filter(|r| !r.retired).map(|r| r.form).collect();
    assert_eq!(live, vec![Form::Poly(1)]);
    assert_eq!(e.state.rules.len(), 2, "nothing is deleted");
}

#[test]
fn pruning_retires_a_wrong_rule_as_a_logged_revision_and_time_travel_shows_it() {
    let lim = Limits { compose: true, prune: true, prune_cfg: PruneCfg { window: 10, min_n: 2, threshold: 0.9, merge_min: 5 }, ..Default::default() };
    for world in 1..=12u64 {
        let dir = tempfile::tempdir().unwrap();
        let mut log = open(dir.path());
        let s = run(&mut log, &cfg2(world, 700, 4000, lim.clone(), true), &ScriptedThinker, &Policy::new(700)).unwrap();
        if s.retired.is_empty() {
            continue;
        }
        let snap = log.open_latest();
        let evs: Vec<EventRow> = snap.rows();
        let revs: Vec<RevisionRow> = snap.rows();
        let causes: Vec<CauseRow> = snap.rows();
        let id = &s.retired[0];
        let note = evs.iter().find(|e| tag(&e.tags, "rule_id") == Some(id.as_str()) && e.tags.contains("rule_retired")).expect("retire note");
        let chain: Vec<&RevisionRow> = revs.iter().filter(|v| v.key == format!("being/rule/{id}")).collect();
        assert!(chain.len() >= 2, "accepted revision then retirement revision");
        let last = chain.last().unwrap();
        assert!(last.stance.starts_with("retired") || last.stance.starts_with("merged"));
        assert_eq!(last.supersedes.as_deref(), Some(chain[chain.len() - 2].rev_id.as_str()));
        assert!(causes.iter().filter(|c| c.rev_id == last.rev_id).count() >= 2);
        assert!(causes.iter().any(|c| c.rev_id == last.rev_id && c.event_id == note.event_id));
        // time travel: the log before the retirement note still has the rule live
        let st_now = BeingState::from_events(&evs);
        assert!(st_now.rules.iter().find(|r| &r.id == id).unwrap().retired);
        let before: Vec<EventRow> = evs.iter().take_while(|e| e.event_id != note.event_id).cloned().collect();
        let st_before = BeingState::from_events(&before);
        assert!(!st_before.rules.iter().find(|r| &r.id == id).unwrap().retired);
        assert_eq!(st_now.rules.len(), s.state.rules.len(), "nothing deleted");
        assert_eq!(st_now.live_rules(), s.state.live_rules());
        return;
    }
    panic!("no retirement in 12 drifting worlds");
}

#[test]
fn controller_sources_have_no_file_or_network_access_and_names_no_hidden_world() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being");
    let src = std::fs::read_to_string(dir.join("controller.rs")).unwrap();
    for b in [["std", "::fs"].concat(), ["std", "::net"].concat(), ["std", "::env"].concat(), ["Command", "::new"].concat(), ["File", "::open"].concat()] {
        assert!(!src.contains(b.as_str()), "controller.rs uses {b}");
    }
    // the being never learns which family a task came from
    let d = std::fs::read_to_string(dir.join("discover.rs")).unwrap();
    for b in ["Ext2", "DriftAff", "NegLcg", "NegMod7", "rule_drift", "task_of_kind"] {
        assert!(!d.contains(b), "discover.rs mentions {b}");
    }
}
