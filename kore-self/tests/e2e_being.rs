//! End to end: the being loop through the real binary (stdio MCP) against a temp data root.
//! being_run (300 steps), being_status, being_explain on one decision, self_sql over the
//! events to show domain counts, self_verify, then a byte flip that verify must catch.
//! Track B2: being_rule_discovery_end_to_end runs a being in the extended pattern world with
//! discovery on, lists its rules (being_rules), explains a decision answered by a rule, verifies
//! and flips a byte.
#![allow(dead_code)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    out: BufReader<ChildStdout>,
    next_id: u64,
}

impl Server {
    fn start(root: &Path, fake_home: &Path, owner: &str) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kore-self"))
            .arg(owner)
            .env("KORE_SELF_HOME", root)
            .env("HOME", fake_home)
            .env("USERPROFILE", fake_home)
            .env_remove("KORE_SELF_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn kore-self");
        let stdin = child.stdin.take();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server { child, stdin, out, next_id: 1 };
        s.rpc("initialize", json!({}));
        s
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin.as_mut().unwrap(), "{line}").unwrap();
        self.stdin.as_mut().unwrap().flush().unwrap();
        let mut resp = String::new();
        self.out.read_line(&mut resp).unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap_or_else(|e| panic!("bad response {resp:?}: {e}"));
        assert_eq!(v["id"], id);
        v
    }

    fn call(&mut self, tool: &str, args: Value) -> (bool, String) {
        let v = self.rpc("tools/call", json!({"name": tool, "arguments": args}));
        let r = &v["result"];
        (r["isError"].as_bool().unwrap_or(false), r["content"][0]["text"].as_str().unwrap_or("").to_string())
    }

    fn ok(&mut self, tool: &str, args: Value) -> String {
        let (e, t) = self.call(tool, args.clone());
        assert!(!e, "{tool} {args} failed: {t}");
        t
    }

    fn json(&mut self, tool: &str, args: Value) -> Value {
        serde_json::from_str(&self.ok(tool, args)).unwrap()
    }

    fn stop(mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn say(label: &str, text: &str) {
    println!("--- {label}
{text}");
}

fn short(t: &str, n: usize) -> String {
    if t.len() > n {
        let mut c = n;
        while !t.is_char_boundary(c) {
            c -= 1;
        }
        format!("{}...", &t[..c])
    } else {
        t.to_string()
    }
}

#[test]
fn being_loop_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2eb";
    let mut s = Server::start(root.path(), fake_home.path(), owner);

    let (e, t) = s.call("being_run", json!({"steps": 0}));
    assert!(e, "{t}");
    let run = s.json("being_run", json!({"steps": 300, "seed": 1, "world_seed": 3, "budget": 3000}));
    say("being_run {steps:300, seed:1, world_seed:3, budget:3000}", &serde_json::to_string_pretty(&run).unwrap());
    assert_eq!(run["steps_run"], 300);
    assert_eq!(run["stopped"], "step limit reached");

    let st = s.json("being_status", json!({}));
    let mut shown = st.clone();
    shown["last_decisions"] = json!(st["last_decisions"].as_array().unwrap().iter().take(1).map(|d| json!({"decision_event_id": d["decision_event_id"], "step": d["step"], "domain": d["domain"], "text": d["text"]})).collect::<Vec<_>>());
    say("being_status (last_decisions cut to one, reasons omitted)", &serde_json::to_string_pretty(&shown).unwrap());
    assert_eq!(st["steps"], 300);
    assert_eq!(st["replay_check"]["decisions"], 300);
    assert_eq!(st["replay_check"]["re_derived_from_log_and_equal"], 300);

    let id = st["last_decisions"][0]["decision_event_id"].as_str().unwrap().to_string();
    let ex = s.json("being_explain", json!({"decision_event_id": id}));
    let mut shown = ex.clone();
    shown["evidence_events_cited"] = json!(ex["evidence_events_cited"].as_array().unwrap().iter().take(3).map(|e| json!({"event_id": short(e["event_id"].as_str().unwrap(), 12), "text": short(e["text"].as_str().unwrap(), 90)})).collect::<Vec<_>>());
    say("being_explain {decision_event_id} (evidence cut to 3 of the cited events)", &serde_json::to_string_pretty(&shown).unwrap());
    assert_eq!(ex["replay_check"]["equal_to_recorded"], true);
    assert!(ex["evidence_events_cited"].as_array().unwrap().len() >= 2);
    assert!(ex["reasons_and_alternatives"]["alternatives"].as_array().unwrap().len() == 4);

    let sql = s.ok("self_sql", json!({"sql": "SELECT kind, COUNT(*) AS n FROM events WHERE source = 'being:loop' GROUP BY kind ORDER BY kind"}));
    say("self_sql: events by kind", &sql);
    assert!(sql.contains("decision") && sql.contains("300"));
    let sql2 = s.ok("self_sql", json!({"sql": "SELECT tags, COUNT(*) AS n FROM events WHERE kind = 'decision' GROUP BY tags ORDER BY n DESC LIMIT 3"}));
    let _ = sql2;
    let by_domain = s.ok("self_sql", json!({"sql": "SELECT r.stance, COUNT(*) AS decisions FROM revisions r WHERE r.entity_type = 'decision' GROUP BY r.stance ORDER BY decisions DESC"}));
    say("self_sql: decisions per chosen domain", &by_domain);
    let preds = s.ok("self_sql", json!({"sql": "SELECT o.actual, COUNT(*) AS n FROM outcomes o GROUP BY o.actual ORDER BY o.actual"}));
    say("self_sql: outcomes rows", &preds);

    // continue the same being: seeds are fixed once it exists
    let (e, t) = s.call("being_run", json!({"steps": 5, "world_seed": 4}));
    assert!(e, "{t}");
    say("being_run with a different world_seed", &t);

    let ver = s.ok("self_verify", json!({}));
    say("self_verify", &ver);
    assert!(ver.starts_with("OK"));
    s.stop();

    let seg_dir = root.path().join(owner).join("log").join("events");
    let seg = std::fs::read_dir(&seg_dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).max().unwrap();
    let good = std::fs::read(&seg).unwrap();
    let mut bad = good.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 1;
    std::fs::write(&seg, &bad).unwrap();
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let (e, t) = s.call("self_verify", json!({}));
    say("self_verify after flipping a byte", &t);
    assert!(e && t.contains("VERIFY FAILED"), "{t}");
    s.stop();
    std::fs::write(&seg, good).unwrap();
}

#[test]
fn being_model_chooser_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2em";
    let mut s = Server::start(root.path(), fake_home.path(), owner);

    let (e, t) = s.call("being_run", json!({"steps": 5, "chooser": "bogus"}));
    assert!(e, "{t}");
    let run = s.json("being_run", json!({"steps": 200, "seed": 1, "world_seed": 3, "budget": 3000, "chooser": "model"}));
    let mut shown = run.clone();
    shown["domains"] = json!("(per-domain table cut)");
    say("being_run {steps:200, seed:1, world_seed:3, budget:3000, chooser:\"model\"}", &serde_json::to_string_pretty(&shown).unwrap());
    assert_eq!(run["steps_run"], 200);
    assert_eq!(run["chooser"], "model");

    let st = s.json("being_status", json!({}));
    assert_eq!(st["chooser"], "model");
    assert_eq!(st["replay_check"]["decisions"], 200);
    assert_eq!(st["replay_check"]["re_derived_from_log_and_equal"], 200);

    let id = st["last_decisions"][0]["decision_event_id"].as_str().unwrap().to_string();
    let ex = s.json("being_explain", json!({"decision_event_id": id}));
    let alts = ex["reasons_and_alternatives"]["alternatives"].as_array().unwrap();
    assert_eq!(alts.len(), 12);
    let mut shown = ex.clone();
    shown["evidence_events_cited"] = json!("(cut)");
    shown["reasons_and_alternatives"]["alternatives"] = json!(alts.iter().take(3).collect::<Vec<_>>());
    say("being_explain on the last decision (3 of 12 alternatives shown)", &serde_json::to_string_pretty(&shown).unwrap());
    assert_eq!(ex["replay_check"]["equal_to_recorded"], true);
    assert!(alts[0]["p_success"].is_number() && alts[0]["uncertainty_sd"].is_number());

    // the chooser is kept once the being exists
    let (e, t) = s.call("being_run", json!({"steps": 1, "chooser": "ucb"}));
    assert!(e && t.contains("chooser"), "{t}");
    let more = s.json("being_run", json!({"steps": 10}));
    assert_eq!(more["chooser"], "model");
    let ver = s.ok("self_verify", json!({}));
    assert!(ver.starts_with("OK"));
    s.stop();
}

#[test]
fn being_rule_discovery_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2ed";
    let mut s = Server::start(root.path(), fake_home.path(), owner);

    // option checks
    let (e, t) = s.call("being_run", json!({"steps": 5, "pattern_families": "extended"}));
    assert!(e && t.contains("chooser model"), "{t}");
    let (e, t) = s.call("being_run", json!({"steps": 5, "discover": true}));
    assert!(e && t.contains("pattern_families extended"), "{t}");
    let (e, t) = s.call("being_run", json!({"steps": 5, "discover_every": 5}));
    assert!(e, "{t}");
    let (e, t) = s.call("being_rules", json!({}));
    assert!(!e && t.contains("\"exists\":false"), "{t}");

    let run = s.json("being_run", json!({"steps": 700, "seed": 1, "world_seed": 6, "budget": 60000, "chooser": "model", "pattern_families": "extended", "discover": true, "discover_every": 10}));
    let mut shown = run.clone();
    shown["domains"] = json!("(per-domain table cut)");
    say("being_run {steps:700, seed:1, world_seed:6, budget:60000, chooser:\"model\", pattern_families:\"extended\", discover:true, discover_every:10}", &serde_json::to_string_pretty(&shown).unwrap());
    assert_eq!(run["steps_run"], 700);
    assert_eq!(run["pattern_families"], "extended");
    assert!(run["rules_total"].as_u64().unwrap() >= 1);

    let rules = s.json("being_rules", json!({}));
    let list = rules["rules"].as_array().unwrap();
    let mut shown = rules.clone();
    shown["rules"] = json!(list.iter().take(2).map(|r| {
        let mut r = r.clone();
        r["cited_sample"] = json!(r["cited_sample"].as_array().unwrap().iter().take(1).map(|c| json!({"terms": c["terms"], "revealed_next": c["revealed_next"]})).collect::<Vec<_>>());
        r
    }).collect::<Vec<_>>());
    say("being_rules (first 2 rules, cited sample cut to 1)", &serde_json::to_string_pretty(&shown).unwrap());
    assert_eq!(list.len() as u64, run["rules_total"].as_u64().unwrap());
    for r in list {
        assert_eq!(r["rederived_from_log"], true);
        assert!(r["cited_example_events"].as_u64().unwrap() >= 4);
        assert!(r["dsl"].as_str().unwrap().starts_with("next = "));
        assert_eq!(r["revision"]["author"], "rule:being-discover");
    }
    assert!(list.iter().any(|r| r["times_used_in_attempts"].as_u64().unwrap() > 0), "a rule was used");

    let st = s.json("being_status", json!({}));
    assert_eq!(st["pattern_families"], "extended");
    assert_eq!(st["replay_check"]["decisions"], 700);
    assert_eq!(st["replay_check"]["re_derived_from_log_and_equal"], 700);

    // explain a decision whose answer came from a rule: find an action event that cites one
    let a = s.ok("self_sql", json!({"sql": "SELECT text, tags FROM events WHERE kind = 'action' AND tags LIKE '%rule_used:%' LIMIT 1"}));
    say("self_sql: an attempt answered by a discovered rule", &a);
    let step: String = a.split("step:").nth(1).unwrap().chars().take_while(|c| c.is_ascii_digit()).collect();
    let d = s.ok("self_sql", json!({"sql": format!("SELECT event_id FROM events WHERE kind = 'decision' AND tags LIKE '%step:{step},%'")}));
    let id: String = d.split(|c: char| !c.is_ascii_hexdigit()).find(|t| t.len() >= 40).expect("a decision event id").to_string();
    let ex = s.json("being_explain", json!({"decision_event_id": id}));
    let mut shown = ex.clone();
    shown["evidence_events_cited"] = json!("(cut)");
    shown["reasons_and_alternatives"] = json!("(cut)");
    say("being_explain on that decision (evidence and alternatives cut)", &serde_json::to_string_pretty(&shown).unwrap());
    assert!(ex["discovered_rule_used"]["rule_id"].is_string());
    assert!(ex["discovered_rule_used"]["stance"].as_str().unwrap().contains("next = "));
    assert_eq!(ex["replay_check"]["equal_to_recorded"], true);

    let by = s.ok("self_sql", json!({"sql": "SELECT entity_type, COUNT(*) AS n FROM revisions GROUP BY entity_type ORDER BY entity_type"}));
    say("self_sql: revisions by entity type", &by);
    assert!(by.contains("rule"));

    // the world options are kept
    let (e, t) = s.call("being_run", json!({"steps": 1, "pattern_families": "standard"}));
    assert!(e && t.contains("pattern_families"), "{t}");
    let ver = s.ok("self_verify", json!({}));
    say("self_verify", &ver);
    assert!(ver.starts_with("OK"));
    s.stop();

    let seg_dir = root.path().join(owner).join("log").join("events");
    let seg = std::fs::read_dir(&seg_dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).max().unwrap();
    let good = std::fs::read(&seg).unwrap();
    let mut bad = good.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 1;
    std::fs::write(&seg, &bad).unwrap();
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let (e, t) = s.call("self_verify", json!({}));
    say("self_verify after flipping a byte", &t);
    assert!(e && t.contains("VERIFY FAILED"), "{t}");
    s.stop();
    std::fs::write(&seg, good).unwrap();
}
