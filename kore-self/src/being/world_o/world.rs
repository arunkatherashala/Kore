//! World O: the generator, the family schedule, the exact checkers and the ground truth.
//!
//! A pure function of `WOCfg` (all integers, logged in the genesis event) and of (step, slot, world
//! clock). Families are numbered 0, 1, 2, ... in order of appearance; family `j`:
//!
//! * appears at energy `intro(j)` (the first `start` families at 0, then one every `intro_every`
//!   energy units) and retires `life_intros x intro_every` energy units later (the starting families
//!   are staggered; a `compose` family retires no later than the family whose rule it reuses);
//! * lives in observable slot `j % SLOTS` (the slot is the family index the learner keys on; two
//!   families sharing a slot never live at the same time);
//! * has tier `min(MAX_TIER, j / tier_every)`: tier 0 simple families that a fixed strategy solves,
//!   tier 1 families that need the right one of the fixed strategies (or are unsolvable without a
//!   learned constant), tier 2 sequence families that need a discovered rule, tiers 3 and 4
//!   compositional families (the term h places ahead under the rule of an earlier sequence family)
//!   and negative controls. Listed rewards grow with the tier;
//! * gets harder as it ages (up to 3 difficulty levels over its life), and with probability
//!   `drift_pm / 1000` its hidden rule is re-drawn once at mid-life (the being is not told).
//!
//! The world announces which families are open (an observable public fact: a family's name, slot,
//! type and, for `compose`, its base slot). It never shows rule classes, constants, move rules or the
//! retirement time. Tasks, checkers and solvers are those of World v2 (`world2/world.rs`,
//! `world2/solve.rs`), reused, not copied.

use crate::being::world::{iso_at, Rng, SECS_PER_ENERGY};
use crate::being::world2::world::{bfs, join, knap_exact, mix2, seq_task, FType, FormulaKind, SeqClass, TaskView2};

/// Source of every log row of a World O being.
pub const SOURCE_O: &str = "being:worldo";
/// Observable family slots (the learner's family index). `sig2(slot, 5)` must fit in a u8.
pub const SLOTS: usize = 50;
/// Index of the model's archive of retired families (feeds the strategy-wide prior only).
pub const ARCHIVE: usize = SLOTS;
pub const MAX_TIER: u32 = 4;
pub const SKIP_COST: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WOCfg {
    pub seed: u64,
    /// Energy units of the life (no refills).
    pub budget: i64,
    pub menu: usize,
    /// Families open at energy 0.
    pub start: usize,
    /// Energy units between two new families.
    pub intro_every: i64,
    /// Life of a family, in units of `intro_every`.
    pub life_intros: u32,
    /// Families per tier step.
    pub tier_every: u32,
    /// Per mille of families whose hidden rule is re-drawn once at mid-life.
    pub drift_pm: u32,
    /// Feedback noise in per mille (a family's rate is 0, 1 or 2 times this).
    pub noise_pm: u32,
}

impl WOCfg {
    pub fn new(seed: u64, budget: i64) -> WOCfg {
        WOCfg { seed, budget, menu: 4, start: 4, intro_every: 6000, life_intros: 12, tier_every: 8, drift_pm: 250, noise_pm: 0 }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=1_000_000_000).contains(&self.budget) {
            return Err("budget must be 1..=1000000000 energy units".into());
        }
        if !(2..=8).contains(&self.menu) {
            return Err("menu must be 2..=8".into());
        }
        if !(1..=8).contains(&self.start) {
            return Err("start must be 1..=8".into());
        }
        if !(100..=10_000_000).contains(&self.intro_every) {
            return Err("intro_every must be 100..=10000000".into());
        }
        if !(2..=40).contains(&self.life_intros) {
            return Err("life_intros must be 2..=40".into());
        }
        if !(1..=10_000).contains(&self.tier_every) {
            return Err("tier_every must be 1..=10000".into());
        }
        if self.drift_pm > 1000 || self.noise_pm > 400 {
            return Err("drift_pm must be 0..=1000 and noise_pm 0..=400".into());
        }
        // the number of families must stay small enough to precompute (one per intro_every energy)
        if self.budget / self.intro_every > 100_000 {
            return Err("budget / intro_every must be at most 100000 families".into());
        }
        Ok(())
    }
    pub fn tags(&self) -> Vec<String> {
        vec![
            format!("wo_seed:{}", self.seed),
            format!("wo_budget:{}", self.budget),
            format!("wo_menu:{}", self.menu),
            format!("wo_start:{}", self.start),
            format!("wo_intro_every:{}", self.intro_every),
            format!("wo_life_intros:{}", self.life_intros),
            format!("wo_tier_every:{}", self.tier_every),
            format!("wo_drift_pm:{}", self.drift_pm),
            format!("wo_noise_pm:{}", self.noise_pm),
        ]
    }
    pub fn from_tags(tags: &str) -> Option<WOCfg> {
        let t = |k: &str| crate::being::state::tag(tags, k);
        let c = WOCfg {
            seed: t("wo_seed")?.parse().ok()?,
            budget: t("wo_budget")?.parse().ok()?,
            menu: t("wo_menu")?.parse().ok()?,
            start: t("wo_start")?.parse().ok()?,
            intro_every: t("wo_intro_every")?.parse().ok()?,
            life_intros: t("wo_life_intros")?.parse().ok()?,
            tier_every: t("wo_tier_every")?.parse().ok()?,
            drift_pm: t("wo_drift_pm")?.parse().ok()?,
            noise_pm: t("wo_noise_pm")?.parse().ok()?,
        };
        c.validate().ok()?;
        Some(c)
    }
}

/// The hidden rule of a family in one regime.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegimeO {
    pub class: SeqClass,
    pub g: i64,
    pub eight: bool,
    pub wall_pm: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FamO {
    pub id: u64,
    pub slot: usize,
    pub tier: u32,
    pub ftype: FType,
    /// `compose`: the family (id) whose rule it reuses.
    pub base: Option<u64>,
    pub formula: FormulaKind,
    /// Energy units at which it opens and retires.
    pub intro: i64,
    pub retire: i64,
    /// Energy units of the hidden rule change (None: never).
    pub drift_at: Option<i64>,
    pub reward_scale: f64,
    pub cost_base: i64,
    pub diff_bias: f64,
    pub slip_pm: u64,
    pub noise_mult: u32,
    /// Horizon added to `compose` tasks (tier 4: one more).
    pub extra_h: u8,
}

impl FamO {
    pub fn name(&self) -> String {
        format!("o{:04}-{}", self.id, self.ftype.name())
    }
}

const T0_SEQ: [SeqClass; 1] = [SeqClass::Arith];
const T1_SEQ: [SeqClass; 3] = [SeqClass::Geom, SeqClass::Quad, SeqClass::Fib];
const T2_SEQ: [SeqClass; 6] = [SeqClass::AffRec, SeqClass::Rec2, SeqClass::Tribo, SeqClass::Cubic, SeqClass::Periodic3, SeqClass::Interleave];
const NEG: [SeqClass; 2] = [SeqClass::Primes, SeqClass::Lcg];

/// A task with its checker; `expected` is private.
#[derive(Debug, Clone)]
pub struct TaskO {
    pub view: TaskView2,
    /// The family id (observable: the prompt and the announcements name it).
    pub fid: u64,
    expected: i64,
}

impl TaskO {
    pub fn check(&self, answer: i64) -> bool {
        answer == self.expected
    }
    /// Ground truth, for the oracle and the measurement only.
    pub fn expected(&self) -> i64 {
        self.expected
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeedbackO {
    pub reported_correct: bool,
    pub revealed: i64,
    pub flipped: bool,
}

/// One public announcement: a family is open (name, slot, type, base slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub fid: u64,
    pub slot: usize,
    pub ftype: FType,
    pub base_slot: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct WorldO {
    pub cfg: WOCfg,
    pub fams: Vec<FamO>,
}

impl WorldO {
    pub fn new(cfg: WOCfg) -> WorldO {
        let life = cfg.life_intros as i64 * cfg.intro_every;
        let n = cfg.start + (cfg.budget / cfg.intro_every) as usize + 1;
        let mut fams: Vec<FamO> = Vec::with_capacity(n);
        for j in 0..n {
            let mut r = Rng::new(mix2(&[cfg.seed, j as u64, 0x0_F_A_3]));
            let tier = ((j as u64) / cfg.tier_every as u64).min(MAX_TIER as u64) as u32;
            let (intro, mut retire) = if j < cfg.start { (0, life * (j as i64 + 1) / cfg.start as i64) } else { let t = (j - cfg.start + 1) as i64 * cfg.intro_every; (t, t + life) };
            // type by tier
            let u = r.below(12);
            let ftype = match tier {
                0 => [FType::Seq, FType::Knap, FType::Grid, FType::Formula][(u % 4) as usize],
                1 | 2 => [FType::Seq, FType::Seq, FType::Knap, FType::Grid, FType::Formula, FType::Seq][(u % 6) as usize],
                _ => [FType::Compose, FType::Compose, FType::Compose, FType::Seq, FType::Seq, FType::Grid, FType::Knap, FType::Formula, FType::Compose, FType::Seq, FType::Compose, FType::Seq][u as usize],
            };
            // compose: the newest open sequence family of tier >= 1 that lives at least a quarter life more
            let mut base = None;
            let mut ftype = ftype;
            if ftype == FType::Compose {
                base = fams.iter().rev().take(SLOTS).find(|f| f.ftype == FType::Seq && f.tier >= 1 && f.intro <= intro && f.retire >= intro + life / 4).map(|f| f.id);
                match base {
                    Some(b) => retire = retire.min(fams[b as usize].retire),
                    None => ftype = FType::Seq,
                }
            }
            let drift_at = if (r.below(1000) as u32) < cfg.drift_pm && ftype != FType::Knap { Some(intro + (retire - intro) / 2) } else { None };
            let reward_scale = (((0.7 + 0.3 * tier as f64) * (0.8 + 0.4 * r.f64())) * 100.0).round() / 100.0;
            let cost_base = r.range(2, 6);
            let diff_bias = ((0.6 + r.f64() * 1.2) * 100.0).round() / 100.0;
            let slip_pm = 30 + r.below(71) as u64;
            let noise_mult = [0u32, 1, 1, 2][r.below(4) as usize];
            let formula = if r.below(2) == 0 { FormulaKind::Fall } else { FormulaKind::Weight };
            let extra_h = if tier >= 4 { 1 } else { 0 };
            fams.push(FamO { id: j as u64, slot: j % SLOTS, tier, ftype, base, formula, intro, retire, drift_at, reward_scale, cost_base, diff_bias, slip_pm, noise_mult, extra_h });
        }
        WorldO { cfg, fams }
    }

    fn energy(clock: i64) -> i64 {
        clock / SECS_PER_ENERGY
    }

    /// Regime index of family `j` at world-clock second `clock` (0, or 1 after its rule change).
    pub fn regime_index(&self, j: u64, clock: i64) -> u64 {
        let f = &self.fams[j as usize];
        f.drift_at.map_or(0, |t| (Self::energy(clock) >= t) as u64)
    }

    pub fn regime_at(&self, j: u64, ri: u64) -> RegimeO {
        let f = &self.fams[j as usize];
        let mut r = Rng::new(mix2(&[self.cfg.seed, j, ri, 0x0_7E6]));
        let pool: &[SeqClass] = match f.tier {
            0 => &T0_SEQ,
            1 => &T1_SEQ,
            _ => &T2_SEQ,
        };
        let mut class = pool[r.below(pool.len() as u64) as usize];
        // negative controls from tier 1 on: one sequence family in ten
        if f.tier >= 1 && f.ftype == FType::Seq && r.below(10) == 0 {
            class = NEG[r.below(2) as usize];
        }
        let g = if f.tier == 0 { 10 } else { [6, 8, 10, 12, 14][r.below(5) as usize] };
        let eight = f.tier >= 1 && r.below(2) == 1;
        let wall_pm = 150 + r.below(201) as u64;
        RegimeO { class, g, eight, wall_pm }
    }

    pub fn regime(&self, j: u64, clock: i64) -> RegimeO {
        self.regime_at(j, self.regime_index(j, clock))
    }

    /// The rule class a `seq` or `compose` task of family `j` is generated from at `clock`.
    pub fn class_of(&self, j: u64, clock: i64) -> SeqClass {
        let src = self.fams[j as usize].base.unwrap_or(j);
        self.regime(src, clock).class
    }

    /// Families open at `clock`, oldest first.
    pub fn open(&self, clock: i64) -> Vec<u64> {
        let e = Self::energy(clock);
        let newest = if e < self.cfg.intro_every { self.cfg.start - 1 } else { (self.cfg.start - 1 + (e / self.cfg.intro_every) as usize).min(self.fams.len() - 1) };
        let lo = newest.saturating_sub(SLOTS - 1);
        (lo..=newest).filter(|&j| self.fams[j].intro <= e && e < self.fams[j].retire).map(|j| j as u64).collect()
    }

    /// The public announcement of an open family.
    pub fn announce(&self, j: u64) -> Open {
        let f = &self.fams[j as usize];
        Open { fid: j, slot: f.slot, ftype: f.ftype, base_slot: f.base.map(|b| self.fams[b as usize].slot) }
    }

    /// Family of menu slot `m` at `step` (uniform over the open families).
    pub fn menu_family(&self, step: u64, m: usize, open: &[u64]) -> u64 {
        open[(mix2(&[self.cfg.seed, step, m as u64, 0xD0_0D_0]) % open.len() as u64) as usize]
    }

    /// The offers at `step` and world-clock second `clock`.
    pub fn offers(&self, step: u64, clock: i64) -> Vec<TaskO> {
        let open = self.open(clock);
        if open.is_empty() {
            return vec![];
        }
        (0..self.cfg.menu).map(|m| self.task(step, m, self.menu_family(step, m, &open), None, clock, 0)).collect()
    }

    /// Difficulty shift of family `j` at `clock` from its age (0..=3).
    pub fn age_shift(&self, j: u64, clock: i64) -> u8 {
        let f = &self.fams[j as usize];
        let life = (f.retire - f.intro).max(1) as f64;
        let age = ((Self::energy(clock) - f.intro) as f64 / life).clamp(0.0, 1.0);
        (3.0 * age).floor().min(3.0) as u8
    }

    /// A task of family `j` (difficulty `forced`, or drawn); `salt` separates measurement samples from real offers.
    pub fn task(&self, step: u64, slot: usize, j: u64, forced: Option<u8>, clock: i64, salt: u64) -> TaskO {
        let fam = &self.fams[j as usize];
        let reg = self.regime(j, clock);
        let mut r = Rng::new(mix2(&[self.cfg.seed, step, slot as u64, j, 0x7A5C_0, salt]));
        let u = r.f64();
        let d0 = (1.0 + (u.powf(fam.diff_bias) * 5.0).floor()).clamp(1.0, 5.0) as u8;
        let difficulty = forced.unwrap_or_else(|| (d0 + self.age_shift(j, clock)).min(5));
        let k = difficulty as i64;
        let reward = ((fam.reward_scale * (4.0 + 3.0 * k as f64)).round() as i64).max(1);
        let cost = fam.cost_base + k;
        let name = fam.name();
        let id = if salt == 0 { format!("wo-{step}-{slot}-o{j:04}") } else { format!("wos{salt}-{step}-{slot}-o{j:04}") };
        let (params, horizon, prompt, expected) = match fam.ftype {
            FType::Seq => {
                let (t, next) = seq_task(self.class_of(j, clock), difficulty, 1, &mut r);
                let p = format!("[{name}] Next term: {}", join(&t));
                (t, 1u8, p, next)
            }
            FType::Compose => {
                let h = 2 + (difficulty - 1) / 2 + fam.extra_h;
                let (t, v) = seq_task(self.class_of(j, clock), difficulty, h as usize, &mut r);
                let b = fam.base.unwrap_or(0);
                let p = format!("[{name}] Sequence with the rule of family o{b:04}: what is the term {h} places after the last one? {}", join(&t));
                (t, h, p, v)
            }
            FType::Grid => {
                let (p, ans) = grid_task(difficulty, reg.wall_pm, reg.eight, &mut r);
                let prompt = format!("[{name}] Shortest number of moves from top-left to bottom-right in a {}x{} grid (1 = wall): {}", p[0], p[1], join(&p[2..]));
                (p, 0, prompt, ans)
            }
            FType::Knap => {
                let (p, ans) = knap_task(difficulty, &mut r);
                let prompt = format!("[{name}] Capacity {}; items (weight, value): {}. Best total value?", p[0], join(&p[1..]));
                (p, 0, prompt, ans)
            }
            FType::Formula => match fam.formula {
                FormulaKind::Fall => {
                    let t = r.range(1, 2 + 2 * k);
                    (vec![t], 0, format!("[{name}] A body falls from rest for {t} s on this world. Distance in m?"), reg.g * t * t / 2)
                }
                FormulaKind::Weight => {
                    let m = r.range(1, 5 + 5 * k);
                    (vec![m], 0, format!("[{name}] A mass of {m} kg rests on the ground of this world. Its weight in N?"), m * reg.g)
                }
            },
        };
        let formula = if fam.ftype == FType::Formula { Some(fam.formula) } else { None };
        let base = fam.base.map(|b| self.fams[b as usize].slot);
        TaskO { view: TaskView2 { id, step, slot, fam: fam.slot, ftype: fam.ftype, base, difficulty, params, horizon, formula, prompt, cost, reward }, fid: j, expected }
    }

    /// Per-attempt random source of menu slot `m` at `step` (execution slips).
    pub fn slip_noise(&self, step: u64, m: usize) -> u64 {
        mix2(&[self.cfg.seed, step, m as u64, 0xA11CE, 0x51_07, 0x0])
    }

    pub fn feedback(&self, task: &TaskO, answer: i64) -> FeedbackO {
        let truth = task.check(answer);
        let rate = self.cfg.noise_pm as u64 * self.fams[task.fid as usize].noise_mult as u64;
        let z = mix2(&[self.cfg.seed, task.view.step, task.view.slot as u64, 0xFEED, 0x0]);
        let flipped = rate > 0 && z % 1000 < rate;
        if !flipped {
            return FeedbackO { reported_correct: truth, revealed: task.expected, flipped: false };
        }
        if truth {
            let off = 1 + ((z / 1000) % 3) as i64;
            FeedbackO { reported_correct: false, revealed: task.expected + if (z / 3000) % 2 == 0 { off } else { -off }, flipped: true }
        } else {
            FeedbackO { reported_correct: true, revealed: answer, flipped: true }
        }
    }

    pub fn iso(clock: i64) -> String {
        iso_at(clock)
    }
}

/// Grid of (3 + difficulty)^2 cells, reachable with 4-neighbour moves by construction (as World v2).
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
