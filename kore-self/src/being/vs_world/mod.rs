//! Track V1: the being's learner versus standard methods from the literature and a language model, on
//! the same worlds, seeds and budgets (see docs/vs_world.md). Measurement code only: nothing here is
//! used by the being, and nothing here writes a log.
//!
//! * `core.rs`: the methods (bandits, linear contextual bandits, tabular RL, a small MLP Q-learner), all
//!   seeing only visible features through `Opt` / `Ctx`;
//! * `adapt.rs`: the methods as choosers of World v2, World O, World L and the first menu world;
//! * `mcts.rs`: MCTS on the TRUE simulator, an upper-bound reference that sees hidden information;
//! * `rules.rs`: rule learning on revealed examples (the B2 guard with brute-force enumeration, k-NN,
//!   regression trees);
//! * `llm.rs`: the language-model player (requests of synthetic features only; the call is a closure).
//!
//! Author-defined simulation; the only claims are the measured numbers in docs/measured.md.

pub mod adapt;
/// Track B13 baseline: rules where a rule covers the offer, tuned LinUCB elsewhere.
pub mod fallback;
pub mod core;
pub mod llm;
pub mod mcts;
pub mod rules;
#[cfg(test)]
mod tests;
