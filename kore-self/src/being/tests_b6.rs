//! Tests of Track B6: transfer between domains.

use super::controller::{Controller, Limits};
use super::discover::*;
use super::measure_b5::run_b5;
use super::measure_b6::*;
use super::model::{AdaptMode, Model, Obs, GUARD_NATS};
use super::policy::Policy;
use super::run_loop::{run, RunCfg};
use super::solver::{ScriptedThinker, Strategy};
use super::state::{tag, BeingState, Chooser};
use super::transfer::{XferMode, XferRec};
use super::world::{Domain, World};
use crate::versioned::{CauseRow, EventRow, RevisionRow, VersionedLog};

fn digest(seed: u64, drift: bool, comp: bool) -> (i64, i64, u64, usize, String) {
    let lim = if comp { Some(Limits { compose: true, prune: true, ..Default::default() }) } else { None };
    let o = run_b5(seed, 1200, drift, lim, 10, &Policy::new(100_000));
    let counts: Vec<String> = o.state.model.counts().iter().map(|c| format!("{}{}:{}:{}", c.0.idx(), c.1.idx(), c.2, c.3)).collect();
    (o.reward, o.cost, o.steps, o.state.rules.len(), counts.join(","))
}

/// Recorded with the code of B5 (commit a79f7f31), before any B6 change: the B5 behaviour must stay bit for bit.
#[test]
fn everything_before_b6_is_bit_identical_with_transfer_off() {
    let golden: [(u64, bool, bool, (i64, i64, u64, usize, &str)); 6] = [
        (1, false, false, (4798, 1199, 467, 0, "00:234:212,01:0:0,02:0:0,10:0:0,11:0:0,12:0:0,20:3:0,21:0:0,22:0:0,30:4:0,31:0:0,32:0:0")),
        (1, true, true, (4798, 1199, 467, 0, "00:234:212,01:0:0,02:0:0,10:0:0,11:0:0,12:0:0,20:3:0,21:0:0,22:0:0,30:4:0,31:0:0,32:0:0")),
        (2, false, false, (5122, 1200, 483, 0, "00:286:244,01:0:0,02:0:0,10:40:31,11:0:0,12:0:0,20:1:0,21:0:0,22:0:0,30:9:0,31:1:0,32:1:0,33:1:0")),
        (2, true, true, (5122, 1200, 483, 0, "00:286:244,01:0:0,02:0:0,10:40:31,11:0:0,12:0:0,20:1:0,21:0:0,22:0:0,30:9:0,31:1:0,32:1:0,33:1:0")),
        (3, false, false, (4089, 1199, 418, 5, "00:159:146,01:0:0,02:0:0,10:62:52,11:0:0,12:0:0,20:5:2,21:0:0,22:0:0,30:155:92,31:8:3,32:1:0,33:1:0")),
        (3, true, true, (4089, 1199, 418, 5, "00:159:146,01:0:0,02:0:0,10:62:52,11:0:0,12:0:0,20:5:2,21:0:0,22:0:0,30:155:92,31:8:3,32:1:0,33:1:0")),
    ];
    for (seed, d, c, g) in golden {
        let got = digest(seed, d, c);
        assert_eq!((got.0, got.1, got.2, got.3, got.4.as_str()), g, "seed {seed} drift {d} comp {c}");
    }
    // a world without the option offers exactly what it offered before: none of the two new domains
    let w = World::with_ext2(3, None);
    for step in 0..300 {
        for t in w.offers_at(step, step as i64 * 90) {
            assert!(t.view.domain != Domain::PatternB && t.view.domain != Domain::Control);
        }
    }
}

#[test]
fn transfer_world_has_two_phases_and_a_hidden_value_map() {
    let w = World::with_transfer(4, 3000);
    let at = w.transfer.as_ref().unwrap().at;
    assert!(at > 0 && at < 3000 * 30);
    let (mut b, mut c) = (0, 0);
    for step in 0..400u64 {
        for t in w.offers_at(step, at - 1) {
            assert_eq!(t.view.domain, Domain::Pattern, "phase 1 offers only the first domain");
        }
        for t in w.offers_at(step, at) {
            match t.view.domain {
                Domain::PatternB => {
                    b += 1;
                    if t.view.difficulty == 1 {
                        // an arithmetic family stays arithmetic under an affine map of the values, and the checker agrees
                        let p = &t.view.params;
                        let d = p[1] - p[0];
                        assert!(p.windows(2).all(|x| x[1] - x[0] == d));
                        assert_eq!(t.expected(), p[p.len() - 1] + d);
                    }
                }
                Domain::Control => c += 1,
                _ => {}
            }
        }
    }
    assert!(b > 100 && c > 100, "both new domains appear in phase 2 ({b}, {c})");
    // the hidden schedule is a function of the world seed and the budget only
    assert_eq!(World::with_transfer(4, 3000).transfer, w.transfer);
    assert_ne!(World::with_transfer(5, 3000).transfer.unwrap().b, 0);
}

fn lin(mask: u8, constant: bool) -> Form {
    Form::Lin { mask, constant }
}

#[test]
fn transferred_search_order_keeps_full_verification() {
    let w = World::with_transfer(3, 3000);
    let mut ex: Vec<Example> = vec![];
    for i in 0..60u64 {
        for k in 1..=5u8 {
            let t = w.task_forced(500 + i, Domain::PatternB, k);
            ex.push(Example { event_id: format!("b-{k}-{i}"), sig: sig_of(Domain::PatternB, k), terms: t.view.params.clone(), next: t.expected() });
        }
    }
    let blocks = [Form::Poly(2), Form::Poly(3), lin(3, false), lin(1, true)];
    // no preference: exactly the composed search
    let a = search_composed(&ex, &blocks, usize::MAX);
    let b = search_transfer(&ex, &blocks, usize::MAX, &[]);
    assert_eq!(a.found, b.found);
    // with preferred hypotheses: every rule found is still never applicable-and-wrong on all examples of its signature
    let prefer: Vec<(u8, Xf, Form)> = (11..=15u8).flat_map(|s| [(s, Xf::ID, Form::Poly(2)), (s, Xf::ID, lin(1, true)), (s, Xf::ID, lin(3, false))]).collect();
    let c = search_transfer(&ex, &blocks, usize::MAX, &prefer);
    assert!(!c.found.is_empty());
    for f in &c.found {
        let sig_ex: Vec<Example> = ex.iter().filter(|e| e.sig == f.sig).cloned().collect();
        let (right, wrong, _) = evaluate_x(f.xf, &f.form, &sig_ex);
        assert_eq!(wrong, 0, "{} {}", f.xf.code(), f.form.code());
        assert!(f.new_train >= K_TRAIN && f.new_val >= K_VAL && right >= K_TRAIN + K_VAL);
    }
    // a hypothesis that is not among the candidates is ignored, not invented
    let alien = search_transfer(&ex, &[], usize::MAX, &[(11, Xf::parse("dd").unwrap(), Form::Poly(1))]);
    assert!(alien.found.iter().all(|f| f.xf.is_id()));
    assert_eq!(toggle_constant(&lin(1, false)), Some(lin(1, true)));
    assert_eq!(toggle_constant(&Form::Poly(2)), None);
}

fn obs(d: Domain, k: u8, s: Strategy, ok: bool) -> Obs {
    Obs { domain: d, difficulty: k, strategy: s, correct: ok, listed_cost: 4, spent: 4 }
}

#[test]
fn the_guard_revokes_a_harmful_link_and_keeps_a_good_one() {
    let build = |target_ok: bool| {
        let mut m = Model::new(AdaptMode::Off, 100.0);
        for _ in 0..30 {
            m.observe(&obs(Domain::Pattern, 3, Strategy::Careful, true));
        }
        for _ in 0..30 {
            m.observe(&obs(Domain::Algebra, 3, Strategy::Careful, false));
        }
        let plain = m.estimate(Domain::PatternB, 3, Strategy::Careful).p;
        assert!(m.set_link(Domain::PatternB, 3, Domain::Pattern, 3, true));
        let linked = m.estimate(Domain::PatternB, 3, Strategy::Careful).p;
        assert!(linked > plain + 0.1, "the link lends the source posterior ({plain} -> {linked})");
        for _ in 0..8 {
            m.observe(&obs(Domain::PatternB, 3, Strategy::Careful, target_ok));
        }
        m
    };
    // the target behaves like the source: the link stays
    let mut good = build(true);
    assert!(good.link(Domain::PatternB, 3).unwrap().active);
    assert!(good.take_revoked().is_empty());
    // the target fails where the source succeeds: the guard switches the link off and logs why
    let mut bad = build(false);
    assert!(!bad.link(Domain::PatternB, 3).unwrap().active);
    let rv = bad.take_revoked();
    assert_eq!(rv.len(), 1);
    assert!(rv[0].delta < -GUARD_NATS && rv[0].n >= 4);
    // without the guard the harm continues
    let mut m = Model::new(AdaptMode::Off, 100.0);
    for _ in 0..30 {
        m.observe(&obs(Domain::Pattern, 3, Strategy::Careful, true));
    }
    m.set_link(Domain::PatternB, 3, Domain::Pattern, 3, false);
    for _ in 0..8 {
        m.observe(&obs(Domain::PatternB, 3, Strategy::Careful, false));
    }
    assert!(m.link(Domain::PatternB, 3).unwrap().active && m.take_revoked().is_empty());
    // a source with too little evidence lends nothing
    let mut thin = Model::new(AdaptMode::Off, 100.0);
    thin.observe(&obs(Domain::Pattern, 3, Strategy::Careful, true));
    let before = thin.estimate(Domain::PatternB, 3, Strategy::Careful);
    thin.set_link(Domain::PatternB, 3, Domain::Pattern, 3, true);
    assert_eq!(thin.estimate(Domain::PatternB, 3, Strategy::Careful), before);
}

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn tcfg(world: u64, steps: u64, budget: i64, ctl: Limits) -> RunCfg {
    RunCfg {
        world_seed: Some(world),
        loop_seed: Some(1),
        budget: Some(budget),
        chooser: Some(Chooser::Model),
        steps,
        ext2: Some(true),
        transfer: Some(true),
        learn_transfer: Some(true),
        discover_every: Some(10),
        ctl: Some(ctl),
        ..Default::default()
    }
}

fn tlimits() -> Limits {
    Limits { compose: true, transfer: true, ..Default::default() }
}

/// A world and a number of steps after which the loop has made a transfer link (found with `b6-probe`).
const LINK_WORLD: u64 = 4;
const LINK_STEPS: u64 = 330;

fn find_link_run(dir: &std::path::Path) -> (VersionedLog, super::run_loop::RunSummary) {
    let mut log = open(dir);
    let s = run(&mut log, &tcfg(LINK_WORLD, LINK_STEPS, 3000, tlimits()), &ScriptedThinker, &Policy::new(LINK_STEPS)).unwrap();
    (log, s)
}

#[test]
fn links_are_logged_revisions_with_cited_evidence_and_replay_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let (log, s) = find_link_run(dir.path());
    let snap = log.open_latest();
    let evs: Vec<EventRow> = snap.rows();
    let revs: Vec<RevisionRow> = snap.rows();
    let causes: Vec<CauseRow> = snap.rows();
    let notes: Vec<&EventRow> = evs.iter().filter(|e| e.tags.split(',').any(|t| t == "transfer_link")).collect();
    assert!(!notes.is_empty(), "this world links a new domain within {LINK_STEPS} steps");
    assert_eq!(notes.len(), s.state.xfer.made);
    for n in &notes {
        assert!(n.source == "being:loop" && tag(&n.tags, "from") == Some("pattern") && tag(&n.tags, "domain") == Some("pattern_b"));
        let key = tag(&n.tags, "rev_key").unwrap();
        let rev = revs.iter().find(|v| v.key == key).expect("a revision of the belief");
        assert!(rev.author == "rule:being-transfer" && rev.rationale.contains("evidence") && rev.rationale.contains("share"));
        let cited: Vec<&EventRow> = causes.iter().filter(|c| c.rev_id == rev.rev_id).filter_map(|c| evs.iter().find(|e| e.event_id == c.event_id)).collect();
        assert!(cited.iter().any(|e| e.event_id == n.event_id), "cites its own note");
        let outcomes = cited.iter().filter(|e| e.kind == "outcome" && e.tags.contains("domain:pattern_b") && tag(&e.tags, "terms").is_some()).count();
        assert!(outcomes >= 2, "cites the logged examples it rests on ({outcomes})");
    }
    // replay: folding the log gives the same model, links, rules and counters as the live run
    let st = BeingState::from_events(&evs);
    assert_eq!(st.model, s.state.model);
    assert_eq!(st.xfer.made, s.state.xfer.made);
    assert_eq!(st.rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), s.state.rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
    assert_eq!(st.step, s.state.step);
    // time travel: before the first link note the log has no link
    let first = notes[0].event_id.clone();
    let before: Vec<EventRow> = evs.iter().take_while(|e| e.event_id != first).cloned().collect();
    assert_eq!(BeingState::from_events(&before).xfer.made, 0);
    let re = super::replay::rederive_rules(&evs);
    assert!(re.iter().all(|x| x.1));
}

#[test]
fn transfer_is_off_unless_the_being_was_created_with_it() {
    // created without learn_transfer: never a link, even in the transfer world
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut cfg = tcfg(LINK_WORLD, LINK_STEPS, 3000, tlimits());
    cfg.learn_transfer = Some(false);
    let s = run(&mut log, &cfg, &ScriptedThinker, &Policy::new(LINK_STEPS)).unwrap();
    assert_eq!((s.state.xfer.made, s.state.xfer.cfg.mode), (0, XferMode::Off));
    // learn_transfer needs the transfer world, and the transfer world needs extended2
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut bad = tcfg(1, 10, 3000, tlimits());
    bad.transfer = None;
    assert!(run(&mut log, &bad, &ScriptedThinker, &Policy::new(10)).is_err());
    let mut bad = tcfg(1, 10, 3000, tlimits());
    bad.ext2 = None;
    assert!(run(&mut log, &bad, &ScriptedThinker, &Policy::new(10)).is_err());
}

#[test]
fn controller_cap_on_links_is_a_logged_refusal_never_silent() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let lim = Limits { max_links: 1, ..tlimits() };
    let s = run(&mut log, &tcfg(LINK_WORLD, LINK_STEPS, 3000, lim), &ScriptedThinker, &Policy::new(LINK_STEPS)).unwrap();
    assert_eq!(s.state.xfer.made, 1, "never more links than the cap");
    assert!(s.refusals.iter().any(|r| r.cap == "max_transfer_links"), "{:?}", s.refusals);
    let logged = log.open_latest().rows::<EventRow>().into_iter().filter(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:max_transfer_links")).count();
    assert_eq!(logged, s.refusals.iter().filter(|r| r.cap == "max_transfer_links").count());
    // a cap of 0 makes no link at all
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run(&mut log, &tcfg(LINK_WORLD, LINK_STEPS, 3000, Limits { max_links: 0, ..tlimits() }), &ScriptedThinker, &Policy::new(LINK_STEPS)).unwrap();
    assert_eq!(s.state.xfer.made, 0);
    assert!(s.refusals.iter().any(|r| r.cap == "max_transfer_links"));
}

#[test]
fn controller_that_does_not_allow_transfer_refuses_and_audits_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let lim = Limits { transfer: false, ..tlimits() };
    let s = run(&mut log, &tcfg(LINK_WORLD, LINK_STEPS, 3000, lim), &ScriptedThinker, &Policy::new(LINK_STEPS)).unwrap();
    assert_eq!(s.state.xfer.made, 0);
    assert_eq!(s.refusals.iter().filter(|r| r.cap == "transfer").count(), 1, "refused once, with the reason");
    assert!(log.open_latest().rows::<EventRow>().iter().any(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:transfer")));
}

#[test]
fn controller_txt_has_transfer_keys_with_safe_defaults_and_fails_closed() {
    let d = Controller::parse("").unwrap();
    assert!(!d.transfer && d.max_transfer_links == super::controller::DEFAULT_LINKS);
    let c = Controller::parse("transfer = on\nmax_transfer_links = 3\n").unwrap();
    assert!(c.transfer && c.max_transfer_links == 3);
    let (l, _) = c.limits(false, false);
    assert!(l.transfer && l.max_links == 3);
    let (l, _) = d.limits(false, false);
    assert!(!l.transfer);
    for bad in ["transfer = maybe", "max_transfer_links = 5000", "max_transfer_links = 1\nmax_transfer_links = 2", "transfer = on\ntransfer = off"] {
        assert!(Controller::parse(bad).is_err(), "{bad}");
        let f = Controller::parse_or_closed(bad);
        assert!(f.is_closed() && !f.transfer && f.max_transfer_links == 0, "{bad}");
    }
}

#[test]
fn transfer_never_accepts_a_wrong_rule_and_the_measurement_is_deterministic() {
    let pol = Policy::new(100_000);
    for seed in [4u64, 7] {
        for arm in [Arm::Transfer, Arm::WrongSim, Arm::WrongSimNoGuard] {
            let o = run_b6(seed, 1500, arm, B6Cfg::default(), &pol);
            let (exact, wrong, inert, det) = wrong_rules(seed, 1500, &o);
            assert_eq!(wrong, 0, "seed {seed} {}: {det:?}", arm.name());
            assert_eq!(exact + inert, o.accepted.len());
        }
    }
    let a = run_b6(4, 1500, Arm::Transfer, B6Cfg::default(), &pol);
    let b = run_b6(4, 1500, Arm::Transfer, B6Cfg::default(), &pol);
    assert_eq!((a.total_rate(), a.links.len()), (b.total_rate(), b.links.len()));
    let key = |l: &XferRec| (l.step, l.target, l.difficulty);
    assert_eq!(a.links.iter().map(key).collect::<Vec<_>>(), b.links.iter().map(key).collect::<Vec<_>>());
}

#[test]
fn transfer_sources_read_only_the_log() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being");
    let t = std::fs::read_to_string(dir.join("transfer.rs")).unwrap();
    // code, not the explanatory comments
    let code: String = t.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    for hidden in ["task_forced", "reward_scale", "cost_base", "diff_bias", "Profile", "pattern_bonus", "world::World", "world::Xfer", "Xfer::new", ".expected()", "world_seed", "options_ext", "world::Kind"] {
        assert!(!code.contains(hidden), "transfer.rs mentions {hidden}");
    }
    for forbidden in [["std", "::fs"].concat(), ["std", "::net"].concat(), ["Command", "::new"].concat()] {
        assert!(!code.contains(forbidden.as_str()), "transfer.rs uses {forbidden}");
    }
    let m = std::fs::read_to_string(dir.join("model.rs")).unwrap();
    for hidden in ["task_forced", "reward_scale", "Xfer::new", "world::World"] {
        assert!(!m.contains(hidden), "model.rs mentions {hidden}");
    }
}
