//! Track W1 measurements: World T (synthetic data-work world, author-defined; see docs/world_t.md).
//!   cargo run -p kore-self --example measure_world_t --offline --release -- tune   [seeds 101..120]
//!   cargo run -p kore-self --example measure_world_t --offline --release -- report [seeds 1..20]
//!   cargo run -p kore-self --example measure_world_t --offline --release -- ood    [seeds 1..20, out-of-distribution worlds]
//! The data root is a temp directory under the scratch directory given by --home (KORE_SELF_HOME is set to
//! it before anything else); every sandbox file is written there through safe_path and removed after its
//! world. Raw outputs are printed and never committed.

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
use being::world_t::engine::{Sandbox, TrueT};
use being::world_t::learner::LearnerTCfg;
use being::world_t::measure::{run_life_cached, LifeT, Pol};
use being::world_t::engine::ToolCache;
use being::world_t::world::{Kind, Plan, WTCfg, WorldT, DEFAULT_BUDGET, KINDS};
use rayon::prelude::*;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn seeds(args: &[String], def: (u64, u64)) -> Vec<u64> {
    let s = arg(args, "--seeds").unwrap_or(format!("{}..{}", def.0, def.1));
    let (a, b) = s.split_once("..").expect("--seeds a..b");
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

fn paired(a: &[f64], b: &[f64]) -> String {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mean_ci(&d);
    let w = d.iter().filter(|x| **x > 1e-12).count();
    let l = d.iter().filter(|x| **x < -1e-12).count();
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}] {w}/{l}/{}", d.len() - w - l)
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

/// Write the world's pool into the sandbox, run every policy with files read back through safe_path,
/// remove the pool.
fn lives(c: &WTCfg, pols: &[Pol]) -> Vec<LifeT> {
    let owner = format!("measure-{}{}", c.seed, if c.ood { "-ood" } else { "" });
    let w = WorldT::new(c.clone());
    world_t_sandbox::write_pool(&owner, &w).expect("sandbox");
    let sb = world_t_sandbox::DirSandbox::new(&owner, c.seed);
    let mut cache = ToolCache::new();
    let out = pols.iter().map(|p| run_life_cached(c, *p, Some(&sb as &dyn Sandbox), &mut cache)).collect();
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

/// Attempts (tasks of the kind met) until the last 10 of the kind have at least 8 good choices (none: all met + 10).
fn competence(l: &LifeT, k: Kind) -> f64 {
    let ts: Vec<&TrueT> = l.truths.iter().filter(|t| t.kind == k).collect();
    for i in 10..=ts.len() {
        if ts[i - 10..i].iter().filter(|t| good(t)).count() >= 8 {
            return i as f64;
        }
    }
    ts.len() as f64 + 10.0
}

/// Mean regret (best reward per energy minus the chosen one) over the first `n` tasks of a kind.
fn early_regret(l: &LifeT, k: Kind, n: usize) -> Option<f64> {
    let ts: Vec<&TrueT> = l.truths.iter().filter(|t| t.kind == k).take(n).collect();
    if ts.len() < n / 2 {
        return None;
    }
    Some(ts.iter().map(|t| t.best_rpe - chosen_rpe(t)).sum::<f64>() / ts.len() as f64)
}

fn being_grid() -> Vec<Pol> {
    let mut v = vec![];
    for b in [500, 1000, 2000] {
        for w in [1000, 2000, 4000] {
            v.push(Pol::Being(LearnerTCfg { beta_milli: b, prior_w_milli: w, pooled: true, skip: true }));
        }
    }
    v
}

fn ucb_grid() -> Vec<Pol> {
    let mut v = vec![];
    for c in [0.03, 0.1, 0.3, 1.0] {
        for per_ctx in [false, true] {
            for skip in [false, true] {
                v.push(Pol::Ucb(UcbCfg { c, per_ctx, skip }));
            }
        }
    }
    v
}

fn lin_grid() -> Vec<Pol> {
    let mut v = vec![];
    for a in [0.1, 0.3, 1.0, 3.0] {
        for skip in [false, true] {
            v.push(Pol::Lin(LinCfg { alpha: a, skip }));
        }
    }
    v
}

fn tune(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let budget: i64 = arg(args, "--budget").map_or(DEFAULT_BUDGET, |b| b.parse().unwrap());
    let mut pols = being_grid();
    pols.extend(ucb_grid());
    pols.extend(lin_grid());
    let per: Vec<Vec<f64>> = ss.par_iter().map(|s| lives(&WTCfg::new(*s, budget), &pols).iter().map(|l| l.rpe()).collect()).collect();
    println!("tuning seeds {:?}, budget {budget}: mean reward per energy", (ss[0], ss[ss.len() - 1]));
    for (i, p) in pols.iter().enumerate() {
        let x: Vec<f64> = per.iter().map(|r| r[i]).collect();
        println!("  {:52} {}", p.label() + &format!(" {:?}", p), fmt_ci(&x));
    }
}

/// The configurations chosen on the tuning seeds (see docs/measured.md, Track W1).
fn tuned() -> (LearnerTCfg, UcbCfg, LinCfg) {
    (LearnerTCfg { beta_milli: 2000, prior_w_milli: 2000, pooled: true, skip: true }, UcbCfg { c: 0.03, per_ctx: true, skip: true }, LinCfg { alpha: 1.0, skip: true })
}

struct Agg {
    name: String,
    rpe: Vec<f64>,
    acc_answered: Vec<f64>,
    acc_tasks: Vec<f64>,
    wrong_per100: Vec<f64>,
    wrong_accepted: u64,
    windows: Vec<Vec<f64>>,
}

fn agg(name: &str, ls: &[&LifeT], budget: i64) -> Agg {
    let mut a = Agg { name: name.into(), rpe: vec![], acc_answered: vec![], acc_tasks: vec![], wrong_per100: vec![], wrong_accepted: 0, windows: vec![vec![]; 10] };
    for l in ls {
        a.rpe.push(l.rpe());
        let ans = l.truths.iter().filter(|t| t.answered).count() as f64;
        let ok = l.truths.iter().filter(|t| t.correct).count() as f64;
        let n = l.truths.len() as f64;
        a.acc_answered.push(ok / ans.max(1.0));
        a.acc_tasks.push(ok / n.max(1.0));
        a.wrong_per100.push(100.0 * (ans - ok) / n.max(1.0));
        a.wrong_accepted += l.truths.iter().filter(|t| t.wrong_accepted).count() as u64;
        for w in 0..10 {
            let (lo, hi) = (budget * w / 10, budget * (w + 1) / 10);
            let (r, c): (i64, i64) = l.truths.iter().filter(|t| t.energy_before >= lo && t.energy_before < hi).fold((0, 0), |s, t| (s.0 + t.reward, s.1 + t.cost));
            a.windows[w as usize].push(r as f64 / c.max(1) as f64);
        }
    }
    a
}

fn report(args: &[String], ood: bool) {
    let ss = seeds(args, (1, 20));
    let budget: i64 = arg(args, "--budget").map_or(DEFAULT_BUDGET, |b| b.parse().unwrap());
    let (lb, uc, lc) = tuned();
    let flat = LearnerTCfg { pooled: false, ..lb };
    let pols = [Pol::Being(lb), Pol::Being(flat), Pol::Random, Pol::AlwaysSql, Pol::Ucb(uc), Pol::Lin(lc), Pol::Oracle];
    let names = ["being (pooled prior)", "being, flat prior (ablation)", "random plan", "always SQL", "UCB1 bandit (tuned)", "LinUCB (tuned)", "oracle (SEES HIDDEN)"];
    let t0 = std::time::Instant::now();
    let all: Vec<Vec<LifeT>> = ss.par_iter().map(|s| lives(&WTCfg { ood, ..WTCfg::new(*s, budget) }, &pols)).collect();
    println!("World T {}: seeds {}..{}, budget {budget} energy, {} worlds, wall {:.0} s", if ood { "OUT OF DISTRIBUTION" } else { "report" }, ss[0], ss[ss.len() - 1], ss.len(), t0.elapsed().as_secs_f64());
    let aggs: Vec<Agg> = (0..pols.len()).map(|i| agg(names[i], &all.iter().map(|w| &w[i]).collect::<Vec<_>>(), budget)).collect();
    println!("\n| policy | reward per energy | right share of answers | right share of tasks | wrong answers per 100 tasks | wrong answers accepted |");
    println!("|---|---|---|---|---|---|");
    for a in &aggs {
        println!("| {} | {} | {} | {} | {} | {} |", a.name, fmt_ci(&a.rpe), fmt_ci(&a.acc_answered), fmt_ci(&a.acc_tasks), fmt_ci(&a.wrong_per100), a.wrong_accepted);
    }
    println!("\nPaired, being (pooled) minus each (mean [95% interval] better/worse/tie of {}):", ss.len());
    println!("| other | reward per energy | right share of answers |");
    println!("|---|---|---|");
    for a in &aggs[1..] {
        println!("| {} | {} | {} |", a.name, paired(&aggs[0].rpe, &a.rpe), paired(&aggs[0].acc_answered, &a.acc_answered));
    }
    println!("\nLearning curve, reward per energy per tenth of the life (mean over worlds):");
    println!("| policy | {} |", (1..=10).map(|w| format!("{}", w)).collect::<Vec<_>>().join(" | "));
    println!("|---|{}", "---|".repeat(10));
    for a in &aggs {
        println!("| {} | {} |", a.name, a.windows.iter().map(|w| format!("{:.2}", w.iter().sum::<f64>() / w.len() as f64)).collect::<Vec<_>>().join(" | "));
    }
    let d: Vec<f64> = (0..ss.len()).map(|i| aggs[0].windows[9][i] - aggs[0].windows[0][i]).collect();
    println!("being, last tenth minus first tenth: {}", fmt_ci(&d));
    let d: Vec<f64> = (0..ss.len()).map(|i| (aggs[6].windows[9][i] - aggs[0].windows[9][i]) - (aggs[6].windows[0][i] - aggs[0].windows[0][i])).collect();
    println!("being's gap to the oracle, last tenth minus first tenth (negative = closes the gap): {}", fmt_ci(&d));
    // later kinds learned faster?
    println!("\nKinds in order of opening (Spearman per world of opening position vs tasks to competence; negative = later kinds faster):");
    for (pi, nm) in [(0usize, "being (pooled)"), (1, "being (flat)"), (4, "UCB1"), (5, "LinUCB")] {
        let mut rho = vec![];
        let (mut early, mut late) = (vec![], vec![]);
        for w in &all {
            let l = &w[pi];
            let pos: Vec<f64> = (0..KINDS.len()).map(|i| i as f64).collect();
            let comp: Vec<f64> = l.order.iter().map(|(k, _)| competence(l, *k)).collect();
            rho.push(spearman(&pos, &comp));
            let r: Vec<Option<f64>> = l.order.iter().map(|(k, _)| early_regret(l, *k, 30)).collect();
            let e: Vec<f64> = r[..3].iter().flatten().copied().collect();
            let la: Vec<f64> = r[3..].iter().flatten().copied().collect();
            early.push(e.iter().sum::<f64>() / e.len().max(1) as f64);
            late.push(la.iter().sum::<f64>() / la.len().max(1) as f64);
        }
        println!("  {nm:16} Spearman {}; regret over the first 30 tasks of a kind: kinds open at the start {}, kinds opened later {}", fmt_ci(&rho), fmt_ci(&early), fmt_ci(&late));
        if pi == 0 {
            let flat_late: Vec<f64> = all.iter().map(|w| {
                let l = &w[1];
                let r: Vec<f64> = l.order[3..].iter().filter_map(|(k, _)| early_regret(l, *k, 30)).collect();
                r.iter().sum::<f64>() / r.len().max(1) as f64
            }).collect();
            println!("  pooled minus flat, early regret on later kinds: {}", paired(&late, &flat_late));
        }
    }
    // per kind
    println!("\nPer kind (pooled over worlds): being right share of answers, most used plan, oracle reward per energy, being reward per energy");
    for k in KINDS {
        let bt: Vec<&TrueT> = all.iter().flat_map(|w| w[0].truths.iter()).filter(|t| t.kind == k).collect();
        let ot: Vec<&TrueT> = all.iter().flat_map(|w| w[6].truths.iter()).filter(|t| t.kind == k).collect();
        let ans = bt.iter().filter(|t| t.answered).count();
        let ok = bt.iter().filter(|t| t.correct).count();
        let mut uses: std::collections::BTreeMap<String, usize> = Default::default();
        for t in &bt {
            *uses.entry(t.plan.map_or("skip".into(), |p| p.name().to_string())).or_default() += 1;
        }
        let mut u: Vec<(String, usize)> = uses.into_iter().collect();
        u.sort_by_key(|x| std::cmp::Reverse(x.1));
        let rp = |v: &[&TrueT]| v.iter().map(|t| t.reward).sum::<i64>() as f64 / v.iter().map(|t| t.cost).sum::<i64>().max(1) as f64;
        println!("  {:9} tasks {:6} right/answered {:.3} plans {} | oracle {:.2} being {:.2}", k.name(), bt.len(), ok as f64 / ans.max(1) as f64, u.iter().take(3).map(|(p, n)| format!("{p} {:.0}%", 100.0 * *n as f64 / bt.len().max(1) as f64)).collect::<Vec<_>>().join(", "), rp(&ot), rp(&bt));
    }
    // rules and the search tools
    let rule_ans: Vec<&TrueT> = all.iter().flat_map(|w| w[0].truths.iter()).filter(|t| t.rule_used).collect();
    let rr = rule_ans.iter().filter(|t| t.correct).count();
    let cs: Vec<&TrueT> = all.iter().flat_map(|w| w[0].truths.iter()).filter(|t| t.plan == Some(Plan::ColSearch) && t.answered).collect();
    let csr = cs.iter().filter(|t| t.correct).count();
    println!("\nbeing: series rules accepted {} (on the noisy series {}), rule answers {} of which right {} (wrong {}); column-rule search answers {} right {} (wrong {})",
        all.iter().map(|w| w[0].rules).sum::<usize>(), all.iter().map(|w| w[0].rules_noisy).sum::<usize>(), rule_ans.len(), rr, rule_ans.len() - rr, cs.len(), csr, cs.len() - csr);
    println!("wrong answers accepted (a reward paid for an answer its checker rejects), all policies: {}", aggs.iter().map(|a| a.wrong_accepted).sum::<u64>());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let home = arg(&args, "--home").expect("--home <scratch dir> (the data root for the sandboxes)");
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var(safety::HOME_ENV, &home);
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("");
    rayon::ThreadPoolBuilder::new().num_threads(arg(&args, "--threads").map_or(4, |t| t.parse().unwrap())).build_global().unwrap();
    match mode {
        "tune" => tune(&args),
        "report" => report(&args, false),
        "ood" => report(&args, true),
        _ => eprintln!("modes: tune | report | ood  (--home DIR required, --seeds a..b, --budget N, --threads N)"),
    }
}
