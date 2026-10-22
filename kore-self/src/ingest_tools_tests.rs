use super::*;

fn call(owner: &str, name: &str, a: Value) -> (bool, String) {
    safety::test_support::init();
    let r = crate::log_tools::handle(name, &a, owner).expect("log tool");
    (r["isError"].as_bool().unwrap_or(false), r["content"][0]["text"].as_str().unwrap().to_string())
}
fn ok(owner: &str, name: &str, a: Value) -> Value {
    let (e, t) = call(owner, name, a.clone());
    assert!(!e, "{name} {a} failed: {t}");
    serde_json::from_str(&t).unwrap_or_else(|_| json!({ "text": t }))
}
fn bad(owner: &str, name: &str, a: Value) -> String {
    let (e, t) = call(owner, name, a.clone());
    assert!(e, "{name} {a} should fail but gave: {t}");
    t
}
fn owner_dir(owner: &str) -> PathBuf {
    safety::test_support::init();
    let d = safety::owner_dir(owner);
    std::fs::create_dir_all(&d).unwrap();
    d
}
fn allow(owner: &str, folder: &Path) {
    let f = owner_dir(owner).join(ROOTS_FILE);
    let mut t = std::fs::read_to_string(&f).unwrap_or_default();
    t.push_str(&format!("# test\n{}\n", folder.display()));
    std::fs::write(f, t).unwrap();
}
fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}
fn event_texts(owner: &str) -> String {
    safety::test_support::init();
    let log = open(owner).unwrap();
    log.open_latest().rows::<EventRow>().iter().map(|e| format!("{} | {} | {} | {}
", e.source, e.occurred_at, e.text, e.redacted)).collect()
}

// ─── dates ───────────────────────────────────────────────────────────────────

#[test]
fn find_iso_cases() {
    assert_eq!(find_iso("on 2024-03-05 we met").as_deref(), Some("2024-03-05T00:00:00Z"));
    assert_eq!(find_iso("2024-03-05T10:20:30Z").as_deref(), Some("2024-03-05T10:20:30Z"));
    assert_eq!(find_iso("2024-03-05T10:20:30").as_deref(), Some("2024-03-05T10:20:30Z"));
    assert_eq!(find_iso("2024-03-05T10:20 odd").as_deref(), Some("2024-03-05T00:00:00Z"));
    for s in ["", "2024-13-01", "2024-02-30", "12024-03-05", "2024-03-051", "no date", "2024/03/05", "日本語 2024-3-5"] {
        assert_eq!(find_iso(s), None, "{s}");
    }
    assert_eq!(find_iso("2024-99-99 then 2023-01-02").as_deref(), Some("2023-01-02T00:00:00Z"));
    assert_eq!(find_iso("日本 2024-03-05 日本").as_deref(), Some("2024-03-05T00:00:00Z"));
}

#[test]
fn journal_date_order_front_matter_heading_filename() {
    let fm = "---\ntitle: x\ndate: 2023-05-06\n---\n# Entry 2020-01-01\nbody";
    assert_eq!(journal_date(fm, "2019-02-03.md").as_deref(), Some("2023-05-06T00:00:00Z"));
    assert_eq!(journal_date("---\ndate: \"2023-05-06T07:08:09Z\"\n---\nbody", "x.md").as_deref(), Some("2023-05-06T07:08:09Z"));
    assert_eq!(journal_date("---\ndate: sometime\n---\n# Notes from 2021-04-05\n", "2019-02-03.md").as_deref(), Some("2021-04-05T00:00:00Z"));
    assert_eq!(journal_date("# Intro\ntext 2000-01-01 not a heading\n## Day 2022-07-08\n# Later 2022-09-09", "n.md").as_deref(), Some("2022-07-08T00:00:00Z"));
    assert_eq!(journal_date("no headings here", "2019-02-03-notes.md").as_deref(), Some("2019-02-03T00:00:00Z"));
    assert_eq!(journal_date("no headings here", "2019-02-03.txt").as_deref(), Some("2019-02-03T00:00:00Z"));
    assert_eq!(journal_date("we met on 2022-01-01", "notes.md"), None);
    assert_eq!(journal_date("x", "2019-02-300.md"), None);
    assert_eq!(journal_date("x", "2019-13-01.md"), None);
    assert_eq!(journal_date("x", "short.md"), None);
    assert_eq!(journal_date("", "日本語日本語日本語.md"), None);
    assert_eq!(journal_date("---\n# 2020-02-02 comment\n---\nbody", "u.md"), None);
    assert_eq!(journal_date("\u{feff}---\ndate: 2023-05-06\n---\n", "u.md").as_deref(), Some("2023-05-06T00:00:00Z"));
}

// ─── allow-list ──────────────────────────────────────────────────────────────

#[test]
fn folder_must_be_inside_an_allowed_root() {
    let o = "ing-allow";
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "2024-01-01.md", "hello");
    let p = outside.path().to_string_lossy().to_string();
    let e = bad(o, "self_ingest_folder", json!({"path": p}));
    assert!(e.contains("not inside an allowed ingest root") && e.contains(ROOTS_FILE), "{e}");
    let e = bad(o, "self_ingest_folder", json!({"path": "../../.."}));
    assert!(e.contains("allowed ingest root"), "{e}");
    let e = bad(o, "self_ingest_folder", json!({"path": "does-not-exist"}));
    assert!(e.contains("cannot read"), "{e}");
    bad(o, "self_ingest_folder", json!({"path": ""}));
    bad(o, "self_ingest_folder", json!({}));
    allow(o, outside.path());
    let r = ok(o, "self_ingest_folder", json!({"path": p}));
    assert_eq!(r["added"], 1);
    let f = owner_dir(o).join(ROOTS_FILE);
    std::fs::write(&f, format!("{}\n/definitely/not/here\n", outside.path().display())).unwrap();
    let r = ok(o, "self_ingest_folder", json!({"path": p}));
    assert!(r["warnings"][0].as_str().unwrap().contains("ignored"), "{r}");
    let other = tempfile::tempdir().unwrap();
    bad(o, "self_ingest_folder", json!({"path": other.path().to_string_lossy()}));
}

#[test]
fn allow_list_and_pattern_files_are_not_reachable_through_tool_paths() {
    let o = "ing-reserved";
    owner_dir(o);
    for name in [ROOTS_FILE, PATTERN_FILE, "INGEST_ROOTS", "ingest_roots.txt"] {
        assert!(safety::safe_path(o, name).is_err(), "{name} must be refused by safe_path");
    }
    assert!(safety::safe_path(o, "context/x.md").is_ok());
    std::fs::write(owner_dir(o).join(PATTERN_FILE), "secretword\n").unwrap();
    let e = bad(o, "self_ingest_csv", json!({"path": PATTERN_FILE, "mapping": {"occurred_at": "a", "text": "b"}}));
    assert!(e.contains("reserved"), "{e}");
    write(&owner_dir(o), "2024-01-01-x.md", "plain");
    let r = ok(o, "self_ingest_folder", json!({"path": "."}));
    assert_eq!(r["added"], 1, "{r}");
    assert!(!event_texts(o).contains("secretword"));
}

// ─── folder ingest ───────────────────────────────────────────────────────────

fn journal_fixture(dir: &Path) {
    for i in 1..=10 {
        write(dir, &format!("2024-02-{i:02}-day.md"), &format!("# Day {i}\nWent for a walk and wrote entry number {i}."));
    }
    write(dir, "undated-one.md", "no date anywhere");
    write(dir, "sub/undated-two.txt", "also nothing");
    write(dir, "sub/2024-03-01-email.md", "Mailed jane.doe@example.com about the plan, call 555-123-4567.");
    write(dir, "frontmatter.md", "---\ndate: 2024-04-04\n---\nbody with front matter");
    write(dir, "heading.txt", "## Retro 2024-05-05\nbody with heading date");
    write(dir, "ignored.csv", "a,b");
    write(dir, ".hidden/2024-01-01.md", "hidden folders are skipped");
}

#[test]
fn folder_ingest_counts_dedupe_undated_redaction() {
    let o = "ing-folder";
    let j = tempfile::tempdir().unwrap();
    journal_fixture(j.path());
    allow(o, j.path());
    let p = j.path().to_string_lossy().to_string();

    let d = ok(o, "self_ingest_folder", json!({"path": p, "dry_run": true}));
    assert_eq!(d["dry_run"], true);
    assert_eq!(d["added"], 0);
    assert_eq!(d["would_add"], 13);
    assert_eq!(ok(o, "self_versions", json!({}))["latest"], 0, "dry run must not create a version");

    let r = ok(o, "self_ingest_folder", json!({"path": p}));
    assert_eq!(r["files_scanned"], 15, "{r}");
    assert_eq!(r["added"], 13, "{r}");
    assert_eq!(r["duplicate"], 0);
    assert_eq!(r["skipped_undated"], 2);
    let undated: Vec<&str> = r["undated_files"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(undated, vec!["sub/undated-two.txt", "undated-one.md"]);
    assert_eq!(r["redacted"], 1);
    assert_eq!(r["errors"], 0);
    assert_eq!(r["version"], 1);
    assert!(r["undated_note"].as_str().unwrap().contains("not stamped"));

    let t = event_texts(o);
    assert!(t.contains("journal:2024-02-01-day.md") && t.contains("2024-02-01T00:00:00Z"), "{t}");
    assert!(t.contains("journal:frontmatter.md") && t.contains("2024-04-04T00:00:00Z"));
    assert!(t.contains("journal:heading.txt") && t.contains("2024-05-05T00:00:00Z"));
    assert!(t.contains("journal:sub/2024-03-01-email.md"));
    assert!(t.contains("[REDACTED:email]") && t.contains("[REDACTED:phone]"), "{t}");
    assert!(!t.contains("jane.doe") && !t.contains("555-123-4567"), "{t}");
    assert!(!t.contains("undated") && !t.contains("hidden folders"));
    let flagged = ok(o, "self_sql", json!({"sql": "SELECT count(*) FROM events WHERE redacted = true"}));
    assert!(flagged["text"].as_str().unwrap().contains('1'), "{flagged}");

    let r2 = ok(o, "self_ingest_folder", json!({"path": p}));
    assert_eq!((r2["added"].as_u64(), r2["duplicate"].as_u64(), r2["skipped_undated"].as_u64()), (Some(0), Some(13), Some(2)), "{r2}");
    assert_eq!(r2["version"], 1);
    assert_eq!(ok(o, "self_versions", json!({}))["latest"], 1);

    write(j.path(), "2024-02-03-day.md", "# Day 3\nrewritten");
    let r3 = ok(o, "self_ingest_folder", json!({"path": p}));
    assert_eq!((r3["added"].as_u64(), r3["duplicate"].as_u64()), (Some(1), Some(12)), "{r3}");
    assert!(ok(o, "self_verify", json!({}))["text"].as_str().unwrap().starts_with("OK"));
}

#[test]
fn folder_ingest_limits_and_bad_files() {
    let o = "ing-limits";
    let j = tempfile::tempdir().unwrap();
    write(j.path(), "2024-01-01-big.md", &"x ".repeat(300_000));
    std::fs::write(j.path().join("2024-01-02-binary.md"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    write(j.path(), "2024-01-03-empty.md", "   \n");
    write(j.path(), "2024-01-04-ok.md", "fine");
    write(j.path(), "2024-01-05-also.md", "also fine");
    allow(o, j.path());
    let r = ok(o, "self_ingest_folder", json!({"path": j.path().to_string_lossy()}));
    assert_eq!(r["added"], 2, "{r}");
    assert_eq!(r["errors"], 3, "{r}");
    let d = r["error_details"].to_string();
    assert!(d.contains("larger than") && d.contains("UTF-8") && d.contains("empty"), "{d}");
    let e = bad(o, "self_ingest_folder", json!({"path": j.path().join("2024-01-04-ok.md").to_string_lossy()}));
    assert!(e.contains("folder"), "{e}");
}

#[test]
fn folder_ingest_file_cap() {
    let o = "ing-cap";
    let j = tempfile::tempdir().unwrap();
    for i in 0..(MAX_FILES + 5) {
        write(j.path(), &format!("2024-01-01-{i:05}.txt"), &format!("n{i}"));
    }
    allow(o, j.path());
    let r = ok(o, "self_ingest_folder", json!({"path": j.path().to_string_lossy(), "dry_run": true}));
    assert_eq!(r["truncated_at_file_cap"], true);
    assert_eq!(r["files_scanned"], MAX_FILES);
}

#[test]
fn symlinks_that_escape_are_skipped() {
    let o = "ing-symlink";
    let j = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "2024-01-01-secret.md", "outside text");
    write(j.path(), "2024-01-02-inside.md", "inside text");
    #[cfg(unix)]
    let made = {
        use std::os::unix::fs::symlink;
        symlink(outside.path().join("2024-01-01-secret.md"), j.path().join("2024-01-03-link.md")).is_ok() && symlink(outside.path(), j.path().join("linkdir")).is_ok()
    };
    #[cfg(windows)]
    let made = {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        symlink_file(outside.path().join("2024-01-01-secret.md"), j.path().join("2024-01-03-link.md")).is_ok()
            && symlink_dir(outside.path(), j.path().join("linkdir")).is_ok()
    };
    if !made {
        eprintln!("symlinks cannot be created here; escape test skipped");
        return;
    }
    allow(o, j.path());
    let r = ok(o, "self_ingest_folder", json!({"path": j.path().to_string_lossy()}));
    assert_eq!(r["added"], 1, "{r}");
    assert_eq!(r["skipped_symlinks"], 2, "{r}");
    assert!(!event_texts(o).contains("outside text"));
}

// ─── CSV ─────────────────────────────────────────────────────────────────────

#[test]
fn csv_ingest_bad_rows_dedupe_and_mapping() {
    let o = "ing-csv";
    let d = tempfile::tempdir().unwrap();
    let csv = "when,what,where,type,labels\r\n\
2024-01-01,\"Went to the gym, then \"\"home\"\"\",app,action,health;gym\r\n\
2024-01-02 08:30:00,\"multi\nline note\",,,\r\n\
,no date at all,app,note,\r\n\
not-a-date,bad date,app,note,\r\n\
2024-01-03,wrong kind,app,gossip,\r\n\
2024-01-04,only three,fields\r\n\
2024-01-05,mail me at bob@example.org,app,statement,x\r\n\
2024-01-01,\"Went to the gym, then \"\"home\"\"\",app,action,health;gym\r\n";
    write(d.path(), "data.csv", csv);
    allow(o, d.path());
    let p = d.path().join("data.csv").to_string_lossy().to_string();
    let m = json!({"occurred_at": "when", "text": "what", "source": "where", "kind": "type", "tags": "labels"});
    let r = ok(o, "self_ingest_csv", json!({"path": p, "mapping": m}));
    assert_eq!(r["rows"], 8, "{r}");
    assert_eq!(r["added"], 3, "{r}");
    assert_eq!(r["duplicate"], 1, "{r}");
    assert_eq!(r["rejected_no_date"], 2, "{r}");
    assert_eq!(r["errors"], 2, "{r}");
    assert_eq!(r["redacted"], 1);
    let det = r["error_details"].to_string();
    assert!(det.contains("row 3") && det.contains("row 4") && det.contains("row 5") && det.contains("row 6"), "{det}");
    let t = event_texts(o);
    assert!(t.contains("2024-01-02T08:30:00Z") && t.contains("csv:data.csv"), "{t}");
    assert!(t.contains("[REDACTED:email]") && !t.contains("bob@example.org"));
    assert!(!t.contains("no date at all") && !t.contains("bad date"));
    let q = ok(o, "self_sql", json!({"sql": "SELECT text, tags FROM events WHERE kind = 'action'"}));
    assert!(q["text"].as_str().unwrap().contains("Went to the gym, then \"home\""), "{q}");
    assert!(q["text"].as_str().unwrap().contains("health,gym"));
    let r2 = ok(o, "self_ingest_csv", json!({"path": p, "mapping": m}));
    assert_eq!((r2["added"].as_u64(), r2["duplicate"].as_u64()), (Some(0), Some(4)), "{r2}");
    let r3 = ok(o, "self_ingest_csv", json!({"path": p, "mapping": {"occurred_at": "when", "text": "what", "source": "type"}, "dry_run": true}));
    assert!(r3["would_add"].as_u64().unwrap() > 0 && r3["added"] == 0, "{r3}");
}

#[test]
fn csv_mapping_validation_and_headerless() {
    let o = "ing-csv2";
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "h.csv", "date,note\n2024-01-01,a\n");
    write(d.path(), "n.csv", "2024-02-01,first\n2024-02-02,second\n");
    allow(o, d.path());
    let h = d.path().join("h.csv").to_string_lossy().to_string();
    let n = d.path().join("n.csv").to_string_lossy().to_string();
    for (args, needle) in [
        (json!({"path": h}), "'mapping' is required"),
        (json!({"path": h, "mapping": {"text": "note"}}), "occurred_at is required"),
        (json!({"path": h, "mapping": {"occurred_at": "date"}}), "text is required"),
        (json!({"path": h, "mapping": {"occurred_at": "nope", "text": "note"}}), "no column named 'nope'"),
        (json!({"path": h, "mapping": {"occurred_at": "date", "text": "note", "color": "x"}}), "unknown mapping key"),
        (json!({"path": h, "mapping": {"occurred_at": "date", "text": "note"}, "has_header": "yes"}), "has_header"),
        (json!({"path": n, "mapping": {"occurred_at": 0, "text": 9}, "has_header": false}), "does not exist"),
        (json!({"path": n, "mapping": {"occurred_at": "date", "text": "note"}, "has_header": false}), "0-based column number"),
    ] {
        let e = bad(o, "self_ingest_csv", args.clone());
        assert!(e.contains(needle), "{args}: {e}");
    }
    let r = ok(o, "self_ingest_csv", json!({"path": h, "mapping": {"occurred_at": "DATE", "text": "Note"}}));
    assert_eq!(r["added"], 1);
    let r = ok(o, "self_ingest_csv", json!({"path": n, "mapping": {"occurred_at": 0, "text": "1"}, "has_header": false}));
    assert_eq!((r["rows"].as_u64(), r["added"].as_u64()), (Some(2), Some(2)), "{r}");
    write(d.path(), "q.csv", "date,note\n2024-01-01,\"open\n");
    let e = bad(o, "self_ingest_csv", json!({"path": d.path().join("q.csv").to_string_lossy(), "mapping": {"occurred_at": "date", "text": "note"}}));
    assert!(e.contains("unterminated"), "{e}");
    write(d.path(), "empty.csv", "");
    bad(o, "self_ingest_csv", json!({"path": d.path().join("empty.csv").to_string_lossy(), "mapping": {"occurred_at": "date", "text": "note"}}));
    let other = tempfile::tempdir().unwrap();
    write(other.path(), "x.csv", "date,note\n2024-01-01,a\n");
    let e = bad(o, "self_ingest_csv", json!({"path": other.path().join("x.csv").to_string_lossy(), "mapping": {"occurred_at": "date", "text": "note"}}));
    assert!(e.contains("allowed ingest root"), "{e}");
}

#[test]
fn csv_row_cap() {
    let o = "ing-csvcap";
    let d = tempfile::tempdir().unwrap();
    let mut s = String::from("d,t\n");
    for i in 0..(MAX_CSV_ROWS + 3) {
        s.push_str(&format!("2024-01-01,row {i}\n"));
    }
    write(d.path(), "big.csv", &s);
    allow(o, d.path());
    let r = ok(o, "self_ingest_csv", json!({"path": d.path().join("big.csv").to_string_lossy(), "mapping": {"occurred_at": "d", "text": "t"}, "dry_run": true}));
    assert_eq!(r["truncated_at_row_cap"], true);
    assert_eq!(r["rows"], MAX_CSV_ROWS);
}

// ─── export / import round trips ─────────────────────────────────────────────

fn sorted_event_rows(owner: &str) -> Vec<(String, String, String, String, String)> {
    let log = open(owner).unwrap();
    let mut v: Vec<_> = log.open_latest().rows::<EventRow>().into_iter().map(|e| (e.occurred_at, e.source, e.kind, e.text, e.tags)).collect();
    v.sort();
    v
}

#[test]
fn csv_to_import_to_export_to_compare_round_trip() {
    let (a, b) = ("ing-rt-a", "ing-rt-b");
    let d = tempfile::tempdir().unwrap();
    let csv = "when,what,src,kind,tags\n\
2024-01-01,\"comma, \"\"quote\"\" and\nnewline\",s1,note,t1;t2\n\
2024-01-02T10:00:00Z,plain,s2,decision,\n\
2024-01-03,ünï 日本 😀,s3,statement,tag\n";
    write(d.path(), "in.csv", csv);
    allow(a, d.path());
    let m = json!({"occurred_at": "when", "text": "what", "source": "src", "kind": "kind", "tags": "tags"});
    ok(a, "self_ingest_csv", json!({"path": d.path().join("in.csv").to_string_lossy(), "mapping": m}));
    let first = sorted_event_rows(a);
    assert_eq!(first.len(), 3);

    let e = ok(a, "self_export", json!({"path": "events.csv", "format": "csv"}));
    assert_eq!(e["events"], 3);
    let exported = PathBuf::from(e["path"].as_str().unwrap());
    assert!(exported.starts_with(owner_dir(a).canonicalize().unwrap().join("exports")), "{exported:?}");
    let text = std::fs::read_to_string(&exported).unwrap();
    let recs = crate::csvio::parse(&text).unwrap();
    assert_eq!(recs[0], EXPORT_CSV_HEADER.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(recs.len(), 4);

    allow(b, &owner_dir(a).join("exports"));
    let m2 = json!({"occurred_at": "occurred_at", "text": "text", "source": "source", "kind": "kind", "tags": "tags"});
    let r = ok(b, "self_ingest_csv", json!({"path": exported.to_string_lossy(), "mapping": m2}));
    assert_eq!((r["added"].as_u64(), r["errors"].as_u64(), r["rejected_no_date"].as_u64()), (Some(3), Some(0), Some(0)), "{r}");
    assert_eq!(sorted_event_rows(b), first, "csv round trip changed content");
    let r = ok(b, "self_ingest_csv", json!({"path": exported.to_string_lossy(), "mapping": m2}));
    assert_eq!((r["added"].as_u64(), r["duplicate"].as_u64()), (Some(0), Some(3)));
    let e2 = ok(b, "self_export", json!({"path": "exports/again.csv", "format": "csv"}));
    let t2 = std::fs::read_to_string(e2["path"].as_str().unwrap()).unwrap();
    let strip = |t: &str| -> Vec<Vec<String>> {
        let mut r = crate::csvio::parse(t).unwrap();
        r.iter_mut().for_each(|x| {
            x.pop();
        });
        r.sort();
        r
    };
    assert_eq!(strip(&text), strip(&t2));
}

#[test]
fn kore_export_import_round_trip_and_validation() {
    let (a, b) = ("ing-kore-a", "ing-kore-b");
    ok(a, "self_event_add", json!({"text": "first, with comma", "occurred_at": "2024-01-01", "source": "s", "tags": ["x", "y"]}));
    ok(a, "self_event_add", json!({"text": "mail zed@example.com", "occurred_at": "2024-01-02T03:04:05Z", "kind": "decision"}));
    ok(a, "self_event_add", json!({"text": "ünï 日本 😀", "occurred_at": "2024-01-03", "kind": "outcome"}));
    let e = ok(a, "self_export", json!({"path": "snapshot.kore", "format": "kore"}));
    assert_eq!(e["events"], 3);
    let path = e["path"].as_str().unwrap().to_string();
    allow(b, &owner_dir(a).join("exports"));
    let r = ok(b, "self_ingest_kore", json!({"path": path}));
    assert_eq!((r["added"].as_u64(), r["errors"].as_u64()), (Some(3), Some(0)), "{r}");
    assert_eq!(sorted_event_rows(b), sorted_event_rows(a));
    let q = ok(b, "self_sql", json!({"sql": "SELECT count(*) FROM events WHERE redacted = true"}));
    assert!(q["text"].as_str().unwrap().contains('1'), "{q}");
    let r = ok(b, "self_ingest_kore", json!({"path": path}));
    assert_eq!((r["added"].as_u64(), r["duplicate"].as_u64()), (Some(0), Some(3)), "{r}");
    // an edited row (id no longer matches the content) is rejected
    let bytes = std::fs::read(&path).unwrap();
    let mut edited = bytes.clone();
    if let Some(at) = edited.windows(5).position(|w| w == b"first") {
        edited[at] = b'F';
        let ex = owner_dir(a).join("exports");
        std::fs::write(ex.join("edited.kore"), &edited).unwrap();
        let c = "ing-kore-c";
        allow(c, &ex);
        let (e, t) = call(c, "self_ingest_kore", json!({"path": ex.join("edited.kore").to_string_lossy()}));
        if !e {
            let v: Value = serde_json::from_str(&t).unwrap();
            assert!(v["errors"].as_u64().unwrap() >= 1, "edited row must be rejected: {t}");
            assert!(!event_texts(c).contains("First, with comma"));
        }
    }
    let ex = owner_dir(a).join("exports");
    std::fs::write(ex.join("junk.kore"), b"not a kore file").unwrap();
    bad("ing-kore-b", "self_ingest_kore", json!({"path": ex.join("junk.kore").to_string_lossy()}));
    // wrong schema (a memory table saved by self_save is not an events export)
    let e = bad("ing-kore-b", "self_ingest_kore", json!({"path": "exports"}));
    assert!(e.contains("file"), "{e}");
}

#[test]
fn export_paths_are_confined_to_exports() {
    let o = "ing-export-paths";
    ok(o, "self_event_add", json!({"text": "x", "occurred_at": "2024-01-01"}));
    let outside = tempfile::tempdir().unwrap();
    for p in [
        outside.path().join("x.csv").to_string_lossy().to_string(),
        "../x.csv".into(),
        "exports/../../x.csv".into(),
        "..".into(),
        "".into(),
        "exports/".into(),
        format!("../{ROOTS_FILE}"),
    ] {
        let (e, t) = call(o, "self_export", json!({"path": p, "format": "csv"}));
        assert!(e, "path {p:?} must be rejected, got {t}");
    }
    assert!(!outside.path().join("x.csv").exists());
    assert!(!owner_dir(o).join(ROOTS_FILE).exists(), "the allow-list file must not be creatable by export");
    bad(o, "self_export", json!({"path": "a.csv", "format": "xml"}));
    bad(o, "self_export", json!({"path": "a.csv"}));
    let r = ok(o, "self_export", json!({"path": "sub/dir/e.kore", "format": "kore"}));
    assert_eq!(r["relative"], "sub/dir/e.kore");
    let o2 = "ing-export-empty";
    let r = ok(o2, "self_export", json!({"path": "e.csv", "format": "csv"}));
    assert_eq!(r["events"], 0);
    let r = ok(o2, "self_export", json!({"path": "e.kore", "format": "kore"}));
    assert_eq!(r["events"], 0);
    ok(o2, "self_export", json!({"path": "e.csv", "format": "csv"}));
    let names: Vec<String> = std::fs::read_dir(owner_dir(o2).join("exports")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
}

// ─── redaction at the other doors ────────────────────────────────────────────

#[test]
fn event_add_and_self_ingest_path_redact_before_storing() {
    let o = "ing-redact";
    owner_dir(o);
    std::fs::write(owner_dir(o).join(PATTERN_FILE), "# mine\nProject Falcon\n").unwrap();
    let r = ok(o, "self_event_add", json!({"text": "Mail a.b@c.io about project falcon", "occurred_at": "2024-01-01"}));
    assert_eq!(r["redacted"], json!(["custom", "email"]));
    let t = event_texts(o);
    assert!(t.contains("[REDACTED:email]") && t.contains("[REDACTED:custom]"), "{t}");
    assert!(!t.contains("a.b@c.io") && !t.to_lowercase().contains("falcon"), "{t}");
    let r2 = ok(o, "self_event_add", json!({"text": "Mail a.b@c.io about project falcon", "occurred_at": "2024-01-01"}));
    assert_eq!(r2["duplicate"], true);
    let r3 = ok(o, "self_event_add", json!({"text": "nothing private", "occurred_at": "2024-01-02"}));
    assert!(r3.get("redacted").is_none());
    let (id, dup) = crate::log_tools::record_ingest(o, "call me on 555-123-4567", "note").unwrap();
    assert!(!id.is_empty() && !dup);
    assert!(!event_texts(o).contains("555-123-4567"));
}

// ─── forget ──────────────────────────────────────────────────────────────────

#[test]
fn forget_tool_end_to_end() {
    let o = "ing-forget";
    let j = tempfile::tempdir().unwrap();
    write(j.path(), "2024-01-01-a.md", "alpha entry zebra-cheese");
    write(j.path(), "2024-01-02-b.md", "beta entry");
    write(j.path(), "2024-01-03-c.md", "gamma entry");
    allow(o, j.path());
    let p = j.path().to_string_lossy().to_string();
    ok(o, "self_ingest_folder", json!({"path": p}));
    let log = open(o).unwrap();
    let id = log.open_latest().rows::<EventRow>().into_iter().find(|e| e.source == "journal:2024-01-01-a.md").unwrap().event_id;
    drop(log);
    let rev = ok(o, "self_belief_revise", json!({"key": "k", "stance": "s", "confidence": 0.5, "caused_by": [id]}));
    assert!(rev["rev_id"].is_string());
    let before = ok(o, "self_versions", json!({}))["latest"].as_u64().unwrap();

    assert!(bad(o, "self_forget", json!({"event_id": id})).contains("confirm"));
    assert!(bad(o, "self_forget", json!({"event_id": id, "confirm": "true"})).contains("confirm"));
    assert!(bad(o, "self_forget", json!({"confirm": true})).contains("event_id or source_prefix"));
    assert!(bad(o, "self_forget", json!({"event_id": id, "source_prefix": "journal:", "confirm": true})).contains("exactly one"));
    assert!(bad(o, "self_forget", json!({"event_id": "00ff", "confirm": true})).contains("not found"));
    assert!(bad(o, "self_forget", json!({"source_prefix": "nothing:", "confirm": true})).contains("nothing to forget"));
    assert_eq!(ok(o, "self_versions", json!({}))["latest"].as_u64().unwrap(), before, "failed calls must not write");

    let r = ok(o, "self_forget", json!({"event_id": id, "confirm": true}));
    assert_eq!((r["forgotten"].as_u64(), r["causes_dropped"].as_u64()), (Some(1), Some(1)), "{r}");
    assert_eq!(r["revisions_now_unsupported"].as_array().unwrap().len(), 1);
    assert!(ok(o, "self_verify", json!({}))["text"].as_str().unwrap().starts_with("OK"));
    for v in 0..=ok(o, "self_versions", json!({}))["latest"].as_u64().unwrap() {
        let t = ok(o, "self_sql", json!({"sql": "SELECT text FROM events", "as_of_version": v}));
        assert!(!t["text"].as_str().unwrap().contains("zebra-cheese"), "resurrected at version {v}");
    }
    let r = ok(o, "self_forget", json!({"event_id": id, "confirm": true}));
    assert_eq!((r["forgotten"].as_u64(), r["already_forgotten"].as_u64()), (Some(0), Some(1)), "{r}");
    let h = ok(o, "self_belief_history", json!({"key": "k"}));
    assert_eq!(h["history"][0]["unsupported"], true);
    let r = ok(o, "self_ingest_folder", json!({"path": p}));
    assert_eq!((r["added"].as_u64(), r["duplicate"].as_u64(), r["suppressed_forgotten"].as_u64()), (Some(0), Some(2), Some(1)), "{r}");
    assert!(!event_texts(o).contains("zebra-cheese"));
    let r = ok(o, "self_forget", json!({"source_prefix": "journal:2024-01-0", "confirm": true}));
    assert_eq!(r["forgotten"], 2);
    assert!(ok(o, "self_verify", json!({}))["text"].as_str().unwrap().starts_with("OK"));
}
