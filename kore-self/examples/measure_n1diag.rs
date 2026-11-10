//! Track N1 step 0: headroom and plateau diagnosis (measurement only; docs/measured.md, "Track N1 step 0").
//!   cargo run -p kore-self --example measure_n1diag --offline --release -- wt --seeds 101..120
//!   cargo run -p kore-self --example measure_n1diag --offline --release -- fw --seeds 101..120 --steps 200000 --arms lib+cur20,lib+cur20:3
//! Prints one tab-separated row per world (statistics are computed from those rows); raw outputs are never committed.
//!
//! World T (`wt`, penalty 0, energy weight 1, budget 6000, in-memory sandbox = the generator's bytes):
//! known-parameter references. Each knows, per key, the true chance that a plan's answer is right and its
//! true mean energy, estimated OUT OF SAMPLE from two all-skip lives of the same world started at steps
//! 10,000,000 and 20,000,000 (every plan evaluated on every task of those lives, about 12,000 tasks; no
//! task of the measured life is among them), but never the realized outcome of the task in front of it.
//! Variants: key = (template, context, plan) (the being's own cells) or (template, file, plan); series
//! rules = the oracle's (a perfect rule on every in-grammar exact series, as the reward-best oracle) or
//! the being's own (B2 search every 10 steps on its own examples, as the being); decision rule = the
//! being's (highest p x reward / energy, skip below the realized rate x (E - 1) / E after 40 energy) or the
//! renewal rule of the reward-best oracle (highest p x reward - rho x energy, skip below -rho).
//!
//! First world (`fw`, the B14 long run): the being's arms as in measure_b14, plus references per world:
//! the renewal oracle (true success rates, Dinkelbach) without rules, with the oracle book, with the
//! oracle book plus the being's own live rules at the end of the life, and the expected-value rate of a
//! renewal oracle with a PERFECT book (the discovered strategy answers every pattern task exactly at its
//! listed energy, with the careful strategy's execution slips). Measurement only.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::world_t::baselines::{LinCfg, RewardBestT};
use being::world_t::engine::{ChooserT, EngineT, MemSandbox, PlanResult, StateT, DEFAULT_DISCOVER_EVERY};
use being::world_t::learner::{options, ChoiceT, LearnerT, LearnerTCfg};
use being::world_t::measure::{run_life, Pol};
use being::world_t::world::{plans_for, Plan, TaskT, Tmpl, ViewT, WTCfg, DEFAULT_BUDGET, SKIP_COST};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn seeds(args: &[String], def: (u64, u64)) -> Vec<u64> {
    let s = arg(args, "--seeds").unwrap_or(format!("{}..{}", def.0, def.1));
    let (a, b) = s.split_once("..").expect("--seeds a..b");
    let v: Vec<u64> = (a.parse().unwrap()..=b.parse().unwrap()).collect();
    assert!(v.iter().all(|s| *s > 100), "diagnosis runs use the tuning seeds (101..) only");
    v
}

// ---------------------------------------------------------------- World T

#[derive(Default, Clone, Copy)]
struct Agg {
    n: f64,
    right: f64,
    wrong: f64,
    energy: f64,
}

#[derive(Default)]
struct Tables {
    cell: HashMap<(usize, usize, usize), Agg>,
    file: HashMap<(usize, String, usize), Agg>,
    tasks: u64,
    unsolvable: u64,
}

impl Tables {
    fn add(&mut self, v: &ViewT, rs: &[PlanResult]) {
        self.tasks += 1;
        if !rs.iter().any(|r| r.correct) {
            self.unsolvable += 1;
        }
        for r in rs {
            let a = Agg { n: 1.0, right: r.correct as u8 as f64, wrong: (r.answer.is_some() && !r.correct) as u8 as f64, energy: r.energy as f64 };
            for t in [self.cell.entry((v.tmpl.idx(), v.ctx.idx(), r.plan.idx())).or_default()] {
                t.n += a.n;
                t.right += a.right;
                t.wrong += a.wrong;
                t.energy += a.energy;
            }
            let t = self.file.entry((v.tmpl.idx(), v.file.clone(), r.plan.idx())).or_default();
            t.n += a.n;
            t.right += a.right;
            t.wrong += a.wrong;
            t.energy += a.energy;
        }
    }
    /// (p right, p wrong, mean energy) of a plan on a view; None when never observed.
    fn get(&self, v: &ViewT, plan: Plan, fine: bool) -> Option<(f64, f64, f64)> {
        // the being's TsRule on a TsNear task has no row (the references evaluate the rule on TsNext only)
        let f = |a: &Agg| (a.right / a.n, a.wrong / a.n, a.energy / a.n);
        if fine {
            if let Some(a) = self.file.get(&(v.tmpl.idx(), v.file.clone(), plan.idx())) {
                return Some(f(a));
            }
        }
        self.cell.get(&(v.tmpl.idx(), v.ctx.idx(), plan.idx())).map(f)
    }
}

/// Records the true result of every plan on every task and skips (the out-of-sample parameter pass).
struct Recorder {
    t: Tables,
}

impl ChooserT for Recorder {
    fn name(&self) -> String {
        "recorder (skips; measurement only)".into()
    }
    fn peek(&mut self, task: &TaskT, results: &[PlanResult]) {
        self.t.add(&task.view, results);
    }
    fn choose(&mut self, _st: &StateT, _v: &ViewT) -> ChoiceT {
        ChoiceT::skip("recorder", "parameter pass")
    }
}

#[derive(Clone, Copy, Debug)]
struct KpCfg {
    fine: bool,
    oracle_rules: bool,
    ratio: bool,
}

impl KpCfg {
    fn label(&self) -> String {
        format!("kp_{}_{}_{}", if self.fine { "file" } else { "cell" }, if self.oracle_rules { "orules" } else { "own" }, if self.ratio { "ratio" } else { "renew" })
    }
}

/// Known-parameter reference: true chances and energies per key, never the realized outcome.
struct Kp {
    t: Arc<Tables>,
    c: KpCfg,
    rule_answer: Option<String>,
    rew: f64,
    cost: f64,
}

impl ChooserT for Kp {
    fn name(&self) -> String {
        self.c.label()
    }
    fn peek(&mut self, _task: &TaskT, results: &[PlanResult]) {
        // only the perfect series rule's answer is taken from here (the oracle-rules variant); it is used
        // only when the rule is chosen on the strength of the in-grammar share of its key
        self.rule_answer = results.iter().find(|r| r.plan == Plan::TsRule && r.correct).and_then(|r| r.answer.clone());
    }
    fn choose(&mut self, st: &StateT, v: &ViewT) -> ChoiceT {
        let plans: Vec<Plan> = if self.c.oracle_rules {
            plans_for(v.tmpl).iter().copied().filter(|p| *p != Plan::TsRule || v.tmpl == Tmpl::TsNext).collect()
        } else {
            options(st, v)
        };
        let (r, pen) = (v.reward as f64, v.penalty());
        let rho_being = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let rho = if self.cost > 0.0 { self.rew / self.cost } else { 0.0 };
        let mut best: Option<(f64, Plan, f64)> = None;
        for p in plans {
            let (pr, pw, e) = self.t.get(v, p, self.c.fine).unwrap_or((0.0, 0.0, p.listed(v) as f64));
            let e = e.max(1.0);
            let s = if self.c.ratio { (pr * r - pw * pen) / e } else { pr * r - pw * pen - rho * e };
            if best.map_or(true, |b| s > b.0) {
                best = Some((s, p, e));
            }
        }
        let Some((s, p, e)) = best else { return ChoiceT::skip("kp", "no plan") };
        let skip = if self.c.ratio {
            st.total_cost >= 40 && s < rho_being * (e - SKIP_COST as f64).max(0.0) / e
        } else {
            s < -rho * SKIP_COST as f64
        };
        if skip && st.budget_left >= SKIP_COST {
            return ChoiceT::skip("kp", "below the skip bar");
        }
        let mut c = ChoiceT::plain("kp", p, e, "known parameters");
        if p == Plan::TsRule && self.c.oracle_rules {
            c.oracle_answer = self.rule_answer.clone();
        }
        c
    }
    fn observe(&mut self, _v: &ViewT, _plan: Option<Plan>, reward: i64, cost: i64) {
        self.rew += reward as f64;
        self.cost += cost as f64;
    }
}

struct LifeOut {
    reward: i64,
    cost: i64,
    tasks: u64,
    answered: u64,
    right: u64,
    skips: u64,
    /// reward per energy per tenth of the life (by energy spent)
    tenths: Vec<f64>,
}

impl LifeOut {
    fn rpe(&self) -> f64 {
        self.reward as f64 / self.cost.max(1) as f64
    }
}

fn life_t(cfg: &WTCfg, ch: &mut dyn ChooserT, discover: bool, step0: u64, lcfg: LearnerTCfg) -> LifeOut {
    let mut st = StateT::default();
    st.start(cfg.clone(), lcfg, "genesis".into());
    st.step = step0;
    let mut eng = EngineT::new(st, None);
    eng.measure = true;
    let world = being::world_t::world::WorldT::new(cfg.clone());
    let mem = MemSandbox(&world);
    let mut o = LifeOut { reward: 0, cost: 0, tasks: 0, answered: 0, right: 0, skips: 0, tenths: vec![] };
    let mut tr = vec![(0i64, 0i64); 10];
    while let Ok(s) = eng.step(ch, &mem) {
        let k = ((o.cost * 10) / cfg.budget).clamp(0, 9) as usize;
        o.reward += s.truth.reward;
        o.cost += s.truth.cost;
        tr[k].0 += s.truth.reward;
        tr[k].1 += s.truth.cost;
        o.tasks += 1;
        o.answered += s.truth.answered as u64;
        o.right += s.truth.correct as u64;
        o.skips += s.truth.plan.is_none() as u64;
        if discover && eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    o.tenths = tr.iter().map(|(r, c)| *r as f64 / (*c).max(1) as f64).collect();
    o
}

fn cmd_wt(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let kps: Vec<KpCfg> = [false, true].iter().flat_map(|&fine| [true, false].iter().flat_map(move |&orr| [false, true].iter().map(move |&ratio| KpCfg { fine, oracle_rules: orr, ratio }))).collect();
    let t0 = std::time::Instant::now();
    let rows: Vec<(u64, Vec<(String, f64)>, String)> = ss
        .par_iter()
        .map(|&s| {
            let cfg = WTCfg::new(s, DEFAULT_BUDGET);
            let mut rec = Recorder { t: Tables::default() };
            for off in [10_000_000u64, 20_000_000] {
                life_t(&cfg, &mut rec, false, off, LearnerTCfg::default());
            }
            let t = Arc::new(rec.t);
            let mut out: Vec<(String, f64)> = vec![];
            let being = life_t(&cfg, &mut LearnerT { cfg: LearnerTCfg::default() }, true, 0, LearnerTCfg::default());
            // check: the copy of the life loop takes the same steps as measure::run_life
            let chk = run_life(&cfg, Pol::Being(LearnerTCfg::default()), None).rpe();
            assert!((chk - being.rpe()).abs() < 1e-12, "life loop differs from run_life");
            out.push(("being".into(), being.rpe()));
            let mut extra = format!("being_tenths={}", being.tenths.iter().map(|x| format!("{x:.4}")).collect::<Vec<_>>().join(","));
            for (name, l) in [("being_flat", LearnerTCfg { pooled: false, ..Default::default() }), ("being_b1", LearnerTCfg { beta_milli: 1000, ..Default::default() }), ("being_b4", LearnerTCfg { beta_milli: 4000, ..Default::default() }), ("being_b0", LearnerTCfg { beta_milli: 0, ..Default::default() })] {
                out.push((name.into(), run_life(&cfg, Pol::Being(l), None).rpe()));
            }
            out.push(("linucb".into(), run_life(&cfg, Pol::Lin(LinCfg { alpha: 1.0, skip: true }), None).rpe()));
            let rb = life_t(&cfg, &mut RewardBestT::new(), false, 0, LearnerTCfg::default());
            out.push(("reward_best".into(), rb.rpe()));
            for c in &kps {
                let mut k = Kp { t: t.clone(), c: *c, rule_answer: None, rew: 0.0, cost: 0.0 };
                let l = life_t(&cfg, &mut k, !c.oracle_rules, 0, LearnerTCfg::default());
                out.push((c.label(), l.rpe()));
                if c.fine && c.oracle_rules && !c.ratio {
                    extra.push_str(&format!(" kpbest_skip={:.4} kpbest_right_of_answered={:.4}", l.skips as f64 / l.tasks as f64, l.right as f64 / l.answered.max(1) as f64));
                }
            }
            let det = t.file.values().filter(|a| a.right == 0.0 || a.right == a.n).count() as f64 / t.file.len() as f64;
            extra.push_str(&format!(" param_tasks={} unsolvable_share={:.4} file_keys={} file_keys_deterministic_share={:.4} cell_keys={}", t.tasks, t.unsolvable as f64 / t.tasks as f64, t.file.len(), det, t.cell.len()));
            extra.push_str(&format!(" being_skip={:.4} being_right_of_answered={:.4} rb_skip={:.4}", being.skips as f64 / being.tasks as f64, being.right as f64 / being.answered.max(1) as f64, rb.skips as f64 / rb.tasks as f64));
            eprintln!("world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            (s, out, extra)
        })
        .collect();
    let names: Vec<String> = rows[0].1.iter().map(|x| x.0.clone()).collect();
    println!("seed\t{}\textra", names.join("\t"));
    for (s, out, extra) in &rows {
        println!("{s}\t{}\t{extra}", out.iter().map(|x| format!("{:.6}", x.1)).collect::<Vec<_>>().join("\t"));
    }
    eprintln!("wt: {:.0} s", t0.elapsed().as_secs_f64());
}

// ---------------------------------------------------------------- first world

use being::b14::measure::{oracle_book, renewal_rate, run_b14, OutB14, RunCfgB14};
use being::measure::{truth_in, Truth};
use being::measure_b11::{genesis, renewal_best};
use being::run_loop::{offers_of, world_for};
use being::solver::{SLIP_PER_MILLE, STRATEGIES_EXT};
use being::world::Domain;

/// Expected reward per energy of the renewal policy with chance table `t` over `budget` energy (no execution:
/// the expected reward p x reward of each choice is summed; Dinkelbach iterations on rho).
fn ev_rate(seed: u64, budget: i64, t: &Truth, iters: usize) -> f64 {
    let w = world_for(&genesis(seed, budget));
    let mut rho = 3.0;
    let mut rate = 0.0;
    for _ in 0..iters.max(1) {
        let (mut rew, mut spent, mut step) = (0.0f64, 0i64, 0u64);
        while budget - spent >= SKIP_COST_FW {
            let offers = offers_of(&w.offers(step));
            let left = budget - spent;
            match renewal_best(t, &STRATEGIES_EXT, &offers, left, rho) {
                Some((v, slot, s)) if v >= -rho * SKIP_COST_FW as f64 => {
                    let o = offers.iter().find(|o| o.slot == slot).unwrap();
                    rew += o.listed_reward as f64 * t.p[o.domain.idx()][o.difficulty as usize - 1][s.idx()];
                    spent += s.energy(o.listed_cost);
                }
                _ => spent += SKIP_COST_FW,
            }
            step += 1;
        }
        rate = rew / spent.max(1) as f64;
        rho = rate;
    }
    rate
}

const SKIP_COST_FW: i64 = being::world::SKIP_COST;

fn arm_cfg(spec: &str, s: u64, steps: u64, window: u64) -> RunCfgB14 {
    use being::b14::curiosity::{CurCfg, DEFAULT_CUR};
    use being::b14::library::LibCfg;
    let mut c = RunCfgB14::base(s, steps, window);
    for part in spec.split('+') {
        if part == "base" {
        } else if part == "lib" {
            c.library = Some(LibCfg::default());
        } else if let Some(x) = part.strip_prefix("cur") {
            let mut it = x.split(':');
            let pm: u32 = it.next().unwrap().parse().expect("cur<pm>");
            let ei: f64 = it.next().map_or(DEFAULT_CUR.ei_w, |v| v.parse().unwrap());
            c.curiosity = Some(CurCfg { pm, ei_w: ei, ..DEFAULT_CUR });
        } else {
            panic!("unknown arm part {part}");
        }
    }
    c
}

fn post(o: &OutB14, from: u64) -> f64 {
    let (r, c) = o.wins.iter().filter(|w| w.from >= from).fold((0i64, 0i64), |a, w| (a.0 + w.reward, a.1 + w.cost));
    r as f64 / c.max(1) as f64
}

fn cmd_fw(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let steps: u64 = arg(args, "--steps").map_or(200_000, |x| x.parse().unwrap());
    let window: u64 = arg(args, "--window").map_or(10_000, |x| x.parse().unwrap());
    let from: u64 = arg(args, "--post").map_or(50_000, |x| x.parse().unwrap());
    let arms: Vec<String> = arg(args, "--arms").unwrap_or("lib+cur20".into()).split(',').map(|x| x.to_string()).collect();
    let t0 = std::time::Instant::now();
    let jobs: Vec<(u64, usize)> = ss.iter().flat_map(|&s| (0..arms.len()).map(move |a| (s, a))).collect();
    let res: Vec<((u64, usize), OutB14)> = jobs
        .par_iter()
        .map(|&(s, a)| {
            let o = run_b14(&RunCfgB14 { cover_n: 40, ..arm_cfg(&arms[a], s, steps, window) });
            eprintln!("world {s} arm {} done at {:.0} s", arms[a], t0.elapsed().as_secs_f64());
            ((s, a), o)
        })
        .collect();
    let refs: Vec<(u64, Vec<(String, f64)>)> = ss
        .par_iter()
        .map(|&s| {
            let w = world_for(&genesis(s, 3000));
            let book = oracle_book(&w, 60);
            let own = &res.iter().find(|r| r.0 == (s, 0)).unwrap().1;
            let mut union = book.clone();
            union.extend(own.rules_final.iter().filter(|r| !r.retired).cloned());
            let t_book = truth_in(&w, &book);
            let t_union = truth_in(&w, &union);
            let mut t_perf = Truth { p: t_union.p };
            let pd = Domain::Pattern.idx();
            for k in 0..5 {
                let exact = 1.0 - SLIP_PER_MILLE[pd] as f64 / 1000.0;
                let x = &mut t_perf.p[pd][k][3];
                *x = x.max(exact);
            }
            let mut t_noslip = Truth { p: t_perf.p };
            for d in 0..t_noslip.p.len() {
                for k in 0..5 {
                    for st in 0..4 {
                        // deep is always exact: without slips it is right every time; the others keep their
                        // non-slip misses (fast heuristics, careful on hard difficulties)
                        let slip = match st { 0 => (3 * SLIP_PER_MILLE[d]).min(400), 2 => SLIP_PER_MILLE[d] / 2, _ => SLIP_PER_MILLE[d] } as f64 / 1000.0;
                        let x = &mut t_noslip.p[d][k][st];
                        if *x > 0.0 {
                            *x = (*x / (1.0 - slip)).min(1.0);
                        }
                    }
                }
            }
            let v = vec![
                ("oracle_norules".to_string(), renewal_rate(s, 30_000, &[], 4)),
                ("oracle_book".to_string(), renewal_rate(s, 30_000, &book, 4)),
                ("oracle_book_plus_own".to_string(), renewal_rate(s, 30_000, &union, 4)),
                ("ev_oracle_book".to_string(), ev_rate(s, 30_000, &t_book, 4)),
                ("ev_oracle_book_plus_own".to_string(), ev_rate(s, 30_000, &t_union, 4)),
                ("ev_oracle_perfect_book".to_string(), ev_rate(s, 30_000, &t_perf, 4)),
                ("ev_oracle_perfect_book_noslip".to_string(), ev_rate(s, 30_000, &t_noslip, 4)),
            ];
            (s, v)
        })
        .collect();
    eprintln!("fw: {:.0} s", t0.elapsed().as_secs_f64());
    let rn: Vec<String> = refs[0].1.iter().map(|x| x.0.clone()).collect();
    println!("seed\t{}\t{}\textra", arms.iter().map(|a| format!("post_{a}")).collect::<Vec<_>>().join("\t"), rn.join("\t"));
    for &s in &ss {
        let r = &refs.iter().find(|x| x.0 == s).unwrap().1;
        let mut extra = String::new();
        for a in 0..arms.len() {
            let o = &res.iter().find(|x| x.0 == (s, a)).unwrap().1;
            let last = o.wins.last().unwrap();
            let rule_ans: u64 = o.wins.iter().filter(|w| w.from >= from).map(|w| w.rule_answers).sum();
            let rule_ok: u64 = o.wins.iter().filter(|w| w.from >= from).map(|w| w.rule_right).sum();
            let skips: u64 = o.wins.iter().filter(|w| w.from >= from).map(|w| w.skips).sum();
            let n: u64 = o.wins.iter().filter(|w| w.from >= from).map(|w| w.steps).sum();
            extra.push_str(&format!(
                " arm{a}_wins={} arm{a}_covered={} arm{a}_live={} arm{a}_rule_answers={} arm{a}_rule_right={:.4} arm{a}_skip_share={:.4} arm{a}_cur_share={:.5}",
                o.wins.iter().map(|w| format!("{:.4}", w.rpe())).collect::<Vec<_>>().join(","),
                last.cover.iter().filter(|c| **c >= 0.9).count(),
                last.live_end,
                rule_ans,
                rule_ok as f64 / rule_ans.max(1) as f64,
                skips as f64 / n.max(1) as f64,
                o.cur_energy as f64 / o.total_cost.max(1) as f64
            ));
        }
        let posts: Vec<String> = (0..arms.len()).map(|a| format!("{:.6}", post(&res.iter().find(|x| x.0 == (s, a)).unwrap().1, from))).collect();
        println!("{s}\t{}\t{}\t{}", posts.join("\t"), r.iter().map(|x| format!("{:.6}", x.1)).collect::<Vec<_>>().join("\t"), extra.trim());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("wt") => cmd_wt(&args[1..]),
        Some("fw") => cmd_fw(&args[1..]),
        _ => eprintln!("usage: measure_n1diag wt|fw [--seeds 101..120] [--steps N] [--arms a,b]"),
    }
}
