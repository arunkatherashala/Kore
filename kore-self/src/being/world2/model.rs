//! The existing B1 world-model learner (`model.rs`, mode `Off`) with the domain index replaced by a
//! family index, so it can hold any number of families. Same formulas, same constants (imported,
//! not copied): per (family, difficulty, strategy) cell a Beta posterior whose prior is the
//! (family, strategy) estimate, shrunk towards the strategy-wide estimate from Beta(1,1); the energy
//! ratio shrunk the same way. A test feeds the same observations to this model and to `model::Model`
//! (families 0..7 as the seven domains) and checks that every estimate is identical bit for bit.
//!
//! Built only from logged outcomes as REPORTED to the being (with feedback noise, these can be
//! wrong). It never reads a hidden parameter; a test scans this file.

use crate::being::model::{Cell, Estimate, COST_K, PRIOR_K};
use crate::being::solver::Strategy;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tot {
    n: f64,
    succ: f64,
    spent: f64,
    listed: f64,
}

impl Tot {
    fn add(&mut self, c: &Cell) {
        self.n += c.n as f64;
        self.succ += c.succ as f64;
        self.spent += c.spent as f64;
        self.listed += c.listed as f64;
    }
}

fn shrink(s: f64, n: f64, prior: f64, k: f64) -> f64 {
    (s + k * prior) / (n + k)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FamModel {
    cells: Vec<[[Cell; 4]; 5]>,
}

impl FamModel {
    pub fn new(families: usize) -> FamModel {
        FamModel { cells: vec![[[Cell::default(); 4]; 5]; families] }
    }

    pub fn families(&self) -> usize {
        self.cells.len()
    }

    fn grow(&mut self, f: usize) {
        if f >= self.cells.len() {
            self.cells.resize(f + 1, [[Cell::default(); 4]; 5]);
        }
    }

    /// One reported attempt.
    pub fn observe(&mut self, fam: usize, difficulty: u8, s: Strategy, correct: bool, listed: i64, spent: i64) {
        self.grow(fam);
        let c = &mut self.cells[fam][(difficulty.clamp(1, 5) - 1) as usize][s.idx()];
        c.n += 1;
        c.succ += correct as u64;
        c.spent += spent;
        c.listed += listed;
    }

    /// Forget one cell (a new rule changes what `discovered` does there), as `Model::reset_cell`.
    pub fn reset_cell(&mut self, fam: usize, difficulty: u8, s: Strategy) {
        self.grow(fam);
        self.cells[fam][(difficulty.clamp(1, 5) - 1) as usize][s.idx()] = Cell::default();
    }

    pub fn attempts(&self) -> u64 {
        self.cells.iter().flatten().flatten().map(|c| c.n).sum()
    }

    pub fn cell(&self, fam: usize, difficulty: u8, s: Strategy) -> Cell {
        self.cells.get(fam).map(|x| x[(difficulty.clamp(1, 5) - 1) as usize][s.idx()]).unwrap_or_default()
    }

    /// The B1 estimate (`Model::estimate` in mode `Off`, without transfer links).
    pub fn estimate(&self, fam: usize, difficulty: u8, s: Strategy) -> Estimate {
        let k = (difficulty.clamp(1, 5) - 1) as usize;
        let mut st = Tot::default();
        for f in &self.cells {
            for kk in 0..5 {
                st.add(&f[kk][s.idx()]);
            }
        }
        let mut dt = Tot::default();
        let mut c = Tot::default();
        if let Some(f) = self.cells.get(fam) {
            for kk in 0..5 {
                dt.add(&f[kk][s.idx()]);
            }
            c.add(&f[k][s.idx()]);
        }
        let p_s = (st.succ + 1.0) / (st.n + 2.0);
        let r_s = shrink(st.spent, st.listed, 1.0, COST_K);
        let p_d = shrink(dt.succ, dt.n, p_s, PRIOR_K);
        let a = c.succ + PRIOR_K * p_d;
        let b = (c.n - c.succ) + PRIOR_K * (1.0 - p_d);
        let p = a / (a + b);
        let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
        let r_d = shrink(dt.spent, dt.listed, r_s, COST_K);
        let cost_ratio = shrink(c.spent, c.listed, r_d, COST_K);
        Estimate { n: c.n as u64, p, sd, cost_ratio, group_n: dt.n }
    }
}
