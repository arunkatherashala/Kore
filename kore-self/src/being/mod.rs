//! The being: a deterministic simulation loop with its own log (stages S1, S2).
//!
//! A simulated world (`world`) offers verifiable tasks; the being's loop
//! (`loop.rs`, as `run_loop`) observes its own log, picks a domain with a
//! learned chooser (`chooser`), has a `Thinker` (`solver`) propose an answer,
//! checks it exactly and records everything. `policy` limits what the loop can
//! touch. This is a simulation with author-defined rewards. It makes no claim
//! of life, understanding or intelligence; the only claims are the measured
//! numbers in docs/measured.md.
#![allow(dead_code)]

pub mod chooser;
pub mod measure;
pub mod model;
pub mod planner;
pub mod policy;
pub mod replay;
pub mod solver;
pub mod state;
pub mod world;
#[path = "loop.rs"]
pub mod run_loop;
#[cfg(test)]
mod tests;

pub use chooser::{DomainPicker, Ucb1};
pub use policy::{Policy, StopReason};
pub use run_loop::{run, RunCfg};
pub use solver::{Context, Proposal, ScriptedThinker, Thinker};
pub use state::BeingState;
pub use world::{Domain, World};
