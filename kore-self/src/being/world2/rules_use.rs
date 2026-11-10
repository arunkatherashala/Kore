//! Track B7 (opt-in): using the rules the being discovers. Three switches, each off by default; with all
//! of them off every log, state and measurement is byte-identical to the code before B7 (a test pins
//! golden digests).
//!
//! * **A, rule estimate** (`rule_est`). Without it, accepting a rule only resets the `discovered` cell of
//!   its (family, difficulty) to the pooled prior of the `discovered` strategy, which mostly reflects
//!   attempts where no rule applied (then `discovered` behaves like `careful`). So the planner sees a
//!   cold cell (measured: mean estimate 0.33 to 0.55 against 0.71 to 0.87 for the best other strategy,
//!   while the rule was right on 99.8% of the offered tasks it covered). With A, when a live rule covers
//!   an offer's (base family, difficulty), the estimate of `discovered` for that offer is a Beta
//!   posterior whose prior comes from the rule's own verified record: mean `(S + 1) / (S + W + 2)` with
//!   `S` the training plus held-out examples it reproduced and `W` the examples it missed (0 under the
//!   B2 guard), weight `min(S + W + 2, rule_k)`; updated by the reported outcomes of the attempts that
//!   this rule actually answered on this (family, difficulty). The cost ratio stays the model's.
//! * **B, noise-robust acceptance** (`robust_milli` > 0). The B2 guard rejects a form for good once it
//!   misses one revealed answer, and with feedback noise revealed answers are sometimes wrong. With B a
//!   plain grammar form is also accepted when it misses some examples, if the 95% Wilson lower bound of
//!   its share right among the examples where it applies is at least `robust_milli / 1000`, with the same
//!   minimum new support (training and held out) as B2. A form that misses nothing is judged exactly as
//!   by B2. Composed candidates keep the B2 guard.
//! * **C, fitted constant** (`fit_const`). A formula family's answer is `g x` (weight) or `g x^2 / 2`
//!   (fall), with the kind and `x` visible in the task. With C the outcome of a formula task also logs
//!   `x` and the kind, and a discovery pass fits one integer `g` per family from the revealed answers:
//!   candidates are the values implied by single examples (1..=1000), smallest first, accepted under the
//!   same guard as a sequence rule (B2's, or B's with B on), split every third example held out.
//!   `discovered` on a formula task then answers with the latest accepted constant of its family.
//!
//! Only observable things are used: offer features, the being's own log (revealed answers as TOLD,
//! with noise), and its accepted rules. No hidden parameter is read; a test scans this file.

use super::engine::State2;
use super::learner::Offer2;
use super::solve::sig2;
use super::world::{bfs, FType, FormulaKind, TaskView2, TYPE_CYCLE};
use crate::being::discover::{forms, is_held_out, predict_x, Example, Form, Found, Rule, Xf, MAX_CITE};
use crate::being::model::Estimate;
use crate::versioned::sha256_hex;
use std::collections::{BTreeMap, HashMap};

/// Default cap on the prior weight of a rule estimate (chosen on the tuning seeds 101..120).
pub const DEFAULT_RULE_K: u32 = 8;
/// Smallest and largest constant the fit considers.
pub const G_MIN: i64 = 1;
pub const G_MAX: i64 = 1000;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseCfg {
    pub rule_est: bool,
    /// A2: `discovered` is a candidate only where a rule covers the offer (elsewhere it is `careful` under another name).
    pub rule_only: bool,
    /// 0: the B2 guard; otherwise the Wilson lower bound threshold in per mille.
    pub robust_milli: u32,
    pub fit_const: bool,
    /// C2: fit the move rule (4 or 8 neighbours) of each grid family from the revealed path lengths.
    pub fit_grid: bool,
    pub rule_k: u32,
}

impl Default for UseCfg {
    fn default() -> Self {
        UseCfg { rule_est: false, rule_only: false, robust_milli: 0, fit_const: false, fit_grid: false, rule_k: DEFAULT_RULE_K }
    }
}

impl UseCfg {
    pub fn any(&self) -> bool {
        self.rule_est || self.rule_only || self.robust_milli > 0 || self.fit_const || self.fit_grid
    }
    /// The genesis tag (only written when a switch is on, so a default being's log is unchanged).
    pub fn tag(&self) -> String {
        format!("w2_use:{}/{}/{}/{}/{}/{}", self.rule_est as u8, self.rule_only as u8, self.robust_milli, self.fit_const as u8, self.fit_grid as u8, self.rule_k)
    }
    pub fn parse(v: &str) -> Option<UseCfg> {
        let p: Vec<&str> = v.split('/').collect();
        if p.len() != 6 {
            return None;
        }
        Some(UseCfg { rule_est: p[0] == "1", rule_only: p[1] == "1", robust_milli: p[2].parse().ok()?, fit_const: p[3] == "1", fit_grid: p[4] == "1", rule_k: p[5].parse().ok()? })
    }
    pub fn words(&self) -> String {
        format!(
            "; rules in use: rule estimate {}, discovered only where a rule applies {}, robust acceptance {}, fitted constants {}, fitted grid move rules {}, rule prior weight cap {}",
            if self.rule_est { "on" } else { "off" },
            if self.rule_only { "on" } else { "off" },
            if self.robust_milli > 0 { format!("at Wilson lower bound {:.3}", self.robust_milli as f64 / 1000.0) } else { "off".into() },
            if self.fit_const { "on" } else { "off" },
            if self.fit_grid { "on" } else { "off" },
            self.rule_k
        )
    }
}

/// 95% Wilson lower bound of `k` successes in `n`.
pub fn wilson_lb(k: u32, n: u32) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let (z, n, p) = (1.96f64, n as f64, k as f64 / n as f64);
    let z2 = z * z;
    ((p + z2 / (2.0 * n)) - z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt()) / (1.0 + z2 / n)
}

/// The robust guard: `right` reproduced, `wrong` missed among the examples where the rule applies.
pub fn robust_ok(right: u32, wrong: u32, theta_milli: u32) -> bool {
    wrong == 0 || (theta_milli > 0 && wilson_lb(right, right + wrong) >= theta_milli as f64 / 1000.0)
}

// ---- fitted constants (C) ----

/// One revealed example of a formula or grid task: the visible input, the kind (`fall`, `weight` or `grid`),
/// and the answer the being was told is right.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct CExample {
    pub event_id: String,
    pub fam: usize,
    pub kind: String,
    /// formula: [t] or [m]; grid: [rows, cols, cells...]
    pub x: Vec<i64>,
    pub revealed: i64,
}

/// An accepted constant for one family: `g` of a formula family, or the move rule (4 or 8 neighbours) of a grid family.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct ConstRule {
    pub id: String,
    pub fam: usize,
    pub g: i64,
    pub train_ok: u32,
    pub val_ok: u32,
    pub wrong: u32,
    pub step: u64,
    pub event_id: String,
}

pub fn const_rule_id(fam: usize, g: i64) -> String {
    sha256_hex(&[b"being-w2-const-v1", &[0x1f], fam.to_string().as_bytes(), &[0x1f], g.to_string().as_bytes()])[..12].to_string()
}

pub fn formula_answer(fall: bool, g: i64, x: i64) -> i64 {
    if fall { g * x * x / 2 } else { x * g }
}

/// The answer of a task of `kind` with input `x` under constant (or move rule) `g`; None if it has none.
pub fn param_answer(kind: &str, g: i64, x: &[i64]) -> Option<i64> {
    match kind {
        "fall" => Some(formula_answer(true, g, *x.first()?)),
        "weight" => Some(formula_answer(false, g, *x.first()?)),
        "grid" if x.len() >= 2 => bfs(x[0] as usize, x[1] as usize, &x[2..], g == 8),
        _ => None,
    }
}

/// The kind word of a task view for the fit (None: not a formula or grid task).
pub fn kind_of(v: &TaskView2) -> Option<&'static str> {
    match v.ftype {
        FType::Formula => Some(if v.formula == Some(FormulaKind::Fall) { "fall" } else { "weight" }),
        FType::Grid => Some("grid"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoundC {
    pub fam: usize,
    pub g: i64,
    pub train_ok: u32,
    pub val_ok: u32,
    pub wrong: u32,
    pub cite: Vec<String>,
}

fn implied_g(e: &CExample) -> Vec<i64> {
    if e.kind == "grid" {
        return vec![4, 8];
    }
    let mut v = vec![];
    let x = e.x.first().copied().unwrap_or(0);
    if x == 0 {
        return v;
    }
    let fall = e.kind == "fall";
    let base = if fall { (2 * e.revealed).div_euclid(x * x) } else { e.revealed.div_euclid(x) };
    for g in [base, base + 1] {
        if (G_MIN..=G_MAX).contains(&g) && formula_answer(fall, g, x) == e.revealed {
            v.push(g);
        }
    }
    v
}

/// The constant search over the being's formula examples (log order): per family, candidates smallest first,
/// accepted when they reproduce at least `k_train` training and `k_val` held-out examples not already explained
/// by an accepted constant, and pass the guard (`theta_milli` 0: never wrong; else `robust_ok`).
pub fn search_const(examples: &[CExample], k_train: u32, k_val: u32, theta_milli: u32) -> Vec<FoundC> {
    let mut by: BTreeMap<usize, Vec<&CExample>> = BTreeMap::new();
    for e in examples {
        by.entry(e.fam).or_default().push(e);
    }
    let mut out = vec![];
    for (fam, ex) in by {
        let mut gs: Vec<i64> = ex.iter().flat_map(|e| implied_g(e)).collect();
        gs.sort_unstable();
        gs.dedup();
        let mut covered = vec![false; ex.len()];
        for g in gs {
            let ok: Vec<bool> = ex.iter().map(|e| param_answer(&e.kind, g, &e.x) == Some(e.revealed)).collect();
            let (mut tr, mut va, mut tr_new, mut va_new, mut wrong) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for (i, r) in ok.iter().enumerate() {
                if !*r {
                    wrong += 1;
                    continue;
                }
                let held = is_held_out(i);
                if held { va += 1 } else { tr += 1 }
                if !covered[i] {
                    if held { va_new += 1 } else { tr_new += 1 }
                }
            }
            if tr_new >= k_train && va_new >= k_val && robust_ok(tr + va, wrong, theta_milli) {
                let mut cite = vec![];
                for pass_new in [true, false] {
                    for (i, r) in ok.iter().enumerate() {
                        if *r && (!covered[i]) == pass_new && cite.len() < MAX_CITE {
                            cite.push(ex[i].event_id.clone());
                        }
                    }
                }
                for (i, r) in ok.iter().enumerate() {
                    if *r {
                        covered[i] = true;
                    }
                }
                out.push(FoundC { fam, g, train_ok: tr, val_ok: va, wrong, cite });
            }
        }
    }
    out
}

/// The constant `discovered` uses for family `fam`: the latest accepted one.
pub fn const_for(consts: &[ConstRule], fam: usize) -> Option<&ConstRule> {
    consts.iter().rev().find(|c| c.fam == fam)
}

// ---- noise-robust search (B) ----

#[derive(Debug, Clone, Default)]
struct RCand {
    upto: usize,
    right: Vec<bool>,
    tr_ok: u32,
    va_ok: u32,
    wrong: u32,
}

#[derive(Debug, Clone, Default)]
struct RSig {
    idx: Vec<usize>,
    cands: HashMap<Form, RCand>,
}

/// Incremental state of the robust search (the example list only grows at its end; it starts over otherwise).
#[derive(Debug, Clone, Default)]
pub struct RobustCache {
    seen: usize,
    first_id: String,
    sigs: BTreeMap<u8, RSig>,
}

/// A form accepted by the robust search, with the number of examples it missed.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundR {
    pub found: Found,
    pub wrong: u32,
}

impl RobustCache {
    fn sync(&mut self, examples: &[Example]) {
        let extends = examples.len() >= self.seen && (self.seen == 0 || examples[0].event_id == self.first_id);
        if !extends {
            *self = RobustCache::default();
        }
        for (i, e) in examples.iter().enumerate().skip(self.seen) {
            self.sigs.entry(e.sig).or_default().idx.push(i);
        }
        if let Some(f) = examples.first() {
            self.first_id = f.event_id.clone();
        }
        self.seen = examples.len();
    }

    /// The plain grammar (B2's forms, B2's order and support rule) with the robust guard.
    pub fn search(&mut self, examples: &[Example], k_train: u32, k_val: u32, theta_milli: u32) -> Vec<FoundR> {
        self.sync(examples);
        let fs = forms();
        let mut out = vec![];
        for (&sig, s) in self.sigs.iter_mut() {
            let n = s.idx.len();
            let mut covered = vec![false; n];
            for form in &fs {
                let c = s.cands.entry(*form).or_default();
                while c.upto < n {
                    let k = c.upto;
                    let e = &examples[s.idx[k]];
                    let r = match predict_x(Xf::ID, form, &e.terms) {
                        None => false,
                        Some(v) if v == e.next => {
                            if is_held_out(k) { c.va_ok += 1 } else { c.tr_ok += 1 }
                            true
                        }
                        Some(_) => {
                            c.wrong += 1;
                            false
                        }
                    };
                    c.right.push(r);
                    c.upto += 1;
                }
                let (mut tr_new, mut va_new) = (0u32, 0u32);
                for (i, r) in c.right.iter().enumerate() {
                    if *r && !covered[i] {
                        if is_held_out(i) { va_new += 1 } else { tr_new += 1 }
                    }
                }
                if tr_new >= k_train && va_new >= k_val && robust_ok(c.tr_ok + c.va_ok, c.wrong, theta_milli) {
                    let mut cite = vec![];
                    for pass_new in [true, false] {
                        for (i, r) in c.right.iter().enumerate() {
                            if *r && (!covered[i]) == pass_new && cite.len() < MAX_CITE {
                                cite.push(examples[s.idx[i]].event_id.clone());
                            }
                        }
                    }
                    for (i, r) in c.right.iter().enumerate() {
                        if *r {
                            covered[i] = true;
                        }
                    }
                    out.push(FoundR { found: Found { sig, xf: Xf::ID, form: *form, train_ok: c.tr_ok, val_ok: c.va_ok, new_train: tr_new, new_val: va_new, cite }, wrong: c.wrong });
                }
            }
        }
        out
    }
}

/// The robust search without a cache (for replay): same answers.
pub fn search_robust(examples: &[Example], theta_milli: u32) -> Vec<FoundR> {
    RobustCache::default().search(examples, crate::being::discover::K_TRAIN, crate::being::discover::K_VAL, theta_milli)
}

// ---- the rule estimate (A) ----

/// The record a rule estimate rests on (logged with every alternative that uses it).
#[derive(Debug, Clone, PartialEq)]
pub struct RuleEst {
    pub rule_id: String,
    pub event_id: String,
    /// Examples reproduced at acceptance (training + held out) and missed.
    pub support: u32,
    pub missed: u32,
    /// Reported outcomes of the attempts this rule answered on this (family, difficulty) since.
    pub post_n: u64,
    pub post_succ: u64,
    pub prior_p: f64,
    pub prior_k: f64,
    pub est: Estimate,
}

/// `seq`/`compose` offers: the (base family, difficulty) whose rules answer them.
pub fn rule_sig(o: &Offer2) -> Option<u8> {
    match o.ftype {
        FType::Seq => Some(sig2(o.fam, o.difficulty)),
        // compose families reuse the rule of the family just before them (the type cycle is public)
        FType::Compose if o.fam >= 1 && TYPE_CYCLE[(o.fam - 1) % TYPE_CYCLE.len()] == FType::Seq => Some(sig2(o.fam - 1, o.difficulty)),
        _ => None,
    }
}

/// The live rule `discover::apply` tries first for a signature (smallest key, then id).
pub fn primary_rule(rules: &[Rule], sig: u8) -> Option<&Rule> {
    rules.iter().filter(|r| r.sig == sig && !r.retired).min_by(|a, b| (a.key(), a.id.as_str()).cmp(&(b.key(), b.id.as_str())))
}

/// The estimate of `discovered` for offer `o` from the record of the rule that covers it (None: no rule covers it).
/// Whether some accepted rule covers offer `o` (what A2 asks before offering `discovered`).
pub fn covered(st: &State2, o: &Offer2) -> bool {
    match o.ftype {
        FType::Formula | FType::Grid => const_for(&st.const_rules, o.fam).is_some(),
        FType::Knap => false,
        _ => rule_sig(o).map_or(false, |sig| primary_rule(&st.rules, sig).is_some()),
    }
}

pub fn rule_estimate(st: &State2, o: &Offer2, rule_k: u32, cell: &Estimate) -> Option<RuleEst> {
    let (id, ev, support, missed) = match o.ftype {
        FType::Knap => return None,
        FType::Formula | FType::Grid => {
            let c = const_for(&st.const_rules, o.fam)?;
            (c.id.clone(), c.event_id.clone(), c.train_ok + c.val_ok, c.wrong)
        }
        _ => {
            let r = primary_rule(&st.rules, rule_sig(o)?)?;
            (r.id.clone(), r.event_id.clone(), r.train_ok + r.val_ok, st.rule_missed.get(&r.id).copied().unwrap_or(0))
        }
    };
    let (post_n, post_succ) = st.rule_post.get(&(o.fam, o.difficulty.clamp(1, 5), id.clone())).copied().unwrap_or((0, 0));
    let prior_p = (support as f64 + 1.0) / ((support + missed) as f64 + 2.0);
    let prior_k = ((support + missed) as f64 + 2.0).min(rule_k.max(1) as f64);
    let a = post_succ as f64 + prior_k * prior_p;
    let b = (post_n - post_succ) as f64 + prior_k * (1.0 - prior_p);
    let p = a / (a + b);
    let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
    let est = Estimate { n: post_n, p, sd, cost_ratio: cell.cost_ratio, group_n: cell.group_n };
    Some(RuleEst { rule_id: id, event_id: ev, support, missed, post_n, post_succ, prior_p, prior_k, est })
}
