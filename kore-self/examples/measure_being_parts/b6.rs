//! Track B6 measurement: transfer between domains.
//!   cargo run -p kore-self --example measure_being --offline --release -- b6
//! Modes: b6 (report), b6-probe SEED, b6-tune. Report seeds 1..=20; tuning seeds 101..=120.
//! Synthetic worlds written by the same author as the learner; hidden truth is used here to score the being, never by it.

use super::being::measure::mean_ci;
use super::being::measure_b6::*;
use super::being::policy::Policy;
use super::being::world::Domain;

const BUDGET: i64 = 3000;
const ARMS: [Arm; 8] = [Arm::NoTransfer, Arm::Transfer, Arm::LinkOnly, Arm::ShiftedCells, Arm::ShiftedNoGuard, Arm::WrongSim, Arm::WrongSimNoGuard, Arm::Oracle];
const WINDOWS: [usize; 3] = [50, 100, 200];
const TT_W: usize = 20;

pub fn probe(seed: u64, budget: i64) {
    let pol = Policy::new(100_000);
    for arm in ARMS {
        let o = run_b6(seed, budget, arm, B6Cfg::default(), &pol);
        let (nb, rb, cb) = o.domain_first(Domain::PatternB, usize::MAX);
        let (nc, rc, cc) = o.domain_first(Domain::Control, usize::MAX);
        let (na, ra, ca) = o.domain_first(Domain::Pattern, usize::MAX);
        println!(
            "{:<28} steps {} t_b clock {} total {:.3} first100 {:.3} after {:.3} | B n={nb} {:.3} | C n={nc} {:.3} | A n={na} {:.3} | links {} (B {}, C {}) revoked {} | rules A {} B {} | b-rule steps {:?} | refusals {}",
            arm.name(), o.recs.len(), o.t_b, o.total_rate(), o.rate_first(100), o.rate_after(),
            rb as f64 / cb.max(1) as f64, rc as f64 / cc.max(1) as f64, ra as f64 / ca.max(1) as f64,
            o.n_links(), o.links_by_target().0, o.links_by_target().1, o.n_revoked(),
            o.accepted.iter().filter(|r| r.1.sig < 10).count(), o.accepted.iter().filter(|r| r.1.sig >= 10).count(), o.b_rule_steps(), o.refusals.len(),
        );
        for l in o.links.iter().take(3) {
            println!("    {:?} target {} k {} from {:?} step {} ev {}", l.kind, l.target.name(), l.difficulty, l.src.map(|d| d.name()), l.step, l.evidence);
        }
    }
}

fn line(name: &str, xs: &[f64]) {
    let (m, lo, hi) = mean_ci(xs);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] |");
}

fn paired(name: &str, a: &[f64], b: &[f64]) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mean_ci(&d);
    let (w, l) = (a.iter().zip(b).filter(|(x, y)| x > y).count(), a.iter().zip(b).filter(|(x, y)| x < y).count());
    println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} wins, {l} losses, {} ties of {} |", a.len() - w - l, a.len());
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(v[v.len() / 2])
}

pub struct Row {
    pub seed: u64,
    pub outs: Vec<B6Out>,
}

pub fn run_rows(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, cfg: B6Cfg, arms: &[Arm]) -> Vec<Row> {
    let seeds: Vec<u64> = seeds.collect();
    let results: std::sync::Mutex<Vec<Row>> = std::sync::Mutex::new(vec![]);
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..6 {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if i >= seeds.len() {
                    break;
                }
                let pol = Policy::new(100_000);
                let s = seeds[i];
                let row = Row { seed: s, outs: arms.iter().map(|a| run_b6(s, budget, *a, cfg, &pol)).collect() };
                results.lock().unwrap().push(row);
            });
        }
    });
    let mut rows = results.into_inner().unwrap();
    rows.sort_by_key(|r| r.seed);
    eprintln!("ran {label}: {} worlds x {} arms", rows.len(), arms.len());
    rows
}

fn col(rows: &[Row], a: usize, f: impl Fn(&B6Out) -> f64) -> Vec<f64> {
    rows.iter().map(|r| f(&r.outs[a])).collect()
}

pub fn report(label: &str, rows: &[Row], budget: i64) {
    let n = rows.len();
    println!("\n### Transfer world, {label} (budget {budget}, discovery pass every 10 steps, composition on, n = {n} worlds)\n");
    let ai = |a: Arm| ARMS.iter().position(|x| *x == a).unwrap();
    let (no, tr, lo, sh, sn, ws, wn, or) = (ai(Arm::NoTransfer), ai(Arm::Transfer), ai(Arm::LinkOnly), ai(Arm::ShiftedCells), ai(Arm::ShiftedNoGuard), ai(Arm::WrongSim), ai(Arm::WrongSimNoGuard), ai(Arm::Oracle));
    let t_b: Vec<f64> = rows.iter().map(|r| r.outs[0].t_b as f64 / 30.0).collect();
    println!("Energy spent before the new domains appear (hidden schedule): mean {:.0}, range {:.0}-{:.0}.", t_b.iter().sum::<f64>() / n as f64, t_b.iter().cloned().fold(f64::MAX, f64::min), t_b.iter().cloned().fold(f64::MIN, f64::max));
    let a_rules: Vec<f64> = col(rows, no, |o| o.a_rules_before_tb() as f64);
    println!("Rules for the first domain held when the new domains appear (no-transfer arm): mean {:.2}; worlds with at least one: {} of {n}.\n", a_rules.iter().sum::<f64>() / n as f64, a_rules.iter().filter(|x| **x > 0.0).count());
    for w in WINDOWS {
        println!("\nReward per energy unit over the first {w} steps after the new domains appear (all domains, skips included), mean [95% interval of the mean]:\n");
        println!("| arm | mean | 95% interval |");
        println!("|---|---|---|");
        for (i, a) in ARMS.iter().enumerate() {
            line(a.name(), &col(rows, i, |o| o.rate_first(w)));
        }
        println!("\n| paired difference (first {w} steps) | mean | 95% interval | first minus second |");
        println!("|---|---|---|---|");
        let f = |i: usize| col(rows, i, |o| o.rate_first(w));
        paired("transfer minus no transfer", &f(tr), &f(no));
        paired("links only minus no transfer", &f(lo), &f(no));
        paired("transfer minus links only (the search order)", &f(tr), &f(lo));
        paired("right domain, shifted cells minus no transfer", &f(sh), &f(no));
        paired("transfer minus right domain with shifted cells", &f(tr), &f(sh));
        paired("shifted cells, guard off, minus no transfer", &f(sn), &f(no));
        paired("shifted cells, guard on minus guard off", &f(sh), &f(sn));
        paired("wrong similarity (guard on) minus no transfer", &f(ws), &f(no));
        paired("wrong similarity (guard off) minus no transfer", &f(wn), &f(no));
        paired("wrong similarity (guard on) minus wrong similarity (guard off)", &f(ws), &f(wn));
        paired("transfer minus wrong similarity (guard on)", &f(tr), &f(ws));
        paired("oracle minus no transfer", &f(or), &f(no));
    }
    println!("\nWhole time after the new domains appear:\n");
    println!("| arm | reward per energy | 95% interval |");
    println!("|---|---|---|");
    for (i, a) in ARMS.iter().enumerate() {
        line(a.name(), &col(rows, i, |o| o.rate_after()));
    }
    println!("\n| paired difference (whole time after) | mean | 95% interval | first minus second |");
    println!("|---|---|---|---|");
    let f = |i: usize| col(rows, i, |o| o.rate_after());
    paired("transfer minus no transfer", &f(tr), &f(no));
    paired("links only minus no transfer", &f(lo), &f(no));
    paired("right domain, shifted cells minus no transfer", &f(sh), &f(no));
    paired("shifted cells, guard off, minus no transfer", &f(sn), &f(no));
    paired("wrong similarity (guard on) minus no transfer", &f(ws), &f(no));
    paired("wrong similarity (guard off) minus no transfer", &f(wn), &f(no));
    println!("\nRegret per energy unit against the oracle of the same world, first 100 steps after the new domains appear (oracle rate minus arm rate):\n");
    println!("| arm | mean regret | 95% interval |");
    println!("|---|---|---|");
    for (i, a) in ARMS.iter().enumerate().filter(|(i, _)| *i != or) {
        let reg: Vec<f64> = rows.iter().map(|r| r.outs[or].rate_first(100) - r.outs[i].rate_first(100)).collect();
        line(a.name(), &reg);
    }
    println!("\nDomain B (the sequence families seen through the hidden map), attempts in the first 150 steps after it appears:\n");
    println!("| arm | attempts on B per world | reward per energy on B attempts (pooled over worlds) |");
    println!("|---|---|---|");
    for (i, a) in ARMS.iter().enumerate() {
        let (mut na, mut r, mut c) = (0usize, 0i64, 0i64);
        for row in rows {
            let x = row.outs[i].domain_first(Domain::PatternB, 150);
            na += x.0;
            r += x.1;
            c += x.2;
        }
        println!("| {} | {:.1} | {:.3} |", a.name(), na as f64 / n as f64, r as f64 / c.max(1) as f64);
    }
    println!("\nSteps and energy after the new domains appear until the last {TT_W} attempts on B reach 90% of the oracle's reward per energy on B (oracle: attempts on B after the change, whole run):\n");
    println!("| arm | worlds that reach it | median steps | median energy |");
    println!("|---|---|---|---|");
    let target = |r: &Row| -> f64 { r.outs[or].domain_rate_after(Domain::PatternB).1 };
    for (i, a) in ARMS.iter().enumerate() {
        let tt: Vec<Option<(usize, i64)>> = rows.iter().map(|r| r.outs[i].time_to(Domain::PatternB, target(r), 0.9, TT_W)).collect();
        let reached: Vec<(usize, i64)> = tt.iter().flatten().cloned().collect();
        println!(
            "| {} | {} of {n} | {} | {} |",
            a.name(),
            reached.len(),
            median(reached.iter().map(|x| x.0 as f64).collect()).map_or("-".into(), |m| format!("{m:.0}")),
            median(reached.iter().map(|x| x.1 as f64).collect()).map_or("-".into(), |m| format!("{m:.0}"))
        );
    }
    let both: Vec<(usize, usize)> = rows
        .iter()
        .filter_map(|r| {
            let a = r.outs[no].time_to(Domain::PatternB, target(r), 0.9, TT_W)?;
            let b = r.outs[tr].time_to(Domain::PatternB, target(r), 0.9, TT_W)?;
            Some((a.0, b.0))
        })
        .collect();
    let d: Vec<f64> = both.iter().map(|x| x.1 as f64 - x.0 as f64).collect();
    if d.len() >= 2 {
        let (m, lo, hi) = mean_ci(&d);
        let (w, l) = (both.iter().filter(|x| x.1 < x.0).count(), both.iter().filter(|x| x.1 > x.0).count());
        println!("\nWorlds where both no-transfer and transfer reach it: {}; steps, transfer minus no transfer {m:+.1} [{lo:+.1}, {hi:+.1}], transfer faster in {w}, slower in {l}, ties {}.", both.len(), both.len() - w - l);
    } else {
        println!("\nWorlds where both no-transfer and transfer reach it: {} (too few for an interval).", both.len());
    }
    println!("\nRules accepted and their exactness on 300 fresh tasks of their own (domain, difficulty), all worlds:\n");
    println!("| arm | rules accepted | exact | applicable-and-wrong | never applicable on fresh tasks | rules for B | first B rule: median steps after the change |");
    println!("|---|---|---|---|---|---|---|");
    for (i, a) in ARMS.iter().enumerate().filter(|(i, _)| *i != or) {
        let (mut tot, mut ex, mut wr, mut ine, mut nb) = (0, 0, 0, 0, 0);
        let mut det = vec![];
        let mut steps = vec![];
        for row in rows {
            let o = &row.outs[i];
            let (e, w, inn, d) = wrong_rules(row.seed, budget, o);
            ine += inn;
            tot += o.accepted.len();
            ex += e;
            wr += w;
            nb += o.accepted.iter().filter(|r| r.1.sig >= 10).count();
            det.extend(d);
            steps.extend(o.b_rule_steps().into_iter().map(|x| x as f64));
        }
        println!("| {} | {tot} | {ex} | {wr} | {ine} | {nb} | {} |", a.name(), median(steps).map_or("-".into(), |m| format!("{m:.0}")));
        for d in det {
            println!("  {d}");
        }
    }
    println!("\nControl domain (unit conversions, shares no sequence structure), first 150 steps after the new domains appear:\n");
    println!("| arm | attempts per world | reward per energy on control attempts (pooled) |");
    println!("|---|---|---|");
    for (i, a) in ARMS.iter().enumerate() {
        let (mut na, mut r, mut c) = (0usize, 0i64, 0i64);
        for row in rows {
            let x = row.outs[i].domain_first(Domain::Control, 150);
            na += x.0;
            r += x.1;
            c += x.2;
        }
        println!("| {} | {:.1} | {:.3} |", a.name(), na as f64 / n as f64, r as f64 / c.max(1) as f64);
    }
    let cf = |i: usize| col(rows, i, |o| o.domain_first(Domain::Control, 150).1 as f64);
    println!("\n| paired difference, reward points earned on control attempts in the first 150 steps | mean | 95% interval | first minus second |");
    println!("|---|---|---|---|");
    paired("transfer minus no transfer", &cf(tr), &cf(no));
    paired("links only minus no transfer", &cf(lo), &cf(no));
    paired("right domain, shifted cells minus no transfer", &cf(sh), &cf(no));
    paired("shifted cells, guard off, minus no transfer", &cf(sn), &cf(no));
    paired("wrong similarity (guard on) minus no transfer", &cf(ws), &cf(no));
    paired("wrong similarity (guard off) minus no transfer", &cf(wn), &cf(no));
    println!("\nLinks:\n");
    println!("| arm | links per world (mean) | links into B | links into control | revoked by the guard | worlds with at least one link |");
    println!("|---|---|---|---|---|---|");
    for (i, a) in ARMS.iter().enumerate().filter(|(i, _)| *i != or && *i != no) {
        let l: Vec<f64> = col(rows, i, |o| o.n_links() as f64);
        let (b, c) = rows.iter().fold((0, 0), |acc, r| {
            let x = r.outs[i].links_by_target();
            (acc.0 + x.0, acc.1 + x.1)
        });
        let rv: usize = rows.iter().map(|r| r.outs[i].n_revoked()).sum();
        println!("| {} | {:.2} | {b} | {c} | {rv} | {} of {n} |", a.name(), l.iter().sum::<f64>() / n as f64, l.iter().filter(|x| **x > 0.0).count());
    }
}

pub fn tune() {
    println!("\n### Transfer settings on tuning seeds 101..120 (never used for the report)\n");
    println!("| tau | xfer_k | transfer minus no transfer, first 100 steps (mean [interval], wins/losses/ties) | links/world |");
    println!("|---|---|---|---|");
    let base = run_rows("tuning no transfer", 101..=120, BUDGET, B6Cfg::default(), &[Arm::NoTransfer]);
    let nb: Vec<f64> = base.iter().map(|r| r.outs[0].rate_first(100)).collect();
    for tau in [0.34, 0.5, 0.67] {
        for k in [4.0, 8.0, 16.0] {
            let cfg = B6Cfg { tau, xfer_k: k, ..B6Cfg::default() };
            let rows = run_rows("tuning", 101..=120, BUDGET, cfg, &[Arm::Transfer]);
            let t: Vec<f64> = rows.iter().map(|r| r.outs[0].rate_first(100)).collect();
            let d: Vec<f64> = t.iter().zip(&nb).map(|(a, b)| a - b).collect();
            let (m, lo, hi) = mean_ci(&d);
            let (w, l) = (d.iter().filter(|x| **x > 1e-12).count(), d.iter().filter(|x| **x < -1e-12).count());
            let links: f64 = rows.iter().map(|r| r.outs[0].n_links() as f64).sum::<f64>() / rows.len() as f64;
            println!("| {tau} | {k} | {m:+.3} [{lo:+.3}, {hi:+.3}] {w}/{l}/{} | {links:.2} |", d.len() - w - l);
        }
    }
}

pub fn tune_guard() {
    println!("\n### Guard threshold on tuning seeds 101..120 (shifted-cells control, never used for the report)\n");
    println!("| guard (nats) | guard on minus guard off, first 100 steps (mean [interval], wins/losses/ties) | guard on minus no transfer | transfer (right source) minus no transfer | links revoked per world (shifted) | links revoked per world (right source) |");
    println!("|---|---|---|---|---|---|");
    let base = run_rows("tuning no transfer", 101..=120, BUDGET, B6Cfg::default(), &[Arm::NoTransfer, Arm::ShiftedNoGuard]);
    let nb: Vec<f64> = base.iter().map(|r| r.outs[0].rate_first(100)).collect();
    let nf: Vec<f64> = base.iter().map(|r| r.outs[1].rate_first(100)).collect();
    for guard in [0.5, 1.0, 2.0, 3.0, 5.0] {
        let cfg = B6Cfg { guard_nats: guard, ..B6Cfg::default() };
        let on = run_rows("tuning guard on", 101..=120, BUDGET, cfg, &[Arm::ShiftedCells, Arm::Transfer]);
        let t: Vec<f64> = on.iter().map(|r| r.outs[0].rate_first(100)).collect();
        let tr: Vec<f64> = on.iter().map(|r| r.outs[1].rate_first(100)).collect();
        let d1: Vec<f64> = t.iter().zip(&nf).map(|(a, b)| a - b).collect();
        let d2: Vec<f64> = t.iter().zip(&nb).map(|(a, b)| a - b).collect();
        let d3: Vec<f64> = tr.iter().zip(&nb).map(|(a, b)| a - b).collect();
        let (m1, l1, h1) = mean_ci(&d1);
        let (m2, l2, h2) = mean_ci(&d2);
        let (m3, l3, h3) = mean_ci(&d3);
        let (w, l) = (d1.iter().filter(|x| **x > 1e-12).count(), d1.iter().filter(|x| **x < -1e-12).count());
        let rv: f64 = on.iter().map(|r| r.outs[0].n_revoked() as f64).sum::<f64>() / on.len() as f64;
        let rv2: f64 = on.iter().map(|r| r.outs[1].n_revoked() as f64).sum::<f64>() / on.len() as f64;
        println!("| {guard} | {m1:+.3} [{l1:+.3}, {h1:+.3}] {w}/{l}/{} | {m2:+.3} [{l2:+.3}, {h2:+.3}] | {m3:+.3} [{l3:+.3}, {h3:+.3}] | {rv:.2} | {rv2:.2} |", d1.len() - w - l);
    }
}

pub fn main_b6(args: &[String]) {
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("b6");
    let g = |i: usize, d: u64| args.get(i).and_then(|x| x.parse::<u64>().ok()).unwrap_or(d);
    if mode == "b6-probe" {
        probe(g(3, 1), g(4, BUDGET as u64) as i64);
        return;
    }
    if mode == "b6-tune" {
        tune();
        return;
    }
    if mode == "b6-tune-guard" {
        tune_guard();
        return;
    }
    let budget = g(3, BUDGET as u64) as i64;
    let rows = run_rows("report seeds 1..20", 1..=20, budget, B6Cfg::default(), &ARMS);
    report("report seeds 1..20", &rows, budget);
}
