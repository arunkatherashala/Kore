//! Self-directed choice of the domain (stage S2).
//!
//! The learned chooser is UCB1 over the four domains. The value of a domain is
//! the reward points it has paid per energy unit spent (ratio of sums, divided
//! by `EFF_NORM`), the bonus is `c * sqrt(2 ln N / n)` with N all completed
//! steps and n the steps in that domain. Domains never tried are tried first,
//! in an order shuffled by the loop seed. Ties go to the earlier domain in
//! that order. There is no hidden state: the next decision is a function of
//! the genesis event (seed, constant) and the outcome events in the log, which
//! is what the replay tests check.
//!
//! Baseline pickers for the measurement live here too: random (seeded),
//! always one domain, and the oracle (which is told the best domain).

use super::state::BeingState;
use super::world::{Domain, Rng, DOMAINS};
use serde_json::{json, Value};

/// Exploration constant used when a new being is created; tuned only on the
/// tuning seeds 101..120 (see docs/measured.md).
pub const DEFAULT_EXPLORE_C: f64 = 0.05;

#[derive(Debug, Clone, PartialEq)]
pub struct Reason {
    pub domain: Domain,
    pub n: u64,
    /// Reward points per energy unit so far.
    pub mean: f64,
    pub bonus: f64,
    pub score: f64,
    pub affordable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub domain: Domain,
    pub policy: String,
    /// "untried" (initial exploration), "highest score", "baseline" ...
    pub why: String,
    pub reasons: Vec<Reason>,
    pub total_pulls: u64,
}

impl Pick {
    pub fn chosen(&self) -> Option<&Reason> {
        self.reasons.iter().find(|r| r.domain == self.domain)
    }
    pub fn to_json(&self) -> Value {
        json!({
            "policy": self.policy, "chosen": self.domain.name(), "why": self.why, "total_pulls": self.total_pulls,
            "alternatives": self.reasons.iter().map(|r| json!({
                "domain": r.domain.name(), "n": r.n, "mean_reward_per_energy": r.mean,
                "bonus": r.bonus, "score": r.score, "affordable": r.affordable,
            })).collect::<Vec<_>>(),
        })
    }
}

pub trait DomainPicker {
    fn name(&self) -> String;
    /// `affordable` is never empty.
    fn pick(&mut self, st: &BeingState, affordable: &[Domain]) -> Pick;
}

fn reasons_for(st: &BeingState, affordable: &[Domain], c: f64) -> Vec<Reason> {
    let total = st.total_pulls().max(1) as f64;
    DOMAINS
        .iter()
        .map(|d| {
            let s = &st.stats[d.idx()];
            let (bonus, score) = if s.n == 0 {
                (f64::INFINITY, f64::INFINITY)
            } else {
                let b = c * (2.0 * total.ln() / s.n as f64).sqrt();
                (b, s.value() + b)
            };
            Reason { domain: *d, n: s.n, mean: s.efficiency(), bonus, score, affordable: affordable.contains(d) }
        })
        .collect()
}

/// The initial exploration order for a loop seed.
pub fn exploration_order(loop_seed: u64) -> [Domain; 4] {
    let mut order = DOMAINS;
    let mut r = Rng::new(loop_seed ^ 0xC40_05E);
    for i in (1..4).rev() {
        let j = r.below(i as u64 + 1) as usize;
        order.swap(i, j);
    }
    order
}

#[derive(Debug, Clone)]
pub struct Ucb1 {
    pub c: f64,
    pub order: [Domain; 4],
}

impl Ucb1 {
    pub fn new(loop_seed: u64, c: f64) -> Ucb1 {
        Ucb1 { c, order: exploration_order(loop_seed) }
    }
}

impl DomainPicker for Ucb1 {
    fn name(&self) -> String {
        "ucb1".into()
    }
    fn pick(&mut self, st: &BeingState, affordable: &[Domain]) -> Pick {
        let reasons = reasons_for(st, affordable, self.c);
        let total_pulls = st.total_pulls();
        // untried first, in the shuffled order
        if let Some(d) = self.order.iter().copied().find(|d| affordable.contains(d) && st.stats[d.idx()].n == 0) {
            return Pick { domain: d, policy: self.name(), why: "untried: initial exploration in seeded order".into(), reasons, total_pulls };
        }
        let mut best: Option<(f64, usize, Domain)> = None;
        for (pos, d) in self.order.iter().enumerate() {
            if !affordable.contains(d) {
                continue;
            }
            let sc = reasons[d.idx()].score;
            if best.map_or(true, |(b, _, _)| sc > b) {
                best = Some((sc, pos, *d));
            }
        }
        let d = best.map(|b| b.2).unwrap_or(affordable[0]);
        Pick { domain: d, policy: self.name(), why: "highest score (mean + exploration bonus) among affordable domains".into(), reasons, total_pulls }
    }
}

/// Baselines. They record the same reasons table so logs look alike, but the
/// choice does not use it.
pub struct RandomPicker {
    rng: Rng,
}
impl RandomPicker {
    pub fn new(seed: u64) -> RandomPicker {
        RandomPicker { rng: Rng::new(seed ^ 0x2A4D_0B) }
    }
}
impl DomainPicker for RandomPicker {
    fn name(&self) -> String {
        "random".into()
    }
    fn pick(&mut self, st: &BeingState, affordable: &[Domain]) -> Pick {
        let d = affordable[self.rng.below(affordable.len() as u64) as usize];
        Pick { domain: d, policy: self.name(), why: "baseline: uniform over affordable domains".into(), reasons: reasons_for(st, affordable, 0.0), total_pulls: st.total_pulls() }
    }
}

/// Always `domain` while affordable, else the first affordable one.
pub struct FixedPicker {
    pub domain: Domain,
    pub label: String,
}
impl FixedPicker {
    pub fn always(domain: Domain) -> FixedPicker {
        FixedPicker { domain, label: format!("always-{}", domain.name()) }
    }
    pub fn oracle(domain: Domain) -> FixedPicker {
        FixedPicker { domain, label: "oracle".into() }
    }
}
impl DomainPicker for FixedPicker {
    fn name(&self) -> String {
        self.label.clone()
    }
    fn pick(&mut self, st: &BeingState, affordable: &[Domain]) -> Pick {
        let d = if affordable.contains(&self.domain) { self.domain } else { affordable[0] };
        Pick { domain: d, policy: self.name(), why: "baseline: fixed domain".into(), reasons: reasons_for(st, affordable, 0.0), total_pulls: st.total_pulls() }
    }
}
