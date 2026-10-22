//! Tests for `forget` (the documented exception to append-only).
use super::super::forget::Crash;
use super::*;

const SECRET: &str = "the secret plan about zebra-cheese-7731";

/// events at versions 1..=4 (event 2 is the secret), revision A cites {e0, secret},
/// revision B cites only the secret; returns (log, secret_id, rev_a, rev_b, ids)
fn forget_fixture(root: &Path) -> (VersionedLog, String, String, String, Vec<String>) {
    let mut l = new_log(root);
    let mut ids = vec![];
    for (i, t) in ["alpha one", "beta two", SECRET, "delta four"].iter().enumerate() {
        let info = l.append_events(&[ev("fx", &format!("2024-01-0{}", i + 1), t)]).unwrap();
        ids.push(info.event_ids[0].clone());
    }
    let a = l.revise(rev("k1", "stance a", 0.5, "2024-01-03", "user", vec![ids[0].clone(), ids[2].clone()])).unwrap();
    let b = l.revise(rev("k2", "stance b", 0.5, "2024-01-03", "user", vec![ids[2].clone()])).unwrap();
    (l, ids[2].clone(), a, b, ids)
}

fn all_bytes_contain(dir: &Path, needle: &str) -> bool {
    fn walk(p: &Path, needle: &[u8], hit: &mut bool) {
        if let Ok(rd) = fs::read_dir(p) {
            for f in rd.flatten() {
                let path = f.path();
                if path.is_dir() {
                    walk(&path, needle, hit);
                } else if let Ok(b) = fs::read(&path) {
                    if b.windows(needle.len()).any(|w| w == needle) {
                        *hit = true;
                    }
                }
            }
        }
    }
    let mut hit = false;
    walk(dir, needle.as_bytes(), &mut hit);
    hit
}

#[test]
fn forget_removes_text_from_every_version_and_keeps_verify_ok() {
    let d = tempfile::tempdir().unwrap();
    let (mut l, secret, rev_a, rev_b, ids) = forget_fixture(d.path());
    assert!(all_bytes_contain(l.dir(), "zebra-cheese-7731"), "fixture must contain the text on disk");
    let before_versions = l.latest_version();
    let r = l.forget_events(&[secret.clone()]).unwrap();
    assert!(r.created);
    assert_eq!(r.forgotten, 1);
    assert_eq!(r.causes_dropped, 2);
    assert_eq!(r.revisions_orphaned, vec![rev_b.clone()], "only revision B lost its last citation");
    assert_eq!(l.latest_version(), before_versions + 1);
    l.verify().unwrap();

    for v in 0..=l.latest_version() {
        let s = l.open_at(AsOf::Version(v)).unwrap();
        assert!(!s.rows::<EventRow>().iter().any(|e| e.text.contains("zebra")), "text resurrected at version {v}");
        assert!(!s.rows::<CauseRow>().iter().any(|c| c.event_id == secret));
    }
    assert!(!all_bytes_contain(l.dir(), "zebra-cheese-7731"), "text still present in some file");
    assert!(!all_bytes_contain(l.dir(), &secret), "event id still present in some file");
    let latest = l.open_latest();
    assert_eq!(latest.rows::<EventRow>().len(), 3);
    let revs = latest.rows::<RevisionRow>();
    assert!(revs.iter().any(|r| r.rev_id == rev_a) && revs.iter().any(|r| r.rev_id == rev_b));
    assert_eq!(latest.rows::<CauseRow>().len(), 1);
    let q = run_readonly_sql(&latest, "SELECT key FROM revisions WHERE unsupported = 1").unwrap();
    assert_eq!(block_strings(&q), vec![vec!["k2"]]);
    let re = VersionedLog::open(d.path(), "tester").unwrap();
    assert_eq!(re.open_latest().content_hash(), l.open_latest().content_hash());
    re.verify().unwrap();
    let m = String::from_utf8(manifest_bytes(&l)).unwrap();
    assert!(m.contains("\"forget\"") && m.contains(&forget_hash(&secret)));
    assert!(!m.contains(&secret) && !m.contains("zebra"));
    assert!(re.has_event(&ids[0]) && !re.has_event(&secret) && re.is_forgotten(&secret));
}

#[test]
fn forget_then_tamper_is_still_detected() {
    let d = tempfile::tempdir().unwrap();
    let (mut l, secret, _, _, _) = forget_fixture(d.path());
    l.forget_events(&[secret]).unwrap();
    l.verify().unwrap();
    let seg = l.dir().join("events").join("0000000001.kore");
    let mut b = fs::read(&seg).unwrap();
    let mid = b.len() / 2;
    b[mid] ^= 1;
    fs::write(&seg, b).unwrap();
    match l.verify() {
        Err(VerifyError::ChecksumMismatch { file }) => assert!(file.contains("0000000001.kore")),
        other => panic!("expected checksum mismatch, got {other:?}"),
    }
    // editing the forget record in the manifest breaks the chain
    let d2 = tempfile::tempdir().unwrap();
    let (mut l2, secret2, _, _, _) = forget_fixture(d2.path());
    l2.forget_events(&[secret2]).unwrap();
    let p = l2.dir().join("MANIFEST");
    let m = fs::read_to_string(&p).unwrap();
    let h = m.split("\"removed\":[\"").nth(1).unwrap()[..64].to_string();
    fs::write(&p, m.replace(&h, &"0".repeat(64))).unwrap();
    assert!(matches!(l2.verify(), Err(VerifyError::ChainBroken { .. })));
}

#[test]
fn forget_is_idempotent_and_unknown_ids_error_without_changes() {
    let d = tempfile::tempdir().unwrap();
    let (mut l, secret, _, _, ids) = forget_fixture(d.path());
    let before = manifest_bytes(&l);
    let e = l.forget_events(&["deadbeef".into()]).unwrap_err();
    assert!(matches!(e, LogError::NotFound(_)), "{e}");
    let e = l.forget_events(&[ids[0].clone(), "deadbeef".into()]).unwrap_err();
    assert!(matches!(e, LogError::NotFound(_)));
    assert_eq!(manifest_bytes(&l), before);
    assert!(l.has_event(&ids[0]));
    assert!(matches!(l.forget_events(&[]), Err(LogError::Invalid(_))));
    let r1 = l.forget_events(&[secret.clone()]).unwrap();
    let after = manifest_bytes(&l);
    let r2 = l.forget_events(&[secret.clone()]).unwrap();
    assert!(r1.created && !r2.created);
    assert_eq!(r2.already_forgotten, 1);
    assert_eq!(r2.version, r1.version);
    assert_eq!(manifest_bytes(&l), after);
}

#[test]
fn forgotten_events_are_not_resurrected_by_reingest() {
    let d = tempfile::tempdir().unwrap();
    let (mut l, secret, _, _, _) = forget_fixture(d.path());
    l.forget_events(&[secret.clone()]).unwrap();
    let v = l.latest_version();
    let info = l.append_events(&[ev("fx", "2024-01-03", SECRET)]).unwrap();
    assert!(!info.created);
    assert_eq!(info.suppressed, 1);
    assert_eq!(l.latest_version(), v);
    let mut re = VersionedLog::open(d.path(), "tester").unwrap();
    let info = re.append_events(&[ev("fx", "2024-01-03", SECRET), ev("fx", "2024-02-02", "something new")]).unwrap();
    assert_eq!((info.created, info.suppressed), (true, 1));
    assert!(!all_bytes_contain(re.dir(), "zebra-cheese-7731"));
    re.verify().unwrap();
}

#[test]
fn forget_by_source_prefix_and_later_commits_still_work() {
    let d = tempfile::tempdir().unwrap();
    let mut l = new_log(d.path());
    l.append_events(&[ev("journal:a.md", "2024-01-01", "one"), ev("journal:b.md", "2024-01-02", "two"), ev("other", "2024-01-03", "three")]).unwrap();
    let ids = l.event_ids_by_source_prefix("journal:");
    assert_eq!(ids.len(), 2);
    let r = l.forget_events(&ids).unwrap();
    assert_eq!(r.forgotten, 2);
    assert_eq!(l.open_latest().rows::<EventRow>().len(), 1);

    // an all-removed segment is deleted and its manifest reference dropped
    let d2 = tempfile::tempdir().unwrap();
    let mut l2 = new_log(d2.path());
    let a = l2.append_events(&[ev("x", "2024-01-01", "only")]).unwrap();
    l2.forget_events(&a.event_ids).unwrap();
    assert!(!l2.dir().join("events").join("0000000001.kore").exists());
    l2.verify().unwrap();
    // appending after a forget works and verifies; a second forget too
    l2.append_events(&[ev("x", "2024-01-02", "next")]).unwrap();
    let n = l2.append_events(&[ev("x", "2024-01-03", "third")]).unwrap();
    l2.forget_events(&n.event_ids).unwrap();
    l2.verify().unwrap();
    let re = VersionedLog::open(d2.path(), "tester").unwrap();
    assert_eq!(re.open_latest().rows::<EventRow>().len(), 1);
    re.verify().unwrap();
}

#[test]
fn forget_edits_predictions_and_outcomes_that_cited_the_event() {
    let d = tempfile::tempdir().unwrap();
    let (mut l, secret, _, _, ids) = forget_fixture(d.path());
    let p = NewPrediction {
        made_at: "2024-02-01".into(),
        question_id: "q".into(),
        target: "t".into(),
        options: vec!["a".into(), "b".into()],
        probs: vec![0.5, 0.5],
        model_id: "m".into(),
        model_version: "1".into(),
        features_cite: vec![secret.clone(), ids[0].clone()],
        resolves_by: None,
    };
    let pid = l.record_prediction(p).unwrap().pred_ids[0].clone();
    l.record_outcome(NewOutcome { pred_id: pid, resolved_at: "2024-03-01".into(), actual: "a".into(), outcome_event_id: Some(secret.clone()) }).unwrap();
    let r = l.forget_events(&[secret.clone()]).unwrap();
    assert_eq!((r.predictions_edited, r.outcomes_edited), (1, 1));
    let s = l.open_latest();
    let pr = s.rows::<PredictionRow>();
    assert_eq!(pr[0].features_cite, serde_json::to_string(&vec![ids[0].clone()]).unwrap());
    assert_eq!(s.rows::<OutcomeRow>()[0].outcome_event_id, None);
    l.verify().unwrap();
    assert!(!all_bytes_contain(l.dir(), &secret));
}

#[test]
fn forget_crash_safety() {
    let cases: Vec<(&str, Crash, bool)> = vec![
        ("before marker", Crash::BeforeMarker, false),
        ("after marker", Crash::AfterMarker, true),
        ("apply op 0", Crash::DuringApply(0), true),
        ("apply op 1", Crash::DuringApply(1), true),
        ("apply op 2", Crash::DuringApply(2), true),
        ("apply op 3", Crash::DuringApply(3), true),
    ];
    for (label, crash, forgotten) in cases {
        let d = tempfile::tempdir().unwrap();
        let (mut l, secret, _, _, _) = forget_fixture(d.path());
        let head_before = l.snapshot_hash(l.latest_version()).unwrap();
        let err = l.forget_inner(&[secret.clone()], Some(crash)).unwrap_err();
        assert!(err.to_string().contains("simulated crash"), "{label}: {err}");
        // "restart": reopening recovers; the log is valid and either fully old or fully new
        let re = VersionedLog::open(d.path(), "tester").unwrap_or_else(|e| panic!("{label}: open failed: {e}"));
        re.verify().unwrap_or_else(|e| panic!("{label}: verify failed: {e}"));
        assert!(!re.dir().join("forget.staging").exists() && !re.dir().join("FORGET_PENDING").exists(), "{label}: leftovers");
        if forgotten {
            assert!(!re.has_event(&secret), "{label}");
            assert!(re.is_forgotten(&secret), "{label}");
            assert!(!all_bytes_contain(re.dir(), "zebra-cheese-7731"), "{label}: text on disk");
            assert_eq!(re.latest_version(), 7, "{label}");
        } else {
            assert!(re.has_event(&secret), "{label}");
            assert_eq!(re.snapshot_hash(re.latest_version()).unwrap(), head_before, "{label}: old state must be untouched");
        }
    }
    // after a rolled-back attempt the forget can simply be repeated
    let d = tempfile::tempdir().unwrap();
    let (mut l, secret, _, _, _) = forget_fixture(d.path());
    let _ = l.forget_inner(&[secret.clone()], Some(Crash::BeforeMarker));
    let mut re = VersionedLog::open(d.path(), "tester").unwrap();
    assert!(re.forget_events(&[secret]).unwrap().created);
    re.verify().unwrap();
}

#[test]
fn half_applied_forget_is_not_reported_ok_until_recovered() {
    let d = tempfile::tempdir().unwrap();
    let (mut l, secret, _, _, _) = forget_fixture(d.path());
    let _ = l.forget_inner(&[secret], Some(Crash::DuringApply(1)));
    assert!(verify_dir(l.dir()).is_err());
    recover_pending(l.dir()).unwrap();
    verify_dir(l.dir()).unwrap();
}
