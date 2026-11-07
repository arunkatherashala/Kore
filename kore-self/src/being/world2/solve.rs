//! The being's fixed solvers in World v2 (the same four strategies as the menu world, `solver.rs`).
//!
//! | type | fast (half energy, 3x slips) | careful (listed energy) | deep (2x energy, half slips) | discovered (listed energy) |
//! |---|---|---|---|---|
//! | seq | arithmetic check, else last difference | the B1 `careful` detector: arithmetic, geometric, quadratic, fibonacci-like, else last difference | careful + cubic, period 2/3, interleaved steps | the being's own accepted rule for (family, difficulty), else careful |
//! | compose | fast, iterated h times | careful, iterated | deep, iterated | the rule of (base family, difficulty), iterated; else careful |
//! | grid | Manhattan distance | breadth-first search, 4-neighbour moves | breadth-first search, 8-neighbour moves | careful |
//! | knap | greedy by value/weight | best of greedy and the single best item | exact dynamic programme | careful |
//! | formula | textbook formula, g = 10 | textbook formula, g = 10 | textbook formula, g = 10 | careful |
//!
//! None of them reads hidden parameters (rule class, constant, move rule): those are what
//! the being would have to learn. A slip (answer off by one) happens when the per-attempt
//! noise falls under the family's slip rate times the strategy's factor, for answers the
//! solver computed by an exact method (as in `solver.rs`).

use super::world::{bfs, knap_exact, FType, FormulaKind, Task2, TaskView2, World2};
use super::rules_use::{const_for, formula_answer, ConstRule};
use crate::being::discover::{self, Rule};
use crate::being::solver::Strategy;

/// Signature of a (family, difficulty) for the rule search: one group per observable cell.
pub fn sig2(fam: usize, difficulty: u8) -> u8 {
    (fam * 5 + (difficulty.clamp(1, 5) as usize - 1)) as u8
}

/// Inverse of `sig2`.
pub fn sig2_split(sig: u8) -> (usize, u8) {
    (sig as usize / 5, (sig % 5) + 1)
}

/// Textbook constant the fixed solvers use.
pub const TEXTBOOK_G: i64 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Prop2 {
    pub answer: i64,
    /// Computed by a method the solver treats as exact (slips apply).
    pub exact: bool,
    pub method: String,
    pub rule_id: Option<String>,
}

pub fn slip_pm(s: Strategy, fam_slip: u64) -> u64 {
    match s {
        Strategy::Fast => (3 * fam_slip).min(400),
        Strategy::Careful | Strategy::Discovered => fam_slip,
        Strategy::Deep => fam_slip / 2,
    }
}

/// Answer `task` with strategy `s`, slips included.
pub fn attempt(w: &World2, task: &Task2, s: Strategy, rules: &[Rule]) -> Prop2 {
    attempt_c(w, task, s, rules, &[])
}

/// As `attempt`, with the being's accepted constants (B7 C; empty: exactly `attempt`).
pub fn attempt_c(w: &World2, task: &Task2, s: Strategy, rules: &[Rule], consts: &[ConstRule]) -> Prop2 {
    let v = &task.view;
    let mut p = solve_c(v, s, rules, consts);
    if p.exact && w.slip_noise(v.step, v.slot) % 1000 < slip_pm(s, w.fams[v.fam].slip_pm) {
        p.answer += 1;
        p.method.push_str("+slip");
    }
    p
}

/// The oracle's answer: the checker's ground truth, slips of the careful strategy included.
pub fn attempt_oracle(w: &World2, task: &Task2) -> Prop2 {
    let v = &task.view;
    let mut a = task.expected();
    let mut method = "oracle:ground-truth".to_string();
    if w.slip_noise(v.step, v.slot) % 1000 < slip_pm(Strategy::Careful, w.fams[v.fam].slip_pm) {
        a += 1;
        method.push_str("+slip");
    }
    Prop2 { answer: a, exact: true, method, rule_id: None }
}

pub fn solve(v: &TaskView2, s: Strategy, rules: &[Rule]) -> Prop2 {
    solve_c(v, s, rules, &[])
}

/// As `solve`; with B7 C, `discovered` on a formula task answers with the latest accepted constant of its family.
pub fn solve_c(v: &TaskView2, s: Strategy, rules: &[Rule], consts: &[ConstRule]) -> Prop2 {
    let name = s.name();
    if let (FType::Formula, Strategy::Discovered, Some(c)) = (v.ftype, s, const_for(consts, v.fam)) {
        let a = formula_answer(v.formula == Some(FormulaKind::Fall), c.g, v.params[0]);
        return Prop2 { answer: a, exact: true, method: format!("{name}/rule:{}:const-g{}", c.id, c.g), rule_id: Some(c.id.clone()) };
    }
    if let (FType::Grid, Strategy::Discovered, Some(c)) = (v.ftype, s, const_for(consts, v.fam)) {
        let a = grid_bfs(v, c.g == 8);
        return Prop2 { answer: a, exact: true, method: format!("{name}/rule:{}:bfs-{}", c.id, c.g), rule_id: Some(c.id.clone()) };
    }
    let mk = |(a, e, m): (i64, bool, String)| Prop2 { answer: a, exact: e, method: format!("{name}/{m}"), rule_id: None };
    match (v.ftype, s) {
        (FType::Seq | FType::Compose, Strategy::Discovered) => {
            let sig = sig2(v.base.unwrap_or(v.fam), v.difficulty);
            let h = v.horizon.max(1) as usize;
            let mut t = v.params.clone();
            let mut used: Option<String> = None;
            let mut ok = true;
            for _ in 0..h {
                match discover::apply(rules, sig, &t) {
                    Some((x, r)) => {
                        used.get_or_insert_with(|| format!("{}:{}{}", r.id, if r.xf.is_id() { String::new() } else { format!("{}+", r.xf.code()) }, r.form.code()));
                        t.push(x);
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            match (ok, used) {
                (true, Some(u)) => {
                    let id = u.split(':').next().unwrap_or("").to_string();
                    Prop2 { answer: *t.last().expect("h >= 1"), exact: true, method: format!("{name}/rule:{u}"), rule_id: Some(id) }
                }
                _ => {
                    let (a, e, m) = seq_iter(v, careful_seq);
                    Prop2 { answer: a, exact: e, method: format!("{name}/fallback:{m}"), rule_id: None }
                }
            }
        }
        (FType::Seq | FType::Compose, Strategy::Fast) => mk(seq_iter(v, fast_seq)),
        (FType::Seq | FType::Compose, Strategy::Careful) => mk(seq_iter(v, careful_seq)),
        (FType::Seq | FType::Compose, Strategy::Deep) => mk(seq_iter(v, deep_seq)),
        (FType::Grid, Strategy::Fast) => mk(((v.params[0] - 1) + (v.params[1] - 1), false, "heuristic:manhattan".into())),
        (FType::Grid, Strategy::Deep) => mk((grid_bfs(v, true), true, "exact:bfs-8".into())),
        (FType::Grid, _) => mk((grid_bfs(v, false), true, "exact:bfs-4".into())),
        (FType::Knap, Strategy::Fast) => mk((knap_greedy(v), false, "heuristic:greedy".into())),
        (FType::Knap, Strategy::Deep) => mk((knap_exact(v.params[0], &items(v)), true, "exact:dp".into())),
        (FType::Knap, _) => {
            let single = items(v).iter().filter(|x| x.0 <= v.params[0]).map(|x| x.1).max().unwrap_or(0);
            mk((knap_greedy(v).max(single), false, "heuristic:greedy-or-single".into()))
        }
        (FType::Formula, _) => {
            let x = v.params[0];
            let a = match v.formula {
                Some(FormulaKind::Fall) => TEXTBOOK_G * x * x / 2,
                _ => x * TEXTBOOK_G,
            };
            mk((a, true, "exact:textbook-g10".into()))
        }
    }
}

fn grid_bfs(v: &TaskView2, eight: bool) -> i64 {
    let (r, c) = (v.params[0] as usize, v.params[1] as usize);
    bfs(r, c, &v.params[2..], eight).unwrap_or(-1)
}

fn items(v: &TaskView2) -> Vec<(i64, i64)> {
    v.params[1..].chunks(2).map(|c| (c[0], c[1])).collect()
}

fn knap_greedy(v: &TaskView2) -> i64 {
    let mut it = items(v);
    // value per weight, highest first; ties by lighter item, then by order (stable sort)
    it.sort_by(|a, b| (b.1 * a.0).cmp(&(a.1 * b.0)).then(a.0.cmp(&b.0)));
    let (mut left, mut total) = (v.params[0], 0);
    for (w, val) in it {
        if w <= left {
            left -= w;
            total += val;
        }
    }
    total
}

/// Apply a next-term solver `horizon` times (1 for seq tasks).
fn seq_iter(v: &TaskView2, f: fn(&[i64]) -> (i64, bool, String)) -> (i64, bool, String) {
    let mut t = v.params.clone();
    let mut exact = true;
    let mut method = String::new();
    for i in 0..v.horizon.max(1) {
        let (a, e, m) = f(&t);
        exact &= e;
        if i == 0 {
            method = m;
        }
        t.push(a);
    }
    (*t.last().expect("non-empty"), exact, method)
}

fn last_difference(t: &[i64]) -> (i64, bool, String) {
    let n = t.len();
    (t[n - 1] + (t[n - 1] - t[n - 2]), false, "heuristic:last-difference".into())
}

fn fast_seq(t: &[i64]) -> (i64, bool, String) {
    let n = t.len();
    let d1: Vec<i64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    if d1.iter().all(|d| *d == d1[0]) {
        return (t[n - 1] + d1[0], true, "exact:arithmetic".into());
    }
    last_difference(t)
}

/// The B1 careful detector (`solver.rs`, `pattern`), restated for World v2.
fn careful_seq(t: &[i64]) -> (i64, bool, String) {
    let n = t.len();
    let d1: Vec<i64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    if d1.iter().all(|d| *d == d1[0]) {
        return (t[n - 1] + d1[0], true, "exact:arithmetic".into());
    }
    if t[0] != 0 && t.windows(2).all(|w| w[0] != 0 && w[1] % w[0] == 0 && w[1] / w[0] == t[1] / t[0]) {
        return (t[n - 1] * (t[1] / t[0]), true, "exact:geometric".into());
    }
    let d2: Vec<i64> = d1.windows(2).map(|w| w[1] - w[0]).collect();
    if d2.iter().all(|d| *d == d2[0]) {
        return (t[n - 1] + d1[d1.len() - 1] + d2[0], true, "exact:quadratic".into());
    }
    if t.windows(3).all(|w| w[2] == w[0] + w[1]) {
        return (t[n - 1] + t[n - 2], true, "exact:fibonacci-like".into());
    }
    last_difference(t)
}

fn deep_seq(t: &[i64]) -> (i64, bool, String) {
    let c = careful_seq(t);
    if c.1 {
        return c;
    }
    let n = t.len();
    let d1: Vec<i64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    let d2: Vec<i64> = d1.windows(2).map(|w| w[1] - w[0]).collect();
    let d3: Vec<i64> = d2.windows(2).map(|w| w[1] - w[0]).collect();
    if d3.len() >= 2 && d3.iter().all(|d| *d == d3[0]) {
        return (t[n - 1] + d1[d1.len() - 1] + d2[d2.len() - 1] + d3[0], true, "exact:cubic".into());
    }
    for p in [2usize, 3] {
        if n >= 2 * p && (p..n).all(|i| t[i] == t[i - p]) {
            return (t[n - p], true, format!("exact:period-{p}"));
        }
    }
    let s2: Vec<i64> = (2..n).map(|i| t[i] - t[i - 2]).collect();
    if s2.len() >= 2 && s2.iter().all(|d| *d == s2[0]) {
        return (t[n - 2] + s2[0], true, "exact:interleaved-steps".into());
    }
    last_difference(t)
}
