//! The gate of a World T run (tool layer): the owner's controller, the kill switch, `authorize()`, the
//! sandbox on disk, and the logged run. In-process only (like World O there is no MCP tool yet).
//!
//! Order of the checks, each refusal audited (source `authorize`):
//! 1. `controller.txt` must exist and parse (fail closed), and contain `world_t_run = on` (default off);
//! 2. the `KILL` file must not exist (and is checked again before every step);
//! 3. `authorize()` must allow the internal tool `world_t_run` (tier L2: it appends to the being's own
//!    log and reads its sandbox; the owner lists it under `[L2]` in `autonomy_policy.txt`; default deny).
//! Then the pool is written into the sandbox (`world_t_sandbox`), and the being runs with the
//! controller's caps on steps, energy, live rules and search candidates. World T has no L3 action:
//! nothing leaves the sandbox and the log.

#![allow(dead_code)]

use crate::authorize::{self, Outcome, BEING_ACTOR};
use crate::being::controller::{self, Controller, Origin};
use crate::being::policy::{KillCheck, Policy};
use crate::being::world_t::ctl;
use crate::being::world_t::engine::{replay_t, run_t, RunTCfg, StateT, DEFAULT_DISCOVER_EVERY};
use crate::being::world_t::learner::LearnerTCfg;
use crate::being::world_t::world::{WTCfg, WorldT, DEFAULT_BUDGET};
use crate::versioned::EventRow;
use crate::world_t_sandbox::{write_pool, DirSandbox};
use serde_json::{json, Value};

pub const AUTH_TOOL: &str = "world_t_run";
pub const MAX_STEPS: u64 = 5000;

fn controller_text(owner: &str) -> Option<String> {
    let p = crate::safety::owner_dir(owner).join(controller::FILE);
    let meta = std::fs::metadata(&p).ok()?;
    if meta.len() > 16 * 1024 || authorize::perm_problem(&p).is_some() {
        return None;
    }
    std::fs::read(&p).ok().and_then(|b| String::from_utf8(b).ok())
}

fn audit(owner: &str, cap: &str, limit: u64, requested: u64, what: &str) {
    let _ = authorize::audit_controller_locked(owner, cap, limit, requested, what);
}

/// Run the World T being of `owner` for `steps` steps (creating it in world `seed` with `budget` energy).
pub fn run_world_t(owner: &str, steps: u64, seed: u64, budget: Option<i64>) -> Result<Value, String> {
    if !(1..=MAX_STEPS).contains(&steps) {
        return Err(format!("'steps' must be 1..{MAX_STEPS}"));
    }
    let text = controller_text(owner);
    let ctl0 = text.as_deref().map(Controller::parse_or_closed).unwrap_or_else(|| Controller::fail_closed(format!("{} is missing", controller::FILE)));
    if ctl0.is_closed() {
        let reason = match &ctl0.origin {
            Origin::FailClosed(r) => r.clone(),
            Origin::File => String::new(),
        };
        audit(owner, "controller_closed", 0, steps, &reason);
        return Err(format!("controller refused the World T run (fail closed): {reason}"));
    }
    if let Err(why) = ctl::allowed(text.as_deref()) {
        audit(owner, ctl::KEY, 0, steps, &why);
        return Err(format!("controller refused the World T run: {why}"));
    }
    if authorize::kill_active(owner) {
        audit(owner, "kill_switch", 0, steps, "the KILL file exists: the World T run was refused before it started");
        return Err("the KILL file exists in the owner directory: the being does not run".into());
    }
    let d = authorize::authorize_locked(owner, BEING_ACTOR, AUTH_TOOL, &json!({ "steps": steps, "world_seed": seed }));
    if d.outcome != Outcome::Allow {
        return Err(format!("{AUTH_TOOL} was not allowed ({}): {}. To allow World T runs, list {AUTH_TOOL} under [L2] in autonomy_policy.txt (edit that file by hand)", d.outcome.name(), d.reason));
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
    let wcfg = st0.cfg.clone().unwrap_or_else(|| WTCfg::new(seed, budget.unwrap_or(DEFAULT_BUDGET)));
    let world = WorldT::new(wcfg.clone());
    write_pool(owner, &world)?;
    let sb = DirSandbox::new(owner, wcfg.seed);
    let mut policy = Policy::new(steps_eff);
    policy.max_energy = Some(ctl0.max_energy_per_run);
    let owner_k = owner.to_string();
    policy.kill = Some(KillCheck(std::sync::Arc::new(move || authorize::kill_active(&owner_k))));
    let cfg = RunTCfg { world: Some(wcfg), learner: Some(LearnerTCfg::default()), discover_every: DEFAULT_DISCOVER_EVERY, ctl: Some(limits), sql: Some(ctl::sql_limits(text.as_deref())) };
    let s = run_t(&mut log, &cfg, &policy, None, &sb).map_err(|e| e.to_string())?;
    for r in &s.refusals {
        refused.push((r.cap.clone(), r.limit, r.requested, r.what.clone()));
    }
    if s.stopped == crate::being::StopReason::Killed {
        refused.push(("kill_switch".into(), 0, s.steps_run, format!("the KILL file stopped the World T run after {} steps", s.steps_run)));
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

/// Fold the World T state from the owner's log and replay every decision (read-only).
pub fn status_world_t(owner: &str) -> Result<Value, String> {
    let log = crate::log_tools::open(owner).map_err(|v| v.to_string())?;
    let rows: Vec<EventRow> = log.open_latest().rows();
    let st = StateT::from_events(&rows);
    let (n, bad) = replay_t(&rows);
    Ok(json!({ "exists": st.cfg.is_some(), "steps": st.step, "reward": st.total_reward, "energy": st.total_cost, "rules": st.rules.len(), "decisions_replayed": n, "replay_mismatches": bad }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety;

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

    #[test]
    fn a_world_t_run_needs_the_controller_key_no_kill_file_and_authorize() {
        let o = "wt-gates";
        let dir = setup(o);
        // no controller.txt: fail closed
        let e = run_world_t(o, 5, 3, Some(800)).unwrap_err();
        assert!(e.contains("fail closed"), "{e}");
        // a controller without the key: World T is off by default, audited
        std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 40\n").unwrap();
        let e = run_world_t(o, 5, 3, Some(800)).unwrap_err();
        assert!(e.contains("world_t_run"), "{e}");
        assert!(audit_tools(o).iter().any(|t| t.contains("world_t_run")), "{:?}", audit_tools(o));
        // an invalid query limit makes the whole controller invalid: fail closed
        std::fs::write(dir.join("controller.txt"), "world_t_run = on\nworld_t_sql_max_rows = lots\n").unwrap();
        assert!(run_world_t(o, 5, 3, Some(800)).unwrap_err().contains("fail closed"));
        // the key, but not in the autonomy policy: authorize() denies (default deny)
        std::fs::write(dir.join("controller.txt"), "world_t_run = on\nmax_steps_per_run = 40\nworld_t_sql_timeout_ms = 1500\nworld_t_sql_max_rows = 50\n").unwrap();
        let e = run_world_t(o, 5, 3, Some(800)).unwrap_err();
        assert!(e.contains("world_t_run was not allowed"), "{e}");
        assert!(!status_world_t(o).unwrap()["exists"].as_bool().unwrap());
        // the KILL file: refused before anything runs
        std::fs::write(dir.join("autonomy_policy.txt"), "[L2]\nworld_t_run\n").unwrap();
        std::fs::write(dir.join("KILL"), "").unwrap();
        assert!(run_world_t(o, 5, 3, Some(800)).unwrap_err().contains("KILL"));
        std::fs::remove_file(dir.join("KILL")).unwrap();
        // allowed: the steps are cut to the cap (audited), the sandbox holds the pool, the log replays
        let v = run_world_t(o, 60, 3, Some(800)).unwrap();
        assert_eq!(v["steps_run"], 40);
        assert!(audit_tools(o).iter().any(|t| t.contains("max_steps_per_run")));
        let sb = dir.join("world_t").join("seed_3");
        let n = std::fs::read_dir(&sb).unwrap().count();
        assert!(n > 100, "{n} files in the sandbox");
        let s = status_world_t(o).unwrap();
        assert_eq!(s["steps"], 40);
        assert_eq!(s["decisions_replayed"], 40);
        assert_eq!(s["replay_mismatches"], 0);
        // the query limits of the controller are the ones logged with every query
        let log = crate::log_tools::open(o).unwrap();
        let rows: Vec<EventRow> = log.open_latest().rows();
        let q: Vec<&EventRow> = rows.iter().filter(|e| e.kind == "action" && e.text.contains("SQL #0")).collect();
        assert!(q.iter().all(|e| e.tags.contains("sql_timeout_ms:1500") && e.tags.contains("sql_max_rows:50")));
        // a second run continues the same being in the same world
        let v = run_world_t(o, 10, 99, None).unwrap();
        assert_eq!(v["steps_run"], 10);
        assert_eq!(status_world_t(o).unwrap()["steps"], 50);
    }

    #[test]
    fn the_sandbox_reads_only_plain_names_inside_the_owner_directory() {
        let o = "wt-sandbox";
        let _ = setup(o);
        let w = WorldT::new(WTCfg::new(5, 600));
        let (n, _) = write_pool(o, &w).unwrap();
        assert_eq!(n, w.files.len());
        let sb = DirSandbox::new(o, 5);
        use crate::being::world_t::engine::Sandbox;
        assert_eq!(sb.read(&w.files[0].name).unwrap(), w.files[0].bytes);
        for bad in ["../controller.txt", "..", "a/b.csv", "C:x.csv", "a\\b.csv", "/etc/passwd", ".hidden", "KILL/../x"] {
            assert!(sb.read(bad).is_err(), "{bad} was read");
        }
        crate::world_t_sandbox::remove_pool(o, &w).unwrap();
        assert!(!safety::owner_dir(o).join("world_t").join("seed_5").exists());
    }
}
