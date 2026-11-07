//! World O measurement: ground truth per family cell, the reference policies, and a run without a log
//! that records learning curves and time to competence per family. Measurement only: nothing here is
//! read by the being, and nothing here is written to a log.

use super::engine::{attempt_o, EngineO, LearnerOCfg, StateO, TrueO};
use super::world::{TaskO, WOCfg, WorldO, SKIP_COST};
use crate::being::controller::Limits;
use crate::being::discover::Rule;
use crate::being::planner::SKIP_MIN_COST;
use crate::being::solver::Strategy;
use crate::being::world2::engine::State2;
use crate::being::world2::learner::{plain, Act2, Choice2, Chooser2, Offer2};
use crate::being::world2::solve::{self, sig2};
use crate::being::world2::world::FType;
use std::collections::HashMap;

pub const TRUTH_N: u64 = 200;
const OFFSET: u64 = 1 << 40;
const FIXED: [Strategy; 3] = [Strategy::Fast, Strategy::Careful, Strategy::Deep];

/// Hidden success rates per (family, regimes, difficulty, strategy, rule in use), sampled lazily.
#[derive(Debug, Default, Clone)]
pub struct TruthO {
    cache: HashMap<(u64, u64, u64, u8, u8, String), f64>,
}

impl TruthO {
    fn key_rule(rules: &[Rule], sig: u8) -> String {
        crate::being::world2::rules_use::primary_rule(rules, sig).map_or(String::new(), |r| r.id.clone())
    }

    /// Success probability of strategy `s` (None: the checker's own answer with careful slips) on a
    /// task of family `j` at difficulty `d` and world clock `clock`, with the rule book `rules`.
    pub fn p(&mut self, w: &WorldO, j: u64, d: u8, s: Option<Strategy>, clock: i64, rules: &[Rule]) -> f64 {
        let f = &w.fams[j as usize];
        if s.is_none() {
            return 1.0 - solve::slip_pm(Strategy::Careful, f.slip_pm) as f64 / 1000.0;
        }
        let s = s.expect("checked");
        let ri = w.regime_index(j, clock);
        let rb = f.base.map_or(0, |b| w.regime_index(b, clock));
        let rk = if s == Strategy::Discovered && matches!(f.ftype, FType::Seq | FType::Compose) {
            let src = f.base.map_or(f.slot, |b| w.fams[b as usize].slot);
            Self::key_rule(rules, sig2(src, d))
        } else {
            String::new()
        };
        let key = (j, ri, rb, d, s.idx() as u8, rk);
        if let Some(p) = self.cache.get(&key) {
            return *p;
        }
        let mut ok = 0u64;
        for i in 0..TRUTH_N {
            let t = w.task(OFFSET + i, (i % w.cfg.menu as u64) as usize, j, Some(d), clock, 0x5A);
            ok += t.check(attempt_o(w, &t, Some(s), rules).answer) as u64;
        }
        let p = ok as f64 / TRUTH_N as f64;
        self.cache.insert(key, p);
        p
    }

    /// The best success-per-energy the being's means allow on this task: the fixed strategies, or a
    /// perfect rule of the B2 grammar (the checker's answer at the listed energy, careful slips) for
    /// sequence and compose families whose hidden class the grammar describes. Returns (value per
    /// energy unit of listed reward, success probability of that option).
    pub fn ceiling(&mut self, w: &WorldO, t: &TaskO, clock: i64) -> (f64, f64) {
        let v = &t.view;
        let mut best = (0.0f64, 0.0f64);
        for s in FIXED {
            let p = self.p(w, t.fid, v.difficulty, Some(s), clock, &[]);
            let x = p / s.energy(v.cost) as f64;
            if x > best.0 {
                best = (x, p);
            }
        }
        let f = &w.fams[t.fid as usize];
        if matches!(f.ftype, FType::Seq | FType::Compose) && w.class_of(t.fid, clock).in_grammar() {
            let p = self.p(w, t.fid, v.difficulty, None, clock, &[]);
            let x = p / v.cost as f64;
            if x > best.0 {
                best = (x, p);
            }
        }
        best
    }
}

/// Reference policy: knows the hidden success rate of every fixed strategy (and, with `knows_rules`,
/// can answer with the checker's own answer at the listed energy with careful slips: a being that has
/// learned every hidden rule and constant). Takes the option with the highest expected reward per
/// energy and skips by the renewal comparison with its own realized rate. A strong reference, not a
/// proven optimum.
pub struct OracleO {
    pub world: WorldO,
    pub truth: TruthO,
    pub knows_rules: bool,
}

impl Chooser2 for OracleO {
    fn name(&self) -> String {
        if self.knows_rules { "wo-oracle-knows-rules".into() } else { "wo-oracle-planner".into() }
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let tasks = self.world.offers(st.step, st.clock);
        let mut best: Option<(f64, f64, f64, Act2)> = None;
        for (o, t) in offers.iter().zip(&tasks) {
            let mut opts: Vec<(Option<Strategy>, i64)> = FIXED.iter().map(|s| (Some(*s), s.energy(o.listed_cost))).collect();
            if self.knows_rules {
                opts.push((None, o.listed_cost));
            }
            for (s, e) in opts {
                if e > st.budget_left {
                    continue;
                }
                let p = self.truth.p(&self.world, t.fid, o.difficulty, s, st.clock, &[]);
                let v = o.listed_reward as f64 * p / e as f64;
                if best.as_ref().map_or(true, |b| v > b.0) {
                    let act = match s {
                        Some(s) => Act2::Attempt { slot: o.slot, strategy: s },
                        None => Act2::Oracle { slot: o.slot },
                    };
                    best = Some((v, p, e as f64, act));
                }
            }
        }
        let (v, p, e, act) = best?;
        let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let skip = st.total_cost >= SKIP_MIN_COST && v < rate * (1.0 - SKIP_COST as f64 / e.max(1.0)) && st.budget_left >= SKIP_COST;
        Some(if skip { plain(&self.name(), "renewal skip", Act2::Skip, 0.0, v) } else { plain(&self.name(), "knows hidden truth", act, p, v) })
    }
}

/// One window of the learning curve (by energy spent).
#[derive(Debug, Clone, Default)]
pub struct WinO {
    pub index: u64,
    pub steps: u64,
    pub true_reward: i64,
    pub cost: i64,
    pub attempts: u64,
    pub skips: u64,
    pub rules_accepted: u64,
    pub rule_uses: u64,
    pub rule_right: u64,
    pub families_opened: u64,
    pub mean_tier_x1000: u64,
}

impl WinO {
    pub fn rpe(&self) -> f64 {
        if self.cost > 0 { self.true_reward as f64 / self.cost as f64 } else { 0.0 }
    }
}

/// Learning record of one family.
#[derive(Debug, Clone, Default)]
pub struct FamRec {
    pub fid: u64,
    pub tier: u32,
    pub ftype: Option<FType>,
    pub intro: i64,
    pub retire: i64,
    pub attempts: u64,
    /// Ratio (chosen option / ceiling) of each attempt, in order.
    pub ratios: Vec<f32>,
    /// Ceiling success probability of each attempt (learnable when it is high).
    pub ceil_p_sum: f64,
    /// Attempts and energy since its opening when the trailing `COMP_K` ratios first averaged >= `COMP_LEVEL`.
    pub competent_after: Option<(u64, i64)>,
}

pub const COMP_K: usize = 10;
pub const COMP_LEVEL: f64 = 0.9;
/// A family counts as learnable when the mean ceiling success probability of its attempts is at least this.
pub const LEARNABLE_P: f64 = 0.5;

impl FamRec {
    pub fn learnable(&self) -> bool {
        self.attempts >= COMP_K as u64 && self.ceil_p_sum / self.attempts as f64 >= LEARNABLE_P
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunWO {
    pub wins: Vec<WinO>,
    pub fams: Vec<FamRec>,
    pub steps: u64,
    pub true_reward: i64,
    pub cost: i64,
    pub rules_total: u64,
    pub refusals: u64,
    pub max_live_rules: usize,
    pub stopped: String,
}

/// Run a being (its own learner from `learner`, or `chooser`) in World O without a log, for at most
/// `max_steps` steps, through the same `EngineO::step` as the logged run. `window` in energy units.
pub fn run_wo(cfg: &WOCfg, learner: LearnerOCfg, chooser: Option<&mut dyn Chooser2>, discover_every: u64, limits: Option<Limits>, max_steps: u64, window: i64, competence: bool) -> RunWO {
    let mut st = StateO::default();
    st.start(cfg.clone(), learner, String::new());
    let mut own = learner.learner();
    let chooser: &mut dyn Chooser2 = match chooser {
        Some(c) => c,
        None => &mut own,
    };
    let mut e = EngineO::new(st, limits);
    let mut truth = TruthO::default();
    let mut out = RunWO::default();
    let mut fams: HashMap<u64, FamRec> = HashMap::new();
    let mut energy = 0i64;
    let mut w = WinO::default();
    let mut tier_sum = 0u64;
    let mut opened_before = 0u64;
    while out.steps < max_steps {
        let clock0 = e.state.s2.clock;
        let rules_before: Vec<Rule> = if competence { e.state.s2.rules.clone() } else { vec![] };
        let o = match e.step(chooser) {
            Ok(o) => o,
            Err(r) => {
                out.stopped = r.as_str().into();
                break;
            }
        };
        out.steps += 1;
        let t: &TrueO = &o.truth;
        energy += o.outcome.cost;
        out.true_reward += t.true_reward;
        out.cost += o.outcome.cost;
        w.steps += 1;
        w.true_reward += t.true_reward;
        w.cost += o.outcome.cost;
        if t.attempted { w.attempts += 1 } else { w.skips += 1 }
        if t.rule_used {
            w.rule_uses += 1;
            w.rule_right += t.true_correct as u64;
        }
        if t.attempted {
            tier_sum += t.tier as u64;
        }
        if competence {
            if let (Some(task), true) = (&o.task, t.attempted) {
                let fr = fams.entry(task.fid).or_insert_with(|| {
                    let f = &e.world.fams[task.fid as usize];
                    FamRec { fid: f.id, tier: f.tier, ftype: Some(f.ftype), intro: f.intro, retire: f.retire, ..Default::default() }
                });
                let (cx, cp) = truth.ceiling(&e.world, task, clock0);
                let p = truth.p(&e.world, task.fid, task.view.difficulty, t.strategy, clock0, &rules_before);
                let x = p / t.cost.max(1) as f64;
                fr.attempts += 1;
                fr.ceil_p_sum += cp;
                fr.ratios.push(if cx > 0.0 { (x / cx).min(1.0) as f32 } else { 1.0 });
                if fr.competent_after.is_none() && fr.ratios.len() >= COMP_K {
                    let m: f64 = fr.ratios[fr.ratios.len() - COMP_K..].iter().map(|r| *r as f64).sum::<f64>() / COMP_K as f64;
                    if m >= COMP_LEVEL {
                        fr.competent_after = Some((fr.attempts, clock0 / crate::being::world::SECS_PER_ENERGY - fr.intro));
                    }
                }
            }
        }
        if discover_every > 0 && e.state.s2.step % discover_every == 0 {
            w.rules_accepted += e.discover_pass().len() as u64;
        }
        let _ = e.take_refusals();
        out.max_live_rules = out.max_live_rules.max(e.state.s2.live_rules());
        if energy >= (w.index as i64 + 1) * window {
            w.families_opened = e.state.opened - opened_before;
            opened_before = e.state.opened;
            w.mean_tier_x1000 = if w.attempts > 0 { tier_sum * 1000 / w.attempts } else { 0 };
            tier_sum = 0;
            let next = w.index + 1;
            out.wins.push(std::mem::take(&mut w));
            w.index = next;
        }
    }
    if w.steps > 0 {
        w.families_opened = e.state.opened - opened_before;
        w.mean_tier_x1000 = if w.attempts > 0 { tier_sum * 1000 / w.attempts } else { 0 };
        out.wins.push(w);
    }
    out.rules_total = out.wins.iter().map(|w| w.rules_accepted).sum();
    out.refusals = e.refusals_total.len() as u64;
    let mut fv: Vec<FamRec> = fams.into_values().collect();
    fv.sort_by_key(|f| f.fid);
    out.fams = fv;
    out
}
