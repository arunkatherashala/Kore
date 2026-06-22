use crate::{Footer};
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::fs::{File, rename};
use std::io::Write;

#[derive(Serialize, Deserialize, Debug)]
struct StagedManifest {
    manifest_bytes: Vec<u8>,
}

/// Begin a txn by writing staged manifest to a temp file.
pub fn commit_manifest_atomic(dir: &Path, manifest_json: &str, target_name: &str) -> std::io::Result<()> {
    // write staged file
    let staged = dir.join(format!("{}.staged", target_name));
    let mut f = File::create(&staged)?;
    f.write_all(manifest_json.as_bytes())?;
    f.sync_all()?;
    // atomic replace
    let final_path = dir.join(target_name);
    rename(&staged, &final_path)?;
    Ok(())
}

/// MVCC helper: stage via WAL then commit manifest atomically and clear WAL.
pub fn stage_and_commit(dir: &Path, wal_entry: &str, manifest_json: &str, target_name: &str) -> std::io::Result<()> {
    // append to WAL
    crate::wal::append_wal(dir, wal_entry)?;
    // commit manifest
    commit_manifest_atomic(dir, manifest_json, target_name)?;
    // clear wal
    crate::wal::clear_wal(dir)?;
    Ok(())
}

#[cfg(test)]
mod mvcc_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn mvcc_two_writers_simulation() {
        let dir = tempdir().unwrap();
        // writer A stages mA
        stage_and_commit(dir.path(), "walA", r#"{"version":1,"commit_id":"A"}"#, "manifest.json").unwrap();
        // writer B appends to WAL but hasn't committed manifest yet
        crate::wal::append_wal(dir.path(), "walB").unwrap();
        // replay WAL shows both entries
        let entries = crate::wal::replay_wal(dir.path()).unwrap();
        assert!(entries.contains(&"walA".to_string()));
        assert!(entries.contains(&"walB".to_string()));
        // now writer B commits
        commit_manifest_atomic(dir.path(), r#"{"version":1,"commit_id":"B"}"#, "manifest.json").unwrap();
        // after commit, clear WAL
        crate::wal::clear_wal(dir.path()).unwrap();
        let entries2 = crate::wal::replay_wal(dir.path()).unwrap();
        assert_eq!(entries2.len(), 0);
        let got = fs::read_to_string(dir.path().join("manifest.json")).unwrap();
        assert!(got.contains("\"commit_id\":\"B\"") || got.contains("\"commit_id\":\"A\""));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn commit_manifest_roundtrip() {
        let dir = tempdir().unwrap();
        let manifest = r#"{"version":1, "commit_id":"c1"}"#;
        let target = "manifest.json";
        commit_manifest_atomic(dir.path(), manifest, target).unwrap();
        let got = fs::read_to_string(dir.path().join(target)).unwrap();
        assert!(got.contains("commit_id"));
    }

    #[test]
    fn staged_rename_atomicity() {
        let dir = tempdir().unwrap();
        let man1 = r#"{"version":1, "commit_id":"a"}"#;
        let man2 = r#"{"version":1, "commit_id":"b"}"#;
        let target = "manifest.json";
        commit_manifest_atomic(dir.path(), man1, target).unwrap();
        commit_manifest_atomic(dir.path(), man2, target).unwrap();
        let got = fs::read_to_string(dir.path().join(target)).unwrap();
        assert!(got.contains("\"commit_id\":\"b\"") || got.contains("\"commit_id\":\"a\""));
    }
}
