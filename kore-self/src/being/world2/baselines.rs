//! Baselines (random, always-X, UCB1) and the two oracles of World v2.

use super::engine::State2;
use super::learner::{plain, Act2, Choice2, Chooser2, Offer2};
use super::solve::{self, slip_pm};
use super::world::{World2, SKIP_COST};
use crate::being::planner::SKIP_MIN_COST;
use crate::being::solver::{Strategy, STRATEGIES};
use crate::being::state::EFF_NORM;
use crate::being::world::Rng;
use std::collections::HashMap;

// ---- baselines ----

/// Uniform over affordable (offer, fixed strategy) pairs.
pub struct Random2 {
    rng: Rng,
}

impl Random2 {
    pub fn new(seed: u64) -> Random2 {
        Random2 { rng: Rng::new(seed ^ 0x2A4D_0C_02) }
    }
}

impl Chooser2 for Random2 {
    fn name(&self) -> String {
        "random".into()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let opts: Vec<(usize, Strategy)> = offers.iter().flat_map(|o| STRATEGIES.iter().filter(move |s| s.energy(o.listed_cost) <= st.budget_left).map(move |s| (o.slot, *s))).collect();
        if opts.is_empty() {
            return None;
        }
        let (slot, strategy) = opts[self.rng.below(opts.len() as u64) as usize];
        Some(plain("random", "baseline: uniform over affordable options", Act2::Attempt { slot, strategy }, 0.0, 0.0))
    }
}

/// Always one strategy, on the offer with the best listed reward per listed cost.
pub struct Always2 {
    pub strategy: Strategy,
}

impl Chooser2 for Always2 {
    fn name(&self) -> String {
        format!("always-{}", self.strategy.name())
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let mut best: Option<&Offer2> = None;
        for o in offers.iter().filter(|o| self.strategy.energy(o.listed_cost) <= st.budget_left) {
            if best.map_or(true, |b| o.listed_reward * b.listed_cost > b.listed_reward * o.listed_cost) {
                best = Some(o);
            }
        }
        let o = best?;
        Some(plain(&self.name(), "baseline: best listed reward per listed cost, fixed strategy", Act2::Attempt { slot: o.slot, strategy: self.strategy }, 0.0, 0.0))
    }
}

/// UCB1 over (family, strategy) arms of the three fixed strategies, on reported reward per energy
/// (normalised by `EFF_NORM` as the original UCB1 chooser). Untried arms first.
pub struct Ucb2 {
    pub c: f64,
}

impl Chooser2 for Ucb2 {
    fn name(&self) -> String {
        "ucb1-family-strategy".into()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let total = st.arms.iter().flatten().map(|a| a.n).sum::<u64>().max(1) as f64;
        let mut best: Option<(f64, f64, usize, Strategy)> = None;
        for o in offers {
            for &s in STRATEGIES.iter() {
                if s.energy(o.listed_cost) > st.budget_left {
                    continue;
                }
                let a = st.arm(o.fam, s);
                let score = if a.n == 0 {
                    f64::INFINITY
                } else {
                    let eff = if a.cost > 0 { a.reward as f64 / a.cost as f64 } else { 0.0 };
                    eff / EFF_NORM + self.c * (2.0 * total.ln() / a.n as f64).sqrt()
                };
                let ratio = o.listed_reward as f64 / s.energy(o.listed_cost) as f64;
                if best.map_or(true, |b| score > b.0 || (score == b.0 && ratio > b.1)) {
                    best = Some((score, ratio, o.slot, s));
                }
            }
        }
        let (score, _, slot, strategy) = best?;
        Some(plain("ucb1-family-strategy", "baseline: UCB1 over (family, strategy) arms", Act2::Attempt { slot, strategy }, 0.0, if score.is_finite() { score } else { 0.0 }))
    }
}

// ---- oracles ----

pub const TRUTH_SAMPLES: u64 = 200;
const TRUTH_SALT: u64 = 0x0AC1E;

/// Knows the hidden world: the true success probability of each fixed strategy for each
/// (family, difficulty) in the family's current regime (estimated once per regime from
/// `TRUTH_SAMPLES` fresh tasks). With `know_rules` it can also answer with the ground truth
/// itself (an upper bound for a being that had learned every hidden rule and constant).
/// It skips with the learner's rule (best expected reward per energy below its own expected rate so
/// far, after 40 energy), so skipping is not an advantage the learner has over it.
pub struct Oracle2 {
    pub world: World2,
    pub know_rules: bool,
    pub skip: bool,
    cache: HashMap<(usize, u8, usize, u64, u64), f64>,
    exp_reward: f64,
    spent: f64,
}

impl Oracle2 {
    pub fn new(world: World2, know_rules: bool) -> Oracle2 {
        Oracle2 { world, know_rules, skip: true, cache: HashMap::new(), exp_reward: 0.0, spent: 0.0 }
    }

    /// True success probability of fixed strategy `s` on (family, difficulty) at `clock` (no learned rules).
    pub fn truth(&mut self, fam: usize, d: u8, s: Strategy, clock: i64) -> f64 {
        let w = &self.world;
        let base = w.fams[fam].base.unwrap_or(fam);
        let key = (fam, d, s.idx(), w.regime_index(fam, clock), w.regime_index(base, clock));
        if let Some(p) = self.cache.get(&key) {
            return *p;
        }
        let mut ok = 0u64;
        for i in 0..TRUTH_SAMPLES {
            let t = w.task(1_000_000 + i, 0, fam, Some(d), clock, TRUTH_SALT);
            ok += t.check(solve::attempt(w, &t, s, &[]).answer) as u64;
        }
        let p = ok as f64 / TRUTH_SAMPLES as f64;
        self.cache.insert(key, p);
        p
    }
}

impl Chooser2 for Oracle2 {
    fn name(&self) -> String {
        if self.know_rules { "oracle-knows-rules".into() } else { "oracle-planner".into() }
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let mut best: Option<(f64, Act2, f64)> = None;
        for o in offers {
            for &s in STRATEGIES.iter() {
                let e = s.energy(o.listed_cost);
                if e > st.budget_left {
                    continue;
                }
                let p = self.truth(o.fam, o.difficulty, s, st.clock);
                let v = o.listed_reward as f64 * p / e as f64;
                if best.as_ref().map_or(true, |b| v > b.0) {
                    best = Some((v, Act2::Attempt { slot: o.slot, strategy: s }, p));
                }
            }
            if self.know_rules && o.listed_cost <= st.budget_left {
                let p = 1.0 - slip_pm(Strategy::Careful, self.world.fams[o.fam].slip_pm) as f64 / 1000.0;
                let v = o.listed_reward as f64 * p / o.listed_cost as f64;
                if best.as_ref().map_or(true, |b| v > b.0) {
                    best = Some((v, Act2::Oracle { slot: o.slot }, p));
                }
            }
        }
        let (v, act, p) = best?;
        let rate = if self.spent > 0.0 { self.exp_reward / self.spent } else { 0.0 };
        if self.skip && self.spent >= SKIP_MIN_COST as f64 && v < rate && st.budget_left >= SKIP_COST {
            self.spent += SKIP_COST as f64;
            return Some(plain(&self.name(), "knows hidden truth: best expected value below its expected rate, skip", Act2::Skip, 0.0, 0.0));
        }
        let (reward, energy) = match act {
            Act2::Attempt { slot, strategy } => {
                let o = offers.iter().find(|o| o.slot == slot).expect("an offer");
                (o.listed_reward as f64, strategy.energy(o.listed_cost) as f64)
            }
            Act2::Oracle { slot } => {
                let o = offers.iter().find(|o| o.slot == slot).expect("an offer");
                (o.listed_reward as f64, o.listed_cost as f64)
            }
            Act2::Skip => (0.0, SKIP_COST as f64),
        };
        self.exp_reward += reward * p;
        self.spent += energy;
        Some(plain(&self.name(), "knows hidden truth", act, p, v))
    }
}
