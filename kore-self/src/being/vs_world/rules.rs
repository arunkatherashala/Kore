//! Rule learning versus standard alternatives, on the same revealed examples (Track V1).
//!
//! * `guarded_search`: the B2 acceptance guard (support of 3 training and 1 held-out example not already
//!   explained by a smaller accepted form, every third example held out, never applicable-and-wrong),
//!   with a pluggable per-task predictor. With `discover::predict` it is the being's B2 search (a test
//!   checks it accepts the same forms as `discover::search`).
//! * `bf_predict`: brute-force enumeration over the SAME 17-form DSL with the same verification (at least
//!   `MIN_SLACK` spare equations, all exact), but the per-task coefficients are found by enumerating a
//!   bounded integer grid instead of by exact rational elimination (Gauss-Jordan). Used inside the same
//!   guard, it is the "enumerate and verify" program-search baseline.
//! * k-nearest-neighbour regression (Fix, Hodges 1951; Cover, Hart 1967) and a CART regression tree
//!   (Breiman et al. 1984) on numeric features of the prefix (the last three differences and two second
//!   differences), predicting the next difference; for formula tasks on the visible input.
//!
//! These predictors see only revealed examples (prefix, revealed next term; formula input, revealed
//! answer), exactly what the being logs.

use crate::being::discover::{forms, is_held_out, predict, Example, Form, K_TRAIN, K_VAL, MIN_SLACK};
use std::collections::BTreeMap;

/// Accepted forms per signature, smallest first.
pub type Book = BTreeMap<u8, Vec<Form>>;

#[derive(Clone, Copy, PartialEq)]
enum St {
    Na,
    Right,
    Wrong,
}

/// The B2 guard with predictor `pred` (examples in log order).
pub fn guarded_search(examples: &[Example], pred: &dyn Fn(&Form, &[i64]) -> Option<i64>) -> Book {
    let mut sigs: Vec<u8> = examples.iter().map(|e| e.sig).collect();
    sigs.sort_unstable();
    sigs.dedup();
    let mut book = Book::new();
    for sig in sigs {
        let ex: Vec<&Example> = examples.iter().filter(|e| e.sig == sig).collect();
        let mut covered = vec![false; ex.len()];
        for f in forms() {
            let st: Vec<St> = ex.iter().map(|e| match pred(&f, &e.terms) {
                None => St::Na,
                Some(v) if v == e.next => St::Right,
                Some(_) => St::Wrong,
            }).collect();
            if st.iter().any(|s| *s == St::Wrong) {
                continue;
            }
            let (mut tr, mut va) = (0u32, 0u32);
            for (i, s) in st.iter().enumerate() {
                if *s == St::Right && !covered[i] {
                    if is_held_out(i) { va += 1 } else { tr += 1 }
                }
            }
            if tr >= K_TRAIN && va >= K_VAL {
                for (i, s) in st.iter().enumerate() {
                    if *s == St::Right {
                        covered[i] = true;
                    }
                }
                book.entry(sig).or_default().push(f);
            }
        }
    }
    book
}

/// The answer of a book on a prefix: the first (smallest) accepted form that applies.
pub fn book_answer(book: &Book, sig: u8, terms: &[i64], pred: &dyn Fn(&Form, &[i64]) -> Option<i64>) -> Option<i64> {
    book.get(&sig)?.iter().find_map(|f| pred(f, terms))
}

/// Coefficient bounds of the brute-force grid.
pub const BF_LAG: i64 = 3;
pub const BF_CONST: i64 = 30;
pub const BF_POLY: i64 = 5;
pub const BF_POLY0: i64 = 60;

fn lags_of(f: &Form) -> Vec<usize> {
    match f {
        Form::Lin { mask, .. } => (1..=3usize).filter(|l| mask & (1 << (l - 1)) != 0).collect(),
        Form::Poly(_) => vec![],
    }
}

/// Brute-force per-task fit: every integer coefficient vector of the grid that reproduces all equations of
/// the prefix (at least `size + MIN_SLACK` equations); the prediction if all such vectors agree, else None.
pub fn bf_predict(f: &Form, t: &[i64]) -> Option<i64> {
    let len = t.len();
    let mut pred: Option<i64> = None;
    let mut agree = true;
    let mut note = |v: i64, pred: &mut Option<i64>| {
        match pred {
            None => *pred = Some(v),
            Some(p) if *p != v => agree = false,
            _ => {}
        }
    };
    match f {
        Form::Lin { constant, .. } => {
            let lags = lags_of(f);
            let m = *lags.last()?;
            if len <= m || len - m < f.size() + MIN_SLACK {
                return None;
            }
            let k = lags.len();
            let span = (2 * BF_LAG + 1) as usize;
            let mut a = vec![0i64; k];
            for code in 0..span.pow(k as u32) {
                let mut c = code;
                for x in a.iter_mut() {
                    *x = (c % span) as i64 - BF_LAG;
                    c /= span;
                }
                let lin = |i: usize| -> i128 { lags.iter().zip(&a).map(|(l, x)| *x as i128 * t[i - l] as i128).sum() };
                // the constant is fixed by the first equation, and must lie in the grid
                let b: i128 = if *constant { t[m] as i128 - lin(m) } else { 0 };
                if b.abs() > BF_CONST as i128 {
                    continue;
                }
                if (m..len).all(|i| lin(i) + b == t[i] as i128) {
                    let v = lin(len) + b;
                    if let Ok(v) = i64::try_from(v) {
                        note(v, &mut pred);
                    }
                }
            }
        }
        Form::Poly(d) => {
            let d = *d as usize;
            if len < d + 1 + MIN_SLACK {
                return None;
            }
            let span = (2 * BF_POLY + 1) as usize;
            let mut c = vec![0i64; d];
            let c0 = t[0];
            if c0.abs() > BF_POLY0 {
                return None;
            }
            for code in 0..span.pow(d as u32) {
                let mut z = code;
                for x in c.iter_mut() {
                    *x = (z % span) as i64 - BF_POLY;
                    z /= span;
                }
                let at = |n: usize| -> i128 { c0 as i128 + c.iter().enumerate().map(|(j, x)| *x as i128 * (n as i128).pow(j as u32 + 1)).sum::<i128>() };
                if (0..len).all(|n| at(n) == t[n] as i128) {
                    if let Ok(v) = i64::try_from(at(len)) {
                        note(v, &mut pred);
                    }
                }
            }
        }
    }
    if agree { pred } else { None }
}

/// The being's per-task fit (B2, exact rational elimination).
pub fn b2_predict(f: &Form, t: &[i64]) -> Option<i64> {
    predict(f, t)
}

// ---- numeric predictors ----

/// Features of a prefix: the last three first differences and the last two second differences.
pub fn seq_features(t: &[i64]) -> Option<[f64; 5]> {
    let n = t.len();
    if n < 4 {
        return None;
    }
    let d = |i: usize| (t[n - i] - t[n - i - 1]) as f64;
    let (d1, d2, d3) = (d(1), d(2), d(3));
    Some([d1, d2, d3, d1 - d2, d2 - d3])
}

/// k-NN regression of the next difference (Euclidean on `seq_features`), rounded; ties: earliest example.
pub fn knn_seq(train: &[Example], t: &[i64], k: usize) -> Option<i64> {
    let q = seq_features(t)?;
    let mut ds: Vec<(f64, usize, f64)> = train
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let f = seq_features(&e.terms)?;
            let dist = f.iter().zip(&q).map(|(a, b)| (a - b) * (a - b)).sum::<f64>();
            Some((dist, i, (e.next - e.terms[e.terms.len() - 1]) as f64))
        })
        .collect();
    if ds.is_empty() {
        return None;
    }
    ds.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
    let k = k.min(ds.len());
    let y = ds[..k].iter().map(|x| x.2).sum::<f64>() / k as f64;
    Some(t[t.len() - 1] + y.round() as i64)
}

/// A CART regression tree (squared error, binary splits at midpoints).
#[derive(Debug, Clone)]
pub enum Tree {
    Leaf(f64),
    Split { f: usize, thr: f64, lo: Box<Tree>, hi: Box<Tree> },
}

impl Tree {
    pub fn fit(x: &[Vec<f64>], y: &[f64], depth: usize, min_leaf: usize) -> Tree {
        let n = y.len();
        let mean = if n > 0 { y.iter().sum::<f64>() / n as f64 } else { 0.0 };
        if depth == 0 || n < 2 * min_leaf || y.iter().all(|v| (*v - y[0]).abs() < 1e-12) {
            return Tree::Leaf(mean);
        }
        let sse = |idx: &[usize]| -> f64 {
            if idx.is_empty() {
                return 0.0;
            }
            let m = idx.iter().map(|i| y[*i]).sum::<f64>() / idx.len() as f64;
            idx.iter().map(|i| (y[*i] - m).powi(2)).sum()
        };
        let all: Vec<usize> = (0..n).collect();
        let base = sse(&all);
        let mut best: Option<(f64, usize, f64)> = None;
        for f in 0..x[0].len() {
            let mut vals: Vec<f64> = x.iter().map(|r| r[f]).collect();
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
            vals.dedup();
            for w in vals.windows(2) {
                let thr = 0.5 * (w[0] + w[1]);
                let (lo, hi): (Vec<usize>, Vec<usize>) = all.iter().partition(|i| x[**i][f] <= thr);
                if lo.len() < min_leaf || hi.len() < min_leaf {
                    continue;
                }
                let s = sse(&lo) + sse(&hi);
                if s < base - 1e-9 && best.map_or(true, |b| s < b.0) {
                    best = Some((s, f, thr));
                }
            }
        }
        match best {
            None => Tree::Leaf(mean),
            Some((_, f, thr)) => {
                let (lo, hi): (Vec<usize>, Vec<usize>) = all.iter().partition(|i| x[**i][f] <= thr);
                let sub = |idx: &[usize]| -> (Vec<Vec<f64>>, Vec<f64>) { (idx.iter().map(|i| x[*i].clone()).collect(), idx.iter().map(|i| y[*i]).collect()) };
                let (xl, yl) = sub(&lo);
                let (xh, yh) = sub(&hi);
                Tree::Split { f, thr, lo: Box::new(Tree::fit(&xl, &yl, depth - 1, min_leaf)), hi: Box::new(Tree::fit(&xh, &yh, depth - 1, min_leaf)) }
            }
        }
    }
    pub fn predict(&self, q: &[f64]) -> f64 {
        match self {
            Tree::Leaf(v) => *v,
            Tree::Split { f, thr, lo, hi } => if q[*f] <= *thr { lo.predict(q) } else { hi.predict(q) },
        }
    }
}

pub const TREE_DEPTH: usize = 8;

/// A regression tree of the next difference on `seq_features` of the training examples.
pub fn tree_seq(train: &[Example]) -> Option<Tree> {
    let rows: Vec<(Vec<f64>, f64)> = train.iter().filter_map(|e| Some((seq_features(&e.terms)?.to_vec(), (e.next - e.terms[e.terms.len() - 1]) as f64))).collect();
    if rows.is_empty() {
        return None;
    }
    let (x, y): (Vec<Vec<f64>>, Vec<f64>) = rows.into_iter().unzip();
    Some(Tree::fit(&x, &y, TREE_DEPTH, 1))
}

pub fn tree_seq_answer(tree: &Tree, t: &[i64]) -> Option<i64> {
    let q = seq_features(t)?;
    Some(t[t.len() - 1] + tree.predict(&q).round() as i64)
}

/// Formula examples: (kind is fall, input x, revealed answer).
pub type FEx = (bool, i64, i64);

/// 1-NN on the input within the same kind: the answer of the nearest input (ties: earliest).
pub fn knn_formula(train: &[FEx], fall: bool, x: i64) -> Option<i64> {
    train.iter().filter(|e| e.0 == fall).min_by_key(|e| (e.1 - x).abs()).map(|e| e.2)
}

/// Regression tree of the answer on (kind, input).
pub fn tree_formula(train: &[FEx]) -> Option<Tree> {
    if train.is_empty() {
        return None;
    }
    let x: Vec<Vec<f64>> = train.iter().map(|e| vec![e.0 as u8 as f64, e.1 as f64]).collect();
    let y: Vec<f64> = train.iter().map(|e| e.2 as f64).collect();
    Some(Tree::fit(&x, &y, TREE_DEPTH, 1))
}
