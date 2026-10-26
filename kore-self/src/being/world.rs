//! The simulated world: a seeded, deterministic stream of verifiable tasks.
//!
//! Everything here is author-defined. A task for (world seed, step, domain)
//! is a pure function of those three numbers, so two policies that look at
//! the same step see the same task and a run can be replayed from the log.
//!
//! Four domains: arithmetic, algebra (solve a linear equation for x), physics
//! formulas (all answers are integers in the stated units) and tiny pattern
//! prediction (next term of a sequence). Each task has a difficulty 1..=5, a
//! cost in energy units, a reward in points and an exact checker.
//!
//! The *hidden profile* (reward scale, base cost and difficulty mix per
//! domain) is derived from the world seed. The being never reads it; it can
//! only learn from the rewards it receives. Together with the fixed
//! capabilities of the solvers (`solver.rs`) it decides which domain pays best
//! in a given world. The preference the being ends up with therefore comes
//! from these defined rewards and nothing else.

/// splitmix64, seeded. Own copy so this module stands alone.
#[derive(Debug, Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform in 0..n (n > 0).
    pub fn below(&mut self, n: u64) -> i64 {
        (self.next_u64() % n) as i64
    }
    /// Uniform in lo..=hi.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64)
    }
}

fn mix(parts: &[u64]) -> u64 {
    let mut r = Rng::new(0x5EED_0F_B1E);
    let mut acc = 0u64;
    for p in parts {
        r.0 ^= p.wrapping_mul(0xA24B_AED4_963E_E407);
        acc ^= r.next_u64();
    }
    acc
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Domain {
    Arithmetic = 0,
    Algebra = 1,
    Physics = 2,
    Pattern = 3,
    /// Unit conversion. Only exists in a world created with `drift`: it appears in the menu from a scheduled time.
    Units = 4,
}

/// The four domains of every world from the start.
pub const DOMAINS: [Domain; 4] = [Domain::Arithmetic, Domain::Algebra, Domain::Physics, Domain::Pattern];
/// All domains that can ever exist (the fifth only appears in a drifting world).
pub const ALL_DOMAINS: [Domain; 5] = [Domain::Arithmetic, Domain::Algebra, Domain::Physics, Domain::Pattern, Domain::Units];
/// Number of domain slots in per-domain arrays.
pub const N_DOM: usize = 5;

impl Domain {
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Domain::Arithmetic => "arithmetic",
            Domain::Algebra => "algebra",
            Domain::Physics => "physics",
            Domain::Pattern => "pattern",
            Domain::Units => "units",
        }
    }
    pub fn parse(s: &str) -> Option<Domain> {
        ALL_DOMAINS.iter().copied().find(|d| d.name() == s)
    }
}

/// Template of a task. One template per (domain, difficulty).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ArithAdd,
    ArithAddSub,
    ArithMulSmall,
    ArithMulBig,
    ArithMulSum,
    AlgXPlusB,
    AlgAX,
    AlgAXPlusB,
    AlgBothSides,
    AlgParen,
    PhysVelocity,
    PhysOhm,
    PhysDisplacement,
    PhysKinetic,
    PhysStopping,
    PatArith,
    PatGeom,
    PatQuad,
    PatFib,
    PatAlt,
    // Extended pattern families (world option `pattern_families: extended`). The fixed
    // solvers have no exact method for any of them; the last two are outside the rule
    // grammar of `discover.rs` on purpose (negative controls).
    ExtAffGeom,
    ExtPoly3,
    ExtRec2,
    ExtTribo,
    ExtAlt,
    ExtPeriodic,
    NegProd,
    NegPrimes,
    // Unit conversion (domain `units`, drifting worlds only).
    UnitKm,
    UnitHm,
    UnitKmh,
    UnitHms,
    UnitTrip,
}

impl Kind {
    pub fn of(d: Domain, difficulty: u8) -> Kind {
        use Kind::*;
        let k = difficulty.clamp(1, 5) as usize - 1;
        match d {
            Domain::Units => [UnitKm, UnitHm, UnitKmh, UnitHms, UnitTrip][k],
            Domain::Arithmetic => [ArithAdd, ArithAddSub, ArithMulSmall, ArithMulBig, ArithMulSum][k],
            Domain::Algebra => [AlgXPlusB, AlgAX, AlgAXPlusB, AlgBothSides, AlgParen][k],
            Domain::Physics => [PhysVelocity, PhysOhm, PhysDisplacement, PhysKinetic, PhysStopping][k],
            Domain::Pattern => [PatArith, PatGeom, PatQuad, PatFib, PatAlt][k],
        }
    }

    /// Families that can appear at (domain, difficulty) in the extended pattern world.
    pub fn options_ext(d: Domain, difficulty: u8) -> Vec<Kind> {
        use Kind::*;
        if d != Domain::Pattern {
            return vec![Kind::of(d, difficulty)];
        }
        match difficulty.clamp(1, 5) {
            1 => vec![PatArith],
            2 => vec![PatGeom, ExtAffGeom],
            3 => vec![PatQuad, ExtPoly3],
            4 => vec![PatFib, ExtRec2, ExtTribo],
            _ => vec![ExtAlt, ExtPeriodic, NegProd, NegPrimes],
        }
    }

    /// True for the families the author wrote outside the rule grammar (negative controls).
    pub fn is_negative_control(self) -> bool {
        matches!(self, Kind::NegProd | Kind::NegPrimes)
    }
}

/// What a solver or thinker may see: no expected answer.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskView {
    pub id: String,
    pub step: u64,
    pub domain: Domain,
    pub difficulty: u8,
    pub kind: Kind,
    /// The numbers of the problem (meaning depends on `kind`, see `generate`).
    pub params: Vec<i64>,
    pub prompt: String,
    /// Energy units spent on an attempt, right or wrong.
    pub cost: i64,
    /// Points earned for an exactly right answer; nothing for a wrong one.
    pub reward: i64,
}

/// A task with its checker. `expected` is private: only `check` can use it.
#[derive(Debug, Clone)]
pub struct Task {
    pub view: TaskView,
    expected: i64,
}

impl Task {
    /// Exact checker.
    pub fn check(&self, answer: i64) -> bool {
        answer == self.expected
    }
    /// For the log and for tests; a thinker is never given this.
    pub fn expected(&self) -> i64 {
        self.expected
    }
}

/// Hidden per-domain profile derived from the world seed.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// Multiplier on the reward of every task in the domain (0.6 .. 1.6).
    pub reward_scale: [f64; N_DOM],
    /// Cost of a difficulty-0 task; the task cost is `cost_base + difficulty` (2 .. 6).
    pub cost_base: [i64; N_DOM],
    /// Difficulty = 1 + floor(u^bias * 5), u uniform in [0,1). Above 1 favours easy tasks (0.6 .. 1.8).
    pub diff_bias: [f64; N_DOM],
}

impl Profile {
    pub fn from_seed(world_seed: u64) -> Profile {
        let mut r = Rng::new(mix(&[world_seed, 0xFAB1E]));
        let mut p = Profile { reward_scale: [1.0; N_DOM], cost_base: [3; N_DOM], diff_bias: [1.0; N_DOM] };
        for i in 0..4 {
            p.reward_scale[i] = ((0.6 + r.f64()) * 100.0).round() / 100.0;
            p.cost_base[i] = r.range(2, 6);
            p.diff_bias[i] = ((0.6 + r.f64() * 1.2) * 100.0).round() / 100.0;
        }
        // the fifth domain (units) has its own stream, so the four original profiles are unchanged
        let mut u = Rng::new(mix(&[world_seed, 0xFAB1E, 0x0017]));
        p.reward_scale[4] = ((1.0 + u.f64()) * 100.0).round() / 100.0;
        p.cost_base[4] = u.range(2, 4);
        p.diff_bias[4] = ((0.6 + u.f64() * 1.2) * 100.0).round() / 100.0;
        p
    }

    /// The profile after the scheduled shift: physics pays twice as much and its difficulty mix turns
    /// easy, arithmetic becomes cheap (base cost 1).
    pub fn shifted(&self) -> Profile {
        let mut p = self.clone();
        p.reward_scale[Domain::Physics.idx()] = (self.reward_scale[Domain::Physics.idx()] * 2.0 * 100.0).round() / 100.0;
        p.diff_bias[Domain::Physics.idx()] = 1.8;
        p.cost_base[Domain::Arithmetic.idx()] = 1;
        p
    }
}

/// Fixed fractions of the initial budget at which the three scheduled changes happen
/// (new domain, profile shift, strategy degradation); each is moved by up to +-0.04 by the world seed.
pub const DRIFT_FRACTIONS: [f64; 3] = [0.25, 0.5, 0.75];
/// Slip rate (per mille) of the `fast` strategy after it degrades.
pub const FAST_DEGRADED_SLIP: u64 = 700;

/// Scheduled, hidden changes of a drifting world. Times are world-clock seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Drift {
    /// World-clock second at which each change takes effect: [new domain, profile shift, degradation].
    pub at: [i64; 3],
    pub shifted: Profile,
}

impl Drift {
    pub fn new(world_seed: u64, budget: i64, base: &Profile) -> Drift {
        let mut r = Rng::new(mix(&[world_seed, 0xD21F7]));
        let mut at = [0i64; 3];
        for (i, f) in DRIFT_FRACTIONS.iter().enumerate() {
            let frac = f + (r.f64() * 2.0 - 1.0) * 0.04;
            at[i] = (budget as f64 * frac).round() as i64 * SECS_PER_ENERGY;
        }
        Drift { at, shifted: base.shifted() }
    }
}

/// Seconds of world time per energy unit of a task.
pub const SECS_PER_ENERGY: i64 = 30;
/// World clock origin (2030-01-01T00:00:00Z) as days since 1970-01-01.
const EPOCH_DAYS: i64 = 21_915;

/// ISO-8601 UTC for `secs` seconds after the world origin.
pub fn iso_at(secs: i64) -> String {
    let total = EPOCH_DAYS * 86_400 + secs.max(0);
    let (days, rem) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[derive(Debug, Clone)]
pub struct World {
    pub seed: u64,
    pub profile: Profile,
    /// Pattern tasks come from the extended hidden families (default off: all earlier results are unchanged).
    pub extended: bool,
    /// Multiplier on the reward of pattern tasks in the extended world (EXT_PATTERN_BONUS; 1 in the standard world).
    pub pattern_bonus: i64,
    /// Scheduled changes (default None: the world is stationary and every earlier result is unchanged).
    pub drift: Option<Drift>,
}

/// In the extended world pattern puzzles pay this many times the usual reward (author-defined; it gives
/// the discovered rules something to be worth). Visible in the listed reward of every pattern task.
pub const EXT_PATTERN_BONUS: i64 = 2;

impl World {
    pub fn new(seed: u64) -> World {
        World { seed, profile: Profile::from_seed(seed), extended: false, pattern_bonus: 1, drift: None }
    }

    pub fn with_extended(seed: u64, extended: bool) -> World {
        World { extended, pattern_bonus: if extended { EXT_PATTERN_BONUS } else { 1 }, ..World::new(seed) }
    }

    /// A world with scheduled changes; the times scale with the being's initial `budget`.
    pub fn with_drift(seed: u64, extended: bool, drift_budget: Option<i64>) -> World {
        let mut w = World::with_extended(seed, extended);
        if let Some(b) = drift_budget {
            w.drift = Some(Drift::new(seed, b, &w.profile));
        }
        w
    }

    /// Number of scheduled changes that have taken effect at world-clock second `clock` (0 without drift).
    pub fn regime(&self, clock: i64) -> usize {
        self.drift.as_ref().map_or(0, |d| d.at.iter().filter(|t| clock >= **t).count())
    }

    /// The `fast` strategy slips much more from the third change on.
    pub fn degraded(&self, clock: i64) -> bool {
        self.regime(clock) >= 3
    }

    fn profile_at(&self, clock: i64) -> &Profile {
        match &self.drift {
            Some(d) if self.regime(clock) >= 2 => &d.shifted,
            _ => &self.profile,
        }
    }

    /// The world's per-attempt random source, handed to the solver as the
    /// stand-in for execution slips. A pure function of (seed, step, domain).
    pub fn noise(&self, step: u64, d: Domain) -> u64 {
        mix(&[self.seed, step, d.idx() as u64, 0xA11CE])
    }

    /// The task on offer in `domain` at `step` (single-task mode).
    pub fn task(&self, step: u64, d: Domain) -> Task {
        self.build(step, d, 0, None, None, format!("{step}-{}", d.name()), &self.profile)
    }

    /// One task per domain, in `DOMAINS` order (single-task mode).
    pub fn menu(&self, step: u64) -> Vec<Task> {
        DOMAINS.iter().map(|d| self.task(step, *d)).collect()
    }

    /// Menu mode: `MENU_SIZE` candidate tasks at `step`. Slot j has a domain
    /// drawn from the world seed (repeats allowed); difficulty, cost and
    /// reward follow the hidden profile as in single-task mode.
    pub fn offers(&self, step: u64) -> Vec<Task> {
        self.offers_at(step, 0)
    }

    /// As `offers`, at world-clock second `clock`: in a drifting world the menu, the profile and
    /// the reward of the offers depend on which scheduled changes have already happened.
    pub fn offers_at(&self, step: u64, clock: i64) -> Vec<Task> {
        let regime = self.regime(clock);
        let n: u64 = if regime >= 1 { 5 } else { 4 };
        (0..MENU_SIZE as u64)
            .map(|j| {
                let d = ALL_DOMAINS[(mix(&[self.seed, step, j, 0xD0_0D]) % n) as usize];
                self.build(step, d, j + 1, None, None, format!("{step}-{j}-{}", d.name()), self.profile_at(clock))
            })
            .collect()
    }

    /// Per-attempt random source for menu slot `slot` (execution slips).
    pub fn menu_noise(&self, step: u64, slot: usize) -> u64 {
        mix(&[self.seed, step, slot as u64, 0xA11CE, 0x51_07])
    }

    /// A task of a given difficulty, used only by the measurement's oracle to
    /// estimate hidden truth; no policy under test calls it.
    pub fn task_forced(&self, step: u64, d: Domain, difficulty: u8) -> Task {
        self.build(step, d, 0x77, Some(difficulty), None, format!("forced-{step}-{}", d.name()), &self.profile)
    }

    /// A pattern task of one given hidden family (extended world), for measurement only.
    pub fn task_of_kind(&self, step: u64, kind: Kind) -> Task {
        let difficulty = (1..=5u8).find(|k| Kind::options_ext(Domain::Pattern, *k).contains(&kind)).unwrap_or(1);
        self.build(step, Domain::Pattern, 0x99 + kind as u64, Some(difficulty), Some(kind), format!("kind-{step}-{}", kind as u64), &self.profile)
    }

    fn build(&self, step: u64, d: Domain, salt: u64, forced: Option<u8>, force_kind: Option<Kind>, id: String, prof: &Profile) -> Task {
        let mut r = if salt == 0 {
            Rng::new(mix(&[self.seed, step, d.idx() as u64, 0x7A5C]))
        } else {
            Rng::new(mix(&[self.seed, step, d.idx() as u64, 0x7A5C, salt]))
        };
        let u = r.f64();
        let difficulty = forced.unwrap_or_else(|| (1.0 + (u.powf(prof.diff_bias[d.idx()]) * 5.0).floor()).clamp(1.0, 5.0) as u8);
        let kind = if let Some(k) = force_kind {
            k
        } else if self.extended && d == Domain::Pattern {
            let opts = Kind::options_ext(d, difficulty);
            opts[r.below(opts.len() as u64) as usize]
        } else {
            Kind::of(d, difficulty)
        };
        let (params, prompt, expected) = generate(kind, &mut r);
        let k = difficulty as i64;
        let bonus = if d == Domain::Pattern { self.pattern_bonus as f64 } else { 1.0 };
        let reward = ((prof.reward_scale[d.idx()] * (4.0 + 3.0 * k as f64) * bonus).round() as i64).max(1);
        let cost = prof.cost_base[d.idx()] + k;
        Task { view: TaskView { id, step, domain: d, difficulty, kind, params, prompt, cost, reward }, expected }
    }
}

/// Candidate tasks per step in menu mode.
pub const MENU_SIZE: usize = 4;
/// Energy spent by skipping the whole menu.
pub const SKIP_COST: i64 = 1;

fn nz(r: &mut Rng, lo: i64, hi: i64) -> i64 {
    loop {
        let v = r.range(lo, hi);
        if v != 0 {
            return v;
        }
    }
}

/// Params, prompt and the exact answer of one task. Meaning of `params`:
/// ArithAdd [a,b]; ArithAddSub [a,b,sign]; MulSmall/MulBig [a,b]; MulSum [a,b,c,d] (a*b+c*d);
/// AlgXPlusB [b,c] (x+b=c); AlgAX [a,c] (a*x=c); AlgAXPlusB [a,b,c]; AlgBothSides [a,b,c,d] (a*x+b=c*x+d);
/// AlgParen [a,b,c,d] (a*(x+b)-c*x=d); PhysVelocity [u,a,t]; PhysOhm [i,r]; PhysDisplacement [u,a,t];
/// PhysKinetic [m,v]; PhysStopping [u,a]; Pat* = the given terms.
fn generate(kind: Kind, r: &mut Rng) -> (Vec<i64>, String, i64) {
    use Kind::*;
    match kind {
        ArithAdd => {
            let (a, b) = (r.range(1, 9), r.range(1, 9));
            (vec![a, b], format!("{a} + {b} = ?"), a + b)
        }
        ArithAddSub => {
            let (a, b, s) = (r.range(10, 99), r.range(10, 99), if r.below(2) == 0 { 1 } else { -1 });
            (vec![a, b, s], format!("{a} {} {b} = ?", if s > 0 { "+" } else { "-" }), a + s * b)
        }
        ArithMulSmall => {
            let (a, b) = (r.range(11, 99), r.range(2, 9));
            (vec![a, b], format!("{a} * {b} = ?"), a * b)
        }
        ArithMulBig => {
            let (a, b) = (r.range(100, 999), r.range(11, 99));
            (vec![a, b], format!("{a} * {b} = ?"), a * b)
        }
        ArithMulSum => {
            let (a, b, c, d) = (r.range(10, 99), r.range(10, 99), r.range(10, 99), r.range(10, 99));
            (vec![a, b, c, d], format!("{a} * {b} + {c} * {d} = ?"), a * b + c * d)
        }
        AlgXPlusB => {
            let (x, b) = (r.range(-20, 20), r.range(1, 20));
            (vec![b, x + b], format!("Solve for x: x + {b} = {}", x + b), x)
        }
        AlgAX => {
            let (a, x) = (r.range(2, 9), nz(r, -9, 9));
            (vec![a, a * x], format!("Solve for x: {a}*x = {}", a * x), x)
        }
        AlgAXPlusB => {
            let (a, x, b) = (r.range(2, 9), r.range(-12, 12), r.range(-20, 20));
            (vec![a, b, a * x + b], format!("Solve for x: {a}*x + {b} = {}", a * x + b), x)
        }
        AlgBothSides => {
            let a = r.range(3, 9);
            let c = r.range(2, a - 1);
            let (x, b) = (r.range(-12, 12), r.range(-20, 20));
            let d = a * x + b - c * x;
            (vec![a, b, c, d], format!("Solve for x: {a}*x + {b} = {c}*x + {d}"), x)
        }
        AlgParen => {
            let a = r.range(3, 9);
            let c = r.range(1, a - 1);
            let (x, b) = (r.range(-40, 40), r.range(-9, 9));
            let d = a * (x + b) - c * x;
            (vec![a, b, c, d], format!("Solve for x: {a}*(x + {b}) - {c}*x = {d}"), x)
        }
        PhysVelocity => {
            let (u, a, t) = (r.range(0, 20), r.range(1, 9), r.range(1, 10));
            (vec![u, a, t], format!("A body starts at {u} m/s and accelerates at {a} m/s^2 for {t} s. Final speed in m/s?"), u + a * t)
        }
        PhysOhm => {
            let (i, rr) = (r.range(1, 9), r.range(2, 50));
            (vec![i, rr], format!("A current of {i} A flows through a {rr} ohm resistor. Voltage in V?"), i * rr)
        }
        PhysDisplacement => {
            let (u, a, t) = (r.range(0, 15), 2 * r.range(1, 5), r.range(1, 10));
            (vec![u, a, t], format!("A body starts at {u} m/s and accelerates at {a} m/s^2 for {t} s. Distance covered in m?"), u * t + a * t * t / 2)
        }
        PhysKinetic => {
            let (m, v) = (2 * r.range(1, 10), r.range(1, 15));
            (vec![m, v], format!("A {m} kg body moves at {v} m/s. Kinetic energy in J?"), m * v * v / 2)
        }
        PhysStopping => {
            let (a, j) = (r.range(1, 5), r.range(1, 6));
            let u = 2 * a * j;
            (vec![u, a], format!("A body at {u} m/s brakes at a constant {a} m/s^2 until it stops. Stopping distance in m?"), 2 * a * j * j)
        }
        PatArith => {
            let (s, d) = (r.range(-10, 20), nz(r, -5, 9));
            let t: Vec<i64> = (0..5).map(|i| s + d * i).collect();
            let next = s + d * 5;
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        PatGeom => {
            let (s, q) = (r.range(1, 5), r.range(2, 4));
            let t: Vec<i64> = (0..5).map(|i| s * q.pow(i as u32)).collect();
            let next = s * q.pow(5);
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        PatQuad => {
            let (a, b, c) = (r.range(1, 3), r.range(-4, 6), r.range(-5, 9));
            let f = |n: i64| a * n * n + b * n + c;
            let t: Vec<i64> = (1..=5).map(f).collect();
            (t.clone(), format!("Next term: {}", join(&t)), f(6))
        }
        PatFib => {
            let (p, q) = (r.range(1, 5), r.range(1, 5));
            let mut t = vec![p, q];
            while t.len() < 6 {
                let n = t[t.len() - 1] + t[t.len() - 2];
                t.push(n);
            }
            let next = t[4] + t[5];
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        ExtAffGeom => {
            let (s, p, c) = (r.range(1, 5), [2, 3, 4, -2][r.below(4) as usize], nz(r, -5, 9));
            let mut t = vec![s];
            while t.len() < 7 {
                let n = p * t[t.len() - 1] + c;
                t.push(n);
            }
            let next = t[6];
            t.truncate(6);
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        ExtPoly3 => {
            let (a, b, c, d) = (r.range(1, 2), r.range(-3, 3), r.range(-5, 5), r.range(-5, 9));
            let f = |n: i64| a * n * n * n + b * n * n + c * n + d;
            let t: Vec<i64> = (1..=7).map(f).collect();
            (t.clone(), format!("Next term: {}", join(&t)), f(8))
        }
        ExtRec2 => {
            let (mut p, mut q) = (r.range(1, 3), nz(r, -2, 3));
            if (p, q) == (1, 1) {
                (p, q) = (2, 1);
            }
            let mut t = vec![r.range(1, 5), r.range(1, 5)];
            while t.len() < 8 {
                let n = p * t[t.len() - 1] + q * t[t.len() - 2];
                t.push(n);
            }
            let next = t[7];
            t.truncate(7);
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        ExtTribo => {
            let mut t = vec![r.range(1, 4), r.range(1, 4), r.range(1, 4)];
            while t.len() < 10 {
                let n = t[t.len() - 1] + t[t.len() - 2] + t[t.len() - 3];
                t.push(n);
            }
            let next = t[9];
            t.truncate(9);
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        ExtAlt => {
            let a = r.range(1, 9);
            let mut b = r.range(-4, 9);
            if b == a {
                b = a % 9 + 1;
            }
            let mut t = vec![r.range(0, 10)];
            for i in 0..7 {
                let n = t[i] + if i % 2 == 0 { a } else { b };
                t.push(n);
            }
            let next = t[7];
            t.truncate(7);
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        ExtPeriodic => {
            let cyc = [r.range(1, 9), r.range(1, 9), r.range(1, 9)];
            let t: Vec<i64> = (0..8).map(|i| cyc[i % 3]).collect();
            let next = t[7];
            let t = t[..7].to_vec();
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        NegProd => {
            let mut t = vec![r.range(2, 3), r.range(2, 3)];
            while t.len() < 7 {
                let n = t[t.len() - 1] * t[t.len() - 2];
                t.push(n);
            }
            let next = t[6];
            t.truncate(6);
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
        NegPrimes => {
            const P: [i64; 20] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71];
            let i0 = r.range(0, 12) as usize;
            let t = P[i0..i0 + 6].to_vec();
            (t.clone(), format!("Next term: {}", join(&t)), P[i0 + 6])
        }
        UnitKm => {
            let k = r.range(2, 90);
            (vec![k], format!("Convert {k} km to metres."), k * 1000)
        }
        UnitHm => {
            let (h, m) = (r.range(1, 9), r.range(1, 59));
            (vec![h, m], format!("How many minutes are {h} h {m} min?"), 60 * h + m)
        }
        UnitKmh => {
            let v = 18 * r.range(1, 12);
            (vec![v], format!("Convert {v} km/h to m/s."), v * 5 / 18)
        }
        UnitHms => {
            let (h, m, s) = (r.range(1, 5), r.range(1, 59), r.range(1, 59));
            (vec![h, m, s], format!("How many seconds are {h} h {m} min {s} s?"), 3600 * h + 60 * m + s)
        }
        UnitTrip => {
            let (v, t) = (6 * r.range(2, 15), r.range(2, 45));
            (vec![v, t], format!("A car drives at {v} km/h for {t} minutes. Distance in metres?"), v * 1000 * t / 60)
        }
        PatAlt => {
            let a = r.range(1, 9);
            let mut b = r.range(1, 9);
            if b == a {
                b = a % 9 + 1;
            }
            let mut t = vec![r.range(0, 10)];
            for i in 0..4 {
                let n = t[i] + if i % 2 == 0 { a } else { b };
                t.push(n);
            }
            let next = t[4] + a;
            (t.clone(), format!("Next term: {}", join(&t)), next)
        }
    }
}

fn join(t: &[i64]) -> String {
    t.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
}
