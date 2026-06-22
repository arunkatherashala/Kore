use serde::{Serialize, Deserialize};
use std::fs;
use crate::manifest::{Manifest, ColumnSchema};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum DdlAction {
    AddColumn { name: String, dtype: String, nullable: bool },
    DropColumn { name: String },
    RenameColumn { old: String, new: String },
}

pub fn apply_ddl(action: DdlAction) -> Result<(), String> {
    // Read current manifest.json, modify schema, and write a new manifest commit.
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

    match action {
        DdlAction::AddColumn { name, dtype, nullable } => {
            manifest.schema.push(ColumnSchema { name, dtype, nullable });
        }
        DdlAction::DropColumn { name } => {
            manifest.schema.retain(|c| c.name != name);
        }
        DdlAction::RenameColumn { old, new } => {
            for c in manifest.schema.iter_mut() {
                if c.name == old { c.name = new.clone(); }
            }
        }
    }

    // bump commit id and created_at
    manifest.parent_commit_id = Some(manifest.commit_id.clone());
    manifest.commit_id = uuid::Uuid::new_v4().to_string();
    manifest.created_at = chrono::Utc::now().to_rfc3339();

    // write atomically
    crate::manifest::write_manifest_atomic(std::path::Path::new("."), &manifest).map_err(|e| e.to_string())?;
    Ok(())
}
