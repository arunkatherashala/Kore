//! Measurement helpers (shared by the unit test floors and examples/measure_being.rs).
//! In-memory runs of the same engine the logged loop uses; no log is written.

use super::chooser::{DomainPicker, FixedPicker, RandomPicker, Ucb1};
use super::run_loop::Engine;
use super::solver::{Context, ScriptedThinker, Thinker};
use super::state::{BeingState, Genesis};
use super::world::{Domain, World, DOMAINS};

pub const BUDGET: i64 = 600;
/// Steps offset used to estimate the hidden truth, far from any run.
const ORACLE_OFFSET: u64 = 10_000_000;
const ORACLE_SAMPLES: u64 = 3000;

/// Hidden truth: expected reward per energy of each domain under the scripted thinker.
pub fn true_efficiency(world_seed: u64) -> [f64; 4] {
    let w = World::new(world_seed);
    let th = ScriptedThinker;
    let st = [Default::default(); 4];
    let mut out = [0.0; 4];
    for d in DOMAINS {
        let (mut r, mut c) = (0i64, 0i64);
        for s in 0..ORACLE_SAMPLES {
            let step = ORACLE_OFFSET + s;
            let t = w.task(step, d);
            let ctx = Context { step, task: &t.view, noise: w.noise(step, d), stats: &st };
            if t.check(th.propose(&ctx).answer) {
                r += t.view.reward;
            }
            c += t.view.cost;
        }
        out[d.idx()] = r as f64 / c as f64;
    }
    out
}

pub fn oracle_domain(world_seed: u64) -> Domain {
    let e = true_efficiency(world_seed);
    let mut best = Domain::Arithmetic;
    for d in DOMAINS {
        if e[d.idx()] > e[best.idx()] {
            best = d;
        }
    }
    best
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub reward: i64,
    pub cost: i64,
    pub steps: u64,
    pub picks: Vec<Domain>,
}

impl RunResult {
    pub fn per_budget(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
}

pub fn run_policy(world_seed: u64, budget: i64, picker: &mut dyn DomainPicker) -> RunResult {
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed, loop_seed: 0, budget, explore_c: 0.0, chooser: super::state::Chooser::Ucb }, String::new());
    let mut e = Engine::new(World::new(world_seed), st, &th);
    let mut picks = vec![];
    while let Ok(o) = e.step(picker) {
        picks.push(o.outcome.domain);
    }
    RunResult { reward: e.state.total_reward, cost: e.state.total_cost, steps: e.state.step, picks }
}

pub fn learned(world_seed: u64, run_seed: u64, c: f64, budget: i64) -> RunResult {
    run_policy(world_seed, budget, &mut Ucb1::new(run_seed, c))
}

/// All six policies for one seed: random, always x4, oracle, learned.
pub struct SeedRow {
    pub seed: u64,
    pub best: Domain,
    pub random: RunResult,
    pub always: [RunResult; 4],
    pub oracle: RunResult,
    pub learned: RunResult,
}

pub fn seed_row(seed: u64, c: f64, budget: i64) -> SeedRow {
    let best = oracle_domain(seed);
    SeedRow {
        seed,
        best,
        random: run_policy(seed, budget, &mut RandomPicker::new(seed)),
        always: [
            run_policy(seed, budget, &mut FixedPicker::always(Domain::Arithmetic)),
            run_policy(seed, budget, &mut FixedPicker::always(Domain::Algebra)),
            run_policy(seed, budget, &mut FixedPicker::always(Domain::Physics)),
            run_policy(seed, budget, &mut FixedPicker::always(Domain::Pattern)),
        ],
        oracle: run_policy(seed, budget, &mut FixedPicker::oracle(best)),
        learned: learned(seed, seed, c, budget),
    }
}

/// Share of the picks in `range` that went to `d`.
pub fn share(picks: &[Domain], d: Domain, from: usize, to: usize) -> f64 {
    let s = &picks[from.min(picks.len())..to.min(picks.len())];
    if s.is_empty() { 0.0 } else { s.iter().filter(|x| **x == d).count() as f64 / s.len() as f64 }
}

/// First step at which the trailing 20-step window has >= 60% picks of `d`.
pub fn concentration_step(picks: &[Domain], d: Domain) -> Option<usize> {
    (20..=picks.len()).find(|&e| share(picks, d, e - 20, e) >= 0.6)
}

pub fn mean_ci(xs: &[f64]) -> (f64, f64, f64) {
    let n = xs.len() as f64;
    let m = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    let h = 2.093 * (var / n).sqrt(); // t(0.975, 19)
    (m, m - h, m + h)
}
