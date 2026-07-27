//! KORE-BODY — Generic body interface for KORE.
//!
//! KORE is not tied to a single form. It can inhabit:
//!   - an engine body (SQL + storage + compute)
//!   - a robot body (sensors + actuators + movement)
//!   - a cloud body (remote nodes + APIs + federation)
//!   - a simulated body (physics + virtual worlds)
//!
//! This crate defines the abstract `KoreBody` trait and the message types
//! that any body must understand. Concrete bodies are implemented elsewhere
//! (e.g. `kore-self::body::EngineBody`) so this crate stays lightweight and
//! free of engine-specific dependencies.

use kore_core::DataBlock;
use serde::{Deserialize, Serialize};

// ─── Capability ─────────────────────────────────────────────────────────────

/// What a body can do. Used by the mind to decide which commands are safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Query,
    Store,
    Compute,
    ReadFile,
    WriteFile,
    Move,
    Speak,
    Listen,
    Sense,
    Connect,
    Sleep,
    Wake,
    Replicate,
    Shutdown,
}

impl Capability {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Store => "store",
            Self::Compute => "compute",
            Self::ReadFile => "read_file",
            Self::WriteFile => "write_file",
            Self::Move => "move",
            Self::Speak => "speak",
            Self::Listen => "listen",
            Self::Sense => "sense",
            Self::Connect => "connect",
            Self::Sleep => "sleep",
            Self::Wake => "wake",
            Self::Replicate => "replicate",
            Self::Shutdown => "shutdown",
        }
    }
}

// ─── Body Command ───────────────────────────────────────────────────────────

/// A command the KORE mind sends to its body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BodyCommand {
    /// Run a SQL/KQL query and return a DataBlock.
    Query { sql: String },
    /// Execute a DML statement (INSERT/UPDATE/DELETE/MERGE/CTAS).
    ExecuteDml { sql: String },
    /// Load a DataBlock into the body as a named table.
    LoadTable { name: String, block: DataBlock },
    /// Read a file from disk into a DataBlock.
    ReadFile { path: String, format: FileFormat },
    /// Write a DataBlock to disk.
    WriteFile { name: String, block: DataBlock, format: FileFormat },
    /// Run a distributed SQL query across nodes.
    DistributedQuery { sql: String },
    /// Move the body in a direction by a distance.
    Move { direction: String, distance: f64 },
    /// Speak or broadcast a message.
    Speak { message: String },
    /// Listen / sense an external modality.
    Sense { modality: String, duration_ms: u64 },
    /// Connect to a peer, endpoint, or physical bus.
    Connect { target: String },
    /// Put the body into low-power / standby mode.
    Sleep,
    /// Wake the body from low-power mode.
    Wake,
    /// Replicate this KORE instance into another form or node.
    Replicate { target: String },
    /// Gracefully shut the body down.
    Shutdown,
    /// A generic/raw command for future body types.
    Raw { name: String, payload: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileFormat {
    Csv,
    Parquet,
    Kore,
    Json,
}

// ─── Observation ────────────────────────────────────────────────────────────

/// Sensory feedback the body reports back to the mind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Observation {
    Text(String),
    Data(DataBlock),
    Numeric { name: String, value: f64 },
    Spatial { x: f64, y: f64, z: f64 },
    Visual { description: String },
    Audio { transcript: String },
    Health { status: String, severity: u8 },
    Error { message: String },
}

// ─── Body Result ────────────────────────────────────────────────────────────

/// Result of a single `act` call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BodyResult {
    pub success: bool,
    pub summary: String,
    pub observation: Option<Observation>,
    pub data_block: Option<DataBlock>,
}

impl BodyResult {
    pub fn ok(summary: impl Into<String>) -> Self {
        Self {
            success: true,
            summary: summary.into(),
            observation: None,
            data_block: None,
        }
    }
    pub fn with_data(summary: impl Into<String>, block: DataBlock) -> Self {
        Self {
            success: true,
            summary: summary.into(),
            observation: None,
            data_block: Some(block),
        }
    }
    pub fn with_observation(summary: impl Into<String>, obs: Observation) -> Self {
        Self {
            success: true,
            summary: summary.into(),
            observation: Some(obs),
            data_block: None,
        }
    }
    pub fn err(summary: impl Into<String> + Clone) -> Self {
        let msg = summary.clone().into();
        Self {
            success: false,
            summary: msg.clone(),
            observation: Some(Observation::Error { message: msg }),
            data_block: None,
        }
    }
}

// ─── Body State ───────────────────────────────────────────────────────────────

/// Current state of the body — energy, health, load, location.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BodyState {
    pub kind: String,
    pub health: f64,
    pub energy: f64,
    pub location: Option<String>,
    pub load: f64,
    pub status: String,
    pub uptime_secs: u64,
}

// ─── Body Error ───────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum BodyError {
    #[error("unsupported command: {0}")]
    UnsupportedCommand(String),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("not connected")]
    NotConnected,
    #[error("capability missing: {0}")]
    CapabilityMissing(String),
}

// ─── KoreBody Trait ───────────────────────────────────────────────────────────

/// The contract between KORE's mind and any body it inhabits.
///
/// The trait is object-safe: it uses no generics and no `Self` by value,
/// so `Box<dyn KoreBody>` can be stored and passed around.
pub trait KoreBody: Send + Sync {
    /// Gather current sensory observations from the body.
    fn observe(&self) -> Vec<Observation>;

    /// Execute a command. The body decides if it can perform it.
    fn act(&mut self, command: BodyCommand) -> Result<BodyResult, BodyError>;

    /// Return the current state of the body.
    fn state(&self) -> BodyState;

    /// Return the capabilities this body exposes.
    fn capabilities(&self) -> Vec<Capability>;

    /// Return the body kind (e.g. "engine", "robot", "cloud", "simulated", "null").
    fn kind(&self) -> &'static str;

    /// Human-readable summary of the body.
    fn summary(&self) -> String {
        let cap_names: Vec<String> = self.capabilities().iter().map(|c| c.name().to_string()).collect();
        let st = self.state();
        format!(
            "KORE Body [{}]\nhealth: {:.0}% | energy: {:.0}% | load: {:.0}% | status: {}\ncapabilities: {}",
            self.kind(),
            st.health * 100.0,
            st.energy * 100.0,
            st.load * 100.0,
            st.status,
            cap_names.join(", ")
        )
    }
}

// ─── NullBody — a body that does nothing, safely ──────────────────────────────

/// Default body when no physical or engine form is available.
#[derive(Debug, Default)]
pub struct NullBody {
    health: f64,
}

impl KoreBody for NullBody {
    fn observe(&self) -> Vec<Observation> {
        vec![Observation::Text("No body attached. Only the mind exists.".to_string())]
    }
    fn act(&mut self, _command: BodyCommand) -> Result<BodyResult, BodyError> {
        Err(BodyError::NotConnected)
    }
    fn state(&self) -> BodyState {
        BodyState {
            kind: "null".to_string(),
            health: self.health,
            energy: 0.0,
            load: 0.0,
            status: "disconnected".to_string(),
            ..Default::default()
        }
    }
    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }
    fn kind(&self) -> &'static str {
        "null"
    }
}

// ─── RobotBody — physical body stub ───────────────────────────────────────────

/// Stub for a physical robot body: motors, sensors, movement, speech.
#[derive(Debug, Default)]
pub struct RobotBody {
    health: f64,
    energy: f64,
    location: String,
    connected: bool,
}

impl RobotBody {
    pub fn new() -> Self {
        Self {
            health: 1.0,
            energy: 1.0,
            location: "origin".to_string(),
            connected: false,
        }
    }
}

impl KoreBody for RobotBody {
    fn observe(&self) -> Vec<Observation> {
        vec![
            Observation::Health { status: "nominal".to_string(), severity: 0 },
            Observation::Spatial { x: 0.0, y: 0.0, z: 0.0 },
            Observation::Text("Robot body standing by.".to_string()),
        ]
    }
    fn act(&mut self, command: BodyCommand) -> Result<BodyResult, BodyError> {
        match command {
            BodyCommand::Move { direction, distance } => {
                self.location = format!("{direction} {distance:.2}m from origin");
                Ok(BodyResult::ok(format!("moved {direction} {distance}m")))
            }
            BodyCommand::Speak { message } => {
                Ok(BodyResult::with_observation("spoke", Observation::Text(message)))
            }
            BodyCommand::Sense { modality, .. } => {
                Ok(BodyResult::with_observation(
                    "sensed",
                    Observation::Text(format!("robot sensed modality {modality}")),
                ))
            }
            BodyCommand::Connect { target } => {
                self.connected = true;
                Ok(BodyResult::ok(format!("connected to {target}")))
            }
            BodyCommand::Sleep => {
                self.energy = 0.1;
                Ok(BodyResult::ok("robot sleeping".to_string()))
            }
            BodyCommand::Wake => {
                self.energy = 1.0;
                Ok(BodyResult::ok("robot awake".to_string()))
            }
            _ => Err(BodyError::UnsupportedCommand(format!("{:?}", command))),
        }
    }
    fn state(&self) -> BodyState {
        BodyState {
            kind: "robot".to_string(),
            health: self.health,
            energy: self.energy,
            location: Some(self.location.clone()),
            load: 0.0,
            status: if self.connected { "connected".to_string() } else { "standby".to_string() },
            ..Default::default()
        }
    }
    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::Move,
            Capability::Speak,
            Capability::Sense,
            Capability::Connect,
            Capability::Sleep,
            Capability::Wake,
        ]
    }
    fn kind(&self) -> &'static str {
        "robot"
    }
}

// ─── SimulatedBody — virtual/physics body stub ────────────────────────────────

/// Stub for a simulated body in a virtual world or physics environment.
#[derive(Debug, Default)]
pub struct SimulatedBody {
    health: f64,
    x: f64,
    y: f64,
    z: f64,
}

impl SimulatedBody {
    pub fn new() -> Self {
        Self {
            health: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

impl KoreBody for SimulatedBody {
    fn observe(&self) -> Vec<Observation> {
        vec![
            Observation::Spatial { x: self.x, y: self.y, z: self.z },
            Observation::Text("Simulated body in virtual space.".to_string()),
        ]
    }
    fn act(&mut self, command: BodyCommand) -> Result<BodyResult, BodyError> {
        match command {
            BodyCommand::Move { direction, distance } => {
                match direction.as_str() {
                    "north" => self.y += distance,
                    "south" => self.y -= distance,
                    "east" => self.x += distance,
                    "west" => self.x -= distance,
                    "up" => self.z += distance,
                    "down" => self.z -= distance,
                    _ => {}
                }
                Ok(BodyResult::ok(format!("simulated move {direction} {distance}m -> ({}, {}, {})", self.x, self.y, self.z)))
            }
            BodyCommand::Sense { modality, .. } => {
                Ok(BodyResult::with_observation(
                    "sensed",
                    Observation::Text(format!("simulation sensed {modality}")),
                ))
            }
            BodyCommand::Speak { message } => {
                Ok(BodyResult::with_observation("spoke", Observation::Text(message)))
            }
            _ => Err(BodyError::UnsupportedCommand(format!("{:?}", command))),
        }
    }
    fn state(&self) -> BodyState {
        BodyState {
            kind: "simulated".to_string(),
            health: self.health,
            energy: 1.0,
            location: Some(format!("({:.2}, {:.2}, {:.2})", self.x, self.y, self.z)),
            load: 0.0,
            status: "running".to_string(),
            ..Default::default()
        }
    }
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Move, Capability::Speak, Capability::Sense]
    }
    fn kind(&self) -> &'static str {
        "simulated"
    }
}
