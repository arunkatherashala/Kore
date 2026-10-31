//! Rolling-origin backtest: for each origin t the predictor sees `obs[..t]`
//! and the context of `obs[t]`, predicts, and is scored against `obs[t].choice`.
//!
//! `predict_at` is the only door to the data: it slices the history, so an
//! event at or after the origin cannot reach a prediction. `leakage_check`
//! proves that by changing the future and comparing the past (see tests).

use super::score::Point;
use super::{option_index, universe, Obs, Pred, Predictor};

/// The prediction a predictor makes at origin `t` (history `obs[..t]`).
pub fn predict_at(p: &dyn Predictor, obs: &[Obs], t: usize) -> (Vec<String>, Pred) {
    let hist = &obs[..t];
    let options = universe(hist);
    let pred = p.predict(hist, &obs[t].ctx, &options);
    (options, pred)
}

/// Per predictor, one entry per origin `start..obs.len()`: the scored point or
/// `None` where the predictor refused (insufficient data).
pub struct Backtest {
    pub start: usize,
    pub ids: Vec<&'static str>,
    pub points: Vec<Vec<Option<Point>>>,
}

pub fn rolling_origin(preds: &[Box<dyn Predictor>], obs: &[Obs], start: usize) -> Backtest {
    let mut points: Vec<Vec<Option<Point>>> = vec![Vec::new(); preds.len()];
    for t in start..obs.len() {
        for (i, p) in preds.iter().enumerate() {
            let (options, pred) = predict_at(p.as_ref(), obs, t);
            points[i].push(match pred {
                Pred::Dist(probs) => Some(Point { probs, actual: option_index(&options, &obs[t].choice) }),
                Pred::Insufficient { .. } => None,
            });
        }
    }
    Backtest { start, ids: preds.iter().map(|p| p.id()).collect(), points }
}

impl Backtest {
    pub fn index(&self, id: &str) -> Option<usize> {
        self.ids.iter().position(|x| *x == id)
    }
    /// All forecasts a predictor made.
    pub fn answered(&self, i: usize) -> Vec<Point> {
        self.points[i].iter().flatten().cloned().collect()
    }
    /// Share of origins where the predictor answered.
    pub fn coverage(&self, i: usize) -> f64 {
        let n = self.points[i].len();
        if n == 0 {
            return 0.0;
        }
        self.points[i].iter().filter(|p| p.is_some()).count() as f64 / n as f64
    }
    /// Forecasts of `a` and `b` at the origins where both answered.
    pub fn common(&self, a: usize, b: usize) -> (Vec<Point>, Vec<Point>) {
        let (mut xa, mut xb) = (Vec::new(), Vec::new());
        for (pa, pb) in self.points[a].iter().zip(&self.points[b]) {
            if let (Some(pa), Some(pb)) = (pa, pb) {
                xa.push(pa.clone());
                xb.push(pb.clone());
            }
        }
        (xa, xb)
    }
}

/// Leakage check. `f(seq, u)` is how a system produces the forecast for origin
/// `u` from the sequence it was handed. Replace everything after index `t`
/// with a deterministic shuffle of itself (contexts and choices rotated across
/// positions) and return true only if every forecast at origins `u <= t` is
/// unchanged. Origin `t` itself predicts `seq[t]` from `seq[..t]`, so it must
/// not change either.
pub fn leakage_check(obs: &[Obs], t: usize, seed: u64, f: &dyn Fn(&[Obs], usize) -> Option<Vec<f64>>) -> bool {
    let mut altered = obs.to_vec();
    let tail = &mut altered[(t + 1).min(obs.len())..];
    // deterministic Fisher-Yates on the future, and flip every choice to the next one for good measure
    let mut s = seed | 1;
    for i in (1..tail.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        tail.swap(i, (s % (i as u64 + 1)) as usize);
    }
    let choices: Vec<String> = obs.iter().map(|o| o.choice.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    for o in tail.iter_mut() {
        let k = choices.iter().position(|c| *c == o.choice).unwrap_or(0);
        o.choice = choices[(k + 1) % choices.len()].clone();
    }
    (0..=t.min(obs.len().saturating_sub(1))).all(|u| f(obs, u) == f(&altered, u))
}

/// `leakage_check` for a real predictor, going through `predict_at`.
pub fn predictor_is_leak_free(p: &dyn Predictor, obs: &[Obs], t: usize, seed: u64) -> bool {
    leakage_check(obs, t, seed, &|seq, u| match predict_at(p, seq, u).1 {
        Pred::Dist(d) => Some(d),
        Pred::Insufficient { .. } => None,
    })
}
