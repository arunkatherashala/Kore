//! Track E1, part 2: evolutionary search (genetic programming) over the rule expression language.
//!
//! One evolution PASS (every `EvoCfg::every` steps, while the budget allows) works on one signature
//! (domain and difficulty) whose recent revealed TRAINING examples are not yet explained by a live
//! rule (no live rule predicts their revealed next term):
//!
//! * population: the elites of the last logged pass on this signature (folded from the log), the
//!   being's verified rules of every signature as genomes (inherited genes), the 17 grammar forms,
//!   and random expressions; every candidate is (transform of B5, genome);
//! * fitness, on a sample of the signature's own training examples (held-out examples are never
//!   looked at): +1 per unexplained example reproduced exactly, -3 per example where the candidate is
//!   applicable and wrong (explained or not), +0.5 x the share of prefix equations it satisfies where
//!   it is not applicable (partial credit for the search only), -0.01 per node;
//! * variation: tournament selection (3), two elites kept, crossover (swap a subtree with another
//!   candidate or with an inherited gene), mutation (point, subtree, constant, modulus, transform,
//!   grow, shrink);
//! * acceptance: a candidate becomes a rule ONLY through the B2 check on ALL the signature's revealed
//!   examples (`verify`): never applicable-and-wrong on training or held-out examples, and at least
//!   `K_TRAIN` training and `K_VAL` held-out exact reproductions that no live rule already explains.
//!   An unchanged grammar form is not accepted here (that is discovery's job).
//!
//! Every pass is a pure function of the state folded from the log (examples, rules, earlier passes'
//! elites, the energy spent) and a seed derived from the genesis and the pass number, so it is
//! deterministic and can be replayed from the log (`replay_check`). Each pass writes one `e1_pass`
//! note (best fitness of every generation, evaluations, seed, elites, accepted rule ids); accepted
//! rules are ordinary `rule` notes tagged `via_evolution` whose revision cites the examples and the
//! pass note. The search reads only the being's own examples and rules: no hidden world parameter.

use super::super::discover::{self, forms, is_held_out, predict_x, transformed, Example, Form, Found, Rule, Xf, XFORMS};
use super::super::run_loop::{Engine, NewRule};
use super::super::state::{tag, BeingState, SOURCE};
use super::super::world::iso_at;
use super::ctl::E1Setup;
use super::genome::{self, Code, ADD, MAX_MOD, MUL, N, P, SUB};
use crate::versioned::{event_id, EventRow, NewEvent};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};

/// Evaluations (one candidate on one revealed example) one energy unit buys (author-set exchange rate).
pub const EVALS_PER_ENERGY: u64 = 1000;
pub const PASS_TAG: &str = "e1_pass";
pub const VIA_TAG: &str = "via_evolution";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvoCfg {
    /// Budget, per mille of the energy spent so far (in `EVALS_PER_ENERGY` evaluations per unit).
    pub pm: u32,
    /// Steps between passes.
    pub every: u64,
    pub pop: usize,
    /// Most generations of one pass.
    pub gens: usize,
    /// Most evaluations of one pass.
    pub max_pass_evals: u64,
    /// A signature is a target when at least this many of its recent training examples are unexplained.
    pub min_unexplained: usize,
    /// Recent examples of a signature looked at for the target choice and the fitness sample.
    pub window: usize,
    pub sample_unexplained: usize,
    pub sample_explained: usize,
    pub p_cross: f64,
    /// Share of crossovers with an inherited gene (a verified rule or a grammar form) instead of a candidate.
    pub p_inherit: f64,
    pub elites_logged: usize,
    /// Most candidates checked against all examples per pass.
    pub max_verify: usize,
}

pub const DEFAULT_EVO: EvoCfg = EvoCfg { pm: 20, every: 250, pop: 48, gens: 24, max_pass_evals: 60_000, min_unexplained: 4, window: 400, sample_unexplained: 32, sample_explained: 8, p_cross: 0.5, p_inherit: 0.3, elites_logged: 4, max_verify: 3 };

/// What the log says about E1 (folded): the setup, passes, evaluations spent, the latest elites per signature.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct E1State {
    pub setup: Option<E1Setup>,
    pub passes: u64,
    pub evals: u64,
    pub elites: BTreeMap<u8, Vec<(Xf, Code)>>,
    /// Candidates the check found applicable-and-wrong on a revealed example (per signature): never checked again.
    pub wrong: BTreeMap<u8, Vec<(Xf, Code)>>,
}

fn elite_tag(v: &[(Xf, Code)]) -> String {
    v.iter().map(|(x, g)| format!("{}/{}", x.code(), g.code_str())).collect::<Vec<_>>().join(";")
}

fn elites_of(s: &str) -> Vec<(Xf, Code)> {
    s.split(';').filter_map(|p| {
        let (x, g) = p.split_once('/')?;
        Some((Xf::parse(x)?, Code::parse(g)?))
    }).collect()
}

impl E1State {
    /// Fold an `e1_pass` note (tags joined by commas).
    pub fn fold_pass(&mut self, tags: &str) {
        let (Some(sig), Some(evals)) = (tag(tags, "sig").and_then(|v| v.parse::<u8>().ok()), tag(tags, "evals").and_then(|v| v.parse::<u64>().ok())) else { return };
        self.passes += 1;
        self.evals += evals;
        let el = elites_of(tag(tags, "elites").unwrap_or(""));
        if !el.is_empty() {
            self.elites.insert(sig, el);
        }
        let w = self.wrong.entry(sig).or_default();
        for x in elites_of(tag(tags, "wrong").unwrap_or("")) {
            if !w.contains(&x) {
                w.push(x);
            }
        }
    }
    pub fn fold_setup(&mut self, tags: &str) {
        if self.setup.is_none() {
            self.setup = E1Setup::from_tags(tags);
        }
    }
}

// ---- deterministic randomness ----

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
}

/// The seed of pass `pass` of the being with this genesis.
pub fn pass_seed(world_seed: u64, loop_seed: u64, pass: u64) -> u64 {
    let mut r = Rng::new(world_seed.wrapping_mul(0x100_0000_01b3) ^ loop_seed.rotate_left(32) ^ 0xe1);
    let mut s = r.next();
    for _ in 0..(pass % 7) {
        s ^= r.next();
    }
    s ^ pass.wrapping_mul(0xd6e8_feb8_6659_fd93)
}

// ---- variation ----

fn rand_terminal(r: &mut Rng) -> u8 {
    match r.below(100) {
        0..=24 => genome::t(1),
        25..=39 => genome::t(2),
        40..=49 => genome::t(3),
        50..=64 => N,
        65..=84 => P,
        _ => {
            if r.chance(0.7) {
                genome::c(r.below(7) as i64 - 3)
            } else {
                genome::c(r.below(33) as i64 - 16)
            }
        }
    }
}

fn rand_tree(r: &mut Rng, depth: usize, out: &mut Vec<u8>) {
    if depth == 0 || r.chance(0.35) {
        out.push(rand_terminal(r));
        return;
    }
    out.push(match r.below(100) {
        0..=34 => ADD,
        35..=59 => SUB,
        _ => MUL,
    });
    rand_tree(r, depth - 1, out);
    rand_tree(r, depth - 1, out);
}

fn rand_xf(r: &mut Rng) -> Xf {
    if r.chance(0.6) { Xf::ID } else { Xf(r.below(XFORMS.len()) as u8) }
}

/// A candidate: transform, genome, and how it was made (for the rule's rationale).
#[derive(Debug, Clone)]
pub struct Ind {
    pub xf: Xf,
    pub g: Code,
    pub born: String,
}

fn splice(a: &[u8], i: usize, sub: &[u8]) -> Option<Vec<u8>> {
    let e = genome::subtree_end(a, i)?;
    let mut v = a[..i].to_vec();
    v.extend_from_slice(sub);
    v.extend_from_slice(&a[e..]);
    Some(v)
}

/// One mutation (retried until the result is a well-formed genome; the parent otherwise).
fn mutate(r: &mut Rng, x: &Ind, data_max: &[i64]) -> Ind {
    for _ in 0..8 {
        let ops = x.g.ops().to_vec();
        let mut m = x.g.modulus();
        let mut xf = x.xf;
        let i = r.below(ops.len());
        let (new, how): (Option<Vec<u8>>, &str) = match r.below(100) {
            0..=34 => {
                let mut v = ops.clone();
                v[i] = if genome::arity(v[i]) == Some(2) { [ADD, SUB, MUL][r.below(3)] } else { rand_terminal(r) };
                (Some(v), "point mutation")
            }
            35..=54 => {
                let mut sub = vec![];
                rand_tree(r, 2, &mut sub);
                (splice(&ops, i, &sub), "subtree mutation")
            }
            55..=64 => {
                let cs: Vec<usize> = (0..ops.len()).filter(|&j| genome::const_of(ops[j]).is_some()).collect();
                if cs.is_empty() {
                    continue;
                }
                let j = cs[r.below(cs.len())];
                let v0 = genome::const_of(ops[j]).unwrap_or(0);
                let mut v = ops.clone();
                v[j] = genome::c(if r.chance(0.7) { v0 + if r.chance(0.5) { 1 } else { -1 } } else { r.below(33) as i64 - 16 });
                (Some(v), "constant mutation")
            }
            65..=74 => {
                m = if m == 0 {
                    if r.chance(0.5) && !data_max.is_empty() {
                        // guided by the being's own examples: just above the largest term of one of them
                        (data_max[r.below(data_max.len())] + 1 + r.below(8) as i64).clamp(2, MAX_MOD as i64) as u8
                    } else {
                        2 + r.below(MAX_MOD as usize - 1) as u8
                    }
                } else if r.chance(0.4) {
                    0
                } else if r.chance(0.5) {
                    (m as i64 + r.below(9) as i64 - 4).clamp(2, MAX_MOD as i64) as u8
                } else {
                    2 + r.below(MAX_MOD as usize - 1) as u8
                };
                (Some(ops.clone()), "modulus mutation")
            }
            75..=84 => {
                xf = rand_xf(r);
                (Some(ops.clone()), "transform mutation")
            }
            85..=92 => {
                let mut v = vec![[ADD, SUB, MUL][r.below(3)]];
                let leaf = rand_terminal(r);
                if r.chance(0.5) {
                    v.extend_from_slice(&ops);
                    v.push(leaf);
                } else {
                    v.push(leaf);
                    v.extend_from_slice(&ops);
                }
                (Some(v), "grow mutation")
            }
            _ => {
                let e = genome::subtree_end(&ops, i);
                (e.map(|e| ops[i..e].to_vec()), "shrink mutation")
            }
        };
        if let Some(g) = new.and_then(|v| Code::new(m, &v)) {
            if g != x.g || xf != x.xf {
                return Ind { xf, g, born: how.into() };
            }
        }
    }
    x.clone()
}

fn crossover(r: &mut Rng, a: &Ind, b: &Ind, with: &str) -> Option<Ind> {
    let (oa, ob) = (a.g.ops(), b.g.ops());
    let i = r.below(oa.len());
    let j = r.below(ob.len());
    let sub = &ob[j..genome::subtree_end(ob, j)?];
    let v = splice(oa, i, sub)?;
    let m = if r.chance(0.5) { a.g.modulus() } else { b.g.modulus() };
    let xf = if r.chance(0.5) { a.xf } else { b.xf };
    Code::new(m, &v).map(|g| Ind { xf, g, born: format!("crossover with {with}") })
}

// ---- fitness and acceptance ----

#[derive(Debug, Clone, Copy, Default)]
struct Fit {
    f: f64,
    right: u32,
    wrong: u32,
}

fn fitness(x: &Ind, unexpl: &[&Example], expl: &[&Example], evals: &mut u64) -> Fit {
    let form = Form::Gp(x.g);
    let mut ft = Fit::default();
    for e in unexpl {
        *evals += 1;
        match predict_x(x.xf, &form, &e.terms) {
            Some(v) if v == e.next => {
                ft.f += 1.0;
                ft.right += 1;
            }
            Some(_) => {
                ft.f -= 3.0;
                ft.wrong += 1;
            }
            None => {
                if let Some(s) = transformed(x.xf, &e.terms) {
                    ft.f += 0.5 * genome::partial(&x.g, &s);
                }
            }
        }
    }
    for e in expl {
        *evals += 1;
        if matches!(predict_x(x.xf, &form, &e.terms), Some(v) if v != e.next) {
            ft.f -= 3.0;
            ft.wrong += 1;
        }
    }
    ft.f -= 0.01 * x.g.len() as f64 + 0.01 * x.xf.cost() as f64;
    ft
}

fn explained(rules: &[Rule], e: &Example, evals: &mut u64) -> bool {
    rules.iter().filter(|r| !r.retired && r.sig == e.sig).any(|r| {
        *evals += 1;
        predict_x(r.xf, &r.form, &e.terms) == Some(e.next)
    })
}

/// The B2 acceptance check for an evolved candidate on ALL revealed examples of the signature:
/// never applicable-and-wrong; at least `K_TRAIN` training and `K_VAL` held-out exact reproductions
/// that no live rule already explains. Returns the `Found` (cited examples: new support first), or
/// `Err(true)` if it is applicable-and-wrong on some example, `Err(false)` if the support is too small.
pub fn verify(examples: &[Example], rules: &[Rule], sig: u8, xf: Xf, g: Code, evals: &mut u64) -> Result<Found, bool> {
    let form = Form::Gp(g);
    let ex: Vec<&Example> = examples.iter().filter(|e| e.sig == sig).collect();
    let (mut tr, mut va, mut trn, mut van) = (0u32, 0u32, 0u32, 0u32);
    let (mut cite_new, mut cite_old) = (vec![], vec![]);
    for (i, e) in ex.iter().enumerate() {
        *evals += 1;
        match predict_x(xf, &form, &e.terms) {
            None => {}
            Some(v) if v == e.next => {
                let held = is_held_out(i);
                let new = !explained(rules, e, evals);
                if held { va += 1 } else { tr += 1 }
                if new {
                    if held { van += 1 } else { trn += 1 }
                    cite_new.push(e.event_id.clone());
                } else {
                    cite_old.push(e.event_id.clone());
                }
            }
            Some(_) => return Err(true),
        }
    }
    if trn < discover::K_TRAIN || van < discover::K_VAL {
        return Err(false);
    }
    let cite: Vec<String> = cite_new.into_iter().chain(cite_old).take(discover::MAX_CITE).collect();
    Ok(Found { sig, xf, form, train_ok: tr, val_ok: va, new_train: trn, new_val: van, cite })
}

/// What one pass did (logged as the `e1_pass` note).
#[derive(Debug, Clone, Default)]
pub struct PassOut {
    pub pass: u64,
    pub sig: u8,
    pub seed: u64,
    pub gens: usize,
    pub evals: u64,
    pub n_unexplained: usize,
    pub n_sample: usize,
    pub gen_best: Vec<f64>,
    pub best: Option<(Xf, Code, f64, u32)>,
    pub elites: Vec<(Xf, Code)>,
    /// Verified candidates: (found, how the candidate was made, generation).
    pub accepted: Vec<(Found, String, usize)>,
    pub verified_tried: usize,
    pub rejected: usize,
    /// Candidates the check found applicable-and-wrong in this pass.
    pub wrong: Vec<(Xf, Code)>,
}

/// The inherited genes: the being's live rules (any signature) and the grammar forms, as genomes.
fn genes(rules: &[Rule]) -> Vec<Ind> {
    let mut v: Vec<Ind> = vec![];
    for r in rules.iter().filter(|r| !r.retired) {
        if let Some(g) = genome::of_form(&r.form) {
            if !v.iter().any(|x| x.xf == r.xf && x.g == g) {
                v.push(Ind { xf: r.xf, g, born: format!("inherited from rule {}", r.id) });
            }
        }
    }
    for f in forms() {
        if let Some(g) = genome::of_form(&f) {
            if !v.iter().any(|x| x.xf == Xf::ID && x.g == g) {
                v.push(Ind { xf: Xf::ID, g, born: format!("grammar form {}", f.code()) });
            }
        }
    }
    v
}

/// The targets: signatures with at least `min_unexplained` unexplained recent training examples, and
/// for each the recent (index in signature, example, explained) list. Evaluations are counted.
fn targets<'e>(st: &'e BeingState, cfg: &EvoCfg, evals: &mut u64) -> Vec<(u8, Vec<(usize, &'e Example, bool)>)> {
    let mut by: BTreeMap<u8, Vec<&Example>> = BTreeMap::new();
    for e in &st.examples {
        by.entry(e.sig).or_default().push(e);
    }
    let mut out = vec![];
    for (sig, ex) in by {
        let from = ex.len().saturating_sub(cfg.window);
        let recent: Vec<(usize, &Example, bool)> = ex.iter().enumerate().skip(from).map(|(i, e)| (i, *e, explained(&st.rules, e, evals))).collect();
        let nu = recent.iter().filter(|x| !is_held_out(x.0) && !x.2).count();
        if nu >= cfg.min_unexplained {
            out.push((sig, recent));
        }
    }
    out
}

/// Evaluations the budget still allows now.
pub fn available(st: &BeingState, cfg: &EvoCfg) -> u64 {
    let allowed = (st.total_cost.max(0) as u64).saturating_mul(cfg.pm as u64).saturating_mul(EVALS_PER_ENERGY) / 1000;
    allowed.saturating_sub(st.e1.evals)
}

/// One evolution pass on the folded state (pure: the same state and seed give the same pass).
pub fn run_pass(st: &BeingState, cfg: &EvoCfg) -> Option<PassOut> {
    let budget = available(st, cfg).min(cfg.max_pass_evals);
    if budget < (cfg.pop as u64) * 4 {
        return None;
    }
    let g = st.genesis.as_ref()?;
    let pass = st.e1.passes;
    let seed = pass_seed(g.world_seed, g.loop_seed, pass);
    let mut r = Rng::new(seed);
    let mut evals = 0u64;
    let tg = targets(st, cfg, &mut evals);
    if tg.is_empty() {
        return None;
    }
    let (sig, recent) = &tg[(pass as usize) % tg.len()];
    let sig = *sig;
    let unexpl: Vec<&Example> = recent.iter().rev().filter(|x| !is_held_out(x.0) && !x.2).take(cfg.sample_unexplained).map(|x| x.1).collect();
    let expl: Vec<&Example> = recent.iter().rev().filter(|x| !is_held_out(x.0) && x.2).take(cfg.sample_explained).map(|x| x.1).collect();
    let data_max: Vec<i64> = unexpl.iter().filter_map(|e| e.terms.iter().max().copied()).filter(|m| *m >= 1).collect();
    let pool = genes(&st.rules);
    let mut popn: Vec<Ind> = vec![];
    for (xf, g) in st.e1.elites.get(&sig).cloned().unwrap_or_default() {
        popn.push(Ind { xf, g, born: "elite of the previous pass".into() });
    }
    for x in &pool {
        if popn.len() >= cfg.pop / 2 {
            break;
        }
        popn.push(x.clone());
    }
    while popn.len() < cfg.pop {
        let mut v = vec![];
        rand_tree(&mut r, 3, &mut v);
        if let Some(g) = Code::new(0, &v) {
            popn.push(Ind { xf: rand_xf(&mut r), g, born: "random".into() });
        }
    }
    let grammar: Vec<(Xf, Code)> = forms().iter().filter_map(genome::of_form).map(|g| (Xf::ID, g)).collect();
    let mut cache: HashMap<(u8, Code), Fit> = HashMap::new();
    let mut hof: Vec<(Ind, Fit, usize)> = vec![];
    let mut out = PassOut { pass, sig, seed, n_unexplained: recent.iter().filter(|x| !is_held_out(x.0) && !x.2).count(), n_sample: unexpl.len() + expl.len(), ..Default::default() };
    let per = (unexpl.len() + expl.len()) as u64;
    let mut scored: Vec<(Ind, Fit)> = vec![];
    for gen in 0..cfg.gens {
        if evals + per * cfg.pop as u64 > budget && gen > 0 {
            break;
        }
        scored = popn
            .iter()
            .map(|x| {
                let ft = *cache.entry((x.xf.0, x.g)).or_insert_with(|| fitness(x, &unexpl, &expl, &mut evals));
                (x.clone(), ft)
            })
            .collect();
        scored.sort_by(|a, b| b.1.f.total_cmp(&a.1.f).then_with(|| (a.0.g, a.0.xf.0).cmp(&(b.0.g, b.0.xf.0))));
        out.gens = gen + 1;
        out.gen_best.push(scored[0].1.f);
        for (x, ft) in scored.iter().take(4) {
            if ft.right as usize >= discover::K_TRAIN as usize && ft.wrong == 0 && !hof.iter().any(|h| h.0.g == x.g && h.0.xf == x.xf) {
                hof.push((x.clone(), *ft, gen));
            }
        }
        // next generation
        let tour = |r: &mut Rng, s: &[(Ind, Fit)]| -> Ind {
            let mut best = r.below(s.len());
            for _ in 0..2 {
                let j = r.below(s.len());
                if s[j].1.f > s[best].1.f {
                    best = j;
                }
            }
            s[best].0.clone()
        };
        let mut next: Vec<Ind> = scored.iter().take(2).map(|x| x.0.clone()).collect();
        while next.len() < cfg.pop {
            let a = tour(&mut r, &scored);
            let mut child = if r.chance(cfg.p_cross) {
                let (b, with) = if r.chance(cfg.p_inherit) {
                    let b = pool[r.below(pool.len())].clone();
                    let w = format!("the inherited gene ({})", b.born);
                    (b, w)
                } else {
                    (tour(&mut r, &scored), "another candidate".to_string())
                };
                crossover(&mut r, &a, &b, &with).unwrap_or(a.clone())
            } else {
                a.clone()
            };
            if r.chance(0.8) || (child.g == a.g && child.xf == a.xf) {
                child = mutate(&mut r, &child, &data_max);
            }
            next.push(child);
        }
        popn = next;
    }
    if let Some((x, ft)) = scored.first() {
        out.best = Some((x.xf, x.g, ft.f, ft.right));
    }
    let mut el: Vec<(Xf, Code)> = vec![];
    for (x, _) in &scored {
        if el.len() >= cfg.elites_logged {
            break;
        }
        if !el.contains(&(x.xf, x.g)) {
            el.push((x.xf, x.g));
        }
    }
    out.elites = el;
    // acceptance: the best verified candidates through the B2 check on all examples of the signature
    hof.sort_by(|a, b| b.1.f.total_cmp(&a.1.f).then_with(|| (a.0.g, a.0.xf.0).cmp(&(b.0.g, b.0.xf.0))));
    let rule_ids: Vec<String> = st.rules.iter().map(|r| r.id.clone()).collect();
    let mut covered: Vec<String> = vec![];
    for (x, _, gen) in hof {
        if out.verified_tried >= cfg.max_verify {
            break;
        }
        if x.xf.is_id() && grammar.iter().any(|q| q.1 == x.g) {
            continue;
        }
        if st.e1.wrong.get(&sig).map_or(false, |w| w.contains(&(x.xf, x.g))) {
            continue;
        }
        let id = discover::rule_id_x(sig, x.xf, &Form::Gp(x.g));
        if rule_ids.contains(&id) || out.accepted.iter().any(|a| discover::rule_id_x(sig, a.0.xf, &a.0.form) == id) {
            continue;
        }
        out.verified_tried += 1;
        match verify(&st.examples, &st.rules, sig, x.xf, x.g, &mut evals) {
            Ok(f) if !f.cite.iter().all(|c| covered.contains(c)) => {
                covered.extend(f.cite.iter().cloned());
                out.accepted.push((f, x.born.clone(), gen));
            }
            Ok(_) => {}
            Err(wrong) => {
                out.rejected += 1;
                if wrong {
                    out.wrong.push((x.xf, x.g));
                }
            }
        }
    }
    out.evals = evals;
    Some(out)
}

impl PassOut {
    /// The `e1_pass` note.
    pub fn event(&self, clock: i64, step: u64, ids: &[String]) -> NewEvent {
        let (d, k) = discover::sig_split(self.sig);
        let best = self.best.map_or("none".to_string(), |(x, g, f, right)| format!("{}{} (fitness {f:.2}, reproduced {right} of {} unexplained sampled examples)", if x.is_id() { String::new() } else { format!("{} then ", x.words()) }, g.dsl(), self.n_sample));
        let curve: Vec<String> = self.gen_best.iter().map(|f| format!("{f:.2}")).collect();
        let text = format!(
            "evolution pass {} (Track E1) on {} difficulty {} at step {step}: {} generations, {} evaluations on {} of its own revealed training examples ({} recent ones not explained by a live rule), seed {}; best fitness per generation {}; best candidate {best}; {} verified against all examples, {} rejected; accepted: {}",
            self.pass, d.name(), k, self.gens, self.evals, self.n_sample, self.n_unexplained, self.seed, curve.join(" "), self.verified_tried, self.rejected, if ids.is_empty() { "none".to_string() } else { ids.join(" ") }
        );
        let mut tags = vec![
            PASS_TAG.to_string(),
            format!("pass:{}", self.pass),
            format!("sig:{}", self.sig),
            format!("seed:{}", self.seed),
            format!("evals:{}", self.evals),
            format!("gens:{}", self.gens),
            format!("at_step:{step}"),
            format!("curve:{}", curve.join(";")),
        ];
        if !self.elites.is_empty() {
            tags.push(format!("elites:{}", elite_tag(&self.elites)));
        }
        if !ids.is_empty() {
            tags.push(format!("accepted:{}", ids.join(";")));
        }
        if !self.wrong.is_empty() {
            tags.push(format!("wrong:{}", elite_tag(&self.wrong)));
        }
        NewEvent { source: SOURCE.into(), occurred_at: iso_at(clock), kind: "note".into(), text, tags, redacted: false }
    }
}

/// The engine's evolution: settings and what it did in this engine's life (measurement).
#[derive(Debug, Clone, Default)]
pub struct Evo {
    pub cfg: Option<EvoCfg>,
    /// Rules accepted by evolution: (step, rule).
    pub accepted: Vec<(u64, Rule)>,
    pub passes: u64,
    pub evals: u64,
    pub nanos: u128,
    pub rejected: u64,
    pub verified: u64,
}

impl<'a> Engine<'a> {
    /// Turn evolution on (measurement and the logged run).
    pub fn enable_evolution(&mut self, cfg: EvoCfg) {
        self.evo = Some(Evo { cfg: Some(cfg), ..Default::default() });
    }

    /// One evolution pass if one is due (every `every` steps, budget left): the pass note first, then
    /// the accepted rules (their events and revisions). Nothing when evolution is off.
    pub fn evolve_pass(&mut self) -> (Vec<NewEvent>, Vec<NewRule>) {
        let Some(cfg) = self.evo.as_ref().and_then(|e| e.cfg) else { return (vec![], vec![]) };
        if cfg.every == 0 || self.state.step % cfg.every != 0 {
            return (vec![], vec![]);
        }
        let t0 = std::time::Instant::now();
        let Some(out) = run_pass(&self.state, &cfg) else {
            if let Some(e) = self.evo.as_mut() {
                e.nanos += t0.elapsed().as_nanos();
            }
            return (vec![], vec![]);
        };
        let ids: Vec<String> = out.accepted.iter().map(|a| discover::rule_id_x(a.0.sig, a.0.xf, &a.0.form)).collect();
        let ev = out.event(self.state.clock, self.state.step, &ids);
        let note_id = event_id(&ev.source, &ev.occurred_at, &ev.text);
        self.state.e1.fold_pass(&ev.tags.join(","));
        let mut rules = vec![];
        for (f, born, gen) in &out.accepted {
            let Some(mut nr) = self.accept_found(f, None) else { continue };
            nr.event.tags.push(VIA_TAG.into());
            nr.event.tags.push(format!("e1_pass:{}", out.pass));
            nr.revision.author = "rule:being-evolution".into();
            nr.revision.caused_by.push(note_id.clone());
            let mut rat: serde_json::Value = serde_json::from_str(&nr.revision.rationale).unwrap_or(json!({}));
            rat["grammar"] = json!("author-defined expression language of Track E1: lags 1-3, the term index, integer constants -16..16, coefficients fitted per task (affine), +, -, *, optional modulus 2..251, behind one of the B5 transforms; searched by mutation and crossover");
            rat["evolution"] = json!({
                "track": "E1", "pass": out.pass, "pass_event": note_id, "seed": out.seed, "generation": gen, "made_by": born,
                "pass_evaluations": out.evals, "genome": f.form.code(), "transform": f.xf.code(),
                "check": format!("reproduced {} training and {} held-out revealed examples exactly ({} and {} not explained by a live rule) and was never applicable-and-wrong on any of the {} revealed examples of its signature (B2 support {} + {})", f.train_ok, f.val_ok, f.new_train, f.new_val, self.state.examples.iter().filter(|e| e.sig == f.sig).count(), discover::K_TRAIN, discover::K_VAL),
            });
            nr.revision.rationale = rat.to_string();
            if let Some(e) = self.evo.as_mut() {
                e.accepted.push((self.state.step, nr.rule.clone()));
            }
            rules.push(nr);
        }
        if let Some(e) = self.evo.as_mut() {
            e.passes += 1;
            e.evals += out.evals;
            e.rejected += out.rejected as u64;
            e.verified += out.verified_tried as u64;
            e.nanos += t0.elapsed().as_nanos();
        }
        (vec![ev], rules)
    }
}

/// Replay every `e1_pass` note of a log: fold the events before it, run the pass again, and compare
/// the note. Returns (passes checked, passes that differ).
pub fn replay_check(rows: &[EventRow], cfg: &EvoCfg) -> (usize, usize) {
    let (mut n, mut bad) = (0, 0);
    let mut st = BeingState::default();
    for e in rows {
        if e.source == SOURCE && e.kind == "note" && e.tags.split(',').any(|t| t == PASS_TAG) {
            n += 1;
            let step: u64 = tag(&e.tags, "at_step").and_then(|v| v.parse().ok()).unwrap_or(0);
            let same = run_pass(&st, cfg).map_or(false, |o| {
                let ids: Vec<String> = o.accepted.iter().map(|a| discover::rule_id_x(a.0.sig, a.0.xf, &a.0.form)).collect();
                let ev = o.event(st.clock, step, &ids);
                ev.text == e.text && ev.tags.join(",") == e.tags
            });
            if !same {
                bad += 1;
            }
        }
        st.fold_event(e);
    }
    (n, bad)
}
