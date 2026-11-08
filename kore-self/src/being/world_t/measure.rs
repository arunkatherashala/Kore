//! World T measurement runs (in memory, no log; the logged run `run_t` takes exactly the same steps, a
//! test checks it). The engine evaluates every plan on every task so that each life also records the
//! best reward per energy the plans allow on the tasks it met (for the regret and the oracle).

use super::baselines::*;
use super::engine::{ChooserT, EngineT, MemSandbox, Sandbox, StateT, ToolCache, TrueT, DEFAULT_DISCOVER_EVERY};
use super::learner::{LearnerT, LearnerTCfg};
use super::world::{WTCfg, WorldT};
use crate::being::world::Rng;

#[derive(Debug, Clone)]
pub struct LifeT {
    pub truths: Vec<TrueT>,
    pub reward: i64,
    pub cost: i64,
    pub rules: usize,
    /// Rules accepted on the noisy series signatures (should be 0).
    pub rules_noisy: usize,
    /// Kind order and opening energies of the world.
    pub order: Vec<(super::world::Kind, i64)>,
}

impl LifeT {
    pub fn rpe(&self) -> f64 {
        self.reward as f64 / self.cost.max(1) as f64
    }
}

/// Which policy a life is run with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pol {
    Being(LearnerTCfg),
    Random,
    AlwaysSql,
    Ucb(UcbCfg),
    Lin(LinCfg),
    Oracle,
}

impl Pol {
    pub fn label(&self) -> String {
        match self {
            Pol::Being(l) => format!("being ({} prior, beta {:.1})", if l.pooled { "pooled" } else { "flat" }, l.beta_milli as f64 / 1000.0),
            Pol::Random => "random plan".into(),
            Pol::AlwaysSql => "always SQL".into(),
            Pol::Ucb(c) => UcbT::new(*c).name(),
            Pol::Lin(c) => LinUcbT::new(*c).name(),
            Pol::Oracle => "oracle (sees hidden)".into(),
        }
    }
}

/// One life of `cfg.budget` energy with a policy, reading files from `sb` (None: the generator's bytes).
pub fn run_life(cfg: &WTCfg, pol: Pol, sb: Option<&dyn Sandbox>) -> LifeT {
    run_life_cached(cfg, pol, sb, &mut ToolCache::new())
}

/// As `run_life`, sharing the results of the deterministic plans with other lives of the same world (same
/// answers and energies; only faster).
pub fn run_life_cached(cfg: &WTCfg, pol: Pol, sb: Option<&dyn Sandbox>, cache: &mut ToolCache) -> LifeT {
    let lcfg = if let Pol::Being(l) = pol { l } else { LearnerTCfg::default() };
    let mut st = StateT::default();
    st.start(cfg.clone(), lcfg, "genesis".into());
    let mut eng = EngineT::new(st, None);
    eng.measure = true;
    eng.cache = std::mem::take(cache);
    let world = WorldT::new(cfg.clone());
    let mem = MemSandbox(&world);
    let sb: &dyn Sandbox = sb.unwrap_or(&mem);
    let mut chooser: Box<dyn ChooserT> = match pol {
        Pol::Being(l) => Box::new(LearnerT { cfg: l }),
        Pol::Random => Box::new(RandomT { rng: Rng::new(cfg.seed ^ 0xA11) }),
        Pol::AlwaysSql => Box::new(AlwaysSqlT),
        Pol::Ucb(c) => Box::new(UcbT::new(c)),
        Pol::Lin(c) => Box::new(LinUcbT::new(c)),
        Pol::Oracle => Box::new(OracleT::new()),
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
    let rules_noisy = eng.state.rules.iter().filter(|r| r.sig >= 120).count();
    let order = world.order.iter().enumerate().map(|(i, k)| (*k, world.intro_energy(i))).collect();
    LifeT { truths, reward: rew, cost, rules, rules_noisy, order }
}
