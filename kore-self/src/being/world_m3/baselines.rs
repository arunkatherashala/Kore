//! Reference policies for World M3 (measurement only): random plan, always SQL, a bandit over plans
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

/// Reward per energy into [0, 1] for UCB1 (with a penalty, negative values are shifted in).
fn scale01(rpe: f64, penalty_pm: u32) -> f64 {
    if penalty_pm == 0 {
        return (rpe / RPE_SCALE).min(1.0);
    }
    let lo = penalty_pm as f64 / 1000.0 * RPE_SCALE;
    ((rpe + lo) / (RPE_SCALE + lo)).clamp(0.0, 1.0)
}

fn unscale(x: f64, penalty_pm: u32) -> f64 {
    if penalty_pm == 0 {
        return x * RPE_SCALE;
    }
    let lo = penalty_pm as f64 / 1000.0 * RPE_SCALE;
    x * (RPE_SCALE + lo) - lo
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
        ChoiceT::plain("ref-random", p, p.listed(v) as f64, "uniform over the template's plans")
    }
}

pub struct AlwaysSqlT;

impl ChooserT for AlwaysSqlT {
    fn name(&self) -> String {
        "always SQL".into()
    }
    fn choose(&mut self, _st: &StateT, v: &ViewT) -> ChoiceT {
        let ps = fixed(v.tmpl);
        let p = if ps.contains(&Plan::SniffSql) { Some(Plan::SniffSql) } else { None };
        match p {
            Some(p) => ChoiceT::plain("ref-sql", p, p.listed(v) as f64, "the SQL plan with the sniffing loader"),
            None => ChoiceT::skip("ref-sql", "no SQL plan for this template: skip"),
        }
    }
}

/// Always the plan of lowest listed energy on the task (first listed on a tie), never a skip.
pub struct CheapestT;

impl ChooserT for CheapestT {
    fn name(&self) -> String {
        "always cheapest".into()
    }
    fn choose(&mut self, _st: &StateT, v: &ViewT) -> ChoiceT {
        let ps = fixed(v.tmpl);
        let mut best = ps[0];
        for p in &ps {
            if p.listed(v) < best.listed(v) {
                best = *p;
            }
        }
        ChoiceT::plain("ref-cheapest", best, best.listed(v) as f64, "the plan of lowest listed energy")
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
        let e = p.listed(v) as f64;
        let rate = if self.cost > 0.0 { self.rew / self.cost } else { 0.0 };
        if self.cfg.skip && self.cost >= 40.0 && idx.is_finite() && unscale(idx, v.penalty_pm) < skip_bar(rate, e) {
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
            a.sum += scale01(reward as f64 / cost.max(1) as f64, v.penalty_pm);
        }
    }
}

/// Features: bias, template, context, size class, template x context (one-hot).
pub const LIN_D: usize = 1 + 21 + 11 + 4 + 21 * 11;

fn feats(v: &ViewT) -> [usize; 5] {
    let (t, c) = (v.tmpl.idx(), v.ctx.idx());
    [0, 1 + t, 22 + c, 33 + v.size as usize, 37 + t * 11 + c]
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct LinCfg {
    pub alpha: f64,
    pub skip: bool,
}

struct LinArm {
    /// Dimension (LIN_D; with the file feature LIN_D plus the files met so far, grown as files are met).
    d: usize,
    ainv: Vec<f64>,
    b: Vec<f64>,
    /// Target "the answer was wrong" (same features, same design matrix).
    bw: Vec<f64>,
}

impl LinArm {
    fn new(d: usize) -> LinArm {
        let mut ainv = vec![0.0; d * d];
        for i in 0..d {
            ainv[i * d + i] = 1.0;
        }
        LinArm { d, ainv, b: vec![0.0; d], bw: vec![0.0; d] }
    }
    /// One more one-hot dimension never seen so far: A = I + sum x x' has the identity in its row and column,
    /// so A^-1 is extended by the identity (exact; the same as having had the dimension from the start).
    fn grow(&mut self, d: usize) {
        while self.d < d {
            let (o, n) = (self.d, self.d + 1);
            let mut a = vec![0.0; n * n];
            for i in 0..o {
                a[i * n..i * n + o].copy_from_slice(&self.ainv[i * o..(i + 1) * o]);
            }
            a[o * n + o] = 1.0;
            self.ainv = a;
            self.b.push(0.0);
            self.bw.push(0.0);
            self.d = n;
        }
    }
    /// (theta . x, x' A^-1 x) for a sparse one-hot x.
    fn eval(&self, x: &[usize]) -> (f64, f64) {
        let mut mean = 0.0;
        let mut var = 0.0;
        for &i in x {
            // theta_i = sum_j ainv[i][j] b[j]
            let d = self.d;
            let row = &self.ainv[i * d..(i + 1) * d];
            mean += row.iter().zip(&self.b).map(|(a, b)| a * b).sum::<f64>();
            for &j in x {
                var += row[j];
            }
        }
        (mean, var.max(0.0))
    }
    fn mean_wrong(&self, x: &[usize]) -> f64 {
        let d = self.d;
        x.iter().map(|&i| self.ainv[i * d..(i + 1) * d].iter().zip(&self.bw).map(|(a, b)| a * b).sum::<f64>()).sum()
    }
    fn update(&mut self, x: &[usize], y: f64, yw: f64) {
        for &j in x {
            self.bw[j] += yw;
        }
        // Sherman-Morrison: ainv -= (ainv x)(x' ainv) / (1 + x' ainv x)
        let d = self.d;
        let mut u = vec![0.0; d];
        for &j in x {
            for i in 0..d {
                u[i] += self.ainv[i * d + j];
            }
        }
        let denom = 1.0 + x.iter().map(|&j| u[j]).sum::<f64>();
        for i in 0..d {
            if u[i] == 0.0 {
                continue;
            }
            let ui = u[i] / denom;
            let row = &mut self.ainv[i * d..(i + 1) * d];
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
    /// Track W2 baseline "LinUCB with the file feature": a one-hot of the task's file (its name, as the view
    /// shows it) appended to the features; a file gets its dimension when first met (exactly a one-hot over
    /// all files, see `LinArm::grow`). None: the W1 features only.
    files: Option<BTreeMap<String, usize>>,
}

impl LinUcbT {
    pub fn new(cfg: LinCfg) -> LinUcbT {
        LinUcbT { cfg, arms: BTreeMap::new(), rew: 0.0, cost: 0.0, files: None }
    }
    /// LinUCB with the file feature (Track W2 baseline, measurement only).
    pub fn with_file(cfg: LinCfg) -> LinUcbT {
        LinUcbT { files: Some(BTreeMap::new()), ..LinUcbT::new(cfg) }
    }
    fn dim(&self) -> usize {
        LIN_D + self.files.as_ref().map_or(0, |f| f.len())
    }
    /// The features of a view; with the file feature, the file's dimension is added (every arm grown) when new.
    fn x(&mut self, v: &ViewT) -> Vec<usize> {
        let mut x = feats(v).to_vec();
        if let Some(f) = &mut self.files {
            let n = f.len();
            let i = *f.entry(v.file.clone()).or_insert(n);
            let d = LIN_D + f.len();
            for a in self.arms.values_mut() {
                a.grow(d);
            }
            x.push(LIN_D + i);
        }
        x
    }
}

impl ChooserT for LinUcbT {
    fn name(&self) -> String {
        format!("LinUCB{} (alpha {}, skip {})", if self.files.is_some() { " with the file feature" } else { "" }, self.cfg.alpha, if self.cfg.skip { "on" } else { "off" })
    }
    fn choose(&mut self, _st: &StateT, v: &ViewT) -> ChoiceT {
        let x = self.x(v);
        let mut best: Option<(f64, Plan, f64)> = None;
        for p in fixed(v.tmpl) {
            let e = p.listed(v) as f64;
            let (m, var) = self.arms.get(&(p.idx() as u8)).map_or((0.0, 5.0), |a| a.eval(&x));
            let ucb = (m + self.cfg.alpha * var.sqrt()).clamp(0.0, 1.0);
            let mw = if v.penalty_pm == 0 { 0.0 } else { self.arms.get(&(p.idx() as u8)).map_or(0.0, |a| a.mean_wrong(&x)).clamp(0.0, 1.0) };
            let s = (ucb * v.reward as f64 - mw * v.penalty()) / e;
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
            let x = self.x(v);
            let d = self.dim();
            self.arms.entry(p.idx() as u8).or_insert_with(|| LinArm::new(d)).update(&x, if reward > 0 { 1.0 } else { 0.0 }, if reward < 0 { 1.0 } else { 0.0 });
        }
    }
}

/// The ANSWER-EVERYTHING oracle. SEES HIDDEN information: the true result of every plan on the task (and
/// whether a perfect series rule would answer it); answers every task that some plan answers right, with
/// the right plan of highest reward per energy, and skips only when no plan is right. It never skips a task
/// it can answer, however expensive, so it is a per-task reference, not the best policy.
pub struct OracleT {
    pick: Option<(Plan, String, i64)>,
}

impl OracleT {
    pub fn new() -> OracleT {
        OracleT { pick: None }
    }
}

/// The REWARD-BEST oracle. SEES HIDDEN information like the answer-everything oracle; on every task it takes
/// the option, skip included, of highest `reward - rho x energy`, where the reward is the true one (the
/// penalty for a wrong answer included), a skip is worth `-rho` (its energy 1), and rho is its own realized
/// reward per energy so far (the renewal comparison of the World O oracle-planner). A strong reference, not a
/// proven optimum.
pub struct RewardBestT {
    pick: Option<(Plan, Option<String>, i64)>,
    rew: f64,
    cost: f64,
}

impl RewardBestT {
    pub fn new() -> RewardBestT {
        RewardBestT { pick: None, rew: 0.0, cost: 0.0 }
    }
}

impl ChooserT for RewardBestT {
    fn name(&self) -> String {
        "reward-best oracle (sees the true result of every plan)".into()
    }
    fn peek(&mut self, task: &TaskT, results: &[PlanResult]) {
        let rho = if self.cost > 0.0 { self.rew / self.cost } else { 0.0 };
        let v = &task.view;
        let mut best: (f64, Option<(Plan, Option<String>, i64)>) = (-rho * SKIP_COST as f64, None);
        for r in results {
            let reward = if r.correct { v.reward as f64 } else if r.answer.is_some() { -((v.reward * v.penalty_pm as i64 + 500) / 1000) as f64 } else { 0.0 };
            let val = reward - rho * r.energy as f64;
            if val > best.0 {
                best = (val, Some((r.plan, r.answer.clone(), r.energy)));
            }
        }
        self.pick = best.1;
    }
    fn choose(&mut self, _st: &StateT, _v: &ViewT) -> ChoiceT {
        match self.pick.take() {
            Some((p, a, e)) => {
                let mut c = ChoiceT::plain("ref-reward-best", p, e as f64, "the option of highest true reward minus rho x energy");
                c.oracle_answer = Some(a.unwrap_or_default());
                c
            }
            None => ChoiceT::skip("ref-reward-best", "skipping is worth more than any plan on this task"),
        }
    }
    fn observe(&mut self, _v: &ViewT, _plan: Option<Plan>, reward: i64, cost: i64) {
        self.rew += reward as f64;
        self.cost += cost as f64;
    }
}

impl ChooserT for OracleT {
    fn name(&self) -> String {
        "answer-everything oracle (sees the true result of every plan)".into()
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
                let mut c = ChoiceT::plain("ref-answer-all", p, e as f64, "the plan whose true answer is right at the highest reward per energy");
                c.oracle_answer = Some(a);
                c
            }
            None => ChoiceT::skip("ref-answer-all", "no plan answers this task right"),
        }
    }
}

/// All plan and template names (for tables).
pub fn names() -> (Vec<&'static str>, Vec<&'static str>) {
    (PLANS.iter().map(|p| p.name()).collect(), TMPLS.iter().map(|t| t.name()).collect())
}
