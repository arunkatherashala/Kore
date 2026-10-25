//! Solvers and the `Thinker` seam.
//!
//! `Thinker::propose(&Context) -> Proposal` is the one interface the loop
//! calls to get an answer for a task. `ScriptedThinker` is the offline
//! baseline: fixed, documented strategies, no model, no randomness except the
//! world's per-attempt `noise`. THIS IS THE SEAM where a language-model-backed
//! thinker can be plugged in later (stage S3 of the plan): it would implement
//! the same trait, and the loop would still check every answer with the
//! world's exact checker. No network call exists in this crate for it.
//!
//! The scripted strategies are deliberately imperfect, in a documented way
//! that depends on the template (so on the difficulty):
//!
//! | domain | exact for | otherwise (heuristic) |
//! |---|---|---|
//! | arithmetic | additions, two-digit add/sub, two-digit x one-digit | operands rounded to 2 significant digits, then computed |
//! | algebra | x+b=c, a*x=c, a*x+b=c | trial search over x in -10..=10, keep the x with the smallest residual |
//! | physics | v=u+a*t and V=I*R | product of all the numbers in the problem |
//! | pattern | arithmetic, geometric, quadratic and fibonacci-like sequences (structure is detected) | last term plus last difference |
//!
//! On top of that each domain has an execution slip rate (per mille, below):
//! when the world's noise falls under it, an exact answer is off by one.
//! These capabilities are hidden from the chooser; it can only observe results.

use super::discover::{self, Rule};
use super::state::DomainStat;
use super::world::{Domain, Kind, TaskView};

/// Slip rate per mille by domain (arithmetic, algebra, physics, pattern).
pub const SLIP_PER_MILLE: [u64; 4] = [80, 50, 100, 30];
/// Self-reported confidence of the scripted thinker (author-set, not calibrated).
pub const CONF_EXACT: f64 = 0.9;
pub const CONF_HEURISTIC: f64 = 0.3;

/// What a thinker sees for one attempt. Never the expected answer.
pub struct Context<'a> {
    pub step: u64,
    pub task: &'a TaskView,
    /// The world's per-attempt random source (execution slips).
    pub noise: u64,
    /// What the being has observed so far in each domain, from its own log.
    pub stats: &'a [DomainStat; 4],
    /// Rules the being has accepted into its own log (empty unless discovery ran).
    pub rules: &'a [Rule],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub answer: i64,
    /// The thinker's own estimate that the answer is right.
    pub confidence: f64,
    /// Short name of the strategy used (goes into the log).
    pub method: String,
    /// Id of the discovered rule that produced the answer, if one did.
    pub rule_id: Option<String>,
}

pub trait Thinker {
    fn id(&self) -> &str;
    fn propose(&self, ctx: &Context) -> Proposal;
    /// Menu mode: answer with a chosen strategy. The default ignores it.
    fn propose_with(&self, ctx: &Context, _strategy: Strategy) -> Proposal {
        self.propose(ctx)
    }
}

/// Solver strategies of menu mode, with different cost/reliability trade-offs.
///
/// | strategy | energy spent | capability | slip rate |
/// |---|---|---|---|
/// | fast | half the listed cost (rounded up) | exact only for difficulty 1-2, a cheap heuristic above | 3x the domain rate (max 40%) |
/// | careful | the listed cost | the scripted table above | the domain rate |
/// | deep | twice the listed cost | exact for every template of the standard world | half the domain rate |
/// | discovered | the listed cost | applies the being's own accepted rule when one fits the prefix, else behaves like careful (extended pattern world only) | the domain rate |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Strategy {
    Fast = 0,
    Careful = 1,
    Deep = 2,
    Discovered = 3,
}

/// The strategies of the standard menu world.
pub const STRATEGIES: [Strategy; 3] = [Strategy::Fast, Strategy::Careful, Strategy::Deep];
/// The strategies of a being created with `pattern_families: extended`.
pub const STRATEGIES_EXT: [Strategy; 4] = [Strategy::Fast, Strategy::Careful, Strategy::Deep, Strategy::Discovered];

impl Strategy {
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Strategy::Fast => "fast",
            Strategy::Careful => "careful",
            Strategy::Deep => "deep",
            Strategy::Discovered => "discovered",
        }
    }
    pub fn parse(s: &str) -> Option<Strategy> {
        STRATEGIES_EXT.iter().copied().find(|x| x.name() == s)
    }
    /// Energy actually spent on an attempt whose listed cost is `listed`.
    pub fn energy(self, listed: i64) -> i64 {
        match self {
            Strategy::Fast => ((listed + 1) / 2).max(1),
            Strategy::Careful | Strategy::Discovered => listed,
            Strategy::Deep => 2 * listed,
        }
    }
    fn slip(self, domain: usize) -> u64 {
        let base = SLIP_PER_MILLE[domain];
        match self {
            Strategy::Fast => (3 * base).min(400),
            Strategy::Careful | Strategy::Discovered => base,
            Strategy::Deep => base / 2,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScriptedThinker;

impl Thinker for ScriptedThinker {
    fn id(&self) -> &str {
        "scripted-1"
    }
    fn propose(&self, ctx: &Context) -> Proposal {
        let t = ctx.task;
        let (mut answer, exact, method) = solve(t);
        if exact && ctx.noise % 1000 < SLIP_PER_MILLE[t.domain.idx()] {
            answer += 1;
            return Proposal { answer, confidence: CONF_EXACT, method: format!("{method}+slip"), rule_id: None };
        }
        Proposal { answer, confidence: if exact { CONF_EXACT } else { CONF_HEURISTIC }, method, rule_id: None }
    }
    fn propose_with(&self, ctx: &Context, strategy: Strategy) -> Proposal {
        let t = ctx.task;
        let mut rule_id = None;
        let (mut answer, exact, method) = match strategy {
            Strategy::Careful => solve(t),
            Strategy::Discovered => match discover::apply(ctx.rules, t.difficulty, &t.params).filter(|_| t.domain == Domain::Pattern) {
                Some((v, r)) => {
                    rule_id = Some(r.id.clone());
                    (v, true, format!("rule:{}:{}", r.id, r.form.code()))
                }
                None => {
                    let (a, e, m) = solve(t);
                    (a, e, format!("fallback:{m}"))
                }
            },
            Strategy::Fast if t.difficulty <= 2 => (exact_answer(t), true, "exact:easy".to_string()),
            Strategy::Fast => (heuristic(t), false, "heuristic:cheap".to_string()),
            Strategy::Deep => (exact_answer(t), true, "exact:deep".to_string()),
        };
        let method = format!("{}/{method}", strategy.name());
        if exact && ctx.noise % 1000 < strategy.slip(t.domain.idx()) {
            answer += 1;
            return Proposal { answer, confidence: CONF_EXACT, method: format!("{method}+slip"), rule_id };
        }
        Proposal { answer, confidence: if exact { CONF_EXACT } else { CONF_HEURISTIC }, method, rule_id }
    }
}

/// The correct answer, computed from the problem's numbers (what the deep strategy does).
fn exact_answer(t: &TaskView) -> i64 {
    use Kind::*;
    let p = &t.params;
    match t.kind {
        ArithAdd => p[0] + p[1],
        ArithAddSub => p[0] + p[2] * p[1],
        ArithMulSmall | ArithMulBig => p[0] * p[1],
        ArithMulSum => p[0] * p[1] + p[2] * p[3],
        AlgXPlusB => p[1] - p[0],
        AlgAX => p[1] / p[0],
        AlgAXPlusB => (p[2] - p[1]) / p[0],
        AlgBothSides => (p[3] - p[1]) / (p[0] - p[2]),
        AlgParen => (p[3] - p[0] * p[1]) / (p[0] - p[2]),
        PhysVelocity => p[0] + p[1] * p[2],
        PhysOhm => p[0] * p[1],
        PhysDisplacement => p[0] * p[2] + p[1] * p[2] * p[2] / 2,
        PhysKinetic => p[0] * p[1] * p[1] / 2,
        PhysStopping => p[0] * p[0] / (2 * p[1]),
        PatArith | PatGeom | PatQuad | PatFib => pattern(p).0,
        PatAlt => p[p.len() - 1] + (p[p.len() - 2] - p[p.len() - 3]),
        // no fixed method knows these families
        ExtAffGeom | ExtPoly3 | ExtRec2 | ExtTribo | ExtAlt | ExtPeriodic | NegProd | NegPrimes => last_difference(p),
    }
}

fn last_difference(p: &[i64]) -> i64 {
    let n = p.len();
    p[n - 1] + (p[n - 1] - p[n - 2])
}

/// The cheap approximation of the fast strategy above difficulty 2.
fn heuristic(t: &TaskView) -> i64 {
    use Kind::*;
    let p = &t.params;
    match t.kind {
        ArithMulSmall | ArithMulBig => round_2sf(p[0]) * round_2sf(p[1]),
        ArithMulSum => round_2sf(p[0]) * round_2sf(p[1]) + round_2sf(p[2]) * round_2sf(p[3]),
        AlgAXPlusB => search(|x| p[0] * x + p[1] - p[2]),
        AlgBothSides => search(|x| p[0] * x + p[1] - (p[2] * x + p[3])),
        AlgParen => search(|x| p[0] * (x + p[1]) - p[2] * x - p[3]),
        PhysDisplacement | PhysKinetic | PhysStopping => p.iter().product(),
        PatQuad | PatFib | PatAlt => last_difference(p),
        _ => exact_answer(t),
    }
}

fn round_2sf(x: i64) -> i64 {
    let mut m = 1;
    while x.abs() / m >= 100 {
        m *= 10;
    }
    ((x as f64 / m as f64).round() as i64) * m
}

/// (answer, strategy was exact, method name)
fn solve(t: &TaskView) -> (i64, bool, String) {
    use Kind::*;
    let p = &t.params;
    match t.kind {
        ArithAdd => (p[0] + p[1], true, "exact:add".into()),
        ArithAddSub => (p[0] + p[2] * p[1], true, "exact:addsub".into()),
        ArithMulSmall => (p[0] * p[1], true, "exact:mul".into()),
        ArithMulBig => (round_2sf(p[0]) * round_2sf(p[1]), false, "heuristic:rounded-operands".into()),
        ArithMulSum => (round_2sf(p[0]) * round_2sf(p[1]) + round_2sf(p[2]) * round_2sf(p[3]), false, "heuristic:rounded-operands".into()),
        AlgXPlusB => (p[1] - p[0], true, "exact:isolate".into()),
        AlgAX => (p[1] / p[0], true, "exact:divide".into()),
        AlgAXPlusB => ((p[2] - p[1]) / p[0], true, "exact:two-step".into()),
        AlgBothSides => (search(|x| p[0] * x + p[1] - (p[2] * x + p[3])), false, "heuristic:search-10..10".into()),
        AlgParen => (search(|x| p[0] * (x + p[1]) - p[2] * x - p[3]), false, "heuristic:search-10..10".into()),
        PhysVelocity => (p[0] + p[1] * p[2], true, "exact:v=u+at".into()),
        PhysOhm => (p[0] * p[1], true, "exact:V=IR".into()),
        PhysDisplacement | PhysKinetic | PhysStopping => (p.iter().product(), false, "heuristic:product-of-numbers".into()),
        PatArith | PatGeom | PatQuad | PatFib | PatAlt => pattern(p),
        ExtAffGeom | ExtPoly3 | ExtRec2 | ExtTribo | ExtAlt | ExtPeriodic | NegProd | NegPrimes => (last_difference(p), false, "heuristic:last-difference".into()),
    }
}

fn search(residual: impl Fn(i64) -> i64) -> i64 {
    let mut best = (i64::MAX, -10);
    for x in -10..=10 {
        let r = residual(x).abs();
        if r < best.0 {
            best = (r, x);
        }
    }
    best.1
}

fn pattern(t: &[i64]) -> (i64, bool, String) {
    let n = t.len();
    let d1: Vec<i64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    if d1.iter().all(|d| *d == d1[0]) {
        return (t[n - 1] + d1[0], true, "exact:arithmetic".into());
    }
    if t[0] != 0 && t.windows(2).all(|w| w[0] != 0 && w[1] % w[0] == 0 && w[1] / w[0] == t[1] / t[0]) {
        return (t[n - 1] * (t[1] / t[0]), true, "exact:geometric".into());
    }
    let d2: Vec<i64> = d1.windows(2).map(|w| w[1] - w[0]).collect();
    if d2.iter().all(|d| *d == d2[0]) {
        let next_d = d1[d1.len() - 1] + d2[0];
        return (t[n - 1] + next_d, true, "exact:quadratic".into());
    }
    if t.windows(3).all(|w| w[2] == w[0] + w[1]) {
        return (t[n - 1] + t[n - 2], true, "exact:fibonacci-like".into());
    }
    (t[n - 1] + d1[d1.len() - 1], false, "heuristic:last-difference".into())
}

/// Convenience for tests and the oracle: does the scripted thinker, with no slip, solve this exactly?
pub fn scripted_is_exact(t: &TaskView) -> bool {
    solve(t).1
}

#[allow(dead_code)]
fn _domain_used(_: Domain) {}
