//! World O (opt-in): an open-ended sandbox for the being. New task families keep appearing over the
//! being's life (procedurally generated from World v2's family types, from simple to compositional),
//! difficulty rises with each family's tier and age, some hidden rules change once, and old families
//! retire. See docs/open_world.md.
//!
//! Separate files and a separate log source (`being:worldo`): the first world, World v2 and every
//! existing measurement are untouched. The learner is the existing World v2 learner (the B1 model and
//! planner keyed by family slot, the B2/B5 rule search), with the Track B11 skip test as a switch.
//! Author-defined simulation; the only claims are the measured numbers in docs/measured.md.

pub mod engine;
pub mod measure;
pub mod world;
#[cfg(test)]
mod tests;
