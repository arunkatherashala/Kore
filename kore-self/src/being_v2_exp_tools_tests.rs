//! Tool-level tests of Track B9 (experiments in World v2): opt-in through `being_run_v2`, gated by
//! controller.txt (refused and audited unless `experiments = on` with a budget), replay in the status.

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
    std::fs::write(dir.join("autonomy_policy.txt"), "[L2]\nworld_v2_run\n").unwrap();
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

fn exp_notes(owner: &str) -> usize {
    let log = log_tools::open(owner).unwrap();
    log.open_latest().rows::<EventRow>().into_iter().filter(|e| e.kind == "note" && e.tags.split(',').any(|t| t == "experiment")).count()
}

#[test]
fn experiments_need_the_controller_and_are_refused_and_audited_otherwise() {
    let o = "w2-exp-gates";
    let dir = setup(o);
    // controller without the B9 keys: the request is refused and audited, the run is passive
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 120\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 120, "world_seed": 4, "families": 12, "budget": 3000, "experiments": true}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert!(v["controller"]["experiments_run"].is_null(), "{t}");
    assert!(audit_tools(o).contains(&"controller_experiments".to_string()), "{:?}", audit_tools(o));
    assert_eq!(exp_notes(o), 0);
    // on, but without a budget: refused (the budget key), audited
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 120\nexperiments = on\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 20, "world_seed": 4, "experiments": true}));
    assert!(!e, "{t}");
    assert!(audit_tools(o).contains(&"controller_max_experiment_energy_pm".to_string()));
    assert_eq!(exp_notes(o), 0);
    // a wrong type is an error, nothing runs
    let (e, _) = call(o, "being_run_v2", json!({"steps": 20, "world_seed": 4, "experiments": "yes"}));
    assert!(e);
    // allowed with a budget: experiments run, are logged, and every decision replays
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 400\nexperiments = on\nmax_experiment_energy_pm = 100\nmax_experiments_per_run = 50\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 300, "world_seed": 4, "experiments": true}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    let n = v["controller"]["experiments_run"].as_u64().unwrap();
    assert!(n > 0 && n <= 50, "{t}");
    assert_eq!(exp_notes(o), n as usize);
    let (e, st) = call(o, "being_status_v2", json!({}));
    assert!(!e, "{st}");
    let s: Value = serde_json::from_str(&st).unwrap();
    assert_eq!(s["replay"]["experimenter_decisions"], 300);
    assert_eq!(s["replay"]["experimenter_decisions"], s["replay"]["experimenter_rederived_identically"]);
    assert_eq!(s["replay"]["rules"], s["replay"]["rules_rederived"]);
    // a KILL file refuses the experiment run before it starts
    std::fs::write(dir.join("KILL"), "").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 5, "world_seed": 4, "experiments": true}));
    assert!(e && t.contains("KILL"), "{t}");
    assert_eq!(exp_notes(o), n as usize);
}
