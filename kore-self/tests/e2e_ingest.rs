//! End to end: the real binary (stdio MCP) against temp data roots.
//! A journal folder (10 dated files, 2 undated, 1 with an e-mail address) and a
//! CSV with a bad row are ingested through the allow-list, re-run to show dedupe,
//! one event is forgotten, verify still passes, and the exports are re-imported
//! into a second owner.
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

#[test]
fn ingest_forget_export_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    let journal = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let owner = "e2e";

    // ── fixtures ────────────────────────────────────────────────────────────
    for i in 1..=10 {
        write(journal.path(), &format!("2024-03-{i:02}-entry.md"), &format!("# Day {i}\nWrote entry {i} and went for a walk."));
    }
    write(journal.path(), "2024-03-05-entry.md", "# Day 5\nThe secret plan about zebra-cheese-7731 was discussed.");
    write(journal.path(), "scratch.md", "no date anywhere in this file");
    write(journal.path(), "ideas/loose-notes.txt", "also undated");
    write(journal.path(), "2024-03-20-contact.md", "Asked jane.doe@example.com to call back on 555-123-4567.");
    write(other.path(), "2024-01-01-elsewhere.md", "this folder is not allow-listed");
    write(
        data.path(),
        "backfill.csv",
        "when,what,where,type,labels\r\n\
2023-12-01,\"Started a new job, \"\"finally\"\"\",hr,decision,work\r\n\
2023-12-02,Moved to a new desk,hr,action,work;office\r\n\
,This row has no date,hr,note,\r\n\
2023-12-03,Read a book,home,note,leisure\r\n",
    );
    // The allow-list is edited by hand (here: by the test), never through a tool.
    let owner_dir = root.path().join(owner);
    std::fs::create_dir_all(&owner_dir).unwrap();
    std::fs::write(owner_dir.join("ingest_roots"), format!("# folders kore-self may read\n{}\n{}\n", journal.path().display(), data.path().display())).unwrap();
    // optional redaction patterns, also hand-edited
    std::fs::write(owner_dir.join("redact_patterns.txt"), "zebra-cheese\n").unwrap();

    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let list = s.rpc("tools/list", json!({}));
    let names: Vec<String> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in ["self_ingest_folder", "self_ingest_csv", "self_ingest_kore", "self_forget", "self_export"] {
        assert!(names.contains(&n.to_string()), "{n} not listed");
    }

    // ── allow-list: an unlisted folder is rejected, and the list is not writable by tools
    let (e, t) = s.call("self_ingest_folder", json!({"path": other.path().to_string_lossy()}));
    say("self_ingest_folder (folder not on the allow-list)", &t);
    assert!(e && t.contains("allowed ingest root"));
    let (e, t) = s.call("self_ingest_folder", json!({"path": "../.."}));
    assert!(e, "{t}");
    let (e, t) = s.call("self_save", json!({"path": "ingest_roots"}));
    say("self_save onto the allow-list file", &t);
    assert!(e, "tools must not be able to write the allow-list: {t}");
    assert_eq!(std::fs::read_to_string(owner_dir.join("ingest_roots")).unwrap().lines().count(), 3);

    // ── journal folder ──────────────────────────────────────────────────────
    let jp = journal.path().to_string_lossy().to_string();
    let t = s.ok("self_ingest_folder", json!({"path": jp, "dry_run": true}));
    say("self_ingest_folder dry_run", &t);
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!((v["added"].as_u64(), v["would_add"].as_u64()), (Some(0), Some(11)));
    let t = s.ok("self_ingest_folder", json!({"path": jp}));
    say("self_ingest_folder", &t);
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["added"], 11);
    assert_eq!(v["skipped_undated"], 2);
    assert_eq!(v["redacted"], 2);
    assert_eq!(v["errors"], 0);
    let t = s.ok("self_ingest_folder", json!({"path": jp}));
    say("self_ingest_folder (second run: dedupe)", &t);
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!((v["added"].as_u64(), v["duplicate"].as_u64()), (Some(0), Some(11)));

    // redaction happened before storing
    let q = s.ok("self_sql", json!({"sql": "SELECT source, text, redacted FROM events WHERE redacted"}));
    say("self_sql (redacted events)", &q);
    assert!(q.contains("[REDACTED:email]") && q.contains("[REDACTED:phone]") && q.contains("[REDACTED:custom]"), "{q}");
    assert!(!q.contains("jane.doe") && !q.contains("zebra-cheese") && !q.contains("555-123-4567"));

    // ── CSV with a bad row ──────────────────────────────────────────────────
    let cp = data.path().join("backfill.csv").to_string_lossy().to_string();
    let mapping = json!({"occurred_at": "when", "text": "what", "source": "where", "kind": "type", "tags": "labels"});
    let t = s.ok("self_ingest_csv", json!({"path": cp, "mapping": mapping}));
    say("self_ingest_csv", &t);
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!((v["added"].as_u64(), v["rejected_no_date"].as_u64(), v["errors"].as_u64()), (Some(3), Some(1), Some(0)));
    let t = s.ok("self_ingest_csv", json!({"path": cp, "mapping": mapping}));
    say("self_ingest_csv (second run: dedupe)", &t);
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!((v["added"].as_u64(), v["duplicate"].as_u64()), (Some(0), Some(3)));

    let before = s.json("self_versions", json!({}));
    let latest_before = before["latest"].as_u64().unwrap();
    let total = s.ok("self_sql", json!({"sql": "SELECT count(*) FROM events"}));
    say("self_sql count before forget", &total);

    // ── forget one event ────────────────────────────────────────────────────
    let (e, t) = s.call("self_forget", json!({"source_prefix": "journal:2024-03-05"}));
    say("self_forget without confirm", &t);
    assert!(e);
    let d = s.json("self_diff", json!({"v1": 0, "v2": latest_before}));
    let id = d["events"].as_array().unwrap().iter().find(|e| e["source"] == "journal:2024-03-05-entry.md").expect("event in diff")["event_id"].as_str().unwrap().to_string();
    let (e, t) = s.call("self_forget", json!({"event_id": "0000", "confirm": true}));
    say("self_forget unknown id", &t);
    assert!(e);
    let t = s.ok("self_forget", json!({"event_id": id, "confirm": true}));
    say("self_forget", &t);
    let t2 = s.ok("self_forget", json!({"event_id": id, "confirm": true}));
    say("self_forget (again: idempotent)", &t2);
    assert_eq!(serde_json::from_str::<Value>(&t2).unwrap()["already_forgotten"], 1);
    let ver = s.ok("self_verify", json!({}));
    say("self_verify after forget", &ver);
    assert!(ver.starts_with("OK"), "{ver}");
    for v in 0..=s.json("self_versions", json!({}))["latest"].as_u64().unwrap() {
        let q = s.ok("self_sql", json!({"sql": "SELECT text FROM events WHERE source = 'journal:2024-03-05-entry.md'", "as_of_version": v}));
        assert!(q.contains("(no rows)"), "forgotten event visible at version {v}: {q}");
    }
    let t = s.ok("self_ingest_folder", json!({"path": jp}));
    say("self_ingest_folder after forget (not resurrected)", &t);
    assert_eq!(serde_json::from_str::<Value>(&t).unwrap()["suppressed_forgotten"], 1);
    assert_eq!(s.json("self_versions", json!({}))["latest"].as_u64().unwrap(), latest_before + 1);

    // ── export ──────────────────────────────────────────────────────────────
    let csv_out = s.ok("self_export", json!({"path": "events.csv", "format": "csv"}));
    say("self_export csv", &csv_out);
    let kore_out = s.ok("self_export", json!({"path": "events.kore", "format": "kore"}));
    say("self_export kore", &kore_out);
    let (e, t) = s.call("self_export", json!({"path": "../outside.csv", "format": "csv"}));
    say("self_export outside exports/", &t);
    assert!(e);
    let (e, _) = s.call("self_export", json!({"path": other.path().join("x.csv").to_string_lossy(), "format": "csv"}));
    assert!(e);
    assert!(!other.path().join("x.csv").exists());
    let csv_path = serde_json::from_str::<Value>(&csv_out).unwrap()["path"].as_str().unwrap().to_string();
    let kore_path = serde_json::from_str::<Value>(&kore_out).unwrap()["path"].as_str().unwrap().to_string();
    let exported_events = serde_json::from_str::<Value>(&csv_out).unwrap()["events"].as_u64().unwrap();
    assert_eq!(exported_events, 14 - 1);
    assert!(!std::fs::read_to_string(&csv_path).unwrap().contains("zebra-cheese"));
    s.stop();

    // ── tamper after forget is still detected ───────────────────────────────
    let log_dir = owner_dir.join("log");
    let seg = log_dir.join("events").join("0000000001.kore");
    let good = std::fs::read(&seg).unwrap();
    let mut bad = good.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 1;
    std::fs::write(&seg, &bad).unwrap();
    let mut s = Server::start(root.path(), fake_home.path(), owner);
    let (e, t) = s.call("self_verify", json!({}));
    say("self_verify after flipping a byte", &t);
    assert!(e && t.contains("0000000001.kore"), "{t}");
    s.stop();
    std::fs::write(&seg, good).unwrap();

    // ── re-import the exports into a second owner ───────────────────────────
    let owner2 = "e2e-import";
    let owner2_dir = root.path().join(owner2);
    std::fs::create_dir_all(&owner2_dir).unwrap();
    std::fs::write(owner2_dir.join("ingest_roots"), format!("{}\n", owner_dir.join("exports").display())).unwrap();
    let mut s2 = Server::start(root.path(), fake_home.path(), owner2);
    let t = s2.ok("self_ingest_kore", json!({"path": kore_path}));
    say("self_ingest_kore (second owner)", &t);
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!((v["added"].as_u64(), v["errors"].as_u64()), (Some(13), Some(0)));
    let t = s2.ok("self_ingest_kore", json!({"path": kore_path}));
    say("self_ingest_kore (again: dedupe)", &t);
    assert_eq!(serde_json::from_str::<Value>(&t).unwrap()["duplicate"], 13);
    let t = s2.ok(
        "self_ingest_csv",
        json!({"path": csv_path, "mapping": {"occurred_at": "occurred_at", "text": "text", "source": "source", "kind": "kind", "tags": "tags"}}),
    );
    say("self_ingest_csv of the CSV export (same content: all duplicates)", &t);
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!((v["added"].as_u64(), v["duplicate"].as_u64(), v["errors"].as_u64()), (Some(0), Some(13), Some(0)));
    let ver = s2.ok("self_verify", json!({}));
    say("self_verify (second owner)", &ver);
    assert!(ver.starts_with("OK"));
    let n = s2.ok("self_sql", json!({"sql": "SELECT count(*) FROM events"}));
    say("self_sql count (second owner)", &n);
    s2.stop();

    assert_eq!(std::fs::read_dir(fake_home.path()).unwrap().count(), 0, "something was written to the home directory");
}
