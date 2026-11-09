//! Tests of Track B14 (rule library, curiosity, their controller keys). Temp dirs only.

use super::super::controller::{Controller, Limits};
use super::super::discover::{rule_id_x, Example, Form, Rule, Xf};
use super::super::measure_b11::{genesis, LONGRUN_CONTROLLER};
use super::super::planner::{MenuChooser, ModelChooser};
use super::super::run_loop::{offers_of, run, world_for, Engine, RunCfg};
use super::super::solver::ScriptedThinker;
use super::super::state::{BeingState, Chooser, SOURCE};
use super::super::{Policy, StopReason};
use super::ctl::{B14Ctl, B14Setup, HARD_CURIOSITY_PM};
use super::curiosity::{expected_improvement, CurCfg, Curious, CUR_POLICY, DEFAULT_CUR};
use super::library::{check_candidate, wilson_ub, LibCfg, LibEntry, IMPORT_TAG};
use super::measure::{run_b14, RunCfgB14};
use crate::versioned::{CauseRow, EventRow, RevisionRow, VersionedLog};

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn events(log: &VersionedLog) -> Vec<EventRow> {
    log.open_latest().rows()
}

fn row(id: &str, kind: &str, text: &str, tags: String) -> EventRow {
    EventRow { event_id: id.into(), occurred_at: String::new(), recorded_at: String::new(), source: SOURCE.into(), kind: kind.into(), text: text.into(), tags, redacted: false }
}

const CTL_ON: &str = "max_live_rules = 16\nmax_building_blocks = 8\nmax_search_candidates = 2000\nmax_steps_per_run = 5000\ncomposition = on\npruning = on\nrule_library = on\ncuriosity = on\nmax_curiosity_energy_pm = 20\n";

fn limits() -> Limits {
    Controller::parse(LONGRUN_CONTROLLER).unwrap().limits(true, true).0
}

fn cfg(world: u64, steps: u64, b14: Option<B14Setup>, ctl: Option<&str>) -> RunCfg {
    RunCfg {
        world_seed: Some(world),
        loop_seed: Some(1),
        budget: Some(3000),
        chooser: Some(Chooser::Model),
        steps,
        ext2: Some(true),
        discover_every: Some(10),
        ctl: Some(limits()),
        refill_when_exhausted: true,
        b14,
        b14_ctl: ctl.map(|t| B14Ctl::from_text(Some(t))),
        ..Default::default()
    }
}

fn pol(n: u64) -> Policy {
    let mut p = Policy::new(n);
    p.refill_when_exhausted = true;
    p
}

fn both() -> Option<B14Setup> {
    Some(B14Setup { library: true, curiosity_pm: 20 })
}

fn cited(causes: &[CauseRow], rev: &str) -> Vec<String> {
    causes.iter().filter(|c| c.rev_id == rev).map(|c| c.event_id.clone()).collect()
}

#[test]
fn controller_keys_validate_coexist_and_fail_closed() {
    // every opt-in key of the earlier tracks and the B14 keys in one file
    let all = "experiments = on\nmax_experiment_energy_pm = 50\nmax_experiments_per_run = 10\ncontext_chooser = on\nworld_t_run = on\nrules_use = on\nrule_retirement = on\nrule_library = on\ncuriosity = on\nmax_curiosity_energy_pm = 20\n";
    assert!(Controller::parse(all).is_ok(), "{:?}", Controller::parse(all));
    let c = B14Ctl::from_text(Some(all));
    assert_eq!((c.library, c.curiosity, c.curiosity_pm, c.closed.is_none()), (true, true, 20, true));
    // each key given twice is refused, and so are bad values and values above the hard maximum
    for bad in [
        "rule_library = on\nrule_library = on",
        "curiosity = on\ncuriosity = off",
        "max_curiosity_energy_pm = 5\nmax_curiosity_energy_pm = 5",
        "rule_library = maybe",
        "curiosity = 2x",
        "max_curiosity_energy_pm = -1",
        "max_curiosity_energy_pm = 101",
        "context_chooser = on\nrule_library = on\ncontext_chooser = on",
        "world_t_run = on\ncuriosity = on\nworld_t_run = off",
    ] {
        assert!(Controller::parse(bad).is_err(), "{bad}");
        let c = B14Ctl::from_text(Some(bad));
        assert!(c.closed.is_some() && !c.library && !c.curiosity && c.curiosity_pm == 0, "{bad}");
    }
    assert_eq!(HARD_CURIOSITY_PM, 100);
    // default off: a valid file without the keys, a missing file
    for t in [Some("max_steps_per_run = 50\n"), None] {
        let c = B14Ctl::from_text(t);
        let (g, r) = c.grant(&B14Setup { library: true, curiosity_pm: 10 });
        assert!(g.is_off() && r.len() == 2, "{t:?}");
    }
    // curiosity needs both the switch and a budget; the budget is capped by the controller
    let (g, r) = B14Ctl::from_text(Some("curiosity = on\n")).grant(&B14Setup { library: false, curiosity_pm: 10 });
    assert!(g.is_off() && r.len() == 1);
    let (g, r) = B14Ctl::from_text(Some("curiosity = on\nmax_curiosity_energy_pm = 5\n")).grant(&B14Setup { library: false, curiosity_pm: 10 });
    assert_eq!((g.curiosity_pm, r.len()), (5, 1));
    // setup tags round trip; bad tags give nothing
    let s = B14Setup { library: true, curiosity_pm: 7 };
    assert_eq!(B14Setup::from_tags(&s.tags().join(",")), Some(s));
    assert_eq!(B14Setup::from_tags("b14_setup,rule_library:yes,curiosity_pm:1"), None);
    assert_eq!(B14Setup::from_tags("b14_setup,rule_library:on,curiosity_pm:500"), None);
}

#[test]
fn off_is_the_planner_bit_for_bit() {
    let g = genesis(3, 3000);
    let w = world_for(&g);
    let mut st = BeingState::default();
    st.start(g.clone(), String::new());
    let th = ScriptedThinker;
    let mut e1 = Engine::new(w.clone(), st.clone(), &th).with_limits(Some(limits()));
    let mut e2 = Engine::new(w.clone(), st, &th).with_limits(Some(limits()));
    let mut mc = ModelChooser::for_genesis(&g);
    let mut cu = Curious::new(ModelChooser::for_genesis(&g), &w, None, None);
    for _ in 0..1500 {
        let a = e1.step_menu(&mut mc);
        let b = e2.step_menu(&mut cu);
        match (a, b) {
            (Ok(a), Ok(b)) => assert_eq!(a.events.iter().map(|x| &x.text).collect::<Vec<_>>(), b.events.iter().map(|x| &x.text).collect::<Vec<_>>()),
            (Err(_), Err(_)) => {
                let c = e1.state.clock;
                e1.state.apply_refill(3000, c + 86_400);
                e2.state.apply_refill(3000, c + 86_400);
            }
            _ => panic!("diverged"),
        }
        if e1.state.step % 10 == 0 {
            let _ = e1.discover_pass();
            let _ = e2.discover_pass();
        }
    }
    // and the measurement loop with everything off has no curiosity and no library record
    let o = run_b14(&RunCfgB14::base(3, 3000, 1000));
    assert_eq!((o.cur_energy, o.wins.iter().map(|w| w.lib_reused).sum::<u64>()), (0, 0));
}

#[test]
fn curiosity_stays_within_its_budget_and_never_stops() {
    let c = RunCfgB14 { curiosity: Some(CurCfg { pm: 20, ..DEFAULT_CUR }), ..RunCfgB14::base(113, 12_000, 1000) };
    let o = run_b14(&c);
    assert!(o.cur_energy > 0);
    assert!(o.cur_energy * 1000 <= 20 * o.total_cost, "{} of {}", o.cur_energy, o.total_cost);
    // the budget never runs out (what is left grows with the life), and curiosity still fires after the start
    assert!(20 * o.total_cost - o.cur_energy * 1000 > 20 * 3000);
    assert!(o.wins.iter().filter(|w| w.from >= 4000).map(|w| w.cur_steps).sum::<u64>() > 0, "{:?}", o.wins.iter().map(|w| w.cur_steps).collect::<Vec<_>>());
    // expected improvement: positive, monotone in the mean, zero at no spread below 0
    assert!(expected_improvement(-1.0, 0.5) > 0.0 && expected_improvement(-1.0, 0.5) < expected_improvement(0.0, 0.5));
    assert_eq!(expected_improvement(-1.0, 0.0), 0.0);
    assert!((expected_improvement(0.0, 1.0) - 0.398_942).abs() < 1e-3);
}

#[test]
fn library_candidates_need_own_examples_and_are_never_wrong() {
    let f = Form::Lin { mask: 1, constant: true };
    // a_{n+1} = 2 a_n + 1 on five prefixes
    let ex: Vec<Example> = (0..5)
        .map(|i| {
            let mut t = vec![i as i64 + 1];
            for _ in 0..6 {
                t.push(2 * t[t.len() - 1] + 1);
            }
            let next = 2 * t[t.len() - 1] + 1;
            Example { event_id: format!("e{i}"), sig: 2, terms: t, next }
        })
        .collect();
    // index 2 is held out: three examples give 2 training + 1 held-out
    assert!(check_candidate(&ex[..1], 2, Xf::ID, &f, 1, 1).is_none());
    let ok = check_candidate(&ex[..3], 2, Xf::ID, &f, 1, 1).expect("passes");
    assert_eq!((ok.train_ok, ok.val_ok), (2, 1));
    // a single applicable-and-wrong example blocks it
    let mut bad = ex.clone();
    bad[1].next += 1;
    assert!(check_candidate(&bad, 2, Xf::ID, &f, 1, 1).is_none());
    // another signature's examples do not count
    assert!(check_candidate(&ex, 3, Xf::ID, &f, 1, 1).is_none());
    assert!(wilson_ub(0, 6) < 0.5 && wilson_ub(3, 6) > 0.5 && wilson_ub(0, 0) == 1.0);
}

#[test]
fn library_imports_are_logged_checked_and_reused_with_citations() {
    // life 1 learns rules; life 2 (another world) imports them as candidates
    let one = run_b14(&RunCfgB14 { library: Some(LibCfg::default()), curiosity: Some(DEFAULT_CUR), ..RunCfgB14::base(113, 6000, 1000) });
    assert!(!one.library.is_empty());
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut c = cfg(116, 1500, both(), Some(CTL_ON));
    c.b14_import = one.library.clone();
    let s = run(&mut log, &c, &ScriptedThinker, &pol(1500)).unwrap();
    assert_eq!(s.steps_run, 1500);
    log.verify().unwrap();
    let ev = events(&log);
    let imports: Vec<&EventRow> = ev.iter().filter(|e| e.tags.split(',').any(|t| t == IMPORT_TAG)).collect();
    assert_eq!(imports.len(), one.library.len());
    let reused: Vec<&EventRow> = ev.iter().filter(|e| e.tags.contains("via_library:import")).collect();
    assert!(!reused.is_empty(), "no import was reused");
    let revs: Vec<RevisionRow> = log.open_latest().rows();
    let causes: Vec<CauseRow> = log.open_latest().rows();
    for r in &reused {
        let id = super::super::state::tag(&r.tags, "rule_id").unwrap();
        let rev = revs.iter().find(|x| x.key == format!("being/rule/{id}") && x.author == "rule:being-library-reuse").expect("revision");
        let c = cited(&causes, &rev.rev_id);
        assert!(imports.iter().any(|i| c.contains(&i.event_id)), "reuse does not cite its import note");
        assert!(c.len() >= 3, "reuse cites the examples it reproduced");
    }
    // the state folds back with the imports; a second run writes no import twice
    let st = BeingState::from_events(&ev);
    assert_eq!(st.b14_imports.len(), one.library.len());
    assert_eq!(st.b14, both());
    let mut c2 = c.clone();
    c2.steps = 10;
    run(&mut log, &c2, &ScriptedThinker, &pol(10)).unwrap();
    assert_eq!(events(&log).iter().filter(|e| e.tags.split(',').any(|t| t == IMPORT_TAG)).count(), one.library.len());
}

#[test]
fn library_uses_cite_the_rule_and_retirement_cites_the_examples() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run(&mut log, &cfg(113, 2500, both(), Some(CTL_ON)), &ScriptedThinker, &pol(2500)).unwrap();
    assert_eq!(s.steps_run, 2500);
    let ev = events(&log);
    let st = BeingState::from_events(&ev);
    assert!(st.rules.iter().any(|r| !r.retired));
    let revs: Vec<RevisionRow> = log.open_latest().rows();
    let causes: Vec<CauseRow> = log.open_latest().rows();
    // a decision that used a library estimate for the discovered strategy cites the rule's note
    let used: Vec<&RevisionRow> = revs.iter().filter(|r| r.entity_type == "decision" && r.rationale.contains("rule_library_used")).collect();
    assert!(!used.is_empty());
    let rule_events: Vec<String> = st.rules.iter().map(|r| r.event_id.clone()).collect();
    for r in used.iter().take(20) {
        let c = cited(&causes, &r.rev_id);
        assert!(c.iter().any(|x| rule_events.contains(x)), "a library use must cite the rule note");
    }
    // curiosity energy folds back from the log and stays in the budget
    assert!(revs.iter().any(|r| r.author == format!("rule:being-{CUR_POLICY}")));
    assert!(st.b14_cur_energy > 0 && st.b14_cur_energy * 1000 <= 20 * st.total_cost, "{} {}", st.b14_cur_energy, st.total_cost);
    assert_eq!(st.b14_cur_energy, s.state.b14_cur_energy);
    // retirement on evidence: a rule whose later revealed examples contradict it
    let g = genesis(5, 3000);
    let mut st = BeingState::default();
    st.start(g.clone(), String::new());
    let f = Form::Lin { mask: 1, constant: false };
    let mut r = Rule::new(4, f, 3, 1, 0);
    r.event_id = "rule-note".into();
    st.apply_rule(r.clone());
    for i in 0..6 {
        st.examples.push(Example { event_id: format!("bad{i}"), sig: 4, terms: vec![1, 2, 4, 8, 16, 32, 64], next: 999 });
    }
    let th = ScriptedThinker;
    let mut e = Engine::new(world_for(&g), st, &th).with_limits(Some(limits()));
    e.enable_library(LibCfg::default(), &[]);
    let out = e.library_retire();
    assert_eq!(out.len(), 1);
    let (ev, rev) = &out[0];
    assert!(ev.tags.iter().any(|t| t == "rule_retired") && ev.tags.iter().any(|t| t == "by:library"));
    assert_eq!(rev.author, "rule:being-library-retire");
    assert!((0..6).all(|i| rev.caused_by.contains(&format!("bad{i}"))));
    assert!(e.state.rules[0].retired);
    // folding the note retires it too
    let mut st2 = BeingState::default();
    st2.start(g, String::new());
    st2.apply_rule(r);
    st2.fold_event(&row("x", "note", &ev.text, ev.tags.join(",")));
    assert!(st2.rules[0].retired);
}

#[test]
fn logged_run_is_deterministic_and_restarts_equal_one_run() {
    let texts = |log: &VersionedLog| events(log).into_iter().map(|e| e.text).collect::<Vec<_>>();
    let d1 = tempfile::tempdir().unwrap();
    let mut l1 = open(d1.path());
    run(&mut l1, &cfg(116, 900, both(), Some(CTL_ON)), &ScriptedThinker, &pol(900)).unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let mut l2 = open(d2.path());
    run(&mut l2, &cfg(116, 400, both(), Some(CTL_ON)), &ScriptedThinker, &pol(400)).unwrap();
    drop(l2);
    let mut l2 = VersionedLog::open(d2.path(), "being").unwrap();
    run(&mut l2, &cfg(116, 500, both(), Some(CTL_ON)), &ScriptedThinker, &pol(500)).unwrap();
    assert_eq!(texts(&l1), texts(&l2));
    let st = BeingState::from_events(&events(&l1));
    assert_eq!(st.step, 900);
    assert!(events(&l1).iter().any(|e| e.tags.contains("b14_setup")));
    // the switches are fixed at creation
    assert!(run(&mut l1, &cfg(116, 5, Some(B14Setup { library: false, curiosity_pm: 5 }), Some(CTL_ON)), &ScriptedThinker, &pol(5)).is_err());
}

#[test]
fn not_allowed_is_refused_logged_and_off() {
    // the controller does not allow B14: the being is created without it and the refusals are in its log
    for ctl in [Some("max_steps_per_run = 50\n"), Some("rule_library = maybe\n"), None] {
        let dir = tempfile::tempdir().unwrap();
        let mut log = open(dir.path());
        let s = run(&mut log, &cfg(116, 50, both(), ctl), &ScriptedThinker, &pol(50)).unwrap();
        assert_eq!(s.steps_run, 50);
        let ev = events(&log);
        assert!(ev.iter().any(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:rule_library")), "{ctl:?}");
        assert!(ev.iter().any(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:curiosity")), "{ctl:?}");
        assert!(!ev.iter().any(|e| e.tags.contains("b14_setup")));
        let st = BeingState::from_events(&ev);
        assert_eq!((st.b14, st.b14_cur_energy), (None, 0));
    }
    // allowed at creation, switched off later: the next run refuses and runs without it
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    run(&mut log, &cfg(116, 300, both(), Some(CTL_ON)), &ScriptedThinker, &pol(300)).unwrap();
    let before = BeingState::from_events(&events(&log)).b14_cur_energy;
    run(&mut log, &cfg(116, 300, both(), Some("max_steps_per_run = 500\n")), &ScriptedThinker, &pol(300)).unwrap();
    let ev = events(&log);
    assert_eq!(BeingState::from_events(&ev).b14_cur_energy, before);
    assert!(ev.iter().any(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:rule_library")));
    // B14 needs the extended pattern world
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut c = cfg(1, 10, both(), Some(CTL_ON));
    c.ext2 = None;
    c.discover_every = None;
    assert!(run(&mut log, &c, &ScriptedThinker, &pol(10)).is_err());
}

#[test]
fn kill_switch_stops_a_b14_run() {
    use super::super::policy::KillCheck;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let n = Arc::new(AtomicU64::new(0));
    let n2 = n.clone();
    let mut p = pol(200);
    p.kill = Some(KillCheck(Arc::new(move || n2.fetch_add(1, Ordering::SeqCst) + 1 >= 41)));
    let s = run(&mut log, &cfg(116, 200, both(), Some(CTL_ON)), &ScriptedThinker, &p).unwrap();
    assert_eq!((s.steps_run, s.stopped), (40, StopReason::Killed));
    assert!(events(&log).iter().any(|e| e.tags.contains("stop:kill_switch")));
}

#[test]
fn old_logs_fold_without_b14_and_import_tags_round_trip() {
    // a being created without B14 has no B14 state and no B14 record
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    run(&mut log, &cfg(116, 200, None, None), &ScriptedThinker, &pol(200)).unwrap();
    let ev = events(&log);
    let st = BeingState::from_events(&ev);
    assert_eq!((st.b14, st.b14_imports.len(), st.b14_cur_energy), (None, 0, 0));
    assert!(!ev.iter().any(|e| e.tags.contains("b14") || e.tags.contains("rule_lib") || e.text.contains("B14")));
    // import note round trip
    let x = LibEntry { sig: 4, xf: Xf::ID, form: Form::Lin { mask: 3, constant: false }, origin: "life:7:rule:abc".into(), note_id: String::new() };
    let ne = x.event(0, 0);
    let r = row("n1", "note", &ne.text, ne.tags.join(","));
    let y = LibEntry::from_event(&r).unwrap();
    assert_eq!((y.sig, y.form, y.id()), (4, x.form, rule_id_x(4, Xf::ID, &x.form)));
    // a tampered id is refused
    let r2 = row("n2", "note", &ne.text, r.tags.replace(&format!("rule_id:{}", x.id()), "rule_id:000000000000"));
    assert!(LibEntry::from_event(&r2).is_none());
}

#[test]
fn b14_sources_read_no_hidden_world_parameter_and_no_io() {
    for (name, src) in [("library.rs", include_str!("library.rs")), ("curiosity.rs", include_str!("curiosity.rs")), ("ctl.rs", include_str!("ctl.rs"))] {
        for bad in ["expected()", "profile", "task_of_kind", "Kind::", "view.kind", "std::fs", "std::net", "std::process", "File::", "truth"] {
            assert!(!src.contains(bad), "{name} contains {bad}");
        }
    }
}

#[test]
fn first_decision_without_rules_or_spending_is_the_planners() {
    let g = genesis(2, 3000);
    let w = world_for(&g);
    let mut st = BeingState::default();
    st.start(g.clone(), String::new());
    let offers = offers_of(&w.offers_at(0, 0));
    let mut cu = Curious::new(ModelChooser::for_genesis(&g), &w, Some(LibCfg::default()), Some(DEFAULT_CUR));
    let a = cu.choose(&st, &offers).unwrap();
    let b = ModelChooser::for_genesis(&g).choose(&st, &offers).unwrap();
    assert_eq!(a.action, b.action);
}
