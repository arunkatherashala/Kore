//! Column-at-a-time evaluation of filters and numeric expressions.
//!
//! The row interpreter (`executor::eval_expr`) allocates a `String` for every string cell it touches and
//! dispatches on the expression tree for every row. These routines walk the tree once per chunk of rows and
//! loop over the raw column data, with SQL three-valued logic for predicates. Chunks of `CHUNK` rows are
//! evaluated in parallel and keep their temporaries cache-sized. They return `None` for anything they do
//! not cover, and the caller falls back to the row interpreter.

use kore_core::{ColumnData, DataBlock};
use rayon::prelude::*;
use crate::ast::*;

const FALSE: u8 = 0;
const TRUE: u8 = 1;
const NULL: u8 = 2;
const CHUNK: usize = 65_536;

fn find_col<'a>(block: &'a DataBlock, name: &str) -> Option<&'a ColumnData> {
    block.columns.iter().find(|c| {
        c.name == name || {
            let (cn, nm) = (c.name.len(), name.len());
            cn > nm && c.name.as_bytes()[cn - nm - 1] == b'.' && &c.name[cn - nm..] == name
        }
    }).or_else(|| crate::executor::ci_find_col(block, name)).map(|c| &c.data)
}

fn col_of<'a>(e: &Expr, block: &'a DataBlock) -> Option<&'a ColumnData> {
    match e {
        Expr::Col(c) => find_col(block, c),
        Expr::QualCol(t, c) => find_col(block, &format!("{t}.{c}")),
        _ => None,
    }
}

/// Rows `lo..hi` of the block; operands are slices of that window, indexed from 0.
#[derive(Clone, Copy)]
struct Win { lo: usize, hi: usize }

impl Win {
    fn len(self) -> usize { self.hi - self.lo }
}

/// A numeric or string operand, borrowing column data where possible.
enum Val<'a> {
    I(&'a [Option<i64>]),
    F(&'a [Option<f64>]),
    Num(f64),
    /// integer literal (kept exact: BIGINT keys above 2^53 do not survive a trip through f64)
    NumI(i64),
    Owned(Vec<Option<f64>>),
    S(&'a [Option<String>]),
    D(&'a [u8], &'a [String]),
    Text(&'a str),
    /// SUBSTR(text operand, literal start, literal length) as a borrowed slice (no per-row String)
    Sub(Box<Val<'a>>, usize, Option<usize>),
}

impl Val<'_> {
    #[inline]
    fn num(&self, i: usize) -> Option<f64> {
        match self {
            Val::I(v) => v[i].map(|x| x as f64),
            Val::F(v) => v[i],
            Val::Num(c) => Some(*c),
            Val::NumI(c) => Some(*c as f64),
            Val::Owned(v) => v[i],
            _ => None,
        }
    }
    #[inline]
    fn text(&self, i: usize) -> Option<&str> {
        match self {
            Val::S(v) => v[i].as_deref(),
            Val::D(codes, dict) => if codes[i] == u8::MAX { None } else { dict.get(codes[i] as usize).map(|s| s.as_str()) },
            Val::Text(t) => Some(t),
            Val::Sub(inner, start, len) => inner.text(i).map(|s| sub_slice(s, *start, *len)),
            _ => None,
        }
    }
    fn is_num(&self) -> bool { matches!(self, Val::I(_) | Val::F(_) | Val::Num(_) | Val::NumI(_) | Val::Owned(_)) }
    /// exact integer value of an integer column / literal
    #[inline]
    fn int(&self, i: usize) -> Option<i64> {
        match self { Val::I(v) => v[i], Val::NumI(c) => Some(*c), _ => None }
    }
    fn is_int(&self) -> bool { matches!(self, Val::I(_) | Val::NumI(_)) }
    fn is_text(&self) -> bool { matches!(self, Val::S(_) | Val::D(..) | Val::Text(_) | Val::Sub(..)) }
}

/// Characters `start..start+len` of `s` (0-based start) as a slice.
#[inline]
fn sub_slice(s: &str, start: usize, len: Option<usize>) -> &str {
    let b = s.as_bytes();
    // ASCII prefix: byte offsets are character offsets
    let probe = match len { Some(l) => (start + l).min(b.len()), None => b.len() };
    if b[..probe].is_ascii() {
        let lo = start.min(b.len());
        let hi = match len { Some(l) => (start + l).min(b.len()), None => b.len() };
        return &s[lo..hi.max(lo)];
    }
    let mut it = s.char_indices();
    let lo = match it.nth(start) { Some((i, _)) => i, None => return "" };
    match len {
        None => &s[lo..],
        Some(0) => "",
        Some(l) => {
            let mut hi = s.len();
            let mut taken = 1;
            for (i, _) in s[lo..].char_indices().skip(1) { if taken == l { hi = lo + i; break; } taken += 1; }
            &s[lo..hi]
        }
    }
}

fn operand<'a>(e: &'a Expr, block: &'a DataBlock, w: Win, c: &Cache) -> Option<Val<'a>> {
    match e {
        Expr::Int(i) => Some(Val::NumI(*i)),
        Expr::Float(f) => Some(Val::Num(*f)),
        Expr::Str(s) => Some(Val::Text(s.as_str())),
        Expr::Col(_) | Expr::QualCol(..) => match col_of(e, block)? {
            ColumnData::Int64(v) => Some(Val::I(&v[w.lo..w.hi])),
            ColumnData::Float64(v) => Some(Val::F(&v[w.lo..w.hi])),
            ColumnData::Str(v) => Some(Val::S(&v[w.lo..w.hi])),
            ColumnData::StrDict { codes, dict } => Some(Val::D(&codes[w.lo..w.hi], dict)),
            ColumnData::Bool(_) => None,
        },
        Expr::FuncCall { name, args } if (name.eq_ignore_ascii_case("SUBSTR") || name.eq_ignore_ascii_case("SUBSTRING")) && (args.len() == 2 || args.len() == 3) => {
            let inner = operand(&args[0], block, w, c)?;
            if !matches!(inner, Val::S(_) | Val::D(..)) { return None; }
            let Expr::Int(start) = &args[1] else { return None };
            if *start < 1 { return None; }
            let len = match args.get(2) { None => None, Some(Expr::Int(l)) => Some((*l).max(0) as usize), _ => return None };
            Some(Val::Sub(Box::new(inner), (*start - 1) as usize, len))
        }
        Expr::BinOp { op: BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod, .. }
        | Expr::Case { .. } => num_win(e, block, w, c).map(Val::Owned),
        _ => None,
    }
}

fn cmp_ok(op: &BinOpKind, a: f64, b: f64) -> bool {
    match op {
        BinOpKind::Eq => (a - b).abs() < 1e-10,
        BinOpKind::Ne => (a - b).abs() >= 1e-10,
        BinOpKind::Lt => a < b,
        BinOpKind::Le => a <= b,
        BinOpKind::Gt => a > b,
        BinOpKind::Ge => a >= b,
        _ => false,
    }
}

fn cmp_text(op: &BinOpKind, a: &str, b: &str) -> bool {
    match op {
        BinOpKind::Eq => a == b,
        BinOpKind::Ne => a != b,
        BinOpKind::Lt => a < b,
        BinOpKind::Le => a <= b,
        BinOpKind::Gt => a > b,
        BinOpKind::Ge => a >= b,
        _ => false,
    }
}

fn cmp_int(op: &BinOpKind, a: i64, b: i64) -> bool {
    match op {
        BinOpKind::Eq => a == b, BinOpKind::Ne => a != b, BinOpKind::Lt => a < b,
        BinOpKind::Le => a <= b, BinOpKind::Gt => a > b, BinOpKind::Ge => a >= b, _ => false,
    }
}

/// Column-versus-literal comparisons with the operator resolved once per chunk (not once per row).
fn compare_lit(op: &BinOpKind, l: &Val, r: &Val) -> Option<Vec<u8>> {
    #[inline(always)]
    fn run_s<F: Fn(&str) -> bool>(v: &[Option<String>], f: F) -> Vec<u8> {
        v.iter().map(|x| match x { Some(s) => f(s.as_str()) as u8, None => NULL }).collect()
    }
    #[inline(always)]
    fn run_f<F: Fn(f64) -> bool>(v: &[Option<f64>], f: F) -> Vec<u8> {
        v.iter().map(|x| match x { Some(a) => f(*a) as u8, None => NULL }).collect()
    }
    #[inline(always)]
    fn run_i<F: Fn(i64) -> bool>(v: &[Option<i64>], f: F) -> Vec<u8> {
        v.iter().map(|x| match x { Some(a) => f(*a) as u8, None => NULL }).collect()
    }
    match (l, r) {
        (Val::S(v), Val::Text(t)) => {
            let t = *t;
            Some(match op {
                BinOpKind::Eq => run_s(v, |s| s == t), BinOpKind::Ne => run_s(v, |s| s != t),
                BinOpKind::Lt => run_s(v, |s| s < t), BinOpKind::Le => run_s(v, |s| s <= t),
                BinOpKind::Gt => run_s(v, |s| s > t), BinOpKind::Ge => run_s(v, |s| s >= t),
                _ => return None,
            })
        }
        (Val::D(codes, dict), Val::Text(t)) => {
            // one comparison per dictionary entry, then a table lookup per row
            let mut table = [NULL; 256];
            for (i, d) in dict.iter().enumerate().take(255) { table[i] = cmp_text(op, d.as_str(), t) as u8; }
            Some(codes.iter().map(|&c| table[c as usize]).collect())
        }
        (Val::I(v), Val::NumI(c)) => {
            let c = *c;
            Some(match op {
                BinOpKind::Eq => run_i(v, |a| a == c), BinOpKind::Ne => run_i(v, |a| a != c),
                BinOpKind::Lt => run_i(v, |a| a < c), BinOpKind::Le => run_i(v, |a| a <= c),
                BinOpKind::Gt => run_i(v, |a| a > c), BinOpKind::Ge => run_i(v, |a| a >= c),
                _ => return None,
            })
        }
        (Val::F(v), Val::Num(_) | Val::NumI(_)) => {
            let c = match r { Val::Num(c) => *c, Val::NumI(c) => *c as f64, _ => unreachable!() };
            Some(match op {
                BinOpKind::Eq => run_f(v, |a| (a - c).abs() < 1e-10), BinOpKind::Ne => run_f(v, |a| (a - c).abs() >= 1e-10),
                BinOpKind::Lt => run_f(v, |a| a < c), BinOpKind::Le => run_f(v, |a| a <= c),
                BinOpKind::Gt => run_f(v, |a| a > c), BinOpKind::Ge => run_f(v, |a| a >= c),
                _ => return None,
            })
        }
        _ => None,
    }
}

fn compare(op: &BinOpKind, l: &Val, r: &Val, n: usize) -> Option<Vec<u8>> {
    if let Some(v) = compare_lit(op, l, r) { return Some(v); }
    if l.is_int() && r.is_int() {
        return Some((0..n).map(|i| match (l.int(i), r.int(i)) {
            (Some(a), Some(b)) => if cmp_int(op, a, b) { TRUE } else { FALSE },
            _ => NULL,
        }).collect());
    }
    if l.is_num() && r.is_num() {
        Some((0..n).map(|i| match (l.num(i), r.num(i)) {
            (Some(a), Some(b)) => if cmp_ok(op, a, b) { TRUE } else { FALSE },
            _ => NULL,
        }).collect())
    } else if l.is_text() && r.is_text() {
        Some((0..n).map(|i| match (l.text(i), r.text(i)) {
            (Some(a), Some(b)) => if cmp_text(op, a, b) { TRUE } else { FALSE },
            _ => NULL,
        }).collect())
    } else {
        None
    }
}

/// Per-call cache of IN-list hash sets, keyed by the address of the list inside the predicate being evaluated
/// (valid for the duration of one top-level call), so each set is built once instead of once per chunk.
#[derive(Default)]
struct Cache {
    ints: std::sync::Mutex<std::collections::HashMap<usize, std::sync::Arc<std::collections::HashSet<i64>>>>,
    nums: std::sync::Mutex<std::collections::HashMap<usize, std::sync::Arc<std::collections::HashSet<u64>>>>,
}

/// LIKE pattern made only of literal text and `%` (no `_`, no escapes): matched with substring search
/// instead of the byte-at-a-time wildcard matcher.
struct LikePlan { segs: Vec<String>, anchored_start: bool, anchored_end: bool }

impl LikePlan {
    fn new(p: &str) -> Option<LikePlan> {
        if p.is_empty() || p.contains('_') || p.contains('\\') { return None; }
        Some(LikePlan {
            segs: p.split('%').filter(|s| !s.is_empty()).map(str::to_string).collect(),
            anchored_start: !p.starts_with('%'),
            anchored_end: !p.ends_with('%'),
        })
    }
    fn matches(&self, s: &str) -> bool {
        let n = self.segs.len();
        if n == 0 { return true; } // only '%'
        let mut rest = s;
        let mut first = 0;
        let mut last = n;
        if self.anchored_start {
            let Some(r) = rest.strip_prefix(self.segs[0].as_str()) else { return false };
            rest = r;
            first = 1;
        }
        if self.anchored_end {
            if n == 1 && self.anchored_start { return rest.is_empty(); }
            let Some(r) = rest.strip_suffix(self.segs[n - 1].as_str()) else { return false };
            rest = r;
            last = n - 1;
        }
        for seg in &self.segs[first..last] {
            match rest.find(seg.as_str()) {
                Some(pos) => rest = &rest[pos + seg.len()..],
                None => return false,
            }
        }
        true
    }
}

fn not(v: Vec<u8>) -> Vec<u8> {
    v.into_iter().map(|x| match x { TRUE => FALSE, FALSE => TRUE, o => o }).collect()
}

fn and3(l: &[u8], r: &[u8]) -> Vec<u8> {
    l.iter().zip(r).map(|(&a, &b)| if a == FALSE || b == FALSE { FALSE } else if a == TRUE && b == TRUE { TRUE } else { NULL }).collect()
}

/// Three-valued truth value of `e` for the rows in `w`, or None if the expression is not covered.
fn tri<'a>(e: &'a Expr, block: &'a DataBlock, w: Win, c: &Cache) -> Option<Vec<u8>> {
    let n = w.len();
    match e {
        Expr::Bool(b) => Some(vec![if *b { TRUE } else { FALSE }; n]),
        Expr::Not(x) => Some(not(tri(x, block, w, c)?)),
        Expr::BinOp { op: BinOpKind::And, left, right } => Some(and3(&tri(left, block, w, c)?, &tri(right, block, w, c)?)),
        Expr::BinOp { op: BinOpKind::Or, left, right } => {
            let (l, r) = (tri(left, block, w, c)?, tri(right, block, w, c)?);
            Some(l.iter().zip(&r).map(|(&a, &b)| if a == TRUE || b == TRUE { TRUE } else if a == FALSE && b == FALSE { FALSE } else { NULL }).collect())
        }
        Expr::BinOp { op: op @ (BinOpKind::Eq | BinOpKind::Ne | BinOpKind::Lt | BinOpKind::Le | BinOpKind::Gt | BinOpKind::Ge), left, right } => {
            let (l, r) = (operand(left, block, w, c)?, operand(right, block, w, c)?);
            compare(op, &l, &r, n)
        }
        Expr::Between { expr, low, high, negated } => {
            let (v, lo, hi) = (operand(expr, block, w, c)?, operand(low, block, w, c)?, operand(high, block, w, c)?);
            let ge = compare(&BinOpKind::Ge, &v, &lo, n)?;
            let le = compare(&BinOpKind::Le, &v, &hi, n)?;
            let both = and3(&ge, &le);
            Some(if *negated { not(both) } else { both })
        }
        Expr::In { expr, values, negated } => {
            let v = operand(expr, block, w, c)?;
            let key_id = values.as_ptr() as usize;
            let int_set: Option<std::sync::Arc<std::collections::HashSet<i64>>> = if v.is_int() {
                let cached = c.ints.lock().unwrap().get(&key_id).cloned();
                match cached {
                    Some(h) => Some(h),
                    None => values.iter().map(|x| if let Expr::Int(i) = x { Some(*i) } else { None }).collect::<Option<std::collections::HashSet<i64>>>()
                        .map(|h| { let h = std::sync::Arc::new(h); c.ints.lock().unwrap().insert(key_id, h.clone()); h }),
                }
            } else { None };
            let out: Vec<u8> = if let Some(hs) = int_set {
                (0..n).map(|i| match v.int(i) { None => NULL, Some(a) => if hs.contains(&a) { TRUE } else { FALSE } }).collect()
            } else if v.is_num() {
                let key = |f: f64| if f == 0.0 { 0.0f64.to_bits() } else { f.to_bits() };
                let cached = c.nums.lock().unwrap().get(&key_id).cloned();
                let big: Option<std::sync::Arc<std::collections::HashSet<u64>>> = match cached {
                    Some(h) => Some(h),
                    None if values.len() > 8 => {
                        let mut hs = std::collections::HashSet::with_capacity(values.len());
                        for x in values {
                            match x { Expr::Int(i) => { hs.insert(key(*i as f64)); } Expr::Float(f) => { hs.insert(key(*f)); } _ => return None }
                        }
                        let h = std::sync::Arc::new(hs);
                        c.nums.lock().unwrap().insert(key_id, h.clone());
                        Some(h)
                    }
                    None => None,
                };
                let mut set = Vec::new();
                if big.is_none() {
                    for x in values {
                        match x { Expr::Int(i) => set.push(*i as f64), Expr::Float(f) => set.push(*f), _ => return None }
                    }
                }
                if let Some(hs) = big {
                    // long lists (decorrelated IN subqueries): exact-value hash set instead of a linear scan
                    (0..n).map(|i| match v.num(i) {
                        None => NULL,
                        Some(a) => if hs.contains(&key(a)) { TRUE } else { FALSE },
                    }).collect()
                } else {
                    (0..n).map(|i| match v.num(i) {
                        None => NULL,
                        Some(a) => if set.iter().any(|b| (a - b).abs() < 1e-10) { TRUE } else { FALSE },
                    }).collect()
                }
            } else if v.is_text() {
                let mut set: Vec<&str> = Vec::with_capacity(values.len());
                for x in values {
                    match x { Expr::Str(s) => set.push(s.as_str()), _ => return None }
                }
                if set.len() <= 16 {
                    (0..n).map(|i| match v.text(i) {
                        None => NULL,
                        Some(a) => if set.iter().any(|b| *b == a) { TRUE } else { FALSE },
                    }).collect()
                } else {
                    let hs: std::collections::HashSet<&str> = set.into_iter().collect();
                    (0..n).map(|i| match v.text(i) {
                        None => NULL,
                        Some(a) => if hs.contains(a) { TRUE } else { FALSE },
                    }).collect()
                }
            } else {
                return None;
            };
            Some(if *negated { not(out) } else { out })
        }
        Expr::Like { expr, pattern, negated } => {
            let (Expr::Str(p), v) = (pattern.as_ref(), operand(expr, block, w, c)?) else { return None };
            if !v.is_text() { return None; }
            let plan = LikePlan::new(p);
            let matches = |s: &str| match &plan {
                Some(pl) => pl.matches(s),
                None => crate::executor::like_match(s, p),
            };
            let out: Vec<u8> = if let Val::D(codes, dict) = &v {
                // dictionary column: decide once per distinct value
                let table: Vec<u8> = dict.iter().map(|s| if matches(s) { TRUE } else { FALSE }).collect();
                codes.iter().map(|&c| if c == u8::MAX { NULL } else { table.get(c as usize).copied().unwrap_or(NULL) }).collect()
            } else {
                (0..n).map(|i| match v.text(i) {
                    None => NULL,
                    Some(s) => if matches(s) { TRUE } else { FALSE },
                }).collect()
            };
            Some(if *negated { not(out) } else { out })
        }
        Expr::IsNull(x) | Expr::IsNotNull(x) => {
            let want_null = matches!(e, Expr::IsNull(_));
            let pick = |is_null: bool| if is_null == want_null { TRUE } else { FALSE };
            Some(match col_of(x, block)? {
                ColumnData::Int64(v) => v[w.lo..w.hi].iter().map(|c| pick(c.is_none())).collect(),
                ColumnData::Float64(v) => v[w.lo..w.hi].iter().map(|c| pick(c.is_none())).collect(),
                ColumnData::Bool(v) => v[w.lo..w.hi].iter().map(|c| pick(c.is_none())).collect(),
                ColumnData::Str(v) => v[w.lo..w.hi].iter().map(|c| pick(c.is_none())).collect(),
                ColumnData::StrDict { codes, .. } => codes[w.lo..w.hi].iter().map(|&c| pick(c == u8::MAX)).collect(),
            })
        }
        _ => None,
    }
}

fn windows(n: usize) -> Vec<Win> {
    (0..n).step_by(CHUNK).map(|lo| Win { lo, hi: (lo + CHUNK).min(n) }).collect()
}

/// Rows for which the predicate is TRUE (NULL counts as not matching), or None when not covered.
pub fn filter_mask(pred: &Expr, block: &DataBlock) -> Option<Vec<bool>> {
    if crate::testing::no_vecexpr() { return None; }
    let n = block.num_rows;
    let wins = windows(n);
    // decide coverage on the first chunk (cheap) before fanning out
    let c = &Cache::default();
    let first = tri(pred, block, *wins.first()?, c)?;
    let mut parts: Vec<Vec<bool>> = vec![first.into_iter().map(|v| v == TRUE).collect()];
    let rest: Vec<Option<Vec<bool>>> = wins[1..].par_iter()
        .map(|&w| tri(pred, block, w, c).map(|v| v.into_iter().map(|x| x == TRUE).collect()))
        .collect();
    for r in rest { parts.push(r?); }
    let mut out = Vec::with_capacity(n);
    for p in parts { out.extend(p); }
    Some(out)
}

/// Indices of the rows for which the predicate is TRUE, ascending; None when not covered.
pub fn filter_idx(pred: &Expr, block: &DataBlock) -> Option<Vec<usize>> {
    let wins = windows(block.num_rows);
    let pick = |w: Win, v: Vec<u8>| -> Vec<usize> {
        // exact capacity: no doubling reallocations (large allocations are expensive page-fault wise)
        let cnt = v.iter().filter(|&&x| x == TRUE).count();
        let mut out = Vec::with_capacity(cnt);
        if cnt == v.len() { out.extend(w.lo..w.hi); return out; }
        if cnt > 0 { for (i, &x) in v.iter().enumerate() { if x == TRUE { out.push(w.lo + i); } } }
        out
    };
    let c = &Cache::default();
    let first = pick(*wins.first()?, tri(pred, block, *wins.first()?, c)?);
    let rest: Vec<Option<Vec<usize>>> = wins[1..].par_iter().map(|&w| tri(pred, block, w, c).map(|v| pick(w, v))).collect();
    let mut parts = vec![first];
    for r in rest { parts.push(r?); }
    let total = parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(total);
    for p in parts { out.extend(p); }
    Some(out)
}

fn num_win(e: &Expr, block: &DataBlock, w: Win, c: &Cache) -> Option<Vec<Option<f64>>> {
    let n = w.len();
    match e {
        Expr::Int(i) => Some(vec![Some(*i as f64); n]),
        Expr::Float(f) => Some(vec![Some(*f); n]),
        Expr::Null => Some(vec![None; n]),
        Expr::Col(_) | Expr::QualCol(..) => match col_of(e, block)? {
            ColumnData::Int64(v) => Some(v[w.lo..w.hi].iter().map(|x| x.map(|i| i as f64)).collect()),
            ColumnData::Float64(v) => Some(v[w.lo..w.hi].to_vec()),
            _ => None,
        },
        Expr::BinOp { op: op @ (BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod), left, right } => {
            let (l, r) = (num_win(left, block, w, c)?, num_win(right, block, w, c)?);
            Some(l.iter().zip(&r).map(|(a, b)| match (a, b) {
                // x / 0 and x % 0 are NULL (Spark semantics), not inf / NaN
                (Some(_), Some(b)) if *b == 0.0 && matches!(op, BinOpKind::Div | BinOpKind::Mod) => None,
                (Some(a), Some(b)) => Some(match op {
                    BinOpKind::Add => a + b,
                    BinOpKind::Sub => a - b,
                    BinOpKind::Mul => a * b,
                    BinOpKind::Div => a / b,
                    _ => a % b,
                }),
                _ => None,
            }).collect())
        }
        Expr::Case { operand: None, branches, else_val } => {
            let mut out: Vec<Option<f64>> = vec![None; n];
            let mut decided = vec![false; n];
            for (cond, val) in branches {
                let cv = tri(cond, block, w, c)?;
                let v = num_win(val, block, w, c)?;
                for i in 0..n {
                    if !decided[i] && cv[i] == TRUE {
                        decided[i] = true;
                        out[i] = v[i];
                    }
                }
            }
            if let Some(ev) = else_val {
                let v = num_win(ev, block, w, c)?;
                for i in 0..n {
                    if !decided[i] { out[i] = v[i]; }
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Numeric value of `e` for every row (NULL = None), or None when the expression is not covered.
/// Mirrors the row interpreter: arithmetic yields floats and propagates NULL.
pub fn num_vec(e: &Expr, block: &DataBlock) -> Option<Vec<Option<f64>>> {
    if crate::testing::no_vecexpr() { return None; }
    let n = block.num_rows;
    let wins = windows(n);
    let c = &Cache::default();
    let first = num_win(e, block, *wins.first()?, c)?;
    let rest: Vec<Option<Vec<Option<f64>>>> = wins[1..].par_iter().map(|&w| num_win(e, block, w, c)).collect();
    let mut out = Vec::with_capacity(n);
    out.extend(first);
    for r in rest { out.extend(r?); }
    Some(out)
}

/// Numeric value of `e` for rows `lo..hi` only (NULL = None); None when the expression is not covered.
pub fn num_range(e: &Expr, block: &DataBlock, lo: usize, hi: usize) -> Option<Vec<Option<f64>>> {
    num_win(e, block, Win { lo, hi }, &Cache::default())
}

#[cfg(test)]
mod like_plan_tests {
    use super::LikePlan;

    /// The substring-search plan must agree with the general wildcard matcher on every pattern it accepts.
    #[test]
    fn like_plan_agrees_with_like_match() {
        let pats = ["%", "a", "a%", "%a", "%a%", "a%b", "a%a", "%a%b%", "ab%ab", "%special%requests%", "x%y%z", "%%a%%", "aa%aa", "é%"];
        let vals = ["", "a", "b", "aa", "ab", "aba", "abab", "ba", "xyz", "xxyyzz", "zyx", "special requests", "a special request", "requests special",
                    "special  requests!", "aaa", "aaaa", "éa", "a%b"];
        for p in pats {
            let plan = LikePlan::new(p).expect("plain pattern");
            for v in vals {
                assert_eq!(plan.matches(v), crate::executor::like_match(v, p), "pattern {p:?} value {v:?}");
            }
        }
        assert!(LikePlan::new("a_b").is_none() && LikePlan::new("").is_none());
    }
}
