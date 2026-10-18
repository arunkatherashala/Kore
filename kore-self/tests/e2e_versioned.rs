//! End to end: the real binary in stdio MCP mode against a temp data root.
//! From an empty root: 20 dated events, 3 belief revisions citing them, queries
//! at several versions, versions list, verify OK; then a flipped byte in a
//! segment file must make verify fail and name the file.
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
            // belt and braces: any code that looked at the real home would land here instead
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
        Server { child, stdin, out, next_id: 1 }
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

    /// Returns (is_error, text).
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

    fn stop(mut self) {
        drop(self.stdin.take()); // EOF ends the loop; the binary saves and exits
        let _ = self.child.wait();
    }
}

fn say(label: &str, text: &str) {
    println!("--- {label}\n{text}");
}

const EVENTS: [(&str, &str); 20] = [
    ("2024-01-03", "Reviewed a pull request that mixed tabs and spaces"),
    ("2024-01-09", "Argued about indentation style in the team channel"),
    ("2024-01-15", "Read the project style guide"),
    ("2024-01-22", "Fixed a diff that was noisy because of tab changes"),
    ("2024-01-29", "Used tabs in a personal script"),
    ("2024-02-02", "Configured editor to show whitespace"),
    ("2024-02-08T09:30:00Z", "Pair programming session, partner prefers tabs"),
    ("2024-02-14", "Merge conflict caused by mixed indentation"),
    ("2024-02-20", "Team agreed on spaces in the contributing guide"),
    ("2024-02-27", "Converted two modules to spaces"),
    ("2024-03-04", "Opened an issue to add an editorconfig"),
    ("2024-03-11", "Editorconfig merged, spaces enforced"),
    ("2024-03-18", "Reviewed a patch that followed the editorconfig"),
    ("2024-03-25", "Fixed a lint warning about indentation"),
    ("2024-04-02", "Added a formatter to the repository"),
    ("2024-04-09", "Formatter check added to CI"),
    ("2024-04-15", "CI blocked a commit with wrong indentation"),
    ("2024-04-22", "Stopped thinking about indentation in reviews"),
    ("2024-04-29", "Onboarded a new teammate with the formatter"),
    ("2024-05-06", "Formatter handled a large refactor without review comments"),
];

fn add_event(s: &mut Server, i: usize) -> String {
    let (at, text) = EVENTS[i];
    let t = s.ok("self_event_add", json!({"text": text, "occurred_at": at, "source": "e2e", "kind": "note"}));
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["duplicate"], false);
    v["event_id"].as_str().unwrap().to_string()
}

#[test]
fn versioned_self_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let owner = "e2e";

    // ── session 1 ───────────────────────────────────────────────────────────
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let init = s.rpc("initialize", json!({}));
    assert_eq!(init["result"]["serverInfo"]["name"], "kore-self");

    let list = s.rpc("tools/list", json!({}));
    let names: Vec<String> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in ["self_event_add", "self_belief_revise", "self_belief_history", "self_sql", "self_versions", "self_diff", "self_verify", "self_migrate_v1"] {
        assert!(names.contains(&n.to_string()), "{n} not listed");
    }
    for n in ["self_delta_save", "self_delta_history", "self_evolve"] {
        assert!(!names.contains(&n.to_string()), "{n} should not be listed");
    }

    // events 1..7, then revision A
    let mut ids: Vec<String> = Vec::new();
    for i in 0..7 {
        ids.push(add_event(&mut s, i));
    }
    // adding an event twice is a no-op
    let (at, text) = EVENTS[0];
    let dup: Value = serde_json::from_str(&s.ok("self_event_add", json!({"text": text, "occurred_at": at, "source": "e2e", "kind": "note"}))).unwrap();
    assert_eq!(dup["duplicate"], true);
    assert_eq!(dup["version"], 7);
    // an unknown citation is refused and writes nothing
    let (e, t) = s.call("self_belief_revise", json!({"key": "indent_style", "stance": "x", "confidence": 0.5, "caused_by": ["00ff"]}));
    assert!(e, "{t}");
    // a missing or invalid occurred_at is refused
    assert!(s.call("self_event_add", json!({"text": "no date"})).0);
    assert!(s.call("self_event_add", json!({"text": "bad date", "occurred_at": "last week"})).0);

    let ra_t = s.ok(
        "self_belief_revise",
        json!({"key": "indent_style", "stance": "tabs", "confidence": 0.55, "rationale": "tabs seemed fine in early reviews", "caused_by": [ids[0], ids[3], ids[4]]}),
    );
    say("self_belief_revise (tabs)", &ra_t);
    let ra: Value = serde_json::from_str(&ra_t).unwrap();
    let va = ra["version"].as_u64().unwrap();
    assert_eq!(va, 8);

    // events 8..14, then revision B
    for i in 7..14 {
        ids.push(add_event(&mut s, i));
    }
    let rb_t = s.ok(
        "self_belief_revise",
        json!({"key": "indent_style", "stance": "spaces", "confidence": 0.8, "rationale": "team agreed and merge conflicts stopped", "caused_by": [ids[7], ids[8], ids[11]]}),
    );
    say("self_belief_revise (spaces)", &rb_t);
    let rb: Value = serde_json::from_str(&rb_t).unwrap();
    let vb = rb["version"].as_u64().unwrap();
    assert_eq!(vb, 16);

    // events 15..20, then revision C
    for i in 14..20 {
        ids.push(add_event(&mut s, i));
    }
    let rc_t = s.ok(
        "self_belief_revise",
        json!({"key": "indent_style", "stance": "spaces, enforced by the formatter", "confidence": 0.9, "caused_by": [ids[14], ids[16], ids[19]]}),
    );
    say("self_belief_revise (formatter)", &rc_t);
    let rc: Value = serde_json::from_str(&rc_t).unwrap();
    let vc = rc["version"].as_u64().unwrap();
    assert_eq!(vc, 23);
    // valid_from defaulted to the newest cited event's occurred_at
    assert_eq!(rc["valid_from"], "2024-05-06T00:00:00Z");

    let hist_t = s.ok("self_belief_history", json!({"key": "indent_style"}));
    say("self_belief_history", &hist_t);
    let hist: Value = serde_json::from_str(&hist_t).unwrap();
    let h = hist["history"].as_array().unwrap();
    assert_eq!(h.len(), 3);
    assert_eq!(h[0]["stance"], "tabs");
    assert_eq!(h[1]["supersedes"], h[0]["rev_id"]);
    assert_eq!(h[2]["supersedes"], h[1]["rev_id"]);
    assert_eq!(h[1]["caused_by"].as_array().unwrap().len(), 3);
    assert!(h[1]["caused_by"].as_array().unwrap().iter().any(|c| c["text"] == "Team agreed on spaces in the contributing guide"));
    for r in h {
        assert_eq!(r["author"], "user");
    }

    // ── time travel ─────────────────────────────────────────────────────────
    let cur = "SELECT key, stance, confidence FROM beliefs_current";
    let cnt = "SELECT count(*) FROM events";
    let mut stances = Vec::new();
    for (v, want_stance, want_events) in [(va, "tabs", "7"), (vb, "spaces", "14"), (vc, "spaces, enforced by the formatter", "20")] {
        let b = s.ok("self_sql", json!({"sql": cur, "as_of_version": v}));
        say(&format!("self_sql as_of_version {v}: {cur}"), &b);
        assert!(b.contains(want_stance) && b.contains("(1 rows)"), "{b}");
        let n = s.ok("self_sql", json!({"sql": cnt, "as_of_version": v}));
        say(&format!("self_sql as_of_version {v}: {cnt}"), &n);
        assert!(n.contains(want_events), "{n}");
        stances.push(b);
    }
    assert!(stances[0] != stances[1] && stances[1] != stances[2] && stances[0] != stances[2]);
    // before any revision there is no belief; versions 1, 2, 3 hold 1, 2, 3 events
    for v in 1..=3u64 {
        let n = s.ok("self_sql", json!({"sql": cnt, "as_of_version": v}));
        assert!(n.contains(&format!("\n{v}\n")) || n.contains(&format!("| {v}")) || n.lines().any(|l| l.trim() == v.to_string()), "{n}");
        let b = s.ok("self_sql", json!({"sql": cur, "as_of_version": v}));
        assert!(b.contains("(no rows)"), "{b}");
    }
    // evidence for the beliefs, as of the latest version (join keeps alias prefixes)
    let join = "SELECT r.stance, e.occurred_at, e.text FROM revisions r JOIN revision_causes c ON r.rev_id = c.rev_id JOIN events e ON c.event_id = e.event_id WHERE r.stance = 'spaces'";
    let j = s.ok("self_sql", json!({"sql": join}));
    say("self_sql (evidence for the 'spaces' revision)", &j);
    assert!(j.contains("r.stance") && j.contains("e.text") && j.contains("(3 rows)"), "{j}");
    // read-only
    assert!(s.call("self_sql", json!({"sql": "DELETE FROM events"})).0);
    assert!(s.call("self_sql", json!({"sql": cnt, "as_of_version": 9999})).0);

    let vers = s.ok("self_versions", json!({}));
    say("self_versions (head)", &vers);
    let vj: Value = serde_json::from_str(&vers).unwrap();
    assert_eq!(vj["latest"], 23);
    assert_eq!(vj["versions"].as_array().unwrap().len(), 24);
    assert_eq!(vj["versions"][23]["rows"]["events"], 20);
    assert_eq!(vj["versions"][23]["rows"]["revisions"], 3);
    assert_eq!(vj["versions"][23]["rows"]["revision_causes"], 9);

    let d = s.ok("self_diff", json!({"v1": 15, "v2": 16}));
    let dj: Value = serde_json::from_str(&d).unwrap();
    assert_eq!(dj["counts"]["revisions"], 1);

    let ver = s.ok("self_verify", json!({}));
    say("self_verify", &ver);
    assert!(ver.starts_with("OK"), "{ver}");

    // self_ingest also lands in the log
    let ing = s.ok("self_ingest", json!({"content": "I prefer small commits", "kind": "preference"}));
    assert!(ing.contains("Logged as event"), "{ing}");
    let src = s.ok("self_sql", json!({"sql": "SELECT source, kind, text FROM events WHERE source = 'self_ingest'"}));
    say("self_sql (self_ingest event)", &src);
    assert!(src.contains("self_ingest") && src.contains("(1 rows)"), "{src}");
    s.stop();

    // ── isolation: data is in the temp root; the (fake) home stayed empty ───
    let log_dir = root.path().join(owner).join("log");
    assert!(log_dir.join("MANIFEST").exists(), "log not under KORE_SELF_HOME");
    assert!(log_dir.join("events").is_dir());
    assert_eq!(std::fs::read_dir(fake_home.path()).unwrap().count(), 0, "something was written to the home directory");

    // ── restart keeps the data ───────────────────────────────────────────────
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    s.rpc("initialize", json!({}));
    let b = s.ok("self_sql", json!({"sql": cur}));
    assert!(b.contains("spaces, enforced by the formatter"), "{b}");
    let ver = s.ok("self_verify", json!({}));
    assert!(ver.starts_with("OK"), "{ver}");
    s.stop();

    // ── tamper: flip one byte in a segment ──────────────────────────────────
    let seg = log_dir.join("events").join("0000000003.kore");
    let mut bytes = std::fs::read(&seg).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    std::fs::write(&seg, bytes).unwrap();

    let mut s = Server::start(root.path(), fake_home.path(), owner);
    s.rpc("initialize", json!({}));
    let (e, t) = s.call("self_verify", json!({}));
    say("self_verify after flipping one byte", &t);
    assert!(e, "verify should fail: {t}");
    assert!(t.contains("0000000003.kore"), "failure should name the file: {t}");
    // writes are refused on a damaged log
    let (e, _) = s.call("self_event_add", json!({"text": "after tamper", "occurred_at": "2024-06-01"}));
    assert!(e);
    s.stop();
    assert_eq!(std::fs::read_dir(fake_home.path()).unwrap().count(), 0);
}
