//! The existing B1 world-model learner (`model.rs`, mode `Off`) with the domain index replaced by a
//! family index, so it can hold any number of families. Same formulas, same constants (imported,
//! not copied): per (family, difficulty, strategy) cell a Beta posterior whose prior is the
//! (family, strategy) estimate, shrunk towards the strategy-wide estimate from Beta(1,1); the energy
//! ratio shrunk the same way. A test feeds the same observations to this model and to `model::Model`
//! (families 0..7 as the seven domains) and checks that every estimate is identical bit for bit.
//!
//! Built only from logged outcomes as REPORTED to the being (with feedback noise, these can be
//! wrong). It never reads a hidden parameter; a test scans this file.

use crate::being::adapt_b3b::{CellSw, SwitchCfg};
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
    /// Track B10 (opt-in): the B3b mechanisms (`adapt_b3b.rs`) on this model. None: off, and the model is
    /// exactly the B1 model above (the fields below stay empty).
    sw_cfg: Option<SwitchCfg>,
    /// Evidence the estimate uses when B3b is on (the raw counts after M1 switches replaced a cell's evidence by its window).
    ev: Vec<[[Cell; 4]; 5]>,
    sw: Vec<CellSw>,
    /// Every change the B3b mechanisms made, in order.
    pub changes: Vec<FamChange>,
}

/// One change of the B3b mechanisms in World v2.
#[derive(Debug, Clone, PartialEq)]
pub struct FamChange {
    /// `cell-switch` (M1: the evidence was replaced by the recent window) or `prior-conflict` (M2: the prior weight was cut).
    pub kind: &'static str,
    pub fam: usize,
    pub difficulty: u8,
    pub strategy: Strategy,
    pub drop: bool,
    pub statistic: f64,
    pub n: u32,
    pub succ: u32,
    pub predicted: f64,
}

impl FamModel {
    pub fn new(families: usize) -> FamModel {
        FamModel { cells: vec![[[Cell::default(); 4]; 5]; families], ..Default::default() }
    }

    pub fn families(&self) -> usize {
        self.cells.len()
    }

    /// B10: a model with the B3b mechanisms on (`cfg`), as `Model` in mode `Switch`.
    pub fn with_switch(families: usize, cfg: SwitchCfg) -> FamModel {
        let mut m = FamModel::new(families);
        m.sw_cfg = Some(cfg);
        m.ev = m.cells.clone();
        m.sw = vec![CellSw::new(); families * 20];
        m
    }

    pub fn switch_cfg(&self) -> Option<SwitchCfg> {
        self.sw_cfg
    }

    fn grow(&mut self, f: usize) {
        if f >= self.cells.len() {
            self.cells.resize(f + 1, [[Cell::default(); 4]; 5]);
            if self.sw_cfg.is_some() {
                self.ev.resize(f + 1, [[Cell::default(); 4]; 5]);
                self.sw.resize((f + 1) * 20, CellSw::new());
            }
        }
    }

    fn sw_idx(fam: usize, k: usize, s: usize) -> usize {
        fam * 20 + k * 4 + s
    }

    /// One reported attempt.
    pub fn observe(&mut self, fam: usize, difficulty: u8, s: Strategy, correct: bool, listed: i64, spent: i64) {
        self.grow(fam);
        let k = (difficulty.clamp(1, 5) - 1) as usize;
        if let Some(cfg) = self.sw_cfg {
            // M2 before the outcome is added (as `Model::observe` in mode `Switch`)
            let pre = self.estimate(fam, difficulty, s);
            let own = self.ev[fam][k][s.idx()];
            let i = Self::sw_idx(fam, k, s.idx());
            if self.sw[i].prior_step(&cfg, pre.p, own.n as f64, own.succ as f64, correct) {
                let succ = own.succ + correct as u64;
                let pred = pre.p * (own.n + 1) as f64;
                self.changes.push(FamChange { kind: "prior-conflict", fam, difficulty: k as u8 + 1, strategy: s, drop: (succ as f64) < pred, statistic: -self.sw[i].prior_ll, n: own.n as u32 + 1, succ: succ as u32, predicted: pred });
            }
        }
        let c = &mut self.cells[fam][k][s.idx()];
        c.n += 1;
        c.succ += correct as u64;
        c.spent += spent;
        c.listed += listed;
        if let Some(cfg) = self.sw_cfg {
            let e = &mut self.ev[fam][k][s.idx()];
            e.n += 1;
            e.succ += correct as u64;
            e.spent += spent;
            e.listed += listed;
            let (cn, cs) = (e.n as f64, e.succ as f64);
            let i = Self::sw_idx(fam, k, s.idx());
            if let Some(h) = self.sw[i].switch_step(&cfg, correct, spent, listed, cn, cs) {
                *e = Cell { n: h.n_window as u64, succ: h.succ_window as u64, spent: h.spent, listed: h.listed };
                self.changes.push(FamChange { kind: "cell-switch", fam, difficulty: k as u8 + 1, strategy: s, drop: h.drop, statistic: h.llr, n: h.n_window, succ: h.succ_window, predicted: h.p_old * h.n_window as f64 });
            }
        }
    }

    /// Forget one cell (a new rule changes what `discovered` does there), as `Model::reset_cell`.
    pub fn reset_cell(&mut self, fam: usize, difficulty: u8, s: Strategy) {
        self.grow(fam);
        let k = (difficulty.clamp(1, 5) - 1) as usize;
        self.cells[fam][k][s.idx()] = Cell::default();
        if self.sw_cfg.is_some() {
            self.ev[fam][k][s.idx()] = Cell::default();
            self.sw[Self::sw_idx(fam, k, s.idx())] = CellSw::new();
        }
    }

    pub fn attempts(&self) -> u64 {
        self.cells.iter().flatten().flatten().map(|c| c.n).sum()
    }

    pub fn cell(&self, fam: usize, difficulty: u8, s: Strategy) -> Cell {
        self.cells.get(fam).map(|x| x[(difficulty.clamp(1, 5) - 1) as usize][s.idx()]).unwrap_or_default()
    }

    /// The evidence the estimate reads for one cell (the raw counts, or under B3b the counts after M1 switches).
    pub fn evidence(&self, fam: usize, difficulty: u8, s: Strategy) -> Cell {
        let cells = if self.sw_cfg.is_some() { &self.ev } else { &self.cells };
        cells.get(fam).map(|x| x[(difficulty.clamp(1, 5) - 1) as usize][s.idx()]).unwrap_or_default()
    }

    /// The B1 estimate (`Model::estimate` in mode `Off`, without transfer links).
    pub fn estimate(&self, fam: usize, difficulty: u8, s: Strategy) -> Estimate {
        let k = (difficulty.clamp(1, 5) - 1) as usize;
        let cells = if self.sw_cfg.is_some() { &self.ev } else { &self.cells };
        let mut st = Tot::default();
        for f in cells {
            for kk in 0..5 {
                st.add(&f[kk][s.idx()]);
            }
        }
        let mut dt = Tot::default();
        let mut c = Tot::default();
        if let Some(f) = cells.get(fam) {
            for kk in 0..5 {
                dt.add(&f[kk][s.idx()]);
            }
            c.add(&f[k][s.idx()]);
        }
        let p_s = (st.succ + 1.0) / (st.n + 2.0);
        let r_s = shrink(st.spent, st.listed, 1.0, COST_K);
        let p_d = shrink(dt.succ, dt.n, p_s, PRIOR_K);
        let mut kc = PRIOR_K;
        if let Some(cfg) = &self.sw_cfg {
            if self.sw.get(Self::sw_idx(fam, k, s.idx())).map_or(false, |x| x.prior_off) {
                kc *= cfg.prior_keep;
            }
        }
        let a = c.succ + kc * p_d;
        let b = (c.n - c.succ) + kc * (1.0 - p_d);
        let p = a / (a + b);
        let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
        let r_d = shrink(dt.spent, dt.listed, r_s, COST_K);
        let cost_ratio = shrink(c.spent, c.listed, r_d, COST_K);
        Estimate { n: c.n as u64, p, sd, cost_ratio, group_n: dt.n }
    }
}
