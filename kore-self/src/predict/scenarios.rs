//! Synthetic ground-truth sequences for measuring predictors. The generating
//! process is known, so what a predictor can and cannot learn is known too.
//! Numbers measured here say nothing about real behaviour.

use super::score::Sm;
use super::Obs;
use crate::learn::dates;

pub const CHOICES: [&str; 4] = ["a", "b", "c", "d"];
pub const CONTEXTS: [&str; 4] = ["c0", "c1", "c2", "c3"];
/// Probability of the favoured choice in the context and drift scenarios.
pub const P_CONTEXT: f64 = 0.8;
pub const P_DRIFT: f64 = 0.7;
/// The dominant choice of the drift scenario changes every this many observations.
pub const DRIFT_EVERY: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// Each context has its own favourite choice (p = 0.8); contexts are drawn uniformly, so
    /// the overall choice frequencies are uniform and only the context carries information.
    Context,
    /// Choices uniform and independent of everything. Nothing can beat uniform.
    Noise,
    /// The context is irrelevant; one choice dominates (p = 0.7) and the dominant choice
    /// changes every `DRIFT_EVERY` observations.
    Drift,
}

impl Scenario {
    pub fn name(self) -> &'static str {
        match self {
            Scenario::Context => "context matters",
            Scenario::Noise => "pure noise",
            Scenario::Drift => "drift",
        }
    }
}

/// Pick the favourite with probability `p`, otherwise uniformly among the others.
fn pick(rng: &mut Sm, fav: usize, p: f64) -> usize {
    if rng.f64() < p {
        fav
    } else {
        let o = (rng.next() % (CHOICES.len() as u64 - 1)) as usize;
        if o >= fav { o + 1 } else { o }
    }
}

/// `n` observations, one per hour from day 19000 (a fixed arbitrary date), deterministic in `seed`.
pub fn generate(s: Scenario, seed: u64, n: usize) -> Vec<Obs> {
    let mut rng = Sm(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5851_F42D_4C95_7F2D);
    (0..n)
        .map(|i| {
            let ctx = (rng.next() % CONTEXTS.len() as u64) as usize;
            let choice = match s {
                Scenario::Context => pick(&mut rng, ctx, P_CONTEXT),
                Scenario::Noise => (rng.next() % CHOICES.len() as u64) as usize,
                Scenario::Drift => pick(&mut rng, (i / DRIFT_EVERY) % CHOICES.len(), P_DRIFT),
            };
            let ts = format!("{}{:02}:00:00Z", &dates::midnight_of(19000 + (i / 24) as i64)[..11], i % 24);
            Obs { ts, ctx: CONTEXTS[ctx].to_string(), choice: CHOICES[choice].to_string() }
        })
        .collect()
}
