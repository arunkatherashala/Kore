//! Key sets for sideways information passing (executor.rs, `sip_*`): a set of Int64 join keys that is
//! built in parallel and tested without materialising a long `IN (...)` list of `Expr` nodes.
//! Dense key ranges use a bitmap (no key-count limit), sparse ones a hash set capped at `max_hash_keys`.
//! A set is only ever an extra restriction on top of the join equality that stays in the plan, so
//! dropping one is always correct.

use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

type FxSet = std::collections::HashSet<i64, std::hash::BuildHasherDefault<crate::executor::FxHasher>>;

pub struct IntSet {
    kind: Kind,
    /// number of distinct keys
    pub count: usize,
    /// max - min + 1 over the keys (1 for the empty set)
    pub span: u64,
}

enum Kind { Bits { lo: i64, bits: Vec<u64> }, Hash(FxSet) }

const CHUNK: usize = 1 << 16;
/// largest bitmap, in keys of range (2^28 bits = 32 MB)
const MAX_SPAN: u64 = 1 << 28;

impl IntSet {
    /// Distinct non-NULL values of `v`; None when a bitmap does not fit and there are more than `max_hash_keys` of them.
    pub fn build(v: &[Option<i64>], max_hash_keys: usize) -> Option<IntSet> {
        let (lo, hi) = v.par_chunks(CHUNK).map(|ch| {
            let (mut lo, mut hi) = (i64::MAX, i64::MIN);
            for x in ch { if let Some(k) = x { if *k < lo { lo = *k; } if *k > hi { hi = *k; } } }
            (lo, hi)
        }).reduce(|| (i64::MAX, i64::MIN), |a, b| (a.0.min(b.0), a.1.max(b.1)));
        if lo > hi { return Some(IntSet { kind: Kind::Bits { lo: 0, bits: Vec::new() }, count: 0, span: 1 }); }
        let span = (hi as i128 - lo as i128) as u128 + 1;
        if span <= MAX_SPAN as u128 && span <= 64 * v.len() as u128 + (1 << 20) {
            let words: Vec<AtomicU64> = (0..(span as usize).div_ceil(64)).map(|_| AtomicU64::new(0)).collect();
            v.par_chunks(CHUNK).for_each(|ch| {
                for x in ch {
                    if let Some(k) = x {
                        let d = k.wrapping_sub(lo) as u64;
                        words[(d >> 6) as usize].fetch_or(1u64 << (d & 63), Ordering::Relaxed);
                    }
                }
            });
            let bits: Vec<u64> = words.into_iter().map(|w| w.into_inner()).collect();
            let count = bits.iter().map(|w| w.count_ones() as usize).sum();
            return Some(IntSet { kind: Kind::Bits { lo, bits }, count, span: span as u64 });
        }
        let mut set = FxSet::default();
        for x in v.iter().flatten() {
            set.insert(*x);
            if set.len() > max_hash_keys { return None; }
        }
        let count = set.len();
        Some(IntSet { kind: Kind::Hash(set), count, span: u64::try_from(span).unwrap_or(u64::MAX) })
    }

    #[inline]
    pub fn contains(&self, k: i64) -> bool {
        match &self.kind {
            Kind::Bits { lo, bits } => {
                let d = k.wrapping_sub(*lo) as u64;
                match bits.get((d >> 6) as usize) { Some(w) => (w >> (d & 63)) & 1 != 0, None => false }
            }
            Kind::Hash(s) => s.contains(&k),
        }
    }

    /// Fraction of the key range that is present (a set near 1.0 filters nothing worth a pass).
    pub fn density(&self) -> f64 { self.count as f64 / self.span.max(1) as f64 }

    /// Ascending indices of the rows of `col` whose key is in the set (NULL never matches); `rows` restricts the
    /// candidates to those row numbers (ascending), None = every row.
    pub fn filter(&self, col: &[Option<i64>], rows: Option<&[usize]>) -> Vec<usize> {
        match rows {
            None => {
                let parts: Vec<Vec<usize>> = col.par_chunks(CHUNK).enumerate().map(|(ci, ch)| {
                    let base = ci * CHUNK;
                    let mut out = Vec::new();
                    for (i, x) in ch.iter().enumerate() {
                        if let Some(k) = x { if self.contains(*k) { out.push(base + i); } }
                    }
                    out
                }).collect();
                concat(parts)
            }
            Some(rows) => {
                let parts: Vec<Vec<usize>> = rows.par_chunks(CHUNK).map(|ch| {
                    ch.iter().copied().filter(|&r| matches!(col[r], Some(k) if self.contains(k))).collect()
                }).collect();
                concat(parts)
            }
        }
    }
}

fn concat(parts: Vec<Vec<usize>>) -> Vec<usize> {
    let total = parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(total);
    for p in parts { out.extend(p); }
    out
}
