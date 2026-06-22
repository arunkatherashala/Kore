use std::fs::{File, rename};
use std::io::{Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> std::io::Result<()> {
    let commit_id = make_commit_id();
    let manifest = serde_json::json!({
        "version": 1,
        "commit_id": commit_id,
        "parent_commit_id": null,
        "created_at": iso_now(),
        "schema": { "columns": [ { "name": "id", "type": "int64", "nullable": false } ] },
        "files": [],
        "tombstones": [
            { "file_path": "data/part-0000.kore", "row_id_range": [100,200], "predicate": null }
        ]
    });

    let out_dir = Path::new(".");
    let tmp_name = format!("manifest.tmp.{}.json", manifest["commit_id"].as_str().unwrap());
    let final_name = "manifest.json";

    let tmp_path = out_dir.join(&tmp_name);
    let mut f = File::create(&tmp_path)?;
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    f.write_all(&bytes)?;
    f.sync_all()?;

    // atomic rename in same filesystem
    let final_path = out_dir.join(final_name);
    rename(&tmp_path, &final_path)?;

    println!("Wrote manifest to {}", final_path.display());
    Ok(())
}

fn make_commit_id() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    format!("c{}", now)
}

fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339()
}
