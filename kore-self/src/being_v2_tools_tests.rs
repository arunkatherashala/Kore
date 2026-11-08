//! Tool-level tests of World v2: controller.txt, authorize(), KILL, caps, audit rows.

use crate::authorize;
use crate::log_tools;
use crate::safety;
use crate::versioned::EventRow;
use serde_json::{json, Value};

fn setup(owner: &str) -> std::path::PathBuf {
    safety::test_support::init();
    let dir = safety::owner_dir(owner);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn call(owner: &str, tool: &str, args: Value) -> (bool, String) {
    let r = log_tools::handle(tool, &args, owner).expect("a log tool");
    (r["isError"].as_bool().unwrap_or(false), r["content"][0]["text"].as_str().unwrap().to_string())
}

fn audit_tools(owner: &str) -> Vec<String> {
    let log = log_tools::open(owner).unwrap();
    log.open_latest().rows::<EventRow>().into_iter().filter(|e| e.source == authorize::SOURCE).filter_map(|e| crate::being::state::tag(&e.tags, "tool").map(|s| s.to_string())).collect()
}

fn allow(dir: &std::path::Path) {
    std::fs::write(dir.join("autonomy_policy.txt"), "[L2]\nworld_v2_run\n").unwrap();
}

#[test]
fn world_v2_run_needs_controller_and_authorization_and_is_audited() {
    let o = "w2-gates";
    let dir = setup(o);
    // no controller.txt: fail closed, nothing created
    let (e, t) = call(o, "being_run_v2", json!({"steps": 5}));
    assert!(e && t.contains("fail closed"), "{t}");
    assert!(audit_tools(o).contains(&"controller_controller_closed".to_string()));
    let (_, st) = call(o, "being_status_v2", json!({}));
    assert!(st.contains("\"exists\":false"), "{st}");
    // controller present but world_v2_run is not in the autonomy policy: authorize() denies, audited
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 40\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 5}));
    assert!(e && t.contains("world_v2_run was not allowed"), "{t}");
    assert!(audit_tools(o).iter().any(|x| x == "world_v2_run"), "{:?}", audit_tools(o));
    let (_, st) = call(o, "being_status_v2", json!({}));
    assert!(st.contains("\"exists\":false"), "{st}");
    // allowed: the run happens, the steps are cut to the controller's cap, the cut is audited
    allow(&dir);
    let (e, t) = call(o, "being_run_v2", json!({"steps": 60, "world_seed": 3, "families": 8, "budget": 2000, "noise_pm": 50, "drift_per_10k": 20}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["steps_run"], 40);
    assert!(audit_tools(o).contains(&"controller_max_steps_per_run".to_string()));
    // the status folds the state from the log and every decision replays
    let (e, st) = call(o, "being_status_v2", json!({}));
    assert!(!e, "{st}");
    let s: Value = serde_json::from_str(&st).unwrap();
    assert_eq!(s["steps"], 40);
    assert_eq!(s["replay"]["decisions"], s["replay"]["rederived_identically"]);
    assert_eq!(s["replay"]["decisions"], 40);
    // a second run with a different world for the same being is refused
    let (e, t) = call(o, "being_run_v2", json!({"steps": 5, "world_seed": 4, "families": 8, "budget": 2000}));
    assert!(e && t.contains("keeps one world"), "{t}");
    // the original being's tools see no being: World v2 rows are a separate source
    let (_, st1) = call(o, "being_status", json!({}));
    assert!(st1.contains("\"exists\":false"), "{st1}");
}

#[test]
fn kill_file_refuses_a_world_v2_run_and_stops_one_midway() {
    let o = "w2-kill";
    let dir = setup(o);
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 500\n").unwrap();
    allow(&dir);
    std::fs::write(dir.join("KILL"), "").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 5}));
    assert!(e && t.contains("KILL"), "{t}");
    assert!(audit_tools(o).contains(&"controller_kill_switch".to_string()));
    let (_, st) = call(o, "being_status_v2", json!({}));
    assert!(st.contains("\"exists\":false"), "{st}");
    std::fs::remove_file(dir.join("KILL")).unwrap();
    // the file appears while the loop runs: no step starts after it
    use crate::being::policy::KillCheck;
    use crate::being::world2::engine::{run_v2, Run2Cfg};
    use crate::being::world2::world::W2Cfg;
    use crate::being::Policy;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let mut log = log_tools::open(o).unwrap();
    let calls = Arc::new(AtomicU64::new(0));
    let (c2, o2, d2) = (calls.clone(), o.to_string(), dir.clone());
    let mut policy = Policy::new(200);
    policy.kill = Some(KillCheck(Arc::new(move || {
        if c2.fetch_add(1, Ordering::SeqCst) + 1 == 9 {
            std::fs::write(d2.join("KILL"), "").unwrap();
        }
        authorize::kill_active(&o2)
    })));
    let cfg = Run2Cfg { world: Some(W2Cfg::new(2, 12, 3000)), discover_every: 10, ..Default::default() };
    let s = run_v2(&mut log, &cfg, &policy, None).unwrap();
    assert_eq!(s.steps_run, 8);
    assert_eq!(s.stopped, crate::being::StopReason::Killed);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().any(|e| e.source == crate::being::world2::world::SOURCE2 && e.tags.contains("controller_stop") && e.tags.contains("stop:kill_switch")));
}

#[test]
fn energy_cap_of_the_controller_stops_a_world_v2_run() {
    let o = "w2-energy";
    let dir = setup(o);
    std::fs::write(dir.join("controller.txt"), "max_energy_per_run = 80\n").unwrap();
    allow(&dir);
    let (e, t) = call(o, "being_run_v2", json!({"steps": 500, "world_seed": 5}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["stopped"], "energy cap of the controller");
    let spent = v["energy_spent_this_run"].as_i64().unwrap();
    assert!((80..110).contains(&spent), "{spent}");
    assert!(audit_tools(o).contains(&"controller_max_energy_per_run".to_string()));
}

#[test]
fn the_b13_chooser_needs_its_controller_key_and_is_fixed_at_creation() {
    // not allowed (key missing): the run happens with the B1 estimate, the request is refused and audited
    let o = "w2-b13-off";
    let dir = setup(o);
    allow(&dir);
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 60\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 30, "world_seed": 2, "families": 6, "budget": 1500, "chooser": "context"}));
    assert!(!e, "{t}");
    assert!(audit_tools(o).contains(&"controller_context_chooser".to_string()), "{:?}", audit_tools(o));
    let rows: Vec<EventRow> = log_tools::open(o).unwrap().open_latest().rows();
    assert!(rows.iter().any(|e| e.tags.contains("w2_genesis") && !e.tags.contains("w2_lin")));
    // a bad value makes controller.txt invalid: fail closed, the run is refused
    let o = "w2-b13-bad";
    let dir = setup(o);
    allow(&dir);
    std::fs::write(dir.join("controller.txt"), "context_chooser = maybe\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 5, "chooser": "context"}));
    assert!(e && t.contains("fail closed"), "{t}");
    // allowed: the being is created with the switch, every decision replays from the log
    let o = "w2-b13-on";
    let dir = setup(o);
    allow(&dir);
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 60\ncontext_chooser = on\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 40, "world_seed": 2, "families": 6, "budget": 1500, "chooser": "context"}));
    assert!(!e, "{t}");
    assert!(!audit_tools(o).contains(&"controller_context_chooser".to_string()));
    let rows: Vec<EventRow> = log_tools::open(o).unwrap().open_latest().rows();
    assert!(rows.iter().any(|e| e.tags.contains("w2_genesis") && e.tags.contains("w2_lin:")));
    let (e, st) = call(o, "being_status_v2", json!({}));
    assert!(!e, "{st}");
    let s: Value = serde_json::from_str(&st).unwrap();
    assert_eq!(s["replay"]["decisions"], 40);
    assert_eq!(s["replay"]["decisions"], s["replay"]["rederived_identically"]);
    // a later plain run keeps the switch the being was created with
    let (e, t) = call(o, "being_run_v2", json!({"steps": 10, "world_seed": 2, "families": 6, "budget": 1500}));
    assert!(!e, "{t}");
    let (_, st) = call(o, "being_status_v2", json!({}));
    let s: Value = serde_json::from_str(&st).unwrap();
    assert_eq!(s["replay"]["decisions"], 50);
    assert_eq!(s["replay"]["decisions"], s["replay"]["rederived_identically"]);
    // a being created without it cannot get it later
    let o2 = "w2-b13-late";
    let dir = setup(o2);
    allow(&dir);
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 60\ncontext_chooser = on\n").unwrap();
    let (e, _) = call(o2, "being_run_v2", json!({"steps": 5, "world_seed": 2, "families": 6, "budget": 1500}));
    assert!(!e);
    let (e, t) = call(o2, "being_run_v2", json!({"steps": 5, "world_seed": 2, "families": 6, "budget": 1500, "chooser": "context"}));
    assert!(!e, "{t}");
    assert!(audit_tools(o2).contains(&"controller_fixed_at_creation".to_string()), "{:?}", audit_tools(o2));
    let _ = dir;
}
