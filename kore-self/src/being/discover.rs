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
//! `MIN_SLACK` more equations than unknowns, all satisfied exactly, (3) the
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
use crate::versioned::{sha256_hex, EventRow, NewEvent};

pub const MIN_SLACK: usize = 2;
pub const K_TRAIN: u32 = 4;
pub const K_VAL: u32 = 2;
pub const MAX_CITE: usize = 24;
pub const MAX_LAG: usize = 3;
pub const MAX_DEGREE: u8 = 3;
/// Every `VAL_EVERY`-th example of a signature (index % VAL_EVERY == VAL_EVERY - 1) is held out.
pub const VAL_EVERY: usize = 3;
const MAX_NUM: i128 = 100;
const MAX_DEN: i128 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    if e.kind != "outcome" || tag(&e.tags, "domain") != Some("pattern") {
        return None;
    }
    let terms: Option<Vec<i64>> = tag(&e.tags, "terms")?.split(';').map(|x| x.parse().ok()).collect();
    Some(Example { event_id: e.event_id.clone(), sig: tag(&e.tags, "difficulty")?.parse().ok()?, terms: terms?, next: tag(&e.tags, "next")?.parse().ok()? })
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
}

pub fn rule_id(sig: u8, form: &Form) -> String {
    sha256_hex(&[b"being-rule-v1", &[0x1f], form.code().as_bytes(), &[0x1f], &[sig]])[..12].to_string()
}

impl Rule {
    pub fn new(sig: u8, form: Form, train_ok: u32, val_ok: u32, step: u64) -> Rule {
        Rule { id: rule_id(sig, &form), sig, form, train_ok, val_ok, step, event_id: String::new() }
    }
    pub fn text(&self) -> String {
        format!(
            "being discovered rule {} for pattern tasks of difficulty {}: {}; coefficients are fitted per task from its own prefix; reproduced the revealed next term exactly on {} training and {} held-out logged examples and was never applicable-and-wrong on them (search inside an author-defined grammar)",
            self.id, self.sig, self.form.dsl(), self.train_ok, self.val_ok
        )
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
    if id != rule_id(sig, &form) {
        return None;
    }
    Some(Rule { id, sig, form, train_ok: tag(&e.tags, "train")?.parse().ok()?, val_ok: tag(&e.tags, "val")?.parse().ok()?, step: tag(&e.tags, "at_step")?.parse().ok()?, event_id: e.event_id.clone() })
}

/// Apply the smallest applicable rule of signature `sig` to a prefix.
pub fn apply<'a>(rules: &'a [Rule], sig: u8, t: &[i64]) -> Option<(i64, &'a Rule)> {
    let mut best: Option<((usize, u8, u8), &str, i64, &Rule)> = None;
    for r in rules.iter().filter(|r| r.sig == sig) {
        if let Some(v) = predict(&r.form, t) {
            let key = r.form.order();
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
    let mut sigs: Vec<u8> = examples.iter().map(|e| e.sig).collect();
    sigs.sort_unstable();
    sigs.dedup();
    let all_forms = forms();
    let mut out = vec![];
    for sig in sigs {
        let ex: Vec<&Example> = examples.iter().filter(|e| e.sig == sig).collect();
        let mut covered = vec![false; ex.len()];
        for form in &all_forms {
            let status: Vec<St> = ex
                .iter()
                .map(|e| match predict(form, &e.terms) {
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
                out.push(Found { sig, form: *form, train_ok: tr_ok, val_ok: va_ok, new_train: tr_new, new_val: va_new, cite });
            }
        }
    }
    out
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
