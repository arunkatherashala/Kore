//! Track W2b tests (docs/w2b_prereg.md): the beta table of the file-keyed estimate (`wt_file_beta = table`, B1p).
//! Key absent: the being of before (both golden tests in `tests_golden.rs` also pin it) and the checkpoint payload
//! layout of the learner constants unchanged (World T snapshot format 3 since Track S1; W2b adds no field); on: exactly B1 with the table's beta for the task's reward design,
//! also across a restart from the fold and from a checkpoint; the setting is recorded at genesis with the whole table
//! and a later run cannot change it; the controller key fails closed and is read only with `wt_file_key = on`.

use super::ctl;
use super::engine::*;
use super::learner::*;
use super::measure::{run_life, Pol};
use super::world::*;
use crate::being::ckpt::{self, Foldable};
use crate::being::controller::Controller;
use crate::being::policy::Policy;
use crate::versioned::{EventRow, VersionedLog};

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn rc(c: &WTCfg) -> RunTCfg {
    RunTCfg { world: Some(c.clone()), learner: None, discover_every: DEFAULT_DISCOVER_EVERY, ctl: None, sql: None }
}

fn b1() -> LearnerTCfg {
    LearnerTCfg { file_key: true, ..Default::default() }
}

fn b1p() -> LearnerTCfg {
    LearnerTCfg { file_key: true, file_beta_table: true, ..Default::default() }
}

fn rows(log: &VersionedLog) -> Vec<EventRow> {
    log.open_latest().rows()
}

fn wc(seed: u64, budget: i64, p: u32, c: u32) -> WTCfg {
    WTCfg { penalty_pm: p, cost_pm: c, ..WTCfg::new(seed, budget) }
}

/// The learner constants as the code of Track W2 serialized them (field for field).
#[derive(serde::Serialize)]
struct W2Cfg {
    beta_milli: u32,
    prior_w_milli: u32,
    pooled: bool,
    skip: bool,
    value_ucb: bool,
    file_key: bool,
}

fn w2_bytes(l: &LearnerTCfg) -> Vec<u8> {
    crate::being::binser::to_bytes(&W2Cfg { beta_milli: l.beta_milli, prior_w_milli: l.prior_w_milli, pooled: l.pooled, skip: l.skip, value_ucb: l.value_ucb, file_key: l.file_key }).unwrap()
}

#[test]
fn key_absent_is_the_being_of_before_and_the_checkpoint_layout_is_unchanged() {
    assert!(!LearnerTCfg::default().file_beta_table);
    // format 3 after the merge with Track S1 (its self-tune state); W2b itself adds no field
    assert_eq!(<StateT as Foldable>::FORMAT, 3);
    for l in [LearnerTCfg::default(), b1(), LearnerTCfg { beta_milli: 500, ..b1() }] {
        // tags and genesis text as before Track W2b
        assert!(l.tags().iter().all(|t| !t.starts_with("wt_file_beta")), "{:?}", l.tags());
        assert!(!genesis_event_t(&WTCfg::new(1, 600), &l).0.text.contains("W2b"));
        // the checkpoint bytes of the constants are the ones of Track W2, and they read back
        let b = crate::being::binser::to_bytes(&l).unwrap();
        assert_eq!(b, w2_bytes(&l));
        assert_eq!(crate::being::binser::from_bytes::<LearnerTCfg>(&b).unwrap(), l);
        assert_eq!(l.for_view(&WorldT::new(wc(1, 600, 10_000, 100)).task(0, 0).view), l);
    }
    // the table switch: same length (no new field), round trip exact, only the top bit of beta_milli differs
    let b = crate::being::binser::to_bytes(&b1p()).unwrap();
    let old = w2_bytes(&b1());
    assert_eq!(b.len(), old.len());
    assert_ne!(b, old);
    assert_eq!(crate::being::binser::from_bytes::<LearnerTCfg>(&b).unwrap(), b1p());
    // without the file key the switch does nothing: no tag, the default being's decisions
    let lone = LearnerTCfg { file_beta_table: true, ..Default::default() };
    assert!(lone.tags().iter().all(|t| !t.starts_with("wt_file_beta")));
    let a = run_life(&wc(101, 1500, 3162, 1000), Pol::Being(LearnerTCfg::default()), None);
    let z = run_life(&wc(101, 1500, 3162, 1000), Pol::Being(lone), None);
    assert_eq!(a.truths, z.truths);
    // a log written without the key: no tag, fold off, decisions replay
    let c = WTCfg::new(11, 1500);
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    run_t_key(&mut log, &rc(&c), Some(true), &Policy::new(5000), None, &MemSandbox(&WorldT::new(c.clone()))).unwrap();
    let rs = rows(&log);
    assert!(rs.iter().all(|e| !e.tags.contains("wt_file_beta")));
    let st = StateT::from_events(&rs);
    assert!(st.learner.file_key && !st.learner.file_beta_table);
    assert_eq!(replay_t(&rs).1, 0);
}

#[test]
fn the_table_covers_the_sweep_and_the_lookup_is_nearest_in_log_space() {
    let pen = [0u32, 100, 316, 1000, 3162, 10000];
    let cost = [100u32, 316, 1000, 3162, 10000];
    let grid: Vec<(u32, u32)> = pen.iter().flat_map(|p| cost.iter().map(move |c| (*p, *c))).collect();
    assert_eq!(W2B_BETA_TABLE.iter().map(|(p, c, _)| (*p, *c)).collect::<Vec<_>>(), grid);
    assert!(W2B_BETA_TABLE.iter().all(|(_, _, b)| [500, 1000, 2000, 4000, 8000, 16000].contains(b)));
    for (p, c, b) in W2B_BETA_TABLE {
        assert_eq!(table_beta(p, c), b);
    }
    let t = [(0, 1000, 1), (100, 1000, 2), (1000, 1000, 3), (10000, 1000, 4), (10000, 100, 5)];
    // a penalty of 0 is only near a penalty of 0; any positive penalty is never near 0
    assert_eq!(nearest_beta(&t, 0, 50_000), 1);
    assert_eq!(nearest_beta(&t, 1, 1000), 2);
    // nearest in log space, not in plain distance: 300 is nearer 100 than 1000 in log space (and nearer 1000 in plain)
    assert_eq!(nearest_beta(&t, 300, 1000), 2);
    assert_eq!(nearest_beta(&t, 400, 1000), 3);
    assert_eq!(nearest_beta(&t, 20_000, 1000), 4);
    assert_eq!(nearest_beta(&t, 10_000, 200), 5);
    // ties go to the higher penalty: 1000 is exactly between 100 and 10000 in log space
    let tie = [(100, 1000, 7), (10000, 1000, 9)];
    assert_eq!(nearest_beta(&tie, 1000, 1000), 9);
    let tie_c = [(1000, 100, 7), (1000, 10000, 9)];
    assert_eq!(nearest_beta(&tie_c, 1000, 1000), 9);
    assert_eq!(nearest_beta(&[], 1000, 1000), 2000);
}

#[test]
fn the_tags_round_trip_and_a_bad_or_other_table_is_not_a_genesis() {
    for l in [LearnerTCfg::default(), b1(), b1p()] {
        assert_eq!(LearnerTCfg::from_tags(&l.tags().join(",")), Some(l));
    }
    let t = b1p().tags();
    assert!(t.iter().any(|x| x == "wt_file_beta:table") && t.iter().any(|x| *x == format!("wt_file_beta_table:{}", table_tag())));
    let (ev, _) = genesis_event_t(&WTCfg::new(1, 600), &b1p());
    assert!(ev.text.contains("Track W2b table") && ev.tags.iter().any(|x| x == "wt_file_beta:table"));
    let bad = |f: &dyn Fn(&mut Vec<String>)| {
        let mut t = b1p().tags();
        f(&mut t);
        LearnerTCfg::from_tags(&t.join(","))
    };
    assert_eq!(bad(&|t| t.retain(|x| x != "wt_file_beta:table")), Some(b1()));
    assert_eq!(bad(&|t| t.iter_mut().for_each(|x| if x == "wt_file_beta:table" { *x = "wt_file_beta:maybe".into() })), None);
    assert_eq!(bad(&|t| t.iter_mut().for_each(|x| if x == "wt_file_beta:table" { *x = "wt_file_beta:fixed".into() })), Some(b1()));
    // another table than the code's is refused, and so is a missing one, and the table without the file key
    let other = W2B_BETA_TABLE.iter().enumerate().map(|(i, (p, c, b))| format!("{p}/{c}/{}", if i == 0 && *b == 500 { 1000 } else if i == 0 { 500 } else { *b })).collect::<Vec<_>>().join(";");
    assert_ne!(other, table_tag());
    assert_eq!(bad(&|t| t.iter_mut().for_each(|x| if x.starts_with("wt_file_beta_table:") { *x = format!("wt_file_beta_table:{other}") })), None);
    assert_eq!(bad(&|t| t.retain(|x| !x.starts_with("wt_file_beta_table:"))), None);
    assert_eq!(bad(&|t| t.retain(|x| x != "wt_file_key:on")), None);
}

/// The logged being B1p in chunks of steps with a cold restart between them.
fn logged(root: &std::path::Path, c: &WTCfg, chunks: &[u64], every: u64) -> (Vec<TrueT>, Vec<RunTSummary>) {
    let w = WorldT::new(c.clone());
    let (mut t, mut sums) = (vec![], vec![]);
    for &n in chunks {
        let mut log = open_log(root);
        let mut p = Policy::new(n);
        p.checkpoint_every = every;
        let s = run_t_keys(&mut log, &rc(c), Some(true), Some(true), &p, None, &MemSandbox(&w)).unwrap();
        t.extend(s.truths.clone());
        sums.push(s);
    }
    (t, sums)
}

#[test]
fn b1p_is_b1_with_the_tables_beta_also_across_a_restart_from_the_fold_and_from_a_checkpoint() {
    for (seed, p, c) in [(101u64, 0u32, 1000u32), (102, 10_000, 1000), (103, 3162, 316), (104, 100, 10_000)] {
        let cfg = wc(seed, 1500, p, c);
        let bt = table_beta(p, c);
        let fixed = run_life(&cfg, Pol::Being(LearnerTCfg { beta_milli: bt, ..b1() }), None);
        let tab = run_life(&cfg, Pol::Being(b1p()), None);
        assert_eq!(tab.truths, fixed.truths, "seed {seed} ({p}, {c}) beta {bt}");
        let mid = (fixed.truths.len() as u64 / 2).max(100);
        for (chunks, every) in [(vec![1_000_000], 0), (vec![mid, 1_000_000], 0), (vec![mid, 1_000_000], 100)] {
            let d = tempfile::tempdir().unwrap();
            let (t, sums) = logged(d.path(), &cfg, &chunks, every);
            assert_eq!(t.iter().map(|x| x.plan).collect::<Vec<_>>(), fixed.truths.iter().map(|x| x.plan).collect::<Vec<_>>(), "chunks {chunks:?} every {every}");
            assert_eq!((t.iter().map(|x| x.reward).sum::<i64>(), t.iter().map(|x| x.cost).sum::<i64>()), (fixed.reward, fixed.cost));
            let log = open_log(d.path());
            let rs = rows(&log);
            let st = StateT::from_events(&rs);
            assert!(st.learner.file_key && st.learner.file_beta_table && st.learner.beta_milli == 2000);
            assert_eq!(replay_t(&rs), (fixed.truths.len() as u64, 0));
            assert_eq!(st, sums.last().unwrap().state);
            if every > 0 {
                assert!(sums[1].ckpt.restored.from_version.is_some(), "{:?}", sums[1].ckpt);
                // the checkpoint is format 3 (the format-1 reader refuses it), restores to the full fold, audits equal
                let files = log.checkpoint_files("world_t");
                assert!(log.read_checkpoint("world_t", &files[0].1).unwrap_err().contains("unknown snapshot format version 3"));
                let (rst, r) = ckpt::restore::<StateT>(&log).unwrap();
                assert!(r.from_version.is_some() && rst == st);
                assert!(ckpt::audit::<StateT>(&log).unwrap().iter().all(|l| l.equal_to_full_fold != Some(false)));
            }
        }
    }
}

#[test]
fn the_setting_is_fixed_at_genesis_and_a_later_run_cannot_change_it() {
    let c = wc(7, 1200, 1000, 1000);
    let w = WorldT::new(c.clone());
    for table in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let mut log = open_log(d.path());
        let s = run_t_keys(&mut log, &rc(&c), Some(true), Some(table), &Policy::new(50), None, &MemSandbox(&w)).unwrap();
        assert!(s.created && s.state.learner.file_key && s.state.learner.file_beta_table == table);
        let g = rows(&log).into_iter().find(|e| e.tags.contains("wt_genesis")).unwrap();
        assert_eq!(g.tags.contains("wt_file_beta:table"), table);
        let v = log.latest_version();
        let e = run_t_keys(&mut log, &rc(&c), Some(true), Some(!table), &Policy::new(50), None, &MemSandbox(&w)).unwrap_err();
        assert!(e.to_string().contains("wt_file_beta") && e.to_string().contains("genesis"), "{e}");
        assert_eq!(log.latest_version(), v);
        // the learner constants of a later run are ignored; no request continues with the genesis setting
        let later = RunTCfg { learner: Some(LearnerTCfg { file_beta_table: !table, ..b1() }), ..rc(&c) };
        let s = run_t_keys(&mut log, &later, None, None, &Policy::new(20), None, &MemSandbox(&w)).unwrap();
        assert!(!s.created && s.state.learner.file_beta_table == table);
        let s = run_t_keys(&mut log, &rc(&c), Some(true), Some(table), &Policy::new(20), None, &MemSandbox(&w)).unwrap();
        assert_eq!((s.state.learner.file_beta_table, s.state.step), (table, 90));
        let rs = rows(&log);
        assert_eq!(StateT::from_events(&rs).learner.file_beta_table, table);
        assert_eq!(replay_t(&rs).1, 0);
    }
    // the table only with the file key: a being created with the key off has it off whatever is asked
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let s = run_t_keys(&mut log, &rc(&c), Some(false), Some(true), &Policy::new(20), None, &MemSandbox(&w)).unwrap();
    assert!(!s.state.learner.file_key && !s.state.learner.file_beta_table);
    assert!(rows(&log).iter().all(|e| !e.tags.contains("wt_file_beta")));
}

#[test]
fn the_controller_key_parses_fails_closed_and_is_read_only_with_the_file_key() {
    assert!(Controller::parse("wt_file_beta = table\n").is_ok());
    assert!(Controller::parse("wt_file_key = on\nwt_file_beta = fixed\nworld_t_run = on\n").is_ok());
    assert!(Controller::parse("wt_file_beta = maybe\n").is_err());
    assert!(Controller::parse("wt_file_beta = on\n").is_err());
    assert!(Controller::parse("wt_file_beta = table\nwt_file_beta = fixed\n").is_err());
    assert_eq!(ctl::file_beta(None), None);
    assert_eq!(ctl::file_beta(Some("wt_file_beta = table\n")), None);
    assert_eq!(ctl::file_beta(Some("wt_file_key = off\nwt_file_beta = table\n")), None);
    assert_eq!(ctl::file_beta(Some("wt_file_key = on\n")), None);
    assert_eq!(ctl::file_beta(Some("wt_file_key = on\nwt_file_beta = table # owner\n")), Some(true));
    assert_eq!(ctl::file_beta(Some("wt_file_key = on\nwt_file_beta = Fixed\n")), Some(false));
    assert_eq!(ctl::file_beta(Some("wt_file_key = on\nwt_file_beta = table\nbogus = 1\n")), None);
    assert!(ctl::allowed(Some("world_t_run = on\nwt_file_key = on\nwt_file_beta = tuned\n")).is_err());
}

// ---- after the merge with Track S1 (World T snapshot format 3) ----

/// Owner rule (additivity): a format-2 checkpoint as Track W2b wrote it (its table bit in the top bit of
/// `beta_milli`, no self-tune state) is refused cleanly by the format-3 code, the whole log is folded to the same
/// state, and the next run continues from that fold and writes a format-3 file that is used.
#[test]
fn a_format_2_checkpoint_of_a_table_being_is_refused_and_the_log_is_folded_to_the_same_state() {
    assert_eq!(<StateT as Foldable>::FORMAT, 3);
    let c = wc(13, 1500, 10_000, 1000);
    let w = WorldT::new(c.clone());
    let d = tempfile::tempdir().unwrap();
    {
        let mut log = open_log(d.path());
        let mut p = Policy::new(330);
        p.checkpoint_every = 100;
        let s = run_t_keys(&mut log, &rc(&c), Some(true), Some(true), &p, None, &MemSandbox(&w)).unwrap();
        assert!(s.ckpt.written.is_some() && s.state.learner.file_beta_table);
    }
    let mut log = open_log(d.path());
    let full: StateT = ckpt::full_fold(&log).unwrap();
    assert!(full.learner.file_beta_table && !full.self_tune.on && full.step == 330);
    let files = log.checkpoint_files("world_t");
    assert_eq!(files.len(), 1);
    // a format-3 file carries the table bit: restored equal to the full fold
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert!(r.from_version.is_some() && st == full && st.learner.file_beta_table);
    // the W2b-era payload: the format-3 payload without its last field (the self-tune state, positional binser)
    let p3 = crate::being::binser::to_bytes(&full).unwrap();
    let tail = crate::being::binser::to_bytes(&full.self_tune).unwrap();
    assert!(p3.ends_with(&tail));
    let p2 = p3[..p3.len() - tail.len()].to_vec();
    assert!(crate::being::binser::from_bytes::<StateT>(&p2).is_err(), "a format-2 payload must not decode as format 3");
    std::fs::remove_file(d.path().join("being").join("log").join(&files[0].1)).unwrap();
    let old = log.write_checkpoint_fmt("world_t", full.step, p2, 2).unwrap();
    log.append_events(&[ckpt::anchor_event(&old, full.clock)]).unwrap();
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert_eq!(r.from_version, None);
    assert!(r.rejected.iter().any(|e| e.contains("unknown snapshot format version 2")), "{:?}", r.rejected);
    let fold = ckpt::full_fold::<StateT>(&log).unwrap();
    assert!(st == fold && fold.learner == full.learner && fold.step == full.step && fold.files == full.files);
    // the next run continues exactly as a being with checkpoints off would, and writes a format-3 file that is used
    let ref_dir = tempfile::tempdir().unwrap();
    let mut ref_log = open_log(ref_dir.path());
    run_t_keys(&mut ref_log, &rc(&c), Some(true), Some(true), &Policy::new(330), None, &MemSandbox(&w)).unwrap();
    let ref_s = run_t_keys(&mut ref_log, &rc(&c), Some(true), Some(true), &Policy::new(120), None, &MemSandbox(&w)).unwrap();
    let mut p = Policy::new(120);
    p.checkpoint_every = 100;
    let s = run_t_keys(&mut log, &rc(&c), Some(true), Some(true), &p, None, &MemSandbox(&w)).unwrap();
    assert_eq!(s.ckpt.restored.from_version, None);
    assert!(s.ckpt.written.is_some());
    assert_eq!(s.truths.iter().map(|x| x.plan).collect::<Vec<_>>(), ref_s.truths.iter().map(|x| x.plan).collect::<Vec<_>>());
    let log = open_log(d.path());
    let (st, r) = ckpt::restore::<StateT>(&log).unwrap();
    assert!(r.from_version.is_some() && st == ckpt::full_fold::<StateT>(&log).unwrap());
    assert!(st.learner.file_beta_table && st.step == 450);
    assert_eq!(replay_t(&rows(&log)).1, 0);
}

/// The format-3 payload carries both W2b's table bit and S1's self-tune state (each round-trips exactly). The two
/// are never on in one being (see the next test); this checks the payload layout itself.
#[test]
fn the_format_3_payload_round_trips_the_table_bit_and_the_self_tune_state() {
    let c = wc(14, 1500, 1000, 1000);
    let w = WorldT::new(c.clone());
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let s = run_t_self(&mut log, &rc(&c), Some(true), Some(true), &Policy::new(450), None, &MemSandbox(&w)).unwrap();
    let mut st = s.state.clone();
    assert!(st.self_tune.on && st.self_tune.blocks.len() == 3);
    for table in [false, true] {
        st.learner.file_beta_table = table;
        let b = crate::being::binser::to_bytes(&st).unwrap();
        let back: StateT = crate::being::binser::from_bytes(&b).unwrap();
        assert!(back == st && back.learner.file_beta_table == table && back.self_tune == st.self_tune);
    }
}

/// `wt_self_tune = on` with `wt_file_beta = table` is defined by neither pre-registration: a new being asking for
/// both is refused (fail closed, nothing appended), a genesis carrying both is not a valid genesis, and the
/// controller tool refuses and audits it.
#[test]
fn self_tune_with_the_beta_table_is_refused_fail_closed() {
    let c = wc(15, 1200, 1000, 1000);
    let w = WorldT::new(c.clone());
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let v = log.latest_version();
    let e = run_t_opts(&mut log, &rc(&c), Some(true), Some(true), Some(true), &Policy::new(50), None, &MemSandbox(&w)).unwrap_err();
    assert!(e.to_string().contains("not defined") && e.to_string().contains("fail closed"), "{e}");
    assert_eq!(log.latest_version(), v);
    assert!(rows(&log).is_empty());
    // each one alone still works; off / no request with the table is the W2b being
    for (fb, stn) in [(Some(true), Some(false)), (Some(true), None), (Some(false), Some(true)), (None, Some(true))] {
        let d = tempfile::tempdir().unwrap();
        let mut log = open_log(d.path());
        let s = run_t_opts(&mut log, &rc(&c), Some(true), fb, stn, &Policy::new(30), None, &MemSandbox(&w)).unwrap();
        assert_eq!((s.state.learner.file_beta_table, s.state.self_tune.on), (fb == Some(true), stn == Some(true)));
        // and a later run on that being asking for both is refused by the genesis rules, not run
        let v = log.latest_version();
        assert!(run_t_opts(&mut log, &rc(&c), Some(true), Some(true), Some(true), &Policy::new(10), None, &MemSandbox(&w)).is_err());
        assert_eq!(log.latest_version(), v);
    }
    // without the file key neither is read: a plain being
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let s = run_t_opts(&mut log, &rc(&c), Some(false), Some(true), Some(true), &Policy::new(10), None, &MemSandbox(&w)).unwrap();
    assert!(!s.state.learner.file_key && !s.state.learner.file_beta_table && !s.state.self_tune.on);
    // a genesis with both tags (hand-written) is not a valid genesis
    let mut tags = b1p().tags();
    tags.push(super::self_tune::GENESIS_TAG.into());
    let tags = tags.join(",");
    assert_eq!(super::self_tune::genesis_setting(&tags), None);
    let (mut ev, _) = genesis_event_t(&c, &b1p());
    ev.tags.push(super::self_tune::GENESIS_TAG.into());
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    log.append_events(&[ev]).unwrap();
    assert!(StateT::from_events(&rows(&log)).cfg.is_none());
}
