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
//! * the step limit,
//! * the stop flag (set from another thread; checked before every step),
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
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::MaxSteps => "step limit reached",
            StopReason::StopFlag => "stop flag set",
            StopReason::BudgetExhausted => "budget exhausted",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub max_steps: u64,
    pub stop: Arc<AtomicBool>,
    pub refill_when_exhausted: bool,
}

impl Policy {
    pub fn new(max_steps: u64) -> Policy {
        Policy { max_steps, stop: Arc::new(AtomicBool::new(false)), refill_when_exhausted: false }
    }
    /// Checked before every step.
    pub fn gate(&self, steps_done: u64) -> Option<StopReason> {
        if self.stop.load(Ordering::SeqCst) {
            Some(StopReason::StopFlag)
        } else if steps_done >= self.max_steps {
            Some(StopReason::MaxSteps)
        } else {
            None
        }
    }
}
