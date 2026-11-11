//! Track S1 tests (`docs/s1_self_tune_prereg.md`): the self-tuned beta (`wt_self_tune`). Key absent: nothing
//! changes (also pinned by `tests_golden.rs`, files unchanged); the setting is fixed at the genesis and read only
//! with `wt_file_key` on; the block rule as pre-registered; the three equivalence and safety tests (decisions inside
//! a block are B1's with that beta, also across a fold restart and a checkpoint restart; replay from the log alone
//! reproduces every pick and promotion; KILL in the middle of a block); a format-2 checkpoint is refused.

use super::engine::*;
use super::learner::*;
use super::self_tune::{self, *};
use super::world::*;
use crate::being::ckpt::{self, Foldable};
use crate::being::controller::Controller;
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::state::tag;
use crate::versioned::{EventRow, RevisionRow, VersionedLog};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn rc(c: &WTCfg) -> RunTCfg {
    RunTCfg { world: Some(c.clone()), learner: None, discover_every: DEFAULT_DISCOVER_EVERY, ctl: None, sql: None }
}

fn rows(log: &VersionedLog) -> Vec<EventRow> {
    log.open_latest().rows()
}

fn b1(beta_milli: u32) -> LearnerTCfg {
    LearnerTCfg { beta_milli, file_key: true, ..Default::default() }
}

/// The in-memory self-tuned life, keeping the state at the start of every block (after the block's pick).
struct S1Life {
    plans: Vec<Option<Plan>>,
    reward: i64,
    cost: i64,
    starts: Vec<StateT>,
    tune: SelfTune,
}

fn s1_life(c: &WTCfg) -> S1Life {
    let mut st = StateT::default();
    st.start(c.clone(), s1_learner(), "genesis".into());
    st.self_tune = SelfTune::on();
    let mut eng = EngineT::new(st, None);
    eng.measure = true;
    let w = WorldT::new(c.clone());
    let (mut plans, mut r, mut e, mut starts) = (vec![], 0, 0, vec![]);
    loop {
        if self_tune::before_step(&mut eng.state).is_some() {
            starts.push(eng.state.clone());
        }
        let Ok(s) = eng.step(&mut SelfTunedT, &MemSandbox(&w)) else { break };
        plans.push(s.truth.plan);
        r += s.truth.reward;
        e += s.truth.cost;
        if eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    S1Life { plans, reward: r, cost: e, starts, tune: eng.state.self_tune.clone() }
}

/// B1 with a fixed beta from a given state (a fresh engine, as after a restart) for at most `n` steps.
fn b1_from(st: &StateT, beta_milli: u32, n: u64) -> Vec<Option<Plan>> {
    let w = WorldT::new(st.cfg.clone().unwrap());
    let mut eng = EngineT::new(st.clone(), None);
    eng.measure = true;
    let mut ch = LearnerT { cfg: b1(beta_milli) };
    let mut plans = vec![];
    for _ in 0..n {
        let Ok(s) = eng.step(&mut ch, &MemSandbox(&w)) else { break };
        plans.push(s.truth.plan);
        if eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    plans
}

/// The logged self-tuned being in chunks of steps with a cold restart between them.
fn logged(root: &std::path::Path, c: &WTCfg, chunks: &[u64], every: u64) -> (Vec<Option<Plan>>, i64, i64, Vec<RunTSummary>) {
    let w = WorldT::new(c.clone());
    let (mut plans, mut r, mut e, mut sums) = (vec![], 0, 0, vec![]);
    for &n in chunks {
        let mut log = open_log(root);
        let mut p = Policy::new(n);
        p.checkpoint_every = every;
        let s = run_t_self(&mut log, &rc(c), Some(true), Some(true), &p, None, &MemSandbox(&w)).unwrap();
        plans.extend(s.truths.iter().map(|t| t.plan));
        r += s.reward;
        e += s.cost;
        sums.push(s);
    }
    (plans, r, e, sums)
}

#[test]
fn key_absent_or_off_changes_nothing_and_the_setting_is_read_only_with_the_file_key() {
    assert!(!StateT::default().self_tune.on);
    assert_eq!(genesis_setting("wt_beta_milli:2000"), Some(false));
    assert_eq!(genesis_setting("wt_file_key:on,wt_self_tune:off"), Some(false));
    assert_eq!(genesis_setting("wt_file_key:on,wt_self_tune:on"), Some(true));
    assert_eq!(genesis_setting("wt_self_tune:on"), None, "on without the file key is not a genesis");
    assert_eq!(genesis_setting("wt_file_key:on,wt_self_tune:yes"), None);
    let c = WTCfg::new(7, 1200);
    let w = WorldT::new(c.clone());
    let genesis = |log: &VersionedLog| rows(log).into_iter().find(|e| e.tags.contains("wt_genesis")).unwrap();
    // with the file key on, an explicit off writes the same genesis and log as no request
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mut l1, mut l2) = (open_log(d1.path()), open_log(d2.path()));
    run_t_self(&mut l1, &rc(&c), Some(true), None, &Policy::new(60), None, &MemSandbox(&w)).unwrap();
    run_t_self(&mut l2, &rc(&c), Some(true), Some(false), &Policy::new(60), None, &MemSandbox(&w)).unwrap();
    assert_eq!(super::tests_golden::log_digest(&l1), super::tests_golden::log_digest(&l2));
    assert!(!genesis(&l1).tags.contains("wt_self_tune"));
    assert!(rows(&l1).iter().all(|e| !e.tags.contains("self_tune")));
    // without the file key the request is not read: the being is B0
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let s = run_t_self(&mut log, &rc(&c), None, Some(true), &Policy::new(30), None, &MemSandbox(&w)).unwrap();
    assert!(s.created && !s.state.self_tune.on && !s.state.learner.file_key);
    assert!(!genesis(&log).tags.contains("wt_self_tune"));
    // and a later request for it on that being is not read either (no refusal)
    assert!(run_t_self(&mut log, &rc(&c), None, Some(true), &Policy::new(10), None, &MemSandbox(&w)).is_ok());
}

#[test]
fn the_setting_is_fixed_at_genesis_and_a_later_change_is_refused() {
    let c = WTCfg::new(8, 1500);
    let w = WorldT::new(c.clone());
    for created in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let mut log = open_log(d.path());
        let s = run_t_self(&mut log, &rc(&c), Some(true), Some(created), &Policy::new(250), None, &MemSandbox(&w)).unwrap();
        assert!(s.created && s.state.self_tune.on == created);
        let g = rows(&log).into_iter().find(|e| e.tags.contains("wt_genesis")).unwrap();
        assert_eq!(g.tags.contains("wt_self_tune:on"), created);
        assert_eq!(g.text.contains("self-tuned beta on"), created);
        let v = log.latest_version();
        let e = run_t_self(&mut log, &rc(&c), Some(true), Some(!created), &Policy::new(50), None, &MemSandbox(&w)).unwrap_err();
        assert!(e.to_string().contains("wt_self_tune") && e.to_string().contains("genesis"), "{e}");
        assert_eq!(log.latest_version(), v, "a refused run writes nothing");
        let s = run_t_self(&mut log, &rc(&c), Some(true), None, &Policy::new(50), None, &MemSandbox(&w)).unwrap();
        assert!(!s.created && s.state.self_tune.on == created && s.state.step == 300);
        let st = StateT::from_events(&rows(&log));
        assert_eq!(st.self_tune.on, created);
        assert_eq!(st.self_tune.blocks.len(), if created { 2 } else { 0 });
        assert_eq!(st, s.state);
    }
}

#[test]
fn the_controller_key_parses_strictly_and_fails_closed() {
    assert!(Controller::parse("wt_self_tune = on\n").is_ok());
    assert!(Controller::parse("wt_self_tune = off\nwt_file_key = on\n").is_ok());
    for bad in ["wt_self_tune = yes\n", "wt_self_tune = 1\n", "wt_self_tune = maybe\n", "wt_self_tune = on\nwt_self_tune = off\n"] {
        assert!(Controller::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(self_tune::request(None), None);
    assert_eq!(self_tune::request(Some("world_t_run = on\n")), None);
    assert_eq!(self_tune::request(Some("wt_self_tune = on\n")), Some(true));
    assert_eq!(self_tune::request(Some("wt_self_tune = off # owner\n")), Some(false));
    assert_eq!(self_tune::request(Some("wt_self_tune = on\nbogus = 1\n")), None);
}

fn blk(k: u64, beta: u32, reward: i64, cost: i64) -> Block {
    Block { k, beta_milli: beta, mode: "x".into(), reward, cost, steps: 200, outcomes: vec![format!("o{k}")], event_id: format!("d{k}") }
}

#[test]
fn the_block_rule_is_the_preregistered_one() {
    assert_eq!((BLOCK, C, MIN_BLOCKS, GRID), (200, 0.1, 3, [500, 1000, 2000, 4000, 8000, 16000]));
    // the first 6 blocks: each beta once, in grid order starting at 2
    let mut s = SelfTune::on();
    let mut got = vec![];
    for k in 0..6u64 {
        let p = plan(&s, k);
        assert_eq!((p.mode, p.promote), ("forced", None));
        got.push(p.beta_milli);
        s.blocks.push(blk(k, p.beta_milli, 400, 100));
    }
    assert_eq!(got, vec![2000, 4000, 8000, 16000, 500, 1000]);
    // UCB1: realized reward per energy of the arm's blocks (sum / sum) + 0.1 sqrt(2 ln N / n)
    s.blocks[1].reward = 500; // beta 4: 5.0 per energy
    let p = plan(&s, 6);
    assert_eq!((p.mode, p.beta_milli), ("ucb", 4000));
    let bonus = 0.1 * (2.0 * 6f64.ln() / 1.0).sqrt();
    assert!((p.ucb[3] - (5.0 + bonus)).abs() < 1e-12 && (p.ucb[2] - (4.0 + bonus)).abs() < 1e-12);
    // ties go to the lower grid value
    s.blocks[1].reward = 400;
    assert_eq!(plan(&s, 6).beta_milli, 500);
    // promotion: >= 3 blocks and mean above the incumbent (2) by more than one pooled standard error
    let mut s = SelfTune::on();
    for (k, (beta, r)) in [(2000, 400), (2000, 410), (2000, 390), (4000, 430), (4000, 440)].iter().enumerate() {
        s.blocks.push(blk(k as u64, *beta, *r, 100));
    }
    assert_eq!(plan(&s, 9).promote, None, "only 2 blocks with beta 4");
    s.blocks.push(blk(5, 4000, 420, 100));
    let p = plan(&s, 9);
    let (from, to, mt, mf, se) = p.promote.unwrap();
    assert_eq!((from, to), (2000, 4000));
    assert!((mt - 4.3).abs() < 1e-12 && (mf - 4.0).abs() < 1e-12);
    // pooled sd of {4.0,4.1,3.9} and {4.3,4.4,4.2}: both 0.1 -> se = 0.1 sqrt(2/3)
    assert!((se - 0.1 * (2f64 / 3.0).sqrt()).abs() < 1e-12);
    // not above by more than one pooled se: no promotion
    let mut s2 = s.clone();
    for b in s2.blocks.iter_mut().filter(|b| b.beta_milli == 4000) {
        b.reward -= 25; // mean 4.05 vs 4.0, se 0.0816
    }
    assert_eq!(plan(&s2, 9).promote, None);
    // after a promotion at block k: blocks k, k+1, k+2 the incumbent, k+3 UCB1, k+4.. the incumbent again
    s.incumbent_milli = 4000;
    s.promotions.push(Promotion { k: 9, from_milli: 2000, to_milli: 4000, event_id: "p".into() });
    let modes: Vec<&str> = (9..18).map(|k| plan(&s, k).mode).collect();
    assert_eq!(modes, vec!["incumbent", "incumbent", "incumbent", "ucb", "incumbent", "incumbent", "incumbent", "ucb", "incumbent"]);
    assert!((9..18).filter(|k| plan(&s, *k).mode == "incumbent").all(|k| plan(&s, k).beta_milli == 4000));
}

/// Pre-registered test 1: with `wt_self_tune = on` the decisions inside a block equal those of B1 run with that
/// block's beta from the same state; also for the logged being with a fold restart and a checkpoint restart in the
/// middle of a block. (The harness `measure_s1 equiv` runs the same on all tuning seeds 101..120 at budget 6000.)
#[test]
fn inside_a_block_the_decisions_are_b1_with_that_beta_also_across_restarts() {
    let c = WTCfg::new(101, 2400);
    let s = s1_life(&c);
    assert!(s.starts.len() >= 7, "{} blocks", s.starts.len());
    for (k, st) in s.starts.iter().enumerate() {
        let beta = st.self_tune.blocks[k].beta_milli;
        let from = k * BLOCK as usize;
        let to = (from + BLOCK as usize).min(s.plans.len());
        assert_eq!(b1_from(st, beta, BLOCK), s.plans[from..to].to_vec(), "block {k} (beta {beta})");
    }
    // the self-tuning changes decisions (the forced blocks use other betas than 2)
    assert_ne!(b1_from(&s.starts[0], 2000, 100_000), s.plans);
    // the logged being: one run; a fold restart and a checkpoint restart in the middle of block 4
    let mid = 4 * BLOCK + 77;
    for (chunks, every) in [(vec![1_000_000], 0), (vec![mid, 1_000_000], 0), (vec![mid, 1_000_000], 150)] {
        let d = tempfile::tempdir().unwrap();
        let (p, r, e, sums) = logged(d.path(), &c, &chunks, every);
        assert_eq!(p, s.plans, "chunks {chunks:?} every {every}");
        assert_eq!((r, e), (s.reward, s.cost));
        if every > 0 {
            assert!(sums[0].ckpt.written.is_some());
            assert!(sums[1].ckpt.restored.from_version.is_some(), "{:?}", sums[1].ckpt);
        }
        let log = open_log(d.path());
        let rs = rows(&log);
        let st = StateT::from_events(&rs);
        assert_eq!(st, sums.last().unwrap().state, "the fold equals the live state");
        assert_eq!(st.self_tune.blocks.iter().map(|b| (b.beta_milli, b.reward, b.cost, b.steps)).collect::<Vec<_>>(), s.tune.blocks.iter().map(|b| (b.beta_milli, b.reward, b.cost, b.steps)).collect::<Vec<_>>());
        let (n, bad) = replay_t(&rs);
        assert_eq!((n as usize, bad), (s.plans.len(), 0));
        assert_eq!(self_tune::replay(&rs), (s.tune.blocks.len() as u64 + s.tune.promotions.len() as u64, 0));
        if every > 0 {
            let (rst, r) = ckpt::restore::<StateT>(&log).unwrap();
            assert!(r.from_version.is_some() && rst == st);
        }
    }
}

/// Pre-registered test 2: replay from the log alone reproduces every block pick and every promotion (0
/// mismatches), and a changed or missing pick or promotion is a mismatch. The life is long enough to promote.
#[test]
fn replay_from_the_log_reproduces_every_pick_and_promotion() {
    // penalty 10 x reward: the beta matters most there (Track W2)
    let c = WTCfg { penalty_pm: 10_000, ..WTCfg::new(103, 6000) };
    let d = tempfile::tempdir().unwrap();
    let (_, _, _, sums) = logged(d.path(), &c, &[1_000_000], 0);
    let s = &sums[0].state.self_tune;
    let log = open_log(d.path());
    let rs = rows(&log);
    let (n, bad) = self_tune::replay(&rs);
    assert_eq!(bad, 0);
    assert_eq!(n as usize, s.blocks.len() + s.promotions.len());
    assert!(s.blocks.len() >= 10, "{} blocks", s.blocks.len());
    assert_eq!(replay_t(&rs).1, 0);
    eprintln!("blocks {:?}; promotions {:?}", s.blocks.iter().map(|b| b.beta_milli).collect::<Vec<_>>(), s.promotions);
    // the revisions: one decision per block citing outcome events of earlier blocks, one rule per incumbent
    let revs: Vec<RevisionRow> = log.open_latest().rows();
    let dec: Vec<&RevisionRow> = revs.iter().filter(|r| r.entity_type == "decision" && r.key.starts_with("being/worldt/self_tune/block/")).collect();
    assert_eq!(dec.len(), s.blocks.len());
    let rule: Vec<&RevisionRow> = revs.iter().filter(|r| r.entity_type == "rule" && r.key == REV_KEY).collect();
    assert_eq!(rule.len(), 1 + s.promotions.len(), "the initial incumbent and every promotion");
    // time travel: the key's history lists every incumbent in order, each superseding the one before
    let mut want = vec![2000u32];
    want.extend(s.promotions.iter().map(|p| p.to_milli));
    let stances: Vec<String> = rule.iter().map(|r| r.stance.split_whitespace().nth(1).unwrap().to_string()).collect();
    assert_eq!(stances, want.iter().map(|b| format!("{:.1}", *b as f64 / 1000.0)).collect::<Vec<_>>());
    for w in rule.windows(2) {
        assert_eq!(w[1].supersedes.as_deref(), Some(w[0].rev_id.as_str()));
    }
    // a changed pick, a removed pick, a forged promotion are mismatches
    let k = rs.iter().position(|e| e.kind == "decision" && e.tags.contains("st_block:7,")).unwrap();
    let mut t = rs.clone();
    let b = tag(&t[k].tags, "beta_milli").unwrap().to_string();
    let other = GRID.iter().find(|g| g.to_string() != b).unwrap();
    t[k].tags = t[k].tags.replace(&format!("beta_milli:{b}"), &format!("beta_milli:{other}"));
    assert!(self_tune::replay(&t).1 > 0);
    let mut t = rs.clone();
    t.remove(k);
    assert!(self_tune::replay(&t).1 > 0);
    let mut t = rs.clone();
    let mut forged = t[k].clone();
    forged.kind = "note".into();
    forged.tags = "self_tune,st_promote,st_block:7,from_milli:2000,to_milli:16000".into();
    t.insert(k, forged);
    assert!(self_tune::replay(&t).1 > 0);
    if let Some(p) = s.promotions.first() {
        let mut t = rs.clone();
        t.retain(|e| e.event_id != p.event_id);
        assert!(self_tune::replay(&t).1 > 0, "a missing promotion");
    }
}

/// Pre-registered test 3: KILL in the middle of a block: no step runs after it, and the self-tune state folds
/// correctly afterwards (and the being continues exactly as without the kill).
#[test]
fn kill_in_the_middle_of_a_block_stops_and_the_state_folds() {
    let c = WTCfg::new(104, 2000);
    let w = WorldT::new(c.clone());
    let full = s1_life(&c);
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let calls = Arc::new(AtomicU64::new(0));
    let k = calls.clone();
    let kill_at = 3 * BLOCK + 111;
    let mut p = Policy::new(1_000_000);
    p.checkpoint_every = 100;
    p.kill = Some(KillCheck(Arc::new(move || k.fetch_add(1, Ordering::SeqCst) >= kill_at)));
    let s = run_t_self(&mut log, &rc(&c), Some(true), Some(true), &p, None, &MemSandbox(&w)).unwrap();
    assert_eq!(s.stopped, StopReason::Killed);
    assert_eq!(s.steps_run, kill_at);
    let rs = rows(&log);
    let outcomes: Vec<u64> = rs.iter().filter(|e| e.source == SOURCE_T && e.kind == "outcome").map(|e| tag(&e.tags, "step").unwrap().parse().unwrap()).collect();
    assert_eq!(outcomes.len() as u64, kill_at);
    assert_eq!(*outcomes.last().unwrap(), kill_at - 1);
    let stop = rs.iter().rposition(|e| e.source == SOURCE_T && e.tags.contains("controller_stop")).unwrap();
    assert!(rs[stop..].iter().all(|e| e.kind != "outcome" && e.kind != "action"), "no step after the kill");
    let st = StateT::from_events(&rs);
    assert_eq!(st, s.state);
    assert_eq!(st.self_tune.blocks.len(), 4);
    assert_eq!(st.self_tune.blocks[3].steps as u64, 111);
    assert_eq!(self_tune::replay(&rs).1, 0);
    let (rst, _) = ckpt::restore::<StateT>(&log).unwrap();
    assert!(rst == st);
    // the next run continues block 3 (no new pick) and ends where the uninterrupted life ends
    let s2 = run_t_self(&mut log, &rc(&c), None, None, &Policy::new(1_000_000), None, &MemSandbox(&w)).unwrap();
    let mut plans: Vec<Option<Plan>> = s.truths.iter().map(|t| t.plan).collect();
    plans.extend(s2.truths.iter().map(|t| t.plan));
    assert_eq!(plans, full.plans);
    assert_eq!((s.reward + s2.reward, s.cost + s2.cost), (full.reward, full.cost));
    let rs = rows(&log);
    assert_eq!(rs.iter().filter(|e| e.kind == "decision" && e.tags.contains("st_block:3,")).count(), 1);
    assert_eq!(self_tune::replay(&rs).1, 0);
    assert_eq!(StateT::from_events(&rs).self_tune, full.tune);
}

#[test]
fn a_format_2_world_t_checkpoint_is_refused_and_the_log_is_folded() {
    assert_eq!(<StateT as Foldable>::FORMAT, 3);
    let c = WTCfg::new(12, 1500);
    let w = WorldT::new(c.clone());
    let d = tempfile::tempdir().unwrap();
    {
        let mut log = open_log(d.path());
        let mut p = Policy::new(330);
        p.checkpoint_every = 100;
        let s = run_t_self(&mut log, &rc(&c), Some(true), Some(true), &p, None, &MemSandbox(&w)).unwrap();
        assert!(s.ckpt.written.is_some());
    }
    let mut log = open_log(d.path());
    let full: StateT = ckpt::full_fold(&log).unwrap();
    assert!(full.self_tune.on && full.self_tune.blocks.len() == 2);
    let files = log.checkpoint_files("world_t");
    assert_eq!(files.len(), 1);
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert!(r.from_version.is_some() && st == full);
    // a format-2 file (what Track W2 wrote), anchored like a real one: never used, the whole log is folded
    std::fs::remove_file(d.path().join("being").join("log").join(&files[0].1)).unwrap();
    let payload = crate::being::binser::to_bytes(&full).unwrap();
    let old = log.write_checkpoint_fmt("world_t", full.step, payload, 2).unwrap();
    log.append_events(&[ckpt::anchor_event(&old, full.clock)]).unwrap();
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert_eq!(r.from_version, None);
    assert!(r.rejected.iter().any(|e| e.contains("unknown snapshot format version 2")), "{:?}", r.rejected);
    assert!(st == ckpt::full_fold::<StateT>(&log).unwrap());
    // the next run continues exactly as from the full fold and writes a format-3 file that is used
    let mut p = Policy::new(120);
    p.checkpoint_every = 100;
    let s = run_t_self(&mut log, &rc(&c), None, None, &p, None, &MemSandbox(&w)).unwrap();
    assert_eq!(s.ckpt.restored.from_version, None);
    assert!(s.ckpt.written.is_some());
    let log = open_log(d.path());
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert!(r.from_version.is_some() && st == ckpt::full_fold::<StateT>(&log).unwrap());
    assert_eq!(st.self_tune.blocks.len(), 3);
    assert_eq!(self_tune::replay(&rows(&log)).1, 0);
}
