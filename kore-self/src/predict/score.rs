//! Scoring: Brier (multi-class, 0 best, 2 worst), log loss (nats), accuracy,
//! calibration bins with expected calibration error (ECE, on the top label),
//! moving-block bootstrap 95% intervals and paired comparisons.
//!
//! Intervals are percentile intervals from a seeded bootstrap over blocks of
//! consecutive points (default block 10), which respects some serial
//! dependence but is still approximate. They say how much the measured mean
//! could move with a different sample of the same process, nothing more.

/// One scored forecast: probabilities over the options and the index that happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub probs: Vec<f64>,
    pub actual: usize,
}

pub const LOG_FLOOR: f64 = 1e-12;
pub const BOOT_ITERS: usize = 2000;
pub const BLOCK: usize = 10;
/// A paired comparison below this many common points gives no verdict.
pub const MIN_PAIRED_N: usize = 30;
pub const BINS: usize = 10;

pub fn brier(p: &Point) -> f64 {
    p.probs.iter().enumerate().map(|(j, x)| (x - if j == p.actual { 1.0 } else { 0.0 }).powi(2)).sum()
}

pub fn log_loss(p: &Point) -> f64 {
    -(p.probs[p.actual].max(LOG_FLOOR)).ln()
}

/// Index of the largest probability (first one on ties) and that probability.
pub fn top(p: &Point) -> (usize, f64) {
    let mut best = (0, p.probs[0]);
    for (j, x) in p.probs.iter().enumerate() {
        if *x > best.1 {
            best = (j, *x);
        }
    }
    best
}

pub fn correct(p: &Point) -> bool {
    top(p).0 == p.actual
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub brier: f64,
    pub log_loss: f64,
    pub accuracy: f64,
    pub ece: f64,
}

pub fn summarize(pts: &[Point]) -> Summary {
    let n = pts.len();
    if n == 0 {
        return Summary { n: 0, brier: f64::NAN, log_loss: f64::NAN, accuracy: f64::NAN, ece: f64::NAN };
    }
    let m = |f: &dyn Fn(&Point) -> f64| pts.iter().map(f).sum::<f64>() / n as f64;
    Summary { n, brier: m(&brier), log_loss: m(&log_loss), accuracy: m(&|p| correct(p) as u8 as f64), ece: ece(&calibration(pts, BINS), n) }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bin {
    pub lo: f64,
    pub hi: f64,
    pub n: usize,
    pub mean_conf: f64,
    pub accuracy: f64,
}

/// Top-label calibration: forecasts grouped by their highest probability into
/// `bins` equal-width bins; per bin the mean confidence and the hit rate.
pub fn calibration(pts: &[Point], bins: usize) -> Vec<Bin> {
    let mut acc: Vec<(usize, f64, usize)> = vec![(0, 0.0, 0); bins];
    for p in pts {
        let (_, c) = top(p);
        let b = ((c * bins as f64) as usize).min(bins - 1);
        acc[b].0 += 1;
        acc[b].1 += c;
        acc[b].2 += correct(p) as usize;
    }
    acc.iter()
        .enumerate()
        .map(|(i, (n, sc, sh))| Bin {
            lo: i as f64 / bins as f64,
            hi: (i + 1) as f64 / bins as f64,
            n: *n,
            mean_conf: if *n > 0 { sc / *n as f64 } else { f64::NAN },
            accuracy: if *n > 0 { *sh as f64 / *n as f64 } else { f64::NAN },
        })
        .collect()
}

pub fn ece(bins: &[Bin], n: usize) -> f64 {
    if n == 0 {
        return f64::NAN;
    }
    bins.iter().filter(|b| b.n > 0).map(|b| b.n as f64 / n as f64 * (b.accuracy - b.mean_conf).abs()).sum()
}

/// A mean with its bootstrap 95% interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ci {
    pub mean: f64,
    pub lo: f64,
    pub hi: f64,
}

impl Ci {
    pub fn excludes_zero(&self) -> bool {
        self.lo > 0.0 || self.hi < 0.0
    }
}

/// splitmix64, private so this module has no dependency on the generator.
pub struct Sm(pub u64);
impl Sm {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Moving-block bootstrap of the mean of `vals` (time order matters). With
/// fewer than 2 values the interval is the mean itself (no claim possible).
pub fn bootstrap_mean(vals: &[f64], iters: usize, block: usize, seed: u64) -> Ci {
    let n = vals.len();
    if n == 0 {
        return Ci { mean: f64::NAN, lo: f64::NAN, hi: f64::NAN };
    }
    let mean = vals.iter().sum::<f64>() / n as f64;
    if n < 2 {
        return Ci { mean, lo: mean, hi: mean };
    }
    let b = block.clamp(1, n);
    let nb = n.div_ceil(b);
    let mut rng = Sm(seed ^ (n as u64).wrapping_mul(0xA24B_AED4_963E_E407));
    let mut means = Vec::with_capacity(iters);
    for _ in 0..iters {
        let mut s = 0.0;
        let mut cnt = 0usize;
        for _ in 0..nb {
            let start = (rng.next() % n as u64) as usize;
            for k in 0..b {
                if cnt == n {
                    break;
                }
                s += vals[(start + k) % n];
                cnt += 1;
            }
        }
        means.push(s / cnt as f64);
    }
    means.sort_by(|a, c| a.partial_cmp(c).unwrap());
    let q = |f: f64| means[((f * (iters - 1) as f64).round() as usize).min(iters - 1)];
    Ci { mean, lo: q(0.025), hi: q(0.975) }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SummaryCi {
    pub n: usize,
    pub brier: Ci,
    pub log_loss: Ci,
    pub accuracy: Ci,
    pub ece: f64,
}

pub fn summarize_ci(pts: &[Point], seed: u64) -> SummaryCi {
    let b: Vec<f64> = pts.iter().map(brier).collect();
    let l: Vec<f64> = pts.iter().map(log_loss).collect();
    let a: Vec<f64> = pts.iter().map(|p| correct(p) as u8 as f64).collect();
    SummaryCi {
        n: pts.len(),
        brier: bootstrap_mean(&b, BOOT_ITERS, BLOCK, seed),
        log_loss: bootstrap_mean(&l, BOOT_ITERS, BLOCK, seed.wrapping_add(1)),
        accuracy: bootstrap_mean(&a, BOOT_ITERS, BLOCK, seed.wrapping_add(2)),
        ece: ece(&calibration(pts, BINS), pts.len()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Fewer than `MIN_PAIRED_N` common points: no verdict, nothing is claimed.
    InsufficientData,
    /// Brier interval of (baseline - predictor) is above zero.
    Better,
    /// Brier interval is below zero.
    Worse,
    /// The interval contains zero.
    NoDifference,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::InsufficientData => "insufficient_data",
            Verdict::Better => "better",
            Verdict::Worse => "worse",
            Verdict::NoDifference => "no_significant_difference",
        }
    }
}

/// Paired comparison on common forecasts. Differences are baseline minus
/// predictor, so positive means the predictor is better. The verdict uses the
/// Brier interval only; log loss is reported next to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Paired {
    pub n: usize,
    pub d_brier: Ci,
    pub d_log_loss: Ci,
    /// Common forecasts where the predictor's Brier was strictly lower.
    pub wins: usize,
    pub verdict: Verdict,
}

pub fn paired(pred: &[Point], base: &[Point], seed: u64) -> Paired {
    assert_eq!(pred.len(), base.len(), "paired comparison needs aligned forecasts");
    let db: Vec<f64> = pred.iter().zip(base).map(|(p, b)| brier(b) - brier(p)).collect();
    let dl: Vec<f64> = pred.iter().zip(base).map(|(p, b)| log_loss(b) - log_loss(p)).collect();
    let d_brier = bootstrap_mean(&db, BOOT_ITERS, BLOCK, seed);
    let d_log_loss = bootstrap_mean(&dl, BOOT_ITERS, BLOCK, seed.wrapping_add(1));
    let n = db.len();
    let verdict = if n < MIN_PAIRED_N {
        Verdict::InsufficientData
    } else if d_brier.lo > 0.0 {
        Verdict::Better
    } else if d_brier.hi < 0.0 {
        Verdict::Worse
    } else {
        Verdict::NoDifference
    };
    Paired { n, d_brier, d_log_loss, wins: db.iter().filter(|x| **x > 0.0).count(), verdict }
}

/// Interval for the mean of per-run values (for example one number per seed): plain bootstrap, block 1.
pub fn mean_ci(vals: &[f64], seed: u64) -> Ci {
    bootstrap_mean(vals, BOOT_ITERS, 1, seed)
}
