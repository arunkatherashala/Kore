//! End to end, Track B4: advice as an input, through the real binary (stdio MCP) against temp roots.
//! * a simulated advisor with trust learning: authorization (default deny, audited), being_run, being_status,
//!   being_explain on a decision the advice changed, self_verify, a byte flip that verify must catch;
//! * the command advisor, tested ONLY with the helper example program `chat_helper` (never a real model):
//!   configuration errors, timeout, bad JSON, output cap, non-zero exit, redaction before the command, env;
//! * blind following (trust false), reserved names (advisor.txt, source advisor).
#![allow(dead_code)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
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

    fn count(&mut self, q: &str) -> i64 {
        let t = self.ok("self_sql", json!({"sql": q}));
        t.lines().filter_map(|l| l.trim().trim_matches('|').trim().parse::<i64>().ok()).next().unwrap_or_else(|| panic!("no number in {t}"))
    }

    /// First data cell of a one-column query (the output is a text table: header, separator, rows).
    fn first_cell(&mut self, q: &str, header: &str) -> Option<String> {
        let t = self.ok("self_sql", json!({"sql": q}));
        t.lines()
            .map(|l| l.trim().trim_matches('|').trim().to_string())
            .find(|c| !c.is_empty() && c != header && !c.ends_with(&format!(".{header}")) && !c.chars().all(|ch| ch == '-' || ch == ':' || ch == '+' || ch == ' ') && !c.starts_with('(') && !c.starts_with("version "))
    }

    fn stop(mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn say(label: &str, text: &str) {
    println!("--- {label}\n{text}");
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap()
}

fn helper() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap().join("examples");
    let name = if cfg!(windows) { "chat_helper.exe" } else { "chat_helper" };
    let p = dir.join(name);
    assert!(p.exists(), "build the example first: {}", p.display());
    p
}

const POLICY: &str = "# hand-written policy\n[L1]\nadvisor_query\nbeing_status\nself_sql\n";

fn advisor_file(od: &Path, mode_args: &[&str], extra: &[&str]) {
    let mut l = vec!["# test advisor: the helper example program, not a model".to_string(), format!("command = {}", helper().display())];
    for a in mode_args {
        l.push(format!("arg = {a}"));
    }
    for e in extra {
        l.push((*e).to_string());
    }
    std::fs::write(od.join("advisor.txt"), l.join("\n")).unwrap();
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
fn being_with_a_simulated_advisor_and_trust_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2ev";
    let od = root.path().join(owner);
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let args = json!({"steps": 300, "seed": 1, "world_seed": 3, "budget": 3000, "chooser": "model", "advisor": "simulated", "advisor_quality": 0.9, "trust": true});

    // 1. no policy: asking an advisor is denied by default, audited, and nothing was created
    let (e, t) = s.call("being_run", args.clone());
    say("being_run with advisor simulated, no autonomy_policy.txt", &t);
    assert!(e && t.contains("advisor_query was not allowed") && t.contains("default deny"), "{t}");
    let denied = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'authorize' AND tags LIKE '%tool:advisor_query%' AND tags LIKE '%decision:deny%'");
    assert_eq!(denied, 1);
    let st = s.json("being_status", json!({}));
    assert_eq!(st["exists"], false);

    // 2. the owner lists advisor_query under [L1] by hand
    std::fs::write(od.join("autonomy_policy.txt"), POLICY).unwrap();
    let (e, t) = s.call("being_run", json!({"steps": 5, "chooser": "model", "advisor": "simulated"}));
    assert!(e && t.contains("advisor_quality"), "a simulated advisor needs its quality: {t}");
    let run = s.json("being_run", args.clone());
    let mut shown = run.clone();
    shown["domains"] = json!("(per-domain table cut)");
    say("being_run {steps:300, seed:1, world_seed:3, budget:3000, chooser:\"model\", advisor:\"simulated\", advisor_quality:0.9, trust:true}", &pretty(&shown));
    assert_eq!(run["steps_run"], 300);
    assert_eq!(run["advisor"], "simulated");
    assert_eq!(run["trust_learning"], true);
    assert!(run["advice"].as_array().unwrap().iter().any(|c| c["tests_followed_against_model"].as_u64().unwrap() > 0));
    let allowed = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'authorize' AND tags LIKE '%tool:advisor_query%' AND tags LIKE '%decision:allow%'");
    assert!(allowed >= 1);

    // 3. status shows trust per (advisor, domain) and the replay check includes the advice part
    let st = s.json("being_status", json!({}));
    let mut shown = st.clone();
    shown["last_decisions"] = json!("(cut)");
    shown["domains"] = json!("(cut)");
    shown["advice"]["latest"] = json!(st["advice"]["latest"].as_array().unwrap().iter().take(1).map(|x| json!({"kind": x["kind"], "text": short(x["text"].as_str().unwrap(), 400)})).collect::<Vec<_>>());
    say("being_status (decisions and domains cut)", &pretty(&shown));
    assert_eq!(st["advisor"], "simulated");
    assert_eq!(st["advisor_quality"], 0.9);
    assert_eq!(st["advice"]["advice_events"], 300);
    assert_eq!(st["replay_check"]["decisions"], 300);
    assert_eq!(st["replay_check"]["re_derived_from_log_and_equal"], 300);

    // 4. explain a decision where the advice differed from what the model alone would have done
    let id = s.first_cell("SELECT event_id FROM events WHERE source = 'being:loop' AND kind = 'decision' AND tags LIKE '%adv_agree:0%' AND tags LIKE '%adv_followed:1%' LIMIT 1", "event_id").expect("a followed disagreement exists").trim_end_matches('~').to_string();
    let ex = s.json("being_explain", json!({"decision_event_id": id}));
    let mut shown = ex.clone();
    shown["evidence_events_cited"] = json!("(cut)");
    shown["reasons_and_alternatives"]["alternatives"] = json!("(12 alternatives cut)");
    shown["advice_for_this_step"] = json!(ex["advice_for_this_step"].as_array().unwrap().iter().map(|e| json!({"kind": e["kind"], "text": short(e["text"].as_str().unwrap(), 420)})).collect::<Vec<_>>());
    say("being_explain on a decision where the being followed advice against its own model (cut)", &pretty(&shown));
    assert_eq!(ex["replay_check"]["equal_to_recorded"], true);
    assert_eq!(ex["how_advice_changed_the_decision"]["advice_followed"], true);
    assert_eq!(ex["how_advice_changed_the_decision"]["decision_equals_model_alone"], false);
    assert!(ex["advice_for_this_step"].as_array().unwrap().len() >= 2, "the advice and its result are shown");

    // 5. settings are fixed once the being exists; advisor events cannot be forged; the config file is reserved
    for bad in [json!({"steps": 1, "advisor_quality": 0.5}), json!({"steps": 1, "trust": false}), json!({"steps": 1, "advisor": "none"}), json!({"steps": 1, "advisor": "command"})] {
        let (e, t) = s.call("being_run", bad.clone());
        assert!(e && t.contains("already has"), "{bad}: {t}");
    }
    let (e, t) = s.call("self_event_add", json!({"text": "x", "occurred_at": "2026-01-01", "source": "advisor"}));
    assert!(e && t.contains("reserved"), "{t}");
    std::fs::write(od.join("advisor.txt"), "# SECRETMARK-advisor\n").unwrap();
    for tool in ["self_ingest_folder", "self_ingest_csv", "self_ingest_kore"] {
        let a = if tool == "self_ingest_csv" { json!({"path": "advisor.txt", "mapping": {"text": "a", "occurred_at": "b"}}) } else { json!({"path": "advisor.txt"}) };
        let (e, t) = s.call(tool, a);
        assert!(e, "{tool}: {t}");
    }
    assert_eq!(s.count("SELECT COUNT(*) AS n FROM events WHERE text LIKE '%SECRETMARK%'"), 0);
    std::fs::remove_file(od.join("advisor.txt")).unwrap();

    // 6. the kill switch stops the next advised run
    std::fs::write(od.join("KILL"), "").unwrap();
    let (e, t) = s.call("being_run", json!({"steps": 2}));
    assert!(e && t.contains("advisor_query was not allowed") && t.contains("kill"), "{t}");
    std::fs::remove_file(od.join("KILL")).unwrap();
    let more = s.json("being_run", json!({"steps": 20}));
    assert_eq!(more["total_steps"], 320);

    let ver = s.ok("self_verify", json!({}));
    say("self_verify", &ver);
    assert!(ver.starts_with("OK"));
    s.stop();

    // 7. a flipped byte is caught
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
fn blind_following_is_a_creation_time_setting() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2eb4";
    std::fs::create_dir_all(root.path().join(owner)).unwrap();
    std::fs::write(root.path().join(owner).join("autonomy_policy.txt"), POLICY).unwrap();
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let run = s.json("being_run", json!({"steps": 60, "seed": 2, "world_seed": 5, "budget": 3000, "chooser": "model", "advisor": "simulated", "advisor_quality": 0.0, "trust": false}));
    assert_eq!(run["trust_learning"], false);
    let followed = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'being:loop' AND kind = 'decision' AND tags LIKE '%adv_followed:1%'");
    assert!(followed >= 55, "blind following follows nearly every piece of advice: {followed}");
    let (e, t) = s.call("being_run", json!({"steps": 1, "trust": true}));
    assert!(e && t.contains("trust"), "{t}");
    let st = s.json("being_status", json!({}));
    assert_eq!(st["trust_learning"], false);
    assert_eq!(st["replay_check"]["re_derived_from_log_and_equal"], 60);
    s.stop();
}

#[test]
fn command_advisor_with_the_helper_program_limits_redaction_and_failures() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let new_owner = |name: &str| -> (PathBuf, Server) {
        let od = root.path().join(name);
        std::fs::create_dir_all(&od).unwrap();
        std::fs::write(od.join("autonomy_policy.txt"), POLICY).unwrap();
        (od.clone(), Server::start(root.path(), fake_home.path(), name))
    };
    let run = json!({"steps": 12, "seed": 1, "world_seed": 3, "budget": 3000, "chooser": "model", "advisor": "command"});

    // a missing or broken advisor.txt refuses the run and creates no being
    let (od, mut s) = new_owner("e2ec0");
    let (e, t) = s.call("being_run", run.clone());
    say("being_run advisor command without advisor.txt", &t);
    assert!(e && t.contains("advisor.txt"), "{t}");
    for bad in ["command = relative/prog\n", "command = /nonexistent/zzz\narg = x\n", "shell = yes\n", "# empty\n"] {
        std::fs::write(od.join("advisor.txt"), bad).unwrap();
        let (e, t) = s.call("being_run", run.clone());
        assert!(e && t.contains("advisor.txt problem"), "{bad}: {t}");
    }
    assert_eq!(s.json("being_status", json!({}))["exists"], false);
    s.stop();

    // a working advisor: its prompt and reply are logged, its advice is one input, the log replays
    let (od, mut s) = new_owner("e2ec1");
    advisor_file(&od, &["advise_best", "careful"], &[]);
    let r = s.json("being_run", json!({"steps": 40, "seed": 1, "world_seed": 3, "budget": 3000, "chooser": "model", "advisor": "command"}));
    say("being_run with the helper as advisor (advise_best), 40 steps", &pretty(&json!({"steps_run": r["steps_run"], "advisor": r["advisor"], "advice": r["advice"]})));
    assert_eq!(r["advisor"], "command");
    assert_eq!(s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%ev:advice%'"), 40);
    assert_eq!(s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%ev:advice%' AND text LIKE '%transcript%'"), 40);
    assert_eq!(s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%error:1%'"), 0);
    let st = s.json("being_status", json!({}));
    assert_eq!(st["replay_check"]["re_derived_from_log_and_equal"], 40);
    assert!(st["advice"]["trust"].as_array().unwrap().iter().all(|c| c["advisor"] == "command:chat_helper.exe" || c["advisor"] == "command:chat_helper"));
    s.stop();

    // redaction: a pattern from redact_patterns.txt is gone from what the command receives and from the log
    let (od, mut s) = new_owner("e2ec2");
    std::fs::write(od.join("redact_patterns.txt"), "algebra\n").unwrap();
    advisor_file(&od, &["advise_echo"], &[]);
    s.json("being_run", json!({"steps": 8, "seed": 1, "world_seed": 3, "budget": 3000, "chooser": "model", "advisor": "command"}));
    let saw = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND text LIKE '%saw_word=false saw_redacted=true%'");
    assert!(saw >= 1, "the helper saw a redaction marker and not the word: {saw}");
    assert_eq!(s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND text LIKE '%saw_word=true%'"), 0);
    assert_eq!(s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%ev:advice%' AND text LIKE '%\"domain\":\"algebra\"%'"), 0, "the logged request is the redacted one");
    s.stop();

    // misbehaving programs: the run goes on without advice, every failure is logged, no hang, no crash
    for (name, mode, extra, needle) in [
        ("e2ec3", "sleep", vec!["timeout_secs = 1"], "timed out"),
        ("e2ec4", "flood", vec!["max_output_bytes = 1024"], "more than"),
        ("e2ec5", "badjson", vec![], "not valid JSON"),
        ("e2ec6", "exit1", vec![], "exited"),
        ("e2ec7", "echo", vec![], "names no option"),
    ] {
        let (od, mut s) = new_owner(name);
        advisor_file(&od, &[mode], &extra);
        let t0 = std::time::Instant::now();
        let r = s.json("being_run", json!({"steps": 6, "seed": 1, "world_seed": 3, "budget": 3000, "chooser": "model", "advisor": "command"}));
        assert_eq!(r["steps_run"], 6, "{mode}");
        let errs = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%error:1%'");
        assert_eq!(errs, 6, "{mode}: every failed request is logged");
        let txt = s.first_cell("SELECT text FROM events WHERE source = 'advisor' AND tags LIKE '%error:1%' LIMIT 1", "text").unwrap();
        say(&format!("advisor helper mode {mode}: first logged failure ({:?})", t0.elapsed()), &short(&txt, 500));
        assert!(txt.contains(needle), "{mode}: {txt}");
        // after three failures in a row the advisor is not asked again in this run
        assert!(s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND text LIKE '%not asked%'") >= 3, "{mode}");
        let st = s.json("being_status", json!({}));
        assert_eq!(st["replay_check"]["re_derived_from_log_and_equal"], 6);
        s.stop();
    }

    // the environment of the command is cleared (env mode of the helper is the thinker's; here: no secret reaches it)
    std::env::set_var("KORE_ADVISOR_TEST_SECRET", "s3cret");
    let (od, mut s) = new_owner("e2ec8");
    advisor_file(&od, &["advise_best"], &[]);
    s.json("being_run", json!({"steps": 3, "seed": 1, "world_seed": 3, "budget": 3000, "chooser": "model", "advisor": "command"}));
    assert_eq!(s.count("SELECT COUNT(*) AS n FROM events WHERE text LIKE '%s3cret%'"), 0);
    s.stop();
}

#[test]
fn rule_proposals_from_a_command_advisor_must_pass_the_verified_search() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2er";
    let od = root.path().join(owner);
    std::fs::create_dir_all(&od).unwrap();
    std::fs::write(od.join("autonomy_policy.txt"), POLICY).unwrap();
    // proposes the linear form with lag 1 and a constant for difficulty 2 on every request that asks for it: right for
    // some hidden families, wrong for others; the being decides by its own logged examples
    advisor_file(&od, &["advise_rule", "0", "lin.1.c"], &[]);
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let r = s.json("being_run", json!({"steps": 400, "seed": 1, "world_seed": 6, "budget": 60000, "chooser": "model", "pattern_families": "extended", "advisor": "command"}));
    assert_eq!(r["steps_run"], 400);
    let proposals = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%ev:verdict%'");
    let accepted = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%verdict:accepted%'");
    let pending = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%verdict:pending%'");
    let held = s.count("SELECT COUNT(*) AS n FROM events WHERE source = 'advisor' AND tags LIKE '%verdict:held%'");
    say("rule proposals by the helper (lin.1.c for the first signature asked)", &format!("verdicts {proposals}: accepted {accepted}, not yet supported {pending}, already held {held}"));
    assert!(proposals >= 10);
    // a rule only exists if some verdict accepted it, and never twice
    let rules = s.json("being_rules", json!({}));
    let list = rules["rules"].as_array().unwrap();
    assert_eq!(list.len() as i64, accepted);
    for rr in list {
        assert_eq!(rr["rederived_from_log"], true, "{rr}");
    }
    let st = s.json("being_status", json!({}));
    assert_eq!(st["replay_check"]["re_derived_from_log_and_equal"], 400);
    s.stop();
}
