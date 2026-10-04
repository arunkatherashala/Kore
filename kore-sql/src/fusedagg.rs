//! Fused scan -> filter -> group-by aggregation over a borrowed table.
//!
//! The classic path copies the surviving rows of every needed column into a new block and only then
//! aggregates. Here the predicate is evaluated per window of rows (cache sized), the surviving row numbers form a
//! selection vector, and the aggregates read the source columns through it, so the filtered block is never built.
//! Shared sub-expressions (`l_extendedprice * (1 - l_discount)` feeds two sums) are evaluated once per window.
//!
//! Two execution modes, both producing groups in order of first appearance:
//!  * few groups: every thread aggregates a contiguous row range into private accumulators, merged at the end;
//!  * many groups (GROUP BY orderkey): rows are hash-partitioned by key first and each partition is aggregated by
//!    one thread into its own table, so there is no single-threaded merge of millions of groups.
//!
//! Anything outside the supported shapes returns None and the caller runs the ordinary path.

use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use rayon::prelude::*;
use kore_core::{Column, ColumnData, DataBlock};
use crate::ast::*;
use crate::executor::{find_col, is_null_at, FxHasher};
use crate::vecexpr::{num_window, tri_window, WinCtx};

const WIN: usize = 8192;
const PART_BITS: u32 = 6;
const NPART: usize = 1 << PART_BITS;
/// Tables smaller than this keep using the ordinary path.
pub(crate) const MIN_ROWS: usize = 4096;

type Key = (i64, i64, i64, u8);
type KeyMap = HashMap<Key, u32, BuildHasherDefault<FxHasher>>;

enum KeyPart<'a> { I(&'a [Option<i64>]), D(&'a [u8]), O(Vec<Option<i64>>) }

impl KeyPart<'_> {
    /// (value, is_null)
    #[inline(always)]
    fn get(&self, r: usize) -> (i64, bool) {
        match self {
            KeyPart::I(v) => match v[r] { Some(x) => (x, false), None => (0, true) },
            KeyPart::D(c) => if c[r] == u8::MAX { (0, true) } else { (c[r] as i64, false) },
            KeyPart::O(v) => match v[r] { Some(x) => (x, false), None => (0, true) },
        }
    }
}

#[inline(always)]
fn key_of(parts: &[KeyPart], r: usize) -> Key {
    let mut key: Key = (0, 0, 0, 0);
    for (i, p) in parts.iter().enumerate() {
        let (v, null) = p.get(r);
        match i { 0 => key.0 = v, 1 => key.1 = v, _ => key.2 = v }
        if null { key.3 |= 1 << i; }
    }
    key
}

#[inline(always)]
fn key_hash(k: &Key) -> u64 {
    let mut h = (k.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= (k.1 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F).rotate_left(21);
    h ^= (k.2 as u64).wrapping_mul(0x1656_67B1_9E37_79F9).rotate_left(42);
    h ^= (k.3 as u64) << 56;
    h ^= h >> 31;
    h.wrapping_mul(0xD6E8_FEB8_6659_FD93)
}

// ─── expression plan with shared sub-expressions ───────────────────────────────────────────────

enum PN<'a> { ColF(&'a [Option<f64>]), ColI(&'a [Option<i64>]), Const(f64), Bin(BinOpKind, usize, usize), Opaque(&'a Expr) }

/// A node's values for one window.
enum R<'a> { B(&'a [Option<f64>]), O(Vec<Option<f64>>), C(f64) }

impl R<'_> {
    #[inline(always)]
    fn at(&self, i: usize) -> Option<f64> {
        match self { R::B(v) => v[i], R::O(v) => v[i], R::C(c) => Some(*c) }
    }
}

struct Plan<'a> { nodes: Vec<PN<'a>>, memo: HashMap<String, usize> }

impl<'a> Plan<'a> {
    fn intern(&mut self, e: &'a Expr, block: &'a DataBlock) -> Option<usize> {
        let (key, node) = match e {
            Expr::Int(i) => (format!("k:{}", *i as f64), PN::Const(*i as f64)),
            Expr::Float(f) => (format!("k:{f}"), PN::Const(*f)),
            Expr::Col(_) | Expr::QualCol(..) => {
                let name = match e { Expr::Col(c) => c.clone(), Expr::QualCol(t, c) => format!("{t}.{c}"), _ => unreachable!() };
                let col = find_col(block, &name)?;
                let node = match &col.data {
                    ColumnData::Float64(v) => PN::ColF(v),
                    ColumnData::Int64(v) => PN::ColI(v),
                    _ => return None,
                };
                (format!("c:{}", col.name), node)
            }
            Expr::BinOp { op: op @ (BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod), left, right } => {
                let (a, b) = (self.intern(left, block)?, self.intern(right, block)?);
                (format!("b:{op:?}:{a}:{b}"), PN::Bin(op.clone(), a, b))
            }
            other => {
                num_window(other, block, 0, 1.min(block.num_rows), &WinCtx::new())?;
                (format!("o:{other:?}"), PN::Opaque(other))
            }
        };
        if let Some(&id) = self.memo.get(&key) { return Some(id); }
        self.nodes.push(node);
        self.memo.insert(key, self.nodes.len() - 1);
        Some(self.nodes.len() - 1)
    }
}

#[inline(always)]
fn apply(op: &BinOpKind, a: f64, b: f64) -> Option<f64> {
    match op {
        // x / 0 and x % 0 are NULL (Spark semantics), not inf / NaN
        BinOpKind::Div => if b == 0.0 { None } else { Some(a / b) },
        BinOpKind::Mod => if b == 0.0 { None } else { Some(a % b) },
        BinOpKind::Add => Some(a + b),
        BinOpKind::Sub => Some(a - b),
        _ => Some(a * b),
    }
}

fn bin_loop(a: &R, b: &R, n: usize, f: impl Fn(f64, f64) -> Option<f64>) -> Vec<Option<f64>> {
    fn sl<'x>(r: &'x R) -> Option<&'x [Option<f64>]> { match r { R::B(v) => Some(v), R::O(v) => Some(v), R::C(_) => None } }
    match (sl(a), sl(b)) {
        (Some(x), Some(y)) => x.iter().zip(y).map(|(p, q)| match (p, q) { (Some(p), Some(q)) => f(*p, *q), _ => None }).collect(),
        (Some(x), None) => { let c = if let R::C(c) = b { *c } else { 0.0 }; x.iter().map(|p| p.and_then(|p| f(p, c))).collect() }
        (None, Some(y)) => { let c = if let R::C(c) = a { *c } else { 0.0 }; y.iter().map(|q| q.and_then(|q| f(c, q))).collect() }
        (None, None) => {
            let (R::C(p), R::C(q)) = (a, b) else { unreachable!() };
            vec![f(*p, *q); n]
        }
    }
}

fn bin(op: &BinOpKind, a: &R, b: &R, n: usize) -> Vec<Option<f64>> {
    match op {
        BinOpKind::Add => bin_loop(a, b, n, |x, y| Some(x + y)),
        BinOpKind::Sub => bin_loop(a, b, n, |x, y| Some(x - y)),
        BinOpKind::Mul => bin_loop(a, b, n, |x, y| Some(x * y)),
        _ => bin_loop(a, b, n, |x, y| apply(op, x, y)),
    }
}

fn eval_nodes<'a>(nodes: &[PN<'a>], block: &DataBlock, lo: usize, hi: usize, wc: &WinCtx) -> Option<Vec<R<'a>>> {
    let n = hi - lo;
    let mut res: Vec<R<'a>> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let r = match node {
            PN::ColF(v) => R::B(&v[lo..hi]),
            PN::ColI(v) => R::O(v[lo..hi].iter().map(|x| x.map(|i| i as f64)).collect()),
            PN::Const(c) => R::C(*c),
            PN::Bin(op, a, b) => R::O(bin(op, &res[*a], &res[*b], n)),
            PN::Opaque(e) => R::O(num_window(e, block, lo, hi, wc)?),
        };
        res.push(r);
    }
    Some(res)
}

// ─── aggregate specification ───────────────────────────────────────────────────────────────────

enum Src<'a> {
    /// COUNT(*)
    Star,
    /// COUNT(col)
    NonNull(&'a Column),
    ColF(&'a [Option<f64>]),
    ColI(&'a [Option<i64>]),
    /// index into the plan nodes
    Node(usize),
}

struct Spec<'a> { func: &'a AggFunc, src: Src<'a> }

enum Out<'a> {
    Key(&'a Column, Option<&'a String>),
    Agg { func: &'a AggFunc, name: String, integral: bool },
}

#[inline]
fn init(f: &AggFunc) -> f64 { match f { AggFunc::Min => f64::INFINITY, AggFunc::Max => f64::NEG_INFINITY, _ => 0.0 } }

#[inline(always)]
fn accum(func: &AggFunc, sel: &[u32], gid: &[u32], get: impl Fn(usize) -> Option<f64>, acc: &mut [f64], cnt: &mut [u64]) {
    match func {
        AggFunc::Min => for (j, &i) in sel.iter().enumerate() {
            if let Some(x) = get(i as usize) { let g = gid[j] as usize; cnt[g] += 1; if x < acc[g] { acc[g] = x } }
        },
        AggFunc::Max => for (j, &i) in sel.iter().enumerate() {
            if let Some(x) = get(i as usize) { let g = gid[j] as usize; cnt[g] += 1; if x > acc[g] { acc[g] = x } }
        },
        _ => for (j, &i) in sel.iter().enumerate() {
            if let Some(x) = get(i as usize) { let g = gid[j] as usize; cnt[g] += 1; acc[g] += x; }
        },
    }
}

/// Per-thread partial aggregates.
struct Local {
    keys: Vec<Key>,
    first: Vec<usize>,
    acc: Vec<Vec<f64>>,
    cnt: Vec<Vec<u64>>,
    index: KeyMap,
    /// dictionary-coded keys (at most two): direct table instead of hashing
    direct: Option<Vec<u32>>,
}

impl Local {
    fn new(nspecs: usize, direct: bool) -> Local {
        Local {
            keys: Vec::new(), first: Vec::new(), acc: vec![Vec::new(); nspecs], cnt: vec![Vec::new(); nspecs],
            index: HashMap::default(), direct: if direct { Some(vec![u32::MAX; 1 << 16]) } else { None },
        }
    }
    #[inline(always)]
    fn new_group(&mut self, key: Key, row: usize, specs: &[Spec]) -> u32 {
        let id = self.keys.len() as u32;
        self.keys.push(key);
        self.first.push(row);
        for (si, s) in specs.iter().enumerate() { self.acc[si].push(init(s.func)); self.cnt[si].push(0); }
        id
    }
}

/// Everything needed to aggregate: borrowed columns, the compiled plan, the output layout.
struct Job<'a> {
    block: &'a DataBlock,
    pred: Option<&'a Expr>,
    parts: Vec<KeyPart<'a>>,
    specs: Vec<Spec<'a>>,
    nodes: Vec<PN<'a>>,
    /// all key parts are dictionary codes and there are at most two: index = code0 | code1 << 8
    dict_keys: bool,
}

impl<'a> Job<'a> {
    /// Selected rows of window lo..hi (offsets relative to lo); None when the predicate is not covered.
    fn select(&self, lo: usize, hi: usize, wc: &WinCtx, sel: &mut Vec<u32>) -> Option<()> {
        sel.clear();
        match self.pred {
            None => sel.extend(0..(hi - lo) as u32),
            Some(p) => {
                let m = tri_window(p, self.block, lo, hi, wc)?;
                sel.extend(m.iter().enumerate().filter_map(|(i, &x)| if x == 1 { Some(i as u32) } else { None }));
            }
        }
        Some(())
    }

    /// Fold window lo..hi into `l`.
    fn window(&self, lo: usize, hi: usize, wc: &WinCtx, l: &mut Local, sel: &mut Vec<u32>, gid: &mut Vec<u32>) -> Option<()> {
        self.select(lo, hi, wc, sel)?;
        if sel.is_empty() { return Some(()); }
        gid.clear();
        if self.parts.is_empty() {
            if l.keys.is_empty() { l.new_group((0, 0, 0, 0), lo + sel[0] as usize, &self.specs); }
            gid.resize(sel.len(), 0);
        } else if self.dict_keys {
            // Hoist the match on the number of key columns out of the row loop.
            for &i in sel.iter() {
                let r = lo + i as usize;
                let (c0, n0) = self.parts[0].get(r);
                let (c1, n1) = if self.parts.len() > 1 { self.parts[1].get(r) } else { (0, false) };
                let slot = (if n0 { 255 } else { c0 as usize }) | ((if n1 { 255 } else { c1 as usize }) << 8);
                let mut g = l.direct.as_ref().unwrap()[slot];
                if g == u32::MAX {
                    let key = key_of(&self.parts, r);
                    g = l.new_group(key, r, &self.specs);
                    l.direct.as_mut().unwrap()[slot] = g;
                }
                gid.push(g);
            }
        } else {
            for &i in sel.iter() {
                let r = lo + i as usize;
                let key = key_of(&self.parts, r);
                let next = l.keys.len() as u32;
                let g = match l.index.get(&key) {
                    Some(&g) => g,
                    None => { l.new_group(key, r, &self.specs); l.index.insert(key, next); next }
                };
                gid.push(g);
            }
        }
        let res = if self.nodes.is_empty() { Vec::new() } else { eval_nodes(&self.nodes, self.block, lo, hi, wc)? };
        for (si, s) in self.specs.iter().enumerate() {
            let (acc, cnt) = (&mut l.acc[si], &mut l.cnt[si]);
            match &s.src {
                Src::Star => for &g in gid.iter() { cnt[g as usize] += 1; },
                Src::NonNull(c) => for (j, &i) in sel.iter().enumerate() { if !is_null_at(c, lo + i as usize) { cnt[gid[j] as usize] += 1; } },
                Src::ColF(v) => { let v = &v[lo..hi]; accum(s.func, sel, gid, |i| v[i], acc, cnt) }
                Src::ColI(v) => { let v = &v[lo..hi]; accum(s.func, sel, gid, |i| v[i].map(|x| x as f64), acc, cnt) }
                Src::Node(k) => match &res[*k] {
                    R::B(v) => accum(s.func, sel, gid, |i| v[i], acc, cnt),
                    R::O(v) => accum(s.func, sel, gid, |i| v[i], acc, cnt),
                    R::C(c) => { let c = *c; accum(s.func, sel, gid, |_| Some(c), acc, cnt) }
                },
            }
        }
        Some(())
    }
}

/// Final per-group state, groups ordered by first appearance.
struct Groups { first: Vec<usize>, acc: Vec<Vec<f64>>, cnt: Vec<Vec<u64>> }

fn merge_locals(job: &Job, locals: Vec<Local>) -> Groups {
    let nspecs = job.specs.len();
    let mut global: KeyMap = HashMap::default();
    let mut g = Groups { first: Vec::new(), acc: vec![Vec::new(); nspecs], cnt: vec![Vec::new(); nspecs] };
    for l in &locals {
        for (lg, key) in l.keys.iter().enumerate() {
            let next = g.first.len() as u32;
            let gi = *global.entry(*key).or_insert_with(|| {
                g.first.push(l.first[lg]);
                for (si, s) in job.specs.iter().enumerate() { g.acc[si].push(init(s.func)); g.cnt[si].push(0); }
                next
            }) as usize;
            for (si, s) in job.specs.iter().enumerate() {
                let (a, c) = (l.acc[si][lg], l.cnt[si][lg]);
                g.cnt[si][gi] += c;
                match s.func {
                    AggFunc::Min => if a < g.acc[si][gi] { g.acc[si][gi] = a },
                    AggFunc::Max => if a > g.acc[si][gi] { g.acc[si][gi] = a },
                    AggFunc::Count => {}
                    _ => g.acc[si][gi] += a,
                }
            }
        }
    }
    g
}

/// Few groups: contiguous row ranges per thread, private accumulators, one small merge.
fn run_local(job: &Job) -> Option<Groups> {
    let n = job.block.num_rows;
    let nparts = if n >= 100_000 { rayon::current_num_threads().max(1) * 2 } else { 1 };
    let span = n.div_ceil(nparts).max(1);
    let locals: Vec<Option<Local>> = (0..nparts).into_par_iter().map(|pi| {
        let (lo, hi) = ((pi * span).min(n), ((pi + 1) * span).min(n));
        let mut l = Local::new(job.specs.len(), job.dict_keys);
        let wc = WinCtx::new();
        let (mut sel, mut gid) = (Vec::with_capacity(WIN), Vec::with_capacity(WIN));
        let mut w = lo;
        while w < hi {
            let we = (w + WIN).min(hi);
            job.window(w, we, &wc, &mut l, &mut sel, &mut gid)?;
            w = we;
        }
        Some(l)
    }).collect();
    let locals: Vec<Local> = locals.into_iter().collect::<Option<Vec<_>>>()?;
    Some(merge_locals(job, locals))
}

/// Selected rows of one window grouped by key partition, plus the values of the computed aggregate inputs.
struct WinOut { lo: usize, lists: Vec<Vec<u32>>, vals: Vec<Vec<Option<f64>>> }

/// Many groups: partition rows by key hash, then one thread per partition builds that partition's groups.
fn run_partitioned(job: &Job) -> Option<Groups> {
    let n = job.block.num_rows;
    let nspecs = job.specs.len();
    let nwin = n.div_ceil(WIN);
    // which node feeds each Node spec, and where its window values go in WinOut::vals
    let node_specs: Vec<usize> = job.specs.iter().enumerate().filter_map(|(i, s)| if matches!(s.src, Src::Node(_)) { Some(i) } else { None }).collect();
    let outs: Vec<Option<WinOut>> = (0..nwin).into_par_iter().map_init(
        || (WinCtx::new(), Vec::with_capacity(WIN)),
        |(wc, sel), wi| {
            let (lo, hi) = (wi * WIN, ((wi + 1) * WIN).min(n));
            job.select(lo, hi, wc, sel)?;
            let mut lists: Vec<Vec<u32>> = vec![Vec::new(); NPART];
            for &i in sel.iter() {
                let r = lo + i as usize;
                let p = (key_hash(&key_of(&job.parts, r)) >> (64 - PART_BITS)) as usize;
                lists[p].push(r as u32);
            }
            let vals = if node_specs.is_empty() || sel.is_empty() { vec![Vec::new(); node_specs.len()] } else {
                let res = eval_nodes(&job.nodes, job.block, lo, hi, wc)?;
                node_specs.iter().map(|&si| {
                    let Src::Node(k) = job.specs[si].src else { unreachable!() };
                    (0..hi - lo).map(|i| res[k].at(i)).collect()
                }).collect()
            };
            Some(WinOut { lo, lists, vals })
        }).collect();
    let outs: Vec<WinOut> = outs.into_iter().collect::<Option<Vec<_>>>()?;

    let parts: Vec<Local> = (0..NPART).into_par_iter().map(|p| {
        let mut l = Local::new(nspecs, false);
        for wo in &outs {
            for &r32 in &wo.lists[p] {
                let r = r32 as usize;
                let key = key_of(&job.parts, r);
                let next = l.keys.len() as u32;
                let g = match l.index.get(&key) {
                    Some(&g) => g as usize,
                    None => { l.new_group(key, r, &job.specs); l.index.insert(key, next); next as usize }
                };
                let mut nk = 0usize;
                for (si, s) in job.specs.iter().enumerate() {
                    let x: Option<f64> = match &s.src {
                        Src::Star => { l.cnt[si][g] += 1; continue }
                        Src::NonNull(c) => { if !is_null_at(c, r) { l.cnt[si][g] += 1; } continue }
                        Src::ColF(v) => v[r],
                        Src::ColI(v) => v[r].map(|x| x as f64),
                        Src::Node(_) => { let x = wo.vals[nk][r - wo.lo]; nk += 1; x }
                    };
                    if let Some(x) = x {
                        l.cnt[si][g] += 1;
                        let a = &mut l.acc[si][g];
                        match s.func {
                            AggFunc::Min => if x < *a { *a = x },
                            AggFunc::Max => if x > *a { *a = x },
                            _ => *a += x,
                        }
                    }
                }
            }
        }
        l
    }).collect();

    // concatenate the partitions and restore first-appearance order
    let total: usize = parts.iter().map(|l| l.keys.len()).sum();
    let mut order: Vec<(usize, u32, u32)> = Vec::with_capacity(total);
    for (pi, l) in parts.iter().enumerate() {
        for (gi, &f) in l.first.iter().enumerate() { order.push((f, pi as u32, gi as u32)); }
    }
    order.par_sort_unstable_by_key(|t| t.0);
    let first: Vec<usize> = order.iter().map(|t| t.0).collect();
    let acc: Vec<Vec<f64>> = (0..nspecs).map(|si| order.iter().map(|&(_, p, g)| parts[p as usize].acc[si][g as usize]).collect()).collect();
    let cnt: Vec<Vec<u64>> = (0..nspecs).map(|si| order.iter().map(|&(_, p, g)| parts[p as usize].cnt[si][g as usize]).collect()).collect();
    Some(Groups { first, acc, cnt })
}

/// True when a sample of the input has so many distinct keys that per-thread tables plus a merge would be slow.
fn looks_high_cardinality(job: &Job) -> bool {
    let n = job.block.num_rows;
    if job.parts.is_empty() || job.dict_keys || n < 200_000 { return false; }
    let wc = WinCtx::new();
    let mut sel = Vec::new();
    let mut seen: std::collections::HashSet<Key, BuildHasherDefault<FxHasher>> = Default::default();
    let mut rows = 0usize;
    for lo in [0, (n / 2) / WIN * WIN] {
        let hi = (lo + WIN).min(n);
        if job.select(lo, hi, &wc, &mut sel).is_none() { return false; }
        rows += sel.len();
        for &i in &sel { seen.insert(key_of(&job.parts, lo + i as usize)); }
    }
    seen.len() > 3000 && seen.len() * 6 > rows
}

pub(crate) struct Request<'a> {
    pub block: &'a DataBlock,
    /// predicate over the block's own column names
    pub pred: Option<&'a Expr>,
    pub group_cols: &'a [String],
    pub projections: &'a [Projection],
    /// prefix for key columns whose name has none (the table alias the ordinary path would have added)
    pub key_prefix: Option<&'a str>,
}

pub(crate) fn run(req: Request) -> Option<DataBlock> {
    let block = req.block;
    let n = block.num_rows;
    if n == 0 || n >= u32::MAX as usize || req.group_cols.len() > 3 { return None; }

    // key columns, read in place (strings are interned to ids once)
    let mut key_cols: Vec<&Column> = Vec::new();
    let mut parts: Vec<KeyPart> = Vec::new();
    for g in req.group_cols {
        let col = find_col(block, g)?;
        key_cols.push(col);
        parts.push(match &col.data {
            ColumnData::Int64(v) => KeyPart::I(v),
            ColumnData::StrDict { codes, .. } => KeyPart::D(codes),
            ColumnData::Str(v) => {
                let mut ids: HashMap<&str, i64, BuildHasherDefault<FxHasher>> = HashMap::default();
                KeyPart::O(v.iter().map(|s| s.as_deref().map(|t| { let next = ids.len() as i64; *ids.entry(t).or_insert(next) })).collect())
            }
            _ => return None,
        });
    }
    let dict_keys = !parts.is_empty() && parts.len() <= 2 && parts.iter().all(|p| matches!(p, KeyPart::D(_)));

    let mut plan = Plan { nodes: Vec::new(), memo: HashMap::new() };
    let mut outs: Vec<Out> = Vec::new();
    let mut specs: Vec<Spec> = Vec::new();
    for p in req.projections {
        let Projection::Expr { expr, alias } = p else { return None };
        match expr {
            Expr::Col(_) | Expr::QualCol(..) => {
                let c = match expr { Expr::Col(c) => c.clone(), Expr::QualCol(t, c) => format!("{t}.{c}"), _ => unreachable!() };
                let col = find_col(block, &c)?;
                if !key_cols.iter().any(|k| std::ptr::eq(*k, col)) { return None; }
                outs.push(Out::Key(col, alias.as_ref()));
            }
            Expr::Agg { func, expr: inner } => {
                if !matches!(func, AggFunc::Sum | AggFunc::Avg | AggFunc::Count | AggFunc::Min | AggFunc::Max) { return None; }
                let star = matches!(inner.as_ref(), Expr::Star) || matches!(inner.as_ref(), Expr::Col(c) if c == "*");
                let col_name = match inner.as_ref() {
                    Expr::Col(c) => c.clone(),
                    Expr::QualCol(t, c) => format!("{t}.{c}"),
                    _ => String::new(),
                };
                let name = alias.clone().unwrap_or_else(|| format!("{:?}({})", func, col_name));
                let mut integral = false;
                let src = if star {
                    if !matches!(func, AggFunc::Count) { return None; }
                    Src::Star
                } else if matches!(func, AggFunc::Count) {
                    match inner.as_ref() {
                        Expr::Col(_) | Expr::QualCol(..) => Src::NonNull(find_col(block, &col_name)?),
                        other => Src::Node(plan.intern(other, block)?),
                    }
                } else {
                    let plain = matches!(inner.as_ref(), Expr::Col(_) | Expr::QualCol(..));
                    match (plain, plain.then(|| find_col(block, &col_name)).flatten().map(|c| &c.data)) {
                        (true, Some(ColumnData::Float64(v))) => Src::ColF(v),
                        (true, Some(ColumnData::Int64(v))) => {
                            // SUM/MIN/MAX of an integer column are integers (checked for exactness below)
                            integral = matches!(func, AggFunc::Sum | AggFunc::Min | AggFunc::Max);
                            Src::ColI(v)
                        }
                        (true, _) => return None,
                        _ => Src::Node(plan.intern(inner, block)?),
                    }
                };
                outs.push(Out::Agg { func, name, integral });
                specs.push(Spec { func, src });
            }
            _ => return None,
        }
    }
    if specs.is_empty() { return None; }
    if req.group_cols.is_empty() && outs.iter().any(|o| matches!(o, Out::Key(..))) { return None; }

    let job = Job { block, pred: req.pred, parts, specs, nodes: plan.nodes, dict_keys };
    let groups = if looks_high_cardinality(&job) { run_partitioned(&job)? } else { run_local(&job)? };
    let ng = groups.first.len();
    if ng == 0 { return None; }

    let mut columns: Vec<Column> = Vec::with_capacity(outs.len());
    let mut si = 0usize;
    for o in &outs {
        match o {
            Out::Key(col, alias) => {
                let name = match alias {
                    Some(a) => (*a).clone(),
                    None => match req.key_prefix {
                        Some(pfx) if !col.name.contains('.') => format!("{pfx}.{}", col.name),
                        _ => col.name.clone(),
                    },
                };
                columns.push(Column { name, data: col.data.take_rows(&groups.first) });
            }
            Out::Agg { func, name, integral } => {
                let (acc, cnt) = (&groups.acc[si], &groups.cnt[si]);
                let data: Vec<Option<f64>> = (0..ng).map(|g| match func {
                    AggFunc::Count => Some(cnt[g] as f64),
                    _ if cnt[g] == 0 => None,
                    AggFunc::Avg => Some(acc[g] / cnt[g] as f64),
                    _ => Some(acc[g]),
                }).collect();
                si += 1;
                if matches!(func, AggFunc::Count) {
                    columns.push(Column { name: name.clone(), data: ColumnData::Int64(data.into_iter().map(|v| v.map(|x| x as i64)).collect()) });
                } else if *integral {
                    // f64 holds integers exactly up to 2^53; beyond that the exact i64 path decides
                    if data.iter().flatten().any(|x| x.abs() > 4.5e15) { return None; }
                    columns.push(Column { name: name.clone(), data: ColumnData::Int64(data.into_iter().map(|v| v.map(|x| x as i64)).collect()) });
                } else {
                    columns.push(Column { name: name.clone(), data: ColumnData::Float64(data) });
                }
            }
        }
    }
    Some(DataBlock { columns, num_rows: ng })
}

/// `e` with every `alias.col` turned into `col`, for evaluation against the unprefixed source table.
/// None for node kinds the fused evaluator does not handle.
fn unq(e: &Expr, alias: &str) -> Option<Expr> {
    let u = |x: &Expr| unq(x, alias).map(Box::new);
    Some(match e {
        Expr::QualCol(t, c) if t == alias => Expr::Col(c.clone()),
        Expr::QualCol(..) => return None,
        Expr::Col(_) | Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null | Expr::Star => e.clone(),
        Expr::BinOp { op, left, right } => Expr::BinOp { op: op.clone(), left: u(left)?, right: u(right)? },
        Expr::Not(x) => Expr::Not(u(x)?),
        Expr::IsNull(x) => Expr::IsNull(u(x)?),
        Expr::IsNotNull(x) => Expr::IsNotNull(u(x)?),
        Expr::Case { operand, branches, else_val } => Expr::Case {
            operand: match operand { Some(o) => Some(u(o)?), None => None },
            branches: branches.iter().map(|(c, v)| Some((u(c)?, u(v)?))).collect::<Option<Vec<_>>>()?,
            else_val: match else_val { Some(x) => Some(u(x)?), None => None },
        },
        Expr::In { expr, values, negated } => Expr::In {
            expr: u(expr)?, values: values.iter().map(|v| unq(v, alias)).collect::<Option<Vec<_>>>()?, negated: *negated,
        },
        Expr::Between { expr, low, high, negated } => Expr::Between { expr: u(expr)?, low: u(low)?, high: u(high)?, negated: *negated },
        Expr::Like { expr, pattern, negated } => Expr::Like { expr: u(expr)?, pattern: u(pattern)?, negated: *negated },
        Expr::Agg { func, expr } => Expr::Agg { func: func.clone(), expr: u(expr)? },
        _ => return None,
    })
}

/// Single-table SELECT <keys, aggregates> FROM t [WHERE ...] [GROUP BY keys], run without materialising the
/// filtered table. `conjuncts` are the WHERE conjuncts (the caller has not consumed them yet).
pub(crate) fn try_fused(stmt: &SelectStmt, src: &DataBlock, alias: &str, conjuncts: &[Expr]) -> Option<DataBlock> {
    if src.num_rows < MIN_ROWS || crate::testing::no_vecexpr() || crate::testing::no_fused() { return None; }
    if !stmt.joins.is_empty() || stmt.grouping != Grouping::Plain || stmt.distinct || !stmt.group_exprs.is_empty() { return None; }
    let mut projections: Vec<Projection> = Vec::with_capacity(stmt.projections.len());
    let mut any_agg = false;
    for p in &stmt.projections {
        let Projection::Expr { expr, alias: pa } = p else { return None };
        match expr {
            Expr::Agg { func, expr: inner } => {
                any_agg = true;
                // output name the ordinary path would give (derived from the query text, qualified or not)
                let col_name = match inner.as_ref() {
                    Expr::Col(c) => c.clone(),
                    Expr::QualCol(t, c) => format!("{t}.{c}"),
                    _ => String::new(),
                };
                let name = pa.clone().unwrap_or_else(|| format!("{:?}({})", func, col_name));
                projections.push(Projection::Expr { expr: unq(expr, alias)?, alias: Some(name) });
            }
            Expr::Col(_) | Expr::QualCol(..) => projections.push(Projection::Expr { expr: unq(expr, alias)?, alias: pa.clone() }),
            _ => return None,
        }
    }
    if !any_agg { return None; }
    // GROUP BY names: bare or qualified with this table's alias, naming real columns
    let mut group_cols: Vec<String> = Vec::new();
    for g in &stmt.group_by {
        let bare = g.strip_prefix(&format!("{alias}.")).unwrap_or(g);
        find_col(src, bare)?;
        group_cols.push(bare.to_string());
    }
    // every conjunct must only use this table's columns and be one the vectorised evaluator covers
    let mut preds: Vec<Expr> = Vec::new();
    for c in conjuncts {
        let mut cols = Vec::new();
        if !crate::rewrite::referenced_cols(c, &mut cols) || cols.is_empty() { return None; }
        let prefix = format!("{alias}.");
        if !cols.iter().all(|n| match n.strip_prefix(&prefix) { Some(b) => find_col(src, b).is_some(), None => !n.contains('.') && find_col(src, n).is_some() }) {
            return None;
        }
        preds.push(unq(c, alias)?);
    }
    let pred = crate::rewrite::and_all(preds);
    if let Some(p) = &pred {
        tri_window(p, src, 0, 2.min(src.num_rows), &WinCtx::new())?;
    }
    // the same "does the fast path apply" check the ordinary path makes, on a small sample of the table
    let sample = DataBlock {
        num_rows: 64.min(src.num_rows),
        columns: src.columns.iter().map(|c| Column { name: format!("{alias}.{}", c.name), data: c.data.take_rows(&(0..64.min(src.num_rows)).collect::<Vec<_>>()) }).collect(),
    };
    if crate::general::block_needs_general(stmt, &sample) { return None; }
    run(Request { block: src, pred: pred.as_ref(), group_cols: &group_cols, projections: &projections, key_prefix: Some(alias) })
}
