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
//! Step B3b adds `Switch` (see `adapt_b3b.rs`): no forgetting; a cell whose recent window disagrees
//! with its older evidence replaces that evidence by the window, and a cell whose outcomes reject the
//! prior keeps only a share of the prior weight. Both are returned as alarms and logged.
//!
//! Everything is a fixed order of float operations on the logged outcomes, so folding the
//! same log twice gives the same model bit for bit.

use super::solver::{Strategy, STRATEGIES, STRATEGIES_EXT};
use super::adapt_b3b::{CellSw, SwitchCfg, DEFAULT_SWITCH};
use super::world::{Domain, ALL_DOMAINS_X, N_DOM};

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
    /// Track B3b: no forgetting and no cold-start bonus; per-cell windowed switch and prior-data
    /// conflict check (`adapt_b3b.rs`).
    Switch,
}

impl AdaptMode {
    pub fn name(self) -> &'static str {
        match self {
            AdaptMode::Off => "off",
            AdaptMode::Forget => "forget",
            AdaptMode::Full => "full",
            AdaptMode::Switch => "switch",
        }
    }
    pub fn parse(s: &str) -> Option<AdaptMode> {
        match s {
            "off" => Some(AdaptMode::Off),
            "forget" => Some(AdaptMode::Forget),
            "full" => Some(AdaptMode::Full),
            "switch" => Some(AdaptMode::Switch),
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
    /// What fired (B3 group CUSUM, or a B3b cell mechanism).
    pub kind: Detector,
    /// Difficulty of the cell (B3b cell mechanisms only).
    pub cell: Option<u8>,
}

/// Which detector produced an alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Detector {
    /// B3: CUSUM per (domain, strategy); the group's evidence is cut to `keep`.
    #[default]
    Group,
    /// B3b M1: the cell's recent window disagrees with its older evidence; the window replaces it.
    CellSwitch,
    /// B3b M2: the cell's outcomes reject its prior; the prior weight is cut.
    PriorConflict,
}

impl Detector {
    pub fn name(self) -> &'static str {
        match self {
            Detector::Group => "group-cusum",
            Detector::CellSwitch => "cell-switch",
            Detector::PriorConflict => "prior-conflict",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Group {
    up: f64,
    down: f64,
    run_n: u32,
    run_succ: u32,
    run_pred: f64,
}

/// Source cells with less (effective) evidence than this do not lend their posterior (B6).
pub const XFER_SRC_MIN_N: f64 = 4.0;
/// B6 guard: a link is revoked when, after at least `guard_min_n` observed attempts it influenced, the
/// observations are less likely under the transferred prior than under the prior without transfer by more than `guard_nats` (log-likelihood).
pub const GUARD_NATS: f64 = 2.0;
pub const GUARD_MIN_N: u32 = 4;
/// Pseudo-observations of the source posterior behind a linked cell. Chosen on tuning seeds 101..120 only.
pub const XFER_K: f64 = 16.0;

/// Track B6: the cells (domain, difficulty, *) of a target domain start from the posterior of the same
/// difficulty and strategy in a source domain instead of from the strategy-wide estimate only.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub src: Domain,
    /// Difficulty of the source cell that is lent (the same difficulty, except in the shifted-cells control).
    pub src_k: u8,
    /// Whether the predictive guard can revoke it (false only in the wrong-similarity control without a guard).
    pub guard: bool,
    pub active: bool,
    /// Cumulative log-likelihood of the observed outcomes under the transferred prior minus under the plain prior.
    pub delta: f64,
    /// Attempts the link changed the prediction for.
    pub n: u32,
}

/// A link switched off by the guard (drained by the state, which logs it).
#[derive(Debug, Clone, PartialEq)]
pub struct Revoked {
    pub domain: Domain,
    pub difficulty: u8,
    pub delta: f64,
    pub n: u32,
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
    /// B6 links per (target domain, difficulty).
    links: [[Option<Link>; 5]; N_DOM],
    /// B6: a rule exists for this (domain, difficulty), so the `discovered` strategy may borrow from a source.
    disc_ready: [[bool; 5]; N_DOM],
    /// Guard thresholds (B6).
    pub guard_nats: f64,
    pub guard_min_n: u32,
    /// Pseudo-observations the source posterior contributes to a linked cell (B6).
    pub xfer_k: f64,
    revoked: Vec<Revoked>,
    /// B3b (mode `Switch` only): settings and per-cell state, indexed by `sw_idx`.
    pub sw_cfg: SwitchCfg,
    sw: Vec<CellSw>,
    /// B3b alarms of the last `observe` beyond the first (an attempt can trip M2 and M1 at once).
    extra_alarms: Vec<Alarm>,
}

fn sw_idx(d: usize, k: usize, s: usize) -> usize {
    (d * 5 + k) * 4 + s
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
            links: Default::default(),
            disc_ready: [[false; 5]; N_DOM],
            guard_nats: GUARD_NATS,
            guard_min_n: GUARD_MIN_N,
            xfer_k: XFER_K,
            revoked: vec![],
            sw_cfg: DEFAULT_SWITCH,
            sw: if mode == AdaptMode::Switch { vec![CellSw::new(); N_DOM * 5 * 4] } else { vec![] },
            extra_alarms: vec![],
        }
        .no_forgetting_in_switch()
    }

    fn no_forgetting_in_switch(mut self) -> Model {
        if self.mode == AdaptMode::Switch {
            self.gamma = 1.0;
        }
        self
    }

    /// B3b alarms of the last `observe` beyond the one it returned.
    pub fn take_extra_alarms(&mut self) -> Vec<Alarm> {
        std::mem::take(&mut self.extra_alarms)
    }

    /// B3b: whether the prior of a cell has been rejected (mode `Switch` only).
    pub fn prior_rejected(&self, d: Domain, difficulty: u8, s: Strategy) -> bool {
        let k = (difficulty.clamp(1, 5) - 1) as usize;
        self.sw.get(sw_idx(d.idx(), k, s.idx())).map_or(false, |c| c.prior_off)
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
        if self.links[d][k].as_ref().map_or(false, |l| l.active && l.guard) {
            let with = self.est(o.domain, o.difficulty, o.strategy, true).p;
            let without = self.est(o.domain, o.difficulty, o.strategy, false).p;
            if with != without {
                let ll = |p: f64| if o.correct { p.ln() } else { (1.0 - p).ln() };
                let (gn, gm) = (self.guard_nats, self.guard_min_n);
                let l = self.links[d][k].as_mut().expect("checked above");
                l.delta += ll(with) - ll(without);
                l.n += 1;
                if l.n >= gm && l.delta < -gn {
                    l.active = false;
                    let rv = Revoked { domain: o.domain, difficulty: o.difficulty, delta: l.delta, n: l.n };
                    self.revoked.push(rv);
                }
            }
        }
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
                    alarm = Some(Alarm { domain: o.domain, strategy: o.strategy, drop: g.down > self.cusum_h, statistic: g.up.max(g.down), run_n: g.run_n, run_succ: g.run_succ, run_pred: g.run_pred, kind: Detector::Group, cell: None });
                    *g = Group::default();
                }
            }
        }
        let mut sw_alarms: Vec<Alarm> = vec![];
        if self.mode == AdaptMode::Switch {
            let pre = self.estimate(o.domain, o.difficulty, o.strategy);
            let own = self.cell_tot(d, k, s);
            let cfg = self.sw_cfg;
            if self.sw[sw_idx(d, k, s)].prior_step(&cfg, pre.p, own.n, own.succ, o.correct) {
                let c = &self.sw[sw_idx(d, k, s)];
                sw_alarms.push(Alarm { domain: o.domain, strategy: o.strategy, drop: (own.succ + o.correct as u8 as f64) < pre.p * (own.n + 1.0), statistic: -c.prior_ll, run_n: own.n as u32 + 1, run_succ: (own.succ + o.correct as u8 as f64) as u32, run_pred: pre.p * (own.n + 1.0), kind: Detector::PriorConflict, cell: Some(o.difficulty) });
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
        if self.mode == AdaptMode::Switch {
            let cfg = self.sw_cfg;
            let (cn, cs) = (self.wcells[d][k][s].n, self.wcells[d][k][s].succ);
            if let Some(h) = self.sw[sw_idx(d, k, s)].switch_step(&cfg, o.correct, o.spent, o.listed_cost, cn, cs) {
                let w = &mut self.wcells[d][k][s];
                w.n = h.n_window as f64;
                w.succ = h.succ_window as f64;
                w.spent = h.spent as f64;
                w.listed = h.listed as f64;
                if cfg.spread < 1.0 {
                    for dd in 0..N_DOM {
                        if dd != d {
                            let c = &mut self.sw[sw_idx(dd, k, s)];
                            c.factor = c.factor.min(cfg.spread);
                        }
                    }
                }
                sw_alarms.push(Alarm { domain: o.domain, strategy: o.strategy, drop: h.drop, statistic: h.llr, run_n: h.n_window, run_succ: h.succ_window, run_pred: h.p_old * h.n_window as f64, kind: Detector::CellSwitch, cell: Some(o.difficulty) });
            }
            let mut it = sw_alarms.into_iter();
            alarm = it.next();
            self.extra_alarms.extend(it);
        }
        if alarm.is_some() && self.mode == AdaptMode::Full {
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
        self.est(d, difficulty, s, true)
    }

    /// Start (or restart) a transfer link for the cells of (`target`, `difficulty`). No-op if one exists.
    pub fn set_link(&mut self, target: Domain, difficulty: u8, src: Domain, src_k: u8, guard: bool) -> bool {
        let slot = &mut self.links[target.idx()][(difficulty.clamp(1, 5) - 1) as usize];
        if slot.is_some() || src == target {
            return false;
        }
        *slot = Some(Link { src, src_k, guard, active: true, delta: 0.0, n: 0 });
        true
    }

    pub fn link(&self, target: Domain, difficulty: u8) -> Option<&Link> {
        self.links[target.idx()][(difficulty.clamp(1, 5) - 1) as usize].as_ref()
    }

    pub fn links_made(&self) -> usize {
        self.links.iter().flatten().filter(|l| l.is_some()).count()
    }

    /// Mark whether the (domain, difficulty) has a live rule (gates the transfer into the `discovered` strategy).
    pub fn set_disc_ready(&mut self, d: Domain, difficulty: u8, ready: bool) {
        self.disc_ready[d.idx()][(difficulty.clamp(1, 5) - 1) as usize] = ready;
    }

    /// (effective attempts, successes) of one strategy pooled over the difficulties of a domain.
    pub fn domain_strategy(&self, d: Domain, s: Strategy) -> (f64, f64) {
        let t = self.domain_total(d, s);
        (t.n, t.succ)
    }

    /// Effective attempts at (domain, difficulty) over all strategies.
    pub fn cell_attempts(&self, d: Domain, difficulty: u8) -> f64 {
        let k = (difficulty.clamp(1, 5) - 1) as usize;
        (0..4).map(|s| self.cell_tot(d.idx(), k, s).n).sum()
    }

    /// Effective attempts in a domain over all strategies and difficulties.
    pub fn domain_attempts(&self, d: Domain) -> f64 {
        (0..5).map(|k| self.cell_attempts(d, k as u8 + 1)).sum()
    }

    /// Links the guard has switched off since the last call.
    pub fn take_revoked(&mut self) -> Vec<Revoked> {
        std::mem::take(&mut self.revoked)
    }

    /// Estimate, with (`use_link`) or without the transfer link. Without a link both are the B1 numbers bit for bit.
    fn est(&self, d: Domain, difficulty: u8, s: Strategy, use_link: bool) -> Estimate {
        self.est_x(d, difficulty, s, use_link, true)
    }

    /// B3b M4: mean posterior success of the same (difficulty, strategy) cell in the other domains that hold
    /// own evidence there (equal weight per domain), or None.
    fn cross_prior(&self, d: Domain, k: usize, s: Strategy) -> Option<f64> {
        let (mut sum, mut m) = (0.0, 0u32);
        for dd in ALL_DOMAINS_X {
            if dd != d && self.cell_tot(dd.idx(), k, s.idx()).n >= 1.0 {
                sum += self.est_x(dd, k as u8 + 1, s, false, false).p;
                m += 1;
            }
        }
        if m > 0 { Some(sum / m as f64) } else { None }
    }

    fn est_x(&self, d: Domain, difficulty: u8, s: Strategy, use_link: bool, use_cross: bool) -> Estimate {
        let k = (difficulty.clamp(1, 5) - 1) as usize;
        let st = self.strategy_total(s);
        let dt = self.domain_total(d, s);
        let c = self.cell_tot(d.idx(), k, s.idx());
        // success probability
        let p_s = (st.succ + 1.0) / (st.n + 2.0);
        let r_s = shrink(st.spent, st.listed, 1.0, COST_K);
        // B6: an active link replaces the whole prior chain of the cell by the posterior of the source cell
        let mut from_src: Option<(f64, f64)> = None;
        if use_link {
            if let Some(l) = self.links[d.idx()][k].as_ref().filter(|l| l.active && (s != Strategy::Discovered || self.disc_ready[d.idx()][k])) {
                let src = self.cell_tot(l.src.idx(), (l.src_k.clamp(1, 5) - 1) as usize, s.idx());
                if src.n >= XFER_SRC_MIN_N {
                    let e = self.est(l.src, l.src_k, s, false);
                    from_src = Some((e.p, e.cost_ratio));
                }
            }
        }
        let p_d = match from_src {
            Some((p, _)) => p,
            None => {
                let plain = shrink(dt.succ, dt.n, p_s, PRIOR_K);
                if use_cross && self.mode == AdaptMode::Switch && self.sw_cfg.cross && dt.n < self.sw_cfg.cross_below {
                    self.cross_prior(d, k, s).unwrap_or(plain)
                } else {
                    plain
                }
            }
        };
        let mut kc = if from_src.is_some() { self.xfer_k } else { PRIOR_K };
        if from_src.is_none() && self.sw.get(sw_idx(d.idx(), k, s.idx())).map_or(false, |c| c.prior_off) {
            kc *= self.sw_cfg.prior_keep;
        }
        let a = c.succ + kc * p_d;
        let b = (c.n - c.succ) + kc * (1.0 - p_d);
        let p = a / (a + b);
        let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
        // cost ratio
        let r_d = match from_src {
            Some((_, r)) => r,
            None => shrink(dt.spent, dt.listed, r_s, COST_K),
        };
        let cost_ratio = shrink(c.spent, c.listed, r_d, COST_K);
        let n = if self.mode == AdaptMode::Off { self.cells[d.idx()][k][s.idx()].n } else { c.n.round() as u64 };
        Estimate { n, p, sd, cost_ratio, group_n: dt.n }
    }

    /// Per (domain, strategy) attempt counts, for status output.
    pub fn counts(&self) -> Vec<(Domain, Strategy, u64, u64)> {
        let mut v = vec![];
        for d in ALL_DOMAINS_X {
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
