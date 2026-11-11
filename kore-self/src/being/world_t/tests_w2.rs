//! Track W2 tests: the file-keyed estimate in the being (`wt_file_key`). Off: the being of before (also pinned by
//! `tests_golden.rs`); on: exactly the Gate 0 harness B1, also across a restart from the fold and from a
//! checkpoint; the setting is recorded at genesis and a later run cannot change it; the World T checkpoint has
//! a new snapshot format version and an old-version file is not used; the controller key fails closed.

use super::ctl;
use super::engine::*;
use super::learner::*;
use super::measure::{run_life, Pol};
use super::world::*;
use crate::being::ckpt::{self, Foldable};
use crate::being::controller::Controller;
use crate::being::policy::Policy;
use crate::versioned::{EventRow, VersionedLog};
use std::collections::HashMap;

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn rc(c: &WTCfg) -> RunTCfg {
    RunTCfg { world: Some(c.clone()), learner: None, discover_every: DEFAULT_DISCOVER_EVERY, ctl: None, sql: None }
}

fn b1() -> LearnerTCfg {
    LearnerTCfg { file_key: true, ..Default::default() }
}

fn rows(log: &VersionedLog) -> Vec<EventRow> {
    log.open_latest().rows()
}

/// The Gate 0 harness B1 (`FileKeyed` of examples/measure_n1diag.rs at d34657b7, same code).
struct FileKeyed {
    l: LearnerT,
    files: HashMap<(usize, String, usize), Cell>,
    pending: Option<((usize, String, usize), (u8, u8, u8), Cell)>,
}

impl ChooserT for FileKeyed {
    fn name(&self) -> String {
        "b1-file-keyed (measurement only)".into()
    }
    fn choose(&mut self, st: &StateT, v: &ViewT) -> ChoiceT {
        if let Some((fk, ck, before)) = self.pending.take() {
            let after = st.cells.get(&ck).copied().unwrap_or_default();
            if after.n > before.n {
                let e = self.files.entry(fk).or_default();
                e.n += after.n - before.n;
                e.succ += after.succ - before.succ;
                e.wrong += after.wrong - before.wrong;
                e.spent += after.spent - before.spent;
            }
        }
        let (l, files) = (&self.l, &self.files);
        let ch = l.choose_with(st, v, |p| {
            let ce = l.estimate(st, v, p);
            let fc = files.get(&(v.tmpl.idx(), v.file.clone(), p.idx())).copied().unwrap_or_default();
            l.estimate_from(v, p, fc, (ce.p, ce.p_wrong, "cell estimate"), ce.energy)
        });
        if let Some(p) = ch.plan {
            let ck = (v.tmpl.idx() as u8, v.ctx.idx() as u8, p.idx() as u8);
            self.pending = Some(((v.tmpl.idx(), v.file.clone(), p.idx()), ck, st.cells.get(&ck).copied().unwrap_or_default()));
        }
        ch
    }
}

/// The Gate 0 measurement loop with a chooser: the plan of every step, reward, energy, file keys at the end.
fn life(c: &WTCfg, ch: &mut dyn ChooserT, l: LearnerTCfg) -> (Vec<Option<Plan>>, i64, i64, usize) {
    let mut st = StateT::default();
    st.start(c.clone(), l, "genesis".into());
    let mut eng = EngineT::new(st, None);
    eng.measure = true;
    let w = WorldT::new(c.clone());
    let (mut plans, mut r, mut e) = (vec![], 0, 0);
    while let Ok(s) = eng.step(ch, &MemSandbox(&w)) {
        plans.push(s.truth.plan);
        r += s.truth.reward;
        e += s.truth.cost;
        if eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    (plans, r, e, eng.state.file_keys())
}

/// The logged being with the key on, in chunks of steps with a cold restart between them.
fn logged(root: &std::path::Path, c: &WTCfg, chunks: &[u64], every: u64) -> (Vec<Option<Plan>>, i64, i64, Vec<RunTSummary>) {
    let w = WorldT::new(c.clone());
    let (mut plans, mut r, mut e, mut sums) = (vec![], 0, 0, vec![]);
    for &n in chunks {
        let mut log = open_log(root);
        let mut p = Policy::new(n);
        p.checkpoint_every = every;
        let s = run_t_key(&mut log, &rc(c), Some(true), &p, None, &MemSandbox(&w)).unwrap();
        plans.extend(s.truths.iter().map(|t| t.plan));
        r += s.reward;
        e += s.cost;
        sums.push(s);
    }
    (plans, r, e, sums)
}

#[test]
fn file_key_off_is_the_being_of_before_and_writes_no_file_counts() {
    // the default is off; its genesis tags and text are those written before Track W2
    assert!(!LearnerTCfg::default().file_key);
    let tags = LearnerTCfg::default().tags();
    assert!(tags.iter().all(|t| !t.starts_with("wt_file_key")), "{tags:?}");
    let (ev, _) = genesis_event_t(&WTCfg::new(1, 600), &LearnerTCfg::default());
    assert!(!ev.text.contains("file-keyed"));
    // a log written with it off: no outcome carries the file, the fold has no file counts, decisions replay
    let c = WTCfg::new(11, 1500);
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    run_t(&mut log, &rc(&c), &Policy::new(5000), None, &MemSandbox(&WorldT::new(c.clone()))).unwrap();
    let rs = rows(&log);
    assert!(rs.iter().filter(|e| e.kind == "outcome" && e.source == SOURCE_T).all(|e| crate::being::state::tag(&e.tags, "file").is_none()));
    let st = StateT::from_events(&rs);
    assert!(!st.learner.file_key && st.files.is_empty());
    assert_eq!(replay_t(&rs).1, 0);
    // the measurement life with the key off equals the explicit default
    let a = run_life(&WTCfg::new(101, 1500), Pol::Being(LearnerTCfg::default()), None);
    let b = run_life(&WTCfg::new(101, 1500), Pol::Being(LearnerTCfg { file_key: false, ..b1() }), None);
    assert_eq!((a.reward, a.cost), (b.reward, b.cost));
}

#[test]
fn the_tags_round_trip_and_a_bad_value_is_not_a_genesis() {
    for l in [LearnerTCfg::default(), b1()] {
        assert_eq!(LearnerTCfg::from_tags(&l.tags().join(",")), Some(l));
    }
    let mut t = LearnerTCfg::default().tags();
    t.push("wt_file_key:maybe".into());
    assert_eq!(LearnerTCfg::from_tags(&t.join(",")), None);
    let (ev, _) = genesis_event_t(&WTCfg::new(1, 600), &b1());
    assert!(ev.tags.iter().any(|t| t == "wt_file_key:on") && ev.text.contains("file-keyed estimate on"));
}

#[test]
fn file_key_on_reproduces_the_gate0_harness_also_across_a_restart_from_the_fold_and_from_a_checkpoint() {
    for seed in [101u64, 102] {
        let c = WTCfg::new(seed, 2000);
        let g = { let l = LearnerTCfg::default(); life(&c, &mut FileKeyed { l: LearnerT { cfg: l }, files: HashMap::new(), pending: None }, l) };
        let i = life(&c, &mut LearnerT { cfg: b1() }, b1());
        assert_eq!(i.0, g.0, "seed {seed}: in-being decisions");
        assert_eq!((i.1, i.2), (g.1, g.2));
        assert!(i.3 > 50, "file keys {}", i.3);
        let b0 = life(&c, &mut LearnerT { cfg: LearnerTCfg::default() }, LearnerTCfg::default());
        assert_ne!(b0.0, g.0, "the key changes decisions");
        let mid = (g.0.len() as u64 / 2).max(150);
        for (chunks, every) in [(vec![1_000_000], 0), (vec![mid, 1_000_000], 0), (vec![mid, 1_000_000], 100)] {
            let d = tempfile::tempdir().unwrap();
            let (p, r, e, sums) = logged(d.path(), &c, &chunks, every);
            assert_eq!(p, g.0, "seed {seed} chunks {chunks:?} every {every}");
            assert_eq!((r, e), (g.1, g.2));
            if every > 0 {
                assert!(sums[0].ckpt.written.is_some());
                assert!(sums[1].ckpt.restored.from_version.is_some(), "{:?}", sums[1].ckpt);
            }
            let log = open_log(d.path());
            let rs = rows(&log);
            let st = StateT::from_events(&rs);
            assert!(st.learner.file_key);
            assert_eq!(st.file_keys(), i.3);
            let (n, bad) = replay_t(&rs);
            assert_eq!((n as usize, bad), (g.0.len(), 0));
            // the fold equals the live state of the last run, the checkpoint restore equals the full fold
            assert_eq!(st, sums.last().unwrap().state);
            if every > 0 {
                let (rst, r) = ckpt::restore::<StateT>(&log).unwrap();
                assert!(r.from_version.is_some() && rst == st);
            }
        }
    }
}

#[test]
fn the_setting_is_fixed_at_genesis_and_a_later_run_cannot_change_it() {
    let c = WTCfg::new(7, 1200);
    let w = WorldT::new(c.clone());
    for created in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let mut log = open_log(d.path());
        let s = run_t_key(&mut log, &rc(&c), Some(created), &Policy::new(50), None, &MemSandbox(&w)).unwrap();
        assert!(s.created && s.state.learner.file_key == created);
        let g = rows(&log).into_iter().find(|e| e.tags.contains("wt_genesis")).unwrap();
        assert_eq!(g.tags.contains("wt_file_key:on"), created);
        let v = log.latest_version();
        // the other setting is refused, nothing is written
        let e = run_t_key(&mut log, &rc(&c), Some(!created), &Policy::new(50), None, &MemSandbox(&w)).unwrap_err();
        assert!(e.to_string().contains("wt_file_key"), "{e}");
        assert_eq!(log.latest_version(), v);
        // also when the request comes through the learner constants of a later run: the genesis setting stays
        let later = RunTCfg { learner: Some(LearnerTCfg { file_key: !created, ..Default::default() }), ..rc(&c) };
        let s = run_t_key(&mut log, &later, None, &Policy::new(20), None, &MemSandbox(&w)).unwrap();
        assert!(!s.created && s.state.learner.file_key == created);
        // the same setting, or no request: the being continues with its genesis setting
        let s = run_t_key(&mut log, &rc(&c), Some(created), &Policy::new(20), None, &MemSandbox(&w)).unwrap();
        assert_eq!(s.state.learner.file_key, created);
        let s = run_t(&mut log, &RunTCfg { world: None, ..rc(&c) }, &Policy::new(20), None, &MemSandbox(&w)).unwrap();
        assert_eq!(s.state.learner.file_key, created);
        assert_eq!(s.state.step, 110);
        let rs = rows(&log);
        assert_eq!(StateT::from_events(&rs).learner.file_key, created);
        assert!(rs.iter().filter(|e| e.kind == "outcome" && e.source == SOURCE_T).all(|e| crate::being::state::tag(&e.tags, "file").is_some() == created));
        assert_eq!(replay_t(&rs).1, 0);
    }
}

#[test]
fn an_old_format_world_t_checkpoint_is_not_used_and_the_log_is_folded() {
    assert_eq!(<StateT as Foldable>::FORMAT, 2);
    assert_eq!(<crate::being::state::BeingState as Foldable>::FORMAT, crate::versioned::CKPT_FORMAT);
    let c = WTCfg::new(12, 1500);
    let w = WorldT::new(c.clone());
    let d = tempfile::tempdir().unwrap();
    {
        let mut log = open_log(d.path());
        let mut p = Policy::new(150);
        p.checkpoint_every = 100;
        let s = run_t_key(&mut log, &rc(&c), Some(true), &p, None, &MemSandbox(&w)).unwrap();
        assert!(s.ckpt.written.is_some());
    }
    let mut log = open_log(d.path());
    let full: StateT = ckpt::full_fold(&log).unwrap();
    let files = log.checkpoint_files("world_t");
    assert_eq!(files.len(), 1);
    // the new file is format 2: the format-1 reader refuses it, the World T reader uses it
    assert!(log.read_checkpoint("world_t", &files[0].1).unwrap_err().contains("unknown snapshot format version 2"));
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert!(r.from_version.is_some() && st == full);
    // a checkpoint written in the old format 1 (what the code before Track W2 wrote), anchored like a real one:
    // never used, never misread; the whole log is folded
    std::fs::remove_file(d.path().join("being").join("log").join(&files[0].1)).unwrap();
    let payload = crate::being::binser::to_bytes(&full).unwrap();
    let old = log.write_checkpoint("world_t", full.step, payload).unwrap();
    log.append_events(&[ckpt::anchor_event(&old, full.clock)]).unwrap();
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert_eq!(r.from_version, None);
    assert!(r.rejected.iter().any(|e| e.contains("unknown snapshot format version 1")), "{:?}", r.rejected);
    assert!(st == ckpt::full_fold::<StateT>(&log).unwrap());
    // the next run with checkpoints on continues exactly as from the full fold and writes a format-2 file
    let mut p = Policy::new(120);
    p.checkpoint_every = 100;
    let s = run_t_key(&mut log, &rc(&c), None, &p, None, &MemSandbox(&w)).unwrap();
    assert_eq!(s.ckpt.restored.from_version, None);
    assert!(s.ckpt.written.is_some());
    let log = open_log(d.path());
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert!(r.from_version.is_some() && st == ckpt::full_fold::<StateT>(&log).unwrap());
    for line in ckpt::audit::<StateT>(&log).unwrap() {
        if line.version == Some(s.ckpt.written.unwrap()) {
            assert_eq!(line.equal_to_full_fold, Some(true));
        }
    }
}

#[test]
fn the_controller_key_parses_and_fails_closed() {
    assert!(Controller::parse("wt_file_key = on\n").is_ok());
    assert!(Controller::parse("wt_file_key = off\nworld_t_run = on\n").is_ok());
    assert!(Controller::parse("wt_file_key = maybe\n").is_err());
    assert!(Controller::parse("wt_file_key = on\nwt_file_key = off\n").is_err());
    assert_eq!(ctl::file_key(None), None);
    assert_eq!(ctl::file_key(Some("world_t_run = on\n")), None);
    assert_eq!(ctl::file_key(Some("wt_file_key = on\n")), Some(true));
    assert_eq!(ctl::file_key(Some("wt_file_key = off # owner\n")), Some(false));
    // an invalid file: no request (and the run itself is refused by `allowed`)
    assert_eq!(ctl::file_key(Some("wt_file_key = on\nbogus = 1\n")), None);
    assert!(ctl::allowed(Some("world_t_run = on\nwt_file_key = yes please\n")).is_err());
}
