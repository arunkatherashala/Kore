//! KORE Layer 43 — ACID Delta Table
//!
//! A Delta Lake-inspired transactional table format:
//!
//! - **Append-only transaction log** — every write creates a new versioned
//!   JSON log entry (`_delta_log/v{N}.json`).
//! - **Snapshot isolation** — reads always see a consistent version.
//! - **Time travel** — `read_version(v)` returns data as of commit `v`.
//! - **ACID operations** — Insert, Delete (mark removed), UpdateSchema.
//! - **Vacuum** — remove data files that are no longer referenced.
//!
//! Data files are stored as JSON-serialised DataBlocks alongside the log.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use kore_core::{Column, ColumnData, DataBlock, KoreError};

// ─── Transaction log types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStats {
    pub rows:       usize,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum TxnAction {
    /// A new data file was added in this commit.
    AddFile   { path: String, stats: FileStats },
    /// A data file was logically removed (delete/overwrite).
    RemoveFile { path: String },
    /// Metadata update (schema change, rename, …).
    Metadata  { schema: Vec<SchemaField>, description: String },
    /// Free-form commit info (timestamp, user, SQL).
    CommitInfo { timestamp: u64, operation: String, user: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaField {
    pub name:     String,
    pub dtype:    String,
    pub nullable: bool,
}

/// One version in the transaction log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub version: u64,
    pub actions: Vec<TxnAction>,
}

// ─── Delta Table ──────────────────────────────────────────────────────────────

pub struct DeltaTable {
    root: PathBuf,
    log:  Vec<LogEntry>,  // in-memory log (also persisted to root/_delta_log/)
}

impl DeltaTable {
    /// Create a new, empty delta table at `path`.
    pub fn create(path: impl AsRef<Path>, schema: Vec<SchemaField>) -> Result<Self, KoreError> {
        let root = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        std::fs::create_dir_all(root.join("_delta_log"))
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        std::fs::create_dir_all(root.join("data"))
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;

        let mut table = DeltaTable { root, log: vec![] };
        // Version 0: metadata commit
        table.commit(vec![
            TxnAction::CommitInfo {
                timestamp: now_ms(),
                operation: "CREATE TABLE".into(),
                user: "kore".into(),
            },
            TxnAction::Metadata {
                schema,
                description: "Created by KORE".into(),
            },
        ])?;
        Ok(table)
    }

    /// Open an existing delta table from `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, KoreError> {
        let root = path.as_ref().to_path_buf();
        let log_dir = root.join("_delta_log");
        if !log_dir.exists() {
            return Err(KoreError::InvalidArgument(format!("not a delta table: {}", root.display())));
        }
        let mut log = vec![];
        let mut v = 0u64;
        loop {
            let p = log_dir.join(format!("v{:020}.json", v));
            if !p.exists() { break; }
            let raw = std::fs::read_to_string(&p)
                .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
            let entry: LogEntry = serde_json::from_str(&raw)
                .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
            log.push(entry);
            v += 1;
        }
        Ok(DeltaTable { root, log })
    }

    /// Current version number (latest committed version).
    pub fn version(&self) -> u64 {
        self.log.len().saturating_sub(1) as u64
    }

    /// Append `data` to the table as a new commit.
    pub fn insert(&mut self, data: DataBlock) -> Result<u64, KoreError> {
        let file_name = format!("data/part-{:020}.kore", self.version() + 1);
        let path = self.root.join(&file_name);
        let bytes = kore_store::KoreWriter::to_bytes(&data);
        let size = bytes.len();
        std::fs::write(&path, &bytes)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;

        self.commit(vec![
            TxnAction::CommitInfo {
                timestamp: now_ms(),
                operation: "INSERT".into(),
                user: "kore".into(),
            },
            TxnAction::AddFile {
                path: file_name,
                stats: FileStats { rows: data.num_rows, size_bytes: size },
            },
        ])
    }

    /// Logically delete rows matching `predicate` (mark files removed, write filtered file).
    pub fn delete<F>(&mut self, predicate: F) -> Result<(u64, usize), KoreError>
    where F: Fn(&DataBlock, usize) -> bool
    {
        let current = self.read()?;
        let keep: Vec<usize> = (0..current.num_rows)
            .filter(|&r| !predicate(&current, r))
            .collect();
        let removed = current.num_rows - keep.len();
        let filtered = current.select_rows(&keep);

        // Collect files to remove
        let active = self.active_files();
        let mut actions: Vec<TxnAction> = active.into_iter()
            .map(|p| TxnAction::RemoveFile { path: p })
            .collect();

        // Write the filtered result as a new file
        let file_name = format!("data/part-{:020}.kore", self.version() + 1);
        let path = self.root.join(&file_name);
        let bytes = kore_store::KoreWriter::to_bytes(&filtered);
        let size = bytes.len();
        std::fs::write(&path, &bytes)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;

        actions.push(TxnAction::CommitInfo {
            timestamp: now_ms(),
            operation: format!("DELETE ({} rows)", removed),
            user: "kore".into(),
        });
        actions.push(TxnAction::AddFile {
            path: file_name,
            stats: FileStats { rows: filtered.num_rows, size_bytes: size },
        });

        let v = self.commit(actions)?;
        Ok((v, removed))
    }

    /// Read the latest snapshot of the table.
    pub fn read(&self) -> Result<DataBlock, KoreError> {
        self.read_at_version(self.version())
    }

    /// **Time travel** — read the table as of `version`.
    pub fn read_at_version(&self, version: u64) -> Result<DataBlock, KoreError> {
        let files = self.active_files_at_version(version);
        if files.is_empty() {
            // Return empty block with current schema
            return Ok(self.empty_block());
        }
        let mut parts: Vec<DataBlock> = Vec::new();
        for f in files {
            let p = self.root.join(&f);
            let bytes = std::fs::read(&p)
                .map_err(|e| KoreError::InvalidArgument(format!("{}: {}", f, e)))?;
            let block = kore_store::KoreReader::from_bytes(&bytes)?;
            parts.push(block);
        }
        DataBlock::concat(parts)
    }

    /// Commit history — most recent first.
    pub fn history(&self) -> Vec<(u64, String, u64)> {
        self.log.iter().rev().filter_map(|entry| {
            entry.actions.iter().find_map(|a| {
                if let TxnAction::CommitInfo { timestamp, operation, .. } = a {
                    Some((entry.version, operation.clone(), *timestamp))
                } else { None }
            })
        }).collect()
    }

    /// Remove log entries and data files older than `retain_versions`.
    /// Returns the number of data files deleted.
    pub fn vacuum(&mut self, retain_versions: usize) -> Result<usize, KoreError> {
        let current = self.version();
        if current < retain_versions as u64 { return Ok(0); }
        let cutoff = current - retain_versions as u64;

        // Files still needed by recent versions
        let mut keep: std::collections::HashSet<String> = std::collections::HashSet::new();
        for v in (cutoff + 1)..=current {
            keep.extend(self.active_files_at_version(v));
        }

        // Delete old data files
        let mut deleted = 0;
        for entry in &self.log[..=cutoff as usize] {
            for action in &entry.actions {
                if let TxnAction::AddFile { path, .. } = action {
                    if !keep.contains(path) {
                        let p = self.root.join(path);
                        if p.exists() { std::fs::remove_file(&p).ok(); deleted += 1; }
                    }
                }
            }
        }

        Ok(deleted)
    }

    /// Current schema (from the latest Metadata action).
    pub fn schema(&self) -> Vec<SchemaField> {
        for entry in self.log.iter().rev() {
            for action in &entry.actions {
                if let TxnAction::Metadata { schema, .. } = action {
                    return schema.clone();
                }
            }
        }
        vec![]
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    fn commit(&mut self, actions: Vec<TxnAction>) -> Result<u64, KoreError> {
        let version = self.log.len() as u64;
        let entry   = LogEntry { version, actions };
        let json    = serde_json::to_string_pretty(&entry)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        let log_path = self.root.join("_delta_log").join(format!("v{:020}.json", version));
        std::fs::write(&log_path, json)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        self.log.push(entry);
        Ok(version)
    }

    fn active_files(&self) -> Vec<String> {
        self.active_files_at_version(self.version())
    }

    fn active_files_at_version(&self, version: u64) -> Vec<String> {
        let mut added:   Vec<String> = Vec::new();
        let mut removed: std::collections::HashSet<String> = std::collections::HashSet::new();
        for entry in &self.log[..=(version as usize).min(self.log.len().saturating_sub(1))] {
            for action in &entry.actions {
                match action {
                    TxnAction::AddFile { path, .. }    => added.push(path.clone()),
                    TxnAction::RemoveFile { path }      => { removed.insert(path.clone()); }
                    _ => {}
                }
            }
        }
        added.into_iter().filter(|p| !removed.contains(p)).collect()
    }

    fn empty_block(&self) -> DataBlock {
        let columns = self.schema().into_iter().map(|f| {
            let data = match f.dtype.to_uppercase().as_str() {
                "INT64" | "INT" | "INTEGER" | "BIGINT" => ColumnData::Int64(vec![]),
                "FLOAT64" | "DOUBLE" | "FLOAT" | "REAL" => ColumnData::Float64(vec![]),
                "BOOL" | "BOOLEAN" => ColumnData::Bool(vec![]),
                _ => ColumnData::Str(vec![]),
            };
            Column { name: f.name, data }
        }).collect();
        DataBlock { columns, num_rows: 0 }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn schema() -> Vec<SchemaField> {
        vec![
            SchemaField { name: "id".into(),    dtype: "INT64".into(),   nullable: false },
            SchemaField { name: "value".into(), dtype: "FLOAT64".into(), nullable: true  },
        ]
    }

    fn block(start: i64, n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "id".into(),
                    data: ColumnData::Int64((start..start+n as i64).map(Some).collect()) },
                Column { name: "value".into(),
                    data: ColumnData::Float64((start..start+n as i64)
                        .map(|i| Some(i as f64 * 1.5)).collect()) },
            ],
        }
    }

    fn tmp_dir(suffix: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("kore_delta_test_{suffix}"));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn test_create_insert_read() {
        let dir = tmp_dir("ins");
        let mut table = DeltaTable::create(&dir, schema()).unwrap();
        assert_eq!(table.version(), 0);

        table.insert(block(0, 5)).unwrap();
        table.insert(block(5, 3)).unwrap();

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 8);
        assert_eq!(table.version(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_time_travel() {
        let dir = tmp_dir("tt");
        let mut table = DeltaTable::create(&dir, schema()).unwrap();

        table.insert(block(0, 3)).unwrap();  // version 1 → 3 rows
        table.insert(block(3, 2)).unwrap();  // version 2 → 5 rows

        let v1 = table.read_at_version(1).unwrap();
        assert_eq!(v1.num_rows, 3);

        let v2 = table.read_at_version(2).unwrap();
        assert_eq!(v2.num_rows, 5);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete() {
        let dir = tmp_dir("del");
        let mut table = DeltaTable::create(&dir, schema()).unwrap();
        table.insert(block(0, 6)).unwrap();

        // Delete rows where id >= 4
        let (_, removed) = table.delete(|b, r| {
            if let ColumnData::Int64(v) = &b.columns[0].data {
                v[r].unwrap_or(0) >= 4
            } else { false }
        }).unwrap();

        assert_eq!(removed, 2);  // id=4, id=5
        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 4);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_history() {
        let dir = tmp_dir("hist");
        let mut table = DeltaTable::create(&dir, schema()).unwrap();
        table.insert(block(0, 1)).unwrap();
        table.insert(block(1, 1)).unwrap();

        let h = table.history();
        assert!(h.len() >= 2);
        assert!(h[0].1.contains("INSERT"));  // most recent first
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_open_existing() {
        let dir = tmp_dir("open");
        {
            let mut t = DeltaTable::create(&dir, schema()).unwrap();
            t.insert(block(0, 4)).unwrap();
        }
        // Re-open
        let t2 = DeltaTable::open(&dir).unwrap();
        let data = t2.read().unwrap();
        assert_eq!(data.num_rows, 4);
        std::fs::remove_dir_all(&dir).ok();
    }
}
