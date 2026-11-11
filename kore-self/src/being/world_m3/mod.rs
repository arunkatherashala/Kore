//! World M3 (opt-in, Track M3): a World T-like world built from one real public file, the NYC TLC Yellow Taxi
//! trip records of January 2024 (`source.rs`, pinned by SHA-256). Same loop, learner, log, fold and replay as
//! World T (copied and adapted here, so World T stays byte-identical); what changes is where the values come
//! from. Separate log source (`being:worldm3`), off by default behind `world_m3_run`. See docs/m3_design.md.
//! Author-defined templates, slices, quirks, rewards and energies; only the values and anomalies are real.

pub mod baselines;
pub mod ctl;
pub mod engine;
pub mod learner;
pub mod measure;
pub mod source;
pub mod tools;
pub mod world;
#[cfg(test)]
mod tests;
