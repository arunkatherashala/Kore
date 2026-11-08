//! Track B13 (opt-in): a context-aware success estimate for the being's chooser.
//!
//! The B1 model keys every success estimate on one fine cell ((family, difficulty, strategy) in World v2,
//! World O and the first world; (family, strategy, previous family and result) in World L) and shares
//! evidence only by shrinking that cell, with a fixed weight of 4, toward its own family's average. The
//! Track B13 diagnosis (docs/measured.md) found that this is where the being loses to a tuned LinUCB: on
//! offers it has already tried it keeps a worse strategy, because each comparison rests on the few
//! attempts of one cell.
//!
//! The estimate here is the one LinUCB uses (Li, Chu, Langford, Schapire 2010), restated over the being's
//! own evidence and kept explainable:
//!
//! * per strategy, a ridge regression (penalty 1 per feature) of the reported success on visible
//!   features of the offer: bias, family, difficulty and, where the world shows it, the family type and
//!   type x difficulty (the "linear part" `z'theta`);
//! * the cell itself is one more feature with its own penalty `k` (`cell_k`): by the block-inverse
//!   identity its estimate is `(k x linear + successes) / (k + n)`, the B1 shrink with the linear part
//!   in place of the family average, and its uncertainty is
//!   `width^2 = (k / (k + n))^2 x z' S^-1 z + 1 / (k + n)` with `S` the Schur complement of the cells;
//!   `cell_k = 0` means no cell feature (pure linear model);
//! * the chooser's optimistic success is `clamp(estimate + alpha x width, 0, 1)` (LinUCB's bonus).
//!
//! The fit is a pure function of the cell counts (attempts, reported successes), which the being folds
//! from its own log, so every decision can be re-derived from the log alone. Everything a decision used
//! is returned: the linear part, each feature's weight and the attempts behind it, the cell's own counts,
//! the width and the bonus. Only observable things are used; this file reads no world at all (a test scans
//! it), and it has no file, network or process access.

use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Ridge penalty of every global feature (bias, family, difficulty, type, type x difficulty), as LinUCB.
pub const GLOBAL_LAMBDA: f64 = 1.0;
/// Chosen on tuning seeds 101..120 only (docs/measured.md, Track B13).
pub const DEFAULT_ALPHA_MILLI: u32 = 500;
pub const DEFAULT_CELL_K_MILLI: u32 = 4000;

/// The Track B13 switch and its constants (fixed at creation, logged in the genesis event where the world has one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinCfg {
    /// Optimism in widths (milli).
    pub alpha_milli: u32,
    /// Penalty of the cell feature (milli); 0 = no cell feature.
    pub cell_k_milli: u32,
    /// Use the family type and type x difficulty features where the world shows the type of every family.
    pub types: bool,
}

impl Default for LinCfg {
    fn default() -> Self {
        LinCfg { alpha_milli: DEFAULT_ALPHA_MILLI, cell_k_milli: DEFAULT_CELL_K_MILLI, types: true }
    }
}

impl LinCfg {
    pub fn alpha(&self) -> f64 {
        self.alpha_milli as f64 / 1000.0
    }
    pub fn cell_k(&self) -> f64 {
        self.cell_k_milli as f64 / 1000.0
    }
    /// Tag value, e.g. `a500/k4000/t1`; `parse` reads it back.
    pub fn code(&self) -> String {
        format!("a{}/k{}/t{}", self.alpha_milli, self.cell_k_milli, self.types as u8)
    }
    pub fn parse(s: &str) -> Option<LinCfg> {
        let p: Vec<&str> = s.split('/').collect();
        if p.len() != 3 {
            return None;
        }
        let alpha_milli = p[0].strip_prefix('a')?.parse().ok()?;
        let cell_k_milli = p[1].strip_prefix('k')?.parse().ok()?;
        let types = match p[2] {
            "t0" => false,
            "t1" => true,
            _ => return None,
        };
        if alpha_milli > 10_000 || cell_k_milli > 1_000_000 {
            return None;
        }
        Some(LinCfg { alpha_milli, cell_k_milli, types })
    }
    /// Words for a genesis text.
    pub fn words(&self) -> String {
        format!(
            "; chooser estimate (Track B13): each cell shrunk with weight {:.3} toward a shared linear estimate over family, difficulty{} per strategy, optimism {:.3} x width",
            self.cell_k(),
            if self.types { ", type and type x difficulty" } else { "" },
            self.alpha()
        )
    }
}

/// One cell of the being's own evidence for one strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellObs {
    pub strat: usize,
    pub fam: usize,
    /// 1..=5 (1 where the world has no difficulty).
    pub diff: u8,
    /// Visible type of the family, if the world shows it for every family.
    pub ftype: Option<u8>,
    /// Further key of the cell (World L: the previous family and result); 0 elsewhere.
    pub ctx: u32,
    pub n: f64,
    pub succ: f64,
}

/// Feature layout of the linear part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dims {
    pub fams: usize,
    /// Difficulty features (5) on or off.
    pub diffs: bool,
    /// Number of types (0: no type features).
    pub types: usize,
}

impl Dims {
    pub fn len(&self) -> usize {
        1 + self.fams + if self.diffs { 5 } else { 0 } + self.types * if self.diffs { 6 } else { 1 }
    }
    /// Sparse indices of the linear features of (family, difficulty, type).
    pub fn z(&self, fam: usize, diff: u8, ftype: Option<u8>) -> Vec<usize> {
        let mut v = vec![0];
        if fam < self.fams {
            v.push(1 + fam);
        }
        let d = (diff.clamp(1, 5) - 1) as usize;
        let mut o = 1 + self.fams;
        if self.diffs {
            v.push(o + d);
            o += 5;
        }
        if let Some(t) = ftype.map(|t| t as usize).filter(|t| *t < self.types) {
            v.push(o + t);
            if self.diffs {
                v.push(o + self.types + t * 5 + d);
            }
        }
        v
    }
    /// Readable name of feature `i` (`type_names` gives the type words).
    pub fn name(&self, i: usize, type_names: &dyn Fn(usize) -> String) -> String {
        if i == 0 {
            return "bias".into();
        }
        if i <= self.fams {
            return format!("family f{:02}", i - 1);
        }
        let mut o = 1 + self.fams;
        if self.diffs {
            if i < o + 5 {
                return format!("difficulty {}", i - o + 1);
            }
            o += 5;
        }
        if i < o + self.types {
            return format!("type {}", type_names(i - o));
        }
        let j = i - o - self.types;
        format!("type {} x difficulty {}", type_names(j / 5), j % 5 + 1)
    }
}

/// The fit of one strategy.
#[derive(Debug, Clone)]
struct Fit1 {
    /// Lower Cholesky factor of S (dense, G x G).
    l: Vec<f64>,
    theta: Vec<f64>,
    /// Attempts behind each global feature.
    ev: Vec<f64>,
    cells: BTreeMap<(usize, u8, u32), (f64, f64)>,
}

/// The fitted estimate for every strategy (a pure function of the cells it was built from).
#[derive(Debug, Clone)]
pub struct LinFit {
    pub cfg: LinCfg,
    pub dims: Dims,
    fits: Vec<Fit1>,
}

/// What the estimate of one option rests on.
#[derive(Debug, Clone, PartialEq)]
pub struct LinEst {
    /// Estimated success probability (clamped to [0, 1]).
    pub p: f64,
    /// Uncertainty width (LinUCB's sqrt(x' A^-1 x)).
    pub width: f64,
    /// Optimistic success used by the chooser: clamp(p + alpha x width, 0, 1).
    pub p_up: f64,
    /// The linear part z'theta (before the cell's own evidence).
    pub linear: f64,
    pub cell_n: f64,
    pub cell_succ: f64,
    /// Active global features: (index, weight, attempts behind it).
    pub terms: Vec<(usize, f64, f64)>,
}

fn chol(a: &[f64], g: usize) -> Vec<f64> {
    let mut l = vec![0.0; g * g];
    for i in 0..g {
        for j in 0..=i {
            let mut s = a[i * g + j];
            for k in 0..j {
                s -= l[i * g + k] * l[j * g + k];
            }
            if i == j {
                l[i * g + i] = s.max(1e-12).sqrt();
            } else {
                l[i * g + j] = s / l[j * g + j];
            }
        }
    }
    l
}

/// Solve L y = x (forward).
fn fwd(l: &[f64], g: usize, x: &[f64]) -> Vec<f64> {
    let mut y = vec![0.0; g];
    for i in 0..g {
        let mut s = x[i];
        for k in 0..i {
            s -= l[i * g + k] * y[k];
        }
        y[i] = s / l[i * g + i];
    }
    y
}

/// Solve L' x = y (backward).
fn bwd(l: &[f64], g: usize, y: &[f64]) -> Vec<f64> {
    let mut x = vec![0.0; g];
    for i in (0..g).rev() {
        let mut s = y[i];
        for k in i + 1..g {
            s -= l[k * g + i] * x[k];
        }
        x[i] = s / l[i * g + i];
    }
    x
}

impl LinFit {
    /// Fit every strategy `0..strategies` on the cells.
    pub fn fit(cfg: LinCfg, dims: Dims, strategies: usize, cells: &[CellObs]) -> LinFit {
        let g = dims.len();
        let k = cfg.cell_k();
        let mut fits = vec![];
        for s in 0..strategies {
            let mut a = vec![0.0; g * g];
            for i in 0..g {
                a[i * g + i] = GLOBAL_LAMBDA;
            }
            let mut b = vec![0.0; g];
            let mut ev = vec![0.0; g];
            let mut map = BTreeMap::new();
            for c in cells.iter().filter(|c| c.strat == s && c.n > 0.0) {
                let z = dims.z(c.fam, c.diff, c.ftype);
                let w = if k > 0.0 { k / (k + c.n) } else { 1.0 };
                for &i in &z {
                    for &j in &z {
                        a[i * g + j] += c.n * w;
                    }
                    b[i] += c.succ * w;
                    ev[i] += c.n;
                }
                let e = map.entry((c.fam, c.diff, c.ctx)).or_insert((0.0, 0.0));
                e.0 += c.n;
                e.1 += c.succ;
            }
            let l = chol(&a, g);
            let theta = bwd(&l, g, &fwd(&l, g, &b));
            fits.push(Fit1 { l, theta, ev, cells: map });
        }
        LinFit { cfg, dims, fits }
    }

    /// The estimate of strategy `s` on an offer of (family, difficulty, type) in context `ctx`.
    pub fn estimate(&self, s: usize, fam: usize, diff: u8, ftype: Option<u8>, ctx: u32) -> LinEst {
        let f = &self.fits[s];
        let g = self.dims.len();
        let z = self.dims.z(fam, diff, ftype);
        let linear: f64 = z.iter().map(|i| f.theta[*i]).sum();
        let mut x = vec![0.0; g];
        for i in &z {
            x[*i] = 1.0;
        }
        let y = fwd(&f.l, g, &x);
        let q: f64 = y.iter().map(|v| v * v).sum();
        let (n, succ) = f.cells.get(&(fam, diff.clamp(1, 5), ctx)).copied().unwrap_or((0.0, 0.0));
        let k = self.cfg.cell_k();
        let (mean, var) = if k > 0.0 {
            let r = k / (k + n);
            ((k * linear + succ) / (k + n), r * r * q + 1.0 / (k + n))
        } else {
            (linear, q)
        };
        let width = var.max(0.0).sqrt();
        let p = mean.clamp(0.0, 1.0);
        let p_up = (mean + self.cfg.alpha() * width).clamp(0.0, 1.0);
        LinEst { p, width, p_up, linear, cell_n: n, cell_succ: succ, terms: z.iter().map(|i| (*i, f.theta[*i], f.ev[*i])).collect() }
    }
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

impl LinEst {
    /// The evidence of this estimate for a decision rationale.
    pub fn to_json(&self, cfg: &LinCfg, dims: &Dims, type_names: &dyn Fn(usize) -> String) -> Value {
        json!({
            "rule": "p = (cell_k x linear + cell successes) / (cell_k + cell attempts); optimistic p = clamp(p + alpha x width, 0, 1)",
            "linear_part": r3(self.linear),
            "terms": self.terms.iter().map(|(i, w, n)| json!({ "feature": dims.name(*i, type_names), "weight": r3(*w), "attempts_behind": *n as u64 })).collect::<Vec<_>>(),
            "cell_attempts": self.cell_n as u64, "cell_successes": self.cell_succ as u64, "cell_k": cfg.cell_k(),
            "p": r3(self.p), "width": r3(self.width), "alpha": cfg.alpha(), "optimistic_p": r3(self.p_up),
        })
    }
}

// ---- the owner's switch in controller.txt ----

/// Key of `controller.txt`: may the World v2 being be created with the Track B13 chooser estimate
/// (`context_chooser = on`; default off). A bad value makes the whole file unparseable; a missing or
/// unparseable file means off (fail closed); asking for it when it is not allowed is a refusal.
pub const KEY_CHOOSER: &str = "context_chooser";

/// The B13 constants a World v2 being gets when the owner allows the switch (chosen on tuning seeds 101..120 only).
pub const LIN_V2: LinCfg = LinCfg { alpha_milli: 500, cell_k_milli: 16000, types: false };
/// The first world's constants (tuning seeds only; docs/measured.md, Track B13).
pub const LIN_MENU: LinCfg = LinCfg { alpha_milli: 500, cell_k_milli: 4000, types: false };
/// World L's constants (tuning seeds only; docs/measured.md, Track B13).
pub const LIN_L: LinCfg = LinCfg { alpha_milli: 1000, cell_k_milli: 0, types: false };

pub fn is_key(k: &str) -> bool {
    k == KEY_CHOOSER
}

fn on_off(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Check the value (called by `Controller::parse`, so a bad value makes the whole file invalid).
pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    on_off(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off"))
}

/// The B13 switch of `controller.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinCtl {
    pub allowed: bool,
    /// None: from a valid file; Some(reason): failed closed.
    pub closed: Option<String>,
}

impl LinCtl {
    /// From the text of `controller.txt` (None: the file is missing). Missing or invalid: off.
    pub fn from_text(text: Option<&str>) -> LinCtl {
        let Some(text) = text else { return LinCtl { allowed: false, closed: Some("controller.txt is missing".into()) } };
        if let Err(e) = crate::being::controller::Controller::parse(text) {
            return LinCtl { allowed: false, closed: Some(format!("controller.txt is not valid: {e}")) };
        }
        let mut allowed = false;
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == KEY_CHOOSER {
                    allowed = on_off(v.trim()).unwrap_or(false);
                }
            }
        }
        LinCtl { allowed, closed: None }
    }

    /// Grant a request for the switch: Ok(true) granted, Ok(false) not asked for, Err(refusal) asked for and not allowed.
    pub fn grant(&self, want: bool) -> Result<bool, crate::being::controller::Refusal> {
        if !want {
            return Ok(false);
        }
        if self.allowed {
            return Ok(true);
        }
        let why = match &self.closed {
            Some(r) => format!("the context-aware chooser estimate (B13) was requested but the controller failed closed ({r}); the B1 estimate is used"),
            None => format!("the context-aware chooser estimate (B13) was requested but controller.txt does not allow it ({KEY_CHOOSER} = off); the B1 estimate is used"),
        };
        Err(crate::being::controller::Refusal::new(KEY_CHOOSER, 0, 1, why))
    }
}
