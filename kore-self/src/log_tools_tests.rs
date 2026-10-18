use super::*;

fn call(owner: &str, name: &str, a: Value) -> (bool, String) {
    crate::safety::test_support::init();
    let r = handle(name, &a, owner).expect("log tool");
    (r["isError"].as_bool().unwrap_or(false), r["content"][0]["text"].as_str().unwrap().to_string())
}
fn ok_json(owner: &str, name: &str, a: Value) -> Value {
    let (e, t) = call(owner, name, a);
    assert!(!e, "{name} failed: {t}");
    serde_json::from_str(&t).unwrap()
}
fn add(owner: &str, text: &str, at: &str) -> String {
    ok_json(owner, "self_event_add", json!({"text": text, "occurred_at": at}))["event_id"].as_str().unwrap().to_string()
}

#[test]
fn event_add_validates_and_dedupes() {
    let o = "lt-event";
    for bad in [
        json!({"text": "x"}),
        json!({"text": "x", "occurred_at": "yesterday"}),
        json!({"occurred_at": "2024-01-01"}),
        json!({"text": "  ", "occurred_at": "2024-01-01"}),
        json!({"text": "x", "occurred_at": "2024-02-31"}),
        json!({"text": "x", "occurred_at": "2024-01-01", "kind": "gossip"}),
        json!({"text": "x", "occurred_at": "2024-01-01", "tags": "a"}),
    ] {
        let (e, t) = call(o, "self_event_add", bad.clone());
        assert!(e, "accepted {bad}: {t}");
    }
    let a = ok_json(o, "self_event_add", json!({"text": "Went running", "occurred_at": "2024-03-01", "tags": ["health"]}));
    assert_eq!(a["duplicate"], false);
    assert_eq!(a["version"], 1);
    let b = ok_json(o, "self_event_add", json!({"text": "Went  running ", "occurred_at": "2024-03-01"}));
    assert_eq!(b["duplicate"], true);
    assert_eq!(b["version"], 1);
    assert_eq!(a["event_id"], b["event_id"]);
}

#[test]
fn revise_history_and_authors() {
    let o = "lt-revise";
    let e1 = add(o, "Tried tabs", "2024-01-05");
    let e2 = add(o, "Switched to spaces", "2024-02-10T08:00:00Z");
    // unknown citation rejected
    let (e, t) = call(o, "self_belief_revise", json!({"key": "indent", "stance": "tabs", "confidence": 0.5, "caused_by": ["deadbeef"]}));
    assert!(e && t.contains("not found"), "{t}");
    for bad in [
        json!({"key":"k","stance":"s","confidence":1.5,"caused_by":[]}),
        json!({"key":"k","stance":"s","caused_by":[]}),
        json!({"key":"k","stance":"s","confidence":0.5,"caused_by":[],"entity_type":"mood"}),
        json!({"key":"k","stance":"s","confidence":0.5,"caused_by":[],"valid_from":"soon"}),
        json!({"key":"k","stance":"s","confidence":0.5,"caused_by":"x"}),
    ] {
        let r = handle("self_belief_revise", &bad, o).unwrap();
        assert_eq!(r["isError"], true, "accepted {bad}");
    }
    // an 'author' argument is ignored: tools can only write author=user
    let sneaky = handle("self_belief_revise", &json!({"key":"sneak","stance":"s","confidence":0.5,"caused_by":[e1],"author":"rule:x"}), o).unwrap();
    assert!(sneaky["isError"].is_null(), "{sneaky}");

    let r1 = ok_json(o, "self_belief_revise", json!({"key": "indent", "stance": "tabs", "confidence": 0.6, "caused_by": [e1]}));
    assert_eq!(r1["valid_from"], "2024-01-05T00:00:00Z");
    assert!(r1.get("note").is_none());
    let r2 = ok_json(o, "self_belief_revise", json!({"key": "indent", "stance": "spaces", "confidence": 0.9, "rationale": "team style", "caused_by": [e2, e1]}));
    assert_eq!(r2["valid_from"], "2024-02-10T08:00:00Z");
    let dup = ok_json(o, "self_belief_revise", json!({"key": "indent", "stance": "spaces", "confidence": 0.9, "rationale": "team style", "caused_by": [e1, e2]}));
    assert_eq!(dup["duplicate"], true);
    assert_eq!(dup["rev_id"], r2["rev_id"]);
    let un = ok_json(o, "self_belief_revise", json!({"key": "tea", "stance": "likes tea", "confidence": 0.3, "caused_by": []}));
    assert!(un["note"].as_str().unwrap().contains("unsupported"));

    let h = ok_json(o, "self_belief_history", json!({"key": "indent"}));
    let hist = h["history"].as_array().unwrap();
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[0]["stance"], "tabs");
    assert_eq!(hist[1]["supersedes"], hist[0]["rev_id"]);
    assert_eq!(hist[1]["caused_by"].as_array().unwrap().len(), 2);
    assert_eq!(hist[0]["caused_by"][0]["text"], "Tried tabs");
    assert_eq!(hist[0]["caused_by"][0]["occurred_at"], "2024-01-05T00:00:00Z");
    for r in hist {
        assert_eq!(r["author"], "user");
    }
    let (_, t) = call(o, "self_sql", json!({"sql": "SELECT author FROM revisions"}));
    assert!(!t.contains("rule:") && !t.contains("legacy"), "{t}");
    assert!(call(o, "self_belief_history", json!({})).0);
}

#[test]
fn sql_as_of_versions_and_guard() {
    let o = "lt-sql";
    let e1 = add(o, "alpha", "2024-01-01");
    ok_json(o, "self_belief_revise", json!({"key": "k", "stance": "one", "confidence": 0.5, "caused_by": [e1]}));
    ok_json(o, "self_belief_revise", json!({"key": "k", "stance": "two", "confidence": 0.6, "caused_by": [e1], "valid_from": "2024-06-01"}));
    let (_, t1) = call(o, "self_sql", json!({"sql": "SELECT stance FROM beliefs_current", "as_of_version": 2}));
    assert!(t1.contains("one") && !t1.contains("two"), "{t1}");
    let (_, t2) = call(o, "self_sql", json!({"sql": "SELECT stance FROM beliefs_current", "as_of_version": 3}));
    assert!(t2.contains("two"), "{t2}");
    let (e, t) = call(o, "self_sql", json!({"sql": "SELECT 1", "as_of_version": 99}));
    assert!(e && t.contains("not found"), "{t}");
    assert!(call(o, "self_sql", json!({"sql": "SELECT 1", "as_of_version": 1, "as_of_time": "2030-01-01"})).0);
    for bad in ["DELETE FROM events", "SELECT 1; DROP TABLE events", "COPY events TO 'x'", ""] {
        let (e, t) = call(o, "self_sql", json!({"sql": bad}));
        assert!(e, "accepted {bad}: {t}");
    }
    let (e, t) = call(
        o,
        "self_sql",
        json!({"sql": "SELECT r.key, r.stance, e.text FROM revisions r JOIN revision_causes c ON r.rev_id = c.rev_id JOIN events e ON c.event_id = e.event_id"}),
    );
    assert!(!e, "{t}");
    assert!(t.contains("r.stance") && t.contains("alpha"), "{t}");
    assert!(call(o, "self_sql", json!({"sql": "SELECT 1", "as_of_time": "2000-01-01"})).0);
    assert!(!call(o, "self_sql", json!({"sql": "SELECT count(*) FROM events", "as_of_time": "2999-01-01"})).0);
}

#[test]
fn versions_diff_verify_and_tamper() {
    let o = "lt-vers";
    let (_, t) = call(o, "self_verify", json!({}));
    assert!(t.starts_with("OK"), "{t}");
    add(o, "a", "2024-01-01");
    add(o, "b", "2024-01-02");
    let v = ok_json(o, "self_versions", json!({}));
    assert_eq!(v["latest"], 2);
    assert_eq!(v["versions"].as_array().unwrap().len(), 3);
    assert_eq!(v["versions"][2]["rows"]["events"], 2);
    let d = ok_json(o, "self_diff", json!({"v1": 1, "v2": 2}));
    assert_eq!(d["counts"]["events"], 1);
    assert_eq!(d["events"][0]["text"], "b");
    for bad in [json!({"v1": 2, "v2": 1}), json!({"v1": 0, "v2": 9}), json!({"v1": 0}), json!({"v1": "a", "v2": 1})] {
        assert!(call(o, "self_diff", bad).0);
    }
    let (e, t) = call(o, "self_verify", json!({}));
    assert!(!e && t.starts_with("OK: 3 version(s)"), "{t}");
    let seg = crate::safety::owner_dir(o).join("log").join("events").join("0000000002.kore");
    let mut b = std::fs::read(&seg).unwrap();
    let n = b.len() / 2;
    b[n] ^= 0xff;
    std::fs::write(&seg, b).unwrap();
    let (e, t) = call(o, "self_verify", json!({}));
    assert!(e && t.contains("VERIFY FAILED") && t.contains("0000000002.kore"), "{t}");
    // other tools refuse to run on a damaged log
    assert!(call(o, "self_event_add", json!({"text": "c", "occurred_at": "2024-01-03"})).0);
}

#[test]
fn conflict_is_a_clear_error() {
    let o = "lt-conflict";
    add(o, "a", "2024-01-01");
    let root = crate::safety::data_root();
    let mut stale = VersionedLog::open(&root, o).unwrap();
    add(o, "b", "2024-01-02");
    let ev = NewEvent { source: "x".into(), occurred_at: "2024-01-03".into(), kind: "note".into(), text: "c".into(), ..Default::default() };
    let e = stale.append_events(&[ev]).unwrap_err();
    assert!(matches!(e, LogError::Conflict(_)));
    let r = log_err(e);
    assert_eq!(r["isError"], true);
    let t = r["content"][0]["text"].as_str().unwrap();
    assert!(t.contains("Conflict") && t.contains("retry"), "{t}");
}

#[test]
fn migrate_v1_is_idempotent_and_marks_unsupported() {
    crate::safety::test_support::init();
    let o = "lt-migrate";
    let b = vec![
        ("rust".to_string(), "good".to_string(), 0.7, "2023-05-06T10:00:00Z".to_string(), vec!["[t] was: 'meh' (40%)".to_string()]),
        ("empty".to_string(), "".to_string(), 0.5, "x".to_string(), vec![]),
        ("odd".to_string(), "s".to_string(), f64::NAN, "garbage".to_string(), vec![]),
    ];
    let parse = |r: Value| -> Value { serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap() };
    let j = parse(migrate_v1(o, &b));
    assert_eq!(j["imported"], 2);
    assert_eq!(j["skipped_invalid"], 1);
    let j2 = parse(migrate_v1(o, &b));
    assert_eq!(j2["imported"], 0);
    assert_eq!(j2["skipped_existing"], 2);
    assert_eq!(j2["version"], j["version"]);
    let (_, t) = call(o, "self_sql", json!({"sql": "SELECT key, author, unsupported, valid_from FROM revisions"}));
    assert!(t.contains("2023-05-06"), "{t}");
    assert_eq!(t.matches("legacy").count(), 2, "{t}");
    let (_, c) = call(o, "self_sql", json!({"sql": "SELECT count(*) FROM revision_causes"}));
    assert!(c.contains('0'), "{c}");
}

#[test]
fn invalid_owner_is_rejected() {
    crate::safety::test_support::init();
    for o in ["../x", "", "a/b", "a.b"] {
        let r = handle("self_versions", &json!({}), o).unwrap();
        assert_eq!(r["isError"], true, "owner {o:?}");
    }
}

#[test]
fn ingest_appends_event_once() {
    crate::safety::test_support::init();
    let o = "lt-ingest";
    let (id, _) = record_ingest(o, "I prefer small commits", "preference").unwrap();
    let (id2, _) = record_ingest(o, "I prefer small commits", "preference").unwrap();
    // same second -> same event id; across a second boundary a new event is legitimate
    let (_, t) = call(o, "self_sql", json!({"sql": "SELECT source, kind FROM events"}));
    assert!(t.contains("self_ingest") && t.contains("note"), "{t}");
    if id == id2 {
        assert!(t.contains("(1 rows)"), "{t}");
    }
}
