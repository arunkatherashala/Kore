//! Integer sequence rules: a small DSL of predictors over the non-null values `v[0..m)` of a column.
//! The rule is stored; the values are regenerated as `v[i] = pred(v[..i], i) + r[i]` (wrapping i64
//! arithmetic) from the residuals `r`, which go through the best version-4 stage. A rule that fits
//! exactly leaves only zeros; a rule that almost fits leaves its exceptions as non-zero residuals.
//!
//! ```text
//! model := 0 Rec    k:u8 (0..=3) | c_1..c_k: zz | c0: zz     pred = c0 + sum c_j * v[i-j]  (v[<0] = 0)
//!        | 1 Affine a: zz | b: zz                            pred = a + b * i
//!        | 2 Lag    p: varint (1..=m)                        pred = v[i-p] (0 before)
//!        | 3 LagTrend p: varint                              pred = v[i-p] + v[i-1] - v[i-p-1]
//!        | 4 Runs                                            run values + run lengths, no residual
//! body  := model | residual stream (m zigzag varints)        (Runs: count | values stream | lengths stream)
//! ```
//! `Rec` with k = 1, 2, 3 and binomial coefficients is delta, delta-of-delta and the third difference
//! (polynomials of degree up to 2 leave a constant residual); other `Rec` coefficients come from an exact
//! fit (linear recurrences of order up to 3, e.g. geometric, Fibonacci-like).

use kore_core::KoreError;
use super::stream::{self, bad, put_varint, put_zz, Levels, Rd};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntModel {
    Rec { c: Vec<i64>, c0: i64 },
    Affine { a: i64, b: i64 },
    Lag { p: usize },
    LagTrend { p: usize },
    Runs,
}

const MAX_K: usize = 3;
/// Longest period searched.
const P_MAX: usize = 2048;
/// Scoring windows.
const WIN: usize = 16 << 10;

impl IntModel {
    pub fn delta(k: usize) -> IntModel {
        let c = match k { 0 => vec![], 1 => vec![1], 2 => vec![2, -1], _ => vec![3, -3, 1] };
        IntModel::Rec { c, c0: 0 }
    }

    pub fn describe(&self) -> String {
        match self {
            IntModel::Rec { c, c0 } if c.is_empty() && *c0 == 0 => "plain (no rule)".into(),
            IntModel::Rec { c, c0 } if c.is_empty() => format!("constant {c0}"),
            IntModel::Rec { c, c0: 0 } if *c == [1] => "delta".into(),
            IntModel::Rec { c, c0 } if *c == [1] => format!("arithmetic step {c0}"),
            IntModel::Rec { c, c0: 0 } if *c == [2, -1] => "delta-of-delta".into(),
            IntModel::Rec { c, c0: 0 } if *c == [3, -3, 1] => "third difference".into(),
            IntModel::Rec { c, c0 } => {
                let mut s = String::from("recurrence v=");
                for (j, x) in c.iter().enumerate() { if *x != 0 { s.push_str(&format!("{x:+}*v[-{}]", j + 1)); } }
                if *c0 != 0 { s.push_str(&format!("{c0:+}")); }
                s
            }
            IntModel::Affine { a, b } => format!("affine {a}+{b}*i"),
            IntModel::Lag { p } => format!("lag {p}"),
            IntModel::LagTrend { p } => format!("lag {p} + trend"),
            IntModel::Runs => "runs".into(),
        }
    }

    fn put(&self, out: &mut Vec<u8>) {
        match self {
            IntModel::Rec { c, c0 } => { out.push(0); out.push(c.len() as u8); for x in c { put_zz(out, *x); } put_zz(out, *c0); }
            IntModel::Affine { a, b } => { out.push(1); put_zz(out, *a); put_zz(out, *b); }
            IntModel::Lag { p } => { out.push(2); put_varint(out, *p as u64); }
            IntModel::LagTrend { p } => { out.push(3); put_varint(out, *p as u64); }
            IntModel::Runs => out.push(4),
        }
    }

    fn get(rd: &mut Rd, m: usize) -> Result<IntModel, KoreError> {
        Ok(match rd.u8()? {
            0 => {
                let k = rd.u8()? as usize;
                if k > MAX_K { return Err(bad("recurrence order out of range")); }
                let mut c = Vec::with_capacity(k);
                for _ in 0..k { c.push(rd.zz()?); }
                IntModel::Rec { c, c0: rd.zz()? }
            }
            1 => IntModel::Affine { a: rd.zz()?, b: rd.zz()? },
            2 => IntModel::Lag { p: period(rd, m)? },
            3 => IntModel::LagTrend { p: period(rd, m)? },
            4 => IntModel::Runs,
            k => return Err(bad(format!("unknown integer model {k}"))),
        })
    }
}

fn period(rd: &mut Rd, m: usize) -> Result<usize, KoreError> {
    let p = rd.count(m.max(1))?;
    if p == 0 { return Err(bad("period 0")); }
    Ok(p)
}

#[inline]
fn at(v: &[i64], i: usize, back: usize) -> i64 { if i >= back { v[i - back] } else { 0 } }

/// Prediction for index `i` from `v[..i]` (only earlier values are read).
#[inline]
fn pred(m: &IntModel, v: &[i64], i: usize) -> i64 {
    match m {
        IntModel::Rec { c, c0 } => {
            let mut s = *c0;
            for (j, x) in c.iter().enumerate() { s = s.wrapping_add(x.wrapping_mul(at(v, i, j + 1))); }
            s
        }
        IntModel::Affine { a, b } => a.wrapping_add(b.wrapping_mul(i as i64)),
        IntModel::Lag { p } => at(v, i, *p),
        IntModel::LagTrend { p } => if i > *p { v[i - p].wrapping_add(v[i - 1]).wrapping_sub(v[i - p - 1]) } else { at(v, i, 1) },
        IntModel::Runs => unreachable!(),
    }
}

/// Residuals of `model` over `range` (needs the whole sequence for history).
fn residuals(model: &IntModel, v: &[i64], range: std::ops::Range<usize>) -> Vec<i64> {
    // the common cases get a tight loop
    match model {
        IntModel::Rec { c, c0 } if c.len() == 1 && c[0] == 1 => range.map(|i| v[i].wrapping_sub(at(v, i, 1).wrapping_add(*c0))).collect(),
        _ => range.map(|i| v[i].wrapping_sub(pred(model, v, i))).collect(),
    }
}

fn runs(v: &[i64]) -> (Vec<i64>, Vec<u64>) {
    let (mut vals, mut lens) = (vec![], vec![]);
    let mut i = 0;
    while i < v.len() {
        let mut j = i + 1;
        while j < v.len() && v[j] == v[i] { j += 1; }
        vals.push(v[i]);
        lens.push((j - i - 1) as u64);
        i = j;
    }
    (vals, lens)
}

fn encode_with(model: &IntModel, v: &[i64], lv: Levels) -> Vec<u8> {
    let mut out = Vec::new();
    model.put(&mut out);
    if *model == IntModel::Runs {
        let (vals, lens) = runs(v);
        put_varint(&mut out, vals.len() as u64);
        let mut d = Vec::with_capacity(vals.len());
        let mut prev = 0i64;
        for x in &vals { d.push(x.wrapping_sub(prev)); prev = *x; }
        stream::put_stream(&mut out, &stream::zz_bytes(&d), lv);
        let mut lb = Vec::with_capacity(lens.len());
        for l in &lens { put_varint(&mut lb, *l); }
        stream::put_stream(&mut out, &lb, lv);
    } else {
        stream::put_stream(&mut out, &stream::zz_bytes(&residuals(model, v, 0..v.len())), lv);
    }
    out
}

/// Decode `m` values written by `encode`.
pub fn decode(rd: &mut Rd, m: usize) -> Result<Vec<i64>, KoreError> {
    let model = IntModel::get(rd, m)?;
    if model == IntModel::Runs {
        let k = rd.count(m)?;
        let d = stream::get_zz_stream(rd, k)?;
        let lens = stream::get_uint_stream(rd, k, m as u64)?;
        let mut out = Vec::with_capacity(m);
        let mut prev = 0i64;
        for (x, l) in d.iter().zip(&lens) {
            prev = prev.wrapping_add(*x);
            let l = *l as usize + 1;
            if out.len() + l > m { return Err(bad("run lengths exceed the row count")); }
            out.extend(std::iter::repeat(prev).take(l));
        }
        if out.len() != m { return Err(bad("run lengths do not cover the row count")); }
        return Ok(out);
    }
    let r = stream::get_zz_stream(rd, m)?;
    let mut v: Vec<i64> = Vec::with_capacity(m);
    match &model {
        IntModel::Rec { c, c0 } if c.len() == 1 && c[0] == 1 => {
            let mut prev = 0i64;
            for x in r { prev = prev.wrapping_add(*c0).wrapping_add(x); v.push(prev); }
        }
        IntModel::Rec { c, c0 } if c.len() == 2 => {
            let (c1, c2, c0) = (c[0], c[1], *c0);
            let (mut p1, mut p2) = (0i64, 0i64);
            for x in r {
                let y = c0.wrapping_add(c1.wrapping_mul(p1)).wrapping_add(c2.wrapping_mul(p2)).wrapping_add(x);
                v.push(y);
                p2 = p1;
                p1 = y;
            }
        }
        IntModel::Affine { a, b } => for (i, x) in r.into_iter().enumerate() { v.push(a.wrapping_add(b.wrapping_mul(i as i64)).wrapping_add(x)); },
        _ => for (i, x) in r.into_iter().enumerate() { let p = pred(&model, &v, i); v.push(p.wrapping_add(x)); },
    }
    Ok(v)
}

pub fn describe(rd: &mut Rd, m: usize) -> String { IntModel::get(rd, m).map(|x| x.describe()).unwrap_or_else(|_| "?".into()) }

// ── search ───────────────────────────────────────────────────────────────────

fn sample_score(model: &IntModel, v: &[i64]) -> usize {
    if *model == IntModel::Runs {
        // score the run layout on the windows
        let mut s = 0;
        for w in stream::windows(v.len(), WIN) {
            let (vals, lens) = runs(&v[w]);
            let mut d = Vec::with_capacity(vals.len());
            let mut prev = 0i64;
            for x in &vals { d.push(x.wrapping_sub(prev)); prev = *x; }
            let mut lb = Vec::new();
            for l in &lens { put_varint(&mut lb, *l); }
            s += stream::score(&stream::zz_bytes(&d)) + stream::score(&lb);
        }
        return s;
    }
    stream::windows(v.len(), WIN).into_iter().map(|w| stream::score(&stream::zz_bytes(&residuals(model, v, w)))).sum()
}

fn bits(x: i64) -> u32 { 64 - stream::zz(x).leading_zeros() }
/// Best period by summed residual bit length on a sample: the smallest period whose cost is within 3 %
/// of the cheapest (multiples of the true period cost about the same).
fn best_period(v: &[i64]) -> Option<usize> {
    let m = v.len();
    let pmax = P_MAX.min(m / 3);
    if pmax < 2 { return None; }
    let s_len = 2048.min(m - pmax);
    let start = pmax;
    let costs: Vec<u64> = (2..=pmax).map(|p| (start..start + s_len).map(|i| bits(v[i].wrapping_sub(v[i - p])) as u64).sum()).collect();
    let min = *costs.iter().min()?;
    let base: u64 = (start..start + s_len).map(|i| bits(v[i].wrapping_sub(v[i - 1])) as u64).sum();
    if min >= base { return None; }
    costs.iter().position(|&c| c * 100 <= min * 103).map(|k| k + 2)
}

// exact rational arithmetic for the recurrence fit (B2-style: unique solution, spare equations agree)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fr { n: i128, d: i128 }
fn gcd(a: i128, b: i128) -> i128 { let (mut a, mut b) = (a.abs(), b.abs()); while b != 0 { let t = a % b; a = b; b = t; } a }
impl Fr {
    fn int(n: i128) -> Fr { Fr { n, d: 1 } }
    fn norm(n: i128, d: i128) -> Option<Fr> {
        if d == 0 { return None; }
        let g = gcd(n, d).max(1);
        let (mut n, mut d) = (n / g, d / g);
        if d < 0 { n = n.checked_neg()?; d = d.checked_neg()?; }
        Some(Fr { n, d })
    }
    fn sub(self, o: Fr) -> Option<Fr> { Fr::norm(self.n.checked_mul(o.d)?.checked_sub(o.n.checked_mul(self.d)?)?, self.d.checked_mul(o.d)?) }
    fn mul(self, o: Fr) -> Option<Fr> { Fr::norm(self.n.checked_mul(o.n)?, self.d.checked_mul(o.d)?) }
    fn div(self, o: Fr) -> Option<Fr> { Fr::norm(self.n.checked_mul(o.d)?, self.d.checked_mul(o.n)?) }
}

/// Solve the square system exactly; None if singular or on overflow.
fn solve(mut a: Vec<Vec<Fr>>, mut b: Vec<Fr>) -> Option<Vec<Fr>> {
    let q = b.len();
    for col in 0..q {
        let piv = (col..q).find(|&r| a[r][col].n != 0)?;
        a.swap(col, piv);
        b.swap(col, piv);
        for r in 0..q {
            if r == col || a[r][col].n == 0 { continue; }
            let f = a[r][col].div(a[col][col])?;
            for c in col..q { a[r][c] = a[r][c].sub(f.mul(a[col][c])?)?; }
            b[r] = b[r].sub(f.mul(b[col])?)?;
        }
    }
    (0..q).map(|i| b[i].div(a[i][i])).collect()
}

/// Recurrence forms: a non-empty subset of lags {1,2,3}, with or without a constant.
fn rec_forms() -> Vec<(Vec<usize>, bool)> {
    let mut f = vec![];
    for mask in 1u8..8 {
        let lags: Vec<usize> = (1..=3).filter(|l| mask >> (l - 1) & 1 == 1).collect();
        for c in [false, true] { f.push((lags.clone(), c)); }
    }
    f.sort_by_key(|(l, c)| (l.len() + *c as usize, *l.last().unwrap()));
    f
}

/// Fit a recurrence form exactly on rows `s..` (q unknowns, q + 3 equations; the 3 spare equations must
/// agree), integer coefficients of magnitude at most 2^20 only.
fn fit_rec(v: &[i64], s: usize, lags: &[usize], konst: bool) -> Option<IntModel> {
    let q = lags.len() + konst as usize;
    let eqs = q + 3;
    if s < 3 || s + eqs > v.len() { return None; }
    let row = |i: usize| -> Vec<Fr> {
        let mut r: Vec<Fr> = lags.iter().map(|&l| Fr::int(v[i - l] as i128)).collect();
        if konst { r.push(Fr::int(1)); }
        r
    };
    let a: Vec<Vec<Fr>> = (s..s + q).map(row).collect();
    let b: Vec<Fr> = (s..s + q).map(|i| Fr::int(v[i] as i128)).collect();
    let x = solve(a, b)?;
    if x.iter().any(|f| f.d != 1 || f.n.abs() > 1 << 20) { return None; }
    let x: Vec<i64> = x.iter().map(|f| f.n as i64).collect();
    let mut c = vec![0i64; *lags.last().unwrap()];
    for (j, &l) in lags.iter().enumerate() { c[l - 1] = x[j]; }
    let c0 = if konst { x[q - 1] } else { 0 };
    let model = IntModel::Rec { c, c0 };
    // spare equations
    if (s + q..s + eqs).all(|i| pred(&model, v, i) == v[i]) { Some(model) } else { None }
}

/// The candidate models for `v`, cheapest family first.
pub fn candidates(v: &[i64]) -> Vec<IntModel> {
    let m = v.len();
    let mut c = vec![IntModel::delta(0), IntModel::delta(1), IntModel::delta(2), IntModel::delta(3), IntModel::Runs];
    if m >= 2 {
        // affine in the row index: slope from the end points and from the most common step
        let (num, den) = (v[m - 1] as i128 - v[0] as i128, m as i128 - 1);
        let b1 = ((2 * num + num.signum() * den) / (2 * den)) as i64; // end-point slope, rounded
        let mut steps: Vec<i64> = (1..m.min(4097)).map(|i| v[i].wrapping_sub(v[i - 1])).collect();
        steps.sort_unstable();
        let mut mode = (steps[0], 0usize);
        let mut k = 0;
        while k < steps.len() { let mut j = k; while j < steps.len() && steps[j] == steps[k] { j += 1; } if j - k > mode.1 { mode = (steps[k], j - k); } k = j; }
        let median = steps[steps.len() / 2];
        for b in [b1, median, mode.0] {
            // intercept: median of v[i] - b*i on a sample
            let step = (m / 4096).max(1);
            let mut off: Vec<i64> = (0..m).step_by(step).map(|i| v[i].wrapping_sub(b.wrapping_mul(i as i64))).collect();
            off.sort_unstable();
            let a = off[off.len() / 2];
            let mdl = IntModel::Affine { a, b };
            if !c.contains(&mdl) { c.push(mdl); }
        }
    }
    if let Some(p) = best_period(v) { c.push(IntModel::Lag { p }); c.push(IntModel::LagTrend { p }); }
    for (lags, konst) in rec_forms() {
        for s in [3, m / 3, 2 * m / 3] {
            if let Some(mdl) = fit_rec(v, s.max(3), &lags, konst) {
                if !c.contains(&mdl) { c.push(mdl); }
                break;
            }
        }
    }
    c
}

/// Encode `v` with the best model: candidates are scored on sample windows (zstd level 1), the two best
/// are encoded in full and the smaller body is returned (ties keep the earlier candidate).
pub fn encode(v: &[i64], lv: Levels) -> (IntModel, Vec<u8>) {
    use rayon::prelude::*;
    let cands = candidates(v);
    if v.len() <= 64 {
        // short: encode every candidate in full
        return cands.into_iter().map(|m| { let b = encode_with(&m, v, lv); (m, b) })
            .fold(None, |best: Option<(IntModel, Vec<u8>)>, x| match best { Some(b) if b.1.len() <= x.1.len() => Some(b), _ => Some(x) }).unwrap();
    }
    let scores: Vec<usize> = cands.par_iter().map(|m| sample_score(m, v)).collect();
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by_key(|&i| (scores[i], i));
    let top: Vec<(IntModel, Vec<u8>)> = order.iter().take(2).collect::<Vec<_>>().par_iter()
        .map(|&&i| (cands[i].clone(), encode_with(&cands[i], v, lv))).collect();
    top.into_iter().fold(None, |best: Option<(IntModel, Vec<u8>)>, x| match best { Some(b) if b.1.len() <= x.1.len() => Some(b), _ => Some(x) }).unwrap()
}

/// Encode with one given model (tests, forced rules).
pub fn encode_model(model: &IntModel, v: &[i64], lv: Levels) -> Vec<u8> { encode_with(model, v, lv) }
