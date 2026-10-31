//! Baseline predictors: uniform, frequency, last-choice, recency-weighted,
//! context-count with smoothing. Each is a few lines on purpose: they are the
//! yardsticks any fancier predictor has to beat.

use super::{option_index, Obs, Pred, Predictor};

/// Tunable constants. The defaults were chosen on seeds 101..120 only
/// (see docs/measured.md, "Prediction").
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Additive smoothing per option for frequency and recency.
    pub alpha: f64,
    /// Recency half-life in observations.
    pub half_life: f64,
    /// Mass the last-choice predictor spreads over the other options.
    pub last_eps: f64,
    /// Pseudo-count of the global frequency mixed into a context's counts.
    pub ctx_m: f64,
    /// Minimum observations before frequency, last-choice and recency answer.
    pub min_n: usize,
    /// Minimum observations IN THE CONTEXT before context-count answers.
    pub min_ctx_n: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config { alpha: 0.5, half_life: 30.0, last_eps: 0.2, ctx_m: 2.0, min_n: 10, min_ctx_n: 5 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Uniform,
    Frequency,
    LastChoice,
    Recency,
    ContextCount,
}

pub const KINDS: [Kind; 5] = [Kind::Uniform, Kind::Frequency, Kind::LastChoice, Kind::Recency, Kind::ContextCount];

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Uniform => "uniform",
            Kind::Frequency => "frequency",
            Kind::LastChoice => "last-choice",
            Kind::Recency => "recency",
            Kind::ContextCount => "context-count",
        }
    }
    pub fn from_id(s: &str) -> Option<Kind> {
        KINDS.iter().copied().find(|k| k.id() == s)
    }
}

#[derive(Debug, Clone)]
pub struct Baseline {
    pub kind: Kind,
    pub cfg: Config,
}

pub fn make(kind: Kind, cfg: &Config) -> Box<dyn Predictor> {
    Box::new(Baseline { kind, cfg: cfg.clone() })
}

pub fn all(cfg: &Config) -> Vec<Box<dyn Predictor>> {
    KINDS.iter().map(|k| make(*k, cfg)).collect()
}

fn normalise(mut v: Vec<f64>) -> Vec<f64> {
    let s: f64 = v.iter().sum();
    for x in &mut v {
        *x /= s;
    }
    v
}

/// Smoothed (optionally decayed) frequency over all of `hist`.
fn freq(hist: &[Obs], options: &[String], alpha: f64, half_life: Option<f64>) -> Vec<f64> {
    let mut c = vec![alpha; options.len()];
    let last = hist.len().saturating_sub(1);
    for (i, o) in hist.iter().enumerate() {
        let w = match half_life {
            Some(h) => 0.5f64.powf((last - i) as f64 / h),
            None => 1.0,
        };
        c[option_index(options, &o.choice)] += w;
    }
    normalise(c)
}

impl Predictor for Baseline {
    fn id(&self) -> &'static str {
        self.kind.id()
    }
    fn version(&self) -> String {
        let c = &self.cfg;
        match self.kind {
            Kind::Uniform => "v1".into(),
            Kind::Frequency => format!("v1 a={} min_n={}", c.alpha, c.min_n),
            Kind::LastChoice => format!("v1 eps={} min_n={}", c.last_eps, c.min_n),
            Kind::Recency => format!("v1 a={} half_life={} min_n={}", c.alpha, c.half_life, c.min_n),
            Kind::ContextCount => format!("v1 a={} m={} min_ctx_n={}", c.alpha, c.ctx_m, c.min_ctx_n),
        }
    }
    fn predict(&self, hist: &[Obs], ctx: &str, options: &[String]) -> Pred {
        let k = options.len();
        let c = &self.cfg;
        let short = |n: usize, min_n: usize| if n < min_n { Some(Pred::Insufficient { n, min_n }) } else { None };
        match self.kind {
            Kind::Uniform => Pred::Dist(vec![1.0 / k as f64; k]),
            Kind::Frequency => short(hist.len(), c.min_n).unwrap_or_else(|| Pred::Dist(freq(hist, options, c.alpha, None))),
            Kind::Recency => short(hist.len(), c.min_n).unwrap_or_else(|| Pred::Dist(freq(hist, options, c.alpha, Some(c.half_life)))),
            Kind::LastChoice => short(hist.len(), c.min_n.max(1)).unwrap_or_else(|| {
                if k == 1 {
                    return Pred::Dist(vec![1.0]);
                }
                let li = option_index(options, &hist[hist.len() - 1].choice);
                let mut p = vec![c.last_eps / (k - 1) as f64; k];
                p[li] = 1.0 - c.last_eps;
                Pred::Dist(p)
            }),
            Kind::ContextCount => {
                let in_ctx: Vec<&Obs> = hist.iter().filter(|o| o.ctx == ctx).collect();
                if let Some(r) = short(in_ctx.len(), c.min_ctx_n) {
                    return r;
                }
                let global = freq(hist, options, c.alpha, None);
                let mut counts = vec![0.0; k];
                for o in &in_ctx {
                    counts[option_index(options, &o.choice)] += 1.0;
                }
                let n = in_ctx.len() as f64;
                Pred::Dist(normalise((0..k).map(|j| (counts[j] + c.ctx_m * global[j]) / (n + c.ctx_m)).collect()))
            }
        }
    }
}
