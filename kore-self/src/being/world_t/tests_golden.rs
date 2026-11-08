//! Golden test: with World T off (no `world_t_run` key, no World T rows in the log), the first world,
//! World v2 and World O write exactly the logs they wrote before World T existed.
//!
//! The digests below were computed at commit 8591da53 plus only the `pub mod world_t;` line (no other
//! shared file touched), then pinned; the controller key and the internal tool were added afterwards
//! and this test still passes, so those edits change nothing for the existing worlds.

use crate::being::controller::{Controller, Limits};
use crate::being::policy::Policy;
use crate::being::run_loop::{run, RunCfg};
use crate::being::solver::ScriptedThinker;
use crate::being::state::Chooser;
use crate::being::world2::engine::{run_v2, Run2Cfg};
use crate::being::world2::world::W2Cfg;
use crate::being::world_o::engine::{run_o, RunOCfg};
use crate::being::world_o::world::WOCfg;
use crate::versioned::{sha256_hex, EventRow, RevisionRow, VersionedLog};

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

/// Digest of every event and revision of a log (content only; `recorded_at` comes from the fixed clock).
pub fn log_digest(log: &VersionedLog) -> String {
    let snap = log.open_latest();
    let ev: Vec<EventRow> = snap.rows();
    let rv: Vec<RevisionRow> = snap.rows();
    let mut s = String::new();
    for e in &ev {
        s.push_str(&format!("{}|{}|{}|{}|{}|{}\n", e.event_id, e.occurred_at, e.source, e.kind, e.text, e.tags));
    }
    for r in &rv {
        s.push_str(&format!("{}|{}|{}|{}|{:.6}|{}|{}\n", r.rev_id, r.entity_type, r.key, r.stance, r.confidence, r.rationale, r.author));
    }
    sha256_hex(&[s.as_bytes()])[..16].to_string()
}

fn lim() -> Limits {
    let (l, _) = Controller::parse("").unwrap().limits(false, false);
    l
}

fn digests() -> Vec<String> {
    let mut out = vec![];
    // the first world (menu world, the B1 model learner with B2 discovery, controller caps)
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let cfg = RunCfg { world_seed: Some(3), loop_seed: Some(4), budget: Some(1500), chooser: Some(Chooser::Model), extended: Some(true), discover_every: Some(10), ctl: Some(lim()), steps: 300, ..Default::default() };
    run(&mut log, &cfg, &ScriptedThinker, &Policy::new(300)).unwrap();
    out.push(log_digest(&log));
    // World v2
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let c = W2Cfg { noise_pm: 80, drift_per_10k: 25, curve_pct: 50, ..W2Cfg::new(5, 12, 3000) };
    run_v2(&mut log, &Run2Cfg { world: Some(c), learner: None, discover_every: 10, ctl: Some(lim()) }, &Policy::new(400), None).unwrap();
    out.push(log_digest(&log));
    // World O
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let w = WOCfg { intro_every: 400, life_intros: 6, tier_every: 2, drift_pm: 500, ..WOCfg::new(7, 20_000) };
    run_o(&mut log, &RunOCfg { world: Some(w), discover_every: 10, ctl: Some(lim()), ..Default::default() }, &Policy::new(400), None).unwrap();
    out.push(log_digest(&log));
    // the controller's parse of a full file without any World T key
    let text = "max_live_rules = 12\nmax_building_blocks = 4\nmax_search_candidates = 900\nmax_steps_per_run = 300\nmax_energy_per_run = 5000\ncomposition = on\npruning = on\ntransfer = on\nexperiments = on\nmax_experiment_energy_pm = 50\nmax_experiments_per_run = 10\nrules_use = on\n";
    out.push(sha256_hex(&[format!("{:?}", Controller::parse(text)).as_bytes()])[..16].to_string());
    out
}

/// Pinned at 8591da53 + `pub mod world_t;` (see the module comment).
const GOLDEN: [&str; 4] = ["40721d08e8ef313f", "2baf2a28200e4308", "c02bb5b35ec8f00a", "d6be59154d3a8f43"];

#[test]
fn existing_worlds_are_byte_identical_with_world_t_off() {
    let got = digests();
    assert_eq!(got, GOLDEN.map(String::from).to_vec(), "an existing world's log changed: {got:?}");
}
