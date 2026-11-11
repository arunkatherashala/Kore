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

pub mod adapt_b3b;
pub mod advice;
/// Track D1b: exact binary encoding of the checkpoint payload.
pub mod binser;
/// Track D1b: state checkpoints (off by default; controller key `checkpoint_every`).
pub mod ckpt;
/// Digital Life item 4: unattended-run scheduler keys (off by default; `sched_run`).
pub mod sched_ctl;
pub mod chooser;
pub mod controller;
pub mod discover;
pub mod discover_scan;
pub mod experiment;
pub mod experiment_menu;
pub mod measure_b9;
/// Track B11: the long-run decline (measurement side channel; see docs/measured.md).
pub mod measure_b11;
/// Track B8: planning ahead (opt-in; see lookahead/mod.rs).
pub mod lookahead;
/// Track B13: a context-aware success estimate for the chooser (opt-in; see linval.rs).
pub mod linval;
pub mod measure;
pub mod measure_b5;
pub mod measure_b6;
pub mod measure_b3b;
pub mod model;
pub mod planner;
pub mod policy;
pub mod replay;
pub mod solver;
pub mod state;
pub mod transfer;
pub mod trust;
pub mod world;
/// World v2 (opt-in: separate files and log source; see docs/world_v2.md).
pub mod world2;
/// World O, the open-ended world (opt-in: separate files and log source; see docs/open_world.md).
pub mod world_o;
/// Track L2: learning to learn across families (opt-in; see l2/mod.rs).
pub mod l2;
/// World T, data work in a sandbox (opt-in: separate files and log source; see docs/world_t.md).
pub mod world_t;
/// Track B14: rule library and always-on curiosity (opt-in; see b14/mod.rs).
pub mod b14;
/// Track E1: evolutionary search over the rule DSL (opt-in; see e1/mod.rs).
pub mod e1;
/// Track V1: versus standard methods (measurement only; see docs/vs_world.md).
pub mod vs_world;
#[path = "loop.rs"]
pub mod run_loop;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_b5;
#[cfg(test)]
mod tests_b5b;
#[cfg(test)]
mod tests_b6;
#[cfg(test)]
mod tests_b3b;
#[cfg(test)]
mod tests_scan;
#[cfg(test)]
mod tests_b9;
#[cfg(test)]
mod tests_b11;
#[cfg(test)]
mod tests_b13;
#[cfg(test)]
mod tests_d1;

pub use chooser::{DomainPicker, Ucb1};
pub use policy::{Policy, StopReason};
pub use run_loop::{run, run_advised, RunCfg};
pub use solver::{Context, Proposal, ScriptedThinker, Thinker};
pub use state::BeingState;
pub use world::{Domain, World};
