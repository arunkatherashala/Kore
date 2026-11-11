//! The gate of a World M3 run (tool layer): the owner's controller, the kill switch, `authorize()`, the source
//! file check, the disk guard, the sandbox on disk, and the logged run. In-process only (no MCP tool).
//!
//! Order of the checks, each refusal audited (source `authorize`):
//! 1. `controller.txt` must exist and parse (fail closed), and contain `world_m3_run = on` (default off);
//! 2. the `KILL` file must not exist (and is checked again before every step);
//! 3. `authorize()` must allow the internal tool `world_m3_run` (tier L2; default deny);
//! 4. the source file `<owner dir>/world_m3/source/yellow_tripdata_2024-01.parquet` (copied there by the owner;
//!    never executed) must have the pinned SHA-256 and pass every cap of `world_m3::source::load`;
//! 5. free disk on the data volume must be at least 4 GB before the sandbox is written.
//! Then the pool is written into `<owner dir>/world_m3/seed_<seed>/` (every path through `safe_path`) and the
//! being runs with the controller's caps. Nothing leaves the sandbox and the log.

#![allow(dead_code)]

use crate::authorize::{self, Outcome, BEING_ACTOR};
use crate::being::controller::{self, Controller, Origin};
use crate::being::policy::{KillCheck, Policy};
use crate::being::world_m3::ctl;
use crate::being::world_m3::engine::{replay_t, run_t_key, RunTCfg, Sandbox, StateT, DEFAULT_DISCOVER_EVERY};
use crate::being::world_m3::learner::LearnerTCfg;
use crate::being::world_m3::source::{self, Base};
use crate::being::world_m3::world::{M3Cfg, WorldT, DEFAULT_BUDGET};
use crate::safety;
use crate::versioned::EventRow;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

pub const AUTH_TOOL: &str = "world_m3_run";
pub const MAX_STEPS: u64 = 5000;
pub const SOURCE_REL: &str = "world_m3/source/yellow_tripdata_2024-01.parquet";
pub const MIN_FREE_MB: u64 = 4096;
/// Largest sandbox file handed to a tool.
pub const MAX_FILE: u64 = 4 * 1024 * 1024;

fn audit(owner: &str, cap: &str, limit: u64, requested: u64, what: &str) {
    let _ = authorize::audit_controller_locked(owner, cap, limit, requested, what);
}

pub fn sandbox_rel(seed: u64) -> String {
    format!("world_m3/seed_{seed}")
}

fn plain_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 80 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') || name.starts_with('.') {
        return Err(format!("not a plain sandbox file name: {name:?}"));
    }
    Ok(())
}

fn path_of(owner: &str, rel: &str, name: &str) -> Result<PathBuf, String> {
    plain_name(name)?;
    safety::safe_path(owner, &format!("{rel}/{name}"))
}

/// The checked source of an owner (hash, size, rows, columns, values), or the refusal.
pub fn load_source(owner: &str) -> Result<Arc<Base>, String> {
    let p = safety::safe_path(owner, SOURCE_REL)?;
    source::load(&p).map(Arc::new)
}

pub fn write_pool(owner: &str, w: &WorldT) -> Result<(usize, usize), String> {
    let rel = sandbox_rel(w.cfg.seed);
    let dir = safety::safe_path(owner, &rel)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create the sandbox: {e}"))?;
    let mut bytes = 0;
    for f in &w.files {
        let p = path_of(owner, &rel, &f.name)?;
        if std::fs::read(&p).map_or(true, |b| b != f.bytes) {
            std::fs::write(&p, &f.bytes).map_err(|e| format!("cannot write {}: {e}", f.name))?;
        }
        bytes += f.bytes.len();
    }
    Ok((w.files.len(), bytes))
}

/// Reads sandbox files through `safe_path` (each once, then from memory).
pub struct DirSandbox {
    owner: String,
    rel: String,
    read_once: std::cell::RefCell<std::collections::HashMap<String, Vec<u8>>>,
}

impl DirSandbox {
    pub fn new(owner: &str, seed: u64) -> DirSandbox {
        DirSandbox { owner: owner.to_string(), rel: sandbox_rel(seed), read_once: Default::default() }
    }
}

impl Sandbox for DirSandbox {
    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        plain_name(name)?;
        if let Some(b) = self.read_once.borrow().get(name) {
            return Ok(b.clone());
        }
        let p = path_of(&self.owner, &self.rel, name)?;
        let m = std::fs::metadata(&p).map_err(|e| format!("{name}: {e}"))?;
        if !m.is_file() || m.len() > MAX_FILE {
            return Err(format!("{name}: not a file of at most {MAX_FILE} bytes"));
        }
        let b = std::fs::read(&p).map_err(|e| format!("{name}: {e}"))?;
        self.read_once.borrow_mut().insert(name.to_string(), b.clone());
        Ok(b)
    }
}

/// Run the World M3 being of `owner` for `steps` steps (creating it in world `seed` with `budget` energy).
pub fn run_world_m3(owner: &str, steps: u64, seed: u64, budget: Option<i64>) -> Result<Value, String> {
    if !(1..=MAX_STEPS).contains(&steps) {
        return Err(format!("'steps' must be 1..{MAX_STEPS}"));
    }
    let text = crate::world_t_tools::controller_text(owner);
    let ctl0 = text.as_deref().map(Controller::parse_or_closed).unwrap_or_else(|| Controller::fail_closed(format!("{} is missing", controller::FILE)));
    if ctl0.is_closed() {
        let reason = match &ctl0.origin {
            Origin::FailClosed(r) => r.clone(),
            Origin::File => String::new(),
        };
        audit(owner, "controller_closed", 0, steps, &reason);
        return Err(format!("controller refused the World M3 run (fail closed): {reason}"));
    }
    if let Err(why) = ctl::allowed(text.as_deref()) {
        audit(owner, ctl::KEY, 0, steps, &why);
        return Err(format!("controller refused the World M3 run: {why}"));
    }
    if authorize::kill_active(owner) {
        audit(owner, "kill_switch", 0, steps, "the KILL file exists: the World M3 run was refused before it started");
        return Err("the KILL file exists in the owner directory: the being does not run".into());
    }
    let d = authorize::authorize_locked(owner, BEING_ACTOR, AUTH_TOOL, &json!({ "steps": steps, "world_seed": seed }));
    if d.outcome != Outcome::Allow {
        return Err(format!("{AUTH_TOOL} was not allowed ({}): {}. To allow World M3 runs, list {AUTH_TOOL} under [L2] in autonomy_policy.txt (edit that file by hand)", d.outcome.name(), d.reason));
    }
    let base = match load_source(owner) {
        Ok(b) => b,
        Err(e) => {
            audit(owner, "m3_source", 0, steps, &e);
            return Err(format!("controller refused the World M3 run: {e}"));
        }
    };
    let dir = safety::owner_dir(owner);
    let free = crate::world_t_sched::free_disk_mb(&dir);
    if free.map_or(true, |f| f < MIN_FREE_MB) {
        let why = format!("free disk on the data volume is {} MB; at least {MIN_FREE_MB} MB are needed: nothing is written", free.map_or("unknown".to_string(), |f| f.to_string()));
        audit(owner, "m3_min_free_disk_mb", MIN_FREE_MB, free.unwrap_or(0), &why);
        return Err(format!("controller refused the World M3 run: {why}"));
    }
    let (limits, _) = ctl0.limits(false, false);
    let mut refused: Vec<(String, u64, u64, String)> = vec![];
    let steps_eff = if steps > ctl0.max_steps_per_run {
        refused.push(("max_steps_per_run".into(), ctl0.max_steps_per_run, steps, format!("{steps} steps were requested, the controller allows {}; the run is cut to the cap", ctl0.max_steps_per_run)));
        ctl0.max_steps_per_run
    } else {
        steps
    };
    let mut log = crate::log_tools::open(owner).map_err(|v| v.to_string())?;
    let rows: Vec<EventRow> = log.open_latest().rows();
    let st0 = StateT::from_events(&rows);
    let wcfg = st0.cfg.clone().unwrap_or_else(|| M3Cfg::new(seed, budget.unwrap_or(DEFAULT_BUDGET)));
    let world = WorldT::new(wcfg.clone(), base.clone());
    write_pool(owner, &world)?;
    let sb = DirSandbox::new(owner, wcfg.seed);
    let mut policy = Policy::new(steps_eff);
    policy.max_energy = Some(ctl0.max_energy_per_run);
    let owner_k = owner.to_string();
    policy.kill = Some(KillCheck(std::sync::Arc::new(move || authorize::kill_active(&owner_k))));
    let cfg = RunTCfg { world: Some(wcfg), learner: Some(LearnerTCfg::default()), discover_every: DEFAULT_DISCOVER_EVERY, ctl: Some(limits), sql: Some(crate::being::world_t::ctl::sql_limits(text.as_deref())) };
    let s = run_t_key(&mut log, &cfg, None, &policy, None, &sb, base).map_err(|e| e.to_string())?;
    for r in &s.refusals {
        refused.push((r.cap.clone(), r.limit, r.requested, r.what.clone()));
    }
    if s.stopped == crate::being::StopReason::Killed {
        refused.push(("kill_switch".into(), 0, s.steps_run, format!("the KILL file stopped the World M3 run after {} steps", s.steps_run)));
    }
    drop(log);
    for r in &refused {
        audit(owner, &r.0, r.1, r.2, &r.3);
    }
    Ok(json!({
        "steps_run": s.steps_run, "stopped": s.stopped.as_str(), "reward": s.reward, "energy": s.cost,
        "rules_accepted_this_run": s.new_rules.len(), "right": s.state.right, "wrong": s.state.wrong, "no_answer": s.state.no_answer, "skips": s.state.skips,
        "refusals_audited": refused.iter().map(|r| json!({ "cap": r.0, "limit": r.1, "requested": r.2, "what": r.3 })).collect::<Vec<_>>(),
    }))
}

/// Fold the World M3 state from the owner's log and replay every decision (read-only; the replay needs the
/// checked source to rebuild the world).
pub fn status_world_m3(owner: &str) -> Result<Value, String> {
    let log = crate::log_tools::open(owner).map_err(|v| v.to_string())?;
    let rows: Vec<EventRow> = log.open_latest().rows();
    let st = StateT::from_events(&rows);
    let (n, bad) = if st.cfg.is_some() { replay_t(&rows, &load_source(owner)?) } else { (0, 0) };
    Ok(json!({ "exists": st.cfg.is_some(), "steps": st.step, "reward": st.total_reward, "energy": st.total_cost, "rules": st.rules.len(), "decisions_replayed": n, "replay_mismatches": bad }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(owner: &str) -> std::path::PathBuf {
        safety::test_support::init();
        let dir = safety::owner_dir(owner);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn audit_tools(owner: &str) -> Vec<String> {
        let log = crate::log_tools::open(owner).unwrap();
        log.open_latest().rows::<EventRow>().into_iter().filter(|e| e.source == authorize::SOURCE).filter_map(|e| crate::being::state::tag(&e.tags, "tool").map(|s| s.to_string())).collect()
    }

    /// Without the source file: every gate before the source refuses, and a wrong source is refused and audited.
    #[test]
    fn a_world_m3_run_is_off_by_default_and_refuses_a_wrong_source() {
        let o = "m3-gates";
        let dir = setup(o);
        assert!(run_world_m3(o, 5, 3, Some(800)).unwrap_err().contains("fail closed"));
        std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 40\n").unwrap();
        let e = run_world_m3(o, 5, 3, Some(800)).unwrap_err();
        assert!(e.contains("world_m3_run"), "{e}");
        std::fs::write(dir.join("controller.txt"), "world_m3_run = maybe\n").unwrap();
        assert!(run_world_m3(o, 5, 3, Some(800)).unwrap_err().contains("fail closed"));
        std::fs::write(dir.join("controller.txt"), "world_m3_run = on\nmax_steps_per_run = 40\n").unwrap();
        assert!(run_world_m3(o, 5, 3, Some(800)).unwrap_err().contains("world_m3_run was not allowed"));
        // World T's tool name does not allow World M3
        std::fs::write(dir.join("autonomy_policy.txt"), "[L2]\nworld_t_run\n").unwrap();
        assert!(run_world_m3(o, 5, 3, Some(800)).unwrap_err().contains("world_m3_run was not allowed"));
        std::fs::write(dir.join("autonomy_policy.txt"), "[L2]\nworld_m3_run\n").unwrap();
        std::fs::write(dir.join("KILL"), "").unwrap();
        assert!(run_world_m3(o, 5, 3, Some(800)).unwrap_err().contains("KILL"));
        std::fs::remove_file(dir.join("KILL")).unwrap();
        // no source file, then a file with the wrong bytes: refused and audited, nothing written
        assert!(run_world_m3(o, 5, 3, Some(800)).unwrap_err().contains("cannot be read"));
        std::fs::create_dir_all(dir.join("world_m3").join("source")).unwrap();
        std::fs::write(dir.join(SOURCE_REL), b"PAR1 not the taxi file PAR1").unwrap();
        let e = run_world_m3(o, 5, 3, Some(800)).unwrap_err();
        assert!(e.contains("SHA-256 mismatch"), "{e}");
        assert!(audit_tools(o).iter().any(|t| t.contains("m3_source")), "{:?}", audit_tools(o));
        assert!(!dir.join("world_m3").join("seed_3").exists());
        assert!(!status_world_m3(o).unwrap()["exists"].as_bool().unwrap());
    }

    /// With the real source (env `KORE_M3_SOURCE` = path of a checked copy; skipped otherwise): an allowed run in a
    /// temp home writes the sandbox, the log replays with no mismatch, and a second run continues the being.
    #[test]
    fn an_allowed_world_m3_run_with_the_source_replays() {
        let Ok(src) = std::env::var("KORE_M3_SOURCE") else {
            eprintln!("KORE_M3_SOURCE not set: skipped");
            return;
        };
        let o = "m3-run";
        let dir = setup(o);
        std::fs::write(dir.join("controller.txt"), "world_m3_run = on\nmax_steps_per_run = 60\nworld_t_sql_timeout_ms = 1500\n").unwrap();
        std::fs::write(dir.join("autonomy_policy.txt"), "[L2]\nworld_m3_run\n").unwrap();
        std::fs::create_dir_all(dir.join("world_m3").join("source")).unwrap();
        std::fs::copy(&src, dir.join(SOURCE_REL)).unwrap();
        let v = run_world_m3(o, 80, 7, Some(1500)).unwrap();
        assert_eq!(v["steps_run"], 60);
        assert!(audit_tools(o).iter().any(|t| t.contains("max_steps_per_run")));
        let n = std::fs::read_dir(dir.join("world_m3").join("seed_7")).unwrap().count();
        assert!(n > 150, "{n} files in the sandbox");
        let s = status_world_m3(o).unwrap();
        assert_eq!((s["steps"].as_u64(), s["decisions_replayed"].as_u64(), s["replay_mismatches"].as_u64()), (Some(60), Some(60), Some(0)));
        let log = crate::log_tools::open(o).unwrap();
        let rows: Vec<EventRow> = log.open_latest().rows();
        assert!(rows.iter().any(|e| e.source == "being:worldm3" && e.tags.contains("m3_genesis") && e.tags.contains("m3_version:1")));
        assert!(rows.iter().filter(|e| e.kind == "action" && e.text.contains("SQL #0")).all(|e| e.tags.contains("sql_timeout_ms:1500")));
        drop(log);
        assert_eq!(run_world_m3(o, 10, 99, None).unwrap()["steps_run"], 10);
        assert_eq!(status_world_m3(o).unwrap()["steps"], 70);
    }
}
