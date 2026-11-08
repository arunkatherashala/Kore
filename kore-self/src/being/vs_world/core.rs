//! Track V1: standard methods from the literature, written from their papers, as players of the being's
//! sandbox worlds. World-independent: every method sees only an `Opt` list (the visible features of each
//! (offer, strategy) pair and the skip) and a `Ctx` (budget left, the reward and energy it was TOLD so far,
//! a visible state id), and after each step the reported reward and the energy spent. Nothing here can
//! read a hidden world parameter: the adapters (`adapt.rs`) build `Opt` from what the being's own planner
//! sees, and a test scans this file for hidden parameter names.
//!
//! Methods (each a clean textbook version; hyper-parameters tuned on seeds 101..120 only, see
//! docs/vs_world.md):
//! * epsilon-greedy with the decaying rate eps_t = min(1, c K / t) (Auer, Cesa-Bianchi, Fischer 2002, "eps_n-greedy");
//! * UCB1-tuned (Auer, Cesa-Bianchi, Fischer 2002);
//! * KL-UCB for Bernoulli rewards (Garivier, Cappe 2011);
//! * Thompson sampling with a Beta-Bernoulli posterior per arm (Thompson 1933; Agrawal, Goyal 2012);
//! * EXP3 (Auer, Cesa-Bianchi, Freund, Schapire 2002), over the arms on offer (sleeping arms);
//! * LinUCB, disjoint model per strategy (Li, Chu, Langford, Schapire 2010, Algorithm 1);
//! * linear Thompson sampling (Agrawal, Goyal 2013) on the same features;
//! * tabular Q-learning (Watkins 1989) and SARSA (Rummery, Niranjan 1994) in their semi-Markov form
//!   (discount gamma^energy, Bradtke, Duff 1995), epsilon-greedy;
//! * a small multilayer perceptron Q-function trained online with experience replay and a target network
//!   (DQN-style, Mnih et al. 2015), hand-written (no libraries).
//!
//! The bandits score an option as `listed_reward x index / estimated_energy` (reward per energy, the
//! measure of every table), where `index` is the method's own statistic of the success share
//! `reward / listed_reward`; they may skip a menu by the being's rule (best score below `skip_frac` times
//! the reward per energy it was told so far, after 40 energy), `skip_frac` being a tuned hyper-parameter
//! (0 = never skip). The value learners treat the skip as one more action.

use crate::being::solver::Strategy;
use std::collections::HashMap;

/// Family one-hot width of the feature vectors (World O has 50 slots, World v2 at most 48 families).
pub const FAM_DIM: usize = 64;
/// Visible type one-hot width (World v2 types 0..4, first-world domains 0..6, World L: 0).
pub const TYPE_DIM: usize = 8;
/// Energy spent before the skip rule may act (as the being's planner).
pub const SKIP_MIN_COST: f64 = 40.0;

/// One option as a player sees it: an (offer, strategy) pair, or the skip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opt {
    pub slot: usize,
    /// None: skip the menu.
    pub strategy: Option<Strategy>,
    pub fam: usize,
    pub ftype: u8,
    /// 1..=5 (1 where the world has no difficulty).
    pub diff: u8,
    pub listed_reward: f64,
    /// Energy this strategy spends on this offer (as listed: fast half, careful listed, deep twice).
    pub energy: f64,
    pub needs_token: bool,
}

impl Opt {
    pub fn skip(energy: f64) -> Opt {
        Opt { slot: usize::MAX, strategy: None, fam: 0, ftype: 0, diff: 1, listed_reward: 0.0, energy, needs_token: false }
    }
    pub fn is_skip(&self) -> bool {
        self.strategy.is_none()
    }
    fn s(&self) -> usize {
        self.strategy.map_or(3, |s| s.idx().min(2))
    }
}

/// What a player knows of its situation: all of it visible to the being too.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ctx {
    pub step: u64,
    pub budget_left: i64,
    /// Reward the player was told it earned so far (feedback noise included) and energy spent.
    pub told_reward: f64,
    pub spent: f64,
    /// A visible state id (World L: token held and the previous (family, result)); 0 elsewhere.
    pub state: u32,
    pub token: bool,
}

impl Ctx {
    pub fn rate(&self) -> f64 {
        if self.spent > 0.0 { self.told_reward / self.spent } else { 0.0 }
    }
}

pub trait Agent: Send {
    fn name(&self) -> String;
    /// Index into `opts` (never empty). Every option in `opts` can be paid.
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize;
    /// The outcome of the last choice as the player was told it (`reward` 0 on a reported failure).
    fn observe(&mut self, before: &Ctx, chosen: &Opt, reward: f64, cost: f64, after: &Ctx);
    /// A family slot was re-used for a new family (World O): forget what was learned about the slot.
    fn reset_fam(&mut self, _fam: usize) {}
}

// ---- random numbers (deterministic, seeded) ----

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x5EED_0F_7E57_0001)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
    /// Standard normal (Box-Muller).
    pub fn normal(&mut self) -> f64 {
        let u = self.f64().max(1e-300);
        let v = self.f64();
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
    /// Gamma(a, 1) (Marsaglia, Tsang 2000; a < 1 by the boost a+1 then U^(1/a)).
    pub fn gamma(&mut self, a: f64) -> f64 {
        if a < 1.0 {
            let u = self.f64().max(1e-300);
            return self.gamma(a + 1.0) * u.powf(1.0 / a);
        }
        let d = a - 1.0 / 3.0;
        let c = 1.0 / (9.0 * d).sqrt();
        loop {
            let x = self.normal();
            let v = (1.0 + c * x).powi(3);
            if v <= 0.0 {
                continue;
            }
            let u = self.f64().max(1e-300);
            if u.ln() < 0.5 * x * x + d - d * v + d * v.ln() {
                return d * v;
            }
        }
    }
    pub fn beta(&mut self, a: f64, b: f64) -> f64 {
        let x = self.gamma(a);
        let y = self.gamma(b);
        if x + y > 0.0 { x / (x + y) } else { 0.5 }
    }
}

// ---- arm statistics shared by the bandits ----

/// Granularity of a bandit arm: (family, difficulty, strategy) or (family, strategy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gran {
    Cell,
    Group,
}

impl Gran {
    pub fn name(self) -> &'static str {
        match self {
            Gran::Cell => "cell",
            Gran::Group => "group",
        }
    }
}

pub fn arm_key(o: &Opt, g: Gran) -> u64 {
    let d = if g == Gran::Cell { o.diff as u64 } else { 0 };
    ((o.fam as u64) << 16) | (d << 8) | o.s() as u64
}

fn key_fam(k: u64) -> usize {
    (k >> 16) as usize
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Stat {
    pub n: f64,
    /// Sum and sum of squares of the success share y = reward / listed reward.
    pub sy: f64,
    pub sy2: f64,
    pub cost: f64,
}

impl Stat {
    pub fn mean(&self) -> f64 {
        if self.n > 0.0 { self.sy / self.n } else { 0.0 }
    }
    /// Energy estimate: the observed mean energy shrunk to the listed energy with weight 1.
    pub fn energy(&self, listed: f64) -> f64 {
        ((self.cost + listed) / (self.n + 1.0)).max(1.0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Arms {
    pub m: HashMap<u64, Stat>,
    pub total: f64,
}

impl Arms {
    pub fn get(&self, k: u64) -> Stat {
        self.m.get(&k).copied().unwrap_or_default()
    }
    pub fn add(&mut self, k: u64, y: f64, cost: f64) {
        let s = self.m.entry(k).or_default();
        s.n += 1.0;
        s.sy += y;
        s.sy2 += y * y;
        s.cost += cost;
        self.total += 1.0;
    }
    pub fn reset_fam(&mut self, fam: usize) {
        let before = self.m.len();
        self.m.retain(|k, _| key_fam(*k) != fam);
        if self.m.len() != before {
            self.total = self.m.values().map(|s| s.n).sum();
        }
    }
}

fn success_share(o: &Opt, reward: f64) -> f64 {
    if o.listed_reward > 0.0 { (reward / o.listed_reward).clamp(0.0, 1.0) } else { 0.0 }
}

/// The being's skip rule applied to a player's own best score.
fn skip_or(best: usize, best_score: f64, cx: &Ctx, opts: &[Opt], skip_frac: f64) -> usize {
    if skip_frac > 0.0 && cx.spent >= SKIP_MIN_COST && best_score < skip_frac * cx.rate() {
        if let Some(i) = opts.iter().position(|o| o.is_skip()) {
            return i;
        }
    }
    best
}

/// argmax of `score` over the non-skip options (ties: first); None if there are none.
fn argmax(opts: &[Opt], mut score: impl FnMut(&Opt) -> f64) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (i, o) in opts.iter().enumerate() {
        if o.is_skip() {
            continue;
        }
        let s = score(o);
        if best.map_or(true, |b| s > b.1) {
            best = Some((i, s));
        }
    }
    best
}

fn skip_index(opts: &[Opt]) -> usize {
    opts.iter().position(|o| o.is_skip()).unwrap_or(0)
}

// ---- 1. epsilon-greedy ----

#[derive(Debug, Clone)]
pub struct EpsGreedy {
    pub c: f64,
    pub gran: Gran,
    pub skip_frac: f64,
    arms: Arms,
    rng: Rng,
    t: f64,
}

impl EpsGreedy {
    pub fn new(c: f64, gran: Gran, skip_frac: f64, seed: u64) -> EpsGreedy {
        EpsGreedy { c, gran, skip_frac, arms: Arms::default(), rng: Rng::new(seed ^ 0xE5), t: 0.0 }
    }
}

impl Agent for EpsGreedy {
    fn name(&self) -> String {
        format!("eps-greedy(c={},{},skip={})", self.c, self.gran.name(), self.skip_frac)
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        self.t += 1.0;
        let k = (self.arms.m.len() as f64).max(1.0);
        let eps = (self.c * k / self.t).min(1.0);
        let real: Vec<usize> = (0..opts.len()).filter(|i| !opts[*i].is_skip()).collect();
        if real.is_empty() {
            return skip_index(opts);
        }
        if self.rng.f64() < eps {
            return real[self.rng.below(real.len())];
        }
        let arms = &self.arms;
        let g = self.gran;
        let (b, s) = argmax(opts, |o| {
            let st = arms.get(arm_key(o, g));
            o.listed_reward * ((st.sy + 1.0) / (st.n + 2.0)) / st.energy(o.energy)
        })
        .expect("a real option");
        skip_or(b, s, cx, opts, self.skip_frac)
    }
    fn observe(&mut self, _b: &Ctx, o: &Opt, reward: f64, cost: f64, _a: &Ctx) {
        if !o.is_skip() {
            self.arms.add(arm_key(o, self.gran), success_share(o, reward), cost);
        }
    }
    fn reset_fam(&mut self, fam: usize) {
        self.arms.reset_fam(fam);
    }
}

// ---- 2. UCB1-tuned ----

#[derive(Debug, Clone)]
pub struct Ucb1Tuned {
    pub c: f64,
    pub gran: Gran,
    pub skip_frac: f64,
    arms: Arms,
}

impl Ucb1Tuned {
    pub fn new(c: f64, gran: Gran, skip_frac: f64) -> Ucb1Tuned {
        Ucb1Tuned { c, gran, skip_frac, arms: Arms::default() }
    }
}

impl Agent for Ucb1Tuned {
    fn name(&self) -> String {
        format!("ucb1-tuned(c={},{},skip={})", self.c, self.gran.name(), self.skip_frac)
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        let lt = (self.arms.total.max(1.0)).ln();
        let arms = &self.arms;
        let (g, c) = (self.gran, self.c);
        let Some((b, s)) = argmax(opts, |o| {
            let st = arms.get(arm_key(o, g));
            if st.n == 0.0 {
                // untried first (ordered by listed reward per energy)
                return 1e9 + o.listed_reward / o.energy;
            }
            let m = st.mean();
            let var = (st.sy2 / st.n - m * m).max(0.0) + (2.0 * lt / st.n).sqrt();
            let idx = m + c * (lt / st.n * var.min(0.25)).sqrt();
            o.listed_reward * idx.min(1.0) / st.energy(o.energy)
        }) else {
            return skip_index(opts);
        };
        if s >= 1e9 {
            return b;
        }
        skip_or(b, s, cx, opts, self.skip_frac)
    }
    fn observe(&mut self, _b: &Ctx, o: &Opt, reward: f64, cost: f64, _a: &Ctx) {
        if !o.is_skip() {
            self.arms.add(arm_key(o, self.gran), success_share(o, reward), cost);
        }
    }
    fn reset_fam(&mut self, fam: usize) {
        self.arms.reset_fam(fam);
    }
}

// ---- 3. KL-UCB ----

fn kl_bern(p: f64, q: f64) -> f64 {
    let e = 1e-12;
    let p = p.clamp(e, 1.0 - e);
    let q = q.clamp(e, 1.0 - e);
    p * (p / q).ln() + (1.0 - p) * ((1.0 - p) / (1.0 - q)).ln()
}

/// Largest q in [p, 1] with n KL(p, q) <= bound (bisection).
pub fn kl_upper(p: f64, n: f64, bound: f64) -> f64 {
    if n <= 0.0 {
        return 1.0;
    }
    let (mut lo, mut hi) = (p, 1.0);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if n * kl_bern(p, mid) > bound {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

#[derive(Debug, Clone)]
pub struct KlUcb {
    /// Multiplier of the log t exploration bound (1 = the paper's c = 0 variant).
    pub c: f64,
    pub gran: Gran,
    pub skip_frac: f64,
    arms: Arms,
}

impl KlUcb {
    pub fn new(c: f64, gran: Gran, skip_frac: f64) -> KlUcb {
        KlUcb { c, gran, skip_frac, arms: Arms::default() }
    }
}

impl Agent for KlUcb {
    fn name(&self) -> String {
        format!("kl-ucb(c={},{},skip={})", self.c, self.gran.name(), self.skip_frac)
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        let bound = self.c * self.arms.total.max(1.0).ln();
        let arms = &self.arms;
        let g = self.gran;
        let Some((b, s)) = argmax(opts, |o| {
            let st = arms.get(arm_key(o, g));
            if st.n == 0.0 {
                return 1e9 + o.listed_reward / o.energy;
            }
            o.listed_reward * kl_upper(st.mean(), st.n, bound) / st.energy(o.energy)
        }) else {
            return skip_index(opts);
        };
        if s >= 1e9 {
            return b;
        }
        skip_or(b, s, cx, opts, self.skip_frac)
    }
    fn observe(&mut self, _b: &Ctx, o: &Opt, reward: f64, cost: f64, _a: &Ctx) {
        if !o.is_skip() {
            self.arms.add(arm_key(o, self.gran), success_share(o, reward), cost);
        }
    }
    fn reset_fam(&mut self, fam: usize) {
        self.arms.reset_fam(fam);
    }
}

// ---- 4. Thompson sampling (Beta-Bernoulli) ----

#[derive(Debug, Clone)]
pub struct Thompson {
    pub prior: f64,
    pub gran: Gran,
    pub skip_frac: f64,
    arms: Arms,
    rng: Rng,
}

impl Thompson {
    pub fn new(prior: f64, gran: Gran, skip_frac: f64, seed: u64) -> Thompson {
        Thompson { prior, gran, skip_frac, arms: Arms::default(), rng: Rng::new(seed ^ 0x75) }
    }
}

impl Agent for Thompson {
    fn name(&self) -> String {
        format!("thompson(prior={},{},skip={})", self.prior, self.gran.name(), self.skip_frac)
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        let mut best: Option<(usize, f64)> = None;
        for (i, o) in opts.iter().enumerate() {
            if o.is_skip() {
                continue;
            }
            let st = self.arms.get(arm_key(o, self.gran));
            // Bernoulli trick (Agrawal, Goyal 2012): fractional successes as Beta pseudo-counts
            let th = self.rng.beta(self.prior + st.sy, self.prior + st.n - st.sy);
            let s = o.listed_reward * th / st.energy(o.energy);
            if best.map_or(true, |b| s > b.1) {
                best = Some((i, s));
            }
        }
        match best {
            Some((b, s)) => skip_or(b, s, cx, opts, self.skip_frac),
            None => skip_index(opts),
        }
    }
    fn observe(&mut self, _b: &Ctx, o: &Opt, reward: f64, cost: f64, _a: &Ctx) {
        if !o.is_skip() {
            self.arms.add(arm_key(o, self.gran), success_share(o, reward), cost);
        }
    }
    fn reset_fam(&mut self, fam: usize) {
        self.arms.reset_fam(fam);
    }
}

// ---- 5. EXP3 over the arms on offer ----

#[derive(Debug, Clone)]
pub struct Exp3 {
    pub gamma: f64,
    pub gran: Gran,
    logw: HashMap<u64, f64>,
    rng: Rng,
    /// Largest listed reward per energy seen (reward normalisation to [0, 1]).
    xmax: f64,
    last_p: f64,
    last_k: f64,
}

impl Exp3 {
    pub fn new(gamma: f64, gran: Gran, seed: u64) -> Exp3 {
        Exp3 { gamma, gran, logw: HashMap::new(), rng: Rng::new(seed ^ 0xE3), xmax: 1.0, last_p: 1.0, last_k: 1.0 }
    }
}

impl Agent for Exp3 {
    fn name(&self) -> String {
        format!("exp3(gamma={},{})", self.gamma, self.gran.name())
    }
    fn choose(&mut self, _cx: &Ctx, opts: &[Opt]) -> usize {
        let real: Vec<usize> = (0..opts.len()).filter(|i| !opts[*i].is_skip()).collect();
        if real.is_empty() {
            return skip_index(opts);
        }
        for &i in &real {
            self.xmax = self.xmax.max(opts[i].listed_reward / opts[i].energy);
        }
        let lw: Vec<f64> = real.iter().map(|&i| *self.logw.get(&arm_key(&opts[i], self.gran)).unwrap_or(&0.0)).collect();
        let mx = lw.iter().cloned().fold(f64::MIN, f64::max);
        let w: Vec<f64> = lw.iter().map(|x| (x - mx).exp()).collect();
        let sw: f64 = w.iter().sum();
        let k = real.len() as f64;
        let p: Vec<f64> = w.iter().map(|x| (1.0 - self.gamma) * x / sw + self.gamma / k).collect();
        let u = self.rng.f64();
        let mut acc = 0.0;
        let mut pick = real.len() - 1;
        for (j, pj) in p.iter().enumerate() {
            acc += pj;
            if u < acc {
                pick = j;
                break;
            }
        }
        self.last_p = p[pick];
        self.last_k = k;
        real[pick]
    }
    fn observe(&mut self, _b: &Ctx, o: &Opt, reward: f64, cost: f64, _a: &Ctx) {
        if o.is_skip() {
            return;
        }
        let x = (reward / cost.max(1.0) / self.xmax).clamp(0.0, 1.0);
        let xhat = x / self.last_p.max(1e-9);
        *self.logw.entry(arm_key(o, self.gran)).or_insert(0.0) += self.gamma * xhat / self.last_k;
    }
    fn reset_fam(&mut self, fam: usize) {
        self.logw.retain(|k, _| key_fam(*k) != fam);
    }
}

// ---- 6./7. linear models (disjoint per strategy) ----

/// Dense length of the linear feature vector.
pub const LIN_D: usize = 1 + FAM_DIM + TYPE_DIM + 5 + TYPE_DIM * 5 + 2;

/// Visible features of an offer (not of the strategy: the linear models are disjoint per strategy):
/// bias, family one-hot, type one-hot, difficulty one-hot, type x difficulty one-hot, needs-token, token held.
pub fn lin_features(o: &Opt, cx: &Ctx) -> Vec<(usize, f64)> {
    let d = (o.diff.clamp(1, 5) - 1) as usize;
    let t = (o.ftype as usize).min(TYPE_DIM - 1);
    let mut v = vec![(0, 1.0), (1 + o.fam % FAM_DIM, 1.0), (1 + FAM_DIM + t, 1.0), (1 + FAM_DIM + TYPE_DIM + d, 1.0), (1 + FAM_DIM + TYPE_DIM + 5 + t * 5 + d, 1.0)];
    if o.needs_token {
        v.push((LIN_D - 2, 1.0));
    }
    if cx.token {
        v.push((LIN_D - 1, 1.0));
    }
    v
}

/// Ridge regression with the inverse kept by Sherman-Morrison (lambda = 1).
#[derive(Debug, Clone)]
pub struct Ridge {
    pub d: usize,
    pub ainv: Vec<f64>,
    pub b: Vec<f64>,
    pub theta: Vec<f64>,
}

impl Ridge {
    pub fn new(d: usize) -> Ridge {
        let mut ainv = vec![0.0; d * d];
        for i in 0..d {
            ainv[i * d + i] = 1.0;
        }
        Ridge { d, ainv, b: vec![0.0; d], theta: vec![0.0; d] }
    }
    pub fn mean(&self, x: &[(usize, f64)]) -> f64 {
        x.iter().map(|(i, v)| self.theta[*i] * v).sum()
    }
    /// x' A^-1 x
    pub fn quad(&self, x: &[(usize, f64)]) -> f64 {
        let mut s = 0.0;
        for (i, vi) in x {
            for (j, vj) in x {
                s += vi * vj * self.ainv[i * self.d + j];
            }
        }
        s.max(0.0)
    }
    pub fn update(&mut self, x: &[(usize, f64)], y: f64) {
        let d = self.d;
        let mut u = vec![0.0; d];
        for r in 0..d {
            let row = &self.ainv[r * d..(r + 1) * d];
            u[r] = x.iter().map(|(i, v)| row[*i] * v).sum();
        }
        let den = 1.0 + x.iter().map(|(i, v)| u[*i] * v).sum::<f64>();
        for r in 0..d {
            if u[r] == 0.0 {
                continue;
            }
            let f = u[r] / den;
            let row = &mut self.ainv[r * d..(r + 1) * d];
            for c in 0..d {
                row[c] -= f * u[c];
            }
        }
        for (i, v) in x {
            self.b[*i] += v * y;
        }
        for r in 0..d {
            let row = &self.ainv[r * d..(r + 1) * d];
            self.theta[r] = row.iter().zip(&self.b).map(|(a, b)| a * b).sum();
        }
    }
    /// Lower Cholesky factor of A^-1 (for sampling).
    pub fn chol(&self) -> Vec<f64> {
        let d = self.d;
        let mut l = vec![0.0; d * d];
        for i in 0..d {
            for j in 0..=i {
                let mut s = self.ainv[i * d + j];
                for k in 0..j {
                    s -= l[i * d + k] * l[j * d + k];
                }
                if i == j {
                    l[i * d + i] = s.max(1e-12).sqrt();
                } else {
                    l[i * d + j] = s / l[j * d + j];
                }
            }
        }
        l
    }
}

#[derive(Debug, Clone)]
pub struct LinUcb {
    pub alpha: f64,
    pub skip_frac: f64,
    models: Vec<Ridge>,
}

impl LinUcb {
    pub fn new(alpha: f64, skip_frac: f64) -> LinUcb {
        LinUcb { alpha, skip_frac, models: vec![Ridge::new(LIN_D); 3] }
    }
    /// The score `choose` maximises for one (non-skip) option, with its mean and width (Track B13's fallback reads it).
    pub fn score(&self, cx: &Ctx, o: &Opt) -> (f64, f64, f64) {
        let x = lin_features(o, cx);
        let m = &self.models[o.s()];
        let (mean, w) = (m.mean(&x), m.quad(&x).sqrt());
        (o.listed_reward * (mean + self.alpha * w).clamp(0.0, 1.0) / o.energy, mean, w)
    }
}

impl Agent for LinUcb {
    fn name(&self) -> String {
        format!("linucb(alpha={},skip={})", self.alpha, self.skip_frac)
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        let models = &self.models;
        let a = self.alpha;
        let Some((b, s)) = argmax(opts, |o| {
            let x = lin_features(o, cx);
            let m = &models[o.s()];
            let p = (m.mean(&x) + a * m.quad(&x).sqrt()).clamp(0.0, 1.0);
            o.listed_reward * p / o.energy
        }) else {
            return skip_index(opts);
        };
        skip_or(b, s, cx, opts, self.skip_frac)
    }
    fn observe(&mut self, b: &Ctx, o: &Opt, reward: f64, _cost: f64, _a: &Ctx) {
        if !o.is_skip() {
            let x = lin_features(o, b);
            self.models[o.s()].update(&x, success_share(o, reward));
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinTs {
    pub v: f64,
    pub skip_frac: f64,
    models: Vec<Ridge>,
    rng: Rng,
}

impl LinTs {
    pub fn new(v: f64, skip_frac: f64, seed: u64) -> LinTs {
        LinTs { v, skip_frac, models: vec![Ridge::new(LIN_D); 3], rng: Rng::new(seed ^ 0x17) }
    }
}

impl Agent for LinTs {
    fn name(&self) -> String {
        format!("lin-thompson(v={},skip={})", self.v, self.skip_frac)
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        // one sample of theta per strategy model per step
        let mut th: Vec<Option<Vec<f64>>> = vec![None, None, None];
        let mut best: Option<(usize, f64)> = None;
        for (i, o) in opts.iter().enumerate() {
            if o.is_skip() {
                continue;
            }
            let s = o.s();
            if th[s].is_none() {
                let m = &self.models[s];
                let l = m.chol();
                let z: Vec<f64> = (0..m.d).map(|_| self.rng.normal()).collect();
                let mut t = m.theta.clone();
                for r in 0..m.d {
                    let lz: f64 = (0..=r).map(|k| l[r * m.d + k] * z[k]).sum();
                    t[r] += self.v * lz;
                }
                th[s] = Some(t);
            }
            let t = th[s].as_ref().expect("sampled");
            let x = lin_features(o, cx);
            let p = x.iter().map(|(j, v)| t[*j] * v).sum::<f64>().clamp(0.0, 1.0);
            let sc = o.listed_reward * p / o.energy;
            if best.map_or(true, |b| sc > b.1) {
                best = Some((i, sc));
            }
        }
        match best {
            Some((b, s)) => skip_or(b, s, cx, opts, self.skip_frac),
            None => skip_index(opts),
        }
    }
    fn observe(&mut self, b: &Ctx, o: &Opt, reward: f64, _cost: f64, _a: &Ctx) {
        if !o.is_skip() {
            let x = lin_features(o, b);
            self.models[o.s()].update(&x, success_share(o, reward));
        }
    }
}

// ---- 8. tabular Q-learning and SARSA (semi-Markov, discount per energy unit) ----

fn q_key(state: u32, o: &Opt) -> u64 {
    if o.is_skip() {
        return ((state as u64) << 40) | 0xFF_FFFF;
    }
    ((state as u64) << 40) | ((o.fam as u64) << 16) | ((o.diff as u64) << 8) | o.s() as u64
}

#[derive(Debug, Clone)]
pub struct QLearn {
    pub sarsa: bool,
    pub eps_c: f64,
    pub gamma: f64,
    pub alpha_min: f64,
    q: HashMap<u64, (f64, f64)>,
    rng: Rng,
    t: f64,
    /// (state, option, reward, cost, told rate before it) of the last choice, completed at the next choice.
    pending: Option<(u32, Opt, f64, f64, f64)>,
}

impl QLearn {
    pub fn new(sarsa: bool, eps_c: f64, gamma: f64, seed: u64) -> QLearn {
        QLearn { sarsa, eps_c, gamma, alpha_min: 0.05, q: HashMap::new(), rng: Rng::new(seed ^ if sarsa { 0x5A } else { 0x0C }), t: 0.0, pending: None }
    }
    fn qv(&self, state: u32, o: &Opt) -> f64 {
        self.q.get(&q_key(state, o)).map_or(0.0, |x| x.0)
    }
    fn update(&mut self, s: u32, o: &Opt, target: f64) {
        let e = self.q.entry(q_key(s, o)).or_insert((0.0, 0.0));
        e.1 += 1.0;
        let a = (1.0 / e.1).max(self.alpha_min);
        e.0 += a * (target - e.0);
    }
}

/// Reward scale of the value learners (rewards are integers up to about 40).
const RSCALE: f64 = 0.1;

/// The TD target. `gamma < 1`: discounted semi-Markov target r + gamma^energy V' (Bradtke, Duff 1995).
/// `gamma >= 1`: the average-reward target (r - rho x energy) + V' of R-learning (Schwartz 1993) in its
/// semi-Markov form (SMART, Das et al. 1999), rho = the reward per energy told so far.
fn td_target(r: f64, c: f64, rho: f64, gamma: f64, next: f64) -> f64 {
    if gamma >= 1.0 { (r - rho * c) * RSCALE + next } else { r * RSCALE + gamma.powf(c) * next }
}

fn gamma_name(g: f64) -> String {
    if g >= 1.0 { "avg-reward".into() } else { format!("{g}") }
}

impl Agent for QLearn {
    fn name(&self) -> String {
        format!("{}(eps={},gamma={})", if self.sarsa { "sarsa" } else { "q-learning" }, self.eps_c, gamma_name(self.gamma))
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        self.t += 1.0;
        let eps = (self.eps_c / self.t.sqrt()).min(1.0);
        let pick = if self.rng.f64() < eps {
            self.rng.below(opts.len())
        } else {
            let mut b = 0;
            for i in 1..opts.len() {
                if self.qv(cx.state, &opts[i]) > self.qv(cx.state, &opts[b]) {
                    b = i;
                }
            }
            b
        };
        if let Some((s, o, r, c, rho)) = self.pending.take() {
            let next = if self.sarsa { self.qv(cx.state, &opts[pick]) } else { opts.iter().map(|x| self.qv(cx.state, x)).fold(f64::MIN, f64::max) };
            self.update(s, &o, td_target(r, c, rho, self.gamma, next));
        }
        pick
    }
    fn observe(&mut self, b: &Ctx, o: &Opt, reward: f64, cost: f64, _a: &Ctx) {
        self.pending = Some((b.state, *o, reward, cost, b.rate()));
    }
    fn reset_fam(&mut self, fam: usize) {
        self.q.retain(|k, _| (k & 0xFF_FFFF) == 0xFF_FFFF || ((k >> 16) & 0xFF_FFFF) as usize != fam);
    }
}

// ---- 9. DQN-style MLP Q-function ----

/// Sparse input width of the MLP.
pub const MLP_IN: usize = 1 + 4 + FAM_DIM * 4 + TYPE_DIM * 5 * 4 + 4 + 4 + 2 + 256;

/// Sparse features of (visible state, option): bias, strategy (incl. skip), family x strategy,
/// type x difficulty x strategy, difficulty x strategy (value), listed reward per energy x strategy (value),
/// needs-token, token held, and (visible state, family, strategy) hashed into 256 buckets.
pub fn mlp_features(o: &Opt, cx: &Ctx) -> Vec<(usize, f64)> {
    let s = o.s();
    let mut v = vec![(0usize, 1.0), (1 + s, 1.0)];
    let mut at = 5;
    if !o.is_skip() {
        let d = (o.diff.clamp(1, 5) - 1) as usize;
        let t = (o.ftype as usize).min(TYPE_DIM - 1);
        v.push((at + (o.fam % FAM_DIM) * 4 + s, 1.0));
        at += FAM_DIM * 4;
        v.push((at + (t * 5 + d) * 4 + s, 1.0));
        at += TYPE_DIM * 5 * 4;
        v.push((at + s, (d + 1) as f64 / 5.0));
        at += 4;
        v.push((at + s, (o.listed_reward / o.energy) / 5.0));
        at += 4;
        if o.needs_token {
            v.push((at, 1.0));
        }
    } else {
        at += FAM_DIM * 4 + TYPE_DIM * 5 * 4 + 8;
    }
    if cx.token {
        v.push((at + 1, 1.0));
    }
    at += 2;
    if cx.state != 0 {
        let h = (cx.state as u64).wrapping_mul(0x9E37_79B9).wrapping_add((o.fam as u64) * 31 + s as u64) % 256;
        v.push((at + h as usize, 1.0));
    }
    v
}

#[derive(Debug, Clone)]
struct Net {
    h: usize,
    w1: Vec<f64>, // MLP_IN x h
    b1: Vec<f64>,
    w2: Vec<f64>,
    b2: f64,
}

impl Net {
    fn new(h: usize, rng: &mut Rng) -> Net {
        let s1 = (1.0 / 8.0f64).sqrt();
        let s2 = (1.0 / h as f64).sqrt();
        Net { h, w1: (0..MLP_IN * h).map(|_| rng.normal() * s1).collect(), b1: vec![0.0; h], w2: (0..h).map(|_| rng.normal() * s2).collect(), b2: 0.0 }
    }
    fn hidden(&self, x: &[(usize, f64)]) -> Vec<f64> {
        let mut z = self.b1.clone();
        for (i, v) in x {
            let row = &self.w1[i * self.h..(i + 1) * self.h];
            for k in 0..self.h {
                z[k] += row[k] * v;
            }
        }
        z.iter().map(|a| a.tanh()).collect()
    }
    fn q(&self, x: &[(usize, f64)]) -> f64 {
        let a = self.hidden(x);
        self.b2 + a.iter().zip(&self.w2).map(|(p, q)| p * q).sum::<f64>()
    }
    /// One SGD step on the Huber loss towards `y`.
    fn sgd(&mut self, x: &[(usize, f64)], y: f64, lr: f64) {
        let a = self.hidden(x);
        let q = self.b2 + a.iter().zip(&self.w2).map(|(p, q)| p * q).sum::<f64>();
        let g = (q - y).clamp(-1.0, 1.0);
        for k in 0..self.h {
            let gz = g * self.w2[k] * (1.0 - a[k] * a[k]);
            self.w2[k] -= lr * g * a[k];
            self.b1[k] -= lr * gz;
            for (i, v) in x {
                self.w1[i * self.h + k] -= lr * gz * v;
            }
        }
        self.b2 -= lr * g;
    }
}

#[derive(Debug, Clone)]
struct Trans {
    cx: Ctx,
    o: Opt,
    r: f64,
    c: f64,
    next_cx: Ctx,
    next: Vec<Opt>,
}

#[derive(Debug, Clone)]
pub struct Dqn {
    pub lr: f64,
    pub gamma: f64,
    pub eps_c: f64,
    pub hidden: usize,
    pub batch: usize,
    pub buffer: usize,
    pub sync_every: u64,
    net: Net,
    target: Net,
    buf: Vec<Trans>,
    rng: Rng,
    t: u64,
    pending: Option<(Ctx, Opt, f64, f64)>,
}

impl Dqn {
    pub fn new(lr: f64, gamma: f64, eps_c: f64, seed: u64) -> Dqn {
        let mut rng = Rng::new(seed ^ 0xD0);
        let net = Net::new(32, &mut rng);
        Dqn { lr, gamma, eps_c, hidden: 32, batch: 16, buffer: 2000, sync_every: 200, target: net.clone(), net, buf: vec![], rng, t: 0, pending: None }
    }
}

impl Agent for Dqn {
    fn name(&self) -> String {
        format!("mlp-dqn(lr={},gamma={},eps={})", self.lr, gamma_name(self.gamma), self.eps_c)
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        self.t += 1;
        if let Some((pcx, o, r, c)) = self.pending.take() {
            let tr = Trans { cx: pcx, o, r, c, next_cx: *cx, next: opts.to_vec() };
            if self.buf.len() < self.buffer {
                self.buf.push(tr);
            } else {
                let i = (self.t as usize) % self.buffer;
                self.buf[i] = tr;
            }
        }
        if self.buf.len() >= 64 {
            for _ in 0..self.batch {
                let tr = self.buf[self.rng.below(self.buf.len())].clone();
                let next = tr.next.iter().map(|x| self.target.q(&mlp_features(x, &tr.next_cx))).fold(f64::MIN, f64::max);
                let y = td_target(tr.r, tr.c, tr.cx.rate(), self.gamma, next);
                self.net.sgd(&mlp_features(&tr.o, &tr.cx), y, self.lr);
            }
        }
        if self.t % self.sync_every == 0 {
            self.target = self.net.clone();
        }
        let eps = (self.eps_c / (self.t as f64).sqrt()).min(1.0);
        if self.rng.f64() < eps {
            return self.rng.below(opts.len());
        }
        let mut b = 0;
        let mut bq = f64::MIN;
        for (i, o) in opts.iter().enumerate() {
            let q = self.net.q(&mlp_features(o, cx));
            if q > bq {
                bq = q;
                b = i;
            }
        }
        b
    }
    fn observe(&mut self, b: &Ctx, o: &Opt, reward: f64, cost: f64, _a: &Ctx) {
        self.pending = Some((*b, *o, reward, cost));
    }
}

// ---- the menu of methods ----

/// A method with its (tuned) hyper-parameters, as the harness names it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Method {
    EpsGreedy { c: f64, gran: Gran, skip: f64 },
    Ucb1Tuned { c: f64, gran: Gran, skip: f64 },
    KlUcb { c: f64, gran: Gran, skip: f64 },
    Thompson { prior: f64, gran: Gran, skip: f64 },
    Exp3 { gamma: f64, gran: Gran },
    LinUcb { alpha: f64, skip: f64 },
    LinTs { v: f64, skip: f64 },
    QLearn { eps: f64, gamma: f64 },
    Sarsa { eps: f64, gamma: f64 },
    Dqn { lr: f64, gamma: f64, eps: f64 },
}

impl Method {
    /// Short family name of the method (without hyper-parameters).
    pub fn family(&self) -> &'static str {
        match self {
            Method::EpsGreedy { .. } => "eps-greedy",
            Method::Ucb1Tuned { .. } => "UCB1-tuned",
            Method::KlUcb { .. } => "KL-UCB",
            Method::Thompson { .. } => "Thompson (Beta)",
            Method::Exp3 { .. } => "EXP3",
            Method::LinUcb { .. } => "LinUCB",
            Method::LinTs { .. } => "linear Thompson",
            Method::QLearn { .. } => "Q-learning",
            Method::Sarsa { .. } => "SARSA",
            Method::Dqn { .. } => "MLP Q (DQN-style)",
        }
    }
    pub fn agent(&self, seed: u64) -> Box<dyn Agent> {
        match *self {
            Method::EpsGreedy { c, gran, skip } => Box::new(EpsGreedy::new(c, gran, skip, seed)),
            Method::Ucb1Tuned { c, gran, skip } => Box::new(Ucb1Tuned::new(c, gran, skip)),
            Method::KlUcb { c, gran, skip } => Box::new(KlUcb::new(c, gran, skip)),
            Method::Thompson { prior, gran, skip } => Box::new(Thompson::new(prior, gran, skip, seed)),
            Method::Exp3 { gamma, gran } => Box::new(Exp3::new(gamma, gran, seed)),
            Method::LinUcb { alpha, skip } => Box::new(LinUcb::new(alpha, skip)),
            Method::LinTs { v, skip } => Box::new(LinTs::new(v, skip, seed)),
            Method::QLearn { eps, gamma } => Box::new(QLearn::new(false, eps, gamma, seed)),
            Method::Sarsa { eps, gamma } => Box::new(QLearn::new(true, eps, gamma, seed)),
            Method::Dqn { lr, gamma, eps } => Box::new(Dqn::new(lr, gamma, eps, seed)),
        }
    }
    pub fn label(&self) -> String {
        self.agent(0).name()
    }
}

/// The tuning grid of every method family (seeds 101..120 only).
pub fn grid() -> Vec<Vec<Method>> {
    let grans = [Gran::Cell, Gran::Group];
    let skips = [0.0, 1.0];
    let mut v = vec![];
    let mut g = vec![];
    for c in [0.1, 0.5, 2.0, 10.0] {
        for gran in grans {
            for skip in skips {
                g.push(Method::EpsGreedy { c, gran, skip });
            }
        }
    }
    v.push(std::mem::take(&mut g));
    for c in [0.1, 0.3, 1.0] {
        for gran in grans {
            for skip in skips {
                g.push(Method::Ucb1Tuned { c, gran, skip });
            }
        }
    }
    v.push(std::mem::take(&mut g));
    for c in [0.1, 0.3, 1.0] {
        for gran in grans {
            for skip in skips {
                g.push(Method::KlUcb { c, gran, skip });
            }
        }
    }
    v.push(std::mem::take(&mut g));
    for prior in [0.5, 1.0, 2.0] {
        for gran in grans {
            for skip in skips {
                g.push(Method::Thompson { prior, gran, skip });
            }
        }
    }
    v.push(std::mem::take(&mut g));
    for gamma in [0.01, 0.05, 0.1, 0.3] {
        for gran in grans {
            g.push(Method::Exp3 { gamma, gran });
        }
    }
    v.push(std::mem::take(&mut g));
    for alpha in [0.05, 0.2, 0.5, 1.0] {
        for skip in skips {
            g.push(Method::LinUcb { alpha, skip });
        }
    }
    v.push(std::mem::take(&mut g));
    for vv in [0.05, 0.2, 0.5, 1.0] {
        for skip in skips {
            g.push(Method::LinTs { v: vv, skip });
        }
    }
    v.push(std::mem::take(&mut g));
    for eps in [0.3, 1.0, 3.0] {
        for gamma in [0.9, 0.97, 0.995] {
            g.push(Method::QLearn { eps, gamma });
        }
    }
    v.push(std::mem::take(&mut g));
    for eps in [0.3, 1.0, 3.0] {
        for gamma in [0.9, 0.97, 0.995] {
            g.push(Method::Sarsa { eps, gamma });
        }
    }
    v.push(std::mem::take(&mut g));
    for lr in [0.003, 0.01, 0.03] {
        for gamma in [0.9, 0.97] {
            g.push(Method::Dqn { lr, gamma, eps: 1.0 });
        }
    }
    v.push(g);
    v
}

/// Second tuning pass: values beyond the edges the first grid chose (same seeds 101..120), and the
/// average-reward variant of the value learners (gamma = "avg-reward").
pub fn grid_ext() -> Vec<Vec<Method>> {
    let grans = [Gran::Cell, Gran::Group];
    let skips = [0.0, 1.0];
    let mut v = vec![];
    let mut g = vec![];
    for c in [0.01, 0.03] {
        for gran in grans {
            for skip in skips {
                g.push(Method::EpsGreedy { c, gran, skip });
            }
        }
    }
    v.push(std::mem::take(&mut g));
    for c in [0.03, 3.0] {
        for gran in grans {
            for skip in skips {
                g.push(Method::Ucb1Tuned { c, gran, skip });
            }
        }
    }
    v.push(std::mem::take(&mut g));
    for c in [0.01, 0.03, 3.0] {
        for gran in grans {
            for skip in skips {
                g.push(Method::KlUcb { c, gran, skip });
            }
        }
    }
    v.push(std::mem::take(&mut g));
    for prior in [0.1, 0.25, 4.0] {
        for gran in grans {
            for skip in skips {
                g.push(Method::Thompson { prior, gran, skip });
            }
        }
    }
    v.push(std::mem::take(&mut g));
    for gamma in [0.5, 0.7] {
        for gran in grans {
            g.push(Method::Exp3 { gamma, gran });
        }
    }
    v.push(std::mem::take(&mut g));
    for alpha in [0.02, 2.0] {
        for skip in skips {
            g.push(Method::LinUcb { alpha, skip });
        }
    }
    v.push(std::mem::take(&mut g));
    for eps in [0.1, 0.3, 1.0] {
        for gamma in [0.5, 0.7, 1.0] {
            g.push(Method::QLearn { eps, gamma });
        }
    }
    v.push(std::mem::take(&mut g));
    for eps in [0.1, 0.3, 1.0] {
        for gamma in [0.5, 0.7, 1.0] {
            g.push(Method::Sarsa { eps, gamma });
        }
    }
    v.push(std::mem::take(&mut g));
    for lr in [0.03, 0.1] {
        for gamma in [0.5, 0.7, 1.0] {
            g.push(Method::Dqn { lr, gamma, eps: 1.0 });
        }
    }
    v.push(g);
    v
}
