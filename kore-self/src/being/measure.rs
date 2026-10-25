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

// ---- Track B1: menu world ----

use super::chooser::{GreedyMenu, RandomMenu, UcbMenu};
use super::planner::{Action, Choice, MenuChooser, ModelChooser, Offer};
use super::solver::{Strategy, STRATEGIES};
use super::state::Chooser;

/// Budget of the longer menu runs.
pub const LONG_BUDGET: i64 = 3000;
const TRUTH_SAMPLES: u64 = 400;

/// Hidden truth: probability that each strategy answers right, per (domain, difficulty).
/// Uses the generator and the noise source; only the oracle and the tests touch it.
#[derive(Debug, Clone)]
pub struct Truth {
    pub p: [[[f64; 3]; 5]; 4],
}

pub fn truth(world_seed: u64) -> Truth {
    let w = World::new(world_seed);
    let th = ScriptedThinker;
    let st = [Default::default(); 4];
    let mut t = Truth { p: [[[0.0; 3]; 5]; 4] };
    for d in DOMAINS {
        for k in 1..=5u8 {
            for s in STRATEGIES {
                let mut ok = 0u64;
                for i in 0..TRUTH_SAMPLES {
                    let step = ORACLE_OFFSET + i;
                    let task = w.task_forced(step, d, k);
                    let ctx = Context { step, task: &task.view, noise: w.menu_noise(step, (i % 4) as usize), stats: &st };
                    ok += task.check(th.propose_with(&ctx, s).answer) as u64;
                }
                t.p[d.idx()][k as usize - 1][s.idx()] = ok as f64 / TRUTH_SAMPLES as f64;
            }
        }
    }
    t
}

/// Knows the hidden success probabilities and the real energy of each strategy.
pub struct OracleMenu {
    pub truth: Truth,
}

impl MenuChooser for OracleMenu {
    fn name(&self) -> String {
        "oracle".into()
    }
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        let mut best: Option<(f64, usize, Strategy)> = None;
        for o in offers {
            for s in STRATEGIES {
                let e = s.energy(o.listed_cost);
                if e > st.budget_left {
                    continue;
                }
                let v = o.listed_reward as f64 * self.truth.p[o.domain.idx()][o.difficulty as usize - 1][s.idx()] / e as f64;
                if best.map_or(true, |b| v > b.0) {
                    best = Some((v, o.slot, s));
                }
            }
        }
        let (v, slot, strategy) = best?;
        Some(Choice { action: Action::Attempt { slot, strategy }, policy: self.name(), why: "knows hidden truth".into(), p: 0.0, sd: 0.0, cost_est: 0.0, score: v, rationale: serde_json::json!({}) })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rec {
    pub reward: i64,
    pub cost: i64,
    pub attempted: bool,
    pub correct: bool,
    /// Predicted success probability made before the attempt (None: no prediction).
    pub pred: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct MenuRun {
    pub reward: i64,
    pub cost: i64,
    pub recs: Vec<Rec>,
    pub skips: u64,
    /// Strategy counts [fast, careful, deep].
    pub strategies: [u64; 3],
}

impl MenuRun {
    pub fn per_budget(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
    /// Reward per energy over steps `from..to` (0-based, by step index).
    pub fn window(&self, from: usize, to: usize) -> f64 {
        let s = &self.recs[from.min(self.recs.len())..to.min(self.recs.len())];
        let (r, c) = s.iter().fold((0i64, 0i64), |a, x| (a.0 + x.reward, a.1 + x.cost));
        if c > 0 { r as f64 / c as f64 } else { 0.0 }
    }
}

pub fn run_menu(world_seed: u64, budget: i64, chooser: &mut dyn MenuChooser) -> MenuRun {
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model }, String::new());
    let mut e = Engine::new(World::new(world_seed), st, &th);
    let (mut recs, mut skips, mut strategies) = (vec![], 0, [0u64; 3]);
    while let Ok(o) = e.step_menu(chooser) {
        let a = o.outcome;
        if !(o.predicted_p.is_some() || a.skipped) {
            continue;
        }
        if a.skipped {
            skips += 1;
        } else if let Some(s) = a.strategy {
            strategies[s.idx()] += 1;
        }
        recs.push(Rec { reward: a.reward, cost: a.cost, attempted: !a.skipped, correct: a.correct, pred: o.predicted_p });
    }
    MenuRun { reward: e.state.total_reward, cost: e.state.total_cost, recs, skips, strategies }
}

pub struct MenuRow {
    pub seed: u64,
    pub random: MenuRun,
    pub greedy: MenuRun,
    pub greedy_fast: MenuRun,
    pub greedy_deep: MenuRun,
    pub ucb: MenuRun,
    pub model: MenuRun,
    pub model_noskip: MenuRun,
    pub oracle: MenuRun,
}

pub fn menu_row(seed: u64, budget: i64, beta: f64, skip_frac: f64) -> MenuRow {
    let mut m = ModelChooser::new(beta);
    m.skip_frac = skip_frac;
    MenuRow {
        seed,
        random: run_menu(seed, budget, &mut RandomMenu::new(seed)),
        greedy: run_menu(seed, budget, &mut GreedyMenu { strategy: Strategy::Careful }),
        greedy_fast: run_menu(seed, budget, &mut GreedyMenu { strategy: Strategy::Fast }),
        greedy_deep: run_menu(seed, budget, &mut GreedyMenu { strategy: Strategy::Deep }),
        ucb: run_menu(seed, budget, &mut UcbMenu::new(seed, super::chooser::DEFAULT_EXPLORE_C)),
        model: run_menu(seed, budget, &mut m),
        model_noskip: run_menu(seed, budget, &mut ModelChooser { beta, skip_frac: 0.0 }),
        oracle: run_menu(seed, budget, &mut OracleMenu { truth: truth(seed) }),
    }
}

/// Reliability bins (10 equal-width bins of predicted success) and Brier scores.
/// `skip_first` attempts of each run are left out (the model starts from its prior).
pub struct Calibration {
    /// (bin low, count, mean predicted, observed rate)
    pub bins: Vec<(f64, u64, f64, f64)>,
    pub brier: f64,
    /// Brier of "predict the running success rate so far" on the same attempts.
    pub brier_base_rate: f64,
    pub n: u64,
}

pub fn calibration(runs: &[&MenuRun], skip_first: usize) -> Calibration {
    let mut acc = [(0u64, 0.0f64, 0u64); 10];
    let (mut b, mut bb, mut n) = (0.0, 0.0, 0u64);
    for r in runs {
        let (mut seen, mut hits) = (0u64, 0u64);
        let mut k = 0usize;
        for x in r.recs.iter().filter(|x| x.attempted) {
            let base = (hits as f64 + 1.0) / (seen as f64 + 2.0);
            if let (true, Some(p)) = (k >= skip_first, x.pred) {
                let y = x.correct as u8 as f64;
                b += (p - y).powi(2);
                bb += (base - y).powi(2);
                n += 1;
                let i = ((p * 10.0) as usize).min(9);
                acc[i].0 += 1;
                acc[i].1 += p;
                acc[i].2 += x.correct as u64;
            }
            seen += 1;
            hits += x.correct as u64;
            k += 1;
        }
    }
    let bins = acc.iter().enumerate().map(|(i, a)| (i as f64 / 10.0, a.0, if a.0 > 0 { a.1 / a.0 as f64 } else { 0.0 }, if a.0 > 0 { a.2 as f64 / a.0 as f64 } else { 0.0 })).collect();
    Calibration { bins, brier: b / n.max(1) as f64, brier_base_rate: bb / n.max(1) as f64, n }
}
