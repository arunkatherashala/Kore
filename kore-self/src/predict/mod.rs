//! Prediction and calibration (milestone M3/S4).
//!
//! Events of the form "in context C the choice was X" are turned into an
//! ordered sequence of observations. A predictor sees only the observations
//! BEFORE the one it predicts, plus the context of the question, and answers
//! with a probability distribution over the choices seen so far plus
//! `(other)`, or with "insufficient data" when it has fewer observations than
//! its minimum (it never guesses). `score` computes Brier, log loss, accuracy,
//! calibration and bootstrap intervals; `backtest` runs a rolling origin with
//! a leakage check; `ledger` writes predictions to the versioned log before
//! outcomes exist and scores them later.
//!
//! No randomness outside the seeded bootstrap, no network, no language model.
//! Uses only `crate::versioned`, `crate::learn` and std (plus serde_json), so
//! examples can include it with `#[path]`. Numbers measured on synthetic data
//! are in `docs/measured.md`; nothing here is evidence about real behaviour.
#![allow(dead_code)]

pub mod backtest;
pub mod baselines;
pub mod ledger;
pub mod scenarios;
pub mod score;

use crate::learn::{Ev, ACT_PREFIX, CTX_PREFIX};

/// Reserved choice that stands for every choice not seen before the cut-off.
pub const OTHER: &str = "(other)";
/// Context of an event that has an `act:` tag but no `ctx:` tag.
pub const NO_CONTEXT: &str = "(none)";

/// One observation: at `ts`, in context `ctx`, the choice was `choice`.
#[derive(Debug, Clone, PartialEq)]
pub struct Obs {
    pub ts: String,
    pub ctx: String,
    pub choice: String,
}

/// Observations from events: every event with an `act:` tag, in the given
/// (time, id) order. A missing `ctx:` tag gives `NO_CONTEXT`.
pub fn obs_from_events(evs: &[Ev]) -> Vec<Obs> {
    evs.iter()
        .filter_map(|e| {
            let choice = e.tag_val(ACT_PREFIX)?;
            Some(Obs { ts: e.ts.clone(), ctx: e.tag_val(CTX_PREFIX).unwrap_or(NO_CONTEXT).to_string(), choice: choice.to_string() })
        })
        .collect()
}

/// The answer options visible after `hist`: the distinct choices seen, sorted, then `(other)`.
pub fn universe(hist: &[Obs]) -> Vec<String> {
    let mut v: Vec<String> = hist.iter().map(|o| o.choice.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().filter(|c| c != OTHER).collect();
    v.push(OTHER.to_string());
    v
}

/// Index of `choice` among `options`; unseen choices map to `(other)` (always last).
pub fn option_index(options: &[String], choice: &str) -> usize {
    options.iter().position(|o| o == choice).unwrap_or(options.len().saturating_sub(1))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pred {
    /// Probabilities aligned with the options passed in; each > 0, sum 1.
    Dist(Vec<f64>),
    /// Fewer than `min_n` relevant observations: refuse, do not guess.
    Insufficient { n: usize, min_n: usize },
}

pub trait Predictor {
    fn id(&self) -> &'static str;
    /// Version string including the parameters, stored with every prediction.
    fn version(&self) -> String;
    /// `hist` is everything known at the cut-off; the target observation is not in it.
    fn predict(&self, hist: &[Obs], ctx: &str, options: &[String]) -> Pred;
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
