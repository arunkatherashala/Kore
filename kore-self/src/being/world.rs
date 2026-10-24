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
}

pub const DOMAINS: [Domain; 4] = [Domain::Arithmetic, Domain::Algebra, Domain::Physics, Domain::Pattern];

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
        }
    }
    pub fn parse(s: &str) -> Option<Domain> {
        DOMAINS.iter().copied().find(|d| d.name() == s)
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
}

impl Kind {
    pub fn of(d: Domain, difficulty: u8) -> Kind {
        use Kind::*;
        let k = difficulty.clamp(1, 5) as usize - 1;
        match d {
            Domain::Arithmetic => [ArithAdd, ArithAddSub, ArithMulSmall, ArithMulBig, ArithMulSum][k],
            Domain::Algebra => [AlgXPlusB, AlgAX, AlgAXPlusB, AlgBothSides, AlgParen][k],
            Domain::Physics => [PhysVelocity, PhysOhm, PhysDisplacement, PhysKinetic, PhysStopping][k],
            Domain::Pattern => [PatArith, PatGeom, PatQuad, PatFib, PatAlt][k],
        }
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
    pub reward_scale: [f64; 4],
    /// Cost of a difficulty-0 task; the task cost is `cost_base + difficulty` (2 .. 6).
    pub cost_base: [i64; 4],
    /// Difficulty = 1 + floor(u^bias * 5), u uniform in [0,1). Above 1 favours easy tasks (0.6 .. 1.8).
    pub diff_bias: [f64; 4],
}

impl Profile {
    pub fn from_seed(world_seed: u64) -> Profile {
        let mut r = Rng::new(mix(&[world_seed, 0xFAB1E]));
        let mut p = Profile { reward_scale: [1.0; 4], cost_base: [3; 4], diff_bias: [1.0; 4] };
        for i in 0..4 {
            p.reward_scale[i] = ((0.6 + r.f64()) * 100.0).round() / 100.0;
            p.cost_base[i] = r.range(2, 6);
            p.diff_bias[i] = ((0.6 + r.f64() * 1.2) * 100.0).round() / 100.0;
        }
        p
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
}

impl World {
    pub fn new(seed: u64) -> World {
        World { seed, profile: Profile::from_seed(seed) }
    }

    /// The world's per-attempt random source, handed to the solver as the
    /// stand-in for execution slips. A pure function of (seed, step, domain).
    pub fn noise(&self, step: u64, d: Domain) -> u64 {
        mix(&[self.seed, step, d.idx() as u64, 0xA11CE])
    }

    /// The task on offer in `domain` at `step`.
    pub fn task(&self, step: u64, d: Domain) -> Task {
        let mut r = Rng::new(mix(&[self.seed, step, d.idx() as u64, 0x7A5C]));
        let u = r.f64();
        let difficulty = (1.0 + (u.powf(self.profile.diff_bias[d.idx()]) * 5.0).floor()).clamp(1.0, 5.0) as u8;
        let kind = Kind::of(d, difficulty);
        let (params, prompt, expected) = generate(kind, &mut r);
        let k = difficulty as i64;
        let reward = ((self.profile.reward_scale[d.idx()] * (4.0 + 3.0 * k as f64)).round() as i64).max(1);
        let cost = self.profile.cost_base[d.idx()] + k;
        Task {
            view: TaskView { id: format!("{step}-{}", d.name()), step, domain: d, difficulty, kind, params, prompt, cost, reward },
            expected,
        }
    }

    /// One task per domain, in `DOMAINS` order.
    pub fn menu(&self, step: u64) -> Vec<Task> {
        DOMAINS.iter().map(|d| self.task(step, *d)).collect()
    }
}

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
