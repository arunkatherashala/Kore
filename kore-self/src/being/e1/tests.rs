//! Tests of Track E1 (evolutionary search over the rule DSL, its controller keys). Temp dirs only.

use super::super::b14::ctl::{B14Ctl, B14Setup};
use super::super::b14::measure::{run_b14, RunCfgB14};
use super::super::controller::{Controller, Limits};
use super::super::discover::{self, forms, predict, predict_x, rule_from_event, Example, Form, Rule, Xf};
use super::super::measure_b11::{genesis, LONGRUN_CONTROLLER};
use super::super::run_loop::{run, world_for, Engine, RunCfg};
use super::super::solver::ScriptedThinker;
use super::super::state::{tag, BeingState, Chooser, SOURCE};
use super::super::world::Kind;
use super::super::{Policy, StopReason};
use super::ctl::{E1Ctl, E1Setup, HARD_BUDGET_PM};
use super::evolve::{available, replay_check, run_pass, verify, EvoCfg, DEFAULT_EVO, EVALS_PER_ENERGY, PASS_TAG, VIA_TAG};
use super::genome::{self, Code, ADD, MUL, N, P};
use super::measure::{run_e1, RunCfgE1};
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

const CTL_ON: &str = "max_live_rules = 16\nmax_building_blocks = 8\nmax_search_candidates = 2000\nmax_steps_per_run = 5000\ncomposition = on\npruning = on\nrule_library = on\ncuriosity = on\nmax_curiosity_energy_pm = 20\nevolution = on\nevolution_budget_pm = 20\n";

fn limits() -> Limits {
    Controller::parse(LONGRUN_CONTROLLER).unwrap().limits(true, true).0
}

fn cfg(world: u64, steps: u64, e1: Option<E1Setup>, ctl: Option<&str>) -> RunCfg {
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
        b14: Some(B14Setup { library: true, curiosity_pm: 20 }),
        b14_ctl: ctl.map(|t| B14Ctl::from_text(Some(t))),
        e1,
        e1_ctl: ctl.map(|t| E1Ctl::from_text(Some(t))),
        ..Default::default()
    }
}

fn pol(n: u64) -> Policy {
    let mut p = Policy::new(n);
    p.refill_when_exhausted = true;
    p
}

fn on() -> Option<E1Setup> {
    Some(E1Setup { pm: 20 })
}

fn g(m: u8, ops: &[u8]) -> Code {
    Code::new(m, ops).unwrap()
}

#[test]
fn controller_keys_validate_coexist_and_fail_closed() {
    // every opt-in key of the earlier tracks, the B14 keys and the E1 keys in one file
    let all = "experiments = on\nmax_experiment_energy_pm = 50\nmax_experiments_per_run = 10\ncontext_chooser = on\nworld_t_run = on\nrules_use = on\nrule_retirement = on\nrule_library = on\ncuriosity = on\nmax_curiosity_energy_pm = 20\nevolution = on\nevolution_budget_pm = 30\n";
    assert!(Controller::parse(all).is_ok(), "{:?}", Controller::parse(all));
    let c = E1Ctl::from_text(Some(all));
    assert_eq!((c.evolution, c.budget_pm, c.closed.is_none()), (true, 30, true));
    let b = B14Ctl::from_text(Some(all));
    assert_eq!((b.library, b.curiosity, b.curiosity_pm), (true, true, 20));
    for bad in [
        "evolution = on\nevolution = on",
        "evolution_budget_pm = 5\nevolution_budget_pm = 5",
        "evolution = sometimes",
        "evolution_budget_pm = -1",
        "evolution_budget_pm = 101",
        "rule_library = on\nevolution = on\nrule_library = on",
        "evolution = on\ncuriosity = on\ncuriosity = off",
    ] {
        assert!(Controller::parse(bad).is_err(), "{bad}");
        let c = E1Ctl::from_text(Some(bad));
        assert!(c.closed.is_some() && !c.evolution && c.budget_pm == 0, "{bad}");
        assert!(c.grant(&E1Setup { pm: 10 }).0.is_off());
    }
    assert_eq!(HARD_BUDGET_PM, 100);
    // default off: a valid file without the keys, a missing file; the switch needs both keys
    for t in [Some("max_steps_per_run = 50\n"), None, Some("evolution = on\n"), Some("evolution_budget_pm = 20\n"), Some("evolution = off\nevolution_budget_pm = 20\n")] {
        let (gr, r) = E1Ctl::from_text(t).grant(&E1Setup { pm: 10 });
        assert!(gr.is_off() && r.len() == 1 && r[0].cap == "evolution", "{t:?}");
    }
    // the budget is capped by the controller, with a refusal
    let (gr, r) = E1Ctl::from_text(Some("evolution = on\nevolution_budget_pm = 5\n")).grant(&E1Setup { pm: 10 });
    assert_eq!((gr.pm, r.len()), (5, 1));
    // asking for nothing is not a refusal
    assert_eq!(E1Ctl::from_text(None).grant(&E1Setup { pm: 0 }).1.len(), 0);
    // setup tags round trip; bad tags give nothing
    let s = E1Setup { pm: 7 };
    assert_eq!(E1Setup::from_tags(&s.tags().join(",")), Some(s));
    assert_eq!(E1Setup::from_tags("e1_setup,evolution_pm:500"), None);
    assert_eq!(E1Setup::from_tags("e1_setup,evolution_pm:x"), None);
}

#[test]
fn genome_codes_round_trip_and_grammar_forms_predict_like_b2() {
    // every grammar form is a genome that predicts exactly what the B2 form predicts
    let w = world_for(&genesis(7, 3000));
    let mut seqs: Vec<Vec<i64>> = vec![];
    for k in [Kind::PatArith, Kind::PatGeom, Kind::ExtAffGeom, Kind::PatQuad, Kind::ExtPoly3, Kind::PatFib, Kind::ExtRec2, Kind::ExtTribo, Kind::ExtAlt, Kind::ExtPeriodic, Kind::NegProd, Kind::NegPrimes, Kind::NegLcg, Kind::NegMod7] {
        for i in 0..15 {
            seqs.push(w.task_of_kind(i, k).view.params.clone());
        }
    }
    seqs.push(vec![1, 1, 1, 1, 1, 1]);
    seqs.push(vec![0, 0, 0, 0]);
    seqs.push(vec![5]);
    let mut applicable = 0;
    for f in forms() {
        let gn = genome::of_form(&f).expect("every grammar form is a genome");
        assert_eq!(gn.nparams(), f.size(), "{}", f.code());
        for s in &seqs {
            let a = predict(&f, s);
            assert_eq!(a, genome::predict(&gn, s), "{} on {s:?}", f.code());
            applicable += a.is_some() as usize;
            for x in 1..7u8 {
                assert_eq!(predict_x(Xf(x), &f, s), predict_x(Xf(x), &Form::Gp(gn), s));
            }
        }
        // the code round trips through the rule tags
        let fg = Form::Gp(gn);
        assert_eq!(Form::parse(&fg.code()), Some(fg));
    }
    assert!(applicable > 100);
    // non-canonical and malformed codes are refused; products of two fitted terms are not genomes
    for bad in ["gp.", "gp.add_t1", "gp.t4", "gp.m1.t1", "gp.m0.t1", "gp.m252.t1", "gp.c17", "gp.cm0", "gp.mul_p_p", "gp.mul_p_add_t1_p", "gp.t1_t2", "gp.x"] {
        assert_eq!(Code::parse(bad), None, "{bad}");
    }
    assert_eq!(Code::new(0, &[MUL, P, P]), None);
    assert_eq!(Code::new(0, &[P, P, P, P, P]), None);
    assert!(Code::new(0, &[ADD; 24]).is_none());
    let x = g(13, &[ADD, MUL, P, N, genome::c(-3)]);
    assert_eq!(Code::parse(&x.code_str()), Some(x));
    assert_eq!(x.code_str(), "gp.m13.add_mul_p_n_cm3");
    assert!(x.dsl().contains("modulo 13") && x.dsl().contains("a1"));
}

#[test]
fn genome_predicts_outside_the_grammar_and_keeps_the_b2_self_check() {
    // product recurrence: no coefficient, four equations of the prefix
    let prod = g(0, &[MUL, genome::t(1), genome::t(2)]);
    assert_eq!(genome::predict(&prod, &[2, 3, 6, 18, 108, 1944]), Some(209952));
    assert_eq!(genome::predict(&prod, &[2, 3, 6, 18, 108, 1945]), None);
    assert_eq!(genome::predict(&prod, &[2, 3, 6, 18, 108]), Some(1944));
    assert_eq!(genome::predict(&prod, &[2, 3, 6, 18]), None, "too few spare equations");
    // a linear congruential sequence: coefficients fitted modulo 101
    let lcg = g(101, &[ADD, MUL, P, genome::t(1), P]);
    let mut s = vec![17i64];
    while s.len() < 14 {
        let n = (3 * s[s.len() - 1] + 5) % 101;
        s.push(n);
    }
    assert_eq!(genome::predict(&lcg, &s[..13]), Some(s[13]));
    for f in discover::forms() {
        assert_eq!(predict(&f, &s[..13]), None, "{} explains an LCG", f.code());
    }
    // where the plain affine rule already describes the prefix, the modulus is not needed: not applied
    assert_eq!(genome::predict(&lcg, &[1, 3, 7, 15, 31, 63]), None);
    assert_eq!(genome::predict(&g(0, lcg.ops()), &[1, 3, 7, 15, 31, 63]), Some(127));
    // terms outside 0..m: not applicable
    assert_eq!(genome::predict(&g(7, &[genome::t(1)]), &[9, 9, 9, 9, 9]), None);
    // the partial credit is a share in 0..1 and 1 on an exact fit
    assert_eq!(genome::partial(&prod, &[2, 3, 6, 18, 108, 1944]), 1.0);
    let p = genome::partial(&prod, &[2, 3, 6, 18, 100, 7]);
    assert!(p > 0.0 && p < 1.0, "{p}");
}

/// A being with examples of the product recurrence (outside the grammar) at difficulty 5, plus the consecutive primes.
fn prod_state(n: usize) -> BeingState {
    let gn = genesis(5, 3000);
    let w = world_for(&gn);
    let mut st = BeingState::default();
    st.start(gn, String::new());
    for i in 0..n {
        // two families interleaved, so both fall on training and held-out positions
        for kind in [Kind::NegProd, Kind::NegPrimes] {
            let t = w.task_of_kind(1000 + i as u64, kind);
            st.examples.push(Example { event_id: format!("ex-{i}-{}", kind as u64), sig: 5, terms: t.view.params.clone(), next: t.expected() });
        }
    }
    st.total_cost = 100_000;
    st.step = 250;
    st
}

#[test]
fn evolution_finds_a_rule_outside_the_grammar_through_the_b2_check() {
    let st = prod_state(12);
    // discovery (grammar and composition over every form) explains none of the product examples
    let all_forms: Vec<Form> = forms();
    let found = discover::search_composed(&st.examples, &all_forms, usize::MAX).found;
    let prod_ex: Vec<Example> = st.examples.iter().filter(|e| e.event_id.ends_with(&format!("-{}", Kind::NegProd as u64))).cloned().collect();
    assert!(prod_ex.iter().all(|e| found.iter().all(|f| predict_x(f.xf, &f.form, &e.terms) != Some(e.next))));
    let th = ScriptedThinker;
    let mut e = Engine::new(world_for(st.genesis.as_ref().unwrap()), st, &th).with_limits(Some(limits()));
    e.enable_evolution(EvoCfg { every: 1, ..DEFAULT_EVO });
    let mut got = None;
    for _ in 0..40 {
        let (notes, rules) = e.evolve_pass();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].tags.iter().any(|t| t == PASS_TAG));
        if let Some(nr) = rules.into_iter().next() {
            got = Some((notes[0].clone(), nr));
            break;
        }
    }
    let (note, nr) = got.expect("evolution found no rule for the product recurrence in 40 passes");
    assert!(nr.rule.form.is_gp());
    assert!(nr.event.tags.iter().any(|t| t == VIA_TAG));
    assert_eq!(nr.revision.author, "rule:being-evolution");
    let note_id = crate::versioned::event_id(&note.source, &note.occurred_at, &note.text);
    assert!(nr.revision.caused_by.contains(&note_id), "the rule cites its pass note");
    assert!(nr.revision.caused_by.iter().filter(|c| c.starts_with("ex-")).count() >= 4, "the rule cites the examples it reproduced");
    assert!(nr.revision.rationale.contains("\"evolution\""));
    // it is never applicable-and-wrong on the being's examples and answers fresh product tasks
    for x in &e.state.examples {
        if let Some(v) = predict_x(nr.rule.xf, &nr.rule.form, &x.terms) {
            assert_eq!(v, x.next);
        }
    }
    let w = world_for(&genesis(5, 3000));
    let right = (0..50)
        .filter(|i| {
            let t = w.task_of_kind(50_000 + i, Kind::NegProd);
            predict_x(nr.rule.xf, &nr.rule.form, &t.view.params) == Some(t.expected())
        })
        .count();
    assert!(right >= 45, "{right}");
    // the rule note folds back as an ordinary rule
    let r = rule_from_event(&row("r1", "note", &nr.event.text, nr.event.tags.join(","))).expect("rule note folds");
    assert_eq!((r.id.clone(), r.form, r.xf), (nr.rule.id.clone(), nr.rule.form, nr.rule.xf));
    let tampered = row("r2", "note", &nr.event.text, nr.event.tags.join(",").replace(&format!("rule_id:{}", r.id), "rule_id:000000000000"));
    assert!(rule_from_event(&tampered).is_none());
    // one applicable-and-wrong example blocks the same candidate in the check
    let code = match nr.rule.form {
        Form::Gp(c) => c,
        _ => unreachable!(),
    };
    let mut ex = e.state.examples.clone();
    let mut bad = prod_ex.iter().find(|x| predict_x(nr.rule.xf, &nr.rule.form, &x.terms).is_some()).unwrap().clone();
    bad.next += 1;
    bad.event_id = "bad".into();
    ex.push(bad);
    let mut n = 0;
    assert_eq!(verify(&ex, &[], 5, nr.rule.xf, code, &mut n).err(), Some(true));
    // too little support: refused (not wrong)
    let few: Vec<Example> = prod_ex.iter().take(2).cloned().collect();
    assert_eq!(verify(&few, &[], 5, nr.rule.xf, code, &mut n).err(), Some(false));
}

#[test]
fn passes_are_deterministic_and_within_budget() {
    let st = prod_state(8);
    let cfg = EvoCfg { every: 1, ..DEFAULT_EVO };
    let a = run_pass(&st, &cfg).unwrap();
    let b = run_pass(&st, &cfg).unwrap();
    assert_eq!(a.event(0, 1, &[]).text, b.event(0, 1, &[]).text);
    let per_gen = (cfg.sample_unexplained + cfg.sample_explained) as u64 * cfg.pop as u64;
    assert!(a.evals <= cfg.max_pass_evals + per_gen + 5 * st.examples.len() as u64 * cfg.max_verify as u64, "{}", a.evals);
    // no budget: no pass
    let mut st0 = st.clone();
    st0.total_cost = 0;
    assert!(run_pass(&st0, &cfg).is_none());
    assert_eq!(available(&st0, &cfg), 0);
    let mut st1 = st.clone();
    st1.e1.evals = available(&st, &cfg);
    assert!(run_pass(&st1, &cfg).is_none());
    // nothing unexplained: no pass
    let mut st2 = st.clone();
    st2.examples.clear();
    assert!(run_pass(&st2, &cfg).is_none());
}

#[test]
fn off_is_run_b14_step_for_step() {
    let c = RunCfgB14 { library: Some(Default::default()), curiosity: Some(super::super::b14::curiosity::DEFAULT_CUR), ..RunCfgB14::base(113, 3000, 500) };
    let a = run_b14(&c);
    let b = run_e1(&RunCfgE1 { b14: c, evo: None });
    assert_eq!(a.wins.len(), b.wins.len());
    for (x, y) in a.wins.iter().zip(&b.wins) {
        assert_eq!((x.reward, x.cost, x.attempts, x.skips, x.accepted, x.retired, x.live_end), (y.reward, y.cost, y.attempts, y.skips, y.accepted, y.retired, y.live_end));
    }
    assert_eq!(a.rules_final, b.rules_final);
    assert_eq!((b.evo_passes, b.evo_evals, b.evolved.len()), (0, 0, 0));
}

#[test]
fn logged_run_is_deterministic_restarts_equal_one_run_and_replays() {
    let texts = |log: &VersionedLog| events(log).into_iter().map(|e| e.text).collect::<Vec<_>>();
    let d1 = tempfile::tempdir().unwrap();
    let mut l1 = open(d1.path());
    let s = run(&mut l1, &cfg(116, 1000, on(), Some(CTL_ON)), &ScriptedThinker, &pol(1000)).unwrap();
    assert_eq!(s.steps_run, 1000);
    l1.verify().unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let mut l2 = open(d2.path());
    run(&mut l2, &cfg(116, 400, on(), Some(CTL_ON)), &ScriptedThinker, &pol(400)).unwrap();
    drop(l2);
    let mut l2 = VersionedLog::open(d2.path(), "being").unwrap();
    run(&mut l2, &cfg(116, 600, on(), Some(CTL_ON)), &ScriptedThinker, &pol(600)).unwrap();
    assert_eq!(texts(&l1), texts(&l2));
    let ev = events(&l1);
    assert!(ev.iter().any(|e| e.tags.contains("e1_setup")));
    let passes: Vec<&EventRow> = ev.iter().filter(|e| e.tags.split(',').any(|t| t == PASS_TAG)).collect();
    assert!(passes.len() >= 2, "{} passes", passes.len());
    // the state folds back: passes, evaluations, within the budget
    let st = BeingState::from_events(&ev);
    assert_eq!(st.e1.setup, on());
    assert_eq!(st.e1.passes, passes.len() as u64);
    let evals: u64 = passes.iter().map(|e| tag(&e.tags, "evals").unwrap().parse::<u64>().unwrap()).sum();
    assert_eq!(st.e1.evals, evals);
    assert!(st.e1.evals <= (st.total_cost as u64) * 20 * EVALS_PER_ENERGY / 1000 + DEFAULT_EVO.max_pass_evals);
    // every pass is recomputed from the events before it, with the same note
    let (n, bad) = replay_check(&ev, &DEFAULT_EVO);
    assert_eq!((n, bad), (passes.len(), 0));
    // evolved rules (if any) cite their pass note
    let revs: Vec<RevisionRow> = l1.open_latest().rows();
    let causes: Vec<CauseRow> = l1.open_latest().rows();
    for r in revs.iter().filter(|r| r.author == "rule:being-evolution") {
        let c: Vec<String> = causes.iter().filter(|c| c.rev_id == r.rev_id).map(|c| c.event_id.clone()).collect();
        assert!(passes.iter().any(|p| c.contains(&p.event_id)));
    }
    // the switch is fixed at creation
    let d3 = tempfile::tempdir().unwrap();
    let mut l3 = open(d3.path());
    run(&mut l3, &cfg(116, 20, None, Some(CTL_ON)), &ScriptedThinker, &pol(20)).unwrap();
    assert!(run(&mut l3, &cfg(116, 5, on(), Some(CTL_ON)), &ScriptedThinker, &pol(5)).is_err());
}

#[test]
fn not_allowed_is_refused_logged_and_off() {
    for ctl in [Some("max_steps_per_run = 600\nrule_library = on\ncuriosity = on\nmax_curiosity_energy_pm = 20\n"), Some("evolution = maybe\n"), None] {
        let dir = tempfile::tempdir().unwrap();
        let mut log = open(dir.path());
        let s = run(&mut log, &cfg(116, 300, on(), ctl), &ScriptedThinker, &pol(300)).unwrap();
        assert_eq!(s.steps_run, 300);
        let ev = events(&log);
        assert!(ev.iter().any(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:evolution")), "{ctl:?}");
        assert!(!ev.iter().any(|e| e.tags.contains("e1_setup") || e.tags.contains(PASS_TAG) || e.tags.contains(VIA_TAG)), "{ctl:?}");
        assert_eq!(BeingState::from_events(&ev).e1, Default::default());
    }
    // allowed at creation, switched off later: the next run refuses and runs without it
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    run(&mut log, &cfg(116, 300, on(), Some(CTL_ON)), &ScriptedThinker, &pol(300)).unwrap();
    let before = events(&log).iter().filter(|e| e.tags.contains(PASS_TAG)).count();
    assert!(before >= 1);
    run(&mut log, &cfg(116, 600, on(), Some("max_steps_per_run = 1000\n")), &ScriptedThinker, &pol(600)).unwrap();
    let ev = events(&log);
    assert_eq!(ev.iter().filter(|e| e.tags.contains(PASS_TAG)).count(), before);
    assert!(ev.iter().any(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:evolution")));
    // E1 needs the extended pattern world
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut c = cfg(1, 10, on(), Some(CTL_ON));
    c.ext2 = None;
    c.discover_every = None;
    c.b14 = None;
    assert!(run(&mut log, &c, &ScriptedThinker, &pol(10)).is_err());
}

#[test]
fn kill_switch_stops_an_e1_run() {
    use super::super::policy::KillCheck;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let n = Arc::new(AtomicU64::new(0));
    let n2 = n.clone();
    let mut p = pol(400);
    p.kill = Some(KillCheck(Arc::new(move || n2.fetch_add(1, Ordering::SeqCst) + 1 >= 41)));
    let s = run(&mut log, &cfg(116, 400, on(), Some(CTL_ON)), &ScriptedThinker, &p).unwrap();
    assert_eq!((s.steps_run, s.stopped), (40, StopReason::Killed));
    assert!(events(&log).iter().any(|e| e.tags.contains("stop:kill_switch")));
}

#[test]
fn old_logs_fold_without_e1() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut c = cfg(116, 300, None, None);
    c.b14 = None;
    run(&mut log, &c, &ScriptedThinker, &pol(300)).unwrap();
    let ev = events(&log);
    let st = BeingState::from_events(&ev);
    assert_eq!(st.e1, Default::default());
    assert!(!ev.iter().any(|e| e.tags.contains("e1_") || e.tags.contains("gp.") || e.tags.contains(VIA_TAG) || e.text.contains("E1")));
    assert!(st.rules.iter().all(|r| !r.form.is_gp()));
    // a B2 rule note still has the same id and folds as before
    let r = Rule::new(3, Form::Poly(2), 3, 1, 9);
    let (ne, _) = r.event("2030-01-01T00:00:00Z".into());
    assert_eq!(rule_from_event(&row("x", "note", &ne.text, ne.tags.join(","))).map(|x| x.id), Some(r.id));
    // an e1_pass note with bad tags changes nothing
    let mut st2 = BeingState::default();
    st2.fold_event(&row("p", "note", "x", format!("{PASS_TAG},sig:x,evals:5")));
    assert_eq!(st2.e1, Default::default());
}

#[test]
fn e1_sources_read_no_hidden_world_parameter_and_no_io() {
    for (name, src) in [("genome.rs", include_str!("genome.rs")), ("evolve.rs", include_str!("evolve.rs")), ("ctl.rs", include_str!("ctl.rs"))] {
        for bad in ["expected()", "profile", "task_of_kind", "Kind::", "view.kind", "std::fs", "std::net", "std::process", "File::", "truth", "thread_rng", "SystemTime"] {
            assert!(!src.contains(bad), "{name} contains {bad}");
        }
    }
}
