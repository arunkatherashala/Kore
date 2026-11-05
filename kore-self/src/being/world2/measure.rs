//! Measurement helpers for World v2 (no log: the same engine, records dropped). The score is the
//! TRUE reward per energy (the checker's verdict), not the reward the being was told.

use super::engine::{Engine2, LearnerCfg, State2, TrueRec};
use super::learner::Chooser2;
use super::solve::sig2_split;
use super::world::{W2Cfg, World2};
use crate::being::controller::Limits;
use crate::being::discover::{apply, Rule};

#[derive(Debug, Clone)]
pub struct RuleCheck {
    pub rule: Rule,
    pub clock: i64,
    /// On fresh tasks of its (family, difficulty) at acceptance time: applicable and wrong at least once.
    pub wrong_at_accept: bool,
    /// On fresh tasks at the end of the life: share right among those where it applies (None: never applies).
    pub right_share_end: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct W2Run {
    pub true_reward: i64,
    pub reported_reward: i64,
    pub cost: i64,
    pub steps: u64,
    pub skips: u64,
    pub recs: Vec<TrueRec>,
    pub rules: Vec<RuleCheck>,
    pub refusals: usize,
    pub state: State2,
}

impl W2Run {
    pub fn per_energy(&self) -> f64 {
        if self.cost > 0 { self.true_reward as f64 / self.cost as f64 } else { 0.0 }
    }
    /// True reward per energy over the steps whose energy position lies in [from, to) of the life.
    pub fn window_e(&self, from: i64, to: i64) -> f64 {
        let (mut e, mut r, mut c) = (0i64, 0i64, 0i64);
        for x in &self.recs {
            if e >= from && e < to {
                r += x.true_reward;
                c += x.cost;
            }
            e += x.cost;
        }
        if c > 0 { r as f64 / c as f64 } else { 0.0 }
    }
}

pub const RULE_CHECK_TASKS: u64 = 50;

/// (applicable, right) of one rule on `n` fresh tasks of its (family, difficulty) at `clock`.
pub fn rule_on_fresh(w: &World2, r: &Rule, clock: i64, n: u64) -> (u64, u64) {
    let (fam, d) = sig2_split(r.sig);
    let book = [r.clone()];
    let (mut app, mut ok) = (0, 0);
    for i in 0..n {
        let t = w.task(2_000_000 + i, 0, fam, Some(d), clock, 0x5EC_C4EC);
        if let Some((v, _)) = apply(&book, r.sig, &t.view.params) {
            app += 1;
            ok += t.check(v) as u64;
        }
    }
    (app, ok)
}

/// One life in World v2 with `chooser`, without a log.
pub fn run_w2(cfg: &W2Cfg, chooser: &mut dyn Chooser2, discover_every: u64, limits: Option<Limits>) -> W2Run {
    let mut st = State2::default();
    st.start(cfg.clone(), LearnerCfg::default(), "genesis".into());
    let mut e = Engine2::new(st, limits);
    let (mut tr, mut rr, mut cost, mut steps) = (0i64, 0i64, 0i64, 0u64);
    let mut recs = vec![];
    let mut rules = vec![];
    while let Ok(o) = e.step(chooser) {
        steps += 1;
        tr += o.truth.true_reward;
        rr += o.outcome.reward;
        cost += o.outcome.cost;
        recs.push(o.truth);
        if discover_every > 0 && e.state.step % discover_every == 0 {
            for nr in e.discover_pass() {
                let (app, ok) = rule_on_fresh(&e.world, &nr.rule, e.state.clock, RULE_CHECK_TASKS);
                rules.push(RuleCheck { rule: nr.rule, clock: e.state.clock, wrong_at_accept: ok < app, right_share_end: None });
            }
        }
    }
    let end = e.state.clock;
    for rc in &mut rules {
        let (app, ok) = rule_on_fresh(&e.world, &rc.rule, end, RULE_CHECK_TASKS);
        rc.right_share_end = if app > 0 { Some(ok as f64 / app as f64) } else { None };
    }
    W2Run { true_reward: tr, reported_reward: rr, cost, steps, skips: e.state.skips, recs, rules, refusals: e.refusals_total.len(), state: e.state }
}
