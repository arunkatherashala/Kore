//! KORE Layer 43 — ACID Delta Table
//!
//! A Delta Lake-inspired transactional table format:
//!
//! - **JSON transaction log** in `_delta_log/` with numbered commit files.
//! - **Parquet data files** via `kore-parquet`.
//! - **Snapshot isolation** — reads always see a consistent version.
//! - **Time travel** — `read_version(v)` / `read_at_timestamp(ts)`.
//! - **ACID guarantees** — atomic commits via create-exclusive, schema validation,
//!   optimistic concurrency control, durable Parquet + JSON writes.
//! - **Schema evolution** — `alter_add_column` with NULL backfill.
//! - **Vacuum** — remove unreferenced data files.
//! - **Optimize** — compact small files into fewer larger ones.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use kore_parquet::{ParquetReader, ParquetWriter};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum DeltaError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Core(KoreError),
    Parquet(String),
    NotATable(String),
    VersionNotFound(i64),
    SchemaMismatch(String),
    ConcurrentWrite(String),
    InvalidPredicate(String),
}

impl std::fmt::Display for DeltaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "delta io: {e}"),
            Self::Json(e) => write!(f, "delta json: {e}"),
            Self::Core(e) => write!(f, "delta core: {e}"),
            Self::Parquet(e) => write!(f, "delta parquet: {e}"),
            Self::NotATable(p) => write!(f, "not a delta table: {p}"),
            Self::VersionNotFound(v) => write!(f, "version not found: {v}"),
            Self::SchemaMismatch(m) => write!(f, "schema mismatch: {m}"),
            Self::ConcurrentWrite(m) => write!(f, "concurrent write conflict: {m}"),
            Self::InvalidPredicate(m) => write!(f, "invalid predicate: {m}"),
        }
    }
}

impl std::error::Error for DeltaError {}

impl DeltaError {
    pub fn into_kore(self) -> KoreError {
        KoreError::InvalidArgument(self.to_string())
    }
}

impl From<std::io::Error> for DeltaError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}
impl From<serde_json::Error> for DeltaError {
    fn from(e: serde_json::Error) -> Self { Self::Json(e) }
}
impl From<KoreError> for DeltaError {
    fn from(e: KoreError) -> Self { Self::Core(e) }
}

// ─── Schema ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeltaSchema {
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    pub dtype: String,
    pub nullable: bool,
}

impl DeltaSchema {
    pub fn new(fields: Vec<SchemaField>) -> Self {
        Self { fields }
    }

    pub fn field(&self, name: &str) -> Option<&SchemaField> {
        self.fields.iter().find(|f| f.name == name)
    }

    fn merge_with(&self, other: &DeltaSchema) -> DeltaSchema {
        let mut merged = self.clone();
        for field in &other.fields {
            if merged.field(&field.name).is_none() {
                merged.fields.push(SchemaField {
                    name: field.name.clone(),
                    dtype: field.dtype.clone(),
                    nullable: true,
                });
            }
        }
        merged
    }
}

// ─── Transaction log types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddFile {
    pub path: String,
    pub size: u64,
    pub timestamp: u64,
    pub stats: FileStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStats {
    pub num_rows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveFile {
    pub path: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub version: i64,
    pub timestamp: u64,
    pub operation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    pub min_reader_version: i32,
    pub min_writer_version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum Action {
    #[serde(rename = "add")]
    Add(AddFile),
    #[serde(rename = "remove")]
    Remove(RemoveFile),
    #[serde(rename = "commitInfo")]
    Commit(CommitInfo),
    #[serde(rename = "protocol")]
    Protocol(Protocol),
    #[serde(rename = "metaData")]
    MetaData { schema: DeltaSchema },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub actions: Vec<Action>,
}

// ─── Optimize result ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OptimizeResult {
    pub files_compacted: usize,
    pub new_files: usize,
    pub version: i64,
}

// ─── DeltaTable ──────────────────────────────────────────────────────────────

pub struct DeltaTable {
    path: PathBuf,
    version: i64,
    files: Vec<AddFile>,
    schema: Option<DeltaSchema>,
    log: Vec<LogEntry>,
}

impl DeltaTable {
    pub fn create(path: &Path, schema: DeltaSchema) -> Result<Self, DeltaError> {
        fs::create_dir_all(path)?;
        let log_dir = path.join("_delta_log");
        fs::create_dir_all(&log_dir)?;
        let data_dir = path.join("data");
        fs::create_dir_all(&data_dir)?;

        let entry = LogEntry {
            actions: vec![
                Action::Protocol(Protocol {
                    min_reader_version: 1,
                    min_writer_version: 2,
                }),
                Action::MetaData { schema: schema.clone() },
                Action::Commit(CommitInfo {
                    version: 0,
                    timestamp: now_ms(),
                    operation: "CREATE TABLE".into(),
                }),
            ],
        };

        let commit_path = log_dir.join(version_filename(0));
        write_commit_exclusive(&commit_path, &entry)?;

        Ok(DeltaTable {
            path: path.to_path_buf(),
            version: 0,
            files: vec![],
            schema: Some(schema),
            log: vec![entry],
        })
    }

    pub fn open(path: &Path) -> Result<Self, DeltaError> {
        let log_dir = path.join("_delta_log");
        if !log_dir.exists() {
            return Err(DeltaError::NotATable(path.display().to_string()));
        }

        let mut log = Vec::new();
        let mut version: i64 = -1;
        let mut files: Vec<AddFile> = Vec::new();
        let mut removed: HashSet<String> = HashSet::new();
        let mut schema: Option<DeltaSchema> = None;

        loop {
            let next = version + 1;
            let p = log_dir.join(version_filename(next));
            if !p.exists() {
                break;
            }
            let raw = fs::read_to_string(&p)?;
            let entry: LogEntry = serde_json::from_str(&raw)?;

            for action in &entry.actions {
                match action {
                    Action::Add(add) => files.push(add.clone()),
                    Action::Remove(rem) => { removed.insert(rem.path.clone()); }
                    Action::MetaData { schema: s } => { schema = Some(s.clone()); }
                    _ => {}
                }
            }
            log.push(entry);
            version = next;
        }

        files.retain(|f| !removed.contains(&f.path));

        Ok(DeltaTable {
            path: path.to_path_buf(),
            version,
            files,
            schema,
            log,
        })
    }

    pub fn version(&self) -> i64 {
        self.version
    }

    pub fn files(&self) -> &[AddFile] {
        &self.files
    }

    pub fn schema(&self) -> Option<&DeltaSchema> {
        self.schema.as_ref()
    }

    pub fn insert(&mut self, data: &DataBlock) -> Result<i64, DeltaError> {
        if data.num_rows == 0 {
            return Ok(self.version);
        }

        if let Some(schema) = &self.schema {
            let data_schema = infer_schema(data);
            let merged = schema.merge_with(&data_schema);
            if merged != *schema {
                self.evolve_schema(merged.clone())?;
            }
        }

        let new_version = self.version + 1;
        let file_name = format!("data/part-{:020}.parquet", new_version);
        let file_path = self.path.join(&file_name);

        ParquetWriter::write_file(data, &file_path)
            .map_err(|e| DeltaError::Parquet(e.to_string()))?;

        let metadata = fs::metadata(&file_path)?;
        let add = AddFile {
            path: file_name,
            size: metadata.len(),
            timestamp: now_ms(),
            stats: FileStats { num_rows: data.num_rows },
        };

        let entry = LogEntry {
            actions: vec![
                Action::Add(add.clone()),
                Action::Commit(CommitInfo {
                    version: new_version,
                    timestamp: now_ms(),
                    operation: "INSERT".into(),
                }),
            ],
        };

        self.try_commit(new_version, &entry)?;
        self.files.push(add);
        self.version = new_version;
        self.log.push(entry);
        Ok(new_version)
    }

    pub fn delete(&mut self, predicate: &str) -> Result<i64, DeltaError> {
        let current = self.read()?;
        if current.num_rows == 0 {
            return Ok(self.version);
        }

        let pred = parse_predicate(predicate)?;
        let keep: Vec<usize> = (0..current.num_rows)
            .filter(|&r| !pred.matches(&current, r))
            .collect();

        self.delete_rows(&current, &keep, format!("DELETE ({predicate})"))
    }

    pub fn delete_all(&mut self) -> Result<i64, DeltaError> {
        let current = self.read()?;
        if current.num_rows == 0 {
            return Ok(self.version);
        }

        self.delete_rows(&current, &[], "DELETE (ALL)".to_string())
    }

    fn delete_rows(
        &mut self,
        current: &DataBlock,
        keep: &[usize],
        operation: String,
    ) -> Result<i64, DeltaError> {

        if keep.len() == current.num_rows {
            return Ok(self.version);
        }

        let filtered = current.select_rows(&keep);
        let new_version = self.version + 1;

        let mut actions: Vec<Action> = self.files.iter().map(|f| {
            Action::Remove(RemoveFile {
                path: f.path.clone(),
                timestamp: now_ms(),
            })
        }).collect();

        if filtered.num_rows > 0 {
            let file_name = format!("data/part-{:020}.parquet", new_version);
            let file_path = self.path.join(&file_name);
            ParquetWriter::write_file(&filtered, &file_path)
                .map_err(|e| DeltaError::Parquet(e.to_string()))?;
            let metadata = fs::metadata(&file_path)?;
            actions.push(Action::Add(AddFile {
                path: file_name,
                size: metadata.len(),
                timestamp: now_ms(),
                stats: FileStats { num_rows: filtered.num_rows },
            }));
        }

        actions.push(Action::Commit(CommitInfo {
            version: new_version,
            timestamp: now_ms(),
            operation,
        }));

        let entry = LogEntry { actions: actions.clone() };
        self.try_commit(new_version, &entry)?;

        self.files.clear();
        for action in &actions {
            if let Action::Add(add) = action {
                self.files.push(add.clone());
            }
        }
        self.version = new_version;
        self.log.push(entry);
        Ok(new_version)
    }

    pub fn read(&self) -> Result<DataBlock, DeltaError> {
        self.read_files(&self.files)
    }

    pub fn read_version(&self, version: i64) -> Result<DataBlock, DeltaError> {
        if version < 0 || version > self.version {
            return Err(DeltaError::VersionNotFound(version));
        }

        let files = self.active_files_at_version(version);
        self.read_files(&files)
    }

    pub fn read_at_timestamp(&self, ts: u64) -> Result<DataBlock, DeltaError> {
        let mut target_version: i64 = -1;
        for entry in &self.log {
            for action in &entry.actions {
                if let Action::Commit(info) = action {
                    if info.timestamp <= ts {
                        target_version = info.version;
                    }
                }
            }
        }
        if target_version < 0 {
            return Ok(DataBlock::empty());
        }
        self.read_version(target_version)
    }

    pub fn history(&self) -> Result<Vec<CommitInfo>, DeltaError> {
        let mut commits: Vec<CommitInfo> = Vec::new();
        for entry in self.log.iter().rev() {
            for action in &entry.actions {
                if let Action::Commit(info) = action {
                    commits.push(info.clone());
                }
            }
        }
        Ok(commits)
    }

    pub fn vacuum(&mut self, retention_hours: u64) -> Result<usize, DeltaError> {
        let cutoff_ms = now_ms().saturating_sub(retention_hours * 3600 * 1000);

        let active: HashSet<String> = self.files.iter().map(|f| f.path.clone()).collect();

        let mut deleted = 0usize;
        for entry in &self.log {
            for action in &entry.actions {
                match action {
                    Action::Add(add) if !active.contains(&add.path) && add.timestamp < cutoff_ms => {
                        let p = self.path.join(&add.path);
                        if p.exists() {
                            fs::remove_file(&p).ok();
                            deleted += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(deleted)
    }

    pub fn optimize(&mut self) -> Result<OptimizeResult, DeltaError> {
        if self.files.len() <= 1 {
            return Ok(OptimizeResult {
                files_compacted: 0,
                new_files: 0,
                version: self.version,
            });
        }

        let all_data = self.read()?;
        if all_data.num_rows == 0 {
            return Ok(OptimizeResult {
                files_compacted: 0,
                new_files: 0,
                version: self.version,
            });
        }

        let files_compacted = self.files.len();
        let new_version = self.version + 1;

        let mut actions: Vec<Action> = self.files.iter().map(|f| {
            Action::Remove(RemoveFile {
                path: f.path.clone(),
                timestamp: now_ms(),
            })
        }).collect();

        let file_name = format!("data/part-{:020}.parquet", new_version);
        let file_path = self.path.join(&file_name);
        ParquetWriter::write_file(&all_data, &file_path)
            .map_err(|e| DeltaError::Parquet(e.to_string()))?;
        let metadata = fs::metadata(&file_path)?;

        let add = AddFile {
            path: file_name,
            size: metadata.len(),
            timestamp: now_ms(),
            stats: FileStats { num_rows: all_data.num_rows },
        };
        actions.push(Action::Add(add.clone()));

        actions.push(Action::Commit(CommitInfo {
            version: new_version,
            timestamp: now_ms(),
            operation: "OPTIMIZE".into(),
        }));

        let entry = LogEntry { actions };
        self.try_commit(new_version, &entry)?;

        self.files = vec![add];
        self.version = new_version;
        self.log.push(entry);

        Ok(OptimizeResult {
            files_compacted,
            new_files: 1,
            version: new_version,
        })
    }

    pub fn update(&mut self, predicate: &str, column: &str, new_value: &str) -> Result<i64, DeltaError> {
        let current = self.read()?;
        if current.num_rows == 0 {
            return Ok(self.version);
        }

        let pred = parse_predicate(predicate)?;
        let col_idx = current.columns.iter().position(|c| c.name == column)
            .ok_or_else(|| DeltaError::SchemaMismatch(format!("column '{}' not found", column)))?;

        let updated = apply_update(&current, &pred, col_idx, new_value);
        let new_version = self.version + 1;

        let mut actions: Vec<Action> = self.files.iter().map(|f| {
            Action::Remove(RemoveFile {
                path: f.path.clone(),
                timestamp: now_ms(),
            })
        }).collect();

        let file_name = format!("data/part-{:020}.parquet", new_version);
        let file_path = self.path.join(&file_name);
        ParquetWriter::write_file(&updated, &file_path)
            .map_err(|e| DeltaError::Parquet(e.to_string()))?;
        let metadata = fs::metadata(&file_path)?;

        let add = AddFile {
            path: file_name,
            size: metadata.len(),
            timestamp: now_ms(),
            stats: FileStats { num_rows: updated.num_rows },
        };
        actions.push(Action::Add(add.clone()));
        actions.push(Action::Commit(CommitInfo {
            version: new_version,
            timestamp: now_ms(),
            operation: format!("UPDATE SET {} ({})", column, predicate),
        }));

        let entry = LogEntry { actions };
        self.try_commit(new_version, &entry)?;

        self.files = vec![add];
        self.version = new_version;
        self.log.push(entry);
        Ok(new_version)
    }

    pub fn merge_into(&mut self, source: &DataBlock, key_column: &str) -> Result<i64, DeltaError> {
        let current = self.read()?;

        let key_idx_target = current.columns.iter().position(|c| c.name == key_column);
        let key_idx_source = source.columns.iter().position(|c| c.name == key_column)
            .ok_or_else(|| DeltaError::SchemaMismatch(
                format!("key column '{}' not found in source", key_column),
            ))?;

        let mut source_keys: HashSet<i64> = HashSet::new();
        if let Some(col) = source.columns.get(key_idx_source) {
            if let ColumnData::Int64(vals) = &col.data {
                for v in vals {
                    if let Some(k) = v {
                        source_keys.insert(*k);
                    }
                }
            }
        }

        let mut result_rows: Vec<Vec<kore_core::Value>> = Vec::new();

        if let Some(ki) = key_idx_target {
            for r in 0..current.num_rows {
                let key_val = current.columns[ki].data.get_value(r);
                let skip = if let kore_core::Value::Int(k) = &key_val {
                    source_keys.contains(k)
                } else {
                    false
                };
                if !skip {
                    let row: Vec<kore_core::Value> = current.columns.iter()
                        .map(|c| c.data.get_value(r))
                        .collect();
                    result_rows.push(row);
                }
            }
        }

        for r in 0..source.num_rows {
            let row: Vec<kore_core::Value> = source.columns.iter()
                .map(|c| c.data.get_value(r))
                .collect();
            result_rows.push(row);
        }

        let schema = self.schema.as_ref()
            .ok_or_else(|| DeltaError::SchemaMismatch("no schema".into()))?;
        let merged_block = rows_to_block(&result_rows, schema);

        let new_version = self.version + 1;

        let mut actions: Vec<Action> = self.files.iter().map(|f| {
            Action::Remove(RemoveFile {
                path: f.path.clone(),
                timestamp: now_ms(),
            })
        }).collect();

        let file_name = format!("data/part-{:020}.parquet", new_version);
        let file_path = self.path.join(&file_name);
        ParquetWriter::write_file(&merged_block, &file_path)
            .map_err(|e| DeltaError::Parquet(e.to_string()))?;
        let metadata = fs::metadata(&file_path)?;

        let add = AddFile {
            path: file_name,
            size: metadata.len(),
            timestamp: now_ms(),
            stats: FileStats { num_rows: merged_block.num_rows },
        };
        actions.push(Action::Add(add.clone()));
        actions.push(Action::Commit(CommitInfo {
            version: new_version,
            timestamp: now_ms(),
            operation: format!("MERGE INTO (key={})", key_column),
        }));

        let entry = LogEntry { actions };
        self.try_commit(new_version, &entry)?;

        self.files = vec![add];
        self.version = new_version;
        self.log.push(entry);
        Ok(new_version)
    }

    pub fn rename_column(&mut self, old_name: &str, new_name: &str) -> Result<i64, DeltaError> {
        let schema = self.schema.as_ref()
            .ok_or_else(|| DeltaError::SchemaMismatch("no schema".into()))?;

        if schema.field(old_name).is_none() {
            return Err(DeltaError::SchemaMismatch(
                format!("column '{}' not found", old_name),
            ));
        }
        if schema.field(new_name).is_some() {
            return Err(DeltaError::SchemaMismatch(
                format!("column '{}' already exists", new_name),
            ));
        }

        let mut new_schema = schema.clone();
        for field in &mut new_schema.fields {
            if field.name == old_name {
                field.name = new_name.into();
            }
        }

        self.evolve_schema(new_schema)
    }

    pub fn alter_add_column(&mut self, name: &str, dtype: &str) -> Result<i64, DeltaError> {
        let schema = self.schema.as_ref()
            .ok_or_else(|| DeltaError::SchemaMismatch("no schema".into()))?;

        if schema.field(name).is_some() {
            return Err(DeltaError::SchemaMismatch(
                format!("column '{}' already exists", name),
            ));
        }

        let mut new_schema = schema.clone();
        new_schema.fields.push(SchemaField {
            name: name.into(),
            dtype: dtype.into(),
            nullable: true,
        });

        self.evolve_schema(new_schema)
    }

    // ─── Private helpers ─────────────────────────────────────────────────────

    fn evolve_schema(&mut self, new_schema: DeltaSchema) -> Result<i64, DeltaError> {
        let new_version = self.version + 1;
        let entry = LogEntry {
            actions: vec![
                Action::MetaData { schema: new_schema.clone() },
                Action::Commit(CommitInfo {
                    version: new_version,
                    timestamp: now_ms(),
                    operation: "ALTER TABLE".into(),
                }),
            ],
        };

        self.try_commit(new_version, &entry)?;
        self.schema = Some(new_schema);
        self.version = new_version;
        self.log.push(entry);
        Ok(new_version)
    }

    fn try_commit(&self, version: i64, entry: &LogEntry) -> Result<(), DeltaError> {
        let log_dir = self.path.join("_delta_log");
        let commit_path = log_dir.join(version_filename(version));

        const MAX_RETRIES: u32 = 50;
        for attempt in 0..MAX_RETRIES {
            match write_commit_exclusive(&commit_path, entry) {
                Ok(()) => return Ok(()),
                Err(DeltaError::Io(ref e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempt < MAX_RETRIES - 1 {
                        thread::sleep(Duration::from_millis(10 * (attempt as u64 + 1)));
                        continue;
                    }
                    return Err(DeltaError::ConcurrentWrite(
                        format!("version {} already committed after {} retries", version, MAX_RETRIES),
                    ));
                }
                Err(e) => return Err(e),
            }
        }
        Err(DeltaError::ConcurrentWrite("exhausted retries".into()))
    }

    /// Atomically commit with retry and version bump for concurrent writers.
    #[allow(dead_code)]
    pub fn commit_with_retry(&mut self, mut entry: LogEntry) -> Result<i64, DeltaError> {
        let log_dir = self.path.join("_delta_log");
        const MAX_RETRIES: u32 = 100;

        for _ in 0..MAX_RETRIES {
            let target_version = self.current_disk_version() + 1;
            update_commit_version(&mut entry, target_version);
            let commit_path = log_dir.join(version_filename(target_version));

            match write_commit_exclusive(&commit_path, &entry) {
                Ok(()) => return Ok(target_version),
                Err(DeltaError::Io(ref e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Err(DeltaError::ConcurrentWrite("exhausted retries".into()))
    }

    #[allow(dead_code)]
    pub fn current_disk_version(&self) -> i64 {
        let log_dir = self.path.join("_delta_log");
        let mut v: i64 = -1;
        loop {
            let next = v + 1;
            if log_dir.join(version_filename(next)).exists() {
                v = next;
            } else {
                break;
            }
        }
        v
    }

    fn active_files_at_version(&self, version: i64) -> Vec<AddFile> {
        let mut added: Vec<AddFile> = Vec::new();
        let mut removed: HashSet<String> = HashSet::new();

        let count = (version + 1) as usize;
        for entry in self.log.iter().take(count) {
            for action in &entry.actions {
                match action {
                    Action::Add(add) => added.push(add.clone()),
                    Action::Remove(rem) => { removed.insert(rem.path.clone()); }
                    _ => {}
                }
            }
        }
        added.retain(|f| !removed.contains(&f.path));
        added
    }

    fn read_files(&self, files: &[AddFile]) -> Result<DataBlock, DeltaError> {
        if files.is_empty() {
            return Ok(self.empty_block());
        }

        let mut parts: Vec<DataBlock> = Vec::new();
        for f in files {
            let p = self.path.join(&f.path);
            let block = ParquetReader::new(&p).read()
                .map_err(|e| DeltaError::Parquet(e.to_string()))?;
            parts.push(block);
        }

        if parts.len() == 1 {
            return Ok(parts.into_iter().next().unwrap());
        }

        let target_cols = self.schema.as_ref().map(|s| s.fields.len()).unwrap_or(0);
        if target_cols > 0 {
            let schema = self.schema.as_ref().unwrap();
            parts = parts.into_iter().map(|b| pad_block_to_schema(&b, schema)).collect();
        }

        DataBlock::concat(parts).map_err(DeltaError::from)
    }

    fn empty_block(&self) -> DataBlock {
        match &self.schema {
            Some(schema) => {
                let columns = schema.fields.iter().map(|f| {
                    let data = empty_column_data(&f.dtype);
                    Column { name: f.name.clone(), data }
                }).collect();
                DataBlock { columns, num_rows: 0 }
            }
            None => DataBlock::empty(),
        }
    }
}

// ─── Predicate parsing ───────────────────────────────────────────────────────

struct Predicate {
    column: String,
    op: PredOp,
    value: PredValue,
}

enum PredOp { Eq, Ne, Lt, Le, Gt, Ge }

enum PredValue { Int(i64), Float(f64), Str(String) }

impl Predicate {
    fn matches(&self, block: &DataBlock, row: usize) -> bool {
        let col = match block.column(&self.column) {
            Some(c) => c,
            None => return false,
        };
        let val = col.data.get_value(row);
        match (&self.op, &self.value) {
            (op, PredValue::Int(pv)) => {
                if let kore_core::Value::Int(v) = val {
                    cmp_op(*op, v, *pv)
                } else { false }
            }
            (op, PredValue::Float(pv)) => {
                let v = match val {
                    kore_core::Value::Float(f) => f,
                    kore_core::Value::Int(i) => i as f64,
                    _ => return false,
                };
                cmp_op_f64(*op, v, *pv)
            }
            (op, PredValue::Str(pv)) => {
                if let kore_core::Value::Str(v) = val {
                    cmp_op_str(*op, &v, pv)
                } else { false }
            }
        }
    }
}

impl Copy for PredOp {}
impl Clone for PredOp {
    fn clone(&self) -> Self { *self }
}

fn cmp_op(op: PredOp, a: i64, b: i64) -> bool {
    match op {
        PredOp::Eq => a == b,
        PredOp::Ne => a != b,
        PredOp::Lt => a < b,
        PredOp::Le => a <= b,
        PredOp::Gt => a > b,
        PredOp::Ge => a >= b,
    }
}

fn cmp_op_f64(op: PredOp, a: f64, b: f64) -> bool {
    match op {
        PredOp::Eq => (a - b).abs() < f64::EPSILON,
        PredOp::Ne => (a - b).abs() >= f64::EPSILON,
        PredOp::Lt => a < b,
        PredOp::Le => a <= b,
        PredOp::Gt => a > b,
        PredOp::Ge => a >= b,
    }
}

fn cmp_op_str(op: PredOp, a: &str, b: &str) -> bool {
    match op {
        PredOp::Eq => a == b,
        PredOp::Ne => a != b,
        PredOp::Lt => a < b,
        PredOp::Le => a <= b,
        PredOp::Gt => a > b,
        PredOp::Ge => a >= b,
    }
}

fn parse_predicate(s: &str) -> Result<Predicate, DeltaError> {
    let ops = [">=", "<=", "!=", "=", ">", "<"];
    for op_str in &ops {
        if let Some(idx) = s.find(op_str) {
            let column = s[..idx].trim().to_string();
            let val_str = s[idx + op_str.len()..].trim();
            let op = match *op_str {
                ">=" => PredOp::Ge,
                "<=" => PredOp::Le,
                "!=" => PredOp::Ne,
                "=" => PredOp::Eq,
                ">" => PredOp::Gt,
                "<" => PredOp::Lt,
                _ => unreachable!(),
            };
            let value = parse_pred_value(val_str)?;
            return Ok(Predicate { column, op, value });
        }
    }
    Err(DeltaError::InvalidPredicate(s.into()))
}

fn parse_pred_value(s: &str) -> Result<PredValue, DeltaError> {
    if let Ok(i) = s.parse::<i64>() {
        return Ok(PredValue::Int(i));
    }
    if let Ok(f) = s.parse::<f64>() {
        return Ok(PredValue::Float(f));
    }
    let trimmed = s.trim_matches('\'').trim_matches('"');
    Ok(PredValue::Str(trimmed.to_string()))
}

// ─── Utility functions ───────────────────────────────────────────────────────

fn version_filename(version: i64) -> String {
    format!("{:020}.json", version)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn write_commit_exclusive(path: &Path, entry: &LogEntry) -> Result<(), DeltaError> {
    let json = serde_json::to_string_pretty(entry)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

#[allow(dead_code)]
pub fn update_commit_version(entry: &mut LogEntry, version: i64) {
    for action in &mut entry.actions {
        if let Action::Commit(info) = action {
            info.version = version;
        }
    }
}

fn infer_schema(data: &DataBlock) -> DeltaSchema {
    let fields = data.columns.iter().map(|col| {
        let dtype = match &col.data {
            ColumnData::Int64(_) => "INT64",
            ColumnData::Float64(_) => "FLOAT64",
            ColumnData::Bool(_) => "BOOL",
            ColumnData::Str(_) | ColumnData::StrDict { .. } => "STRING",
        };
        SchemaField {
            name: col.name.clone(),
            dtype: dtype.into(),
            nullable: true,
        }
    }).collect();
    DeltaSchema { fields }
}

fn empty_column_data(dtype: &str) -> ColumnData {
    match dtype.to_uppercase().as_str() {
        "INT64" | "INT" | "INTEGER" | "BIGINT" => ColumnData::Int64(vec![]),
        "FLOAT64" | "DOUBLE" | "FLOAT" | "REAL" => ColumnData::Float64(vec![]),
        "BOOL" | "BOOLEAN" => ColumnData::Bool(vec![]),
        _ => ColumnData::Str(vec![]),
    }
}

fn null_column_data(dtype: &str, num_rows: usize) -> ColumnData {
    match dtype.to_uppercase().as_str() {
        "INT64" | "INT" | "INTEGER" | "BIGINT" => ColumnData::Int64(vec![None; num_rows]),
        "FLOAT64" | "DOUBLE" | "FLOAT" | "REAL" => ColumnData::Float64(vec![None; num_rows]),
        "BOOL" | "BOOLEAN" => ColumnData::Bool(vec![None; num_rows]),
        _ => ColumnData::Str(vec![None; num_rows]),
    }
}

fn pad_block_to_schema(block: &DataBlock, schema: &DeltaSchema) -> DataBlock {
    let mut columns = Vec::with_capacity(schema.fields.len());
    for field in &schema.fields {
        if let Some(col) = block.column(&field.name) {
            columns.push(col.clone());
        } else {
            columns.push(Column {
                name: field.name.clone(),
                data: null_column_data(&field.dtype, block.num_rows),
            });
        }
    }
    DataBlock {
        columns,
        num_rows: block.num_rows,
    }
}

fn apply_update(block: &DataBlock, pred: &Predicate, col_idx: usize, new_value: &str) -> DataBlock {
    let mut columns: Vec<Column> = block.columns.iter().map(|c| {
        Column { name: c.name.clone(), data: c.data.clone() }
    }).collect();

    for r in 0..block.num_rows {
        if pred.matches(block, r) {
            match &mut columns[col_idx].data {
                ColumnData::Int64(v) => {
                    if let Ok(val) = new_value.parse::<i64>() {
                        v[r] = Some(val);
                    }
                }
                ColumnData::Float64(v) => {
                    if let Ok(val) = new_value.parse::<f64>() {
                        v[r] = Some(val);
                    }
                }
                ColumnData::Str(v) => {
                    let trimmed = new_value.trim_matches('\'').trim_matches('"');
                    v[r] = Some(trimmed.to_string());
                }
                ColumnData::Bool(v) => {
                    v[r] = Some(new_value.eq_ignore_ascii_case("true"));
                }
                ColumnData::StrDict { .. } => {}
            }
        }
    }
    DataBlock { columns, num_rows: block.num_rows }
}

fn rows_to_block(rows: &[Vec<kore_core::Value>], schema: &DeltaSchema) -> DataBlock {
    if rows.is_empty() {
        let columns = schema.fields.iter().map(|f| Column {
            name: f.name.clone(),
            data: empty_column_data(&f.dtype),
        }).collect();
        return DataBlock { columns, num_rows: 0 };
    }

    let num_cols = schema.fields.len();
    let mut columns: Vec<Column> = Vec::with_capacity(num_cols);

    for (ci, field) in schema.fields.iter().enumerate() {
        let data = match field.dtype.to_uppercase().as_str() {
            "INT64" | "INT" | "INTEGER" | "BIGINT" => {
                let vals: Vec<Option<i64>> = rows.iter().map(|row| {
                    if ci < row.len() {
                        match &row[ci] {
                            kore_core::Value::Int(v) => Some(*v),
                            _ => None,
                        }
                    } else { None }
                }).collect();
                ColumnData::Int64(vals)
            }
            "FLOAT64" | "DOUBLE" | "FLOAT" | "REAL" => {
                let vals: Vec<Option<f64>> = rows.iter().map(|row| {
                    if ci < row.len() {
                        match &row[ci] {
                            kore_core::Value::Float(v) => Some(*v),
                            kore_core::Value::Int(v) => Some(*v as f64),
                            _ => None,
                        }
                    } else { None }
                }).collect();
                ColumnData::Float64(vals)
            }
            "BOOL" | "BOOLEAN" => {
                let vals: Vec<Option<bool>> = rows.iter().map(|row| {
                    if ci < row.len() {
                        match &row[ci] {
                            kore_core::Value::Bool(v) => Some(*v),
                            _ => None,
                        }
                    } else { None }
                }).collect();
                ColumnData::Bool(vals)
            }
            _ => {
                let vals: Vec<Option<String>> = rows.iter().map(|row| {
                    if ci < row.len() {
                        match &row[ci] {
                            kore_core::Value::Str(v) => Some(v.clone()),
                            _ => None,
                        }
                    } else { None }
                }).collect();
                ColumnData::Str(vals)
            }
        };
        columns.push(Column { name: field.name.clone(), data });
    }

    DataBlock { columns, num_rows: rows.len() }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn test_schema() -> DeltaSchema {
        DeltaSchema::new(vec![
            SchemaField { name: "id".into(), dtype: "INT64".into(), nullable: false },
            SchemaField { name: "value".into(), dtype: "FLOAT64".into(), nullable: true },
        ])
    }

    fn make_block(start: i64, n: usize) -> DataBlock {
        DataBlock::new(vec![
            Column::int64("id", (start..start + n as i64).map(Some).collect()),
            Column::float64("value", (start..start + n as i64).map(|i| Some(i as f64 * 1.5)).collect()),
        ]).unwrap()
    }

    fn tmp_dir(suffix: &str) -> PathBuf {
        let p = std::env::temp_dir()
            .join(format!("kore_delta_test_{suffix}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn test_create_insert_read() {
        let dir = tmp_dir("create_ins_read");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        assert_eq!(table.version(), 0);

        table.insert(&make_block(0, 5)).unwrap();
        table.insert(&make_block(5, 3)).unwrap();

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 8);
        assert_eq!(table.version(), 2);

        if let ColumnData::Int64(v) = &data.columns[0].data {
            assert_eq!(v[0], Some(0));
            assert_eq!(v[7], Some(7));
        } else {
            panic!("expected Int64");
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_multi_version_insert() {
        let dir = tmp_dir("multi_ver");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();

        for i in 0..5 {
            table.insert(&make_block(i * 10, 10)).unwrap();
        }

        assert_eq!(table.version(), 5);
        assert_eq!(table.files().len(), 5);

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 50);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_time_travel() {
        let dir = tmp_dir("time_travel");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();

        table.insert(&make_block(0, 3)).unwrap();   // version 1, 3 rows
        table.insert(&make_block(3, 2)).unwrap();   // version 2, 5 rows total

        let v1 = table.read_version(1).unwrap();
        assert_eq!(v1.num_rows, 3);

        let v2 = table.read_version(2).unwrap();
        assert_eq!(v2.num_rows, 5);

        let v0 = table.read_version(0).unwrap();
        assert_eq!(v0.num_rows, 0);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_with_predicate() {
        let dir = tmp_dir("delete_pred");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 6)).unwrap();

        table.delete("id >= 4").unwrap();

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 4);

        if let ColumnData::Int64(v) = &data.columns[0].data {
            assert!(v.iter().all(|x| x.unwrap() < 4));
        } else {
            panic!("expected Int64");
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_schema_evolution() {
        let dir = tmp_dir("schema_evo");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 3)).unwrap();

        table.alter_add_column("tag", "STRING").unwrap();

        let schema = table.schema().unwrap();
        assert_eq!(schema.fields.len(), 3);
        assert_eq!(schema.fields[2].name, "tag");

        let data_with_tag = DataBlock::new(vec![
            Column::int64("id", vec![Some(10), Some(11)]),
            Column::float64("value", vec![Some(1.0), Some(2.0)]),
            Column::str_col("tag", vec![Some("a".into()), Some("b".into())]),
        ]).unwrap();
        table.insert(&data_with_tag).unwrap();

        let all = table.read().unwrap();
        assert_eq!(all.num_rows, 5);
        assert_eq!(all.columns.len(), 3);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_vacuum() {
        let dir = tmp_dir("vacuum");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 5)).unwrap();
        table.insert(&make_block(5, 5)).unwrap();

        table.delete("id >= 3").unwrap();

        let deleted = table.vacuum(0).unwrap();
        assert!(deleted >= 1);

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 3);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_history() {
        let dir = tmp_dir("history");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 1)).unwrap();
        table.insert(&make_block(1, 1)).unwrap();

        let h = table.history().unwrap();
        assert!(h.len() >= 3);
        assert!(h[0].operation.contains("INSERT"));
        assert!(h.last().unwrap().operation.contains("CREATE"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_concurrent_writes() {
        let dir = tmp_dir("concurrent");
        let table = DeltaTable::create(&dir, test_schema()).unwrap();
        drop(table);

        let dir1 = dir.clone();
        let dir2 = dir.clone();

        let h1 = thread::spawn(move || {
            let mut t = DeltaTable::open(&dir1).unwrap();
            for i in 0..5 {
                let block = make_block(i * 100, 10);
                let file_name = format!("data/concurrent_a_{}.parquet", i);
                let file_path = dir1.join(&file_name);
                ParquetWriter::write_file(&block, &file_path).unwrap();
                let metadata = fs::metadata(&file_path).unwrap();

                let entry = LogEntry {
                    actions: vec![
                        Action::Add(AddFile {
                            path: file_name,
                            size: metadata.len(),
                            timestamp: now_ms(),
                            stats: FileStats { num_rows: 10 },
                        }),
                        Action::Commit(CommitInfo {
                            version: 0, // will be updated
                            timestamp: now_ms(),
                            operation: "INSERT".into(),
                        }),
                    ],
                };
                t.commit_with_retry(entry).unwrap();
            }
        });

        let h2 = thread::spawn(move || {
            let mut t = DeltaTable::open(&dir2).unwrap();
            for i in 0..5 {
                let block = make_block(i * 100 + 50, 10);
                let file_name = format!("data/concurrent_b_{}.parquet", i);
                let file_path = dir2.join(&file_name);
                ParquetWriter::write_file(&block, &file_path).unwrap();
                let metadata = fs::metadata(&file_path).unwrap();

                let entry = LogEntry {
                    actions: vec![
                        Action::Add(AddFile {
                            path: file_name,
                            size: metadata.len(),
                            timestamp: now_ms(),
                            stats: FileStats { num_rows: 10 },
                        }),
                        Action::Commit(CommitInfo {
                            version: 0,
                            timestamp: now_ms(),
                            operation: "INSERT".into(),
                        }),
                    ],
                };
                t.commit_with_retry(entry).unwrap();
            }
        });

        h1.join().unwrap();
        h2.join().unwrap();

        let final_table = DeltaTable::open(&dir).unwrap();
        assert_eq!(final_table.version(), 10); // 10 commits after version 0
        assert_eq!(final_table.files().len(), 10);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_open_existing() {
        let dir = tmp_dir("open_exist");
        {
            let mut t = DeltaTable::create(&dir, test_schema()).unwrap();
            t.insert(&make_block(0, 4)).unwrap();
            t.insert(&make_block(4, 3)).unwrap();
        }

        let t2 = DeltaTable::open(&dir).unwrap();
        assert_eq!(t2.version(), 2);
        assert_eq!(t2.files().len(), 2);

        let data = t2.read().unwrap();
        assert_eq!(data.num_rows, 7);

        let schema = t2.schema().unwrap();
        assert_eq!(schema.fields.len(), 2);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_optimize() {
        let dir = tmp_dir("optimize");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();

        for i in 0..5 {
            table.insert(&make_block(i * 2, 2)).unwrap();
        }
        assert_eq!(table.files().len(), 5);

        let result = table.optimize().unwrap();
        assert_eq!(result.files_compacted, 5);
        assert_eq!(result.new_files, 1);
        assert_eq!(table.files().len(), 1);

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 10);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_version_not_found() {
        let dir = tmp_dir("ver_notfound");
        let table = DeltaTable::create(&dir, test_schema()).unwrap();

        let result = table.read_version(99);
        assert!(result.is_err());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_protocol_in_log() {
        let dir = tmp_dir("protocol");
        DeltaTable::create(&dir, test_schema()).unwrap();

        let log_path = dir.join("_delta_log").join(version_filename(0));
        let raw = fs::read_to_string(&log_path).unwrap();
        let entry: LogEntry = serde_json::from_str(&raw).unwrap();

        let has_protocol = entry.actions.iter().any(|a| {
            matches!(a, Action::Protocol(p) if p.min_reader_version == 1 && p.min_writer_version == 2)
        });
        assert!(has_protocol);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_schema_merge_on_insert() {
        let dir = tmp_dir("schema_merge");
        let schema = DeltaSchema::new(vec![
            SchemaField { name: "id".into(), dtype: "INT64".into(), nullable: false },
        ]);
        let mut table = DeltaTable::create(&dir, schema).unwrap();

        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(1), Some(2)]),
            Column::float64("score", vec![Some(9.5), Some(8.0)]),
        ]).unwrap();
        table.insert(&block).unwrap();

        let s = table.schema().unwrap();
        assert_eq!(s.fields.len(), 2);
        assert_eq!(s.fields[1].name, "score");
        assert!(s.fields[1].nullable);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_update() {
        let dir = tmp_dir("update");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 5)).unwrap();

        table.update("id >= 3", "value", "99.0").unwrap();

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 5);

        if let ColumnData::Float64(v) = &data.columns[1].data {
            assert_eq!(v[3], Some(99.0));
            assert_eq!(v[4], Some(99.0));
            assert!(v[0].unwrap() < 99.0);
        } else {
            panic!("expected Float64");
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_merge_into() {
        let dir = tmp_dir("merge_into");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 5)).unwrap();

        let source = DataBlock::new(vec![
            Column::int64("id", vec![Some(3), Some(4), Some(10)]),
            Column::float64("value", vec![Some(100.0), Some(200.0), Some(300.0)]),
        ]).unwrap();

        table.merge_into(&source, "id").unwrap();

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 6); // kept 0,1,2 from original + replaced 3,4 + new 10

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_rename_column() {
        let dir = tmp_dir("rename_col");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 3)).unwrap();

        table.rename_column("value", "score").unwrap();

        let s = table.schema().unwrap();
        assert!(s.field("score").is_some());
        assert!(s.field("value").is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_schema_evolution_null_backfill() {
        let dir = tmp_dir("null_backfill");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 3)).unwrap();

        table.alter_add_column("tag", "STRING").unwrap();

        let data_with_tag = DataBlock::new(vec![
            Column::int64("id", vec![Some(10), Some(11)]),
            Column::float64("value", vec![Some(1.0), Some(2.0)]),
            Column::str_col("tag", vec![Some("x".into()), Some("y".into())]),
        ]).unwrap();
        table.insert(&data_with_tag).unwrap();

        let all = table.read().unwrap();
        assert_eq!(all.num_rows, 5);
        assert_eq!(all.columns.len(), 3);

        if let ColumnData::Str(vals) = &all.columns[2].data {
            assert_eq!(vals[0], None);
            assert_eq!(vals[1], None);
            assert_eq!(vals[2], None);
            assert_eq!(vals[3], Some("x".into()));
            assert_eq!(vals[4], Some("y".into()));
        } else {
            panic!("expected Str column for tag");
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_at_timestamp() {
        let dir = tmp_dir("timestamp");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();

        let _ts_before = now_ms();
        std::thread::sleep(std::time::Duration::from_millis(50));
        table.insert(&make_block(0, 3)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let ts_mid = now_ms();
        std::thread::sleep(std::time::Duration::from_millis(50));
        table.insert(&make_block(3, 2)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let ts_after = now_ms();

        let data_mid = table.read_at_timestamp(ts_mid).unwrap();
        assert_eq!(data_mid.num_rows, 3);

        let data_after = table.read_at_timestamp(ts_after).unwrap();
        assert_eq!(data_after.num_rows, 5);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_three_version_time_travel() {
        let dir = tmp_dir("three_ver_tt");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();

        table.insert(&make_block(0, 2)).unwrap();   // v1: 2 rows
        table.insert(&make_block(10, 3)).unwrap();  // v2: 5 rows
        table.insert(&make_block(20, 4)).unwrap();  // v3: 9 rows

        let v1 = table.read_version(1).unwrap();
        assert_eq!(v1.num_rows, 2);

        let v2 = table.read_version(2).unwrap();
        assert_eq!(v2.num_rows, 5);

        let v3 = table.read_version(3).unwrap();
        assert_eq!(v3.num_rows, 9);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_verify_remaining() {
        let dir = tmp_dir("del_verify");
        let mut table = DeltaTable::create(&dir, test_schema()).unwrap();
        table.insert(&make_block(0, 10)).unwrap();

        table.delete("id >= 5").unwrap();

        let data = table.read().unwrap();
        assert_eq!(data.num_rows, 5);

        if let ColumnData::Int64(v) = &data.columns[0].data {
            for val in v {
                assert!(val.unwrap() < 5);
            }
        } else {
            panic!("expected Int64");
        }

        fs::remove_dir_all(&dir).ok();
    }
}
