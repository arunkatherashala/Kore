//! World v2 (opt-in): a larger, more varied sandbox for the being, with many procedurally generated
//! task families (hidden-rule sequences, grid paths, knapsacks, formulas with a hidden constant,
//! compositional sequence tasks), feedback noise and hidden drift. See docs/world_v2.md.
//!
//! Nothing here changes the first world: separate files, a separate log source (`being:world2`);
//! the original `World`, `BeingState`, loop and replay are untouched. The learner measured here is
//! the existing B1 model and planner, keyed by family (`model.rs`, `learner.rs`), with the existing
//! B2/B5 rule search (`discover.rs`). Author-defined simulation; the only claims are the measured
//! numbers in docs/measured.md.

pub mod baselines;
pub mod engine;
pub mod learner;
pub mod measure;
pub mod model;
pub mod replay;
pub mod solve;
pub mod world;
#[cfg(test)]
mod tests;
