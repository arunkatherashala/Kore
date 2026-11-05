//! Tool-level tests of the B3b learner (`adapt_mode: switch`): the controller caps, the KILL file and the audit rows apply unchanged.

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

fn call(owner: &str, args: Value) -> (bool, String) {
    let r = log_tools::handle("being_run", &args, owner).expect("being_run");
    (r["isError"].as_bool().unwrap_or(false), r["content"][0]["text"].as_str().unwrap().to_string())
}

fn audited_tools(owner: &str) -> Vec<String> {
    let log = log_tools::open(owner).unwrap();
    log.open_latest().rows::<EventRow>().into_iter().filter(|e| e.source == authorize::SOURCE).filter_map(|e| crate::being::state::tag(&e.tags, "tool").map(|s| s.to_string())).collect()
}

#[test]
fn b3b_runs_through_the_tool_under_the_controller_and_the_kill_file() {
    let o = "b3b-tool";
    let dir = setup(o);
    // no controller.txt: fail closed, audited, nothing created
    let (e, t) = call(o, json!({"steps": 5, "chooser": "model", "drift": true, "adapt": true, "adapt_mode": "switch"}));
    assert!(e && t.contains("fail closed"), "{t}");
    assert!(audited_tools(o).contains(&"controller_controller_closed".to_string()));
    // a step cap: the run is cut and the refusal audited
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 10\n").unwrap();
    let (e, t) = call(o, json!({"steps": 40, "chooser": "model", "drift": true, "adapt": true, "adapt_mode": "switch", "world_seed": 3, "budget": 3000}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["steps_run"], 10);
    assert!(audited_tools(o).contains(&"controller_max_steps_per_run".to_string()));
    let st = log_tools::handle("being_status", &json!({}), o).unwrap();
    assert!(st["content"][0]["text"].as_str().unwrap().contains("\"adapt\":\"switch\""), "{st}");
    // the KILL file refuses the next run and is audited
    std::fs::write(dir.join("KILL"), "").unwrap();
    let (e, t) = call(o, json!({"steps": 5}));
    assert!(e && t.contains("KILL"), "{t}");
    assert!(audited_tools(o).contains(&"controller_kill_switch".to_string()));
    std::fs::remove_file(dir.join("KILL")).unwrap();
    // the mode is fixed once the being exists, and an unknown mode is an error
    let (e, _) = call(o, json!({"steps": 1, "adapt_mode": "full"}));
    assert!(e);
    let (e, t) = call(o, json!({"steps": 1, "adapt_mode": "forget"}));
    assert!(e && t.contains("adapt_mode"), "{t}");
}
