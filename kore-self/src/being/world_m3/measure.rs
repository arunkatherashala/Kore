//! World M3 measurement runs (in memory, no log; the logged run `run_t` takes exactly the same steps, a test
//! checks it). The engine evaluates every fixed plan on every task so that each life also records the best
//! reward per energy the plans allow on the tasks it met (for the oracle and the regret). One world (and one
//! base table) is shared read-only by all lives of a seed.

use super::baselines::*;
use super::engine::{ChooserT, EngineT, MemSandbox, Sandbox, StateT, ToolCache, TrueT, DEFAULT_DISCOVER_EVERY};
use super::learner::{LearnerT, LearnerTCfg};
use super::world::WorldT;
use crate::being::world::Rng;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct LifeT {
    pub truths: Vec<TrueT>,
    pub reward: i64,
    pub cost: i64,
    pub rules: usize,
    pub order: Vec<(super::world::Kind, i64)>,
}

impl LifeT {
    pub fn rpe(&self) -> f64 {
        self.reward as f64 / self.cost.max(1) as f64
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pol {
    Being(LearnerTCfg),
    Random,
    AlwaysSql,
    Cheapest,
    Ucb(UcbCfg),
    Lin(LinCfg),
    LinFile(LinCfg),
    AnswerAll,
    RewardBest,
}

impl Pol {
    pub fn label(&self) -> String {
        match self {
            Pol::Being(l) => format!("being (beta {:.1}{})", l.beta_milli as f64 / 1000.0, if l.file_key { ", file key" } else { "" }),
            Pol::Random => "random plan".into(),
            Pol::AlwaysSql => "always SQL".into(),
            Pol::Cheapest => "always cheapest".into(),
            Pol::Ucb(c) => UcbT::new(*c).name(),
            Pol::Lin(c) => LinUcbT::new(*c).name(),
            Pol::LinFile(c) => LinUcbT::with_file(*c).name(),
            Pol::AnswerAll => "answer-everything oracle (sees hidden)".into(),
            Pol::RewardBest => "reward-best oracle (sees hidden)".into(),
        }
    }
}

/// One life of `world.cfg.budget` energy with a policy on a shared world, sharing the deterministic plans'
/// results with other lives of the same world through `cache` (same answers and energies; only faster).
pub fn run_life_cached(world: &Arc<WorldT>, pol: Pol, sb: Option<&dyn Sandbox>, cache: &mut ToolCache) -> LifeT {
    let cfg = world.cfg.clone();
    let lcfg = if let Pol::Being(l) = pol { l } else { LearnerTCfg::default() };
    let mut st = StateT::default();
    st.start(cfg.clone(), lcfg, "genesis".into());
    let mut eng = EngineT::with_world(st, None, world.clone());
    eng.measure = true;
    eng.cache = std::mem::take(cache);
    let mem = MemSandbox(world);
    let sb: &dyn Sandbox = sb.unwrap_or(&mem);
    let mut chooser: Box<dyn ChooserT> = match pol {
        Pol::Being(l) => Box::new(LearnerT { cfg: l }),
        Pol::Random => Box::new(RandomT { rng: Rng::new(cfg.seed ^ 0xA11) }),
        Pol::AlwaysSql => Box::new(AlwaysSqlT),
        Pol::Cheapest => Box::new(CheapestT),
        Pol::Ucb(c) => Box::new(UcbT::new(c)),
        Pol::Lin(c) => Box::new(LinUcbT::new(c)),
        Pol::LinFile(c) => Box::new(LinUcbT::with_file(c)),
        Pol::AnswerAll => Box::new(OracleT::new()),
        Pol::RewardBest => Box::new(RewardBestT::new()),
    };
    let discover = matches!(pol, Pol::Being(_));
    let mut truths = vec![];
    let (mut rew, mut cost) = (0, 0);
    while let Ok(s) = eng.step(chooser.as_mut(), sb) {
        rew += s.truth.reward;
        cost += s.truth.cost;
        truths.push(s.truth);
        if discover && eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    *cache = std::mem::take(&mut eng.cache);
    let rules = eng.state.rules.len();
    let order = world.order.iter().enumerate().map(|(i, k)| (*k, world.intro_energy(i))).collect();
    LifeT { truths, reward: rew, cost, rules, order }
}

pub fn run_life(world: &Arc<WorldT>, pol: Pol) -> LifeT {
    run_life_cached(world, pol, None, &mut ToolCache::new())
}
