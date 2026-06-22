use std::fs;
use tempfile::tempdir;
use kore_fileformat::manifest::{Manifest, Tombstone};

#[test]
fn test_compaction_applies_row_range_tombstones() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    // create an example data file with 5 lines
    let file_path = base.join("data1.kore");
    fs::write(&file_path, "r0\nr1\nr2\nr3\nr4\n").unwrap();

    // manifest referencing the file, with a tombstone deleting rows 1..2
    let mut manifest = Manifest {
        version: 1,
        commit_id: "c1".to_string(),
        parent_commit_id: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        schema: vec![],
        files: vec![file_path.to_string_lossy().to_string()],
        tombstones: vec![Tombstone { file_path: file_path.to_string_lossy().to_string(), row_id_range: Some((1,2)), predicate: None, commit_id: "c1".to_string() }],
    };

    // run compaction
    let out = base.join("out");
    fs::create_dir_all(&out).unwrap();
    let new_manifest = kore_fileformat::compaction::compact_manifest(&manifest, &out).unwrap();

    // compacted file should exist and contain rows r0, r3, r4 in that order
    let compacted = out.join(&new_manifest.files[0]);
    let s = fs::read_to_string(&compacted).unwrap();
    assert!(s.contains("r0"));
    assert!(!s.contains("r1"));
    assert!(!s.contains("r2"));
    assert!(s.contains("r3"));
    assert!(s.contains("r4"));

    // tombstones with ranges should be cleared from new manifest
    assert!(new_manifest.tombstones.iter().all(|t| t.row_id_range.is_none()));
}
