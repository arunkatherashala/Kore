//! Tool-level tests of the controller (Track B5): controller.txt, KILL, caps, audit rows.

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

fn audit_rows(owner: &str) -> Vec<EventRow> {
    let log = log_tools::open(owner).unwrap();
    log.open_latest().rows::<EventRow>().into_iter().filter(|e| e.source == authorize::SOURCE).collect()
}

fn tools_of(rows: &[EventRow]) -> Vec<String> {
    rows.iter().filter_map(|e| crate::being::state::tag(&e.tags, "tool").map(|s| s.to_string())).collect()
}

#[test]
fn missing_or_unparseable_controller_fails_closed_and_is_audited() {
    let o = "b5-closed";
    let dir = setup(o);
    let (e, t) = call(o, json!({"steps": 5, "chooser": "model"}));
    assert!(e && t.contains("fail closed") && t.contains("controller.txt"), "{t}");
    // nothing was created
    let st = log_tools::handle("being_status", &json!({}), o).unwrap();
    assert!(st["content"][0]["text"].as_str().unwrap().contains("\"exists\":false"));
    assert!(tools_of(&audit_rows(o)).contains(&"controller_controller_closed".to_string()), "{:?}", tools_of(&audit_rows(o)));
    // unparseable: same
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = lots\n").unwrap();
    let (e, t) = call(o, json!({"steps": 5, "chooser": "model"}));
    assert!(e && t.contains("fail closed") && t.contains("not valid"), "{t}");
    assert_eq!(tools_of(&audit_rows(o)).iter().filter(|x| x.as_str() == "controller_controller_closed").count(), 2);
}

#[test]
fn controller_txt_is_reserved_for_every_tool_path() {
    let o = "b5-reserved";
    setup(o);
    for f in ["controller.txt", "Controller.TXT", "controller.txt.", "controller.txt.bak", "controller.txt::$DATA", "controller"] {
        let e = safety::safe_path(o, f).unwrap_err();
        assert!(e.contains("reserved"), "{f}: {e}");
    }
    assert!(safety::HAND_EDITED_FILES.contains(&"controller.txt"));
    assert!(safety::is_hand_edited_name("controller.txt"));
    assert!(safety::safe_path(o, "notes.txt").is_ok());
}

#[test]
fn caps_are_enforced_by_the_tool_and_refusals_are_audited() {
    let o = "b5-caps";
    let dir = setup(o);
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 10\nmax_energy_per_run = 100000\n").unwrap();
    let (e, t) = call(o, json!({"steps": 40, "chooser": "model", "world_seed": 3, "budget": 3000}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["steps_run"], 10);
    assert!(t.contains("max_steps_per_run"));
    let tools = tools_of(&audit_rows(o));
    assert!(tools.contains(&"controller_max_steps_per_run".to_string()), "{tools:?}");
    // composition is off by default in controller.txt: the request is refused, audited, and no composed rule appears
    let (e, t) = call(o, json!({"steps": 5, "chooser": "model", "compose": true, "discover": true}));
    assert!(e || t.contains("composition"), "{t}");
    let tools = tools_of(&audit_rows(o));
    assert!(tools.contains(&"controller_composition".to_string()), "{tools:?}");
}

#[test]
fn energy_cap_from_controller_stops_the_run() {
    let o = "b5-energy";
    let dir = setup(o);
    std::fs::write(dir.join("controller.txt"), "max_energy_per_run = 60\n").unwrap();
    let (e, t) = call(o, json!({"steps": 500, "chooser": "model", "world_seed": 3, "budget": 3000}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["stopped"], "energy cap of the controller");
    let spent = v["energy_spent_this_run"].as_i64().unwrap();
    assert!((60..90).contains(&spent), "{spent}");
    assert!(tools_of(&audit_rows(o)).contains(&"controller_max_energy_per_run".to_string()));
}

#[test]
fn kill_file_refuses_a_run_and_stops_one_midway_within_one_step() {
    let o = "b5-kill";
    let dir = setup(o);
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 500\n").unwrap();
    // a run first, so that the being exists
    let (e, t) = call(o, json!({"steps": 5, "chooser": "model", "world_seed": 3, "budget": 3000}));
    assert!(!e, "{t}");
    // KILL file present at the start: refused, nothing runs, audited
    std::fs::write(dir.join("KILL"), "").unwrap();
    let (e, t) = call(o, json!({"steps": 5}));
    assert!(e && t.contains("KILL"), "{t}");
    assert!(tools_of(&audit_rows(o)).contains(&"controller_kill_switch".to_string()));
    std::fs::remove_file(dir.join("KILL")).unwrap();

    // mid-run: the file appears while the loop runs; the next check stops it
    use crate::being::policy::KillCheck;
    use crate::being::run_loop::{run_advised, RunCfg};
    use crate::being::{Policy, ScriptedThinker};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let mut log = log_tools::open(o).unwrap();
    let calls = Arc::new(AtomicU64::new(0));
    let (c2, o2, d2) = (calls.clone(), o.to_string(), dir.clone());
    let mut policy = Policy::new(200);
    policy.kill = Some(KillCheck(Arc::new(move || {
        if c2.fetch_add(1, Ordering::SeqCst) + 1 == 8 {
            std::fs::write(d2.join("KILL"), "").unwrap(); // the owner creates the file while the loop runs
        }
        authorize::kill_active(&o2)
    })));
    let s = run_advised(&mut log, &RunCfg { steps: 200, ..Default::default() }, &ScriptedThinker, &policy, None).unwrap();
    assert_eq!(s.steps_run, 7, "the file appeared at the 8th check: no step may start after it");
    assert_eq!(s.stopped, crate::being::StopReason::Killed);
}
