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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub answer: i64,
    /// The thinker's own estimate that the answer is right.
    pub confidence: f64,
    /// Short name of the strategy used (goes into the log).
    pub method: String,
}

pub trait Thinker {
    fn id(&self) -> &str;
    fn propose(&self, ctx: &Context) -> Proposal;
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
            return Proposal { answer, confidence: CONF_EXACT, method: format!("{method}+slip") };
        }
        Proposal { answer, confidence: if exact { CONF_EXACT } else { CONF_HEURISTIC }, method }
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
