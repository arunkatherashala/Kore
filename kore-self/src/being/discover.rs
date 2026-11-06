//! Rule discovery by verified search (Track B, step B2).
//!
//! The being looks at its OWN past attempts at sequence tasks. After every
//! attempt the checker's correct next term is recorded in the log (tags
//! `terms` and `next` of the outcome event), so each logged attempt is one
//! revealed example: a prefix of terms and the exact next term. The search
//! below runs over those examples only. It never sees how tasks are produced,
//! which hidden family a task came from, or any world parameter; a test scans
//! this file for those names.
//!
//! IMPORTANT: the grammar below is written by the author, and so are the
//! sequence families of the simulated world. "Discovery" here means a search
//! inside this small, defined space; it is not open-ended invention.
//!
//! ## Grammar (bounded)
//!
//! ```text
//! rule  := form                      (one rule = one form, for one signature)
//! form  := lin(L, c)                 next = sum_{l in L} a_l * t[-l]  (+ b if c)
//!        | poly(d)                   next = b0 + b1*n + ... + bd*n^d  (n = index of the term, first term n = 0)
//! L     := non-empty subset of {1, 2, 3}      (lags: 1 = previous term)
//! c     := with or without a constant term b
//! d     := 1, 2 or 3
//! ```
//!
//! The coefficients (a_l, b, b_j) are NOT part of the rule: they are fitted
//! per task from the task's own prefix by exact rational Gauss-Jordan
//! elimination (i128 fractions, no floats). A rule is APPLICABLE to a prefix
//! only if (1) the fit is unique, (2) the equations leave at least
//! `MIN_SLACK` (3) more equations than unknowns, all satisfied exactly, (3) the
//! coefficients are small (numerators up to 100, denominators up to 12) and
//! (4) the predicted next term is an integer. That self-check on the prefix
//! is what keeps a rule from being applied where it does not describe the
//! sequence. There are 17 forms (14 linear, 3 polynomial), sorted by size
//! (number of unknowns), then by kind and lags.
//!
//! ## Acceptance (overfit guard)
//!
//! The signature of an example is (domain pattern, difficulty), which the
//! being can observe. Examples of one signature, in log order, are split
//! deterministically: every third one (index 2, 5, 8, ...) is HELD OUT for
//! validation, the rest is training. Forms are tried smallest first. A form
//! is ACCEPTED for a signature only if
//!   * it is never applicable-and-wrong, on training or on held-out examples;
//!   * it reproduces the revealed next term exactly on at least `K_TRAIN`
//!     training examples and `K_VAL` held-out examples that no smaller
//!     accepted form already explains (minimum support, simplicity tie-break:
//!     a larger form that only repeats what a smaller one does is not added).
//! The being stores accepted rules in its log (see `Rule`), never the search
//! internals; running the search again over the same examples gives the same
//! set, so discovering twice is idempotent.

use super::state::tag;
use super::world::Domain;
use crate::versioned::{sha256_hex, EventRow, NewEvent};

pub const MIN_SLACK: usize = 3;
pub const K_TRAIN: u32 = 3;
pub const K_VAL: u32 = 1;
pub const MAX_CITE: usize = 24;
pub const MAX_LAG: usize = 3;
pub const MAX_DEGREE: u8 = 3;
/// Every `VAL_EVERY`-th example of a signature (index % VAL_EVERY == VAL_EVERY - 1) is held out.
pub const VAL_EVERY: usize = 3;
const MAX_NUM: i128 = 100;
const MAX_DEN: i128 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Form {
    /// Bit i of `mask` set = lag i+1 is used.
    Lin { mask: u8, constant: bool },
    Poly(u8),
}

impl Form {
    /// Number of unknown coefficients.
    pub fn size(&self) -> usize {
        match self {
            Form::Lin { mask, constant } => mask.count_ones() as usize + *constant as usize,
            Form::Poly(d) => *d as usize + 1,
        }
    }
    fn order(&self) -> (usize, u8, u8) {
        match self {
            Form::Lin { mask, constant } => (self.size(), 0, mask * 2 + *constant as u8),
            Form::Poly(d) => (self.size(), 1, *d),
        }
    }
    /// Stable short code used in tags and ids: `lin.12.c`, `lin.3`, `poly.2`.
    pub fn code(&self) -> String {
        match self {
            Form::Lin { mask, constant } => {
                let lags: String = (1..=MAX_LAG as u8).filter(|l| mask & (1 << (l - 1)) != 0).map(|l| l.to_string()).collect();
                format!("lin.{lags}{}", if *constant { ".c" } else { "" })
            }
            Form::Poly(d) => format!("poly.{d}"),
        }
    }
    pub fn parse(code: &str) -> Option<Form> {
        let parts: Vec<&str> = code.split('.').collect();
        match parts.as_slice() {
            ["poly", d] => d.parse::<u8>().ok().filter(|d| (1..=MAX_DEGREE).contains(d)).map(Form::Poly),
            ["lin", lags, rest @ ..] => {
                let mut mask = 0u8;
                for ch in lags.chars() {
                    let l = ch.to_digit(10)? as usize;
                    if !(1..=MAX_LAG).contains(&l) {
                        return None;
                    }
                    mask |= 1 << (l - 1);
                }
                let constant = match rest {
                    [] => false,
                    ["c"] => true,
                    _ => return None,
                };
                (mask != 0).then_some(Form::Lin { mask, constant })
            }
            _ => None,
        }
    }
    /// The rule as text in the DSL notation.
    pub fn dsl(&self) -> String {
        match self {
            Form::Lin { mask, constant } => {
                let mut terms: Vec<String> = (1..=MAX_LAG as u8).filter(|l| mask & (1 << (l - 1)) != 0).map(|l| format!("a{l}*t[-{l}]")).collect();
                if *constant {
                    terms.push("b".into());
                }
                format!("next = {}", terms.join(" + "))
            }
            Form::Poly(d) => {
                let mut terms = vec!["b0".to_string()];
                for j in 1..=*d {
                    terms.push(if j == 1 { "b1*n".into() } else { format!("b{j}*n^{j}") });
                }
                format!("next = {}   (n = index of the term, first term n = 0)", terms.join(" + "))
            }
        }
    }
    fn lags(&self) -> Vec<usize> {
        match self {
            Form::Lin { mask, .. } => (1..=MAX_LAG).filter(|l| mask & (1 << (l - 1)) != 0).collect(),
            Form::Poly(_) => vec![],
        }
    }
}

/// All forms of the grammar, smallest first.
pub fn forms() -> Vec<Form> {
    let mut v = vec![];
    for mask in 1u8..(1 << MAX_LAG) {
        for constant in [false, true] {
            v.push(Form::Lin { mask, constant });
        }
    }
    for d in 1..=MAX_DEGREE {
        v.push(Form::Poly(d));
    }
    v.sort_by_key(|f| f.order());
    v
}

// ---- exact rational arithmetic (i128 fractions, checked) ----

#[derive(Clone, Copy, Debug)]
struct Fr {
    n: i128,
    d: i128,
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Fr {
    fn int(n: i128) -> Fr {
        Fr { n, d: 1 }
    }
    fn norm(n: i128, d: i128) -> Option<Fr> {
        if d == 0 {
            return None;
        }
        let g = gcd(n, d).max(1);
        let s = if d < 0 { -1 } else { 1 };
        Some(Fr { n: s * n / g, d: s * d / g })
    }
    fn add(self, o: Fr) -> Option<Fr> {
        Fr::norm(self.n.checked_mul(o.d)?.checked_add(o.n.checked_mul(self.d)?)?, self.d.checked_mul(o.d)?)
    }
    fn sub(self, o: Fr) -> Option<Fr> {
        self.add(Fr { n: o.n.checked_neg()?, d: o.d })
    }
    fn mul(self, o: Fr) -> Option<Fr> {
        Fr::norm(self.n.checked_mul(o.n)?, self.d.checked_mul(o.d)?)
    }
    fn div(self, o: Fr) -> Option<Fr> {
        Fr::norm(self.n.checked_mul(o.d)?, self.d.checked_mul(o.n)?)
    }
}

/// Unique exact solution of `rows * x = rhs` (more rows than columns allowed), or None
/// if the system is inconsistent, under-determined or the arithmetic overflows.
fn solve(rows: &[Vec<i64>], rhs: &[i64]) -> Option<Vec<Fr>> {
    let m = rows.len();
    let n = rows.first()?.len();
    let mut a: Vec<Vec<Fr>> = rows.iter().zip(rhs).map(|(r, y)| r.iter().map(|v| Fr::int(*v as i128)).chain(std::iter::once(Fr::int(*y as i128))).collect()).collect();
    for c in 0..n {
        let p = (c..m).find(|&i| a[i][c].n != 0)?;
        a.swap(c, p);
        let pv = a[c][c];
        for j in 0..=n {
            a[c][j] = a[c][j].div(pv)?;
        }
        for i in 0..m {
            if i != c && a[i][c].n != 0 {
                let f = a[i][c];
                for j in 0..=n {
                    let t = a[c][j].mul(f)?;
                    a[i][j] = a[i][j].sub(t)?;
                }
            }
        }
    }
    if (n..m).any(|i| a[i][n].n != 0) {
        return None;
    }
    Some((0..n).map(|i| a[i][n]).collect())
}

/// Fit `form` to the prefix `t` and predict the next term. None = not applicable (see module doc).
pub fn predict(form: &Form, t: &[i64]) -> Option<i64> {
    let len = t.len();
    let (rows, rhs, next_row): (Vec<Vec<i64>>, Vec<i64>, Vec<i64>) = match form {
        Form::Lin { constant, .. } => {
            let lags = form.lags();
            let maxlag = *lags.last()?;
            if len <= maxlag {
                return None;
            }
            let row = |i: usize| -> Vec<i64> {
                let mut r: Vec<i64> = lags.iter().map(|l| t[i - l]).collect();
                if *constant {
                    r.push(1);
                }
                r
            };
            ((maxlag..len).map(row).collect(), (maxlag..len).map(|i| t[i]).collect(), row(len))
        }
        Form::Poly(d) => {
            let row = |i: usize| -> Vec<i64> { (0..=*d as u32).map(|j| (i as i64).pow(j)).collect() };
            ((0..len).map(row).collect(), t.to_vec(), row(len))
        }
    };
    if rows.len() < form.size() + MIN_SLACK {
        return None;
    }
    let x = solve(&rows, &rhs)?;
    let mut acc = Fr::int(0);
    for (c, f) in x.iter().zip(&next_row) {
        if c.n.abs() > MAX_NUM || c.d > MAX_DEN {
            return None;
        }
        acc = acc.add(c.mul(Fr::int(*f as i128))?)?;
    }
    if acc.d != 1 {
        return None;
    }
    i64::try_from(acc.n).ok()
}

// ---- building blocks: transforms of the prefix (Track B5, composition) ----

/// One primitive transform of a prefix. Each has an exact inverse for the predicted next term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// First differences: u[i] = t[i+1] - t[i]; next = last + predicted difference.
    Diff,
    /// Running sums: u[i] = t[0] + ... + t[i]; next = predicted sum - sum of the prefix.
    Cum,
    /// The subsequence of terms whose index has the same parity as the index of the next term.
    Par,
}

/// The transforms the search may put in front of a form (depth at most 2), simplest first.
pub const XFORMS: [(&str, &[Op]); 7] = [
    ("id", &[]),
    ("d", &[Op::Diff]),
    ("s", &[Op::Cum]),
    ("p", &[Op::Par]),
    ("dd", &[Op::Diff, Op::Diff]),
    ("dp", &[Op::Diff, Op::Par]),
    ("pd", &[Op::Par, Op::Diff]),
];

/// A chain of transforms (index into `XFORMS`); `Xf::ID` is the plain grammar of B2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Xf(pub u8);

impl Xf {
    pub const ID: Xf = Xf(0);
    pub fn ops(self) -> &'static [Op] {
        XFORMS[self.0 as usize % XFORMS.len()].1
    }
    pub fn code(self) -> &'static str {
        XFORMS[self.0 as usize % XFORMS.len()].0
    }
    pub fn parse(code: &str) -> Option<Xf> {
        XFORMS.iter().position(|x| x.0 == code).map(|i| Xf(i as u8))
    }
    /// Number of primitive transforms (0 for the identity).
    pub fn cost(self) -> usize {
        self.ops().len()
    }
    pub fn is_id(self) -> bool {
        self.0 == 0
    }
    /// In words, for the rule text.
    pub fn words(self) -> String {
        self.ops()
            .iter()
            .map(|o| match o {
                Op::Diff => "first differences",
                Op::Cum => "running sums",
                Op::Par => "the subsequence at the parity of the next index",
            })
            .collect::<Vec<_>>()
            .join(", then ")
    }
}

fn fwd(op: Op, s: &[i64]) -> Option<Vec<i64>> {
    match op {
        Op::Diff => {
            if s.len() < 2 {
                return None;
            }
            s.windows(2).map(|w| w[1].checked_sub(w[0])).collect()
        }
        Op::Cum => {
            let mut acc = 0i64;
            s.iter()
                .map(|x| {
                    acc = acc.checked_add(*x)?;
                    Some(acc)
                })
                .collect()
        }
        Op::Par => {
            let len = s.len();
            Some(s.iter().enumerate().filter(|(i, _)| i % 2 == len % 2).map(|(_, x)| *x).collect())
        }
    }
}

fn inv(op: Op, before: &[i64], pred: i64) -> Option<i64> {
    match op {
        Op::Diff => before.last()?.checked_add(pred),
        Op::Cum => pred.checked_sub(before.iter().try_fold(0i64, |a, x| a.checked_add(*x))?),
        Op::Par => Some(pred),
    }
}

/// Apply the transforms to the prefix, fit `form` on the transformed sequence (same applicability
/// rules as `predict`), and map the predicted next term back. None = not applicable.
pub fn predict_x(xf: Xf, form: &Form, t: &[i64]) -> Option<i64> {
    if xf.is_id() {
        return predict(form, t);
    }
    let ops = xf.ops();
    let mut seqs: Vec<Vec<i64>> = vec![t.to_vec()];
    for op in ops {
        let n = fwd(*op, seqs.last()?)?;
        seqs.push(n);
    }
    let mut v = predict(form, seqs.last()?)?;
    for (k, op) in ops.iter().enumerate().rev() {
        v = inv(*op, &seqs[k], v)?;
    }
    Some(v)
}

// ---- examples, rules ----

/// One revealed example: the prefix the being was shown and the exact next term the checker later revealed.
#[derive(Debug, Clone, PartialEq)]
pub struct Example {
    /// The outcome event this example was read from.
    pub event_id: String,
    /// Difficulty of the task (the observable part of the signature).
    pub sig: u8,
    pub terms: Vec<i64>,
    pub next: i64,
}

/// Read the example out of an outcome event (tags `terms`, `next`, `domain:pattern`, `difficulty`).
pub fn example_from_event(e: &EventRow) -> Option<Example> {
    if e.kind != "outcome" {
        return None;
    }
    let dom = match tag(&e.tags, "domain") {
        Some("pattern") => Domain::Pattern,
        Some("pattern_b") => Domain::PatternB,
        _ => return None,
    };
    let terms: Option<Vec<i64>> = tag(&e.tags, "terms")?.split(';').map(|x| x.parse().ok()).collect();
    let diff: u8 = tag(&e.tags, "difficulty")?.parse().ok()?;
    Some(Example { event_id: e.event_id.clone(), sig: sig_of(dom, diff), terms: terms?, next: tag(&e.tags, "next")?.parse().ok()? })
}

/// Signature of the examples of a task: the difficulty, plus 10 for the B6 domain `pattern_b` (its rules are its own).
pub fn sig_of(d: Domain, difficulty: u8) -> u8 {
    if d == Domain::PatternB { difficulty + 10 } else { difficulty }
}

/// Inverse of `sig_of`: (domain, difficulty).
pub fn sig_split(sig: u8) -> (Domain, u8) {
    if sig >= 10 { (Domain::PatternB, sig - 10) } else { (Domain::Pattern, sig) }
}

pub fn terms_tag(terms: &[i64]) -> String {
    terms.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(";")
}

/// An accepted rule: the form for one signature, plus how it was accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Content-derived: sha256 of (version, form code, signature), first 12 hex characters.
    pub id: String,
    pub sig: u8,
    pub form: Form,
    /// Training / held-out examples it reproduced exactly when it was accepted.
    pub train_ok: u32,
    pub val_ok: u32,
    /// Step count of the being when it was accepted.
    pub step: u64,
    /// The `rule` note event in the log (empty until written).
    pub event_id: String,
    /// Transform put in front of the form (`Xf::ID` for a plain B2 rule).
    pub xf: Xf,
    /// Number of verified building blocks the search had when it accepted a composed rule (0 for plain rules).
    pub nblocks: u32,
    /// Retired (or merged into a simpler rule) by a logged revision: kept in the book, never applied again.
    pub retired: bool,
}

pub fn rule_id(sig: u8, form: &Form) -> String {
    sha256_hex(&[b"being-rule-v1", &[0x1f], form.code().as_bytes(), &[0x1f], &[sig]])[..12].to_string()
}

/// Id of a rule with a transform; identical to `rule_id` for the identity transform (older logs stay valid).
pub fn rule_id_x(sig: u8, xf: Xf, form: &Form) -> String {
    if xf.is_id() {
        return rule_id(sig, form);
    }
    sha256_hex(&[b"being-rule-v2", &[0x1f], xf.code().as_bytes(), &[0x1f], form.code().as_bytes(), &[0x1f], &[sig]])[..12].to_string()
}

impl Rule {
    pub fn new(sig: u8, form: Form, train_ok: u32, val_ok: u32, step: u64) -> Rule {
        Rule { id: rule_id(sig, &form), sig, form, train_ok, val_ok, step, event_id: String::new(), xf: Xf::ID, nblocks: 0, retired: false }
    }
    pub fn new_x(sig: u8, xf: Xf, form: Form, train_ok: u32, val_ok: u32, step: u64, nblocks: u32) -> Rule {
        Rule { id: rule_id_x(sig, xf, &form), xf, nblocks, ..Rule::new(sig, form, train_ok, val_ok, step) }
    }
    /// Sort key for "smallest applicable rule first".
    pub fn key(&self) -> (usize, usize, u8, u8) {
        let o = self.form.order();
        (self.xf.cost(), o.0, o.1, o.2)
    }
    pub fn text(&self) -> String {
        if !self.xf.is_id() {
            return format!(
                "being composed rule {} for {} tasks of difficulty {}: take {} of the prefix, then {}; the form is a building block the being had already verified; coefficients are fitted per task from its own prefix; reproduced the revealed next term exactly on {} training and {} held-out logged examples and was never applicable-and-wrong on them (composition of verified blocks inside an author-defined space)",
                self.id, self.dom_name(), self.sig % 10, self.xf.words(), self.form.dsl(), self.train_ok, self.val_ok
            );
        }
        format!(
            "being discovered rule {} for {} tasks of difficulty {}: {}; coefficients are fitted per task from its own prefix; reproduced the revealed next term exactly on {} training and {} held-out logged examples and was never applicable-and-wrong on them (search inside an author-defined grammar)",
            self.id, self.dom_name(), self.sig % 10, self.form.dsl(), self.train_ok, self.val_ok
        )
    }
    /// `pattern` or (B6) `pattern_b`.
    pub fn dom_name(&self) -> &'static str {
        sig_split(self.sig).0.name()
    }
    /// The log event that carries the rule. Returns the event and its id.
    pub fn event(&self, occurred_at: String) -> (NewEvent, String) {
        let text = self.text();
        let ev = NewEvent {
            source: super::state::SOURCE.into(),
            occurred_at,
            kind: "note".into(),
            text,
            tags: vec![
                "rule".into(),
                format!("rule_id:{}", self.id),
                format!("sig:{}", self.sig),
                format!("form:{}", self.form.code()),
                format!("train:{}", self.train_ok),
                format!("val:{}", self.val_ok),
                format!("at_step:{}", self.step),
            ],
            redacted: false,
        };
        let mut ev = ev;
        if !self.xf.is_id() {
            ev.tags.push(format!("xf:{}", self.xf.code()));
            ev.tags.push(format!("blocks:{}", self.nblocks));
        }
        let id = crate::versioned::event_id(&ev.source, &ev.occurred_at, &ev.text);
        (ev, id)
    }
}

pub fn rule_from_event(e: &EventRow) -> Option<Rule> {
    if e.kind != "note" || !e.tags.split(',').any(|t| t == "rule") {
        return None;
    }
    let sig: u8 = tag(&e.tags, "sig")?.parse().ok()?;
    let form = Form::parse(tag(&e.tags, "form")?)?;
    let id = tag(&e.tags, "rule_id")?.to_string();
    let xf = match tag(&e.tags, "xf") {
        Some(c) => Xf::parse(c)?,
        None => Xf::ID,
    };
    if id != rule_id_x(sig, xf, &form) {
        return None;
    }
    let nblocks = tag(&e.tags, "blocks").and_then(|b| b.parse().ok()).unwrap_or(0);
    Some(Rule { id, sig, form, train_ok: tag(&e.tags, "train")?.parse().ok()?, val_ok: tag(&e.tags, "val")?.parse().ok()?, step: tag(&e.tags, "at_step")?.parse().ok()?, event_id: e.event_id.clone(), xf, nblocks, retired: false })
}

/// Apply the smallest applicable rule of signature `sig` to a prefix.
pub fn apply<'a>(rules: &'a [Rule], sig: u8, t: &[i64]) -> Option<(i64, &'a Rule)> {
    let mut best: Option<((usize, usize, u8, u8), &str, i64, &Rule)> = None;
    for r in rules.iter().filter(|r| r.sig == sig && !r.retired) {
        if let Some(v) = predict_x(r.xf, &r.form, t) {
            let key = r.key();
            if best.as_ref().map_or(true, |b| (key, r.id.as_str()) < (b.0, b.1)) {
                best = Some((key, r.id.as_str(), v, r));
            }
        }
    }
    best.map(|b| (b.2, b.3))
}

// ---- the search ----

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub sig: u8,
    /// Transform in front of the form (`Xf::ID` for a plain grammar rule).
    pub xf: Xf,
    pub form: Form,
    pub train_ok: u32,
    pub val_ok: u32,
    /// Support not already explained by a smaller accepted rule.
    pub new_train: u32,
    pub new_val: u32,
    /// Event ids of examples it reproduced (new support first), at most `MAX_CITE`.
    pub cite: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum St {
    NotApplicable,
    Right,
    Wrong,
}

pub fn is_held_out(index_in_signature: usize) -> bool {
    index_in_signature % VAL_EVERY == VAL_EVERY - 1
}

/// Deterministic search over all signatures present in `examples` (log order).
/// Pure function of its input; returns accepted rules ordered by signature, then by form size.
pub fn search(examples: &[Example]) -> Vec<Found> {
    search_with(examples, K_TRAIN, K_VAL)
}

pub fn search_with(examples: &[Example], k_train: u32, k_val: u32) -> Vec<Found> {
    search_core(examples, k_train, k_val, &[], usize::MAX, &[]).found
}

/// Result of a search that may be cut short by a cap on candidates.
#[derive(Debug, Clone)]
pub struct SearchOut {
    pub found: Vec<Found>,
    /// Candidates (transform, form) evaluated against one signature each.
    pub candidates: usize,
    /// Candidates a complete search would evaluate.
    pub wanted: usize,
    /// The cap stopped the search before all candidates were tried.
    pub truncated: bool,
}

/// Search with composition: after the plain grammar (exactly the B2 search, same order), the transforms
/// of `XFORMS` are put in front of each building block in `blocks` (forms the being has already
/// verified), simplest first. At most `max_candidates` (transform, form) pairs are evaluated in total,
/// in a fixed order, so a capped search returns a prefix of what the uncapped one returns.
pub fn search_composed(examples: &[Example], blocks: &[Form], max_candidates: usize) -> SearchOut {
    search_core(examples, K_TRAIN, K_VAL, blocks, max_candidates, &[])
}

/// As `search_composed`, with transferred hypotheses (B6): for a signature, the candidates `(xf, form)` listed
/// for it in `prefer` (rules already accepted in another domain) are tried FIRST, in the given order, then
/// all the others in the usual order. Acceptance is exactly the same (same support, held-out split, spare
/// equations, never applicable-and-wrong): only the order of trial changes, and with it which of two
/// forms that explain the same examples is seen first. Hypotheses that are not among the candidates are ignored.
pub fn search_transfer(examples: &[Example], blocks: &[Form], max_candidates: usize, prefer: &[(u8, Xf, Form)]) -> SearchOut {
    search_core(examples, K_TRAIN, K_VAL, blocks, max_candidates, prefer)
}

/// The same form with the constant term toggled (a block that is shared up to an offset); None for polynomials.
pub fn toggle_constant(f: &Form) -> Option<Form> {
    match f {
        Form::Lin { mask, constant } => Some(Form::Lin { mask: *mask, constant: !*constant }),
        Form::Poly(_) => None,
    }
}

pub(crate) fn candidates_for(blocks: &[Form]) -> Vec<(Xf, Form)> {
    let mut v: Vec<(Xf, Form)> = forms().into_iter().map(|f| (Xf::ID, f)).collect();
    let mut c: Vec<(Xf, Form)> = vec![];
    for x in 1..XFORMS.len() {
        for b in blocks {
            c.push((Xf(x as u8), *b));
        }
    }
    c.sort_by_key(|(x, f)| (x.cost(), f.order(), x.0));
    v.extend(c);
    v
}

fn search_core(examples: &[Example], k_train: u32, k_val: u32, blocks: &[Form], max_candidates: usize, prefer: &[(u8, Xf, Form)]) -> SearchOut {
    let mut sigs: Vec<u8> = examples.iter().map(|e| e.sig).collect();
    sigs.sort_unstable();
    sigs.dedup();
    let cands = candidates_for(blocks);
    let mut out = vec![];
    let (mut tried, mut truncated) = (0usize, false);
    'sigs: for sig in &sigs {
        let sig = *sig;
        let ex: Vec<&Example> = examples.iter().filter(|e| e.sig == sig).collect();
        let mut covered = vec![false; ex.len()];
        let ordered: Vec<&(Xf, Form)> = if prefer.iter().any(|p| p.0 == sig) {
            let first: Vec<&(Xf, Form)> = prefer.iter().filter(|p| p.0 == sig).filter_map(|p| cands.iter().find(|c| c.0 == p.1 && c.1 == p.2)).fold(vec![], |mut v, c| {
                if !v.iter().any(|x| std::ptr::eq(*x, c)) {
                    v.push(c);
                }
                v
            });
            let rest = cands.iter().filter(|c| !first.iter().any(|x| std::ptr::eq(*x, *c)));
            first.iter().copied().chain(rest).collect()
        } else {
            cands.iter().collect()
        };
        for (xf, form) in ordered {
            if tried >= max_candidates {
                truncated = true;
                break 'sigs;
            }
            tried += 1;
            let status: Vec<St> = ex
                .iter()
                .map(|e| match predict_x(*xf, form, &e.terms) {
                    None => St::NotApplicable,
                    Some(v) if v == e.next => St::Right,
                    Some(_) => St::Wrong,
                })
                .collect();
            let (mut tr_ok, mut va_ok, mut tr_new, mut va_new, mut wrong) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for (i, s) in status.iter().enumerate() {
                match s {
                    St::Wrong => wrong += 1,
                    St::Right => {
                        let held = is_held_out(i);
                        if held {
                            va_ok += 1;
                        } else {
                            tr_ok += 1;
                        }
                        if !covered[i] {
                            if held {
                                va_new += 1;
                            } else {
                                tr_new += 1;
                            }
                        }
                    }
                    St::NotApplicable => {}
                }
            }
            if wrong == 0 && tr_new >= k_train && va_new >= k_val {
                let mut cite: Vec<String> = vec![];
                for pass_new in [true, false] {
                    for (i, s) in status.iter().enumerate() {
                        if *s == St::Right && (!covered[i]) == pass_new && cite.len() < MAX_CITE {
                            cite.push(ex[i].event_id.clone());
                        }
                    }
                }
                for (i, s) in status.iter().enumerate() {
                    if *s == St::Right {
                        covered[i] = true;
                    }
                }
                out.push(Found { sig, xf: *xf, form: *form, train_ok: tr_ok, val_ok: va_ok, new_train: tr_new, new_val: va_new, cite });
            }
        }
    }
    SearchOut { found: out, candidates: tried, wanted: sigs.len() * cands.len(), truncated }
}

/// Exactly how a form does on a set of examples: (right, wrong, not applicable).
pub fn evaluate(form: &Form, examples: &[Example]) -> (u32, u32, u32) {
    let (mut r, mut w, mut n) = (0, 0, 0);
    for e in examples {
        match predict(form, &e.terms) {
            None => n += 1,
            Some(v) if v == e.next => r += 1,
            Some(_) => w += 1,
        }
    }
    (r, w, n)
}

/// As `evaluate`, for a form behind a transform.
pub fn evaluate_x(xf: Xf, form: &Form, examples: &[Example]) -> (u32, u32, u32) {
    let (mut r, mut w, mut n) = (0, 0, 0);
    for e in examples {
        match predict_x(xf, form, &e.terms) {
            None => n += 1,
            Some(v) if v == e.next => r += 1,
            Some(_) => w += 1,
        }
    }
    (r, w, n)
}

/// Acceptance of a PROPOSED form (from an outside advisor) for one signature: exactly the acceptance
/// of the search above. The form is accepted only if the full search over the being's own logged
/// examples accepts that very form for that signature, with the same minimum support, the same
/// held-out split and the same never-applicable-and-wrong condition. A proposal cannot lower any bar.
pub fn verify_proposal(examples: &[Example], sig: u8, form: &Form) -> Option<Found> {
    search(examples).into_iter().find(|f| f.sig == sig && f.form == *form && f.xf.is_id())
}
