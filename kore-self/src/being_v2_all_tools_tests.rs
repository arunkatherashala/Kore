//! Tool-level tests of Track B10: the all-skills profile through `being_run_v2` (each part needs its controller
//! key, refusals audited, KILL refuses), and the read-only observer `being_observe` (log only, writes nothing,
//! allowed by authorize() for the chat thinker only when the owner lists it, and while KILL exists).

use crate::authorize::{self, Outcome};
use crate::log_tools;
use crate::safety;
use crate::versioned::EventRow;
use serde_json::{json, Value};

fn setup(owner: &str) -> std::path::PathBuf {
    safety::test_support::init();
    let dir = safety::owner_dir(owner);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("autonomy_policy.txt"), "[L1]\nbeing_observe\n[L2]\nworld_v2_run\n").unwrap();
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

fn version(owner: &str) -> u64 {
    log_tools::open(owner).unwrap().latest_version()
}

#[test]
fn all_skills_profile_is_granted_part_by_part_and_audited() {
    let o = "w2-all-gates";
    let dir = setup(o);
    // controller without the B10 keys: the run happens with every part off, each request refused and audited
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 60\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 40, "world_seed": 5, "families": 12, "budget": 3000, "drift_per_10k": 60, "profile": "all_skills"}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["profile"]["rules_use"], false);
    assert!(v["profile"]["experiments"].is_null() && v["profile"]["lookahead"].is_null());
    let au = audit_tools(o);
    for cap in ["controller_rules_use", "controller_adaptation", "controller_rule_retirement", "controller_experiments", "controller_lookahead"] {
        assert!(au.iter().any(|x| x == cap), "{cap} not audited: {au:?}");
    }
    // a bad profile name is an error, nothing runs
    let v0 = version(o);
    let (e, _) = call(o, "being_run_v2", json!({"steps": 5, "profile": "everything"}));
    assert!(e);
    assert_eq!(version(o), v0);
    // a new owner with every key on: everything is on and logged
    let o = "w2-all-on";
    let dir = setup(o);
    std::fs::write(dir.join("controller.txt"), "max_steps_per_run = 300\nrules_use = on\nadaptation = on\nrule_retirement = on\nexperiments = on\nmax_experiment_energy_pm = 50\nmax_experiments_per_run = 100\nlookahead = on\n").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 200, "world_seed": 5, "families": 12, "budget": 3000, "noise_pm": 100, "drift_per_10k": 60, "profile": "all_skills"}));
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["steps_run"], 200);
    assert!(v["profile"]["rules_use"] == true && v["profile"]["adaptation"] == true && v["profile"]["rule_retirement"] == true);
    assert!(v["profile"]["lookahead_decisions"].as_u64().unwrap() > 0);
    // the status replays the rules; the observer reports the run, read-only
    let (e, st) = call(o, "being_status_v2", json!({}));
    assert!(!e, "{st}");
    let s: Value = serde_json::from_str(&st).unwrap();
    assert_eq!(s["replay"]["rules"], s["replay"]["rules_rederived"]);
    let v0 = version(o);
    let (e, md) = call(o, "being_observe", json!({"window": 50, "max_items": 3}));
    assert!(!e, "{md}");
    assert!(md.contains("## Over time") && md.contains("all-skills profile for this run") && md.contains("Plans of the lookahead planner"), "{md}");
    let (e, js) = call(o, "being_observe", json!({"format": "json"}));
    assert!(!e);
    let j: Value = serde_json::from_str(&js).unwrap();
    assert_eq!(j["steps"], 200);
    assert_eq!(version(o), v0, "the observer wrote to the log");
    let (e, _) = call(o, "being_observe", json!({"format": "pdf"}));
    assert!(e);
    // KILL: the run is refused and audited; the observer still answers (read-only)
    std::fs::write(dir.join("KILL"), "").unwrap();
    let (e, t) = call(o, "being_run_v2", json!({"steps": 5, "profile": "all_skills"}));
    assert!(e && t.contains("KILL"), "{t}");
    let (e, _) = call(o, "being_observe", json!({}));
    assert!(!e);
}

#[test]
fn the_observer_goes_through_authorize_for_the_thinker() {
    let o = "w2-observe-auth";
    let dir = setup(o);
    assert_eq!(authorize::authorize(o, "thinker", "being_observe", &json!({})).outcome, Outcome::Allow);
    // allowed while the kill switch is on (read-only)
    std::fs::write(dir.join("KILL"), "").unwrap();
    assert_eq!(authorize::authorize(o, "thinker", "being_observe", &json!({})).outcome, Outcome::Allow);
    std::fs::remove_file(dir.join("KILL")).unwrap();
    // not listed by the owner: denied, and the denial is audited
    std::fs::write(dir.join("autonomy_policy.txt"), "[L1]\nbeing_status\n").unwrap();
    assert_eq!(authorize::authorize(o, "thinker", "being_observe", &json!({})).outcome, Outcome::Deny);
    assert!(audit_tools(o).iter().any(|t| t == "being_observe"));
    // no policy file: denied
    std::fs::remove_file(dir.join("autonomy_policy.txt")).unwrap();
    assert_eq!(authorize::authorize(o, "thinker", "being_observe", &json!({})).outcome, Outcome::Deny);
}
