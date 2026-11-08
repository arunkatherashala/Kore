//! Track W1 measurements: World T (synthetic data-work world, author-defined; see docs/world_t.md).
//!   cargo run -p kore-self --example measure_world_t --offline --release -- sweep  --home DIR   [tune on 101..120, report on 1..20, 6 x 5 reward-design grid]
//!   cargo run -p kore-self --example measure_world_t --offline --release -- report --home DIR --beta B --ucb-c C --lin-alpha A   [seeds 1..20]
//!   cargo run -p kore-self --example measure_world_t --offline --release -- ood    --home DIR --beta B --ucb-c C --lin-alpha A   [seeds 1..20, out of distribution]
//!   cargo run -p kore-self --example measure_world_t --offline --release -- prof   --home DIR   [time per life]
//! The data root is a temp directory under the scratch directory given by --home (KORE_SELF_HOME is set to
//! it before anything else); every sandbox file is written there through safe_path and removed after its
//! world. Raw outputs are printed and never committed.
//!
//! Tuning budget (the same for every tuned policy and every grid point): 6 configurations, seeds 101..120,
//! budget 6000, objective = mean reward per energy. Being: beta 0.5 1 2 4 8 16 (prior weight 2, pooled,
//! skip on). UCB1: c 0.001 0.003 0.01 0.03 0.1 0.3 (arms per template x context, skip on). LinUCB: alpha
//! 0.1 0.3 1 3 10 30 (skip on). Random plan and always SQL have nothing to tune.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;
#[path = "../src/safety.rs"]
mod safety;
#[path = "../src/world_t_sandbox.rs"]
mod world_t_sandbox;

use being::world_t::baselines::{LinCfg, UcbCfg};
use being::world_t::engine::{Sandbox, ToolCache, TrueT};
use being::world_t::learner::LearnerTCfg;
use being::world_t::measure::{run_life_cached, LifeT, Pol};
use being::world_t::world::{Kind, Plan, WTCfg, WorldT, DEFAULT_BUDGET, KINDS};
use rayon::prelude::*;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn seeds(args: &[String], k: &str, def: (u64, u64)) -> Vec<u64> {
    let s = arg(args, k).unwrap_or(format!("{}..{}", def.0, def.1));
    let (a, b) = s.split_once("..").expect("seeds a..b");
    (a.parse().unwrap()..=b.parse().unwrap()).collect()
}

fn tq(df: usize) -> f64 {
    const T: [f64; 30] = [12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160, 2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.048, 2.045, 2.042];
    if df == 0 { f64::NAN } else if df <= 30 { T[df - 1] } else { 1.96 }
}

fn mean_ci(x: &[f64]) -> (f64, f64, f64) {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    let v = x.iter().map(|a| (a - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    let h = tq(x.len() - 1) * (v / n).sqrt();
    (m, m - h, m + h)
}

fn fmt_ci(x: &[f64]) -> String {
    let (m, lo, hi) = mean_ci(x);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len().max(1) as f64
}

fn diff(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| x - y).collect()
}

fn paired(a: &[f64], b: &[f64]) -> String {
    let d = diff(a, b);
    let (m, lo, hi) = mean_ci(&d);
    let w = d.iter().filter(|x| **x > 1e-12).count();
    let l = d.iter().filter(|x| **x < -1e-12).count();
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}] {w}/{l}/{}", d.len() - w - l)
}

/// +1 (interval above 0), -1 (below), 0 (contains 0).
fn verdict(a: &[f64], b: &[f64]) -> i32 {
    let (_, lo, hi) = mean_ci(&diff(a, b));
    if lo > 0.0 { 1 } else if hi < 0.0 { -1 } else { 0 }
}

fn rank(x: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..x.len()).collect();
    idx.sort_by(|a, b| x[*a].partial_cmp(&x[*b]).unwrap());
    let mut r = vec![0.0; x.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && x[idx[j + 1]] == x[idx[i]] {
            j += 1;
        }
        for k in i..=j {
            r[idx[k]] = (i + j) as f64 / 2.0;
        }
        i = j + 1;
    }
    r
}

fn spearman(x: &[f64], y: &[f64]) -> f64 {
    let (rx, ry) = (rank(x), rank(y));
    let n = x.len() as f64;
    let (mx, my) = (rx.iter().sum::<f64>() / n, ry.iter().sum::<f64>() / n);
    let c: f64 = rx.iter().zip(&ry).map(|(a, b)| (a - mx) * (b - my)).sum();
    let vx: f64 = rx.iter().map(|a| (a - mx).powi(2)).sum();
    let vy: f64 = ry.iter().map(|b| (b - my).powi(2)).sum();
    if vx == 0.0 || vy == 0.0 { 0.0 } else { c / (vx * vy).sqrt() }
}

/// Write the world's pool into the sandbox, run `f` with a DirSandbox reading it back through safe_path, remove the pool.
fn with_sandbox<T>(c: &WTCfg, f: impl FnOnce(&dyn Sandbox) -> T) -> T {
    let owner = format!("measure-{}{}", c.seed, if c.ood { "-ood" } else { "" });
    let w = WorldT::new(c.clone());
    world_t_sandbox::write_pool(&owner, &w).expect("sandbox");
    let sb = world_t_sandbox::DirSandbox::new(&owner, c.seed);
    let out = f(&sb);
    world_t_sandbox::remove_pool(&owner, &w).expect("cleanup");
    let _ = std::fs::remove_dir_all(safety::owner_dir(&owner));
    out
}

fn chosen_rpe(t: &TrueT) -> f64 {
    t.reward as f64 / t.cost.max(1) as f64
}

fn good(t: &TrueT) -> bool {
    if t.best_rpe == 0.0 { t.plan.is_none() } else { chosen_rpe(t) >= 0.9 * t.best_rpe }
}

fn competence(l: &LifeT, k: Kind) -> f64 {
    let ts: Vec<&TrueT> = l.truths.iter().filter(|t| t.kind == k).collect();
    for i in 10..=ts.len() {
        if ts[i - 10..i].iter().filter(|t| good(t)).count() >= 8 {
            return i as f64;
        }
    }
    ts.len() as f64 + 10.0
}

fn early_regret(l: &LifeT, k: Kind, n: usize) -> Option<f64> {
    let ts: Vec<&TrueT> = l.truths.iter().filter(|t| t.kind == k).take(n).collect();
    if ts.len() < n / 2 {
        return None;
    }
    Some(ts.iter().map(|t| t.best_rpe - chosen_rpe(t)).sum::<f64>() / ts.len() as f64)
}

// ---- the tuning grids: 6 configurations per tuned policy ----

const BETAS: [u32; 6] = [500, 1000, 2000, 4000, 8000, 16000];
const UCB_C: [f64; 6] = [0.001, 0.003, 0.01, 0.03, 0.1, 0.3];
const LIN_A: [f64; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];

fn being_cfg(b: u32) -> LearnerTCfg {
    LearnerTCfg { beta_milli: b, prior_w_milli: 2000, pooled: true, skip: true }
}
fn ucb_cfg(c: f64) -> UcbCfg {
    UcbCfg { c, per_ctx: true, skip: true }
}
fn lin_cfg(a: f64) -> LinCfg {
    LinCfg { alpha: a, skip: true }
}

fn tune_pols() -> Vec<Pol> {
    let mut v: Vec<Pol> = BETAS.iter().map(|b| Pol::Being(being_cfg(*b))).collect();
    v.extend(UCB_C.iter().map(|c| Pol::Ucb(ucb_cfg(*c))));
    v.extend(LIN_A.iter().map(|a| Pol::Lin(lin_cfg(*a))));
    v
}

#[derive(Clone, Copy, Debug)]
struct Tuned {
    beta: u32,
    ucb_c: f64,
    lin_a: f64,
    /// index of the chosen value in each grid (0 or 5 = edge)
    idx: [usize; 3],
}

fn pick(scores: &[f64]) -> usize {
    let mut bi = 0;
    for (i, s) in scores.iter().enumerate() {
        if *s > scores[bi] {
            bi = i;
        }
    }
    bi
}

fn tuned_from(per: &[f64]) -> Tuned {
    let (b, u, l) = (pick(&per[0..6]), pick(&per[6..12]), pick(&per[12..18]));
    Tuned { beta: BETAS[b], ucb_c: UCB_C[u], lin_a: LIN_A[l], idx: [b, u, l] }
}

// ---- metrics ----

#[derive(Clone, Debug, Default)]
struct M {
    rpe: f64,
    right_ans: f64,
    coverage: f64,
    skip: f64,
    wrong100: f64,
}

fn metrics(l: &LifeT) -> M {
    let n = l.truths.len().max(1) as f64;
    let ans = l.truths.iter().filter(|t| t.answered).count() as f64;
    let ok = l.truths.iter().filter(|t| t.correct).count() as f64;
    let skip = l.truths.iter().filter(|t| t.plan.is_none()).count() as f64;
    let wrong = l.truths.iter().filter(|t| t.wrong).count() as f64;
    M { rpe: l.rpe(), right_ans: ok / ans.max(1.0), coverage: ans / n, skip: skip / n, wrong100: 100.0 * wrong / n }
}

fn col(ms: &[M], f: impl Fn(&M) -> f64) -> Vec<f64> {
    ms.iter().map(f).collect()
}

// ---- sweep ----

const PENALTY: [u32; 6] = [0, 100, 316, 1000, 3162, 10000];
const COST: [u32; 5] = [100, 316, 1000, 3162, 10000];

fn grid() -> Vec<(u32, u32)> {
    let mut g = vec![];
    for p in PENALTY {
        for c in COST {
            g.push((p, c));
        }
    }
    g
}

fn wcfg(seed: u64, p: u32, c: u32, ood: bool) -> WTCfg {
    WTCfg { penalty_pm: p, cost_pm: c, ood, ..WTCfg::new(seed, DEFAULT_BUDGET) }
}

fn sweep(args: &[String]) {
    let tune_seeds = seeds(args, "--tune-seeds", (101, 120));
    let rep_seeds = seeds(args, "--seeds", (1, 20));
    let g = grid();
    let pols = tune_pols();
    let t0 = std::time::Instant::now();
    // stage 1: tuning, seeds 101..120 only. per seed: [grid point][config] reward per energy
    let tune: Vec<Vec<Vec<f64>>> = tune_seeds.par_iter().map(|s| {
        with_sandbox(&wcfg(*s, 0, 1000, false), |sb| {
            let mut cache = ToolCache::new();
            g.iter().map(|(p, c)| pols.iter().map(|pol| run_life_cached(&wcfg(*s, *p, *c, false), *pol, Some(sb), &mut cache).rpe()).collect()).collect()
        })
    }).collect();
    println!("World T reward-design sweep: tuning seeds {}..{}, report seeds {}..{}, budget {DEFAULT_BUDGET}; tuning wall {:.0} s", tune_seeds[0], tune_seeds[tune_seeds.len() - 1], rep_seeds[0], rep_seeds[rep_seeds.len() - 1], t0.elapsed().as_secs_f64());
    let mut tuned = vec![];
    println!("\nTuning (mean reward per energy on the tuning seeds; chosen value, * = at the edge of its 6-value grid):");
    println!("| penalty x reward | energy weight | being beta | UCB1 c | LinUCB alpha |");
    println!("|---|---|---|---|---|");
    for (gi, (p, c)) in g.iter().enumerate() {
        let per: Vec<f64> = (0..pols.len()).map(|k| mean(&tune.iter().map(|s| s[gi][k]).collect::<Vec<_>>())).collect();
        let t = tuned_from(&per);
        let e = |i: usize| if t.idx[i] == 0 || t.idx[i] == 5 { "*" } else { "" };
        println!("| {:.3} | {:.3} | {}{} ({:.3}) | {}{} ({:.3}) | {}{} ({:.3}) |", *p as f64 / 1000.0, *c as f64 / 1000.0, t.beta as f64 / 1000.0, e(0), per[t.idx[0]], t.ucb_c, e(1), per[6 + t.idx[1]], t.lin_a, e(2), per[12 + t.idx[2]]);
        tuned.push(t);
    }
    // stage 2: report, seeds 1..20, with each grid point's tuned configurations
    let names = ["being", "LinUCB", "UCB1", "always SQL", "random plan", "reward-best oracle", "answer-everything oracle"];
    let t1 = std::time::Instant::now();
    let rep: Vec<Vec<Vec<M>>> = rep_seeds.par_iter().map(|s| {
        with_sandbox(&wcfg(*s, 0, 1000, false), |sb| {
            let mut cache = ToolCache::new();
            g.iter().zip(&tuned).map(|((p, c), t)| {
                let ps = [Pol::Being(being_cfg(t.beta)), Pol::Lin(lin_cfg(t.lin_a)), Pol::Ucb(ucb_cfg(t.ucb_c)), Pol::AlwaysSql, Pol::Random, Pol::RewardBest, Pol::AnswerAll];
                ps.iter().map(|pol| metrics(&run_life_cached(&wcfg(*s, *p, *c, false), *pol, Some(sb), &mut cache))).collect()
            }).collect()
        })
    }).collect();
    println!("\nreport wall {:.0} s", t1.elapsed().as_secs_f64());
    println!("\nPer grid point (report seeds; mean over worlds): reward per energy / right share of answers / coverage (answered / tasks) / skip share / wrong answers per 100 tasks");
    println!("| penalty | weight | policy | reward per energy | right of answers | coverage | skip share | wrong per 100 |");
    println!("|---|---|---|---|---|---|---|---|");
    let mut lead = [0usize; 3];
    let mut lead_sig = [0usize; 3];
    let mut behind_sig = [0usize; 3];
    let mut flips: Vec<String> = vec![];
    let mut summary = vec![];
    for (gi, (p, c)) in g.iter().enumerate() {
        let at = |k: usize| -> Vec<M> { rep.iter().map(|s| s[gi][k].clone()).collect() };
        for (k, nm) in names.iter().enumerate() {
            let m = at(k);
            println!("| {:.3} | {:.3} | {nm} | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} |", *p as f64 / 1000.0, *c as f64 / 1000.0, mean(&col(&m, |x| x.rpe)), mean(&col(&m, |x| x.right_ans)), mean(&col(&m, |x| x.coverage)), mean(&col(&m, |x| x.skip)), mean(&col(&m, |x| x.wrong100)));
        }
        let b = col(&at(0), |x| x.rpe);
        let mut row = format!("| {:.3} | {:.3} |", *p as f64 / 1000.0, *c as f64 / 1000.0);
        for (j, k) in [1usize, 2, 3].iter().enumerate() {
            let o = col(&at(*k), |x| x.rpe);
            let v = verdict(&b, &o);
            if mean(&b) > mean(&o) {
                lead[j] += 1;
            }
            if v > 0 {
                lead_sig[j] += 1;
            }
            if v < 0 {
                behind_sig[j] += 1;
                flips.push(format!("{} beats the being at penalty {:.3} x reward, energy weight {:.3}: being minus {} {}", names[*k], *p as f64 / 1000.0, *c as f64 / 1000.0, names[*k], paired(&b, &o)));
            }
            row.push_str(&format!(" {} |", paired(&b, &o)));
        }
        let rb = col(&at(5), |x| x.rpe);
        row.push_str(&format!(" {} |", paired(&b, &rb)));
        summary.push(row);
    }
    println!("\nPaired, being minus LinUCB / UCB1 / always SQL / reward-best oracle (reward per energy, better/worse/tie of {}):", rep_seeds.len());
    println!("| penalty | weight | minus LinUCB | minus UCB1 | minus always SQL | minus reward-best oracle |");
    println!("|---|---|---|---|---|---|");
    for r in &summary {
        println!("{r}");
    }
    let n = g.len();
    for (j, nm) in ["LinUCB", "UCB1", "always SQL"].iter().enumerate() {
        println!("against {nm}: being ahead on the mean at {} of {n} grid points, interval above 0 at {}, interval below 0 at {}", lead[j], lead_sig[j], behind_sig[j]);
    }
    println!("\nWhere a reference beats the being (interval of the paired difference below 0):");
    if flips.is_empty() {
        println!("  nowhere on the grid");
    }
    for f in &flips {
        println!("  {f}");
    }
}

// ---- main report (one grid point) and OOD ----

fn report(args: &[String], ood: bool) {
    let ss = seeds(args, "--seeds", (1, 20));
    let beta: u32 = arg(args, "--beta").map_or(2000, |b| (b.parse::<f64>().unwrap() * 1000.0).round() as u32);
    let uc: f64 = arg(args, "--ucb-c").map_or(0.03, |b| b.parse().unwrap());
    let la: f64 = arg(args, "--lin-alpha").map_or(1.0, |b| b.parse().unwrap());
    let lb = being_cfg(beta);
    let pols = [
        Pol::Being(lb),
        Pol::Being(LearnerTCfg { skip: false, ..lb }),
        Pol::Being(LearnerTCfg { pooled: false, ..lb }),
        Pol::Random,
        Pol::AlwaysSql,
        Pol::Ucb(ucb_cfg(uc)),
        Pol::Ucb(UcbCfg { skip: false, ..ucb_cfg(uc) }),
        Pol::Lin(lin_cfg(la)),
        Pol::Lin(LinCfg { skip: false, ..lin_cfg(la) }),
        Pol::RewardBest,
        Pol::AnswerAll,
    ];
    let names = ["being", "being, skip off", "being, flat prior", "random plan", "always SQL", "UCB1", "UCB1, skip off", "LinUCB", "LinUCB, skip off", "reward-best oracle (SEES HIDDEN)", "answer-everything oracle (SEES HIDDEN)"];
    let t0 = std::time::Instant::now();
    let all: Vec<Vec<LifeT>> = ss.par_iter().map(|s| {
        let c = wcfg(*s, 0, 1000, ood);
        with_sandbox(&c, |sb| {
            let mut cache = ToolCache::new();
            pols.iter().map(|p| run_life_cached(&c, *p, Some(sb), &mut cache)).collect()
        })
    }).collect();
    println!("World T {}: seeds {}..{}, budget {DEFAULT_BUDGET}, being beta {}, UCB1 c {uc}, LinUCB alpha {la}; wall {:.0} s", if ood { "OUT OF DISTRIBUTION" } else { "report" }, ss[0], ss[ss.len() - 1], beta as f64 / 1000.0, t0.elapsed().as_secs_f64());
    let ms: Vec<Vec<M>> = (0..pols.len()).map(|i| all.iter().map(|w| metrics(&w[i])).collect()).collect();
    println!("\n| policy | reward per energy | right share of answers | coverage (answered / tasks) | skip share | wrong answers per 100 tasks | wrong accepted |");
    println!("|---|---|---|---|---|---|---|");
    for (i, nm) in names.iter().enumerate() {
        let wa: usize = all.iter().map(|w| w[i].truths.iter().filter(|t| t.wrong_accepted).count()).sum();
        println!("| {nm} | {} | {} | {} | {} | {} | {wa} |", fmt_ci(&col(&ms[i], |x| x.rpe)), fmt_ci(&col(&ms[i], |x| x.right_ans)), fmt_ci(&col(&ms[i], |x| x.coverage)), fmt_ci(&col(&ms[i], |x| x.skip)), fmt_ci(&col(&ms[i], |x| x.wrong100)));
    }
    let b = col(&ms[0], |x| x.rpe);
    println!("\nPaired, being minus each (reward per energy; right share of answers), better/worse/tie of {}:", ss.len());
    println!("| other | reward per energy | right share of answers |");
    println!("|---|---|---|");
    for i in 1..pols.len() {
        println!("| {} | {} | {} |", names[i], paired(&b, &col(&ms[i], |x| x.rpe)), paired(&col(&ms[0], |x| x.right_ans), &col(&ms[i], |x| x.right_ans)));
    }
    // how much of the lead comes from skipping
    let r = |i: usize| col(&ms[i], |x| x.rpe);
    println!("\nSkipping: being minus being with skip off {}; LinUCB minus LinUCB skip off {}; UCB1 minus UCB1 skip off {}", paired(&r(0), &r(1)), paired(&r(7), &r(8)), paired(&r(5), &r(6)));
    println!("  with skip on for both: being minus LinUCB {}; with skip off for both: {}", paired(&r(0), &r(7)), paired(&r(1), &r(8)));
    println!("  with skip on for both: being minus UCB1 {}; with skip off for both: {}", paired(&r(0), &r(5)), paired(&r(1), &r(6)));
    // learning curve
    println!("\nLearning curve, reward per energy per tenth of the life (mean over worlds):");
    println!("| policy | {} |", (1..=10).map(|w| w.to_string()).collect::<Vec<_>>().join(" | "));
    println!("|---|{}", "---|".repeat(10));
    let wins = |i: usize| -> Vec<Vec<f64>> {
        (0..10).map(|w| all.iter().map(|lv| {
            let (lo, hi) = (DEFAULT_BUDGET * w / 10, DEFAULT_BUDGET * (w + 1) / 10);
            let (rr, cc) = lv[i].truths.iter().filter(|t| t.energy_before >= lo && t.energy_before < hi).fold((0i64, 0i64), |s, t| (s.0 + t.reward, s.1 + t.cost));
            rr as f64 / cc.max(1) as f64
        }).collect()).collect()
    };
    for i in [0usize, 1, 2, 5, 7, 9, 10] {
        let w = wins(i);
        println!("| {} | {} |", names[i], w.iter().map(|x| format!("{:.2}", mean(x))).collect::<Vec<_>>().join(" | "));
    }
    let (wb, wr) = (wins(0), wins(9));
    println!("being, last tenth minus first: {}; gap to the reward-best oracle, last tenth minus first (negative = closes): {}", fmt_ci(&diff(&wb[9], &wb[0])), fmt_ci(&diff(&diff(&wr[9], &wb[9]), &diff(&wr[0], &wb[0]))));
    // later kinds
    println!("\nKinds in order of opening: Spearman of opening position vs tasks to competence (negative = later kinds faster); regret vs the answer-everything oracle's best plan over the first 30 tasks of a kind:");
    for (pi, nm) in [(0usize, "being"), (2, "being, flat prior"), (5, "UCB1"), (7, "LinUCB")] {
        let mut rho = vec![];
        let (mut e, mut la2) = (vec![], vec![]);
        for w in &all {
            let l = &w[pi];
            let pos: Vec<f64> = (0..KINDS.len()).map(|i| i as f64).collect();
            let comp: Vec<f64> = l.order.iter().map(|(k, _)| competence(l, *k)).collect();
            rho.push(spearman(&pos, &comp));
            let rr: Vec<Option<f64>> = l.order.iter().map(|(k, _)| early_regret(l, *k, 30)).collect();
            e.push(mean(&rr[..3].iter().flatten().copied().collect::<Vec<_>>()));
            la2.push(mean(&rr[3..].iter().flatten().copied().collect::<Vec<_>>()));
        }
        println!("  {nm:18} Spearman {}; early regret, kinds open at the start {}, kinds opened later {}", fmt_ci(&rho), fmt_ci(&e), fmt_ci(&la2));
    }
    // per kind
    println!("\nPer kind (being, pooled over worlds): right share of answers, plan shares, reward per energy of being / reward-best oracle");
    for k in KINDS {
        let bt: Vec<&TrueT> = all.iter().flat_map(|w| w[0].truths.iter()).filter(|t| t.kind == k).collect();
        let ot: Vec<&TrueT> = all.iter().flat_map(|w| w[9].truths.iter()).filter(|t| t.kind == k).collect();
        let ans = bt.iter().filter(|t| t.answered).count();
        let ok = bt.iter().filter(|t| t.correct).count();
        let mut uses: std::collections::BTreeMap<String, usize> = Default::default();
        for t in &bt {
            *uses.entry(t.plan.map_or("skip".into(), |p| p.name().to_string())).or_default() += 1;
        }
        let mut u: Vec<(String, usize)> = uses.into_iter().collect();
        u.sort_by_key(|x| std::cmp::Reverse(x.1));
        let rp = |v: &[&TrueT]| v.iter().map(|t| t.reward).sum::<i64>() as f64 / v.iter().map(|t| t.cost).sum::<i64>().max(1) as f64;
        println!("  {:9} tasks {:6} right/answered {:.3} plans {} | being {:.2} reward-best oracle {:.2}", k.name(), bt.len(), ok as f64 / ans.max(1) as f64, u.iter().take(4).map(|(p, n)| format!("{p} {:.0}%", 100.0 * *n as f64 / bt.len().max(1) as f64)).collect::<Vec<_>>().join(", "), rp(&bt), rp(&ot));
    }
    // rule precision before the checker
    let bt: Vec<&TrueT> = all.iter().flat_map(|w| w[0].truths.iter()).collect();
    for (nm, f) in [("series rules (ts-rule)", Plan::TsRule), ("column-rule search (col-search)", Plan::ColSearch)] {
        let chosen = bt.iter().filter(|t| t.plan == Some(f)).count();
        let produced = bt.iter().filter(|t| t.plan == Some(f) && t.answered).count();
        let rejected = bt.iter().filter(|t| t.plan == Some(f) && t.answered && !t.correct).count();
        println!("\n{nm}: chosen {chosen}, answers produced {produced} (no answer {}), rejected by the checker {rejected}, precision before the checker {:.4}", chosen - produced, (produced - rejected) as f64 / produced.max(1) as f64);
    }
    println!("rules accepted {} (on the noisy series {})", all.iter().map(|w| w[0].rules).sum::<usize>(), all.iter().map(|w| w[0].rules_noisy).sum::<usize>());
}

fn prof(_args: &[String]) {
    let c = WTCfg::new(101, DEFAULT_BUDGET);
    let mut cache = ToolCache::new();
    for round in 0..2 {
        for p in [Pol::AnswerAll, Pol::RewardBest, Pol::Being(being_cfg(2000)), Pol::Random, Pol::Ucb(ucb_cfg(0.03)), Pol::Lin(lin_cfg(1.0))] {
            let t = std::time::Instant::now();
            let l = run_life_cached(&c, p, None, &mut cache);
            println!("round {round} {:44} {:6} steps rpe {:.3} {:7.2} s", p.label(), l.truths.len(), l.rpe(), t.elapsed().as_secs_f64());
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let home = arg(&args, "--home").expect("--home <scratch dir> (the data root for the sandboxes)");
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var(safety::HOME_ENV, &home);
    rayon::ThreadPoolBuilder::new().num_threads(arg(&args, "--threads").map_or(4, |t| t.parse().unwrap())).build_global().unwrap();
    match args.get(1).map(|s| s.as_str()).unwrap_or("") {
        "sweep" => sweep(&args),
        "report" => report(&args, false),
        "ood" => report(&args, true),
        "prof" => prof(&args),
        _ => eprintln!("modes: sweep | report | ood | prof  (--home DIR required)"),
    }
}
