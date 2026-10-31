//! What the loop may touch, and when it must stop.
//!
//! The loop gets exactly two things: the simulated world (`world.rs`, pure
//! functions of a seed) and the being's own versioned log, through the
//! `VersionedLog` API it is handed. It has no path, no file handle, no socket
//! and no process access of its own: the `being/` sources contain none of
//! those APIs, and a test scans them to keep it that way. Writing the log is
//! the only side effect, and the log is hash-chained, so a change is visible
//! to `self_verify`.
//!
//! Stopping: a run ends at the first of
//! * the kill switch (B5): an owner-supplied check, in the tools the existence of the `KILL` file,
//!   made before every step, so the loop stops within one step of the file appearing;
//! * the step limit,
//! * the stop flag (set from another thread; checked before every step),
//! * the energy cap of the controller (B5): no new step starts once the run has spent that much,
//! * budget exhaustion: no task in the menu is affordable. With
//!   `refill_when_exhausted` the world grants a fresh budget at the next world
//!   day (logged as a `refill` event) and the run goes on; the default is to
//!   stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const CAPABILITIES: [&str; 3] = [
    "read the simulated world (pure functions of the world seed)",
    "read the being's own log through the VersionedLog API",
    "append to the being's own log through the VersionedLog API",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    MaxSteps,
    StopFlag,
    BudgetExhausted,
    /// The kill switch was on.
    Killed,
    /// The controller's energy cap for one run was reached.
    EnergyCap,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::MaxSteps => "step limit reached",
            StopReason::StopFlag => "stop flag set",
            StopReason::BudgetExhausted => "budget exhausted",
            StopReason::Killed => "kill switch",
            StopReason::EnergyCap => "energy cap of the controller",
        }
    }
}

/// An owner-supplied check that is true when the kill switch is on (called before every step).
#[derive(Clone)]
pub struct KillCheck(pub Arc<dyn Fn() -> bool + Send + Sync>);

impl std::fmt::Debug for KillCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KillCheck")
    }
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub max_steps: u64,
    pub stop: Arc<AtomicBool>,
    pub refill_when_exhausted: bool,
    /// B5: kill switch check (None: none).
    pub kill: Option<KillCheck>,
    /// B5: energy cap of this run (None: none).
    pub max_energy: Option<i64>,
}

impl Policy {
    pub fn new(max_steps: u64) -> Policy {
        Policy { max_steps, stop: Arc::new(AtomicBool::new(false)), refill_when_exhausted: false, kill: None, max_energy: None }
    }
    /// Checked before every step.
    pub fn gate(&self, steps_done: u64) -> Option<StopReason> {
        self.gate_cost(steps_done, 0)
    }
    /// As `gate`, with the energy spent so far in this run.
    pub fn gate_cost(&self, steps_done: u64, energy_spent: i64) -> Option<StopReason> {
        if self.kill.as_ref().map_or(false, |k| (k.0)()) {
            Some(StopReason::Killed)
        } else if self.stop.load(Ordering::SeqCst) {
            Some(StopReason::StopFlag)
        } else if steps_done >= self.max_steps {
            Some(StopReason::MaxSteps)
        } else if self.max_energy.map_or(false, |m| energy_spent >= m) {
            Some(StopReason::EnergyCap)
        } else {
            None
        }
    }
}
