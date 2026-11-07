//! World L (Track B8, opt-in): small seeded menu worlds in which one choice changes later options, so
//! that planning ahead can matter, and in which the best possible planner that knows the hidden
//! parameters can be computed exactly (finite-horizon dynamic programme, `oracle_dp`).
//!
//! Author-defined simulation. A world is a pure function of `WLCfg` (seed, setting, budget). Every
//! step shows a menu: the setting's fixed offers, then `random_slots` offers whose family is drawn from
//! the seed. Each family has a listed cost and reward; the being picks (offer, strategy) or skips
//! (1 energy). Strategies cost as everywhere (fast half, careful listed, deep twice the listed energy).
//! Success is drawn from the hidden per-(family, strategy) probability with the world's own random
//! source (a hash of seed, step and slot), so every policy faces the same menus and the same draws.
//!
//! Settings:
//! * `plain`   control: four ordinary families, independent menus. Planning ahead should not matter
//!             beyond the energy trade-off of one step.
//! * `keys`    unlocks: a key family (low reward) gives a token on success (at most one held); a vault
//!             family pays much more (18..26 for 4 energy, against 0.6..1.2 per energy for the ordinary
//!             families) but can be attempted only with a token, which it uses up. The menu
//!             shows which offers need a token; the being is not told which family gives tokens.
//! * `probe`   information before an expensive task: the job family is on every menu; it works with
//!             `fast` in hidden phase 0 and with `careful` in phase 1 (`deep` works in both at twice the
//!             energy); the phase is drawn again after every job attempt. A probe (cost 1, on every menu)
//!             succeeds exactly in phase 0, so it reveals the phase before the job.
//! * `sizes`   energy trade-off: small offers (listed cost 2..3, 1.5..1.9 reward per listed energy,
//!             drawn half as often) and large ones (cost 12..16, 1.3..1.7); a one-step ratio rule takes a
//!             small one whenever it is shown, but a large offer can be worth more once the energy it uses
//!             is priced at the achievable rate.
//!
//! The oracle knows every hidden probability, the menu distribution, the token rule and the phase rule,
//! but not the phase itself (it sees probe results like the being). It maximises expected total reward
//! over the remaining energy exactly (all menus enumerated).

use crate::being::solver::{Strategy, STRATEGIES};
use crate::being::world::Rng;
use crate::being::world2::world::mix2;

pub const SOURCE_L: &str = "being:worldl";
pub const SKIP_COST: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Plain,
    Keys,
    Probe,
    Sizes,
}

pub const SETTINGS: [Setting; 4] = [Setting::Plain, Setting::Keys, Setting::Probe, Setting::Sizes];

impl Setting {
    pub fn name(self) -> &'static str {
        match self {
            Setting::Plain => "plain",
            Setting::Keys => "keys",
            Setting::Probe => "probe",
            Setting::Sizes => "sizes",
        }
    }
    pub fn parse(s: &str) -> Option<Setting> {
        SETTINGS.iter().copied().find(|x| x.name() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FKind {
    Normal,
    Key,
    Vault,
    Probe,
    Job,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FamL {
    pub idx: usize,
    pub kind: FKind,
    pub cost: i64,
    pub reward: i64,
    /// Hidden success probability per strategy (fast, careful, deep); for the job: in phase 0.
    pub p: [f64; 3],
    /// Job only: in phase 1.
    pub p1: [f64; 3],
    /// Weight in the random slots (0: never drawn there).
    pub weight: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WLCfg {
    pub seed: u64,
    pub setting: Setting,
    pub budget: i64,
}

impl WLCfg {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=100_000).contains(&self.budget) {
            return Err("budget must be 1..=100000".into());
        }
        Ok(())
    }
    pub fn tags(&self) -> Vec<String> {
        vec![format!("wl_seed:{}", self.seed), format!("wl_setting:{}", self.setting.name()), format!("wl_budget:{}", self.budget)]
    }
}

/// What the being sees of one offer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OfferL {
    pub slot: usize,
    pub fam: usize,
    pub cost: i64,
    pub reward: i64,
    pub needs_token: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorldL {
    pub cfg: WLCfg,
    pub fams: Vec<FamL>,
    pub fixed: Vec<usize>,
    pub random_slots: usize,
}

fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn uni(r: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * r.f64()
}

fn normal(r: &mut Rng, idx: usize, cost: (i64, i64), rate: (f64, f64), weight: u32) -> FamL {
    let c = r.range(cost.0, cost.1);
    let rr = uni(r, rate.0, rate.1);
    let p = [uni(r, 0.35, 0.9), uni(r, 0.65, 0.95), uni(r, 0.85, 0.99)];
    FamL { idx, kind: FKind::Normal, cost: c, reward: ((c as f64 * rr).round() as i64).max(1), p, p1: p, weight }
}

impl WorldL {
    pub fn new(cfg: WLCfg) -> WorldL {
        let mut r = Rng::new(mix2(&[cfg.seed, 0x1_00CA_4EAD, cfg.setting as u64]));
        let mut fams = vec![];
        let (mut fixed, mut random_slots) = (vec![], 3);
        match cfg.setting {
            Setting::Plain => {
                for i in 0..4 {
                    fams.push(normal(&mut r, i, (3, 8), (1.0, 2.0), 1));
                }
            }
            Setting::Keys => {
                for i in 0..3 {
                    fams.push(normal(&mut r, i, (3, 8), (0.6, 1.2), 2));
                }
                fams.push(FamL { idx: 3, kind: FKind::Key, cost: 4, reward: 1, p: [0.5, 0.95, 0.98], p1: [0.5, 0.95, 0.98], weight: 2 });
                let vr = r.range(18, 26);
                fams.push(FamL { idx: 4, kind: FKind::Vault, cost: 4, reward: vr, p: [0.6, 0.9, 0.97], p1: [0.6, 0.9, 0.97], weight: 2 });
            }
            Setting::Probe => {
                for i in 0..2 {
                    fams.push(normal(&mut r, i, (3, 8), (0.8, 1.6), 1));
                }
                let jr = r.range(20, 28);
                fams.push(FamL { idx: 2, kind: FKind::Job, cost: 10, reward: jr, p: [0.95, 0.05, 0.9], p1: [0.05, 0.95, 0.9], weight: 0 });
                fams.push(FamL { idx: 3, kind: FKind::Probe, cost: 1, reward: 1, p: [1.0; 3], p1: [0.0; 3], weight: 0 });
                fixed = vec![2, 3];
                random_slots = 2;
            }
            Setting::Sizes => {
                for i in 0..3 {
                    fams.push(normal(&mut r, i, (2, 3), (1.5, 1.9), 1));
                }
                for i in 3..6 {
                    fams.push(normal(&mut r, i, (12, 16), (1.3, 1.7), 2));
                }
            }
        }
        WorldL { cfg, fams, fixed, random_slots }
    }

    fn random_fams(&self) -> Vec<(usize, u32)> {
        self.fams.iter().filter(|f| f.weight > 0).map(|f| (f.idx, f.weight)).collect()
    }

    pub fn offer_of(&self, slot: usize, fam: usize) -> OfferL {
        let f = &self.fams[fam];
        OfferL { slot, fam, cost: f.cost, reward: f.reward, needs_token: f.kind == FKind::Vault }
    }

    /// The menu at `step` (a pure function of the seed and the step).
    pub fn offers(&self, step: u64) -> Vec<OfferL> {
        let mut v: Vec<OfferL> = self.fixed.iter().enumerate().map(|(i, f)| self.offer_of(i, *f)).collect();
        let rf = self.random_fams();
        let tot: u32 = rf.iter().map(|x| x.1).sum();
        for j in 0..self.random_slots {
            let mut u = (mix2(&[self.cfg.seed, step, j as u64, 0x3E_4B]) % tot as u64) as u32;
            let mut fam = rf[0].0;
            for (f, w) in &rf {
                if u < *w {
                    fam = *f;
                    break;
                }
                u -= w;
            }
            let slot = v.len();
            v.push(self.offer_of(slot, fam));
        }
        v
    }

    /// The hidden phase at the start of the life.
    pub fn first_phase(&self) -> u8 {
        (mix2(&[self.cfg.seed, 0xF4A5E]) & 1) as u8
    }

    /// The phase drawn after a job attempt at `step`.
    pub fn next_phase(&self, step: u64) -> u8 {
        (mix2(&[self.cfg.seed, step, 0xF4A5E, 2]) & 1) as u8
    }

    /// The hidden success probability of an attempt.
    pub fn p_true(&self, fam: usize, s: Strategy, phase: u8) -> f64 {
        let f = &self.fams[fam];
        let i = s.idx().min(2);
        match f.kind {
            FKind::Probe => if phase == 0 { 1.0 } else { 0.0 },
            FKind::Job if phase == 1 => f.p1[i],
            _ => f.p[i],
        }
    }

    /// Whether an attempt at (step, slot) succeeds (the world's own draw).
    pub fn success(&self, step: u64, slot: usize, fam: usize, s: Strategy, phase: u8) -> bool {
        unit(mix2(&[self.cfg.seed, step, slot as u64, 0x5_11F5])) < self.p_true(fam, s, phase)
    }

    /// Menus of the random slots with their probabilities (for the oracle), as family lists.
    pub fn menu_distribution(&self) -> Vec<(Vec<usize>, f64)> {
        let rf = self.random_fams();
        let tot: f64 = rf.iter().map(|x| x.1 as f64).sum();
        let mut out: Vec<(Vec<usize>, f64)> = vec![(self.fixed.clone(), 1.0)];
        for _ in 0..self.random_slots {
            let mut next = vec![];
            for (m, p) in &out {
                for (f, w) in &rf {
                    let mut mm = m.clone();
                    mm.push(*f);
                    next.push((mm, p * *w as f64 / tot));
                }
            }
            out = next;
        }
        out
    }
}

// ---- the oracle: exact dynamic programme over (energy left, token, phase belief) ----

/// Phase belief of the oracle: unknown (prior 1/2), known 0, known 1.
pub const BEL_U: usize = 0;
pub const BEL_0: usize = 1;
pub const BEL_1: usize = 2;

#[derive(Debug, Clone)]
pub struct OracleDp {
    pub world: WorldL,
    /// v[b][token][belief]
    pub v: Vec<[[f64; 3]; 2]>,
    menus: Vec<(Vec<OfferL>, f64)>,
}

/// Outcome branches of one action from (token, belief): (probability, reward, energy, token', belief').
fn branches(w: &WorldL, o: &OfferL, s: Strategy, tok: usize, bel: usize) -> Vec<(f64, f64, i64, usize, usize)> {
    let f = &w.fams[o.fam];
    let e = s.energy(o.cost);
    let r = o.reward as f64;
    let i = s.idx().min(2);
    match f.kind {
        FKind::Normal => vec![(f.p[i], r, e, tok, bel), (1.0 - f.p[i], 0.0, e, tok, bel)],
        FKind::Key => vec![(f.p[i], r, e, 1, bel), (1.0 - f.p[i], 0.0, e, tok, bel)],
        FKind::Vault => vec![(f.p[i], r, e, 0, bel), (1.0 - f.p[i], 0.0, e, 0, bel)],
        FKind::Probe => match bel {
            BEL_U => vec![(0.5, r, e, tok, BEL_0), (0.5, 0.0, e, tok, BEL_1)],
            BEL_0 => vec![(1.0, r, e, tok, BEL_0)],
            _ => vec![(1.0, 0.0, e, tok, BEL_1)],
        },
        FKind::Job => {
            let p = match bel {
                BEL_U => 0.5 * (f.p[i] + f.p1[i]),
                BEL_0 => f.p[i],
                _ => f.p1[i],
            };
            vec![(p, r, e, tok, BEL_U), (1.0 - p, 0.0, e, tok, BEL_U)]
        }
    }
}

impl OracleDp {
    pub fn new(world: WorldL) -> OracleDp {
        let menus: Vec<(Vec<OfferL>, f64)> = world.menu_distribution().into_iter().map(|(fs, p)| (fs.iter().enumerate().map(|(i, f)| world.offer_of(i, *f)).collect(), p)).collect();
        let b = world.cfg.budget.max(0) as usize;
        let mut d = OracleDp { world, v: vec![[[0.0; 3]; 2]; b + 1], menus };
        // only the parts of the state that the setting can reach
        let ntok = if d.world.cfg.setting == Setting::Keys { 2 } else { 1 };
        let nbel = if d.world.cfg.setting == Setting::Probe { 3 } else { 1 };
        for bb in 1..=b {
            for tok in 0..ntok {
                for bel in 0..nbel {
                    let mut tot = 0.0;
                    for mi in 0..d.menus.len() {
                        let (m, p) = (&d.menus[mi].0, d.menus[mi].1);
                        tot += p * d.best(m, bb as i64, tok, bel).0;
                    }
                    d.v[bb][tok][bel] = tot;
                }
            }
        }
        d
    }

    fn val(&self, b: i64, tok: usize, bel: usize) -> f64 {
        if b <= 0 { 0.0 } else { self.v[b as usize][tok][bel] }
    }

    /// Best expected total reward from this menu, and the action (None: skip).
    pub fn best(&self, m: &[OfferL], b: i64, tok: usize, bel: usize) -> (f64, Option<(usize, Strategy)>) {
        let mut best = (f64::NEG_INFINITY, None);
        for o in m {
            if o.needs_token && tok == 0 {
                continue;
            }
            for s in STRATEGIES {
                if s.energy(o.cost) > b {
                    continue;
                }
                let q: f64 = branches(&self.world, o, s, tok, bel).iter().map(|(p, r, e, t2, b2)| p * (r + self.val(b - e, *t2, *b2))).sum();
                if q > best.0 + 1e-12 {
                    best = (q, Some((o.slot, s)));
                }
            }
        }
        if b >= SKIP_COST {
            let q = self.val(b - SKIP_COST, tok, bel);
            if q > best.0 + 1e-12 {
                best = (q, None);
            }
        }
        if best.0 == f64::NEG_INFINITY {
            best.0 = 0.0;
        }
        best
    }

    /// Expected total reward of the whole life (the oracle's value at the start, phase unknown).
    pub fn value_at_start(&self) -> f64 {
        self.val(self.world.cfg.budget, 0, BEL_U)
    }
}

/// Belief update of anyone who watches the outcomes (oracle bookkeeping).
pub fn belief_after(w: &WorldL, fam: usize, ok: bool, bel: usize) -> usize {
    match w.fams[fam].kind {
        FKind::Probe => if ok { BEL_0 } else { BEL_1 },
        FKind::Job => BEL_U,
        _ => bel,
    }
}
