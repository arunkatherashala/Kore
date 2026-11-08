//! Reference policies for World T (measurement only): random plan, always SQL, a bandit over plans
//! (UCB1), LinUCB over the visible task features, and an oracle that SEES the true result of every plan
//! on every task. Except for the oracle they see only the visible task view and the reward and energy
//! they were told, and they use only the fixed plans (no rule book of their own).

use super::engine::{ChooserT, PlanResult, StateT};
use super::learner::ChoiceT;
use super::world::{plans_for, Plan, TaskT, Tmpl, ViewT, PLANS, SKIP_COST, TMPLS};
use crate::being::world::Rng;
use std::collections::BTreeMap;

/// Reward-per-energy scale used to put rewards into [0, 1] for the bandits.
pub const RPE_SCALE: f64 = 16.0;

fn fixed(t: Tmpl) -> Vec<Plan> {
    plans_for(t).iter().copied().filter(|p| *p != Plan::TsRule).collect()
}

/// The renewal skip used by the being, available to the references as a tuned option.
fn skip_bar(rate: f64, energy: f64) -> f64 {
    rate * (energy - SKIP_COST as f64).max(0.0) / energy.max(1.0)
}

pub struct RandomT {
    pub rng: Rng,
}

impl ChooserT for RandomT {
    fn name(&self) -> String {
        "random plan".into()
    }
    fn choose(&mut self, _st: &StateT, v: &ViewT) -> ChoiceT {
        let ps = fixed(v.tmpl);
        let p = ps[self.rng.below(ps.len() as u64) as usize];
        ChoiceT::plain("ref-random", p, p.energy(v.rows) as f64, "uniform over the template's plans")
    }
}

pub struct AlwaysSqlT;

impl ChooserT for AlwaysSqlT {
    fn name(&self) -> String {
        "always SQL".into()
    }
    fn choose(&mut self, _st: &StateT, v: &ViewT) -> ChoiceT {
        let ps = fixed(v.tmpl);
        let p = if ps.contains(&Plan::SniffSql) { Some(Plan::SniffSql) } else if ps.contains(&Plan::LogSql) { Some(Plan::LogSql) } else { None };
        match p {
            Some(p) => ChoiceT::plain("ref-sql", p, p.energy(v.rows) as f64, "the SQL plan with the sniffing loader"),
            None => ChoiceT::skip("ref-sql", "no SQL plan for this template: skip"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UcbCfg {
    pub c: f64,
    /// Arms keyed by (template, context, plan) instead of (template, plan).
    pub per_ctx: bool,
    pub skip: bool,
}

#[derive(Default, Clone, Copy)]
struct Arm {
    n: f64,
    sum: f64,
}

pub struct UcbT {
    pub cfg: UcbCfg,
    arms: BTreeMap<(u8, u8, u8), Arm>,
    t: f64,
    rew: f64,
    cost: f64,
}

impl UcbT {
    pub fn new(cfg: UcbCfg) -> UcbT {
        UcbT { cfg, arms: BTreeMap::new(), t: 0.0, rew: 0.0, cost: 0.0 }
    }
    fn key(&self, v: &ViewT, p: Plan) -> (u8, u8, u8) {
        (v.tmpl.idx() as u8, if self.cfg.per_ctx { v.ctx.idx() as u8 } else { 0 }, p.idx() as u8)
    }
}

impl ChooserT for UcbT {
    fn name(&self) -> String {
        format!("UCB1 bandit (c {}, {}, skip {})", self.cfg.c, if self.cfg.per_ctx { "template x context" } else { "template" }, if self.cfg.skip { "on" } else { "off" })
    }
    fn choose(&mut self, _st: &StateT, v: &ViewT) -> ChoiceT {
        let mut best: Option<(f64, Plan)> = None;
        for p in fixed(v.tmpl) {
            let a = self.arms.get(&self.key(v, p)).copied().unwrap_or_default();
            let idx = if a.n == 0.0 { f64::INFINITY } else { a.sum / a.n + self.cfg.c * ((self.t + 1.0).ln() / a.n).sqrt() };
            if best.map_or(true, |b| idx > b.0) {
                best = Some((idx, p));
            }
        }
        let (idx, p) = best.unwrap();
        let e = p.energy(v.rows) as f64;
        let rate = if self.cost > 0.0 { self.rew / self.cost } else { 0.0 };
        if self.cfg.skip && self.cost >= 40.0 && idx.is_finite() && idx * RPE_SCALE < skip_bar(rate, e) {
            return ChoiceT::skip("ref-ucb", "best index below the skip bar");
        }
        ChoiceT::plain("ref-ucb", p, e, "highest UCB1 index")
    }
    fn observe(&mut self, v: &ViewT, plan: Option<Plan>, reward: i64, cost: i64) {
        self.t += 1.0;
        self.rew += reward as f64;
        self.cost += cost as f64;
        if let Some(p) = plan {
            let k = self.key(v, p);
            let a = self.arms.entry(k).or_default();
            a.n += 1.0;
            a.sum += (reward as f64 / cost.max(1) as f64 / RPE_SCALE).min(1.0);
        }
    }
}

/// Features: bias, template, context, size class, template x context (one-hot).
pub const LIN_D: usize = 1 + 16 + 15 + 3 + 16 * 15;

fn feats(v: &ViewT) -> [usize; 5] {
    let (t, c) = (v.tmpl.idx(), v.ctx.idx());
    [0, 1 + t, 17 + c, 32 + v.size as usize, 35 + t * 15 + c]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinCfg {
    pub alpha: f64,
    pub skip: bool,
}

struct LinArm {
    ainv: Vec<f64>,
    b: Vec<f64>,
}

impl LinArm {
    fn new() -> LinArm {
        let mut ainv = vec![0.0; LIN_D * LIN_D];
        for i in 0..LIN_D {
            ainv[i * LIN_D + i] = 1.0;
        }
        LinArm { ainv, b: vec![0.0; LIN_D] }
    }
    /// (theta . x, x' A^-1 x) for a sparse one-hot x.
    fn eval(&self, x: &[usize]) -> (f64, f64) {
        let mut mean = 0.0;
        let mut var = 0.0;
        for &i in x {
            // theta_i = sum_j ainv[i][j] b[j]
            let row = &self.ainv[i * LIN_D..(i + 1) * LIN_D];
            mean += row.iter().zip(&self.b).map(|(a, b)| a * b).sum::<f64>();
            for &j in x {
                var += row[j];
            }
        }
        (mean, var.max(0.0))
    }
    fn update(&mut self, x: &[usize], y: f64) {
        // Sherman-Morrison: ainv -= (ainv x)(x' ainv) / (1 + x' ainv x)
        let mut u = vec![0.0; LIN_D];
        for &j in x {
            for i in 0..LIN_D {
                u[i] += self.ainv[i * LIN_D + j];
            }
        }
        let denom = 1.0 + x.iter().map(|&j| u[j]).sum::<f64>();
        for i in 0..LIN_D {
            if u[i] == 0.0 {
                continue;
            }
            let ui = u[i] / denom;
            let row = &mut self.ainv[i * LIN_D..(i + 1) * LIN_D];
            for (r, uj) in row.iter_mut().zip(&u) {
                *r -= ui * uj;
            }
        }
        for &j in x {
            self.b[j] += y;
        }
    }
}

pub struct LinUcbT {
    pub cfg: LinCfg,
    arms: BTreeMap<u8, LinArm>,
    rew: f64,
    cost: f64,
}

impl LinUcbT {
    pub fn new(cfg: LinCfg) -> LinUcbT {
        LinUcbT { cfg, arms: BTreeMap::new(), rew: 0.0, cost: 0.0 }
    }
}

impl ChooserT for LinUcbT {
    fn name(&self) -> String {
        format!("LinUCB (alpha {}, skip {})", self.cfg.alpha, if self.cfg.skip { "on" } else { "off" })
    }
    fn choose(&mut self, _st: &StateT, v: &ViewT) -> ChoiceT {
        let x = feats(v);
        let mut best: Option<(f64, Plan, f64)> = None;
        for p in fixed(v.tmpl) {
            let e = p.energy(v.rows) as f64;
            let (m, var) = self.arms.get(&(p.idx() as u8)).map_or((0.0, 5.0), |a| a.eval(&x));
            let ucb = (m + self.cfg.alpha * var.sqrt()).clamp(0.0, 1.0);
            let s = ucb * v.reward as f64 / e;
            if best.map_or(true, |b| s > b.0) {
                best = Some((s, p, e));
            }
        }
        let (s, p, e) = best.unwrap();
        let rate = if self.cost > 0.0 { self.rew / self.cost } else { 0.0 };
        if self.cfg.skip && self.cost >= 40.0 && s < skip_bar(rate, e) {
            return ChoiceT::skip("ref-linucb", "best score below the skip bar");
        }
        ChoiceT::plain("ref-linucb", p, e, "highest LinUCB score x reward / energy")
    }
    fn observe(&mut self, v: &ViewT, plan: Option<Plan>, reward: i64, cost: i64) {
        self.rew += reward as f64;
        self.cost += cost as f64;
        if let Some(p) = plan {
            let x = feats(v);
            self.arms.entry(p.idx() as u8).or_insert_with(LinArm::new).update(&x, if reward > 0 { 1.0 } else { 0.0 });
        }
    }
}

/// SEES HIDDEN information: the true result of every plan on the task (and whether a perfect series rule
/// would answer it); takes the right answer of highest reward per energy, or skips when none is right.
pub struct OracleT {
    pick: Option<(Plan, String, i64)>,
}

impl OracleT {
    pub fn new() -> OracleT {
        OracleT { pick: None }
    }
}

impl ChooserT for OracleT {
    fn name(&self) -> String {
        "oracle (sees the true result of every plan)".into()
    }
    fn peek(&mut self, task: &TaskT, results: &[PlanResult]) {
        let mut best: Option<(f64, Plan, String, i64)> = None;
        for r in results.iter().filter(|r| r.correct) {
            let s = task.view.reward as f64 / r.energy.max(1) as f64;
            if best.as_ref().map_or(true, |b| s > b.0) {
                best = Some((s, r.plan, r.answer.clone().unwrap_or_default(), r.energy));
            }
        }
        self.pick = best.map(|b| (b.1, b.2, b.3));
    }
    fn choose(&mut self, _st: &StateT, _v: &ViewT) -> ChoiceT {
        match self.pick.take() {
            Some((p, a, e)) => {
                let mut c = ChoiceT::plain("ref-oracle", p, e as f64, "the plan whose true answer is right at the lowest energy");
                c.oracle_answer = Some(a);
                c
            }
            None => ChoiceT::skip("ref-oracle", "no plan answers this task right"),
        }
    }
}

/// All plan and template names (for tables).
pub fn names() -> (Vec<&'static str>, Vec<&'static str>) {
    (PLANS.iter().map(|p| p.name()).collect(), TMPLS.iter().map(|t| t.name()).collect())
}
