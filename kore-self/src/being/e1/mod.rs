//! Track E1: evolutionary search (genetic programming) over the being's rule DSL (first world;
//! opt-in, default off).
//!
//! * `genome.rs`: a rule expression (lags, index, constants, coefficients fitted per task, +, -, *,
//!   optional modulus) as a `discover::Form::Gp`; every B2 grammar form is a genome.
//! * `evolve.rs`: populations of candidate rules, mutation, crossover (also with the being's verified
//!   rules: inherited genes), selection by exact matches on the being's own training examples; a
//!   candidate becomes a rule only through the B2 check (training and held-out exact reproduction,
//!   never applicable-and-wrong). Budgeted, deterministic per seed, logged, replayable from the log.
//! * `ctl.rs`: the owner's switches in controller.txt (fail closed, default off).
//! * `measure.rs`: the in-process measurement (side channel, never logged, never read by the being).
//!
//! Synthetic world, author-defined families, expression language and rewards; the claims are the
//! measured numbers in docs/measured.md (Track E1), nothing more.

pub mod ctl;
pub mod evolve;
pub mod genome;
pub mod measure;
#[cfg(test)]
mod tests;
