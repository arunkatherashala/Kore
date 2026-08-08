//! Body — the engine layer that `kore-self` (the mind/soul) commands.
//!
//! In the KORE architecture, `kore-self` is the reflective, self-modelling layer
//! (the "mind" or "soul"). The `Body` module is the deterministic, physical
//! engine layer that performs the actual work on behalf of the mind: reading
//! files, running KQL queries, persisting results, and scaling out across CPU
//! cores. It keeps the self layer decoupled from the underlying engine crates
//! (`kore-io`, `kore-parquet`, `kore-store`, `kore-sql`, `kore-distributed`).
//!
//! This module now implements the generic `kore_body::KoreBody` trait, so the
//! engine form is just one possible body KORE can inhabit.
//!
//! All paths are resolved relative to the `EngineBody`'s data directory unless they
//! are already absolute.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use kore_body::{BodyCommand, BodyResult, BodyState, Capability, FileFormat, KoreBody, Observation};
use kore_core::DataBlock;
use kore_federation::Constitution;
use kore_sql::executor::KqlContext;

/// Errors that can occur when the body executes a command.
#[derive(Debug)]
pub enum BodyError {
    /// Generic I/O failure (file creation, missing file, etc.).
    Io(std::io::Error),
    /// Failure from the CSV/NDJSON engine.
    Csv(kore_io::IoError),
    /// Failure from the Parquet engine.
    Parquet(kore_parquet::ParquetError),
    /// Failure from the core KORE engine (KQL execution, schema mismatch, etc.).
    Kore(kore_core::KoreError),
    /// Failure from the distributed query engine.
    Distributed(String),
    /// DML-specific failure message.
    Dml(String),
    /// Unsupported body-level command.
    Unsupported(String),
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BodyError::Io(e) => write!(f, "I/O error: {e}"),
            BodyError::Csv(e) => write!(f, "CSV engine error: {e}"),
            BodyError::Parquet(e) => write!(f, "Parquet engine error: {e}"),
            BodyError::Kore(e) => write!(f, "KORE engine error: {e}"),
            BodyError::Distributed(e) => write!(f, "distributed query error: {e}"),
            BodyError::Dml(e) => write!(f, "DML error: {e}"),
            BodyError::Unsupported(e) => write!(f, "unsupported body command: {e}"),
        }
    }
}

impl Error for BodyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            BodyError::Io(e) => Some(e),
            BodyError::Csv(e) => Some(e),
            BodyError::Parquet(e) => Some(e),
            BodyError::Kore(e) => Some(e),
            BodyError::Distributed(_) | BodyError::Dml(_) | BodyError::Unsupported(_) => None,
        }
    }
}

impl From<std::io::Error> for BodyError {
    fn from(err: std::io::Error) -> Self {
        BodyError::Io(err)
    }
}

impl From<kore_io::IoError> for BodyError {
    fn from(err: kore_io::IoError) -> Self {
        BodyError::Csv(err)
    }
}

impl From<kore_parquet::ParquetError> for BodyError {
    fn from(err: kore_parquet::ParquetError) -> Self {
        BodyError::Parquet(err)
    }
}

impl From<kore_core::KoreError> for BodyError {
    fn from(err: kore_core::KoreError) -> Self {
        BodyError::Kore(err)
    }
}

impl From<BodyError> for kore_body::BodyError {
    fn from(err: BodyError) -> Self {
        match err {
            BodyError::Unsupported(cmd) => kore_body::BodyError::UnsupportedCommand(cmd),
            _ => kore_body::BodyError::ExecutionFailed(err.to_string()),
        }
    }
}

/// The engine-layer body that the mind commands.
pub struct EngineBody {
    /// Root directory for relative file paths.
    data_dir: PathBuf,
    /// Internal KQL workspace context.
    ctx: KqlContext,
    /// Optional ethical constitution that can veto actions.
    constitution: Option<Constitution>,
}

impl EngineBody {
    /// Create a new `EngineBody` rooted at `data_dir`.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            ctx: KqlContext::new(),
            constitution: None,
        }
    }

    /// Attach an ethical constitution to this body.
    pub fn with_constitution(mut self, constitution: &Constitution) -> Self {
        self.constitution = Some(constitution.clone());
        self
    }

    /// Check whether this body is allowed to perform a command by its constitution.
    fn constitution_allows(&self, command: &BodyCommand) -> bool {
        let Some(c) = &self.constitution else { return true };
        let desc = format!("{:?}", command);
        c.can_act(&desc)
    }

    /// Resolve a path relative to `data_dir` when it is not absolute.
    fn resolve(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.data_dir.join(path)
        }
    }

    // ── Read methods ──────────────────────────────────────────────────────────

    /// Read a CSV file into a `DataBlock`.
    pub fn read_csv(&self, path: impl AsRef<Path>) -> Result<DataBlock, BodyError> {
        let path = self.resolve(path);
        Ok(kore_io::CsvReader::new(path).read()?)
    }

    /// Read a Parquet file into a `DataBlock`.
    pub fn read_parquet(&self, path: impl AsRef<Path>) -> Result<DataBlock, BodyError> {
        let path = self.resolve(path);
        Ok(kore_parquet::ParquetReader::new(path).read()?)
    }

    /// Read a native `.kore` file into a `DataBlock`.
    pub fn read_kore(&self, path: impl AsRef<Path>) -> Result<DataBlock, BodyError> {
        let path = self.resolve(path);
        Ok(kore_store::KoreReader::read_file(&path)?)
    }

    // ── Compute methods ───────────────────────────────────────────────────────

    /// Execute a KQL `SELECT` query against the registered workspace.
    pub fn query(&self, sql: &str) -> Result<DataBlock, BodyError> {
        Ok(self.ctx.query(sql)?)
    }

    /// Execute a KQL DML statement (`INSERT`, `UPDATE`, `DELETE`, etc.).
    ///
    /// Returns the operation name and the number of rows affected.
    pub fn execute_dml(&mut self, sql: &str) -> Result<(String, usize), BodyError> {
        Ok(self.ctx.execute_dml(sql)?)
    }

    /// Register a `DataBlock` under a table name for subsequent queries.
    pub fn register(&mut self, name: impl Into<String>, block: DataBlock) {
        self.ctx.register(name, block);
    }

    /// Load KORE's memories into the body as a queryable table named `memories`.
    pub fn load_memories(&mut self, memories: &[crate::Memory]) {
        self.register("memories", crate::kore_query::memories_to_block(memories));
    }

    /// Look up a previously registered table by name.
    pub fn get(&self, name: &str) -> Option<&DataBlock> {
        self.ctx.get(name)
    }

    // ── Store methods ─────────────────────────────────────────────────────────

    /// Write a `DataBlock` to the native `.kore` format.
    pub fn write_kore(&self, path: impl AsRef<Path>, block: &DataBlock) -> Result<(), BodyError> {
        let path = self.resolve(path);
        Ok(kore_store::KoreWriter::write_file(&path, block)?)
    }

    /// Write a `DataBlock` to Parquet format.
    pub fn write_parquet(&self, path: impl AsRef<Path>, block: &DataBlock) -> Result<(), BodyError> {
        let path = self.resolve(path);
        Ok(kore_parquet::ParquetWriter::write_file(block, path)?)
    }

    // ── Scale methods ─────────────────────────────────────────────────────────

    /// Execute a KQL query in distributed mode using all available CPU cores.
    ///
    /// The `table_name` must be a table previously registered in this body.
    /// The SQL should reference the same table name.
    pub fn distributed_query(&self, sql: &str, table_name: &str) -> Result<DataBlock, BodyError> {
        let data = self
            .ctx
            .get(table_name)
            .cloned()
            .ok_or_else(|| BodyError::Distributed(format!("table '{}' not registered", table_name)))?;
        kore_distributed::distributed_query(sql, data).map_err(BodyError::Distributed)
    }
}

impl KoreBody for EngineBody {
    fn observe(&self) -> Vec<Observation> {
        let constitution_status = match &self.constitution {
            Some(c) => format!("constitution with {} rules", c.rules.len()),
            None => "no constitution".to_string(),
        };
        vec![
            Observation::Health { status: "engine ready".to_string(), severity: 0 },
            Observation::Numeric { name: "registered_tables".to_string(), value: self.ctx.table_names().len() as f64 },
            Observation::Text(constitution_status),
        ]
    }

    fn act(&mut self, command: BodyCommand) -> Result<BodyResult, kore_body::BodyError> {
        if !self.constitution_allows(&command) {
            return Err(kore_body::BodyError::ExecutionFailed(
                "command rejected by KORE constitution".to_string()
            ));
        }
        match command {
            BodyCommand::Query { sql } => {
                let block = self.query(&sql).map_err(kore_body::BodyError::from)?;
                let summary = format!("query returned {} rows, {} columns", block.num_rows, block.columns.len());
                Ok(BodyResult::with_data(summary, block))
            }
            BodyCommand::ExecuteDml { sql } => {
                let (op, rows) = self.execute_dml(&sql).map_err(kore_body::BodyError::from)?;
                Ok(BodyResult::ok(format!("{op} affected {rows} rows")))
            }
            BodyCommand::LoadTable { name, block } => {
                self.register(name, block);
                Ok(BodyResult::ok("table loaded".to_string()))
            }
            BodyCommand::ReadFile { path, format } => {
                let block = match format {
                    FileFormat::Csv => self.read_csv(&path),
                    FileFormat::Parquet => self.read_parquet(&path),
                    FileFormat::Kore => self.read_kore(&path),
                    FileFormat::Json => Err(BodyError::Unsupported("json read not yet implemented".to_string())),
                }
                .map_err(kore_body::BodyError::from)?;
                Ok(BodyResult::with_data(format!("read {path}"), block))
            }
            BodyCommand::WriteFile { name, block, format } => {
                match format {
                    FileFormat::Kore => self.write_kore(&name, &block),
                    FileFormat::Parquet => self.write_parquet(&name, &block),
                    _ => Err(BodyError::Unsupported(format!("write format {:?} not supported", format))),
                }
                .map_err(kore_body::BodyError::from)?;
                Ok(BodyResult::ok(format!("wrote {name}")))
            }
            BodyCommand::DistributedQuery { sql } => {
                // Default to the 'memories' table; if absent, try the first registered table.
                let table_names = self.ctx.table_names();
                let table_name = if self.ctx.get("memories").is_some() {
                    "memories"
                } else {
                    table_names.first().map(|s| s.as_str()).unwrap_or("memories")
                };
                let block = self.distributed_query(&sql, table_name).map_err(kore_body::BodyError::from)?;
                Ok(BodyResult::with_data(format!("distributed query returned {} rows", block.num_rows), block))
            }
            BodyCommand::Move { direction, distance } => {
                Ok(BodyResult::with_observation(
                    "engine body cannot physically move; recorded intent",
                    Observation::Text(format!("would move {direction} by {distance}")),
                ))
            }
            BodyCommand::Speak { message } => {
                Ok(BodyResult::with_observation("spoke", Observation::Text(message)))
            }
            BodyCommand::Sense { modality, .. } => {
                Ok(BodyResult::with_observation(
                    "sensed",
                    Observation::Text(format!("engine body sensed {modality}")),
                ))
            }
            BodyCommand::Connect { target } => {
                Ok(BodyResult::ok(format!("engine body connected to {target}")))
            }
            BodyCommand::Sleep => Ok(BodyResult::ok("engine body standby".to_string())),
            BodyCommand::Wake => Ok(BodyResult::ok("engine body active".to_string())),
            BodyCommand::Replicate { target } => {
                Ok(BodyResult::ok(format!("replication intent recorded for {target}")))
            }
            BodyCommand::Shutdown => Ok(BodyResult::ok("engine body shutdown intent recorded".to_string())),
            BodyCommand::Raw { name, payload } => {
                Ok(BodyResult::ok(format!("raw command {name}: {payload}")))
            }
        }
    }

    fn state(&self) -> BodyState {
        BodyState {
            kind: "engine".to_string(),
            health: 1.0,
            energy: 1.0,
            location: Some(self.data_dir.to_string_lossy().to_string()),
            load: 0.0,
            status: "ready".to_string(),
            uptime_secs: 0,
        }
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::Query,
            Capability::Store,
            Capability::Compute,
            Capability::ReadFile,
            Capability::WriteFile,
            Capability::Connect,
        ]
    }

    fn kind(&self) -> &'static str {
        "engine"
    }
}
