//! The learned world model (Track B, steps B1 and B3).
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
//! Step B3 adds three optional behaviours, chosen once when the being is
//! created (`AdaptMode`); with `Off` the numbers are the B1 numbers bit for bit.
//!   * `Forget`: every count is discounted exponentially with a half-life measured in
//!     attempts (`DEFAULT_HALF_LIFE`): evidence that is `h` attempts old counts half. Cells
//!     that are no longer visited lose weight too, so their uncertainty grows back and the
//!     planner's optimism sends it to look again.
//!   * `Full`: forgetting plus a change detector per (domain, strategy) group. Before each
//!     attempt is added, the model's own prediction for that cell is compared with the
//!     outcome; the standardized residuals feed a two-sided CUSUM (Page, 1954) with drift
//!     `CUSUM_K` and threshold `CUSUM_H`. On an alarm the group's evidence is cut to
//!     `KEEP_AFTER_ALARM` of its weight and the alarm is returned to the caller, which logs
//!     it with the outcome events it rests on.
//!
//! Everything is a fixed order of float operations on the logged outcomes, so folding the
//! same log twice gives the same model bit for bit.

use super::solver::{Strategy, STRATEGIES, STRATEGIES_EXT};
use super::world::{Domain, ALL_DOMAINS, N_DOM};

/// Pseudo-observations the parent level contributes to a child level.
pub const PRIOR_K: f64 = 4.0;
/// Pseudo energy units (listed) behind the prior cost ratio of a level.
pub const COST_K: f64 = 4.0;

/// Half-life of evidence, in attempts, for the adaptive learners.
/// Chosen on tuning seeds 101..120 only (see docs/measured.md).
pub const DEFAULT_HALF_LIFE: f64 = 5000.0;
/// CUSUM reference value (per standardized residual).
pub const CUSUM_K: f64 = 1.0;
/// CUSUM alarm threshold.
pub const CUSUM_H: f64 = 8.0;
/// Share of a group's weight kept after an alarm.
pub const KEEP_AFTER_ALARM: f64 = 0.1;
/// A cell must hold at least this much (effective) evidence before it feeds the detector.
pub const MIN_EVIDENCE: f64 = 3.0;
/// Floor of the Bernoulli variance when standardizing a residual.
const VAR_FLOOR: f64 = 0.09;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdaptMode {
    /// B1 learner: all evidence counts forever.
    #[default]
    Off,
    /// Exponential forgetting only.
    Forget,
    /// Forgetting, change detection and a cold-start bonus for domains never tried with a strategy.
    Full,
}

impl AdaptMode {
    pub fn name(self) -> &'static str {
        match self {
            AdaptMode::Off => "off",
            AdaptMode::Forget => "forget",
            AdaptMode::Full => "full",
        }
    }
    pub fn parse(s: &str) -> Option<AdaptMode> {
        match s {
            "off" => Some(AdaptMode::Off),
            "forget" => Some(AdaptMode::Forget),
            "full" => Some(AdaptMode::Full),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cell {
    pub n: u64,
    pub succ: u64,
    pub spent: i64,
    pub listed: i64,
}

/// A cell with discounted counts; `t` is the attempt counter at which it was last brought up to date.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct WCell {
    n: f64,
    succ: f64,
    spent: f64,
    listed: f64,
    t: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tot {
    n: f64,
    succ: f64,
    spent: f64,
    listed: f64,
}

impl Tot {
    fn add(&mut self, o: &Tot) {
        self.n += o.n;
        self.succ += o.succ;
        self.spent += o.spent;
        self.listed += o.listed;
    }
}

impl From<&Cell> for Tot {
    fn from(c: &Cell) -> Tot {
        Tot { n: c.n as f64, succ: c.succ as f64, spent: c.spent as f64, listed: c.listed as f64 }
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
    /// Attempts observed in this exact cell (discounted count, rounded, when forgetting).
    pub n: u64,
    pub p: f64,
    /// Posterior standard deviation of the success probability.
    pub sd: f64,
    /// Expected energy spent divided by the listed cost.
    pub cost_ratio: f64,
    /// Effective attempts observed in the whole (domain, strategy) group.
    pub group_n: f64,
}

/// A detected change in one (domain, strategy) group.
#[derive(Debug, Clone, PartialEq)]
pub struct Alarm {
    pub domain: Domain,
    pub strategy: Strategy,
    /// True when the success rate looks lower than the model predicted.
    pub drop: bool,
    /// CUSUM statistic at the alarm.
    pub statistic: f64,
    /// Attempts since the statistic last sat at zero (the run the alarm rests on).
    pub run_n: u32,
    pub run_succ: u32,
    /// Sum of the model's predicted success probabilities over that run.
    pub run_pred: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Group {
    up: f64,
    down: f64,
    run_n: u32,
    run_succ: u32,
    run_pred: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    cells: [[[Cell; 4]; 5]; N_DOM],
    mode: AdaptMode,
    /// Per-attempt retention factor 0.5^(1/half-life).
    gamma: f64,
    /// Detector settings (CUSUM drift, threshold, share of weight kept after an alarm).
    pub cusum_k: f64,
    pub cusum_h: f64,
    pub keep: f64,
    wcells: [[[WCell; 4]; 5]; N_DOM],
    groups: [[Group; 4]; N_DOM],
    /// Attempts observed so far (the clock of the discounting).
    t: u64,
    /// Discounted reward and energy of recent steps (attempts and skips), for the skip rule.
    rw: f64,
    cw: f64,
}

impl Default for Model {
    fn default() -> Model {
        Model::new(AdaptMode::Off, DEFAULT_HALF_LIFE)
    }
}

fn shrink(s: f64, n: f64, prior: f64, k: f64) -> f64 {
    (s + k * prior) / (n + k)
}

impl Model {
    pub fn new(mode: AdaptMode, half_life: f64) -> Model {
        Model {
            cells: [[[Cell::default(); 4]; 5]; N_DOM],
            mode,
            gamma: 0.5f64.powf(1.0 / half_life.max(1.0)),
            cusum_k: CUSUM_K,
            cusum_h: CUSUM_H,
            keep: KEEP_AFTER_ALARM,
            wcells: [[[WCell::default(); 4]; 5]; N_DOM],
            groups: [[Group::default(); 4]; N_DOM],
            t: 0,
            rw: 0.0,
            cw: 0.0,
        }
    }

    pub fn mode(&self) -> AdaptMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: AdaptMode, half_life: f64) {
        *self = Model::new(mode, half_life);
    }

    pub fn adaptive(&self) -> bool {
        self.mode != AdaptMode::Off
    }

    fn retained(&self, c: &WCell) -> f64 {
        let dt = (self.t - c.t).min(i32::MAX as u64) as i32;
        self.gamma.powi(dt)
    }

    fn cell_tot(&self, d: usize, k: usize, s: usize) -> Tot {
        if self.mode == AdaptMode::Off {
            Tot::from(&self.cells[d][k][s])
        } else {
            let c = &self.wcells[d][k][s];
            let f = self.retained(c);
            Tot { n: c.n * f, succ: c.succ * f, spent: c.spent * f, listed: c.listed * f }
        }
    }

    /// Observe one attempt. Returns an alarm when the change detector fires (mode `Full` only).
    pub fn observe(&mut self, o: &Obs) -> Option<Alarm> {
        let (d, k, s) = (o.domain.idx(), (o.difficulty.clamp(1, 5) - 1) as usize, o.strategy.idx());
        let mut alarm = None;
        if self.mode == AdaptMode::Full {
            let pre = self.estimate(o.domain, o.difficulty, o.strategy);
            let c = self.cell_tot(d, k, s);
            if c.n >= MIN_EVIDENCE {
                let y = o.correct as u8 as f64;
                let z = (y - pre.p) / (pre.p * (1.0 - pre.p)).max(VAR_FLOOR).sqrt();
                let g = &mut self.groups[d][s];
                g.up = (g.up + z - self.cusum_k).max(0.0);
                g.down = (g.down - z - self.cusum_k).max(0.0);
                g.run_n += 1;
                g.run_succ += o.correct as u32;
                g.run_pred += pre.p;
                if g.up == 0.0 && g.down == 0.0 {
                    g.run_n = 0;
                    g.run_succ = 0;
                    g.run_pred = 0.0;
                } else if g.up > self.cusum_h || g.down > self.cusum_h {
                    alarm = Some(Alarm { domain: o.domain, strategy: o.strategy, drop: g.down > self.cusum_h, statistic: g.up.max(g.down), run_n: g.run_n, run_succ: g.run_succ, run_pred: g.run_pred });
                    *g = Group::default();
                }
            }
        }
        self.t += 1;
        let c = &mut self.cells[d][k][s];
        c.n += 1;
        c.succ += o.correct as u64;
        c.spent += o.spent;
        c.listed += o.listed_cost;
        if self.mode != AdaptMode::Off {
            let f = self.retained(&self.wcells[d][k][s]);
            let w = &mut self.wcells[d][k][s];
            w.n = w.n * f + 1.0;
            w.succ = w.succ * f + o.correct as u8 as f64;
            w.spent = w.spent * f + o.spent as f64;
            w.listed = w.listed * f + o.listed_cost as f64;
            w.t = self.t;
        }
        if alarm.is_some() {
            // bring the group up to date, then keep only a share of its weight
            for kk in 0..5 {
                let f = self.retained(&self.wcells[d][kk][s]);
                let w = &mut self.wcells[d][kk][s];
                w.n *= f * self.keep;
                w.succ *= f * self.keep;
                w.spent *= f * self.keep;
                w.listed *= f * self.keep;
                w.t = self.t;
            }
        }
        alarm
    }

    /// Account for one step of the being (attempt or skip): reward and energy, discounted, for the skip rule.
    pub fn note_step(&mut self, reward: i64, cost: i64) {
        if self.mode != AdaptMode::Off {
            self.rw = self.rw * self.gamma + reward as f64;
            self.cw = self.cw * self.gamma + cost as f64;
        }
    }

    /// Discounted reward per energy over recent steps (None when the model does not forget or has seen nothing).
    pub fn recent_rate(&self) -> Option<f64> {
        if self.mode != AdaptMode::Off && self.cw > 0.0 {
            Some(self.rw / self.cw)
        } else {
            None
        }
    }

    /// Forget one cell (used when a new rule changes what the `discovered` strategy does there).
    pub fn reset_cell(&mut self, d: Domain, difficulty: u8, s: Strategy) {
        let k = (difficulty.clamp(1, 5) - 1) as usize;
        self.cells[d.idx()][k][s.idx()] = Cell::default();
        self.wcells[d.idx()][k][s.idx()] = WCell::default();
    }

    pub fn attempts(&self) -> u64 {
        self.cells.iter().flatten().flatten().map(|c| c.n).sum()
    }

    fn strategy_total(&self, s: Strategy) -> Tot {
        let mut t = Tot::default();
        for d in 0..N_DOM {
            for k in 0..5 {
                t.add(&self.cell_tot(d, k, s.idx()));
            }
        }
        t
    }

    fn domain_total(&self, d: Domain, s: Strategy) -> Tot {
        let mut t = Tot::default();
        for k in 0..5 {
            t.add(&self.cell_tot(d.idx(), k, s.idx()));
        }
        t
    }

    pub fn estimate(&self, d: Domain, difficulty: u8, s: Strategy) -> Estimate {
        let st = self.strategy_total(s);
        let dt = self.domain_total(d, s);
        let c = self.cell_tot(d.idx(), (difficulty.clamp(1, 5) - 1) as usize, s.idx());
        // success probability
        let p_s = (st.succ + 1.0) / (st.n + 2.0);
        let p_d = shrink(dt.succ, dt.n, p_s, PRIOR_K);
        let a = c.succ + PRIOR_K * p_d;
        let b = (c.n - c.succ) + PRIOR_K * (1.0 - p_d);
        let p = a / (a + b);
        let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
        // cost ratio
        let r_s = shrink(st.spent, st.listed, 1.0, COST_K);
        let r_d = shrink(dt.spent, dt.listed, r_s, COST_K);
        let cost_ratio = shrink(c.spent, c.listed, r_d, COST_K);
        let n = if self.mode == AdaptMode::Off { self.cells[d.idx()][(difficulty.clamp(1, 5) - 1) as usize][s.idx()].n } else { c.n.round() as u64 };
        Estimate { n, p, sd, cost_ratio, group_n: dt.n }
    }

    /// Per (domain, strategy) attempt counts, for status output.
    pub fn counts(&self) -> Vec<(Domain, Strategy, u64, u64)> {
        let mut v = vec![];
        for d in ALL_DOMAINS {
            for s in STRATEGIES_EXT {
                let mut n = 0;
                let mut succ = 0;
                for k in 0..5 {
                    let c = &self.cells[d.idx()][k][s.idx()];
                    n += c.n;
                    succ += c.succ;
                }
                if n > 0 || (d.idx() < 4 && STRATEGIES.contains(&s)) {
                    v.push((d, s, n, succ));
                }
            }
        }
        v
    }
}
