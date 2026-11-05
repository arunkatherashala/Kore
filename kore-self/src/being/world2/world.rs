//! World v2: the generator, the exact checkers and the ground truth.
//!
//! Everything here is author-defined and a pure function of the config (`W2Cfg`) and of
//! (step, slot, world clock). Two policies that look at the same step and clock see the
//! same offers, and a run can be replayed from its log.
//!
//! A world has `families` task families. Family `i` has a type from a fixed cycle
//! (`TYPE_CYCLE`) and hidden parameters drawn from the seed:
//!
//! | type | task | hidden per family (can drift) | exact checker / ground truth |
//! |---|---|---|---|
//! | `seq` | next term of an integer sequence | the generating rule, one of 12 classes (`SeqClass`) | the generator's next term |
//! | `grid` | shortest path length in a walled grid, start top-left, goal bottom-right | the move rule (4 or 8 neighbours) and the wall density | breadth-first search under the hidden move rule |
//! | `knap` | best total value of items that fit a capacity | nothing beyond the profile (a pure strategy-choice family) | exact dynamic programme |
//! | `formula` | a physics formula with a constant of "this world" | the constant g (6, 8, 10, 12 or 14; textbook value 10) | the formula with the hidden g |
//! | `compose` | the term `h` steps ahead (h = 2..4) of a sequence from the rule of another `seq` family | the base family's rule | the generator, h terms on |
//!
//! Plus, per family: reward scale, base cost, difficulty mix, slip rate of the solvers and a
//! feedback-noise multiplier (0, 1 or 2 times the world's `noise_pm`).
//!
//! World parameters (`W2Cfg`): number of families, budget, feedback noise (per mille),
//! drift rate (hidden changes per 10 000 energy units), difficulty curve (0 = flat, 100 = the
//! difficulty rises by up to 3 levels over the budget) and menu size.
//!
//! Feedback noise: with probability `noise_pm * multiplier / 1000` the result the being is
//! told is the opposite of the checker's verdict, and the revealed answer is made consistent
//! with that lie (a reported-wrong right answer is shown a different "correct" value; a
//! reported-right wrong answer is shown the being's own answer). The world pays (and the
//! measurement scores) the TRUE result; the being only ever sees the reported one.
//!
//! Drift: hidden changes at seeded times. A change of family `f` re-draws its whole hidden
//! regime (profile, rule class, constant, move rule). The being is never told.

use crate::being::world::{iso_at, Rng, SECS_PER_ENERGY};

/// Source of every log row of a World v2 being (separate from `being:loop`, so the
/// original being's state and replay never see these rows and stay byte-identical).
pub const SOURCE2: &str = "being:world2";
pub const MAX_FAMILIES: usize = 48;
pub const DEFAULT_MENU: usize = 4;
/// Energy spent by skipping a whole menu.
pub const SKIP_COST: i64 = 1;

pub fn mix2(parts: &[u64]) -> u64 {
    let mut r = Rng::new(0x2_0D_5EED_0F_B1E);
    let mut acc = 0u64;
    for p in parts {
        r.0 ^= p.wrapping_mul(0xA24B_AED4_963E_E407);
        acc ^= r.next_u64();
    }
    acc
}

/// The parameters of one World v2. All integers, so the logged genesis round-trips exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct W2Cfg {
    pub seed: u64,
    pub families: usize,
    pub budget: i64,
    /// Feedback noise level in per mille (0..=400); a family's own rate is 0, 1 or 2 times this.
    pub noise_pm: u32,
    /// Expected hidden changes per 10 000 energy units (0..=200).
    pub drift_per_10k: u32,
    /// Difficulty curve, 0..=100: 0 flat; 100 the difficulty rises by up to 3 levels over the budget.
    pub curve_pct: u32,
    /// Offers per step (2..=8).
    pub menu: usize,
}

impl W2Cfg {
    pub fn new(seed: u64, families: usize, budget: i64) -> W2Cfg {
        W2Cfg { seed, families, budget, noise_pm: 0, drift_per_10k: 0, curve_pct: 0, menu: DEFAULT_MENU }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=MAX_FAMILIES).contains(&self.families) {
            return Err(format!("families must be 1..={MAX_FAMILIES}"));
        }
        if !(1..=1_000_000).contains(&self.budget) {
            return Err("budget must be 1..=1000000 energy units".into());
        }
        if self.noise_pm > 400 {
            return Err("noise_pm must be 0..=400".into());
        }
        if self.drift_per_10k > 200 {
            return Err("drift_per_10k must be 0..=200".into());
        }
        if self.curve_pct > 100 {
            return Err("curve_pct must be 0..=100".into());
        }
        if !(2..=8).contains(&self.menu) {
            return Err("menu must be 2..=8".into());
        }
        Ok(())
    }
    pub fn tags(&self) -> Vec<String> {
        vec![
            format!("w2_seed:{}", self.seed),
            format!("w2_families:{}", self.families),
            format!("w2_budget:{}", self.budget),
            format!("w2_noise_pm:{}", self.noise_pm),
            format!("w2_drift_per_10k:{}", self.drift_per_10k),
            format!("w2_curve_pct:{}", self.curve_pct),
            format!("w2_menu:{}", self.menu),
        ]
    }
    pub fn from_tags(tags: &str) -> Option<W2Cfg> {
        let t = |k: &str| crate::being::state::tag(tags, k);
        let c = W2Cfg {
            seed: t("w2_seed")?.parse().ok()?,
            families: t("w2_families")?.parse().ok()?,
            budget: t("w2_budget")?.parse().ok()?,
            noise_pm: t("w2_noise_pm")?.parse().ok()?,
            drift_per_10k: t("w2_drift_per_10k")?.parse().ok()?,
            curve_pct: t("w2_curve_pct")?.parse().ok()?,
            menu: t("w2_menu")?.parse().ok()?,
        };
        c.validate().ok()?;
        Some(c)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FType {
    Seq = 0,
    Grid = 1,
    Knap = 2,
    Formula = 3,
    Compose = 4,
}

pub const FTYPES: [FType; 5] = [FType::Seq, FType::Grid, FType::Knap, FType::Formula, FType::Compose];
/// Family i has type TYPE_CYCLE[i % 6]: one third of the families are sequences.
pub const TYPE_CYCLE: [FType; 6] = [FType::Seq, FType::Grid, FType::Knap, FType::Formula, FType::Seq, FType::Compose];

impl FType {
    pub fn name(self) -> &'static str {
        match self {
            FType::Seq => "seq",
            FType::Grid => "grid",
            FType::Knap => "knap",
            FType::Formula => "formula",
            FType::Compose => "compose",
        }
    }
    pub fn parse(s: &str) -> Option<FType> {
        FTYPES.iter().copied().find(|t| t.name() == s)
    }
}

/// Hidden rule classes of the `seq` (and `compose`) families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeqClass {
    Arith,
    Geom,
    Quad,
    Fib,
    AffRec,
    Cubic,
    Rec2,
    Tribo,
    Periodic3,
    Interleave,
    /// Negative controls: no fixed solver and no grammar form describes them.
    Primes,
    Lcg,
}

pub const SEQ_CLASSES: [SeqClass; 12] = [
    SeqClass::Arith,
    SeqClass::Geom,
    SeqClass::Quad,
    SeqClass::Fib,
    SeqClass::AffRec,
    SeqClass::Cubic,
    SeqClass::Rec2,
    SeqClass::Tribo,
    SeqClass::Periodic3,
    SeqClass::Interleave,
    SeqClass::Primes,
    SeqClass::Lcg,
];

impl SeqClass {
    pub fn name(self) -> &'static str {
        match self {
            SeqClass::Arith => "arith",
            SeqClass::Geom => "geom",
            SeqClass::Quad => "quad",
            SeqClass::Fib => "fib",
            SeqClass::AffRec => "affrec",
            SeqClass::Cubic => "cubic",
            SeqClass::Rec2 => "rec2",
            SeqClass::Tribo => "tribo",
            SeqClass::Periodic3 => "periodic3",
            SeqClass::Interleave => "interleave",
            SeqClass::Primes => "primes",
            SeqClass::Lcg => "lcg",
        }
    }
    /// Shortest prefix at difficulty 5 (one or two terms more at lower difficulty).
    fn base_len(self) -> usize {
        match self {
            SeqClass::Arith | SeqClass::Geom => 5,
            SeqClass::Quad | SeqClass::AffRec => 6,
            SeqClass::Fib | SeqClass::Cubic | SeqClass::Rec2 | SeqClass::Periodic3 | SeqClass::Interleave => 7,
            SeqClass::Tribo => 9,
            SeqClass::Primes => 6,
            SeqClass::Lcg => 10,
        }
    }
    /// True if a form of the B2 grammar (`discover.rs`) describes the class (author's knowledge, for analysis only).
    pub fn in_grammar(self) -> bool {
        !matches!(self, SeqClass::Primes | SeqClass::Lcg)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaKind {
    /// d = g t^2 / 2 (t given)
    Fall,
    /// W = m g (m given)
    Weight,
}

/// The hidden regime of one family (re-drawn at every drift event of that family).
#[derive(Debug, Clone, PartialEq)]
pub struct Regime {
    pub reward_scale: f64,
    pub cost_base: i64,
    pub diff_bias: f64,
    pub class: SeqClass,
    pub g: i64,
    pub eight: bool,
    pub wall_pm: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Family {
    pub idx: usize,
    pub ftype: FType,
    /// `compose` families: the `seq` family whose rule they reuse.
    pub base: Option<usize>,
    pub slip_pm: u64,
    pub noise_mult: u32,
    pub formula: FormulaKind,
}

impl Family {
    pub fn name(&self) -> String {
        format!("f{:02}-{}", self.idx, self.ftype.name())
    }
}

/// What the being sees of a task. Never the expected answer.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskView2 {
    pub id: String,
    pub step: u64,
    pub slot: usize,
    pub fam: usize,
    pub ftype: FType,
    /// `compose` tasks: the base family (observable: the prompt names it).
    pub base: Option<usize>,
    pub difficulty: u8,
    /// Meaning by type: seq/compose = the terms; grid = [rows, cols, cells (0 free, 1 wall) row by row];
    /// knap = [capacity, w1, v1, w2, v2, ...]; formula = [t] (fall) or [m] (weight).
    pub params: Vec<i64>,
    /// `compose`: steps ahead (2..=4); 1 for `seq`; 0 otherwise.
    pub horizon: u8,
    pub formula: Option<FormulaKind>,
    pub prompt: String,
    pub cost: i64,
    pub reward: i64,
}

/// A task with its checker; `expected` is private.
#[derive(Debug, Clone)]
pub struct Task2 {
    pub view: TaskView2,
    expected: i64,
}

impl Task2 {
    /// Exact checker.
    pub fn check(&self, answer: i64) -> bool {
        answer == self.expected
    }
    /// Ground truth, for the oracle, the measurement and the log's revealed answer; no policy under test reads it.
    pub fn expected(&self) -> i64 {
        self.expected
    }
}

/// What the being is told after an attempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Feedback {
    pub reported_correct: bool,
    /// The answer revealed as "correct" (consistent with the report).
    pub revealed: i64,
    /// The report is a lie (measurement only; never logged, never shown to the being).
    pub flipped: bool,
}

#[derive(Debug, Clone)]
pub struct World2 {
    pub cfg: W2Cfg,
    pub fams: Vec<Family>,
    /// Hidden change schedule: (world-clock second, family), sorted by time.
    pub drift: Vec<(i64, usize)>,
}

fn nz(r: &mut Rng, lo: i64, hi: i64) -> i64 {
    loop {
        let v = r.range(lo, hi);
        if v != 0 {
            return v;
        }
    }
}

const PRIMES: [i64; 24] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89];

impl World2 {
    pub fn new(cfg: W2Cfg) -> World2 {
        let n = cfg.families;
        let mut fams = Vec::with_capacity(n);
        for i in 0..n {
            let mut r = Rng::new(mix2(&[cfg.seed, i as u64, 0xFA_317]));
            let ftype = TYPE_CYCLE[i % TYPE_CYCLE.len()];
            let base = if ftype == FType::Compose { Some(i - 1) } else { None };
            let slip_pm = 30 + r.below(71) as u64;
            let noise_mult = [0u32, 1, 1, 2][r.below(4) as usize];
            let formula = if r.below(2) == 0 { FormulaKind::Fall } else { FormulaKind::Weight };
            fams.push(Family { idx: i, ftype, base, slip_pm, noise_mult, formula });
        }
        let mut drift = vec![];
        if cfg.drift_per_10k > 0 {
            let mut r = Rng::new(mix2(&[cfg.seed, 0xD21F7_2]));
            let mean = 10_000.0 / cfg.drift_per_10k as f64;
            let mut e = 0.0;
            loop {
                e += mean * (0.5 + r.f64());
                if e > 4.0 * cfg.budget as f64 {
                    break;
                }
                let f = r.below(n as u64) as usize;
                drift.push(((e.round() as i64) * SECS_PER_ENERGY, f));
            }
        }
        World2 { cfg, fams, drift }
    }

    /// Number of hidden changes of family `f` that have happened at world-clock second `clock`.
    pub fn regime_index(&self, f: usize, clock: i64) -> u64 {
        self.drift.iter().filter(|(t, g)| *g == f && clock >= *t).count() as u64
    }

    /// The hidden regime of family `f` at `clock`.
    pub fn regime(&self, f: usize, clock: i64) -> Regime {
        self.regime_at(f, self.regime_index(f, clock))
    }

    pub fn regime_at(&self, f: usize, ri: u64) -> Regime {
        let mut r = Rng::new(mix2(&[self.cfg.seed, f as u64, ri, 0x7E61]));
        let reward_scale = ((0.6 + r.f64()) * 100.0).round() / 100.0;
        let cost_base = r.range(2, 6);
        let diff_bias = ((0.6 + r.f64() * 1.2) * 100.0).round() / 100.0;
        let class = SEQ_CLASSES[r.below(SEQ_CLASSES.len() as u64) as usize];
        let g = [6, 8, 10, 12, 14][r.below(5) as usize];
        let eight = r.below(2) == 1;
        let wall_pm = 150 + r.below(201) as u64;
        Regime { reward_scale, cost_base, diff_bias, class, g, eight, wall_pm }
    }

    /// The rule class a `seq` or `compose` task of family `f` is generated from at `clock`.
    pub fn class_of(&self, f: usize, clock: i64) -> SeqClass {
        let src = self.fams[f].base.unwrap_or(f);
        self.regime(src, clock).class
    }

    /// Difficulty shift of the curve at `clock` (0..=3).
    pub fn curve_shift(&self, clock: i64) -> u8 {
        let life = (self.cfg.budget * SECS_PER_ENERGY).max(1) as f64;
        let progress = (clock as f64 / life).clamp(0.0, 1.0);
        (3.0 * progress * self.cfg.curve_pct as f64 / 100.0).floor() as u8
    }

    /// Family of menu slot `j` at `step`.
    pub fn slot_family(&self, step: u64, j: usize) -> usize {
        (mix2(&[self.cfg.seed, step, j as u64, 0xD0_0D]) % self.cfg.families as u64) as usize
    }

    /// The offers at `step` and world-clock second `clock`.
    pub fn offers(&self, step: u64, clock: i64) -> Vec<Task2> {
        (0..self.cfg.menu).map(|j| self.task(step, j, self.slot_family(step, j), None, clock, 0)).collect()
    }

    /// A task of family `f` (difficulty `forced`, or drawn); `salt` separates measurement samples from real offers.
    pub fn task(&self, step: u64, slot: usize, f: usize, forced: Option<u8>, clock: i64, salt: u64) -> Task2 {
        let fam = &self.fams[f];
        let reg = self.regime(f, clock);
        let mut r = Rng::new(mix2(&[self.cfg.seed, step, slot as u64, f as u64, 0x7A5C, salt]));
        let u = r.f64();
        let d0 = (1.0 + (u.powf(reg.diff_bias) * 5.0).floor()).clamp(1.0, 5.0) as u8;
        let difficulty = forced.unwrap_or_else(|| (d0 + self.curve_shift(clock)).min(5));
        let k = difficulty as i64;
        let reward = ((reg.reward_scale * (4.0 + 3.0 * k as f64)).round() as i64).max(1);
        let cost = reg.cost_base + k;
        let id = if salt == 0 { format!("w2-{step}-{slot}-f{f:02}") } else { format!("w2s{salt}-{step}-{slot}-f{f:02}") };
        let (params, horizon, prompt, expected) = match fam.ftype {
            FType::Seq => {
                let (t, next) = seq_task(self.class_of(f, clock), difficulty, 1, &mut r);
                let p = format!("[{}] Next term: {}", fam.name(), join(&t));
                (t, 1u8, p, next)
            }
            FType::Compose => {
                let h = 2 + (difficulty - 1) / 2;
                let (t, v) = seq_task(self.class_of(f, clock), difficulty, h as usize, &mut r);
                let p = format!("[{}] Sequence with the rule of family f{:02}: what is the term {h} places after the last one? {}", fam.name(), fam.base.unwrap_or(0), join(&t));
                (t, h, p, v)
            }
            FType::Grid => {
                let (p, ans) = grid_task(difficulty, reg.wall_pm, reg.eight, &mut r);
                let prompt = format!("[{}] Shortest number of moves from top-left to bottom-right in a {}x{} grid (1 = wall): {}", fam.name(), p[0], p[1], join(&p[2..]));
                (p, 0, prompt, ans)
            }
            FType::Knap => {
                let (p, ans) = knap_task(difficulty, &mut r);
                let prompt = format!("[{}] Capacity {}; items (weight, value): {}. Best total value?", fam.name(), p[0], join(&p[1..]));
                (p, 0, prompt, ans)
            }
            FType::Formula => match fam.formula {
                FormulaKind::Fall => {
                    let t = r.range(1, 2 + 2 * k);
                    (vec![t], 0, format!("[{}] A body falls from rest for {t} s on this world. Distance in m?", fam.name()), reg.g * t * t / 2)
                }
                FormulaKind::Weight => {
                    let m = r.range(1, 5 + 5 * k);
                    (vec![m], 0, format!("[{}] A mass of {m} kg rests on the ground of this world. Its weight in N?", fam.name()), m * reg.g)
                }
            },
        };
        let formula = if fam.ftype == FType::Formula { Some(fam.formula) } else { None };
        Task2 { view: TaskView2 { id, step, slot, fam: f, ftype: fam.ftype, base: fam.base, difficulty, params, horizon, formula, prompt, cost, reward }, expected }
    }

    /// Per-attempt random source of slot `slot` at `step` (execution slips).
    pub fn slip_noise(&self, step: u64, slot: usize) -> u64 {
        mix2(&[self.cfg.seed, step, slot as u64, 0xA11CE, 0x51_07])
    }

    /// The feedback the being gets for `answer` on `task` (a pure function of seed, step and slot).
    pub fn feedback(&self, task: &Task2, answer: i64) -> Feedback {
        let truth = task.check(answer);
        let rate = self.cfg.noise_pm as u64 * self.fams[task.view.fam].noise_mult as u64;
        let z = mix2(&[self.cfg.seed, task.view.step, task.view.slot as u64, 0xFEED]);
        let flipped = rate > 0 && z % 1000 < rate;
        if !flipped {
            return Feedback { reported_correct: truth, revealed: task.expected, flipped: false };
        }
        if truth {
            let off = 1 + ((z / 1000) % 3) as i64;
            Feedback { reported_correct: false, revealed: task.expected + if (z / 3000) % 2 == 0 { off } else { -off }, flipped: true }
        } else {
            Feedback { reported_correct: true, revealed: answer, flipped: true }
        }
    }

    /// ISO time of a world-clock second (same origin as the first world).
    pub fn iso(clock: i64) -> String {
        iso_at(clock)
    }
}

/// A sequence of `class` at `difficulty`: the prefix and the term `h` places after its last term.
pub fn seq_task(class: SeqClass, difficulty: u8, h: usize, r: &mut Rng) -> (Vec<i64>, i64) {
    let d = difficulty as i64;
    let len = class.base_len() + ((5 - difficulty.clamp(1, 5)) / 2) as usize;
    let total = len + h;
    let t: Vec<i64> = match class {
        SeqClass::Arith => {
            let (s, st) = (r.range(-10, 20), nz(r, -5, 3 + 2 * d));
            (0..total as i64).map(|i| s + st * i).collect()
        }
        SeqClass::Geom => {
            let (s, q): (i64, i64) = (r.range(1, 4), [2, 3, -2][r.below(if d >= 3 { 3 } else { 2 }) as usize]);
            (0..total as u32).map(|i| s * q.pow(i)).collect()
        }
        SeqClass::Quad => {
            let (a, b, c) = (r.range(1, 1 + d / 2), r.range(-4, 6), r.range(-5, 9));
            (1..=total as i64).map(|n| a * n * n + b * n + c).collect()
        }
        SeqClass::Fib => {
            let mut t = vec![r.range(1, 3 + d), r.range(1, 3 + d)];
            while t.len() < total {
                t.push(t[t.len() - 1] + t[t.len() - 2]);
            }
            t
        }
        SeqClass::AffRec => {
            let (p, c) = ([2, 3, -2][r.below(3) as usize], nz(r, -5, 9));
            let mut t = vec![r.range(1, 5)];
            while t.len() < total {
                t.push(p * t[t.len() - 1] + c);
            }
            t
        }
        SeqClass::Cubic => {
            let (a, b, c, e) = (r.range(1, 2), r.range(-3, 3), r.range(-5, 5), r.range(-5, 9));
            (1..=total as i64).map(|n| a * n * n * n + b * n * n + c * n + e).collect()
        }
        SeqClass::Rec2 => {
            let (mut p, q) = (r.range(1, 3), nz(r, -2, 3));
            if (p, q) == (1, 1) {
                p = 2;
            }
            let mut t = vec![r.range(1, 5), r.range(1, 5)];
            while t.len() < total {
                t.push(p * t[t.len() - 1] + q * t[t.len() - 2]);
            }
            t
        }
        SeqClass::Tribo => {
            let mut t = vec![r.range(1, 4), r.range(1, 4), r.range(1, 4)];
            while t.len() < total {
                t.push(t[t.len() - 1] + t[t.len() - 2] + t[t.len() - 3]);
            }
            t
        }
        SeqClass::Periodic3 => {
            let cyc = [r.range(1, 9 + d), r.range(1, 9 + d), r.range(1, 9 + d)];
            (0..total).map(|i| cyc[i % 3]).collect()
        }
        SeqClass::Interleave => {
            let a = r.range(1, 9);
            let mut b = r.range(-4, 9);
            if b == a {
                b = a % 9 + 1;
            }
            let mut t = vec![r.range(0, 10)];
            while t.len() < total {
                let i = t.len() - 1;
                t.push(t[i] + if i % 2 == 0 { a } else { b });
            }
            t
        }
        SeqClass::Primes => {
            let i0 = r.range(0, (PRIMES.len() - total) as i64) as usize;
            PRIMES[i0..i0 + total].to_vec()
        }
        SeqClass::Lcg => {
            let (a, c) = ([2, 3, 5][r.below(3) as usize], r.range(1, 9));
            let mut t = vec![r.range(1, 100)];
            while t.len() < total {
                t.push((a * t[t.len() - 1] + c) % 101);
            }
            t
        }
    };
    let v = t[total - 1];
    (t[..len].to_vec(), v)
}

/// Grid of (3 + difficulty)^2 cells; reachable under 4-neighbour moves by construction.
fn grid_task(difficulty: u8, wall_pm: u64, eight: bool, r: &mut Rng) -> (Vec<i64>, i64) {
    let n = 3 + difficulty as usize;
    for attempt in 0..30 {
        let mut cells = vec![0i64; n * n];
        if attempt < 29 {
            for (i, c) in cells.iter_mut().enumerate() {
                if i != 0 && i != n * n - 1 && (r.next_u64() % 1000) < wall_pm {
                    *c = 1;
                }
            }
        }
        if bfs(n, n, &cells, false).is_some() {
            let ans = bfs(n, n, &cells, eight).expect("reachable with 4 moves, so with 8");
            let mut p = vec![n as i64, n as i64];
            p.extend(cells);
            return (p, ans);
        }
    }
    unreachable!("the last attempt has no walls")
}

/// Breadth-first search from the top-left to the bottom-right cell; None if unreachable.
pub fn bfs(rows: usize, cols: usize, cells: &[i64], eight: bool) -> Option<i64> {
    let mut dist = vec![-1i64; rows * cols];
    let mut q = std::collections::VecDeque::new();
    dist[0] = 0;
    q.push_back(0usize);
    let moves: &[(i64, i64)] = if eight { &[(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)] } else { &[(-1, 0), (1, 0), (0, -1), (0, 1)] };
    while let Some(i) = q.pop_front() {
        let (y, x) = ((i / cols) as i64, (i % cols) as i64);
        for (dy, dx) in moves {
            let (ny, nx) = (y + dy, x + dx);
            if ny < 0 || nx < 0 || ny >= rows as i64 || nx >= cols as i64 {
                continue;
            }
            let j = ny as usize * cols + nx as usize;
            if cells[j] == 0 && dist[j] < 0 {
                dist[j] = dist[i] + 1;
                q.push_back(j);
            }
        }
    }
    let d = dist[rows * cols - 1];
    if d >= 0 {
        Some(d)
    } else {
        None
    }
}

fn knap_task(difficulty: u8, r: &mut Rng) -> (Vec<i64>, i64) {
    let n = 2 + difficulty as usize;
    let items: Vec<(i64, i64)> = (0..n).map(|_| (r.range(1, 9), r.range(1, 20))).collect();
    let cap = (items.iter().map(|x| x.0).sum::<i64>() / 2).max(1);
    let mut p = vec![cap];
    for (w, v) in &items {
        p.push(*w);
        p.push(*v);
    }
    let best = knap_exact(cap, &items);
    (p, best)
}

/// Exact 0/1 knapsack by dynamic programming over the capacity.
pub fn knap_exact(cap: i64, items: &[(i64, i64)]) -> i64 {
    let c = cap.max(0) as usize;
    let mut best = vec![0i64; c + 1];
    for (w, v) in items {
        let w = *w as usize;
        for x in (w..=c).rev() {
            best[x] = best[x].max(best[x - w] + v);
        }
    }
    best[c]
}

pub fn join(t: &[i64]) -> String {
    t.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
}
