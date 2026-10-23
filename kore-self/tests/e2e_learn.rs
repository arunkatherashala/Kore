//! End to end: the real binary (stdio MCP) against a temp data root.
//! A synthetic life (generated with the library) is written to a CSV, ingested
//! through the tools, learned from, explained, rule-evolved (dry run and real),
//! snapshotted into the prediction ledger, verified, and then tampered with.
#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/learn/mod.rs"]
mod learn;
#[path = "../src/synth.rs"]
mod synth;

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
    println!("--- {label}\n{text}");
}

fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
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
fn learn_explain_evolve_ledger_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let owner = "e2el";

    // synthetic life -> CSV (the ingestion door the tools offer)
    let (events, truth) = synth::generate(&synth::default_config(11, 0.2));
    let mut csv = String::from("when,what,src,type,labels\n");
    for e in &events {
        csv.push_str(&format!("{},{},{},{},{}\n", e.occurred_at, e.text, e.source, e.kind, e.tags.join(";")));
        assert!(!e.text.contains(',') && !e.text.contains('"'));
    }
    write(data.path(), "life.csv", &csv);
    let owner_dir = root.path().join(owner);
    std::fs::create_dir_all(&owner_dir).unwrap();
    std::fs::write(owner_dir.join("ingest_roots"), format!("{}\n", data.path().display())).unwrap();

    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let list = s.rpc("tools/list", json!({}));
    let names: Vec<String> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in ["self_learn", "self_explain", "self_evolve_rules", "self_ledger_snapshot", "self_ledger_outcome"] {
        assert!(names.contains(&n.to_string()), "{n} missing");
    }
    let csv_path = data.path().join("life.csv").display().to_string();
    let t = s.ok("self_ingest_csv", json!({"path": csv_path, "mapping": {"occurred_at": "when", "text": "what", "source": "src", "kind": "type", "tags": "labels"}}));
    let added = serde_json::from_str::<Value>(&t).unwrap()["added"].as_u64().unwrap();
    assert_eq!(added as usize, events.len(), "{t}");
    say("self_ingest_csv (synthetic life, seed 11, noise 20%)", &short(&t, 160));

    // learn: dry run, real, repeat
    let dry = s.json("self_learn", json!({"dry_run": true}));
    say("self_learn dry_run", &short(&dry.to_string(), 700));
    assert_eq!(dry["version_before"], dry["version_after"]);
    let real = s.json("self_learn", json!({}));
    say("self_learn counts", &real["counts"].to_string());
    say("self_learn insufficient_data", &real["insufficient_data"].to_string());
    assert!(real["counts"]["habits"]["added"].as_u64().unwrap() >= 2);
    assert_eq!(real["counts"]["stances"]["added"].as_u64().unwrap() as usize, truth.stances.len());
    let again = s.json("self_learn", json!({}));
    say("self_learn (second run)", &again["counts"].to_string());
    assert_eq!(again["counts"]["habits"]["added"], 0);
    assert_eq!(again["counts"]["changes"]["added"], 0);
    assert_eq!(again["counts"]["stances"]["added"], 0);
    assert_eq!(again["version_before"], again["version_after"]);

    // explain one revision
    let h = real["derived"].as_array().unwrap().iter().find(|d| d["key"] == "ctx:morning|act:run").expect("morning/run habit");
    say("habit revision", &short(&h.to_string(), 420));
    let ex = s.json("self_explain", json!({"rev_id": h["rev_id"]}));
    say("self_explain", &short(&ex.to_string(), 900));
    assert_eq!(ex["author"], "rule:habits-v1");
    assert!(ex["cited_event_count"].as_u64().unwrap() > 20);

    // evolve: dry run, real
    let dry = s.json("self_evolve_rules", json!({"dry_run": true}));
    say("self_evolve_rules dry_run", &dry.to_string());
    let real = s.json("self_evolve_rules", json!({}));
    say("self_evolve_rules", &short(&real.to_string(), 300));
    assert_eq!(dry["verdict"], real["verdict"]);
    assert!(dry["recorded"].is_null() && !real["recorded"].is_null());
    let rules = s.ok("self_sql", json!({"sql": "SELECT key, author, confidence FROM beliefs_current WHERE entity_type = 'rule'"}));
    say("rule revisions (self_sql)", &rules);

    // ledger
    let snap = s.json("self_ledger_snapshot", json!({}));
    say("self_ledger_snapshot", &short(&snap.to_string(), 600));
    let n = snap["written"].as_array().unwrap().len();
    assert!(n >= 2);
    let snap2 = s.json("self_ledger_snapshot", json!({}));
    say("self_ledger_snapshot (again)", &short(&snap2.to_string(), 200));
    assert_eq!(snap2["written"].as_array().unwrap().len(), 0);
    let ver = s.ok("self_verify", json!({}));
    say("self_verify", &ver);
    assert!(ver.starts_with("OK"));
    s.stop();

    // tamper: flip one byte in the first revisions segment
    let seg_dir = owner_dir.join("log").join("revisions");
    let seg = std::fs::read_dir(&seg_dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).min().unwrap();
    let good = std::fs::read(&seg).unwrap();
    let mut bad = good.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 1;
    std::fs::write(&seg, &bad).unwrap();
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let (e, t) = s.call("self_verify", json!({}));
    say("self_verify after flipping a byte", &t);
    assert!(e && t.contains("VERIFY FAILED") && t.contains(&seg.file_name().unwrap().to_string_lossy().to_string()), "{t}");
    s.stop();
    std::fs::write(&seg, good).unwrap();
}
