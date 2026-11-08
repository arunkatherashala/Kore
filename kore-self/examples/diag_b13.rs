//! Track B13 diagnosis: why does a tuned LinUCB beat the being's chooser where rules matter little?
//! Tuning seeds 101..120 only. Synthetic worlds, author-defined families and rewards.
//!
//!   cargo run -p kore-self --example diag_b13 --offline --release -- v2 | b2 | l | ablate-v2
//!
//! `v2`: every decision of a policy in World v2 is scored with the TRUE expected value of each option
//! (measurement only, through the World v2 oracle's truth table; never seen by the policy) and the loss
//! against the best option on the same menu is split by where it arises.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::lookahead::beingl::{run_wl, GreedyL};
use being::lookahead::worldl::{WLCfg, SETTINGS};
use being::measure::{run_menu_ext, DISCOVER_EVERY};
use being::planner::{ModelChooser, SkipRule, DEFAULT_BETA, DEFAULT_SETTLE_K};
use being::solver::{Strategy, STRATEGIES};
use being::vs_world::adapt::{Player2, PlayerL, PlayerM};
use being::vs_world::core::{lin_features, Agent, Ctx, Opt, Ridge, FAM_DIM, LIN_D, SKIP_MIN_COST, TYPE_DIM};
use being::world::World;
use being::world2::all_skills::profile_use;
use being::world2::baselines::Oracle2;
use being::world2::engine::{LearnerCfg, State2};
use being::world2::learner::{Act2, Choice2, Chooser2, Learner2, Offer2};
use being::world2::measure::{run_w2, run_w2_with};
use being::world2::rules_use::{covered, UseCfg};
use being::world2::solve::slip_pm;
use being::world2::world::{W2Cfg, World2};
use being::world_o::engine::LearnerO;
use rayon::prelude::*;

const TUNING: std::ops::RangeInclusive<u64> = 101..=120;

fn mci(xs: &[f64]) -> (f64, f64, f64) {
    let n = xs.len() as f64;
    let m = xs.iter().sum::<f64>() / n;
    let v = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    let h = 2.093 * (v / n).sqrt();
    (m, m - h, m + h)
}
fn fd(a: &[f64], b: &[f64]) -> String {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mci(&d);
    let w = d.iter().filter(|x| **x > 1e-9).count();
    let l = d.iter().filter(|x| **x < -1e-9).count();
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}] {w}/{l}/{}", d.len() - w - l)
}
fn fm(a: &[f64]) -> String {
    let (m, lo, hi) = mci(a);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

// ---- World v2 probe ----

#[derive(Default, Clone, Debug)]
struct Stats {
    steps: u64,
    skips: u64,
    attempts: u64,
    energy: f64,
    /// Energy-weighted loss of the chosen option against the best option on the same menu (true values).
    gap_e: f64,
    /// ... of attempts on a cell (family, difficulty, strategy) with < 3 earlier attempts ("young").
    gap_young: f64,
    e_young: f64,
    /// ... of attempts on cells with >= 3 attempts, chosen offer = the best offer (strategy wrong).
    gap_strat: f64,
    /// ... with >= 3 attempts, another offer.
    gap_offer: f64,
    /// `discovered` with no covering rule (careful under another name).
    disc_nocover: u64,
    e_disc_nocover: f64,
    /// Skipped menus: their best true value (later compared with the life's own rate).
    skipped_best: Vec<f64>,
    /// Attempts: (true value of the chosen option, its energy).
    attempted_v: Vec<(f64, f64)>,
    true_reward: f64,
}

struct Probe<'a> {
    inner: &'a mut dyn Chooser2,
    oracle: Oracle2,
    s: Stats,
}

impl<'a> Probe<'a> {
    fn value(&mut self, st: &State2, o: &Offer2, s: Strategy) -> (f64, f64) {
        let slip = slip_pm(Strategy::Careful, self.oracle.world.fams[o.fam].slip_pm) as f64 / 1000.0;
        let r = o.listed_reward as f64;
        match s {
            Strategy::Discovered => {
                let e = o.listed_cost as f64;
                let p = if covered(st, o) { 1.0 - slip } else { self.oracle.truth(o.fam, o.difficulty, Strategy::Careful, st.clock) };
                (r * p / e, e)
            }
            _ => {
                let e = s.energy(o.listed_cost) as f64;
                (r * self.oracle.truth(o.fam, o.difficulty, s, st.clock) / e, e)
            }
        }
    }
}

impl<'a> Chooser2 for Probe<'a> {
    fn name(&self) -> String {
        self.inner.name()
    }
    fn prepare_o(&mut self, st: &being::world_o::engine::StateO) {
        self.inner.prepare_o(st)
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let ch = self.inner.choose(st, offers)?;
        self.s.steps += 1;
        let mut best = (f64::MIN, usize::MAX);
        for o in offers {
            for s in STRATEGIES {
                if s.energy(o.listed_cost) <= st.budget_left {
                    let (v, _) = self.value(st, o, s);
                    if v > best.0 {
                        best = (v, o.slot);
                    }
                }
            }
            if covered(st, o) && o.listed_cost <= st.budget_left {
                let (v, _) = self.value(st, o, Strategy::Discovered);
                if v > best.0 {
                    best = (v, o.slot);
                }
            }
        }
        match ch.act {
            Act2::Attempt { slot, strategy } => {
                let o = *offers.iter().find(|o| o.slot == slot).unwrap();
                let (v, e) = self.value(st, &o, strategy);
                let n = st.model.cell(o.fam, o.difficulty, strategy).n;
                let gap = (best.0 - v).max(0.0) * e;
                self.s.attempts += 1;
                self.s.energy += e;
                self.s.gap_e += gap;
                self.s.attempted_v.push((v, e));
                if n < 3 {
                    self.s.gap_young += gap;
                    self.s.e_young += e;
                } else if slot == best.1 {
                    self.s.gap_strat += gap;
                } else {
                    self.s.gap_offer += gap;
                }
                if strategy == Strategy::Discovered && !covered(st, &o) {
                    self.s.disc_nocover += 1;
                    self.s.e_disc_nocover += e;
                }
            }
            Act2::Skip => {
                self.s.skips += 1;
                self.s.skipped_best.push(best.0);
            }
            _ => {}
        }
        Some(ch)
    }
}

// ---- LinUCB variants (diagnosis only) ----

#[derive(Clone, Copy, Debug, PartialEq)]
enum Feat {
    /// The tuned LinUCB of Track V1 (bias, family, type, difficulty, type x difficulty).
    Full,
    /// bias + family only (no sharing of difficulty effects).
    FamOnly,
    /// bias + family x difficulty cells (one free parameter per cell, like the B1 model's leaves).
    Cells,
    /// Full without the family one-hot (only type and difficulty structure).
    NoFam,
}

struct LinVar {
    alpha: f64,
    feat: Feat,
    skip: bool,
    /// Use the reward share (false) or score with p only.
    models: Vec<Ridge>,
}

const CELL_D: usize = 1 + FAM_DIM * 5;

fn feats(f: Feat, o: &Opt, cx: &Ctx) -> Vec<(usize, f64)> {
    let d = (o.diff.clamp(1, 5) - 1) as usize;
    let t = (o.ftype as usize).min(TYPE_DIM - 1);
    match f {
        Feat::Full => lin_features(o, cx),
        Feat::FamOnly => vec![(0, 1.0), (1 + o.fam % FAM_DIM, 1.0)],
        Feat::Cells => vec![(0, 1.0), (1 + (o.fam % FAM_DIM) * 5 + d, 1.0)],
        Feat::NoFam => vec![(0, 1.0), (1 + FAM_DIM + t, 1.0), (1 + FAM_DIM + TYPE_DIM + d, 1.0), (1 + FAM_DIM + TYPE_DIM + 5 + t * 5 + d, 1.0)],
    }
}

impl LinVar {
    fn new(alpha: f64, feat: Feat, skip: bool) -> LinVar {
        let d = if feat == Feat::Cells { CELL_D } else { LIN_D };
        LinVar { alpha, feat, skip, models: vec![Ridge::new(d); 3] }
    }
}

impl Agent for LinVar {
    fn name(&self) -> String {
        format!("lin-{:?}-a{}", self.feat, self.alpha)
    }
    fn choose(&mut self, cx: &Ctx, opts: &[Opt]) -> usize {
        let mut best: Option<(usize, f64)> = None;
        for (i, o) in opts.iter().enumerate() {
            let Some(s) = o.strategy else { continue };
            let x = feats(self.feat, o, cx);
            let m = &self.models[s.idx().min(2)];
            let p = (m.mean(&x) + self.alpha * m.quad(&x).sqrt()).clamp(0.0, 1.0);
            let sc = o.listed_reward * p / o.energy;
            if best.map_or(true, |b| sc > b.1) {
                best = Some((i, sc));
            }
        }
        let skip_i = opts.iter().position(|o| o.is_skip()).unwrap_or(0);
        let Some((b, s)) = best else { return skip_i };
        if self.skip && cx.spent >= SKIP_MIN_COST && s < cx.rate() {
            return skip_i;
        }
        b
    }
    fn observe(&mut self, b: &Ctx, o: &Opt, reward: f64, _c: f64, _a: &Ctx) {
        if let Some(s) = o.strategy {
            let x = feats(self.feat, o, b);
            let y = if o.listed_reward > 0.0 { (reward / o.listed_reward).clamp(0.0, 1.0) } else { 0.0 };
            self.models[s.idx().min(2)].update(&x, y);
        }
    }
}

// ---- World v2 policies ----

#[derive(Clone, Copy, Debug)]
enum P2 {
    Current,
    Beta(f64),
    NoArm,
    Recommended,
    Lin(f64, Feat),
    B13(bool),
}

fn name2(p: P2) -> String {
    match p {
        P2::Current => "ours current (B1 planner, beta 1.5)".into(),
        P2::Beta(b) => format!("ours current, beta {b}"),
        P2::NoArm => "ours current, A2 (discovered only under a rule)".into(),
        P2::Recommended => "ours recommended (B7 + settled skip)".into(),
        P2::Lin(a, f) => format!("LinUCB {f:?} alpha {a}"),
        P2::B13(r) => if r { "ours B13 profile (recommended + B13 chooser)".into() } else { "ours current + B13 chooser".into() },
    }
}

fn run2(p: P2, cfg: &W2Cfg) -> Stats {
    let mut oracle = Oracle2::new(World2::new(cfg.clone()), false);
    oracle.skip = false;
    let (r, s) = match p {
        P2::Current | P2::Beta(_) | P2::NoArm => {
            let mut l = Learner2::default();
            if let P2::Beta(b) = p {
                l.beta = b;
            }
            let mut lc = LearnerCfg::default();
            if let P2::NoArm = p {
                l.use_ = UseCfg { rule_only: true, ..UseCfg::default() };
                lc.use_ = l.use_;
            }
            let mut pr = Probe { inner: &mut l, oracle, s: Stats::default() };
            let r = run_w2_with(cfg, &mut pr, 10, None, lc);
            (r, pr.s)
        }
        P2::Recommended => {
            let lc = LearnerCfg { use_: profile_use(), ..LearnerCfg::default() };
            let mut l = LearnerO { inner: lc.learner(), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), l2l: Default::default(), view: None };
            let mut pr = Probe { inner: &mut l, oracle, s: Stats::default() };
            let r = run_w2_with(cfg, &mut pr, 10, None, lc);
            (r, pr.s)
        }
        P2::B13(rec) => {
            let lin = Some(being::linval::LIN_V2);
            if rec {
                let lc = LearnerCfg { use_: profile_use(), lin, ..LearnerCfg::default() };
                let mut l = LearnerO { inner: lc.learner(), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), l2l: Default::default(), view: None };
                let mut pr = Probe { inner: &mut l, oracle, s: Stats::default() };
                let r = run_w2_with(cfg, &mut pr, 10, None, lc);
                (r, pr.s)
            } else {
                let lc = LearnerCfg { lin, ..LearnerCfg::default() };
                let mut l = lc.learner();
                let mut pr = Probe { inner: &mut l, oracle, s: Stats::default() };
                let r = run_w2_with(cfg, &mut pr, 10, None, lc);
                (r, pr.s)
            }
        }
        P2::Lin(a, f) => {
            let mut pl = Player2::new(Box::new(LinVar::new(a, f, true)));
            let mut pr = Probe { inner: &mut pl, oracle, s: Stats::default() };
            let r = run_w2(cfg, &mut pr, 0, None);
            (r, pr.s)
        }
    };
    let mut s = s;
    s.true_reward = r.per_energy();
    s
}

fn conds2() -> Vec<(&'static str, usize, i64, u32, u32, u32)> {
    vec![("small clean", 6, 1500, 0, 0, 0), ("medium clean", 12, 3000, 0, 0, 0), ("medium hard", 12, 3000, 100, 20, 100), ("large clean", 24, 6000, 0, 0, 0)]
}

fn diag_v2(policies: &[P2]) {
    let seeds: Vec<u64> = TUNING.collect();
    for (name, fams, budget, noise, drift, curve) in conds2() {
        let jobs: Vec<(usize, u64)> = (0..policies.len()).flat_map(|i| seeds.iter().map(move |s| (i, *s))).collect();
        let res: Vec<Stats> = jobs.par_iter().map(|(i, s)| run2(policies[*i], &W2Cfg { noise_pm: noise, drift_per_10k: drift, curve_pct: curve, ..W2Cfg::new(*s, fams, budget) })).collect();
        let get = |i: usize| -> Vec<&Stats> { jobs.iter().zip(&res).filter(|(j, _)| j.0 == i).map(|(_, r)| r).collect() };
        println!("\n### World v2 {name}, tuning seeds 101..120 (n = 20)\n");
        println!("| policy | true reward per energy | minus first row | skip share of steps | menu-choice loss per attempted energy (all) | of which young cells (<3 tries) | strategy on the best offer | another offer | energy share in young cells | `discovered` without a rule, share of energy | skipped menus whose best option beat the life's rate (per 100 steps) | attempts below 0.7 x the life's rate (per 100 steps) |");
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
        let base: Vec<f64> = get(0).iter().map(|s| s.true_reward).collect();
        for (i, p) in policies.iter().enumerate() {
            let g = get(i);
            let rpe: Vec<f64> = g.iter().map(|s| s.true_reward).collect();
            let per = |f: &dyn Fn(&Stats) -> f64| -> Vec<f64> { g.iter().map(|s| f(s)).collect() };
            let skip = per(&|s| s.skips as f64 / s.steps.max(1) as f64);
            let gap = per(&|s| s.gap_e / s.energy.max(1.0));
            let gy = per(&|s| s.gap_young / s.energy.max(1.0));
            let gs = per(&|s| s.gap_strat / s.energy.max(1.0));
            let go = per(&|s| s.gap_offer / s.energy.max(1.0));
            let ey = per(&|s| s.e_young / s.energy.max(1.0));
            let dn = per(&|s| s.e_disc_nocover / s.energy.max(1.0));
            let over = per(&|s| 100.0 * s.skipped_best.iter().filter(|b| **b > s.true_reward).count() as f64 / s.steps.max(1) as f64);
            let under = per(&|s| 100.0 * s.attempted_v.iter().filter(|(v, _)| *v < 0.7 * s.true_reward).count() as f64 / s.steps.max(1) as f64);
            println!("| {} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.2} | {:.2} |", name2(*p), fm(&rpe), fd(&rpe, &base), mci(&skip).0, mci(&gap).0, mci(&gy).0, mci(&gs).0, mci(&go).0, mci(&ey).0, mci(&dn).0, mci(&over).0, mci(&under).0);
        }
    }
}

// ---- first world and World L: ablations on tuning seeds ----

fn diag_b2() {
    let seeds: Vec<u64> = TUNING.collect();
    for budget in [600i64, 3000] {
        let names = ["current (B1, beta 1.5)", "beta 1.0", "beta 0.5", "no discovered arm", "skip settled", "LinUCB Full a0.5 (tuned)", "LinUCB FamOnly", "LinUCB Cells", "LinUCB NoFam"];
        let jobs: Vec<(usize, u64)> = (0..names.len()).flat_map(|i| seeds.iter().map(move |s| (i, *s))).collect();
        let res: Vec<f64> = jobs
            .par_iter()
            .map(|(i, s)| {
                let w = World::with_extended(*s, true);
                let m = |c: &mut ModelChooser| run_menu_ext(World::with_extended(*s, true), budget, c, DISCOVER_EVERY, &[]).run.per_budget();
                match i {
                    0 => m(&mut ModelChooser::new(DEFAULT_BETA)),
                    1 => m(&mut ModelChooser::new(1.0)),
                    2 => m(&mut ModelChooser::new(0.5)),
                    3 => m(&mut ModelChooser { no_discovered: true, ..ModelChooser::new(DEFAULT_BETA) }),
                    4 => m(&mut ModelChooser { skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), ..ModelChooser::new(DEFAULT_BETA) }),
                    _ => {
                        let f = [Feat::Full, Feat::FamOnly, Feat::Cells, Feat::NoFam][*i - 5];
                        run_menu_ext(w, budget, &mut PlayerM::new(Box::new(LinVar::new(0.5, f, true))), 0, &[]).run.per_budget()
                    }
                }
            })
            .collect();
        let get = |i: usize| -> Vec<f64> { jobs.iter().zip(&res).filter(|(j, _)| j.0 == i).map(|(_, r)| *r).collect() };
        println!("\n### First world (B2 families), budget {budget}, tuning seeds\n\n| policy | reward per energy | minus current | minus LinUCB Full |\n|---|---|---|---|");
        for (i, n) in names.iter().enumerate() {
            println!("| {n} | {} | {} | {} |", fm(&get(i)), fd(&get(i), &get(0)), fd(&get(i), &get(5)));
        }
    }
}

fn diag_l() {
    let seeds: Vec<u64> = TUNING.collect();
    let names = ["B1 (GreedyL, beta 1.5)", "beta 1.0", "beta 0.5", "B1 no skip", "LinUCB Full a1 no skip (tuned)", "LinUCB Full a1 with skip", "LinUCB Cells a1 no skip"];
    for st in SETTINGS {
        let jobs: Vec<(usize, u64)> = (0..names.len()).flat_map(|i| seeds.iter().map(move |s| (i, *s))).collect();
        let res: Vec<f64> = jobs
            .par_iter()
            .map(|(i, s)| {
                let cfg = WLCfg { seed: *s, setting: st, budget: 400 };
                match i {
                    0 => run_wl(&cfg, &mut GreedyL::default()).per_energy(),
                    1 => run_wl(&cfg, &mut GreedyL { beta: 1.0, ..GreedyL::default() }).per_energy(),
                    2 => run_wl(&cfg, &mut GreedyL { beta: 0.5, ..GreedyL::default() }).per_energy(),
                    3 => run_wl(&cfg, &mut GreedyL { skip_frac: 0.0, ..GreedyL::default() }).per_energy(),
                    4 => run_wl(&cfg, &mut PlayerL::new(Box::new(LinVar::new(1.0, Feat::Full, false)))).per_energy(),
                    5 => run_wl(&cfg, &mut PlayerL::new(Box::new(LinVar::new(1.0, Feat::Full, true)))).per_energy(),
                    _ => run_wl(&cfg, &mut PlayerL::new(Box::new(LinVar::new(1.0, Feat::Cells, false)))).per_energy(),
                }
            })
            .collect();
        let get = |i: usize| -> Vec<f64> { jobs.iter().zip(&res).filter(|(j, _)| j.0 == i).map(|(_, r)| *r).collect() };
        println!("\n### World L {} budget 400, tuning seeds\n\n| policy | reward per energy | minus B1 | minus LinUCB tuned |\n|---|---|---|---|", st.name());
        for (i, n) in names.iter().enumerate() {
            println!("| {n} | {} | {} | {} |", fm(&get(i)), fd(&get(i), &get(0)), fd(&get(i), &get(4)));
        }
    }
}

// ---- B13 tuning (tuning seeds only) ----

use being::linval::LinCfg;

fn grid_lin() -> Vec<LinCfg> {
    let mut v = vec![];
    for a in [250u32, 500, 1000] {
        for k in [0u32, 1000, 4000, 16000] {
            for t in [true, false] {
                v.push(LinCfg { alpha_milli: a, cell_k_milli: k, types: t });
            }
        }
    }
    v
}

fn w2_run(rec: bool, lin: Option<LinCfg>, cfg: &W2Cfg) -> f64 {
    if rec {
        let lc = LearnerCfg { use_: profile_use(), lin, ..LearnerCfg::default() };
        let mut l = LearnerO { inner: lc.learner(), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), l2l: Default::default(), view: None };
        run_w2_with(cfg, &mut l, 10, None, lc).per_energy()
    } else {
        let lc = LearnerCfg { lin, ..LearnerCfg::default() };
        run_w2_with(cfg, &mut lc.learner(), 10, None, lc).per_energy()
    }
}

fn grid_v2_ext() -> Vec<LinCfg> {
    let mut v = vec![];
    for a in [350u32, 500, 700] {
        for k in [16000u32, 64000, 256000] {
            v.push(LinCfg { alpha_milli: a, cell_k_milli: k, types: false });
        }
    }
    v
}

fn grid_l_ext() -> Vec<LinCfg> {
    let mut v = vec![];
    for a in [1000u32, 1500, 2000, 3000] {
        for k in [0u32, 16000] {
            v.push(LinCfg { alpha_milli: a, cell_k_milli: k, types: true });
        }
    }
    v
}

fn tune_v2(ext: bool) {
    let seeds: Vec<u64> = TUNING.collect();
    let cs = conds2();
    let mut configs: Vec<Option<LinCfg>> = vec![None];
    configs.extend(if ext { grid_v2_ext() } else { grid_lin() }.into_iter().map(Some));
    for rec in [false, true] {
        println!("
### B13 tuning, World v2 ({}), tuning seeds 101..120; objective = mean over small clean, medium clean, medium hard, large clean
", if rec { "on top of recommended: B7 + settled skip" } else { "on top of current" });
        println!("| B13 config | small clean | medium clean | medium hard | large clean | objective | minus off |");
        println!("|---|---|---|---|---|---|---|");
        let jobs: Vec<(usize, usize, u64)> = (0..configs.len()).flat_map(|i| (0..cs.len()).flat_map(move |c| TUNING.map(move |s| (i, c, s)))).collect();
        let res: Vec<f64> = jobs
            .par_iter()
            .map(|(i, c, s)| {
                let (_, fams, budget, noise, drift, curve) = cs[*c];
                w2_run(rec, configs[*i], &W2Cfg { noise_pm: noise, drift_per_10k: drift, curve_pct: curve, ..W2Cfg::new(*s, fams, budget) })
            })
            .collect();
        let get = |i: usize, c: usize| -> Vec<f64> { jobs.iter().zip(&res).filter(|(j, _)| j.0 == i && j.1 == c).map(|(_, r)| *r).collect() };
        let obj = |i: usize| -> Vec<f64> { (0..seeds.len()).map(|k| (0..cs.len()).map(|c| get(i, c)[k]).sum::<f64>() / cs.len() as f64).collect() };
        let base = obj(0);
        let mut best = (0usize, f64::MIN);
        for i in 0..configs.len() {
            let o = obj(i);
            let m = mci(&o).0;
            if m > best.1 {
                best = (i, m);
            }
            println!("| {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {} |", configs[i].map_or("off".into(), |c| c.code()), mci(&get(i, 0)).0, mci(&get(i, 1)).0, mci(&get(i, 2)).0, mci(&get(i, 3)).0, m, fd(&o, &base));
        }
        println!("
best: {}", configs[best.0].map_or("off".into(), |c| c.code()));
    }
}

fn tune_b2() {
    let seeds: Vec<u64> = TUNING.collect();
    let mut configs: Vec<Option<LinCfg>> = vec![None];
    configs.extend(grid_lin().into_iter().filter(|c| c.types).map(Some));
    for settled in [false, true] {
        println!("
### B13 tuning, first world (B2 families, discovery every 10 steps{}), tuning seeds; objective = mean over budgets 600 and 3000
", if settled { ", settled skip" } else { "" });
        println!("| B13 config | 600 | 3000 | objective | minus off |
|---|---|---|---|---|");
        let jobs: Vec<(usize, i64, u64)> = (0..configs.len()).flat_map(|i| [600i64, 3000].into_iter().flat_map(move |b| TUNING.map(move |s| (i, b, s)))).collect();
        let res: Vec<f64> = jobs
            .par_iter()
            .map(|(i, b, s)| {
                let mut c = ModelChooser { lin: configs[*i], ..ModelChooser::new(DEFAULT_BETA) };
                if settled {
                    c.skip_rule = SkipRule::Settled(DEFAULT_SETTLE_K);
                }
                run_menu_ext(World::with_extended(*s, true), *b, &mut c, DISCOVER_EVERY, &[]).run.per_budget()
            })
            .collect();
        let get = |i: usize, b: i64| -> Vec<f64> { jobs.iter().zip(&res).filter(|(j, _)| j.0 == i && j.1 == b).map(|(_, r)| *r).collect() };
        let obj = |i: usize| -> Vec<f64> { (0..seeds.len()).map(|k| (get(i, 600)[k] + get(i, 3000)[k]) / 2.0).collect() };
        let base = obj(0);
        for i in 0..configs.len() {
            println!("| {} | {:.3} | {:.3} | {:.3} | {} |", configs[i].map_or("off".into(), |c| c.code()), mci(&get(i, 600)).0, mci(&get(i, 3000)).0, mci(&obj(i)).0, fd(&obj(i), &base));
        }
    }
}

fn tune_l(ext: bool) {
    let seeds: Vec<u64> = TUNING.collect();
    let mut configs: Vec<Option<LinCfg>> = vec![None];
    configs.extend(if ext { grid_l_ext() } else { grid_lin().into_iter().filter(|c| c.types).collect() }.into_iter().map(Some));
    for (skipname, skip_frac, rule) in [("B1 skip", 1.0, SkipRule::Rate), ("no skip", 0.0, SkipRule::Rate), ("settled skip", 1.0, SkipRule::Settled(DEFAULT_SETTLE_K))] {
        println!("
### B13 tuning, World L budget 400, {skipname}, tuning seeds; objective = mean over the four settings
");
        println!("| B13 config | plain | keys | probe | sizes | objective | minus B1 (off, B1 skip) |
|---|---|---|---|---|---|---|");
        let jobs: Vec<(usize, usize, u64)> = (0..configs.len()).flat_map(|i| (0..4).flat_map(move |st| TUNING.map(move |s| (i, st, s)))).collect();
        let res: Vec<f64> = jobs
            .par_iter()
            .map(|(i, st, s)| {
                let cfg = WLCfg { seed: *s, setting: SETTINGS[*st], budget: 400 };
                run_wl(&cfg, &mut GreedyL { lin: configs[*i], skip_frac, skip_rule: rule, ..GreedyL::default() }).per_energy()
            })
            .collect();
        let b1: Vec<f64> = TUNING.map(|s| (0..4).map(|st| run_wl(&WLCfg { seed: s, setting: SETTINGS[st], budget: 400 }, &mut GreedyL::default()).per_energy()).sum::<f64>() / 4.0).collect();
        let get = |i: usize, st: usize| -> Vec<f64> { jobs.iter().zip(&res).filter(|(j, _)| j.0 == i && j.1 == st).map(|(_, r)| *r).collect() };
        let obj = |i: usize| -> Vec<f64> { (0..seeds.len()).map(|k| (0..4).map(|st| get(i, st)[k]).sum::<f64>() / 4.0).collect() };
        for i in 0..configs.len() {
            println!("| {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {} |", configs[i].map_or("off".into(), |c| c.code()), mci(&get(i, 0)).0, mci(&get(i, 1)).0, mci(&get(i, 2)).0, mci(&get(i, 3)).0, mci(&obj(i)).0, fd(&obj(i), &b1));
        }
    }
}

fn diag_b2_disc() {
    use being::linval::LIN_MENU;
    let names = ["B13 (a500/k4000)", "B13, no discovered arm, no discovery", "B13 + settled skip", "LinUCB tuned"];
    for budget in [600i64, 3000] {
        let jobs: Vec<(usize, u64)> = (0..names.len()).flat_map(|i| TUNING.map(move |s| (i, s))).collect();
        let res: Vec<f64> = jobs
            .par_iter()
            .map(|(i, s)| {
                let w = World::with_extended(*s, true);
                match i {
                    0 => run_menu_ext(w, budget, &mut ModelChooser { lin: Some(LIN_MENU), ..ModelChooser::new(DEFAULT_BETA) }, DISCOVER_EVERY, &[]).run.per_budget(),
                    1 => run_menu_ext(w, budget, &mut ModelChooser { lin: Some(LIN_MENU), no_discovered: true, ..ModelChooser::new(DEFAULT_BETA) }, 0, &[]).run.per_budget(),
                    2 => run_menu_ext(w, budget, &mut ModelChooser { lin: Some(LIN_MENU), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), ..ModelChooser::new(DEFAULT_BETA) }, DISCOVER_EVERY, &[]).run.per_budget(),
                    _ => run_menu_ext(w, budget, &mut PlayerM::new(Box::new(LinVar::new(0.5, Feat::Full, true))), 0, &[]).run.per_budget(),
                }
            })
            .collect();
        let get = |i: usize| -> Vec<f64> { jobs.iter().zip(&res).filter(|(j, _)| j.0 == i).map(|(_, r)| *r).collect() };
        println!("
### First world, budget {budget}, tuning seeds: the discovered arm under B13

| policy | reward per energy | minus B13 | minus LinUCB |
|---|---|---|---|");
        for (i, n) in names.iter().enumerate() {
            println!("| {n} | {} | {} | {} |", fm(&get(i)), fd(&get(i), &get(0)), fd(&get(i), &get(3)));
        }
    }
}

fn diag_o() {
    use being::world_o::measure::run_wo;
    use being::world_o::world::WOCfg;
    use being::world2::world::FType;
    let seeds: Vec<u64> = (101..=110).collect();
    let names = ["current", "current + B13 (LIN_V2)"];
    let limits = || being::controller::Limits { max_live_rules: 200, max_candidates: 100_000, compose: false, max_blocks: 8, ..being::controller::Limits::default() };
    let jobs: Vec<(usize, u64)> = (0..2).flat_map(|j| seeds.iter().map(move |s| (j, *s))).collect();
    let res: Vec<_> = jobs
        .par_iter()
        .map(|(j, s)| {
            let c = WOCfg::new(*s, 300_000);
            let l = being::world_o::engine::LearnerOCfg { lin: if *j == 1 { Some(being::linval::LIN_V2) } else { None }, ..Default::default() };
            run_wo(&c, l, None, 10, Some(limits()), u64::MAX, 100_000, true)
        })
        .collect();
    println!("
### World O diagnosis of B13, tuning seeds 101..110, 300,000 energy
");
    println!("| policy | reward per energy after 100k | rules accepted | attempts per step | attempts per family: seq | compose | formula | grid | knap | families reaching competence |
|---|---|---|---|---|---|---|---|---|---|");
    for (j, n) in names.iter().enumerate() {
        let rs: Vec<_> = jobs.iter().zip(&res).filter(|(x, _)| x.0 == j).map(|(_, r)| r).collect();
        let rpe: Vec<f64> = rs.iter().map(|r| { let (x, y) = r.wins[1..].iter().fold((0i64, 0i64), |a, w| (a.0 + w.true_reward, a.1 + w.cost)); x as f64 / y.max(1) as f64 }).collect();
        let rules: Vec<f64> = rs.iter().map(|r| r.rules_total as f64).collect();
        let aps: Vec<f64> = rs.iter().map(|r| r.fams.iter().map(|f| f.attempts).sum::<u64>() as f64 / r.steps.max(1) as f64).collect();
        let per = |t: FType| -> f64 { let v: Vec<f64> = rs.iter().flat_map(|r| r.fams.iter().filter(|f| f.ftype == Some(t)).map(|f| f.attempts as f64)).collect(); v.iter().sum::<f64>() / v.len().max(1) as f64 };
        let comp: usize = rs.iter().map(|r| r.fams.iter().filter(|f| f.competent_after.is_some()).count()).sum();
        let tot: usize = rs.iter().map(|r| r.fams.len()).sum();
        println!("| {n} | {} | {:.0} | {:.3} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {comp} of {tot} |", fm(&rpe), mci(&rules).0, mci(&aps).0, per(FType::Seq), per(FType::Compose), per(FType::Formula), per(FType::Grid), per(FType::Knap));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t0 = std::time::Instant::now();
    match args.get(1).map(|s| s.as_str()).unwrap_or("v2") {
        "v2" => diag_v2(&[P2::Current, P2::Recommended, P2::Lin(0.5, Feat::Full), P2::B13(false), P2::B13(true)]),
        "ablate-v2" => diag_v2(&[P2::Current, P2::Beta(1.0), P2::Beta(0.5), P2::NoArm, P2::Lin(0.5, Feat::Full), P2::Lin(0.5, Feat::FamOnly), P2::Lin(0.5, Feat::Cells), P2::Lin(0.5, Feat::NoFam), P2::Lin(1.5, Feat::Full)]),
        "b2" => diag_b2(),
        "l" => diag_l(),
        "b2-disc" => diag_b2_disc(),
        "o" => diag_o(),
        "tune-v2" => tune_v2(false),
        "tune-v2-ext" => tune_v2(true),
        "tune-l-ext" => tune_l(true),
        "tune-b2" => tune_b2(),
        "tune-l" => tune_l(false),
        m => eprintln!("unknown mode {m}"),
    }
    eprintln!("total {:.0} s", t0.elapsed().as_secs_f64());
}
