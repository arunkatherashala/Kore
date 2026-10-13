//! Hash Join — build a hash table on the smaller (build) side, probe with the larger side.
//!
//! Supports INNER, LEFT, RIGHT and FULL OUTER joins.
//! Probe phase is parallelized via Rayon (O(n/T) per thread).

use std::collections::HashMap;
use std::sync::Arc;
use rayon::prelude::*;
use kore_core::{Column, DataBlock, JoinKey, JoinType, KoreError};

use crate::JoinConfig;
use std::sync::atomic::{AtomicBool, Ordering};

static SERIAL: AtomicBool = AtomicBool::new(false);
/// Per-stage switches (mask 1 = hash build, 2 = probe concatenation, 4 = in-column gather); used by the benchmark.
static STAGE_OFF: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Turn single stages of the parallel join mechanics off (mask 1 build, 2 concat, 4 gather); 0 = all on.
pub fn set_serial_stages(mask: u8) { STAGE_OFF.store(mask, Ordering::Relaxed); }

#[inline]
fn stage_off(bit: u8) -> bool { STAGE_OFF.load(Ordering::Relaxed) & bit != 0 }

/// Force the original single-threaded hash build, serial output concatenation and per-column-only gather
/// (A/B oracle for the parallel kernels). Results are identical either way.
pub fn set_serial_join(on: bool) { SERIAL.store(on, Ordering::Relaxed); }

#[inline]
pub fn serial_join() -> bool {
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    SERIAL.load(Ordering::Relaxed) || *ENV.get_or_init(|| std::env::var_os("KORE_SERIAL_JOIN").is_some())
}

/// Build sides at least this long are built in parallel.
pub const PAR_BUILD_MIN: usize = 1 << 17;
/// Gathers of at least this many rows are split within the column.
pub const PAR_GATHER_MIN: usize = 1 << 16;
const GATHER_CHUNK: usize = 1 << 15;

/// Raw pointer that may be shared across threads; every use writes disjoint elements.
struct SendPtr<T>(*mut T);
unsafe impl<T> Send for SendPtr<T> {}
unsafe impl<T> Sync for SendPtr<T> {}
impl<T> Clone for SendPtr<T> { fn clone(&self) -> Self { SendPtr(self.0) } }
impl<T> Copy for SendPtr<T> {}
impl<T> SendPtr<T> {
    #[inline(always)] fn get(self) -> *mut T { self.0 }
}

pub struct HashJoin;

impl HashJoin {
    /// Execute the join.  `left` is the probe side; `right` is the build side.
    pub fn join(
        left: &DataBlock,
        right: &DataBlock,
        cfg: &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        // ── Fast path: Int64 key columns (most common, no JoinKey allocation) ──
        let right_int_col = right.columns.iter().find(|c| c.name == cfg.right_key);
        let left_int_col  = left.columns.iter().find(|c| c.name == cfg.left_key);

        if let (Some(rc), Some(lc)) = (right_int_col, left_int_col) {
            if let (kore_core::ColumnData::Int64(rv), kore_core::ColumnData::Int64(lv)) =
                (&rc.data, &lc.data)
            {
                return Self::join_int64(left, right, lv, rv, cfg);
            }
        }

        // ── Fallback: generic JoinKey path (handles Str/Bool/Null keys) ────────
        let mut table: HashMap<JoinKey, Vec<usize>> = HashMap::with_capacity(right.num_rows);
        for i in 0..right.num_rows {
            let key = join_key(right, i, &cfg.right_key)?;
            // a NULL key equals nothing, not even another NULL
            if key == JoinKey::Null { continue; }
            table.entry(key).or_default().push(i);
        }
        let table = Arc::new(table);

        let n_left = left.num_rows;
        let n_threads = rayon::current_num_threads();
        let chunk_sz = ((n_left + n_threads - 1) / n_threads).max(1);

        let local_pairs: Vec<Vec<(Option<usize>, Option<usize>)>> = (0..n_threads)
            .into_par_iter()
            .map(|t| {
                let start = t * chunk_sz;
                let end   = (start + chunk_sz).min(n_left);
                if start >= end { return vec![]; }
                let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
                for l in start..end {
                    if let Ok(key) = join_key(left, l, &cfg.left_key) {
                        let hit = if key == JoinKey::Null { None } else { table.get(&key) };
                        if let Some(right_rows) = hit {
                            for &r in right_rows { pairs.push((Some(l), Some(r))); }
                        } else if matches!(cfg.join_type, JoinType::Left | JoinType::Full) {
                            pairs.push((Some(l), None));
                        }
                    }
                }
                pairs
            })
            .collect();

        let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        for lp in local_pairs { pairs.extend(lp); }

        if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
            let mut right_matched = vec![false; right.num_rows];
            for &(_, r) in &pairs { if let Some(ri) = r { right_matched[ri] = true; } }
            for (r, matched) in right_matched.iter().enumerate() {
                if !matched { pairs.push((None, Some(r))); }
            }
        }

        build_result(left, right, &pairs)
    }

    /// Int64-key hash join: flat (array based) hash table on the build side, chunked parallel probe, and
    /// a parallel column gather. For inner joins the smaller input is the build side, so a handful of
    /// filtered dimension rows never forces a hash table over a multi-million-row fact table.
    fn join_int64(
        left: &DataBlock,
        right: &DataBlock,
        lv: &[Option<i64>],
        rv: &[Option<i64>],
        cfg: &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        if lv.len() >= u32::MAX as usize || rv.len() >= u32::MAX as usize {
            return Self::join_int64_wide(left, right, lv, rv, cfg);
        }
        let (lidx, ridx) = Self::join_int64_idx(lv, rv, cfg);
        build_result_idx(left, right, &lidx, &ridx)
    }

    /// Like [`HashJoin::join`], but consumes both inputs and frees every source column as soon as it has been
    /// gathered, so the peak is the output plus the not-yet-gathered inputs instead of inputs plus output.
    pub fn join_owned(left: DataBlock, right: DataBlock, cfg: &JoinConfig) -> Result<DataBlock, KoreError> {
        let idx = {
            let lc = left.columns.iter().find(|c| c.name == cfg.left_key);
            let rc = right.columns.iter().find(|c| c.name == cfg.right_key);
            match (lc, rc) {
                (Some(lc), Some(rc)) => match (&lc.data, &rc.data) {
                    (kore_core::ColumnData::Int64(lv), kore_core::ColumnData::Int64(rv))
                        if lv.len() < u32::MAX as usize && rv.len() < u32::MAX as usize =>
                        Some(Self::join_int64_idx(lv, rv, cfg)),
                    _ => None,
                },
                _ => None,
            }
        };
        match idx {
            Some((lidx, ridx)) => Ok(build_result_idx_owned(left, right, &lidx, &ridx)),
            None => Self::join(&left, &right, cfg),
        }
    }

    /// Row-id pairs (left, right) of an INNER join of two key slices (NULL keys never match).
    /// Both slices must be shorter than u32::MAX.
    pub fn inner_join_row_ids(lv: &[Option<i64>], rv: &[Option<i64>]) -> (Vec<u32>, Vec<u32>) {
        let cfg = JoinConfig { left_key: String::new(), right_key: String::new(), join_type: JoinType::Inner };
        Self::join_int64_idx(lv, rv, &cfg)
    }

    /// Row-id pairs (left, right) of any join type over two key slices (u32::MAX = no row; NULL keys never
    /// match). Both slices must be shorter than u32::MAX.
    pub fn join_row_ids(lv: &[Option<i64>], rv: &[Option<i64>], join_type: JoinType) -> (Vec<u32>, Vec<u32>) {
        let cfg = JoinConfig { left_key: String::new(), right_key: String::new(), join_type };
        Self::join_int64_idx(lv, rv, &cfg)
    }

    /// Row-id pairs (left, right) of the join; u32::MAX = no row. Keys must be shorter than u32::MAX rows.
    fn join_int64_idx(lv: &[Option<i64>], rv: &[Option<i64>], cfg: &JoinConfig) -> (Vec<u32>, Vec<u32>) {
        const NONE: u32 = u32::MAX;
        let inner = matches!(cfg.join_type, JoinType::Inner);
        // build on the right unless this is an inner join and the left side is smaller
        let swap = inner && lv.len() < rv.len();
        let (bk, pk) = if swap { (lv, rv) } else { (rv, lv) };
        let table = FlatTable::build(bk);

        let n_probe = pk.len();
        const PCHUNK: usize = 32_768;
        let keep_unmatched = matches!(cfg.join_type, JoinType::Left | JoinType::Full);
        let parts: Vec<(Vec<u32>, Vec<u32>)> = (0..n_probe.div_ceil(PCHUNK)).into_par_iter().map(|c| {
            let lo = c * PCHUNK;
            let hi = (lo + PCHUNK).min(n_probe);
            let mut pi: Vec<u32> = Vec::with_capacity(hi - lo);
            let mut bi: Vec<u32> = Vec::with_capacity(hi - lo);
            for p in lo..hi {
                if let Some(k) = pk[p] {
                    let mut e = table.first(k);
                    if e == NONE {
                        if keep_unmatched { pi.push(p as u32); bi.push(NONE); }
                    } else {
                        while e != NONE {
                            pi.push(p as u32);
                            bi.push(e);
                            e = table.next[e as usize];
                        }
                    }
                } else if keep_unmatched {
                    // a NULL key matches nothing, but an outer join still keeps the row
                    pi.push(p as u32); bi.push(NONE);
                }
            }
            (pi, bi)
        }).collect();
        let total: usize = parts.iter().map(|p| p.0.len()).sum();
        let (pidx, bidx) = if serial_join() || stage_off(2) || total < PAR_GATHER_MIN {
            let mut pidx: Vec<u32> = Vec::with_capacity(total);
            let mut bidx: Vec<u32> = Vec::with_capacity(total);
            for (a, b) in &parts { pidx.extend_from_slice(a); bidx.extend_from_slice(b); }
            (pidx, bidx)
        } else {
            // prefix sums, then every part copies into its own disjoint range
            let mut offs: Vec<usize> = Vec::with_capacity(parts.len());
            let mut acc = 0usize;
            for p in &parts { offs.push(acc); acc += p.0.len(); }
            let mut pidx: Vec<u32> = Vec::with_capacity(total);
            let mut bidx: Vec<u32> = Vec::with_capacity(total);
            let (pp, bp) = (SendPtr(pidx.as_mut_ptr()), SendPtr(bidx.as_mut_ptr()));
            parts.par_iter().zip(offs.par_iter()).for_each(|((a, b), &o)| unsafe {
                std::ptr::copy_nonoverlapping(a.as_ptr(), pp.get().add(o), a.len());
                std::ptr::copy_nonoverlapping(b.as_ptr(), bp.get().add(o), b.len());
            });
            // SAFETY: the parts cover exactly 0..total and each element is written once
            unsafe { pidx.set_len(total); bidx.set_len(total); }
            (pidx, bidx)
        };
        drop(parts);
        // probe side = left unless we swapped
        let (mut lidx, mut ridx) = if swap { (bidx, pidx) } else { (pidx, bidx) };

        if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
            let mut matched = vec![false; rv.len()];
            for &r in &ridx { if r != NONE { matched[r as usize] = true; } }
            for (r, m) in matched.iter().enumerate() {
                if !m { lidx.push(NONE); ridx.push(r as u32); }
            }
        }
        (lidx, ridx)
    }

    /// Fallback for inputs too large for 32-bit row ids (never hit in practice).
    fn join_int64_wide(
        left: &DataBlock, right: &DataBlock, lv: &[Option<i64>], rv: &[Option<i64>], cfg: &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        let mut table: HashMap<i64, Vec<usize>> = HashMap::with_capacity(rv.len() / 4 + 16);
        for (i, k) in rv.iter().enumerate() { if let Some(k) = k { table.entry(*k).or_default().push(i); } }
        let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        for (l, k) in lv.iter().enumerate() {
            if let Some(k) = k {
                if let Some(rows) = table.get(k) { for &r in rows { pairs.push((Some(l), Some(r))); } }
                else if matches!(cfg.join_type, JoinType::Left | JoinType::Full) { pairs.push((Some(l), None)); }
            }
        }
        if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
            let mut matched = vec![false; rv.len()];
            for &(_, r) in &pairs { if let Some(ri) = r { matched[ri] = true; } }
            for (r, m) in matched.iter().enumerate() { if !m { pairs.push((None, Some(r))); } }
        }
        build_result(left, right, &pairs)
    }
}

/// Chained hash table over i64 keys: `next[row]` links rows with the same key (ascending row order).
/// Dense key ranges (surrogate keys) use a direct-address table, everything else open addressing.
struct FlatTable {
    min: i64,
    direct: bool,
    heads: Vec<u32>,      // direct: indexed by key-min; hashed: head row per slot
    slot_keys: Vec<i64>,  // hashed only
    mask: usize,          // hashed: slot mask inside one partition (partition size - 1)
    pbits: u32,           // hashed: log2 of the number of partitions (0 = one plain table)
    next: Vec<u32>,
}

impl FlatTable {
    const NONE: u32 = u32::MAX;

    #[inline(always)]
    fn hash(k: i64) -> usize {
        let h = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (h ^ (h >> 32)) as usize
    }

    /// First slot of the probe sequence of hash `h` (partition base; the in-partition position is `h & mask`).
    #[inline(always)]
    fn base(&self, h: usize) -> usize {
        if self.pbits == 0 { 0 } else { (h >> (64 - self.pbits)) * (self.mask + 1) }
    }

    fn build(keys: &[Option<i64>]) -> Self {
        if keys.len() >= PAR_BUILD_MIN && !serial_join() && !stage_off(1) && rayon::current_num_threads() > 1 {
            if let Some(t) = Self::build_par(keys) { return t; }
        }
        Self::build_serial(keys)
    }

    /// Parallel build with the same chain order as the serial one (rows of a key in ascending order).
    /// Rows are bucketed by slot range (direct) or hash partition (hashed) per row chunk; every partition
    /// is then filled by one task, walking its buckets from the last chunk to the first. None = skewed
    /// partitions, the caller builds serially.
    fn build_par(keys: &[Option<i64>]) -> Option<Self> {
        let n = keys.len();
        let nt = rayon::current_num_threads();
        let chunk = n.div_ceil(nt * 4).max(1 << 14);
        let nchunks = n.div_ceil(chunk);
        let (lo, hi, any) = keys.par_chunks(chunk).map(|c| {
            let (mut lo, mut hi, mut any) = (i64::MAX, i64::MIN, false);
            for k in c.iter().flatten() { any = true; if *k < lo { lo = *k; } if *k > hi { hi = *k; } }
            (lo, hi, any)
        }).reduce(|| (i64::MAX, i64::MIN, false), |a, b| (a.0.min(b.0), a.1.max(b.1), a.2 || b.2));
        let mut next: Vec<u32> = Vec::with_capacity(n);
        if !any {
            next.resize(n, Self::NONE);
            return Some(FlatTable { min: 0, direct: true, heads: Vec::new(), slot_keys: Vec::new(), mask: 0, pbits: 0, next });
        }
        let range = (hi as i128 - lo as i128) as u128 + 1;
        let direct = range <= (4 * n as u128 + 1024).max(1 << 16).min(1 << 27);
        let (np, pshift, pbits, cap, sub);
        if direct {
            let r = range as usize;
            let span = r.div_ceil(nt * 4).max(1 << 12).next_power_of_two();
            np = r.div_ceil(span); pshift = span.trailing_zeros(); pbits = 0u32; cap = r; sub = 0usize;
        } else {
            let pb = (nt * 4).next_power_of_two().trailing_zeros();
            cap = (n * 2).next_power_of_two().max(16);
            np = 1usize << pb; sub = cap / np; pshift = 0; pbits = pb;
            if pb == 0 || sub < 1024 { return None; }
        }
        let part_of = |k: i64| -> usize {
            if direct { ((k as i128 - lo as i128) as usize) >> pshift } else { Self::hash(k) >> (64 - pbits) }
        };
        // phase 1: bucket the row ids of every chunk by partition (ascending inside a bucket)
        let buckets: Vec<(Vec<u32>, Vec<usize>)> = (0..nchunks).into_par_iter().map(|c| {
            let lo_r = c * chunk;
            let hi_r = (lo_r + chunk).min(n);
            let mut counts = vec![0usize; np + 1];
            for k in keys[lo_r..hi_r].iter().flatten() { counts[part_of(*k) + 1] += 1; }
            for p in 0..np { counts[p + 1] += counts[p]; }
            let offs = counts.clone();
            let mut rows = vec![0u32; counts[np]];
            for r in lo_r..hi_r {
                if let Some(k) = keys[r] { let p = part_of(k); rows[counts[p]] = r as u32; counts[p] += 1; }
            }
            (rows, offs)
        }).collect();
        if !direct {
            // a partition holding more than 3/4 of its slots in rows could overflow its probe range
            let skew = (0..np).any(|p| buckets.iter().map(|(_, o)| o[p + 1] - o[p]).sum::<usize>() * 4 > sub * 3);
            if skew { return None; }
        }
        // phase 2: one task per partition (disjoint slots / rows)
        next.resize(n, Self::NONE);
        let mut heads = vec![Self::NONE; cap];
        let mut slot_keys: Vec<i64> = if direct { Vec::new() } else { vec![0i64; cap] };
        let (nx, hp, sk) = (SendPtr(next.as_mut_ptr()), SendPtr(heads.as_mut_ptr()), SendPtr(slot_keys.as_mut_ptr()));
        (0..np).into_par_iter().for_each(|p| unsafe {
            for (rows, offs) in buckets.iter().rev() {
                for &r in rows[offs[p]..offs[p + 1]].iter().rev() {
                    let k = keys[r as usize].unwrap_unchecked();
                    if direct {
                        let s = (k as i128 - lo as i128) as usize;
                        *nx.get().add(r as usize) = *hp.get().add(s);
                        *hp.get().add(s) = r;
                    } else {
                        let base = p * sub;
                        let mut s = Self::hash(k) & (sub - 1);
                        loop {
                            if *hp.get().add(base + s) == Self::NONE { *sk.get().add(base + s) = k; break; }
                            if *sk.get().add(base + s) == k { break; }
                            s = (s + 1) & (sub - 1);
                        }
                        *nx.get().add(r as usize) = *hp.get().add(base + s);
                        *hp.get().add(base + s) = r;
                    }
                }
            }
        });
        Some(if direct {
            FlatTable { min: lo, direct: true, heads, slot_keys, mask: 0, pbits: 0, next }
        } else {
            FlatTable { min: 0, direct: false, heads, slot_keys, mask: sub - 1, pbits, next }
        })
    }

    fn build_serial(keys: &[Option<i64>]) -> Self {
        let n = keys.len();
        let (mut lo, mut hi, mut any) = (i64::MAX, i64::MIN, false);
        for k in keys.iter().flatten() { any = true; if *k < lo { lo = *k; } if *k > hi { hi = *k; } }
        let mut next = vec![Self::NONE; n];
        if !any {
            return FlatTable { min: 0, direct: true, heads: Vec::new(), slot_keys: Vec::new(), mask: 0, pbits: 0, next };
        }
        let range = (hi as i128 - lo as i128) as u128 + 1;
        if range <= (4 * n as u128 + 1024).max(1 << 16).min(1 << 27) {
            let mut heads = vec![Self::NONE; range as usize];
            for i in (0..n).rev() {
                if let Some(k) = keys[i] {
                    let s = (k as i128 - lo as i128) as usize;
                    next[i] = heads[s];
                    heads[s] = i as u32;
                }
            }
            FlatTable { min: lo, direct: true, heads, slot_keys: Vec::new(), mask: 0, pbits: 0, next }
        } else {
            let cap = (n * 2).next_power_of_two().max(16);
            let mask = cap - 1;
            let mut heads = vec![Self::NONE; cap];
            let mut slot_keys = vec![0i64; cap];
            for i in (0..n).rev() {
                if let Some(k) = keys[i] {
                    let mut s = Self::hash(k) & mask;
                    loop {
                        if heads[s] == Self::NONE { slot_keys[s] = k; break; }
                        if slot_keys[s] == k { break; }
                        s = (s + 1) & mask;
                    }
                    next[i] = heads[s];
                    heads[s] = i as u32;
                }
            }
            FlatTable { min: 0, direct: false, heads, slot_keys, mask, pbits: 0, next }
        }
    }

    /// First build row with key `k`, or NONE.
    #[inline(always)]
    fn first(&self, k: i64) -> u32 {
        if self.direct {
            let d = k as i128 - self.min as i128;
            if d < 0 || d >= self.heads.len() as i128 { return Self::NONE; }
            self.heads[d as usize]
        } else {
            let h = Self::hash(k);
            let base = self.base(h);
            let mut s = h & self.mask;
            loop {
                let hd = self.heads[base + s];
                if hd == Self::NONE { return Self::NONE; }
                if self.slot_keys[base + s] == k { return hd; }
                s = (s + 1) & self.mask;
            }
        }
    }
}

/// Gather both sides' columns for row-id pairs (u32::MAX = no row, i.e. NULL), one column per task.
pub(crate) fn build_result_idx(left: &DataBlock, right: &DataBlock, lidx: &[u32], ridx: &[u32]) -> Result<DataBlock, KoreError> {
    let left_names: std::collections::HashSet<&str> = left.columns.iter().map(|c| c.name.as_str()).collect();
    let mut jobs: Vec<(String, &kore_core::ColumnData, &[u32])> = Vec::with_capacity(left.columns.len() + right.columns.len());
    for c in &left.columns { jobs.push((c.name.clone(), &c.data, lidx)); }
    for c in &right.columns {
        let name = if left_names.contains(c.name.as_str()) { format!("{}_r", c.name) } else { c.name.clone() };
        jobs.push((name, &c.data, ridx));
    }
    let columns: Vec<Column> = jobs.into_par_iter().map(|(name, data, idx)| Column { name, data: gather(data, idx) }).collect();
    Ok(DataBlock { columns, num_rows: lidx.len() })
}

/// Same output as [`build_result_idx`], but each input column is dropped right after it has been gathered.
fn build_result_idx_owned(left: DataBlock, right: DataBlock, lidx: &[u32], ridx: &[u32]) -> DataBlock {
    let left_names: std::collections::HashSet<String> = left.columns.iter().map(|c| c.name.clone()).collect();
    let mut jobs: Vec<(String, kore_core::ColumnData, &[u32])> = Vec::with_capacity(left.columns.len() + right.columns.len());
    for c in left.columns { jobs.push((c.name, c.data, lidx)); }
    for c in right.columns {
        let name = if left_names.contains(c.name.as_str()) { format!("{}_r", c.name) } else { c.name };
        jobs.push((name, c.data, ridx));
    }
    let columns: Vec<Column> = jobs.into_par_iter().map(|(name, data, idx)| {
        let g = gather(&data, idx);
        drop(data);
        Column { name, data: g }
    }).collect();
    DataBlock { columns, num_rows: lidx.len() }
}

/// Take rows `idx` of `src` (u32::MAX = NULL).
pub fn gather_rows(src: &kore_core::ColumnData, idx: &[u32]) -> kore_core::ColumnData { gather(src, idx) }

/// `out[i] = f(idx[i])`, in parallel chunks for long index vectors.
fn gather_with<T: Send, F: Fn(u32) -> T + Sync>(idx: &[u32], f: F) -> Vec<T> {
    let n = idx.len();
    if n < PAR_GATHER_MIN || serial_join() || stage_off(4) { return idx.iter().map(|&i| f(i)).collect(); }
    let mut out: Vec<T> = Vec::with_capacity(n);
    out.spare_capacity_mut()[..n].par_chunks_mut(GATHER_CHUNK).zip(idx.par_chunks(GATHER_CHUNK)).for_each(|(o, ix)| {
        for (slot, &i) in o.iter_mut().zip(ix) { slot.write(f(i)); }
    });
    // SAFETY: every one of the n slots was written above (a panic in `f` unwinds before this point)
    unsafe { out.set_len(n); }
    out
}

fn gather(src: &kore_core::ColumnData, idx: &[u32]) -> kore_core::ColumnData {
    use kore_core::ColumnData;
    const NONE: u32 = u32::MAX;
    match src {
        ColumnData::Int64(v) => ColumnData::Int64(gather_with(idx, |i| if i == NONE { None } else { v[i as usize] })),
        ColumnData::Float64(v) => ColumnData::Float64(gather_with(idx, |i| if i == NONE { None } else { v[i as usize] })),
        ColumnData::Bool(v) => ColumnData::Bool(gather_with(idx, |i| if i == NONE { None } else { v[i as usize] })),
        ColumnData::Str(v) => ColumnData::Str(gather_with(idx, |i| if i == NONE { None } else { v[i as usize].clone() })),
        ColumnData::StrDict { codes, dict } => ColumnData::StrDict {
            codes: gather_with(idx, |i| if i == NONE { u8::MAX } else { codes[i as usize] }),
            dict: dict.clone(),
        },
    }
}

/// Parallel copy of a whole column (plain `clone` below the parallel threshold or in serial mode).
pub fn clone_column(src: &kore_core::ColumnData) -> kore_core::ColumnData {
    let n = src.len();
    if serial_join() || n < PAR_GATHER_MIN || n >= u32::MAX as usize { return src.clone(); }
    let ident: Vec<u32> = (0..n as u32).collect();
    gather(src, &ident)
}

/// Parallel `take_rows`: rows `idx` of `src` (out-of-range indices give NULL, like `take_rows`).
pub fn take_rows_par(src: &kore_core::ColumnData, idx: &[usize]) -> kore_core::ColumnData {
    if serial_join() || idx.len() < PAR_GATHER_MIN || src.len() >= u32::MAX as usize { return src.take_rows(idx); }
    let n = src.len();
    let ix: Vec<u32> = idx.par_iter().map(|&i| if i < n { i as u32 } else { u32::MAX }).collect();
    gather(src, &ix)
}

/// Join key of a cell. Floats are comparable (an integral float equals the same integer, so a DOUBLE key can
/// join a BIGINT key); NULL stays `JoinKey::Null`, which never matches.
fn join_key(block: &DataBlock, row: usize, col: &str) -> Result<JoinKey, KoreError> {
    let c = block.column(col).ok_or_else(|| KoreError::ColumnNotFound(col.into()))?;
    Ok(match c.data.get_value(row) {
        kore_core::Value::Float(f) => {
            if f.is_nan() { JoinKey::Null }
            else if f.fract() == 0.0 && f.abs() < 9.0e15 { JoinKey::Int(f as i64) }
            else { JoinKey::Str(format!("float#{:?}", f)) }
        }
        v => JoinKey::from(&v),
    })
}

/// Materialise a DataBlock from (left_idx | None, right_idx | None) pairs.
/// Uses bulk column-at-a-time copy — replaces 102M per-row virtual dispatch
/// calls with tight indexed iterator chains that LLVM can vectorize.
pub(crate) fn build_result(
    left:  &DataBlock,
    right: &DataBlock,
    pairs: &[(Option<usize>, Option<usize>)],
) -> Result<DataBlock, KoreError> {
    let n = pairs.len();

    // Pre-extract index vecs once (avoids re-scanning pairs per column)
    let left_idxs:  Vec<Option<usize>> = pairs.iter().map(|(l, _)| *l).collect();
    let right_idxs: Vec<Option<usize>> = pairs.iter().map(|(_, r)| *r).collect();

    let left_names: std::collections::HashSet<&str> =
        left.columns.iter().map(|c| c.name.as_str()).collect();

    let mut columns: Vec<Column> = Vec::with_capacity(left.columns.len() + right.columns.len());

    // Bulk-copy left columns column-at-a-time
    for col in &left.columns {
        let data = bulk_copy(&col.data, &left_idxs);
        columns.push(Column { name: col.name.clone(), data });
    }

    // Bulk-copy right columns column-at-a-time (suffix _r on name clash)
    for col in &right.columns {
        let name = if left_names.contains(col.name.as_str()) {
            format!("{}_r", col.name)
        } else {
            col.name.clone()
        };
        let data = bulk_copy(&col.data, &right_idxs);
        columns.push(Column { name, data });
    }

    Ok(DataBlock { columns, num_rows: n })
}

/// Bulk-copy a column using a pre-computed index array — O(n), no virtual dispatch per row.
fn bulk_copy(src: &kore_core::ColumnData, idxs: &[Option<usize>]) -> kore_core::ColumnData {
    use kore_core::ColumnData;
    match src {
        ColumnData::Int64(v) =>
            ColumnData::Int64(idxs.iter().map(|i| i.and_then(|r| v.get(r).and_then(|x| *x))).collect()),
        ColumnData::Float64(v) =>
            ColumnData::Float64(idxs.iter().map(|i| i.and_then(|r| v.get(r).and_then(|x| *x))).collect()),
        ColumnData::Bool(v) =>
            ColumnData::Bool(idxs.iter().map(|i| i.and_then(|r| v.get(r).and_then(|x| *x))).collect()),
        ColumnData::Str(v) =>
            ColumnData::Str(idxs.iter().map(|i| i.and_then(|r| v.get(r).and_then(|x| x.clone()))).collect()),
        ColumnData::StrDict { codes, dict } =>
            ColumnData::Str(idxs.iter().map(|i| i.and_then(|r| {
                let c = codes.get(r).copied().unwrap_or(u8::MAX);
                if c == u8::MAX { None } else { dict.get(c as usize).cloned() }
            })).collect()),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock, JoinType};

    fn make_blocks() -> (DataBlock, DataBlock) {
        let left = DataBlock::new(vec![
            Column::int64("id",  vec![Some(1), Some(2), Some(3)]),
            Column::str_col("name", vec![Some("alice".into()), Some("bob".into()), Some("carol".into())]),
        ]).unwrap();
        let right = DataBlock::new(vec![
            Column::int64("id",    vec![Some(2), Some(3), Some(4)]),
            Column::float64("score", vec![Some(9.1), Some(8.5), Some(7.2)]),
        ]).unwrap();
        (left, right)
    }

    #[test]
    fn inner_join() {
        let (l, r) = make_blocks();
        let cfg = JoinConfig::inner("id", "id");
        let result = HashJoin::join(&l, &r, &cfg).unwrap();
        assert_eq!(result.num_rows, 2);
    }

    #[test]
    fn left_join() {
        let (l, r) = make_blocks();
        let cfg = JoinConfig::left("id", "id");
        let result = HashJoin::join(&l, &r, &cfg).unwrap();
        assert_eq!(result.num_rows, 3);
    }

    #[test]
    fn full_outer_join() {
        let (l, r) = make_blocks();
        let cfg = JoinConfig::new("id", "id", JoinType::Full);
        let result = HashJoin::join(&l, &r, &cfg).unwrap();
        assert_eq!(result.num_rows, 4); // alice(unmatched) + bob+carol(matched) + 4(unmatched)
    }

    #[test]
    fn null_keys_never_match_but_outer_joins_keep_the_row() {
        // integer keys (fast path)
        let l = DataBlock::new(vec![Column::int64("k", vec![Some(1), None, Some(3)])]).unwrap();
        let r = DataBlock::new(vec![Column::int64("k", vec![None, Some(3)]), Column::int64("v", vec![Some(10), Some(30)])]).unwrap();
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::inner("k", "k")).unwrap().num_rows, 1);
        // LEFT keeps the NULL-key row and the unmatched row: 1 -> none, NULL -> none, 3 -> 30
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::left("k", "k")).unwrap().num_rows, 3);
        // string keys (generic path): NULL = NULL is not a match
        let l = DataBlock::new(vec![Column::str_col("k", vec![Some("a".into()), None])]).unwrap();
        let r = DataBlock::new(vec![Column::str_col("k", vec![None, Some("a".into())])]).unwrap();
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::inner("k", "k")).unwrap().num_rows, 1);
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::new("k", "k", JoinType::Full)).unwrap().num_rows, 3);
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 { self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); self.0 >> 33 }
        fn below(&mut self, n: u64) -> u64 { self.next() % n.max(1) }
    }

    fn chain(t: &FlatTable, k: i64) -> Vec<u32> {
        let mut v = Vec::new();
        let mut e = t.first(k);
        while e != FlatTable::NONE { v.push(e); e = t.next[e as usize]; }
        v
    }

    /// The parallel build gives the same chains (ascending row order) as the serial one for every key, and
    /// nothing for absent keys.
    #[test]
    fn parallel_build_matches_serial_chains() {
        let mut r = Rng(42);
        for &n in &[PAR_BUILD_MIN - 1, PAR_BUILD_MIN, PAR_BUILD_MIN + 1, 300_000] {
            // (dense unique, dense with duplicates, sparse, above 2^53 / near i64 limits, few keys, negative, all NULL)
            let gens: Vec<Box<dyn Fn(&mut Rng, usize) -> Option<i64>>> = vec![
                Box::new(|_, i| Some(i as i64)),
                Box::new(|r, _| if r.below(10) == 0 { None } else { Some(r.below(50_000) as i64) }),
                Box::new(|r, _| Some(r.next() as i64 * 7_919_017)),
                Box::new(|r, _| if r.below(7) == 0 { None } else { Some((1i64 << 53) + r.below(1 << 20) as i64) }),
                Box::new(|r, _| Some(i64::MAX - r.below(5) as i64)),
                Box::new(|r, _| Some(-(r.below(300_000) as i64) - 5)),
                Box::new(|_, _| None),
                Box::new(|r, _| if r.below(2) == 0 { Some(i64::MIN + r.below(3) as i64) } else { Some(r.next() as i64) }),
            ];
            for (gi, g) in gens.iter().enumerate() {
                let keys: Vec<Option<i64>> = (0..n).map(|i| g(&mut r, i)).collect();
                let s = FlatTable::build_serial(&keys);
                let t = FlatTable::build(&keys);
                assert_eq!(s.next, t.next, "next differs n={n} gen={gi}");
                for k in keys.iter().flatten().step_by(4999) { assert_eq!(chain(&s, *k), chain(&t, *k), "chain n={n} gen={gi} key={k}"); }
                for probe in [i64::MIN, -1, 0, 1, 12345, i64::MAX, (1i64 << 53) + (1 << 21), 999_999_999_937] {
                    assert_eq!(chain(&s, probe), chain(&t, probe), "absent n={n} gen={gi} probe={probe}");
                }
            }
        }
    }

    fn naive(lv: &[Option<i64>], rv: &[Option<i64>], jt: JoinType) -> Vec<(u32, u32)> {
        use std::collections::HashMap;
        let mut m: HashMap<i64, Vec<u32>> = HashMap::new();
        for (i, k) in rv.iter().enumerate() { if let Some(k) = k { m.entry(*k).or_default().push(i as u32); } }
        let mut out = Vec::new();
        let mut matched = vec![false; rv.len()];
        for (l, k) in lv.iter().enumerate() {
            match k.and_then(|k| m.get(&k)) {
                Some(rs) => for &r in rs { matched[r as usize] = true; out.push((l as u32, r)); },
                None => if matches!(jt, JoinType::Left | JoinType::Full) { out.push((l as u32, u32::MAX)); },
            }
        }
        if matches!(jt, JoinType::Right | JoinType::Full) {
            for (r, m) in matched.iter().enumerate() { if !m { out.push((u32::MAX, r as u32)); } }
        }
        out.sort();
        out
    }

    #[test]
    fn join_row_ids_match_naive_and_serial_across_sizes() {
        let mut r = Rng(7);
        for &(nl, nr, dom) in &[(0usize, 0usize, 4u64), (5, 0, 4), (0, 5, 4), (40_000, 140_000, 50_000), (200_000, 131_072, 1 << 40), (150_000, 20_000, 3_000), (70_000, 300_000, 1000)] {
            let mk = |r: &mut Rng, n: usize| -> Vec<Option<i64>> {
                (0..n).map(|_| if r.below(12) == 0 { None } else { Some(((1i64 << 53) + 3) * (r.below(2) as i64) + r.below(dom) as i64) }).collect()
            };
            let lv = mk(&mut r, nl);
            let rv = mk(&mut r, nr);
            for jt in [JoinType::Inner, JoinType::Left, JoinType::Right, JoinType::Full] {
                let (li, ri) = HashJoin::join_row_ids(&lv, &rv, jt);
                assert_eq!(li.len(), ri.len());
                let mut got: Vec<(u32, u32)> = li.iter().copied().zip(ri.iter().copied()).collect();
                got.sort();
                assert_eq!(got, naive(&lv, &rv, jt), "nl={nl} nr={nr} dom={dom} {jt:?}");
            }
        }
    }

    #[test]
    fn parallel_gather_matches_serial() {
        use kore_core::ColumnData;
        let n = PAR_GATHER_MIN * 3 + 17;
        let mut r = Rng(3);
        let ints: Vec<Option<i64>> = (0..1000).map(|i| if i % 5 == 0 { None } else { Some(i) }).collect();
        let strs: Vec<Option<String>> = (0..1000).map(|i| if i % 7 == 0 { None } else { Some(format!("s{i}")) }).collect();
        let codes: Vec<u8> = (0..1000).map(|i| if i % 9 == 0 { u8::MAX } else { (i % 5) as u8 }).collect();
        let idx: Vec<u32> = (0..n).map(|_| if r.below(10) == 0 { u32::MAX } else { r.below(1000) as u32 }).collect();
        let cols = vec![
            ColumnData::Int64(ints),
            ColumnData::Str(strs),
            ColumnData::StrDict { codes, dict: (0..5).map(|i| format!("d{i}")).collect() },
        ];
        for c in &cols {
            let par = gather(c, &idx);
            let ser: ColumnData = match c {
                ColumnData::Int64(v) => ColumnData::Int64(idx.iter().map(|&i| if i == u32::MAX { None } else { v[i as usize] }).collect()),
                ColumnData::Str(v) => ColumnData::Str(idx.iter().map(|&i| if i == u32::MAX { None } else { v[i as usize].clone() }).collect()),
                ColumnData::StrDict { codes, dict } => ColumnData::StrDict { codes: idx.iter().map(|&i| if i == u32::MAX { u8::MAX } else { codes[i as usize] }).collect(), dict: dict.clone() },
                _ => unreachable!(),
            };
            assert_eq!(par, ser);
            assert_eq!(clone_column(c), c.clone());
        }
    }

    /// Micro-benchmark of the join kernel stages, serial vs parallel in the same process (alternating, best of 5):
    /// `cargo test --release -p kore-join --offline -- --ignored --nocapture --test-threads 1 bench_join_kernel`
    #[test]
    #[ignore]
    fn bench_join_kernel() {
        use kore_core::ColumnData;
        use std::time::Instant;
        let mut r = Rng(1);
        for &(nb, np, stride) in &[(1_500_000usize, 6_000_000usize, 4i64), (4_500_000, 18_000_000, 4), (1_000_000, 6_000_000, 40_000_000_011)] {
            // unique build keys in random row order, fact keys drawn from them
            let mut bk: Vec<Option<i64>> = (0..nb as i64).map(|i| Some(i * stride)).collect();
            for i in (1..nb).rev() { let j = r.below(i as u64 + 1) as usize; bk.swap(i, j); }
            let pk: Vec<Option<i64>> = (0..np).map(|_| Some(r.below(nb as u64) as i64 * stride)).collect();
            let sc = ColumnData::Str((0..nb).map(|i| Some(format!("clerk#{:09}", i % 1000))).collect());
            let ic = ColumnData::Int64((0..nb as i64).map(Some).collect());
            // stage masks: 0 all parallel, 7 all serial, 1 only build serial, 2 only concat serial, 4 only gather serial
            let masks = [7u8, 0, 1, 2, 4];
            let mut best = vec![[f64::MAX; 4]; masks.len()];
            for _ in 0..5 {
                for (m, &mask) in masks.iter().enumerate() {
                    set_serial_stages(mask);
                    let t = Instant::now();
                    let tb = FlatTable::build(&bk);
                    best[m][0] = best[m][0].min(t.elapsed().as_secs_f64() * 1e3);
                    drop(tb);
                    let t = Instant::now();
                    let (li, ri) = HashJoin::join_row_ids(&pk, &bk, JoinType::Inner);
                    best[m][1] = best[m][1].min(t.elapsed().as_secs_f64() * 1e3);
                    let t = Instant::now();
                    let g = gather(&sc, &ri);
                    best[m][2] = best[m][2].min(t.elapsed().as_secs_f64() * 1e3);
                    let t = Instant::now();
                    let g2 = gather(&ic, &ri);
                    best[m][3] = best[m][3].min(t.elapsed().as_secs_f64() * 1e3);
                    assert_eq!(g.len(), li.len()); assert_eq!(g2.len(), li.len());
                }
            }
            set_serial_stages(0);
            println!("build={nb} probe={np} stride={stride}  [ms: FlatTable::build | join_row_ids | Str gather | Int gather]", );
            for (m, &mask) in masks.iter().enumerate() {
                println!("   serial-mask {mask}: {:.0} | {:.0} | {:.0} | {:.0}", best[m][0], best[m][1], best[m][2], best[m][3]);
            }
        }
    }

    #[test]
    fn float_keys_join_on_value_and_against_integers() {
        let l = DataBlock::new(vec![Column::float64("k", vec![Some(1.0), Some(2.5), Some(3.0)])]).unwrap();
        let r = DataBlock::new(vec![Column::float64("k", vec![Some(2.5), Some(9.0)])]).unwrap();
        // previously every float key hashed to NULL, which made this a cross join
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::inner("k", "k")).unwrap().num_rows, 1);
        let ri = DataBlock::new(vec![Column::int64("k", vec![Some(1), Some(3)])]).unwrap();
        assert_eq!(HashJoin::join(&l, &ri, &JoinConfig::inner("k", "k")).unwrap().num_rows, 2);
    }
}
