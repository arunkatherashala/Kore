//! Tests of World v2: determinism and replay from the log, checker correctness, equivalence of the
//! learner with the existing B1 learner, controller caps, kill switch, isolation from the first world.

use super::baselines::{Always2, Oracle2, Random2, Ucb2};
use super::engine::{run_v2, Engine2, LearnerCfg, Run2Cfg, Run2Summary, State2};
use super::learner::{Chooser2, Learner2, Offer2};
use super::measure::{rule_on_fresh, run_w2};
use super::model::FamModel;
use super::replay::{rederive_rules2, replay_decisions2};
use super::solve::{self, sig2};
use super::world::*;
use crate::being::controller::Limits;
use crate::being::discover::{search, Example, Rule};
use crate::being::model::{Model, Obs};
use crate::being::planner::{MenuChooser, ModelChooser, Offer};
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::solver::{Strategy, STRATEGIES_EXT};
use crate::being::state::{BeingState, Genesis, Outcome};
use crate::being::world::{Domain, Rng, ALL_DOMAINS_X};
use crate::versioned::{EventRow, VersionedLog};

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn events(log: &VersionedLog) -> Vec<EventRow> {
    log.open_latest().rows()
}

fn cfg(seed: u64, fams: usize, budget: i64) -> W2Cfg {
    W2Cfg { noise_pm: 80, drift_per_10k: 25, curve_pct: 50, ..W2Cfg::new(seed, fams, budget) }
}

fn run_logged(root: &std::path::Path, c: W2Cfg, steps: u64, limits: Option<Limits>) -> (VersionedLog, Run2Summary) {
    let mut log = open(root);
    let s = run_v2(&mut log, &Run2Cfg { world: Some(c), learner: None, discover_every: 10, ctl: limits }, &Policy::new(steps), None).unwrap();
    (log, s)
}

/// Independent statement of each hidden rule: does the whole sequence follow it?
fn fits(class: SeqClass, t: &[i64]) -> bool {
    let n = t.len();
    let d = |k: usize| -> Vec<i64> {
        let mut v = t.to_vec();
        for _ in 0..k {
            v = v.windows(2).map(|w| w[1] - w[0]).collect();
        }
        v
    };
    let constant = |v: &[i64]| v.iter().all(|x| *x == v[0]);
    match class {
        SeqClass::Arith => constant(&d(1)),
        SeqClass::Geom => t[0] != 0 && (1..n).all(|i| t[i] * t[0] == t[i - 1] * t[1]),
        SeqClass::Quad => constant(&d(2)),
        SeqClass::Cubic => constant(&d(3)),
        SeqClass::Fib => (2..n).all(|i| t[i] == t[i - 1] + t[i - 2]),
        SeqClass::Tribo => (3..n).all(|i| t[i] == t[i - 1] + t[i - 2] + t[i - 3]),
        SeqClass::Periodic3 => (3..n).all(|i| t[i] == t[i - 3]),
        SeqClass::Interleave => constant(&(2..n).map(|i| t[i] - t[i - 2]).collect::<Vec<_>>()),
        SeqClass::AffRec => [2i64, 3, -2].iter().any(|p| constant(&(1..n).map(|i| t[i] - p * t[i - 1]).collect::<Vec<_>>())),
        SeqClass::Rec2 => (1..=3i64).any(|p| (-2..=3i64).any(|q| (2..n).all(|i| t[i] == p * t[i - 1] + q * t[i - 2]))),
        SeqClass::Primes => t.iter().all(|x| *x > 1 && (2..*x).all(|k| x % k != 0)) && t.windows(2).all(|w| w[1] > w[0] && (w[0] + 1..w[1]).all(|y| (2..y).any(|k| y % k == 0))),
        SeqClass::Lcg => [2i64, 3, 5].iter().any(|a| (1..=9i64).any(|c| (1..n).all(|i| t[i] == (a * t[i - 1] + c) % 101))),
    }
}

// ---- determinism and replay ----

#[test]
fn world_is_a_pure_function_of_its_config() {
    let a = World2::new(cfg(7, 18, 3000));
    let b = World2::new(cfg(7, 18, 3000));
    assert_eq!(a.fams, b.fams);
    assert_eq!(a.drift, b.drift);
    assert!(!a.drift.is_empty());
    for step in [0u64, 5, 99] {
        for clock in [0i64, 30_000, 80_000] {
            let (x, y) = (a.offers(step, clock), b.offers(step, clock));
            assert_eq!(x.iter().map(|t| (&t.view, t.expected())).collect::<Vec<_>>(), y.iter().map(|t| (&t.view, t.expected())).collect::<Vec<_>>());
        }
    }
    assert_ne!(World2::new(cfg(8, 18, 3000)).offers(0, 0)[0].view, a.offers(0, 0)[0].view);
    for t in FTYPES {
        assert!(a.fams.iter().any(|f| f.ftype == t), "{t:?}");
    }
}

#[test]
fn two_logged_runs_write_identical_logs_and_match_the_unlogged_measurement() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (l1, s1) = run_logged(d1.path(), cfg(3, 12, 3000), 250, None);
    let (l2, s2) = run_logged(d2.path(), cfg(3, 12, 3000), 250, None);
    let (e1, e2) = (events(&l1), events(&l2));
    assert_eq!(e1.len(), e2.len());
    for (a, b) in e1.iter().zip(&e2) {
        assert_eq!((&a.event_id, &a.text, &a.tags), (&b.event_id, &b.text, &b.tags));
    }
    assert_eq!(s1.truths, s2.truths);
    assert_eq!(s1.state, s2.state);
    let m = run_w2(&cfg(3, 12, 3000), &mut Learner2::default(), 10, None);
    assert_eq!(&m.recs[..250], &s1.truths[..]);
}

#[test]
fn decisions_and_rules_replay_from_the_log_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut log, s) = run_logged(dir.path(), cfg(3, 12, 4000), 400, None);
    assert_eq!(s.steps_run, 400);
    let rows = events(&log);
    let st = State2::from_events(&rows);
    assert_eq!(st, s.state, "the state folded from the log equals the live state");
    let rep = replay_decisions2(&rows);
    assert_eq!(rep.len(), 400);
    let bad: Vec<_> = rep.iter().filter(|r| !r.matches()).collect();
    assert!(bad.is_empty(), "{} decisions differ, first {:?}", bad.len(), bad.first());
    let rr = rederive_rules2(&rows);
    assert!(!rr.is_empty(), "this world yields rules within 400 steps");
    assert!(rr.iter().all(|r| r.1), "{rr:?}");
    // every decision revision cites the genesis event, and only events that exist in the log
    let snap = log.open_latest();
    let ids: std::collections::HashSet<String> = rows.iter().map(|e| e.event_id.clone()).collect();
    let causes: Vec<crate::versioned::CauseRow> = snap.rows();
    let revs: Vec<crate::versioned::RevisionRow> = snap.rows();
    let dec: Vec<_> = revs.iter().filter(|r| r.key.starts_with("being/world2/choice/")).collect();
    assert_eq!(dec.len(), 400);
    for r in dec.iter().step_by(7) {
        let cited: Vec<_> = causes.iter().filter(|c| c.rev_id == r.rev_id).collect();
        assert!(cited.iter().any(|c| c.event_id == st.genesis_event));
        assert!(cited.len() >= 2 || r.key.ends_with("000000"));
        assert!(cited.iter().all(|c| ids.contains(&c.event_id)));
    }
    // a continued run picks up from the log, and still replays
    let s2 = run_v2(&mut log, &Run2Cfg { discover_every: 10, ..Default::default() }, &Policy::new(50), None).unwrap();
    assert!(!s2.created);
    assert_eq!(s2.state.step, 450);
    let rep2 = replay_decisions2(&events(&log));
    assert!(rep2.len() == 450 && rep2.iter().all(|r| r.matches()));
}

// ---- checkers and ground truth ----

#[test]
fn bfs_and_knapsack_are_exact() {
    let cells = [0, 0, 0, 0, 1, 0, 0, 0, 0];
    assert_eq!(bfs(3, 3, &cells, false), Some(4));
    assert_eq!(bfs(3, 3, &cells, true), Some(3));
    assert_eq!(bfs(2, 2, &[0, 1, 1, 0], false), None);
    assert_eq!(bfs(2, 2, &[0, 1, 1, 0], true), Some(1));
    let mut r = Rng::new(9);
    for _ in 0..300 {
        let n = r.range(1, 8) as usize;
        let it: Vec<(i64, i64)> = (0..n).map(|_| (r.range(1, 9), r.range(1, 20))).collect();
        let cap = r.range(0, 30);
        let mut best = 0;
        for m in 0..(1u32 << n) {
            let (w, v) = it.iter().enumerate().filter(|(i, _)| m >> i & 1 == 1).fold((0, 0), |a, (_, x)| (a.0 + x.0, a.1 + x.1));
            if w <= cap {
                best = best.max(v);
            }
        }
        assert_eq!(knap_exact(cap, &it), best);
    }
}

#[test]
fn every_class_generator_follows_its_rule() {
    let mut r = Rng::new(21);
    for class in SEQ_CLASSES {
        for d in 1..=5u8 {
            for _ in 0..40 {
                let (mut t, next) = seq_task(class, d, 1, &mut r);
                t.push(next);
                assert!(fits(class, &t), "{class:?} d{d}: {t:?}");
            }
        }
    }
}

#[test]
fn checkers_agree_with_independent_ground_truth() {
    let w = World2::new(W2Cfg::new(11, 30, 3000));
    for f in 0..30 {
        for d in 1..=5u8 {
            for i in 0..20u64 {
                let t = w.task(i, 0, f, Some(d), 0, 77);
                let v = &t.view;
                assert!(t.check(t.expected()) && !t.check(t.expected() + 1));
                match v.ftype {
                    FType::Grid => {
                        let eight = w.regime(f, 0).eight;
                        let (rr, cc) = (v.params[0] as usize, v.params[1] as usize);
                        assert_eq!(bfs(rr, cc, &v.params[2..], eight), Some(t.expected()));
                        assert!(bfs(rr, cc, &v.params[2..], false).is_some(), "reachable with 4 moves");
                    }
                    FType::Knap => {
                        let it: Vec<(i64, i64)> = v.params[1..].chunks(2).map(|c| (c[0], c[1])).collect();
                        assert_eq!(knap_exact(v.params[0], &it), t.expected());
                        assert_eq!(solve::solve(v, Strategy::Deep, &[]).answer, t.expected());
                    }
                    FType::Formula => {
                        let g = w.regime(f, 0).g;
                        let x = v.params[0];
                        let want = if v.formula == Some(FormulaKind::Fall) { g * x * x / 2 } else { g * x };
                        assert_eq!(t.expected(), want);
                        assert_eq!(solve::solve(v, Strategy::Careful, &[]).answer == t.expected(), g == solve::TEXTBOOK_G);
                    }
                    FType::Seq => {
                        let mut full = v.params.clone();
                        full.push(t.expected());
                        assert!(fits(w.class_of(f, 0), &full));
                        assert_eq!(v.horizon, 1);
                    }
                    FType::Compose => {
                        let class = w.class_of(f, 0);
                        assert!(fits(class, &v.params));
                        assert!((2..=4).contains(&v.horizon));
                        if matches!(class, SeqClass::Arith | SeqClass::Geom | SeqClass::Quad | SeqClass::Fib) {
                            // the careful detector is exact for these classes: iterating it h times must give the checker's answer
                            assert_eq!(solve::solve(v, Strategy::Careful, &[]).answer, t.expected(), "{class:?}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn fixed_solvers_and_grammar_cover_what_the_design_says() {
    let mut r = Rng::new(5);
    for class in SEQ_CLASSES {
        let mut exs = vec![];
        let (mut careful_ok, mut deep_ok) = (0, 0);
        for i in 0..60 {
            let (t, next) = seq_task(class, 3, 1, &mut r);
            let v = TaskView2 { id: String::new(), step: 0, slot: 0, fam: 0, ftype: FType::Seq, base: None, difficulty: 3, params: t.clone(), horizon: 1, formula: None, prompt: String::new(), cost: 1, reward: 1 };
            careful_ok += (solve::solve(&v, Strategy::Careful, &[]).answer == next) as u32;
            deep_ok += (solve::solve(&v, Strategy::Deep, &[]).answer == next) as u32;
            exs.push(Example { event_id: format!("e{i}"), sig: sig2(0, 3), terms: t, next });
        }
        let found = search(&exs);
        let book: Vec<Rule> = found.iter().map(|f| Rule::new_x(f.sig, f.xf, f.form, f.train_ok, f.val_ok, 0, 0)).collect();
        let mut rule_ok = 0;
        for _ in 0..60 {
            let (t, next) = seq_task(class, 3, 1, &mut r);
            rule_ok += crate::being::discover::apply(&book, sig2(0, 3), &t).map_or(false, |(x, _)| x == next) as u32;
        }
        match class {
            SeqClass::Arith | SeqClass::Geom | SeqClass::Quad | SeqClass::Fib => assert!(careful_ok >= 58, "{class:?} careful {careful_ok}"),
            SeqClass::Cubic | SeqClass::Periodic3 | SeqClass::Interleave => assert!(deep_ok >= 58 && careful_ok < 30, "{class:?} deep {deep_ok} careful {careful_ok}"),
            _ => assert!(deep_ok < 30, "{class:?} deep {deep_ok}"),
        }
        if class.in_grammar() {
            assert!(rule_ok >= 50, "{class:?}: rules {:?} right on {rule_ok}/60 fresh", found.iter().map(|f| f.form.code()).collect::<Vec<_>>());
        } else {
            assert!(found.is_empty(), "{class:?}: a negative control was accepted");
        }
    }
}

#[test]
fn feedback_noise_has_the_configured_rate_and_is_consistent() {
    let clean = World2::new(W2Cfg::new(4, 12, 3000));
    let noisy = World2::new(W2Cfg { noise_pm: 150, ..W2Cfg::new(4, 12, 3000) });
    let (mut flips, mut n, mut expect) = (0u64, 0u64, 0u64);
    for step in 0..3000u64 {
        let cl = clean.offers(step, 0);
        for t in noisy.offers(step, 0) {
            let ans = if step % 2 == 0 { t.expected() } else { t.expected() + 5 };
            let fb = noisy.feedback(&t, ans);
            assert_eq!(fb.reported_correct, ans == fb.revealed, "the revealed answer agrees with the report");
            assert_eq!(fb.flipped, fb.reported_correct != t.check(ans));
            flips += fb.flipped as u64;
            n += 1;
            expect += 150 * noisy.fams[t.view.fam].noise_mult as u64;
            assert!(!clean.feedback(&cl[t.view.slot], ans).flipped);
        }
    }
    let (rate, want) = (flips as f64 / n as f64, expect as f64 / 1000.0 / n as f64);
    assert!((rate - want).abs() < 0.02, "flip rate {rate:.3}, expected about {want:.3}");
}

#[test]
fn drift_changes_hidden_rules_on_schedule_only() {
    let w = World2::new(W2Cfg { drift_per_10k: 40, ..W2Cfg::new(6, 12, 3000) });
    let (t, f) = w.drift[0];
    assert_eq!(w.regime_index(f, t - 1), 0);
    assert_eq!(w.regime_index(f, t), 1);
    assert_ne!(w.regime(f, t - 1), w.regime(f, t));
    for g in (0..12).filter(|g| w.drift.iter().all(|(tt, gg)| gg != g || *tt > t)) {
        assert_eq!(w.regime(g, 0), w.regime(g, t));
    }
    let n = w.drift.len() as f64;
    assert!((n - 48.0).abs() < 15.0, "{n} events (expected about 40 per 10000 energy over 4 x the budget)");
    assert!(World2::new(W2Cfg::new(6, 12, 3000)).drift.is_empty());
}

#[test]
fn config_round_trips_through_genesis_tags_and_is_validated() {
    let c = cfg(42, 20, 5000);
    assert_eq!(W2Cfg::from_tags(&c.tags().join(",")), Some(c.clone()));
    assert!(W2Cfg { families: 0, ..c.clone() }.validate().is_err());
    assert!(W2Cfg { families: 49, ..c.clone() }.validate().is_err());
    assert!(W2Cfg { noise_pm: 401, ..c.clone() }.validate().is_err());
    assert!(W2Cfg { menu: 1, ..c.clone() }.validate().is_err());
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    assert!(run_v2(&mut log, &Run2Cfg { world: Some(W2Cfg { families: 0, ..c.clone() }), ..Default::default() }, &Policy::new(5), None).is_err());
    assert!(run_v2(&mut log, &Run2Cfg::default(), &Policy::new(5), None).is_err());
    assert_eq!(events(&log).len(), 0, "a refused request creates nothing");
}

// ---- the learner is the existing learner ----

#[test]
fn family_model_equals_the_b1_model_bit_for_bit() {
    let mut m = Model::default();
    let mut f = FamModel::new(7);
    let mut r = Rng::new(3);
    for i in 0..3000 {
        let d = ALL_DOMAINS_X[r.below(7) as usize];
        let k = 1 + r.below(5) as u8;
        let s = STRATEGIES_EXT[r.below(4) as usize];
        let ok = r.below(3) > 0;
        let listed = r.range(3, 11);
        let spent = s.energy(listed);
        m.observe(&Obs { domain: d, difficulty: k, strategy: s, correct: ok, listed_cost: listed, spent });
        f.observe(d.idx(), k, s, ok, listed, spent);
        if i % 97 == 0 {
            m.reset_cell(d, k, Strategy::Discovered);
            f.reset_cell(d.idx(), k, Strategy::Discovered);
        }
        if i % 13 == 0 {
            for dd in ALL_DOMAINS_X {
                for kk in 1..=5u8 {
                    for ss in STRATEGIES_EXT {
                        let (a, b) = (m.estimate(dd, kk, ss), f.estimate(dd.idx(), kk, ss));
                        assert_eq!((a.n, a.p.to_bits(), a.sd.to_bits(), a.cost_ratio.to_bits(), a.group_n.to_bits()), (b.n, b.p.to_bits(), b.sd.to_bits(), b.cost_ratio.to_bits(), b.group_n.to_bits()));
                    }
                }
            }
        }
    }
    assert_eq!(m.attempts(), f.attempts());
}

#[test]
fn learner_chooses_exactly_what_the_b1_planner_chooses() {
    let g = Genesis { world_seed: 1, loop_seed: 1, budget: 100_000, explore_c: 1.5, chooser: crate::being::state::Chooser::Model, extended: true, drift: false, adapt: Default::default(), advice: Default::default(), ext2: false, rule_drift: false, transfer: false, learn_transfer: false };
    let mut bs = BeingState::default();
    bs.start(g, "genesis".into());
    let mut s2 = State2::default();
    s2.start(W2Cfg::new(1, 7, 100_000), LearnerCfg::default(), "genesis".into());
    let mut mc = ModelChooser::new(1.5);
    let mut l2 = Learner2::default();
    let mut r = Rng::new(17);
    let (mut budget, mut clock) = (100_000i64, 0i64);
    let mut skips = 0;
    for step in 0..1500u64 {
        let offers: Vec<(Domain, u8, i64, i64)> = (0..4).map(|_| (ALL_DOMAINS_X[r.below(7) as usize], 1 + r.below(5) as u8, r.range(3, 11), r.range(2, 30))).collect();
        let o1: Vec<Offer> = offers.iter().enumerate().map(|(i, o)| Offer { slot: i, domain: o.0, difficulty: o.1, listed_cost: o.2, listed_reward: o.3 }).collect();
        let o2: Vec<Offer2> = offers.iter().enumerate().map(|(i, o)| Offer2 { slot: i, fam: o.0.idx(), ftype: FType::Seq, difficulty: o.1, listed_cost: o.2, listed_reward: o.3 }).collect();
        let a = mc.choose(&bs, &o1).unwrap();
        let b = l2.choose(&s2, &o2).unwrap();
        assert_eq!(a.action.label(), b.act.label(), "step {step}");
        assert_eq!(a.score.to_bits(), b.score.to_bits());
        let (slot, strategy) = match a.action {
            crate::being::planner::Action::Attempt { slot, strategy } => (Some(slot), Some(strategy)),
            crate::being::planner::Action::Skip => (None, None),
        };
        let (cost, reward, ok) = match (slot, strategy) {
            (Some(sl), Some(s)) => {
                let ok = r.below(2) == 0;
                (s.energy(offers[sl].2), if ok { offers[sl].3 } else { 0 }, ok)
            }
            _ => {
                skips += 1;
                (1, 0, false)
            }
        };
        budget -= cost;
        clock += cost * 30;
        let o = Outcome { step, domain: slot.map_or(Domain::Arithmetic, |s| offers[s].0), task_id: "t".into(), difficulty: slot.map_or(1, |s| offers[s].1), correct: ok, reward, cost, budget_left: budget, clock, event_id: format!("e{step}"), strategy, listed_cost: slot.map_or(cost, |s| offers[s].2), skipped: slot.is_none(), example: None, rule_used: None };
        bs.apply(&o);
        let res = if slot.is_none() { "skip" } else if ok { "correct" } else { "wrong" };
        let o2 = super::engine::Outcome2 { step, fam: slot.map(|s| offers[s].0.idx()), difficulty: o.difficulty, strategy, result: res.into(), reward, cost, listed: o.listed_cost, budget_left: budget, clock, event_id: o.event_id.clone(), example: None };
        s2.apply(&o2);
    }
    assert!(bs.model.attempts() > 1000 && skips > 0, "both branches exercised: {} attempts, {skips} skips", bs.model.attempts());
}

// ---- controller caps and kill switch ----

#[test]
fn controller_caps_hold_and_refusals_are_logged() {
    let dir = tempfile::tempdir().unwrap();
    let lim = Limits { max_live_rules: 2, max_candidates: 100_000, compose: true, max_blocks: 1, ..Limits::default() };
    let (log, s) = run_logged(dir.path(), W2Cfg::new(3, 12, 6000), 700, Some(lim));
    assert!(s.state.live_rules() <= 2, "{} live rules", s.state.live_rules());
    let ev = events(&log);
    let refusals: Vec<&EventRow> = ev.iter().filter(|e| e.source == SOURCE2 && e.tags.contains("controller_refusal")).collect();
    assert!(refusals.iter().any(|e| e.tags.contains("cap:max_live_rules")), "the live-rule cap binds in this run");
    // a small candidate cap: no discovery pass evaluates more, and the cut is a refusal
    let lim = Limits { max_candidates: 40, compose: true, max_blocks: 1, ..Limits::default() };
    let mut st = State2::default();
    st.start(W2Cfg::new(3, 12, 6000), LearnerCfg::default(), "g".into());
    let mut e = Engine2::new(st, Some(lim));
    let mut l = Learner2::default();
    for i in 0..400 {
        if e.step(&mut l).is_err() {
            break;
        }
        if i % 10 == 9 {
            e.discover_pass();
        }
    }
    assert!(e.refusals_total.iter().any(|r| r.cap == "max_search_candidates"));
    assert!(e.max_candidates_seen <= 40, "{}", e.max_candidates_seen);
    // energy cap: the run stops; the stop and the refusal are logged
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut pol = Policy::new(1000);
    pol.max_energy = Some(150);
    let s = run_v2(&mut log, &Run2Cfg { world: Some(W2Cfg::new(2, 12, 3000)), ..Default::default() }, &pol, None).unwrap();
    assert_eq!(s.stopped, StopReason::EnergyCap);
    assert!(s.cost >= 150 && s.cost < 150 + 25, "{}", s.cost);
    let ev = events(&log);
    assert!(ev.iter().any(|e| e.tags.contains("cap:max_energy_per_run")));
    assert!(ev.iter().any(|e| e.tags.contains("controller_stop")));
    // step cap
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run_v2(&mut log, &Run2Cfg { world: Some(W2Cfg::new(2, 12, 3000)), ..Default::default() }, &Policy::new(33), None).unwrap();
    assert_eq!((s.steps_run, s.stopped), (33, StopReason::MaxSteps));
}

#[test]
fn kill_switch_stops_world_v2_within_one_step() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(|| true)));
    let s = run_v2(&mut log, &Run2Cfg { world: Some(cfg(1, 12, 3000)), discover_every: 10, ..Default::default() }, &pol, None).unwrap();
    assert_eq!((s.steps_run, s.stopped), (0, StopReason::Killed));
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let calls = Arc::new(AtomicU64::new(0));
    let c2 = calls.clone();
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 13)));
    let s = run_v2(&mut log, &Run2Cfg { world: Some(cfg(1, 12, 3000)), discover_every: 10, ..Default::default() }, &pol, None).unwrap();
    assert_eq!((s.steps_run, s.stopped), (12, StopReason::Killed));
    let ev = events(&log);
    let stops: Vec<&EventRow> = ev.iter().filter(|e| e.tags.contains("controller_stop")).collect();
    assert_eq!(stops.len(), 1);
    assert!(stops[0].tags.contains("stop:kill_switch") && stops[0].source == SOURCE2);
    assert_eq!(State2::from_events(&ev).step, 12);
}

// ---- isolation from the first world ----

#[test]
fn world_v2_rows_do_not_change_the_first_being() {
    use crate::being::run_loop::{run, RunCfg};
    use crate::being::ScriptedThinker;
    let c1 = RunCfg { world_seed: Some(3), loop_seed: Some(2), budget: Some(3000), chooser: Some(crate::being::state::Chooser::Model), steps: 120, ..Default::default() };
    let d1 = tempfile::tempdir().unwrap();
    let mut l1 = open(d1.path());
    let a = run(&mut l1, &c1, &ScriptedThinker, &Policy::new(120)).unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let mut l2 = open(d2.path());
    run_v2(&mut l2, &Run2Cfg { world: Some(cfg(9, 12, 3000)), discover_every: 10, ..Default::default() }, &Policy::new(80), None).unwrap();
    let b = run(&mut l2, &c1, &ScriptedThinker, &Policy::new(120)).unwrap();
    assert_eq!((&a.state.model, a.state.step, a.state.budget_left, a.state.total_reward, a.state.total_cost, a.state.clock), (&b.state.model, b.state.step, b.state.budget_left, b.state.total_reward, b.state.total_cost, b.state.clock));
    let r1: Vec<_> = crate::being::replay::replay_decisions(&events(&l1)).into_iter().map(|r| (r.recorded, r.rederived)).collect();
    let r2: Vec<_> = crate::being::replay::replay_decisions(&events(&l2)).into_iter().map(|r| (r.recorded, r.rederived)).collect();
    assert_eq!(r1, r2);
    let ev = events(&l2);
    assert_eq!(State2::from_events(&ev).step, 80);
    assert!(replay_decisions2(&ev).iter().all(|r| r.matches()));
    let v1: Vec<_> = events(&l1).into_iter().filter(|e| e.source == crate::being::state::SOURCE).map(|e| (e.event_id, e.text, e.tags)).collect();
    let v2: Vec<_> = ev.into_iter().filter(|e| e.source == crate::being::state::SOURCE).map(|e| (e.event_id, e.text, e.tags)).collect();
    assert_eq!(v1, v2, "the first being's rows are identical with or without a World v2 being in the log");
}

// ---- baselines, oracles, rules, sources ----

#[test]
fn baselines_and_oracles_run_and_the_oracle_knowing_rules_is_strong() {
    let mut sums = [0f64; 3];
    for seed in 1..=4u64 {
        let c = W2Cfg::new(seed, 12, 2000);
        sums[0] += run_w2(&c, &mut Random2::new(seed), 0, None).per_energy();
        let _ = run_w2(&c, &mut Always2 { strategy: Strategy::Careful }, 0, None);
        let _ = run_w2(&c, &mut Ucb2 { c: 0.05 }, 0, None);
        sums[1] += run_w2(&c, &mut Oracle2::new(World2::new(c.clone()), true), 0, None).per_energy();
        sums[2] += run_w2(&c, &mut Learner2::default(), 10, None).per_energy();
    }
    assert!(sums[1] > sums[0] && sums[1] > sums[2], "{sums:?}");
}

#[test]
fn accepted_rules_are_checked_on_fresh_tasks() {
    let c = W2Cfg::new(3, 12, 4000);
    let run = run_w2(&c, &mut Learner2::default(), 10, None);
    let w = World2::new(c);
    assert!(!run.rules.is_empty());
    for rc in &run.rules {
        let (app, ok) = rule_on_fresh(&w, &rc.rule, rc.clock, 50);
        assert_eq!(rc.wrong_at_accept, ok < app);
    }
    // clean, stationary world: no accepted rule is wrong on fresh tasks
    assert!(run.rules.iter().all(|r| !r.wrong_at_accept), "{:?}", run.rules.iter().filter(|r| r.wrong_at_accept).map(|r| r.rule.form.code()).collect::<Vec<_>>());
}

#[test]
fn world2_sources_have_no_file_network_or_process_access_and_the_learner_reads_no_hidden_parameter() {
    let banned = [
        ["std", "::fs"].concat(), ["std", "::net"].concat(), ["std", "::process"].concat(), ["std", "::env"].concat(),
        ["File", "::open"].concat(), ["File", "::create"].concat(), ["Tcp", "Stream"].concat(), ["Udp", "Socket"].concat(),
        ["Command", "::new"].concat(), ["tokio", "::net"].concat(), ["safety", "::"].concat(), ["log_tools", "::"].concat(),
        ["read_", "to_string"].concat(), ["std", "::thread"].concat(),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being").join("world2");
    let mut n = 0;
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = f.file_name().to_string_lossy().to_string();
        if name.starts_with("tests") || !name.ends_with(".rs") {
            continue;
        }
        n += 1;
        let src = std::fs::read_to_string(f.path()).unwrap();
        for b in &banned {
            assert!(!src.contains(b.as_str()), "{name} uses {b}");
        }
    }
    assert!(n >= 8);
    let hidden = ["regime", "Regime", ".expected(", "slip_noise", "class_of", "noise_mult", "slip_pm", "World2", "truth", "feedback(", "wall_pm", "SeqClass"];
    for f in ["model.rs", "learner.rs"] {
        let src = std::fs::read_to_string(dir.join(f)).unwrap();
        for b in hidden {
            assert!(!src.contains(b), "{f} mentions {b}");
        }
    }
}
