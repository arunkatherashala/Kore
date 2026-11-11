//! World T (opt-in, Track W1): a sandbox world of data work. Tasks over realistic-looking data files
//! (tables with quirks, time series, log files) that live in a sandbox; each task has an exact checker;
//! the being's actions are tools (the KORE SQL engine read-only on in-memory copies, its own discovered
//! rules, simple parsers and statistics), and it learns from its own log which plan works for which kind
//! of task. See docs/world_t.md.
//!
//! Separate files and a separate log source (`being:worldt`): the first world, World v2, World O and
//! every existing measurement are untouched (a golden test pins their logs). Author-defined simulation;
//! the only claims are the measured numbers in docs/measured.md, Track W1.

pub mod baselines;
pub mod ctl;
pub mod data;
pub mod engine;
pub mod learner;
pub mod measure;
pub mod tools;
pub mod world;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_golden;
#[cfg(test)]
mod tests_engine;
#[cfg(test)]
mod tests_w2;
