//! Track L2, the pure part: a hierarchical (empirical-Bayes) prior over what earlier families taught.
//!
//! Two quantities are learned across families, each a pure function of counts the being folds from its own
//! log, and each citing the families it came from:
//!
//! * **Form posterior.** Over the sequence families met before (closed, or open with a family form), a
//!   Dirichlet prior over rule forms: `pi(f) = (n_f + A) / (N + A (L + 1))`, where `n_f` counts the earlier
//!   families whose rule had form `f`, `L` the forms seen, and the extra category "other" (a form not seen, or
//!   no learnable rule: the families that closed without a rule, plus `A`). With `k` revealed answers of a new
//!   family that form `f` reproduces and never contradicts, the posterior that `f` is the family's rule is
//!   `pi(f) / (sum of pi(g) over the seen forms g consistent with the same answers + pi(other) * eps^k)`,
//!   where `eps` is the chance that a form that is NOT a family's rule still reproduces one of its revealed
//!   answers, estimated (Beta(1, 1)) from the being's own closed families: every seen form tried on every
//!   example of a family whose rule was another form. A form is accepted for the whole family when that
//!   posterior is at least `1 - delta` and `k >= min_ex`. Early in life `N` is small, "other" carries much
//!   mass and `eps` is uncertain (about 1/2), so many answers are needed (the plain 3 + 1 of B2 is then the
//!   binding rule); as families accumulate both sharpen and fewer answers suffice. That is the mechanism by
//!   which later families can be learned from fewer attempts.
//! * **Rule trust.** For `discovered` on an offer a live rule covers, the being's success estimate starts from
//!   an empirical-Bayes Beta prior over the post-rule success shares of the earlier families' rules (method of
//!   moments, weight between `TRUST_K_MIN` and `TRUST_K_MAX`, shrunk by the uncertainty of the mean when few
//!   families are known), in place of the strategy-wide pool of every `discovered` attempt (with and without a
//!   rule) that the B1 model uses.

/// Dirichlet pseudo-count per category.
pub const A: f64 = 1.0;
/// Families (with post-rule records of at least `TRUST_MIN_N` attempts) needed for a trust prior.
pub const TRUST_MIN_FAMS: usize = 3;
pub const TRUST_MIN_N: u64 = 3;
pub const TRUST_K_MIN: f64 = 2.0;
pub const TRUST_K_MAX: f64 = 40.0;

/// The prior over forms, from the earlier families.
#[derive(Debug, Clone, PartialEq)]
pub struct FormPrior {
    /// (form code, families with it), most families first.
    pub forms: Vec<(String, u64)>,
    /// Families that closed without a rule.
    pub none: u64,
    /// Chance-agreement counts (applicable, agreed).
    pub applicable: u64,
    pub agreed: u64,
}

impl FormPrior {
    pub fn n(&self) -> u64 {
        self.forms.iter().map(|f| f.1).sum::<u64>() + self.none
    }
    fn denom(&self) -> f64 {
        self.n() as f64 + A * (self.forms.len() as f64 + 1.0)
    }
    pub fn pi(&self, code: &str) -> f64 {
        let n = self.forms.iter().find(|f| f.0 == code).map_or(0, |f| f.1);
        if n == 0 {
            return 0.0;
        }
        (n as f64 + A) / self.denom()
    }
    pub fn pi_other(&self) -> f64 {
        (self.none as f64 + A) / self.denom()
    }
    /// Chance that a form that is not the family's rule reproduces one revealed answer (Beta(1, 1) mean).
    pub fn eps(&self) -> f64 {
        (self.agreed as f64 + 1.0) / (self.applicable as f64 + 2.0)
    }
    /// Posterior that `code` is the family's rule, given that it reproduces `k` answers and never contradicts
    /// one, and `rivals` (other seen forms) do the same.
    pub fn posterior(&self, code: &str, k: u32, rivals: &[String]) -> f64 {
        let p = self.pi(code);
        if p <= 0.0 {
            return 0.0;
        }
        let rv: f64 = rivals.iter().filter(|r| r.as_str() != code).map(|r| self.pi(r)).sum();
        p / (p + rv + self.pi_other() * self.eps().powi(k as i32))
    }
    /// Smallest number of reproduced answers with which `code` (no rivals) would be accepted at `delta`.
    pub fn needed(&self, code: &str, delta: f64, min_ex: u32) -> Option<u32> {
        (min_ex.max(1)..=12).find(|k| self.posterior(code, *k, &[]) >= 1.0 - delta)
    }
}

/// Empirical-Bayes Beta prior (mean, weight) over per-family (successes, attempts).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trust {
    pub mean: f64,
    pub k: f64,
    pub fams: usize,
}

pub fn trust_prior(xs: &[(u64, u64)]) -> Option<Trust> {
    let xs: Vec<(u64, u64)> = xs.iter().copied().filter(|x| x.1 >= TRUST_MIN_N).collect();
    if xs.len() < TRUST_MIN_FAMS {
        return None;
    }
    let rates: Vec<f64> = xs.iter().map(|(s, n)| (*s as f64 + 0.5) / (*n as f64 + 1.0)).collect();
    let nf = rates.len() as f64;
    let m = rates.iter().sum::<f64>() / nf;
    let var = rates.iter().map(|r| (r - m).powi(2)).sum::<f64>() / (nf - 1.0);
    let samp = xs.iter().map(|(_, n)| m * (1.0 - m) / (*n as f64 + 1.0)).sum::<f64>() / nf;
    let between = (var - samp).max(1e-4);
    // the variance of a new family's share around the estimated mean includes the mean's own uncertainty
    let pred = between + var / nf;
    let k = (m * (1.0 - m) / pred - 1.0).clamp(TRUST_K_MIN, TRUST_K_MAX);
    Some(Trust { mean: m.clamp(0.01, 0.99), k, fams: xs.len() })
}

/// Beta posterior of a cell with `succ` of `n` under the trust prior: (p, sd).
pub fn trust_estimate(t: &Trust, succ: u64, n: u64) -> (f64, f64) {
    let a = succ as f64 + t.k * t.mean;
    let b = (n - succ) as f64 + t.k * (1.0 - t.mean);
    let p = a / (a + b);
    (p, (p * (1.0 - p) / (a + b + 1.0)).sqrt())
}
