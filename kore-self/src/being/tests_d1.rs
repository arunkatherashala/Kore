//! Track D1: new logs are format 3 (D1a) with unchanged decisions; state checkpoints (D1b) give the
//! full fold, and every bad checkpoint falls back to the full fold with the same state.

use super::b14::ctl::{B14Ctl, B14Setup};
use super::ckpt::{self, Foldable};
use super::controller::Controller;
use super::e1::ctl::{E1Ctl, E1Setup};
use super::measure_b11::LONGRUN_CONTROLLER;
use super::run_loop::{run_full, RunCfg};
use super::state::{BeingState, Chooser, SOURCE};
use super::world_o::engine::{run_o, LearnerOCfg, RunOCfg, StateO};
use super::world_o::world::{WOCfg, SOURCE_O};
use super::world_t::engine::{run_t, MemSandbox, RunTCfg, StateT, DEFAULT_DISCOVER_EVERY};
use super::world_t::world::{WTCfg, WorldT, SOURCE_T};
use super::{Policy, ScriptedThinker};
use crate::versioned::checkpoint::CkptCrash;
use crate::versioned::{EventRow, LogFormat, NewEvent, VersionedLog, CKPT_DIR, DEFAULT_FORMAT};
use std::path::Path;

const CTL_ALL: &str = "max_live_rules = 16\nmax_building_blocks = 8\nmax_search_candidates = 2000\nmax_steps_per_run = 5000\ncomposition = on\npruning = on\nrule_library = on\ncuriosity = on\nmax_curiosity_energy_pm = 20\nevolution = on\nevolution_budget_pm = 20\n";

fn clock() -> String {
    "2030-01-01T00:00:00Z".to_string()
}

fn open_fmt(root: &Path, fmt: LogFormat) -> VersionedLog {
    VersionedLog::open_or_create_with_format(root, "being", clock, fmt).unwrap()
}

fn open(root: &Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", clock).unwrap()
}

fn dbg<T: std::fmt::Debug>(x: &T) -> String {
    format!("{x:?}")
}

/// (kind, occurred_at, text, tags) of the rows of `source`: the being's decisions, outcomes and notes.
fn content(log: &VersionedLog, source: &str) -> Vec<(String, String, String, String)> {
    let mut v = vec![];
    log.for_each_event(|e| {
        if e.source == source || (source == SOURCE && e.source == super::advice::ADVISOR_SOURCE) {
            v.push((e.kind.clone(), e.occurred_at.clone(), e.text.clone(), e.tags.clone()));
        }
        true
    })
    .unwrap();
    v
}

fn limits() -> super::controller::Limits {
    Controller::parse(LONGRUN_CONTROLLER).unwrap().limits(true, true).0
}

/// First-world settings that cover the tracks with restart tests (UCB, B5 rules, B6 transfer, B14, E1).
fn first_worlds() -> Vec<(&'static str, RunCfg)> {
    let base = RunCfg { chooser: Some(Chooser::Model), budget: Some(3000), refill_when_exhausted: true, loop_seed: Some(1), ..Default::default() };
    vec![
        ("first world ucb", RunCfg { chooser: None, budget: Some(800), refill_when_exhausted: true, ..Default::default() }),
        ("rules b5", RunCfg { ext2: Some(true), discover_every: Some(10), ctl: Some(limits()), world_seed: Some(4), ..base.clone() }),
        ("transfer b6", RunCfg { ext2: Some(true), transfer: Some(true), learn_transfer: Some(true), discover_every: Some(10), ctl: Some(limits()), ..base.clone() }),
        (
            "b14 + e1",
            RunCfg {
                ext2: Some(true),
                discover_every: Some(10),
                ctl: Some(limits()),
                world_seed: Some(2),
                b14: Some(B14Setup { library: true, curiosity_pm: 20 }),
                b14_ctl: Some(B14Ctl::from_text(Some(CTL_ALL))),
                e1: Some(E1Setup { pm: 20 }),
                e1_ctl: Some(E1Ctl::from_text(Some(CTL_ALL))),
                ..base.clone()
            },
        ),
    ]
}

fn run_first(root: &Path, fmt: LogFormat, cfg: &RunCfg, n: u64, every: u64) -> super::run_loop::RunSummary {
    let mut log = open_fmt(root, fmt);
    let mut p = Policy::new(n);
    p.refill_when_exhausted = cfg.refill_when_exhausted;
    p.checkpoint_every = every;
    run_full(&mut log, &RunCfg { steps: n, ..cfg.clone() }, &ScriptedThinker, &p, None, None).unwrap()
}

fn wt_cfg() -> WTCfg {
    WTCfg::new(12, 1500)
}

fn run_world_t(root: &Path, fmt: LogFormat, n: u64, every: u64) -> super::world_t::engine::RunTSummary {
    let c = wt_cfg();
    let w = WorldT::new(c.clone());
    let mut log = open_fmt(root, fmt);
    let lim = Controller::parse("").unwrap().limits(false, false).0;
    let mut p = Policy::new(n);
    p.checkpoint_every = every;
    run_t(&mut log, &RunTCfg { world: Some(c), learner: None, discover_every: DEFAULT_DISCOVER_EVERY, ctl: Some(lim), sql: None }, &p, None, &MemSandbox(&w)).unwrap()
}

fn run_world_o(root: &Path, fmt: LogFormat, n: u64, every: u64) -> super::world_o::engine::RunOSummary {
    let c = WOCfg { intro_every: 400, life_intros: 6, tier_every: 2, drift_pm: 500, ..WOCfg::new(7, 40_000) };
    let mut log = open_fmt(root, fmt);
    let mut p = Policy::new(n);
    p.checkpoint_every = every;
    run_o(&mut log, &RunOCfg { world: Some(c), learner: Some(LearnerOCfg::default()), discover_every: 10, ctl: None, max_explore_pm: None }, &p, None).unwrap()
}

// ─── D1a ─────────────────────────────────────────────────────────────────────

#[test]
fn new_logs_are_format_3_and_a_format_1_log_keeps_its_format() {
    assert_eq!(DEFAULT_FORMAT, LogFormat::V3);
    let d = tempfile::tempdir().unwrap();
    assert_eq!(open(d.path()).format(), LogFormat::V3);
    assert_eq!(VersionedLog::open_or_create(d.path(), "other").unwrap().format(), LogFormat::V3);
    // an existing format-1 log opened through the default path stays format 1, appends and reopens as format 1
    let d1 = tempfile::tempdir().unwrap();
    drop(open_fmt(d1.path(), LogFormat::V1));
    let mut l = open(d1.path());
    assert_eq!(l.format(), LogFormat::V1);
    let ev = NewEvent { source: "test".into(), occurred_at: clock(), kind: "note".into(), text: "one".into(), tags: vec![], redacted: false };
    l.append_events(&[ev]).unwrap();
    drop(l);
    let l = VersionedLog::open(d1.path(), "being").unwrap();
    assert_eq!((l.format(), l.event_count()), (LogFormat::V1, 1));
    l.verify().unwrap();
}

#[test]
fn format_1_and_format_3_logs_give_the_same_being_and_the_same_decisions() {
    for (name, cfg) in first_worlds() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        for n in [150, 250] {
            let sa = run_first(a.path(), LogFormat::V1, &cfg, n, 0);
            let sb = run_first(b.path(), LogFormat::V3, &cfg, n, 0);
            assert_eq!(dbg(&sa.state), dbg(&sb.state), "{name}: live state");
        }
        let (la, lb) = (VersionedLog::open(a.path(), "being").unwrap(), VersionedLog::open(b.path(), "being").unwrap());
        assert_eq!((la.format(), lb.format()), (LogFormat::V1, LogFormat::V3));
        assert_eq!(dbg(&BeingState::from_log(&la).unwrap()), dbg(&BeingState::from_log(&lb).unwrap()), "{name}: fold");
        let (ca, cb) = (content(&la, SOURCE), content(&lb, SOURCE));
        assert!(ca.iter().filter(|r| r.0 == "decision").count() >= 400, "{name}");
        assert_eq!(ca, cb, "{name}: decision sequence");
    }
    // World T
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for n in [120, 180] {
        let sa = run_world_t(a.path(), LogFormat::V1, n, 0);
        let sb = run_world_t(b.path(), LogFormat::V3, n, 0);
        assert_eq!(sa.state, sb.state);
    }
    let (la, lb) = (VersionedLog::open(a.path(), "being").unwrap(), VersionedLog::open(b.path(), "being").unwrap());
    assert_eq!(ckpt::full_fold::<StateT>(&la).unwrap(), ckpt::full_fold::<StateT>(&lb).unwrap());
    assert_eq!(content(&la, SOURCE_T), content(&lb, SOURCE_T));
}

// ─── D1b: checkpoint fold equals the full fold ───────────────────────────────

/// After every chunk (a cold restart: fresh open), the restore equals the full fold; at the end the
/// being's rows equal those of the same chunks without checkpoints and of one run without restarts.
fn check_restarts<S: Foldable>(source: &str, chunks: &[u64], run: &dyn Fn(&Path, u64, u64)) {
    let (with, without, one) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut used = 0;
    for (i, &n) in chunks.iter().enumerate() {
        run(with.path(), n, 100);
        run(without.path(), n, 0);
        let log = VersionedLog::open(with.path(), "being").unwrap();
        let (st, r) = ckpt::restore::<S>(&log).unwrap();
        assert!(st.same(&ckpt::full_fold::<S>(&log).unwrap()), "{} chunk {i}", S::KIND);
        assert!(r.rejected.is_empty(), "{:?}", r.rejected);
        used += r.from_version.is_some() as usize;
        for line in ckpt::audit::<S>(&log).unwrap() {
            assert_eq!((line.equal_to_full_fold, line.problem.clone()), (Some(true), None), "{}", line.file);
        }
    }
    assert!(used >= chunks.len() - 1, "{}: checkpoints used {used}", S::KIND);
    run(one.path(), chunks.iter().sum(), 0);
    let (lw, lo, l1) = (VersionedLog::open(with.path(), "being").unwrap(), VersionedLog::open(without.path(), "being").unwrap(), VersionedLog::open(one.path(), "being").unwrap());
    assert_eq!(content(&lw, source), content(&lo, source), "{}: checkpoints change no decision", S::KIND);
    assert_eq!(content(&lw, source), content(&l1, source), "{}: restarts change no decision", S::KIND);
    assert!(lw.checkpoint_files(S::KIND).len() <= ckpt::KEEP);
    lw.verify().unwrap();
}

#[test]
fn checkpoint_restore_equals_the_full_fold_in_every_world_and_track() {
    let chunks = [150, 120, 1, 130, 99];
    for (_, cfg) in first_worlds() {
        for fmt in [LogFormat::V1, LogFormat::V3] {
            check_restarts::<BeingState>(SOURCE, &chunks, &|root, n, every| {
                run_first(root, fmt, &cfg, n, every);
            });
        }
    }
    check_restarts::<StateT>(SOURCE_T, &chunks, &|root, n, every| {
        run_world_t(root, LogFormat::V3, n, every);
    });
    check_restarts::<StateO>(SOURCE_O, &[400, 300, 300], &|root, n, every| {
        run_world_o(root, LogFormat::V3, n, every);
    });
}

#[test]
fn checkpoints_off_write_nothing_and_ignore_existing_files() {
    let cfg = first_worlds().remove(0).1;
    let d = tempfile::tempdir().unwrap();
    let s = run_first(d.path(), LogFormat::V3, &cfg, 150, 0);
    assert!(!d.path().join("being").join("log").join(CKPT_DIR).exists());
    assert_eq!(s.ckpt, Default::default());
    let s = run_first(d.path(), LogFormat::V3, &cfg, 150, 100);
    assert_eq!(s.ckpt.written, Some(s.version_after - 1));
    let s = run_first(d.path(), LogFormat::V3, &cfg, 10, 0);
    assert_eq!((s.ckpt.restored.from_version, s.ckpt.written), (None, None));
    // a checkpoint is written only when due: 10 more steps after one at step 300 is not due
    let s = run_first(d.path(), LogFormat::V3, &cfg, 150, 100);
    assert!(s.ckpt.restored.from_version.is_some() && s.ckpt.written.is_some());
    let s = run_first(d.path(), LogFormat::V3, &cfg, 10, 100);
    assert!(s.ckpt.restored.from_version.is_some() && s.ckpt.written.is_none());
}

// ─── D1b: bad checkpoints fall back ──────────────────────────────────────────

/// A being with two checkpoints (after 150 and 300 steps), and its full fold.
fn two_checkpoints() -> (tempfile::TempDir, String) {
    let cfg = first_worlds().remove(1).1;
    let d = tempfile::tempdir().unwrap();
    run_first(d.path(), LogFormat::V3, &cfg, 150, 100);
    run_first(d.path(), LogFormat::V3, &cfg, 150, 100);
    let log = VersionedLog::open(d.path(), "being").unwrap();
    assert_eq!(log.checkpoint_files("being").len(), 2);
    let full = dbg(&ckpt::full_fold::<BeingState>(&log).unwrap());
    (d, full)
}

fn ckpt_path(root: &Path, file: &str) -> std::path::PathBuf {
    root.join("being").join("log").join(file)
}

/// Restore after a cold open: the version used and the number of rejected files; the state must be the full fold.
fn restore_check(root: &Path, full: &str) -> (Option<u64>, usize) {
    let log = VersionedLog::open(root, "being").unwrap();
    let (st, r) = ckpt::restore::<BeingState>(&log).unwrap();
    assert_eq!(dbg(&st), full);
    (r.from_version, r.rejected.len())
}

#[test]
fn corrupted_or_truncated_checkpoints_fall_back_with_the_same_state() {
    // one byte flipped anywhere (also in bytes the reader never decodes, such as the column statistics
    // of the kore file): the file hash no longer matches its anchor, the older checkpoint is used
    {
        let (d, full) = two_checkpoints();
        let files = VersionedLog::open(d.path(), "being").unwrap().checkpoint_files("being");
        let p = ckpt_path(d.path(), &files[0].1);
        let orig = std::fs::read(&p).unwrap();
        let mut rejected = 0;
        for k in 0..48 {
            let mut b = orig.clone();
            let i = k * b.len() / 48;
            b[i] ^= 0x5a;
            std::fs::write(&p, &b).unwrap();
            let (v, rej) = restore_check(d.path(), &full);
            assert_eq!((v, rej), (Some(files[1].0), 1), "flip at byte {i}");
            rejected += rej;
        }
        assert_eq!(rejected, 48);
    }
    for damage in ["payload", "truncate", "empty"] {
        let (d, full) = two_checkpoints();
        let log = VersionedLog::open(d.path(), "being").unwrap();
        let files = log.checkpoint_files("being");
        drop(log);
        let hurt = |file: &str| {
            let p = ckpt_path(d.path(), file);
            let mut b = std::fs::read(&p).unwrap();
            match damage {
                "payload" => {
                    // a forged payload written as a well-formed file with the old content hash
                    let log = VersionedLog::open(d.path(), "being").unwrap();
                    let mut c = log.read_checkpoint("being", file).unwrap();
                    c.payload[0] ^= 1;
                    b = crate::versioned::checkpoint::encode_with_format(&c, crate::versioned::CKPT_FORMAT);
                }
                "truncate" => b.truncate(b.len() / 2),
                _ => b.clear(),
            }
            std::fs::write(&p, b).unwrap();
        };
        // the newest is damaged: the older one is used
        hurt(&files[0].1);
        assert_eq!(restore_check(d.path(), &full), (Some(files[1].0), 1), "{damage}");
        // both damaged: the whole log is folded
        hurt(&files[1].1);
        assert_eq!(restore_check(d.path(), &full), (None, 2), "{damage}");
    }
}

#[test]
fn stale_and_foreign_checkpoints_are_not_used() {
    let (d, full) = two_checkpoints();
    let cfg = first_worlds().remove(1).1;
    // a shorter copy of the same being (150 steps): the longer log's checkpoint is after its head
    let short = tempfile::tempdir().unwrap();
    run_first(short.path(), LogFormat::V3, &cfg, 150, 0);
    let short_full = dbg(&ckpt::full_fold::<BeingState>(&VersionedLog::open(short.path(), "being").unwrap()).unwrap());
    let log = VersionedLog::open(d.path(), "being").unwrap();
    let files = log.checkpoint_files("being");
    std::fs::create_dir_all(ckpt_path(short.path(), CKPT_DIR)).unwrap();
    std::fs::copy(ckpt_path(d.path(), &files[0].1), ckpt_path(short.path(), &files[0].1)).unwrap();
    assert_eq!(restore_check(short.path(), &short_full), (None, 1));
    // the older checkpoint of the long log is at a version the short log has, but the short log was
    // written with another history (no checkpoint anchor, other recorded commits): chain mismatch or no anchor
    std::fs::copy(ckpt_path(d.path(), &files[1].1), ckpt_path(short.path(), &files[1].1)).unwrap();
    let (v, rej) = restore_check(short.path(), &short_full);
    assert_eq!((v, rej), (None, 2));
    // another being (other world seed) with the same number of steps: chain mismatch
    let other = tempfile::tempdir().unwrap();
    let ocfg = RunCfg { world_seed: Some(9), ..cfg.clone() };
    run_first(other.path(), LogFormat::V3, &ocfg, 150, 0);
    run_first(other.path(), LogFormat::V3, &ocfg, 150, 0);
    let ofull = dbg(&ckpt::full_fold::<BeingState>(&VersionedLog::open(other.path(), "being").unwrap()).unwrap());
    std::fs::create_dir_all(ckpt_path(other.path(), CKPT_DIR)).unwrap();
    for (_, f) in &files {
        std::fs::copy(ckpt_path(d.path(), f), ckpt_path(other.path(), f)).unwrap();
    }
    let olog = VersionedLog::open(other.path(), "being").unwrap();
    for (_, f) in &files {
        let e = olog.read_checkpoint("being", f).unwrap_err();
        assert!(e.contains("chain hash") || e.contains("after the latest version"), "{e}");
    }
    assert_eq!(restore_check(other.path(), &ofull), (None, 2));
    // an older but valid checkpoint is fine: a longer tail, the same state
    std::fs::remove_file(ckpt_path(d.path(), &files[0].1)).unwrap();
    assert_eq!(restore_check(d.path(), &full), (Some(files[1].0), 0));
}

#[test]
fn a_forged_checkpoint_with_a_valid_looking_hash_is_not_used() {
    let (d, full) = two_checkpoints();
    let mut log = VersionedLog::open(d.path(), "being").unwrap();
    let files = log.checkpoint_files("being");
    let good = log.read_checkpoint("being", &files[0].1).unwrap();
    let mut st: BeingState = super::binser::from_bytes(&good.payload).unwrap();
    st.total_reward += 1000;
    st.budget_left += 1000;
    let payload = super::binser::to_bytes(&st).unwrap();
    // (1) the same file name and version, content hash recomputed: self-consistent, but not anchored
    let forged = log.write_checkpoint("being", st.step, payload.clone()).unwrap();
    assert!(log.read_checkpoint("being", &forged.file).is_ok(), "the forged file passes the file checks");
    assert_eq!(restore_check(d.path(), &full), (Some(files[0].0), 1), "the forged newest file is rejected, the good one used");
    // (2) a forged file at the version of the good one (overwriting it): not anchored either
    let c = crate::versioned::checkpoint::content_hash(crate::versioned::CKPT_FORMAT, "being", good.version, &good.chain_hash, st.step, &payload);
    assert_ne!(c, good.content_hash);
    let audit = ckpt::audit_summary(&log).unwrap();
    assert!(audit.contains("NOT usable") && audit.contains("no anchor"), "{audit}");
    // (3) appending an anchor for the forged hash is a change of the hash-chained log: the log's head
    // moves, so `verify_against(old head)` reports it
    let head = log.snapshot_hash(log.latest_version()).unwrap();
    log.append_events(&[ckpt::anchor_event(&forged, 0)]).unwrap();
    assert!(log.verify_against(&head).is_err());
    log.verify().unwrap();
}

#[test]
fn an_unknown_snapshot_format_version_is_ignored() {
    let (d, full) = two_checkpoints();
    let log = VersionedLog::open(d.path(), "being").unwrap();
    let files = log.checkpoint_files("being");
    let good = log.read_checkpoint("being", &files[0].1).unwrap();
    let bytes = crate::versioned::checkpoint::encode_with_format(&good, crate::versioned::CKPT_FORMAT + 1);
    std::fs::write(ckpt_path(d.path(), &files[0].1), bytes).unwrap();
    let e = log.read_checkpoint("being", &files[0].1).unwrap_err();
    assert!(e.contains("unknown snapshot format version"), "{e}");
    assert_eq!(restore_check(d.path(), &full), (Some(files[1].0), 1));
}

#[test]
fn a_crash_while_writing_a_checkpoint_leaves_the_previous_one_in_use() {
    let (d, full) = two_checkpoints();
    let cfg = first_worlds().remove(1).1;
    run_first(d.path(), LogFormat::V3, &cfg, 50, 0);
    let full2 = {
        let log = VersionedLog::open(d.path(), "being").unwrap();
        let files = log.checkpoint_files("being");
        let st = ckpt::full_fold::<BeingState>(&log).unwrap();
        let p = super::binser::to_bytes(&st).unwrap();
        assert!(log.write_checkpoint_at("being", st.step, p, CkptCrash::BeforeRename).is_err());
        let tmp: Vec<_> = std::fs::read_dir(ckpt_path(d.path(), CKPT_DIR)).unwrap().flatten().filter(|f| f.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert_eq!(tmp.len(), 1, "the torn temp file is left behind");
        assert_eq!(log.checkpoint_files("being"), files, "the reader does not see it");
        let (v, rej) = restore_check(d.path(), &dbg(&st));
        assert_eq!((v, rej), (Some(files[0].0), 0));
        dbg(&st)
    };
    assert_ne!(full, full2);
    // the next write cleans the leftover
    run_first(d.path(), LogFormat::V3, &cfg, 150, 100);
    let left: Vec<_> = std::fs::read_dir(ckpt_path(d.path(), CKPT_DIR)).unwrap().flatten().filter(|f| f.file_name().to_string_lossy().ends_with(".tmp")).collect();
    assert!(left.is_empty());
}

#[test]
fn forget_before_the_checkpoint_invalidates_it_and_forget_after_it_does_not() {
    // (a) forget an outcome of the being from early in the log: entry v is re-sealed, the checkpoint is not used
    let (d, _) = two_checkpoints();
    let mut log = VersionedLog::open(d.path(), "being").unwrap();
    let mut early = None;
    log.for_each_event(|e| {
        if e.source == SOURCE && e.kind == "outcome" {
            early = Some(e.event_id.clone());
        }
        early.is_none()
    })
    .unwrap();
    log.forget_events(&[early.unwrap()]).unwrap();
    drop(log);
    let log = VersionedLog::open(d.path(), "being").unwrap();
    let full = dbg(&ckpt::full_fold::<BeingState>(&log).unwrap());
    for (_, f) in log.checkpoint_files("being") {
        assert!(log.read_checkpoint("being", &f).unwrap_err().contains("chain hash"));
    }
    assert_eq!(restore_check(d.path(), &full), (None, 2));
    // (b) forget an event written after the newest checkpoint: entry v keeps its hash, the checkpoint is used
    let (d, _) = two_checkpoints();
    let mut log = VersionedLog::open(d.path(), "being").unwrap();
    let newest = log.checkpoint_files("being")[0].0;
    let ev = NewEvent { source: "owner:note".into(), occurred_at: clock(), kind: "note".into(), text: "a private note".into(), tags: vec![], redacted: false };
    log.append_events(&[ev]).unwrap();
    let id = log.event_ids_by_source_prefix("owner:note");
    log.forget_events(&id).unwrap();
    drop(log);
    let log = VersionedLog::open(d.path(), "being").unwrap();
    let full = dbg(&ckpt::full_fold::<BeingState>(&log).unwrap());
    assert_eq!(restore_check(d.path(), &full), (Some(newest), 0));
}

#[test]
fn the_controller_key_validates_and_fails_closed_to_off() {
    let base = "max_steps_per_run = 5000\n";
    assert_eq!(ckpt::every_from_text(Some(&format!("{base}checkpoint_every = 5000\n"))), 5000);
    assert_eq!(ckpt::every_from_text(Some(&format!("{base}checkpoint_every = 0\n"))), 0);
    assert_eq!(ckpt::every_from_text(Some(base)), 0, "missing key: off");
    assert_eq!(ckpt::every_from_text(None), 0, "missing file: off");
    for bad in ["checkpoint_every = 50", "checkpoint_every = 2000000", "checkpoint_every = x", "checkpoint_every = 5000\ncheckpoint_every = 5000"] {
        let t = format!("{base}{bad}\n");
        assert!(Controller::parse(&t).is_err(), "{bad}");
        assert_eq!(ckpt::every_from_text(Some(&t)), 0, "invalid file: off");
    }
    // coexists with every other opt-in key
    let all = format!("{CTL_ALL}checkpoint_every = 1000\nworld_t_run = on\nl2_meta_prior = on\n");
    assert!(Controller::parse(&all).is_ok(), "{:?}", Controller::parse(&all));
    assert_eq!(ckpt::every_from_text(Some(&all)), 1000);
}

#[test]
fn the_anchor_note_is_folded_by_no_reader() {
    let (d, full) = two_checkpoints();
    let log = VersionedLog::open(d.path(), "being").unwrap();
    let mut anchors = vec![];
    log.for_each_event(|e| {
        if e.source == ckpt::ANCHOR_SOURCE {
            anchors.push(e);
        }
        true
    })
    .unwrap();
    assert_eq!(anchors.len(), 2);
    let mut st = BeingState::default();
    let mut t = StateT::default();
    let mut o = StateO::default();
    for a in &anchors {
        st.fold_event(a);
        t.fold_event(a);
        o.fold_event(a);
    }
    assert_eq!((dbg(&st), t, o), (dbg(&BeingState::default()), StateT::default(), StateO::default()));
    let _ = full;
    let _: Vec<EventRow> = anchors;
}
