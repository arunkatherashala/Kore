use crate::manifest::{Manifest, Tombstone};
use std::fs;
use std::io::Write;

/// Prototype DML: insert rows by creating a new data file and publishing a manifest commit.
pub fn insert_rows(data: &str) -> Result<(), String> {
    // write a new data file
    let fname = format!("data-insert-{}.kore", uuid::Uuid::new_v4().to_string());
    let mut f = fs::File::create(&fname).map_err(|e| e.to_string())?;
    f.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;

    // load existing manifest or create new
    let path = std::path::Path::new("manifest.json");
    let mut manifest: Manifest = if path.exists() {
        let s = fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&s).map_err(|e| e.to_string())?
    } else {
        Manifest {
            version: 1,
            commit_id: uuid::Uuid::new_v4().to_string(),
            parent_commit_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            schema: vec![],
            files: vec![],
            tombstones: vec![],
        }
    };

    manifest.files.push(fname);
    manifest.parent_commit_id = Some(manifest.commit_id.clone());
    manifest.commit_id = uuid::Uuid::new_v4().to_string();
    manifest.created_at = chrono::Utc::now().to_rfc3339();

    crate::manifest::write_manifest_atomic(std::path::Path::new("."), &manifest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Stage a tombstone for a given file and optional row-id range, then publish manifest.
pub fn delete_range(file_path: &str, start: Option<u64>, end: Option<u64>) -> Result<(), String> {
    let path = std::path::Path::new("manifest.json");
    let mut manifest: Manifest = if path.exists() {
        let s = fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&s).map_err(|e| e.to_string())?
    } else {
        Manifest {
            version: 1,
            commit_id: uuid::Uuid::new_v4().to_string(),
            parent_commit_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            schema: vec![],
            files: vec![],
            tombstones: vec![],
        }
    };

    let ts = Tombstone {
        file_path: file_path.to_string(),
        row_id_range: match (start, end) { (Some(s), Some(e)) => Some((s,e)), _ => None },
        predicate: None,
        commit_id: manifest.commit_id.clone(),
    };

    manifest.tombstones.push(ts);
    manifest.parent_commit_id = Some(manifest.commit_id.clone());
    manifest.commit_id = uuid::Uuid::new_v4().to_string();
    manifest.created_at = chrono::Utc::now().to_rfc3339();

    crate::manifest::write_manifest_atomic(std::path::Path::new("."), &manifest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Prototype DML: insert a data file into table by copying source file into dest_dir
/// and publishing a new manifest that references the new file.
pub fn insert_data_file(src: &std::path::Path, dest_dir: &std::path::Path) -> Result<(), String> {
    if !src.exists() { return Err(format!("source {} does not exist", src.display())); }
    fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let basename = src.file_name().and_then(|s| s.to_str()).ok_or("invalid src name")?;
    let new_name = format!("data-{}-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"), basename);
    let dest_path = dest_dir.join(&new_name);
    fs::copy(src, &dest_path).map_err(|e| e.to_string())?;

    // read manifest
    let mf_path = std::path::Path::new("manifest.json");
    let mut manifest = if mf_path.exists() {
        let s = fs::read_to_string(mf_path).map_err(|e| e.to_string())?;
        serde_json::from_str::<Manifest>(&s).map_err(|e| e.to_string())?
    } else {
        crate::manifest::new_manifest_example()
    };

    manifest.files.push(dest_path.to_string_lossy().to_string());
    manifest.parent_commit_id = Some(manifest.commit_id.clone());
    manifest.commit_id = uuid::Uuid::new_v4().to_string();
    manifest.created_at = chrono::Utc::now().to_rfc3339();

    crate::manifest::write_manifest_atomic(dest_dir, &manifest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Update rows in a file by row-range: materialize a new file with the
/// replacement content for the given `start..=end` (inclusive) range, stage a
/// tombstone for the old range, and publish a manifest that appends the new file.
pub fn update_range(file_path: &str, start: u64, end: u64, replacement: &str) -> Result<(), String> {
    let original = std::path::Path::new(file_path);
    if !original.exists() { return Err(format!("original {} not found", file_path)); }

    // read original lines
    let s = fs::read_to_string(original).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = s.lines().collect();

    // build new content: keep prefix, then replacement (as lines), then suffix
    let start_idx = start as usize;
    let end_idx = end as usize;
    let mut new_lines: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if i < start_idx || i > end_idx {
            new_lines.push(l.to_string());
        }
    }
    // insert replacement at position `start_idx`
    let _prefix = new_lines.split_off(0);
    let mut out: Vec<String> = Vec::new();
    // rebuild from original but splice replacement
    for i in 0..start_idx { if i < lines.len() { out.push(lines[i].to_string()); } }
    for rep in replacement.lines() { out.push(rep.to_string()); }
    for i in (end_idx+1)..lines.len() { out.push(lines[i].to_string()); }

    let new_name = format!("data-update-{}.kore", uuid::Uuid::new_v4().to_string());
    let mut fh = fs::File::create(&new_name).map_err(|e| e.to_string())?;
    for l in &out { writeln!(fh, "{}", l).map_err(|e| e.to_string())?; }
    fh.sync_all().map_err(|e| e.to_string())?;

    // stage tombstone for old range and append new file to manifest
    let mf_path = std::path::Path::new("manifest.json");
    let mut manifest = if mf_path.exists() {
        let s = fs::read_to_string(mf_path).map_err(|e| e.to_string())?;
        serde_json::from_str::<Manifest>(&s).map_err(|e| e.to_string())?
    } else {
        crate::manifest::new_manifest_example()
    };

    let ts = Tombstone { file_path: file_path.to_string(), row_id_range: Some((start, end)), predicate: None, commit_id: manifest.commit_id.clone() };
    manifest.tombstones.push(ts);
    manifest.files.push(new_name.clone());
    manifest.parent_commit_id = Some(manifest.commit_id.clone());
    manifest.commit_id = uuid::Uuid::new_v4().to_string();
    manifest.created_at = chrono::Utc::now().to_rfc3339();

    crate::manifest::write_manifest_atomic(std::path::Path::new("."), &manifest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Upsert: append rows if no matching key exists. Prototype simply appends data
/// as a new data file and updates manifest (no merge by key implemented yet).
pub fn upsert_rows(data: &str) -> Result<(), String> {
    // For prototype, just append like insert_rows
    insert_rows(data)
}
