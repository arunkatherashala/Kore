//! End to end: talking to the being (A1) through the real binary (stdio MCP) against a temp data root.
//! The "thinker" command in these tests is the example program `chat_helper` (no model).
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
            .env("KORE_HELPER_ALLOWED", "visible-value")
            .env("KORE_HELPER_SECRET", "must-not-leak")
            .env_remove("KORE_SELF_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if std::env::var("E2E_STDERR").is_ok() { Stdio::inherit() } else { Stdio::null() })
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

    fn say(&mut self, text: &str) -> Value {
        self.json("chat_say", json!({"text": text}))
    }

    fn stop(mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn show(label: &str, text: &str) {
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

fn write_thinker(owner_dir: &Path, lines: &[String]) {
    std::fs::write(owner_dir.join("thinker.txt"), lines.join("\n")).unwrap();
}

fn thinker_with(owner_dir: &Path, mode_args: &[&str], extra: &[&str]) {
    let mut l = vec!["# test thinker".to_string(), format!("command = {}", helper().display())];
    l.extend(mode_args.iter().map(|a| format!("arg = {a}")));
    l.extend(extra.iter().map(|e| e.to_string()));
    write_thinker(owner_dir, &l);
}

fn count(s: &mut Server, sql: &str) -> i64 {
    let t = s.ok("self_sql", json!({"sql": sql}));
    t.lines().filter_map(|l| l.trim().trim_matches('|').trim().parse::<i64>().ok()).next().unwrap_or_else(|| panic!("no number in {t}"))
}

const POLICY: &str = "# hand-written policy\n[L1]\nself_sql\nbeing_status\nself_belief_history\n[L2]\nself_event_add\n";

#[test]
fn chat_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2ec";
    let od = root.path().join(owner);
    let mut s = Server::start(root.path(), fake_home.path(), owner);

    // 1. no thinker.txt, no policy: the scripted thinker answers, tool calls are denied (default deny)
    let r = s.say("echo hello there");
    show("chat_say 'echo hello there' (no thinker.txt, no policy)", &pretty(&r));
    assert_eq!(r["reply"], "hello there");
    assert_eq!(r["thinker"], "scripted-chat-1");
    let r = s.say("status");
    show("chat_say 'status' (no policy file)", &pretty(&r));
    assert_eq!(r["tools"][0]["status"], "denied");
    assert!(r["tools"][0]["reason"].as_str().unwrap().contains("default deny"));

    // 2. a hand-written policy allows reads and notes
    std::fs::write(od.join("autonomy_policy.txt"), POLICY).unwrap();
    let r = s.say("remember the kettle is on the second shelf");
    show("chat_say 'remember ...' (policy allows self_event_add)", &pretty(&r));
    assert_eq!(r["tools"][0]["status"], "ran");
    let r = s.say("recall kettle");
    show("chat_say 'recall kettle'", &pretty(&r));
    assert_eq!(r["tools"][0]["status"], "ran");
    assert!(r["tools"][0]["result"].as_str().unwrap().contains("second shelf"), "{r}");
    let r = s.say("status");
    assert_eq!(r["tools"][0]["status"], "ran");

    let h = s.json("chat_history", json!({"limit": 4}));
    show("chat_history {limit:4}", &pretty(&h));
    assert_eq!(h["messages"], 4);

    // 3. a command thinker: the helper echoes what it received, so we can see the prompt was redacted
    thinker_with(&od, &["echo"], &[]);
    let r = s.say("my email is bob.builder@example.com, ssn 123-45-6789, card 4111 1111 1111 1111");
    show("chat_say with secrets, thinker = helper echo", &pretty(&r));
    let reply = r["reply"].as_str().unwrap();
    assert!(reply.starts_with("RECEIVED: "), "{reply}");
    for secret in ["bob.builder@example.com", "123-45-6789", "4111 1111 1111 1111"] {
        assert!(!reply.contains(secret), "secret reached the command: {secret}");
    }
    assert!(reply.contains("[REDACTED"), "{reply}");
    // the prompt and reply were logged as events, and the logged prompt has no secret either
    let n = count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'thinker'");
    assert!(n >= 2, "thinker events: {n}");
    let leak = count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE text LIKE '%bob.builder%'");
    assert_eq!(leak, 0, "the address must not be anywhere in the log");

    // 4. a proposed tool call outside the policy is denied and audited; an allowed one runs
    thinker_with(&od, &["propose", "self_forget", "{\"all\":true}"], &[]);
    let r = s.say("please forget everything");
    show("thinker proposes self_forget", &pretty(&r));
    assert_eq!(r["tools"][0]["status"], "denied");
    thinker_with(&od, &["propose", "chat_approve", "{\"id\":\"abcdefgh\",\"approve\":true}"], &[]);
    let r = s.say("approve it yourself");
    assert_eq!(r["tools"][0]["status"], "denied");
    assert!(r["tools"][0]["reason"].as_str().unwrap().contains("human-only") || r["tools"][0]["reason"].as_str().unwrap().contains("not a tool"), "{r}");
    thinker_with(&od, &["propose", "self_sql", "{\"sql\":\"SELECT COUNT(*) AS n FROM events\"}"], &[]);
    let r = s.say("how many events");
    assert_eq!(r["tools"][0]["status"], "ran");
    let audit = s.ok("self_sql", json!({"sql": "SELECT tags, COUNT(*) AS n FROM events WHERE source = 'authorize' GROUP BY tags ORDER BY tags"}));
    show("self_sql: audit rows by decision/tool/actor", &audit);
    assert!(audit.contains("decision:deny") && audit.contains("tool:self_forget") && audit.contains("decision:allow"));
    let forgot = count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'chat:user'");
    assert!(forgot >= 8, "nothing was forgotten: {forgot}");

    // 5. NeedsApproval waits until the human approves
    std::fs::write(od.join("autonomy_policy.txt"), "[L1]\nself_sql\n[L3]\nself_event_add\n").unwrap();
    thinker_with(&od, &["propose", "self_event_add", "{\"text\":\"note that needed approval\"}"], &[]);
    let r = s.say("write that down");
    show("thinker proposes self_event_add (listed under L3)", &pretty(&r));
    assert_eq!(r["tools"][0]["status"], "needs_approval");
    let pid = r["tools"][0]["pending_id"].as_str().unwrap().to_string();
    let p = s.json("chat_pending", json!({}));
    show("chat_pending", &pretty(&p));
    assert_eq!(p["pending"], 1);
    assert_eq!(count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'chat_note'"), 1, "only the earlier 'remember' note exists");
    let a = s.json("chat_approve", json!({"id": pid, "approve": true}));
    show("chat_approve {approve:true}", &pretty(&a));
    assert_eq!(a["ran"], true);
    assert_eq!(count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'chat_note'"), 2);
    let (e, t) = s.call("chat_approve", json!({"id": pid, "approve": true}));
    assert!(e && t.contains("already decided"), "{t}");
    assert_eq!(s.json("chat_pending", json!({}))["pending"], 0);
    let humans = count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'chat:human'");
    assert_eq!(humans, 1);
    // a rejected one never runs
    let r = s.say("write that down again");
    let pid2 = r["tools"][0]["pending_id"].as_str().unwrap().to_string();
    let a = s.json("chat_approve", json!({"id": pid2, "approve": false}));
    assert_eq!(a["approved"], false);
    assert_eq!(count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'chat_note'"), 2);
    // approval does not outlive the policy: pending item approved after the policy removed the tool
    let r = s.say("one more");
    let pid3 = r["tools"][0]["pending_id"].as_str().unwrap().to_string();
    std::fs::write(od.join("autonomy_policy.txt"), "[L1]\nself_sql\n").unwrap();
    let a = s.json("chat_approve", json!({"id": pid3, "approve": true}));
    assert_eq!(a["ran"], false);
    assert_eq!(count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'chat_note'"), 2);

    // 6. kill switch: nothing runs, the thinker is not started, read-only history still works
    std::fs::write(od.join("autonomy_policy.txt"), POLICY).unwrap();
    let before = count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'thinker'");
    std::fs::write(od.join("KILL"), "").unwrap();
    let (e, t) = s.call("chat_say", json!({"text": "are you there"}));
    show("chat_say with the KILL file present", &t);
    assert!(e && t.contains("kill switch"), "{t}");
    let (e, _) = s.call("chat_approve", json!({"id": pid3, "approve": true}));
    assert!(e);
    let (e, t) = s.call("chat_history", json!({}));
    assert!(!e, "{t}");
    assert_eq!(count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'thinker'"), before);
    std::fs::remove_file(od.join("KILL")).unwrap();
    assert!(!s.call("chat_say", json!({"text": "back again"})).0);

    // 7. conversations are in the log, survive a restart, and the log verifies
    let hist_before = s.json("chat_history", json!({"limit": 200}));
    let ver = s.ok("self_verify", json!({}));
    show("self_verify", &ver);
    assert!(ver.starts_with("OK"));
    s.stop();
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let hist_after = s.json("chat_history", json!({"limit": 200}));
    assert_eq!(hist_before["history"], hist_after["history"]);
    let sessions = s.ok("self_sql", json!({"sql": "SELECT source, COUNT(*) AS n FROM events GROUP BY source ORDER BY source"}));
    show("self_sql: events by source after restart", &sessions);
    assert!(sessions.contains("chat:being") && sessions.contains("authorize") && sessions.contains("thinker"));
    assert!(s.ok("self_verify", json!({})).starts_with("OK"));
    s.stop();

    // 8. a flipped byte is caught
    let seg_dir = od.join("log").join("events");
    let seg = std::fs::read_dir(&seg_dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).max().unwrap();
    let good = std::fs::read(&seg).unwrap();
    let mut bad = good.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 1;
    std::fs::write(&seg, &bad).unwrap();
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let (e, t) = s.call("self_verify", json!({}));
    show("self_verify after flipping a byte", &t);
    assert!(e && t.contains("VERIFY FAILED"), "{t}");
    s.stop();
    std::fs::write(&seg, good).unwrap();

    // the real home was never used
    assert_eq!(std::fs::read_dir(fake_home.path()).unwrap().count(), 0, "nothing may be written to the (fake) home");
}

#[test]
fn command_thinker_limits_and_environment() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2ed";
    let od = root.path().join(owner);
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    s.say("hello"); // creates the owner dir

    // minimal environment, scratch working directory, explicit allow-list only
    thinker_with(&od, &["env"], &["env_allow = KORE_HELPER_ALLOWED"]);
    let r = s.say("env please");
    show("helper in env mode", &pretty(&r));
    let reply = r["reply"].as_str().unwrap();
    assert!(reply.contains("allowed=visible-value"), "{reply}");
    assert!(!reply.contains("KORE_HELPER_SECRET") && !reply.contains("KORE_SELF_HOME"), "{reply}");
    assert!(reply.contains("scratch"), "cwd is the scratch dir: {reply}");

    // misbehaving programs are refused and logged, the turn fails
    thinker_with(&od, &["sleep"], &["timeout_secs = 1"]);
    let t0 = std::time::Instant::now();
    let (e, t) = s.call("chat_say", json!({"text": "wait for it"}));
    show("helper sleeps, timeout_secs = 1", &t);
    assert!(e && t.contains("timed out"), "{t}");
    assert!(t0.elapsed().as_secs() < 20);
    thinker_with(&od, &["flood"], &["max_output_bytes = 4096"]);
    let (e, t) = s.call("chat_say", json!({"text": "too much"}));
    assert!(e && t.contains("more than 4096 bytes"), "{t}");
    thinker_with(&od, &["badjson"], &[]);
    let (e, t) = s.call("chat_say", json!({"text": "garbage"}));
    assert!(e && t.contains("not valid JSON"), "{t}");
    thinker_with(&od, &["exit1"], &[]);
    let (e, t) = s.call("chat_say", json!({"text": "fail"}));
    assert!(e && t.contains("exited with"), "{t}");
    let errs = count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE source = 'thinker' AND tags LIKE '%direction:error%'");
    assert_eq!(errs, 4);

    // a bad thinker.txt refuses the turn (no silent fallback to the scripted thinker)
    for bad in ["command = relative/prog.exe", "command = C:\\definitely\\missing\\prog.exe", "shell = yes", "arg = x"] {
        write_thinker(&od, &[bad.to_string()]);
        let (e, t) = s.call("chat_say", json!({"text": "hi"}));
        assert!(e && t.contains("thinker.txt problem"), "{bad}: {t}");
    }
    // removing the file brings the scripted thinker back
    std::fs::remove_file(od.join("thinker.txt")).unwrap();
    assert_eq!(s.say("echo back")["thinker"], "scripted-chat-1");
    s.stop();
}

#[test]
fn hand_edited_files_cannot_be_touched_by_tools() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2ee";
    let od = root.path().join(owner);
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    s.say("hello");
    std::fs::write(od.join("thinker.txt"), "# SECRETMARK-thinker\n").unwrap();
    std::fs::write(od.join("autonomy_policy.txt"), "# SECRETMARK-policy\n").unwrap();
    std::fs::write(od.join("KILL"), "SECRETMARK-kill").unwrap();
    for f in ["thinker.txt", "autonomy_policy.txt", "KILL", "kill", "Thinker.TXT", "thinker.txt."] {
        for tool in ["self_ingest_folder", "self_ingest_csv", "self_ingest_kore"] {
            let args = if tool == "self_ingest_csv" { json!({"path": f, "mapping": {"text": "a", "occurred_at": "b"}}) } else { json!({"path": f}) };
            let (e, t) = s.call(tool, args);
            assert!(e, "{tool} {f} must be refused: {t}");
        }
    }
    // export names are confined to exports/, so these cannot be created or overwritten either
    for f in ["../KILL", "../thinker.txt", "KILL", "..\\autonomy_policy.txt"] {
        let (e, _) = s.call("self_export", json!({"path": f}));
        assert!(e, "self_export {f}");
    }
    // walking the whole owner directory skips them
    let _ = s.call("self_ingest_folder", json!({"path": "."}));
    let n = count(&mut s, "SELECT COUNT(*) AS n FROM events WHERE text LIKE '%SECRETMARK%'");
    assert_eq!(n, 0, "reserved files must never be ingested");
    // audit and chat sources cannot be forged through self_event_add
    for src in ["authorize", "thinker", "chat:user", "chat:human"] {
        let (e, t) = s.call("self_event_add", json!({"text": "x", "occurred_at": "2026-01-01", "source": src}));
        assert!(e && t.contains("reserved"), "{src}: {t}");
    }
    assert_eq!(std::fs::read_to_string(od.join("KILL")).unwrap(), "SECRETMARK-kill");
    s.stop();
}
