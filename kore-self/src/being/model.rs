//! The learned world model (Track B, step B1).
//!
//! Built ONLY from the being's logged outcomes: each attempt adds one
//! observation (domain, difficulty, strategy, right or wrong, listed cost,
//! energy actually spent). The model never sees the world's hidden parameters,
//! the generator or the expected answers; a test scans this file and
//! `planner.rs` for those names.
//!
//! Per (domain, difficulty, strategy) cell it estimates
//!   * the success probability, as a Beta posterior whose prior is the
//!     (domain, strategy) estimate, which in turn is shrunk towards the
//!     strategy-wide estimate, which starts from a flat Beta(1,1):
//!     `p_domain = (s_d + K p_strategy) / (n_d + K)`, and for the cell
//!     `Beta(s + K p_domain, f + K (1 - p_domain))`, with `K = PRIOR_K`;
//!   * its uncertainty, the posterior standard deviation (shrinks as 1/sqrt(n));
//!   * the energy actually spent as a ratio to the listed cost, shrunk the
//!     same way from a prior ratio of 1.
//!
//! Everything is integer counts plus a fixed order of float operations, so
//! folding the same log twice gives the same model bit for bit.

use super::solver::{Strategy, STRATEGIES};
use super::world::{Domain, DOMAINS};

/// Pseudo-observations the parent level contributes to a child level.
pub const PRIOR_K: f64 = 4.0;
/// Pseudo energy units (listed) behind the prior cost ratio of a level.
pub const COST_K: f64 = 4.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cell {
    pub n: u64,
    pub succ: u64,
    pub spent: i64,
    pub listed: i64,
}

impl Cell {
    fn add(&mut self, o: &Cell) {
        self.n += o.n;
        self.succ += o.succ;
        self.spent += o.spent;
        self.listed += o.listed;
    }
}

/// One logged attempt.
#[derive(Debug, Clone, Copy)]
pub struct Obs {
    pub domain: Domain,
    pub difficulty: u8,
    pub strategy: Strategy,
    pub correct: bool,
    pub listed_cost: i64,
    pub spent: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    /// Attempts observed in this exact cell.
    pub n: u64,
    pub p: f64,
    /// Posterior standard deviation of the success probability.
    pub sd: f64,
    /// Expected energy spent divided by the listed cost.
    pub cost_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    cells: [[[Cell; 3]; 5]; 4],
}

fn shrink(s: f64, n: f64, prior: f64, k: f64) -> f64 {
    (s + k * prior) / (n + k)
}

impl Model {
    pub fn observe(&mut self, o: &Obs) {
        let c = &mut self.cells[o.domain.idx()][(o.difficulty.clamp(1, 5) - 1) as usize][o.strategy.idx()];
        c.n += 1;
        c.succ += o.correct as u64;
        c.spent += o.spent;
        c.listed += o.listed_cost;
    }

    pub fn attempts(&self) -> u64 {
        self.cells.iter().flatten().flatten().map(|c| c.n).sum()
    }

    fn strategy_total(&self, s: Strategy) -> Cell {
        let mut t = Cell::default();
        for d in 0..4 {
            for k in 0..5 {
                t.add(&self.cells[d][k][s.idx()]);
            }
        }
        t
    }

    fn domain_total(&self, d: Domain, s: Strategy) -> Cell {
        let mut t = Cell::default();
        for k in 0..5 {
            t.add(&self.cells[d.idx()][k][s.idx()]);
        }
        t
    }

    pub fn estimate(&self, d: Domain, difficulty: u8, s: Strategy) -> Estimate {
        let st = self.strategy_total(s);
        let dt = self.domain_total(d, s);
        let c = self.cells[d.idx()][(difficulty.clamp(1, 5) - 1) as usize][s.idx()];
        // success probability
        let p_s = (st.succ as f64 + 1.0) / (st.n as f64 + 2.0);
        let p_d = shrink(dt.succ as f64, dt.n as f64, p_s, PRIOR_K);
        let a = c.succ as f64 + PRIOR_K * p_d;
        let b = (c.n - c.succ) as f64 + PRIOR_K * (1.0 - p_d);
        let p = a / (a + b);
        let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
        // cost ratio
        let r_s = shrink(st.spent as f64, st.listed as f64, 1.0, COST_K);
        let r_d = shrink(dt.spent as f64, dt.listed as f64, r_s, COST_K);
        let cost_ratio = shrink(c.spent as f64, c.listed as f64, r_d, COST_K);
        Estimate { n: c.n, p, sd, cost_ratio }
    }

    /// Per (domain, strategy) attempt counts, for status output.
    pub fn counts(&self) -> Vec<(Domain, Strategy, u64, u64)> {
        let mut v = vec![];
        for d in DOMAINS {
            for s in STRATEGIES {
                let t = self.domain_total(d, s);
                v.push((d, s, t.n, t.succ));
            }
        }
        v
    }
}
