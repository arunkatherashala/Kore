use serde::{Serialize, Deserialize};
use std::fs::{File, rename};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ColumnSchema {
    pub name: String,
    pub dtype: String,
    pub nullable: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tombstone {
    pub file_path: String,
    pub row_id_range: Option<(u64,u64)>,
    pub predicate: Option<String>,
    pub commit_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Manifest {
    pub version: u32,
    pub commit_id: String,
    pub parent_commit_id: Option<String>,
    pub created_at: String,
    pub schema: Vec<ColumnSchema>,
    pub files: Vec<String>,
    pub tombstones: Vec<Tombstone>,
}

pub fn write_manifest_atomic<P: AsRef<Path>>(dir: P, manifest: &Manifest) -> std::io::Result<()> {
    let commit = &manifest.commit_id;
    let tmp_name = format!("manifest.tmp.{}.json", commit);
    let final_name = "manifest.json";

    let tmp_path = dir.as_ref().join(&tmp_name);
    let final_path = dir.as_ref().join(final_name);

    let mut f = File::create(&tmp_path)?;
    let bytes = serde_json::to_vec_pretty(manifest).unwrap();
    f.write_all(&bytes)?;
    f.sync_all()?;

    rename(&tmp_path, &final_path)?;
    Ok(())
}

pub fn new_manifest_example() -> Manifest {
    let commit = Uuid::new_v4().to_string();
    Manifest {
        version: 1,
        commit_id: commit.clone(),
        parent_commit_id: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        schema: vec![ColumnSchema { name: "id".into(), dtype: "int64".into(), nullable: false }],
        files: vec![],
        tombstones: vec![Tombstone { file_path: "data/part-000.kore".into(), row_id_range: Some((100,200)), predicate: None, commit_id: commit }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_write_manifest_atomic() {
        let dir = tempdir().unwrap();
        let m = new_manifest_example();
        write_manifest_atomic(dir.path(), &m).unwrap();
        let final_path = dir.path().join("manifest.json");
        assert!(final_path.exists());
    }
}
