//! Tests of Track L2 (the meta-prior across families): the maths, the controller key (fail closed, coexisting
//! with the other keys), off by default, the form posterior pass, logging with citations, fold / replay from the
//! log, and the kill switch and caps with L2 on.

use super::ctl;
use super::meta::*;
use super::wo::*;
use crate::being::controller::{Controller, Limits};
use crate::being::discover::{Example, Form};
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::world2::solve::sig2;
use crate::being::world2::world::FType;
use crate::being::world_o::engine::*;
use crate::being::world_o::world::{Open, WOCfg};
use crate::versioned::{EventRow, VersionedLog};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn small(seed: u64) -> WOCfg {
    WOCfg { intro_every: 400, life_intros: 6, tier_every: 2, drift_pm: 500, ..WOCfg::new(seed, 40_000) }
}

fn lim() -> Limits {
    Limits { max_live_rules: 200, max_candidates: 100_000, ..Limits::default() }
}

fn l2_on() -> LearnerOCfg {
    LearnerOCfg { l2: Some(L2Cfg::on()), ..LearnerOCfg::default() }
}

const ON: &str = "l2_meta_prior = on\n";

fn prior(forms: &[(&str, u64)], none: u64, app: u64, agr: u64) -> FormPrior {
    FormPrior { forms: forms.iter().map(|(c, n)| (c.to_string(), *n)).collect(), none, applicable: app, agreed: agr }
}

#[test]
fn the_form_posterior_needs_fewer_answers_as_families_accumulate() {
    // early: two earlier families with the form, one without a rule, chance agreement not yet measured (1/2)
    let early = prior(&[("lin.12", 2)], 1, 0, 0);
    // later: many families, a measured chance agreement of 1 in 100
    let late = prior(&[("lin.12", 20), ("poly.3", 12), ("lin.123", 8)], 4, 2000, 20);
    let (ne, nl) = (early.needed("lin.12", 0.02, 1).unwrap(), late.needed("lin.12", 0.02, 1).unwrap());
    assert!(ne > nl, "early {ne} answers, late {nl}");
    assert_eq!(nl, 1);
    assert!(late.posterior("lin.12", 2, &[]) > late.posterior("lin.12", 1, &[]));
    assert!(late.posterior("lin.12", 2, &["poly.3".into()]) < late.posterior("lin.12", 2, &[]), "a consistent rival lowers it");
    assert_eq!(late.pi("lin.3"), 0.0);
    assert_eq!(late.posterior("lin.3", 5, &[]), 0.0, "a form never seen has no prior mass");
    assert!((late.eps() - 21.0 / 2002.0).abs() < 1e-12);
    assert!((early.eps() - 0.5).abs() < 1e-12);
    let s: f64 = late.forms.iter().map(|f| late.pi(&f.0)).sum::<f64>() + late.pi_other();
    assert!((s - 1.0).abs() < 1e-12, "the masses sum to one");
}

#[test]
fn the_trust_prior_needs_families_and_follows_them() {
    assert_eq!(trust_prior(&[(9, 10), (9, 10)]), None, "two families: no prior");
    assert_eq!(trust_prior(&[(9, 10), (9, 10), (1, 2), (0, 1)]), None, "records below the minimum do not count");
    let t = trust_prior(&[(19, 20), (18, 20), (19, 20), (20, 20), (18, 20)]).unwrap();
    assert!(t.mean > 0.85 && t.k >= TRUST_K_MIN && t.k <= TRUST_K_MAX && t.fams == 5);
    let spread = trust_prior(&[(20, 20), (2, 20), (19, 20), (3, 20), (18, 20)]).unwrap();
    assert!(spread.k < t.k, "a wide spread gives a weaker prior");
    let (p0, _) = trust_estimate(&t, 0, 0);
    assert!((p0 - t.mean).abs() < 1e-12, "an empty cell's estimate is the prior mean");
    assert!(trust_estimate(&t, 0, 10).0 < p0);
}

#[test]
fn the_l2_tag_round_trips_and_bad_tags_are_refused() {
    let c = L2Cfg::on();
    assert_eq!(L2Cfg::parse(c.tag().trim_start_matches("wo_l2m:")), Some(c));
    for bad in ["1/1/20", "1/1/0/1", "1/1/20/0", "2/1/20/1", "0/0/20/1", "1/1/501/1", "a/1/20/1"] {
        assert_eq!(L2Cfg::parse(bad), None, "{bad}");
    }
    let (g, _) = genesis_event_o(&small(1), &l2_on());
    assert!(g.tags.iter().any(|t| t == &c.tag()));
    assert!(g.text.contains("Track L2"));
    let (g0, _) = genesis_event_o(&small(1), &LearnerOCfg::default());
    assert!(!g0.tags.iter().any(|t| t.starts_with("wo_l2m")) && !g0.text.contains("Track L2"));
}

#[test]
fn the_controller_key_fails_closed_and_coexists_with_context_chooser_and_the_world_t_keys() {
    let all = "max_steps_per_run = 50\ncontext_chooser = on\nworld_t_run = on\nworld_t_sql_timeout_ms = 2000\nworld_t_sql_max_rows = 1000\nl2_meta_prior = on\n";
    assert!(Controller::parse(all).is_ok());
    assert!(ctl::allowed(Some(all)));
    assert!(crate::being::world_t::ctl::allowed(Some(all)).is_ok());
    assert_eq!(Controller::parse(all).unwrap().max_steps_per_run, 50);
    // a duplicate of any key (the new one or the others) makes the whole file unparseable, so: not allowed
    for dup in [
        "l2_meta_prior = on\nl2_meta_prior = on\n",
        "l2_meta_prior = off\nl2_meta_prior = on\n",
        "l2_meta_prior = on\ncontext_chooser = on\ncontext_chooser = off\n",
        "world_t_run = on\nl2_meta_prior = on\nworld_t_run = on\n",
    ] {
        assert!(Controller::parse(dup).is_err(), "{dup}");
        assert!(!ctl::allowed(Some(dup)), "{dup}");
    }
    assert!(Controller::parse("l2_meta_prior = maybe\n").is_err());
    assert!(Controller::parse("l2_meta_prior\n").is_err());
    // fail closed: missing file, empty file, missing key, off, another bad line, a bad value
    for t in [None, Some(""), Some("max_steps_per_run = 50\n"), Some("l2_meta_prior = off\n"), Some("l2_meta_prior = on\nbogus = 1\n"), Some("l2_meta_prior = maybe\n")] {
        assert!(!ctl::allowed(t), "{t:?}");
        assert!(ctl::grant(true, t).is_err());
        assert_eq!(ctl::grant(false, t).ok(), Some(false));
    }
    assert_eq!(ctl::grant(true, Some("l2_meta_prior = on # owner\n")).ok(), Some(true));
}

fn run_logged(root: &std::path::Path, c: WOCfg, l: LearnerOCfg, pol: &Policy, limits: Limits, ctl_text: Option<&str>) -> (VersionedLog, RunOSummary) {
    let mut log = open_log(root);
    let s = run_o_ctl(&mut log, &RunOCfg { world: Some(c), learner: Some(l), discover_every: 10, ctl: Some(limits), max_explore_pm: None }, pol, None, ctl_text).unwrap();
    (log, s)
}

#[test]
fn l2_is_off_by_default_and_refused_without_the_key() {
    assert!(LearnerOCfg::default().l2.is_none());
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = run_logged(dir.path(), small(7), LearnerOCfg::default(), &Policy::new(800), lim(), Some(ON));
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(!rows.iter().any(|e| e.tags.contains("wo_l2m") || e.tags.contains("l2_update") || e.tags.contains("l2_form")));
    assert_eq!(s.state.l2, L2State::default());
    // asked for without the key: created without it, the refusal logged before the genesis
    for (i, t) in [None, Some("l2_meta_prior = off\n"), Some("context_chooser = on\n"), Some("l2_meta_prior = on\nl2_meta_prior = on\n")].into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let (log, s) = run_logged(dir.path(), small(7), l2_on(), &Policy::new(300), lim(), t);
        let rows: Vec<EventRow> = log.open_latest().rows();
        let ref_i = rows.iter().position(|e| e.tags.contains("controller_refusal") && e.tags.contains(ctl::KEY)).unwrap_or_else(|| panic!("case {i}: refusal logged"));
        let gen_i = rows.iter().position(|e| e.tags.contains("wo_genesis")).unwrap();
        assert!(ref_i < gen_i, "case {i}: the refusal comes before the genesis");
        assert!(!rows[gen_i].tags.contains("wo_l2m"));
        assert!(s.state.learner.l2.is_none() && !s.state.l2.on());
        assert!(s.refusals.iter().any(|r| r.cap == ctl::KEY));
    }
    // the plain entry never grants it
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let s = run_o(&mut log, &RunOCfg { world: Some(small(7)), learner: Some(l2_on()), discover_every: 10, ctl: Some(lim()), max_explore_pm: None }, &Policy::new(100), None).unwrap();
    assert!(s.state.learner.l2.is_none());
}

fn ex(slot: usize, d: u8, terms: Vec<i64>, next: i64, i: usize) -> Example {
    Example { event_id: format!("ev{slot}-{d}-{i}"), sig: sig2(slot, d), terms, next }
}

fn fib(a: i64, b: i64, len: usize) -> (Vec<i64>, i64) {
    let mut v = vec![a, b];
    while v.len() < len + 1 {
        let n = v[v.len() - 1] + v[v.len() - 2];
        v.push(n);
    }
    let next = v.pop().unwrap();
    (v, next)
}

/// An engine with one open sequence family (fid 30, slot 3), the given examples and `earlier` families with `form`.
fn engine_with(examples: Vec<Example>, earlier: u64, form: &str, app: u64, agr: u64) -> EngineO {
    let mut st = StateO::default();
    st.start(small(1), l2_on(), "genesis".into());
    st.open_family(&Open { fid: 30, slot: 3, ftype: FType::Seq, base_slot: None });
    for j in 0..earlier {
        st.l2.fam_form.insert(j, (form.to_string(), format!("rule-event-{j}")));
    }
    st.l2.applicable = app;
    st.l2.agreed = agr;
    st.s2.examples = examples;
    EngineO::new(st, Some(lim()))
}

#[test]
fn the_form_posterior_accepts_a_family_rule_from_one_answer_once_earlier_families_taught_the_form() {
    let code = Form::Lin { mask: 0b11, constant: false }.code();
    let (t, n) = fib(2, 5, 8);
    let one = vec![ex(3, 2, t, n, 0)];
    let mut e0 = engine_with(one.clone(), 0, &code, 0, 0);
    assert!(e0.discover_pass().is_empty(), "no earlier families: nothing");
    let mut e1 = engine_with(one.clone(), 2, &code, 0, 0);
    assert!(e1.discover_pass().is_empty(), "two earlier families and no measured chance agreement: one answer is not enough");
    let mut e2 = engine_with(one.clone(), 12, &code, 2000, 10);
    let new = e2.discover_pass();
    assert_eq!(new.len(), 5, "one rule per difficulty");
    for r in &new {
        assert_eq!(r.rule.form.code(), code);
        assert!(r.event.tags.iter().any(|t| t == "l2_form"));
        assert!(r.event.text.contains("Track L2 form posterior") && r.event.text.contains("o0011"), "cites the earlier families: {}", r.event.text);
        assert!(r.revision.caused_by.iter().any(|c| c == "rule-event-11"), "the revision cites the earlier families' rule events");
        assert!(r.revision.caused_by.iter().any(|c| c == "ev3-2-0"), "and the family's own example");
    }
    assert_eq!(e2.state.l2.accepted, 5);
    assert!(e2.discover_pass().is_empty(), "a second pass adds nothing");
    let mut bad = one.clone();
    let (t2, n2) = fib(1, 3, 8);
    bad.push(ex(3, 4, t2, n2 + 1, 1));
    let mut e3 = engine_with(bad, 12, &code, 2000, 10);
    assert!(!e3.discover_pass().iter().any(|r| r.rule.form.code() == code), "an answer the form contradicts blocks it");
    let mut e4 = engine_with(one, 12, &code, 2000, 10);
    e4.state.l2.cfg = Some(L2Cfg { forms: false, ..L2Cfg::on() });
    assert!(e4.discover_pass().is_empty(), "form switch off: no L2 pass");
}

#[test]
fn closing_a_family_updates_the_meta_prior_and_logs_it() {
    let code = Form::Lin { mask: 0b11, constant: false }.code();
    let (t, n) = fib(2, 5, 8);
    let mut e = engine_with(vec![ex(3, 2, t, n, 0)], 12, &code, 2000, 10);
    e.state.l2.fam_form.insert(5, ("poly.2".into(), "rule-event-x".into()));
    e.discover_pass();
    let v0 = e.state.l2.version;
    e.state.close_slot(3);
    let up = e.state.l2.last_close.clone().unwrap();
    assert_eq!(up.fid, 30);
    assert_eq!(up.seq, Some(Some(code)));
    assert_eq!(e.state.l2.applicable, 2000 + up.applicable, "the chance-agreement counts grow by what the other seen forms did on its examples");
    assert!(up.agreed <= up.applicable);
    assert!(e.state.l2.version > v0 && e.state.l2.seq_slots.is_empty());
    let ev = update_event(&e.state, &up, 100, 0);
    assert!(ev.tags.contains(&"l2_update".to_string()) && ev.text.contains("o0030"));
    let mut st = StateO::default();
    st.start(small(1), l2_on(), "g".into());
    st.open_family(&Open { fid: 40, slot: 4, ftype: FType::Seq, base_slot: None });
    st.close_slot(4);
    assert!(st.l2.none.contains(&40), "a sequence family closed without a rule counts for other");
}

#[test]
fn a_logged_run_with_l2_folds_and_replays_exactly_and_logs_every_update() {
    let dir = tempfile::tempdir().unwrap();
    let c = WOCfg { budget: 30_000, ..small(3) };
    let (log, s) = run_logged(dir.path(), c.clone(), l2_on(), &Policy::new(6000), lim(), Some(ON));
    assert!(s.state.l2.on());
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().any(|e| e.tags.contains("wo_l2m")));
    let closes = rows.iter().filter(|e| e.tags.contains("wo_family") && e.tags.split(',').any(|t| t == "close")).count();
    let updates = rows.iter().filter(|e| e.tags.contains("l2_update")).count();
    assert!(closes > 0 && closes == updates, "one logged update per closed family ({closes} / {updates})");
    assert!(!s.state.l2.trust_hist.is_empty() || !s.state.l2.fam_form.is_empty(), "the meta-prior learned something");
    let folded = StateO::from_events(&rows);
    assert_eq!(folded.l2, s.state.l2, "every prior re-derived from the log");
    assert_eq!(folded.s2.rules, s.state.s2.rules);
    let (n, bad) = replay_o(&rows);
    assert!(n > 1000);
    assert_eq!(bad, 0, "every decision re-derived from the log alone");
    if s.state.l2.trust().0.is_some() {
        assert!(rows.iter().any(|e| e.kind == "decision" && e.text.contains("rule-trust prior")), "decisions say what the trust estimate rests on");
    }
    // a continued run from the log, without asking (and without the key): the being keeps what it was created with
    let mut log2 = open_log(dir.path());
    let s2 = run_o_ctl(&mut log2, &RunOCfg { world: Some(c), learner: None, discover_every: 10, ctl: Some(lim()), max_explore_pm: None }, &Policy::new(500), None, None).unwrap();
    assert!(s2.state.l2.on());
    assert_eq!(replay_o(&log2.open_latest().rows()).1, 0);
}

#[test]
fn the_kill_switch_and_caps_hold_with_l2_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut pol = Policy::new(5000);
    let c = Arc::new(AtomicU64::new(0));
    let c2 = c.clone();
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 40)));
    let (_, s) = run_logged(dir.path(), small(4), l2_on(), &pol, Limits { max_live_rules: 2, ..lim() }, Some(ON));
    assert_eq!(s.stopped, StopReason::Killed);
    assert_eq!(s.steps_run, 39);
    assert!(s.state.s2.live_rules() <= 2);
}
