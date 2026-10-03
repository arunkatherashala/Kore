//! Memory-bounded operators used by the executor when a memory limit is set and
//! an operator's input is estimated to exceed it:
//!
//! * [`external_sort`]       — run generation + k-way merge (stable, multi-key)
//! * [`partitioned_group_by`] — grace-hash GROUP BY (partition to disk, aggregate per partition)
//! * [`partitioned_join`]    — grace-hash JOIN (partition both sides to disk, join per partition)
//!
//! What is bounded: each operator's *working state* (sort run + keys, hash tables, join
//! index pairs).  The input blocks already live in memory (tables are registered as
//! `DataBlock`s) and the final result is materialised in memory; neither is bounded here.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering as AtOrd;

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use kore_join::{HashJoin, JoinConfig};

use crate::ast::Projection;
use crate::executor::{group_by_agg_ex, group_partition_key};
use crate::spill::{estimate_bytes, SpillDir, SpillFile, SpillReader, SpillStats};

/// Everything an operator needs to decide about / perform spilling.
pub(crate) struct SpillCtx<'a> {
    pub limit: usize,
    pub dir: Option<&'a Path>,
    pub stats: &'a Arc<SpillStats>,
}

const MAX_PARTITIONS: usize = 64;
const MAX_RUNS: usize = 256;
const MIN_RUN_ROWS: usize = 8;
const MAX_CHUNK_ROWS: usize = 8192;
const MIN_CHUNK_ROWS: usize = 16;
const MAX_PAGE_ROWS: usize = 1024;

fn bytes_per_row(b: &DataBlock) -> usize {
    (estimate_bytes(b) / b.num_rows.max(1)).max(1)
}

fn n_partitions(total_bytes: usize, limit: usize) -> usize {
    let half = (limit / 2).max(1);
    ((total_bytes + half - 1) / half).clamp(2, MAX_PARTITIONS)
}

// ─── Sort ────────────────────────────────────────────────────────────────────

/// Sort key value.  NULL mapping mirrors `DataBlock::sort_by` exactly:
/// Int64 NULL = i64::MIN, Float64 NULL = f64::MAX, Bool NULL = false, string NULL = "".
#[derive(Clone, Debug)]
enum Key { I(i64), F(f64), B(u8), S(String) }

fn key_at(data: &ColumnData, row: usize) -> Key {
    match data {
        ColumnData::Int64(v)   => Key::I(v[row].unwrap_or(i64::MIN)),
        ColumnData::Float64(v) => Key::F(v[row].unwrap_or(f64::MAX)),
        ColumnData::Bool(v)    => Key::B(v[row].map_or(0, |b| b as u8)),
        ColumnData::Str(v)     => Key::S(v[row].clone().unwrap_or_default()),
        ColumnData::StrDict { .. } => Key::S(data.get_str(row).unwrap_or("").to_string()),
    }
}

fn cmp_key(a: &Key, b: &Key, desc: bool) -> Ordering {
    let o = match (a, b) {
        (Key::I(x), Key::I(y)) => x.cmp(y),
        (Key::F(x), Key::F(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Key::B(x), Key::B(y)) => x.cmp(y),
        (Key::S(x), Key::S(y)) => x.cmp(y),
        _ => Ordering::Equal,
    };
    if desc { o.reverse() } else { o }
}

fn cmp_keys(a: &[Key], b: &[Key], desc: &[bool]) -> Ordering {
    for i in 0..a.len() {
        let o = cmp_key(&a[i], &b[i], desc[i]);
        if o != Ordering::Equal { return o; }
    }
    Ordering::Equal
}

fn row_keys(block: &DataBlock, cols: &[usize], row: usize) -> Vec<Key> {
    cols.iter().map(|&c| key_at(&block.columns[c].data, row)).collect()
}

/// Stable lexicographic multi-key sort, entirely in memory.
#[allow(dead_code)]
#[allow(dead_code)]
pub(crate) fn sort_multi_in_memory(block: &DataBlock, keys: &[(usize, bool)]) -> DataBlock {
    let cols: Vec<usize> = keys.iter().map(|k| k.0).collect();
    let desc: Vec<bool>  = keys.iter().map(|k| k.1).collect();
    let mut rows: Vec<(Vec<Key>, usize)> =
        (0..block.num_rows).map(|r| (row_keys(block, &cols, r), r)).collect();
    rows.sort_by(|a, b| cmp_keys(&a.0, &b.0, &desc).then(a.1.cmp(&b.1)));
    let idx: Vec<usize> = rows.into_iter().map(|(_, r)| r).collect();
    block.select_rows(&idx)
}

struct HeapEntry<'a> { keys: Vec<Key>, run: usize, desc: &'a [bool] }

impl PartialEq for HeapEntry<'_> { fn eq(&self, o: &Self) -> bool { self.cmp(o) == Ordering::Equal } }
impl Eq for HeapEntry<'_> {}
impl PartialOrd for HeapEntry<'_> { fn partial_cmp(&self, o: &Self) -> Option<Ordering> { Some(self.cmp(o)) } }
impl Ord for HeapEntry<'_> {
    // BinaryHeap is a max-heap; reverse so the smallest (key, run) pops first.
    // Ties break on run index => globally stable (runs are in input order).
    fn cmp(&self, o: &Self) -> Ordering {
        cmp_keys(&self.keys, &o.keys, self.desc).then(self.run.cmp(&o.run)).reverse()
    }
}

struct RunCursor { rd: SpillReader, page: Option<DataBlock>, pos: usize }

impl RunCursor {
    fn open(f: &SpillFile) -> Result<Self, KoreError> {
        let mut c = RunCursor { rd: f.reader()?, page: None, pos: 0 };
        c.page = c.rd.next_page()?;
        Ok(c)
    }
    /// Advance past the current row; true if another row is available.
    fn advance(&mut self) -> Result<bool, KoreError> {
        self.pos += 1;
        if let Some(p) = &self.page {
            if self.pos < p.num_rows { return Ok(true); }
        }
        loop {
            self.page = self.rd.next_page()?;
            self.pos = 0;
            match &self.page {
                None => return Ok(false),
                Some(p) if p.num_rows > 0 => return Ok(true),
                _ => {}
            }
        }
    }
}

fn push_row(dst: &mut ColumnData, src: &ColumnData, row: usize) -> Result<(), KoreError> {
    match (dst, src) {
        (ColumnData::Int64(d),   ColumnData::Int64(s))   => d.push(s[row]),
        (ColumnData::Float64(d), ColumnData::Float64(s)) => d.push(s[row]),
        (ColumnData::Bool(d),    ColumnData::Bool(s))    => d.push(s[row]),
        (ColumnData::Str(d),     ColumnData::Str(s))     => d.push(s[row].clone()),
        (ColumnData::StrDict { codes: d, .. }, ColumnData::StrDict { codes: s, .. }) => d.push(s[row]),
        _ => return Err(KoreError::SchemaMismatch("spill page type mismatch".into())),
    }
    Ok(())
}

/// External merge sort over `keys` (column index, descending) — stable.
pub(crate) fn external_sort(
    block: &DataBlock,
    keys: &[(usize, bool)],
    sc: &SpillCtx,
) -> Result<DataBlock, KoreError> {
    let n = block.num_rows;
    if n == 0 { return Ok(block.clone()); }
    sc.stats.sort_spills.fetch_add(1, AtOrd::Relaxed);

    let cols: Vec<usize> = keys.iter().map(|k| k.0).collect();
    let desc: Vec<bool>  = keys.iter().map(|k| k.1).collect();

    // Rows per run: a run needs its keys + a sorted copy of one page at a time.
    let mut run_rows = (sc.limit / (3 * bytes_per_row(block))).max(MIN_RUN_ROWS);
    if (n + run_rows - 1) / run_rows > MAX_RUNS { run_rows = (n + MAX_RUNS - 1) / MAX_RUNS; }
    let page_rows = run_rows.min(MAX_PAGE_ROWS).max(1);

    let mut dir = SpillDir::create(sc.dir, sc.stats.clone())?;

    // ── Phase 1: sorted runs ────────────────────────────────────────────────
    let mut runs: Vec<SpillFile> = Vec::new();
    let mut start = 0;
    while start < n {
        let end = (start + run_rows).min(n);
        let mut rows: Vec<(Vec<Key>, usize)> =
            (start..end).map(|r| (row_keys(block, &cols, r), r)).collect();
        rows.sort_by(|a, b| cmp_keys(&a.0, &b.0, &desc).then(a.1.cmp(&b.1)));
        let idx: Vec<usize> = rows.into_iter().map(|(_, r)| r).collect();
        let mut w = dir.new_file()?;
        for chunk in idx.chunks(page_rows) { w.write_page(&block.select_rows(chunk))?; }
        runs.push(w.finish()?);
        start = end;
    }

    // ── Phase 2: k-way merge ────────────────────────────────────────────────
    let mut cursors: Vec<RunCursor> = Vec::with_capacity(runs.len());
    let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::with_capacity(runs.len());
    for (i, f) in runs.iter().enumerate() {
        let c = RunCursor::open(f)?;
        if let Some(p) = &c.page {
            if p.num_rows > 0 { heap.push(HeapEntry { keys: row_keys(p, &cols, 0), run: i, desc: &desc }); }
        }
        cursors.push(c);
    }
    let mut out: Vec<ColumnData> = block.columns.iter().map(|c| c.data.empty_like()).collect();
    while let Some(e) = heap.pop() {
        let cur = &mut cursors[e.run];
        {
            let page = cur.page.as_ref().unwrap();
            for (o, c) in out.iter_mut().zip(&page.columns) { push_row(o, &c.data, cur.pos)?; }
        }
        if cur.advance()? {
            let page = cur.page.as_ref().unwrap();
            heap.push(HeapEntry { keys: row_keys(page, &cols, cur.pos), run: e.run, desc: &desc });
        }
    }
    let columns = block.columns.iter().zip(out)
        .map(|(c, data)| Column { name: c.name.clone(), data })
        .collect();
    Ok(DataBlock { columns, num_rows: n })
}

// ─── Partitioning helpers ────────────────────────────────────────────────────

fn chunk_rows(limit: usize, block: &DataBlock) -> usize {
    (limit / (4 * bytes_per_row(block))).clamp(MIN_CHUNK_ROWS, MAX_CHUNK_ROWS)
}

fn mix(h: u64) -> u64 {
    // splitmix64 finaliser
    let mut z = h.wrapping_add(0x9e3779b97f4a7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Stream `block` into `nparts` spill files; `part_of(row)` picks the partition.
/// If `with_rowid`, an extra trailing Int64 column holds each row's original index.
fn partition_to_disk(
    block: &DataBlock,
    nparts: usize,
    dir: &mut SpillDir,
    chunk: usize,
    with_rowid: bool,
    part_of: &dyn Fn(usize) -> usize,
) -> Result<Vec<SpillFile>, KoreError> {
    let mut writers = Vec::with_capacity(nparts);
    for _ in 0..nparts { writers.push(dir.new_file()?); }
    let mut start = 0;
    while start < block.num_rows {
        let end = (start + chunk).min(block.num_rows);
        let mut lists: Vec<Vec<usize>> = vec![Vec::new(); nparts];
        for r in start..end { lists[part_of(r)].push(r); }
        for (p, idx) in lists.iter().enumerate() {
            if idx.is_empty() { continue; }
            let mut page = block.select_rows(idx);
            if with_rowid {
                page.columns.push(Column {
                    name: "__kore_rowid".into(),
                    data: ColumnData::Int64(idx.iter().map(|&i| Some(i as i64)).collect()),
                });
            }
            writers[p].write_page(&page)?;
        }
        start = end;
    }
    writers.into_iter().map(|w| w.finish()).collect()
}

// ─── Grace-hash GROUP BY ─────────────────────────────────────────────────────

/// Same result (including group order = order of first appearance) as the in-memory
/// GROUP BY, but aggregates one hash partition at a time.  A partition that is itself
/// larger than the limit (e.g. one giant group) is NOT re-partitioned.
pub(crate) fn partitioned_group_by(
    block: DataBlock,
    group_cols: &[String],
    projections: &[Projection],
    sc: &SpillCtx,
) -> Result<DataBlock, KoreError> {
    if block.num_rows == 0 {
        return group_by_agg_ex(block, group_cols, projections).map(|r| r.0);
    }
    sc.stats.group_spills.fetch_add(1, AtOrd::Relaxed);
    let nparts = n_partitions(estimate_bytes(&block), sc.limit);
    let mut dir = SpillDir::create(sc.dir, sc.stats.clone())?;

    let files = partition_to_disk(
        &block, nparts, &mut dir, chunk_rows(sc.limit, &block), true,
        &|r| {
            let k = group_partition_key(&block, group_cols, r);
            (mix((k >> 64) as u64 ^ k as u64) % nparts as u64) as usize
        },
    )?;

    let mut template = block.select_rows(&[]);
    template.columns.push(Column { name: "__kore_rowid".into(), data: ColumnData::Int64(vec![]) });
    drop(block); // input no longer needed; partitions are on disk

    let mut results: Vec<DataBlock> = Vec::new();
    let mut order: Vec<(i64, usize, usize)> = Vec::new(); // (first original row, result block, row)
    for f in &files {
        if f.pages == 0 { continue; }
        let mut part = f.read_all(&template)?;
        let rowid_col = part.columns.pop().unwrap();
        let rowids: Vec<i64> = match rowid_col.data {
            ColumnData::Int64(v) => v.into_iter().map(|x| x.unwrap_or(0)).collect(),
            _ => unreachable!(),
        };
        part.num_rows = rowids.len();
        let (res, firsts) = group_by_agg_ex(part, group_cols, projections)?;
        let bi = results.len();
        for (i, fr) in firsts.iter().enumerate() { order.push((rowids[*fr], bi, i)); }
        results.push(res);
    }

    let mut offsets = Vec::with_capacity(results.len());
    let mut acc = 0usize;
    for r in &results { offsets.push(acc); acc += r.num_rows; }
    let all = DataBlock::concat(results)?;
    order.sort_by_key(|o| o.0);
    // A group's output row count can differ from its group count only if projections
    // produced no columns; num_rows is carried by `concat` in that case.
    let perm: Vec<usize> = order.iter().map(|&(_, b, i)| offsets[b] + i).collect();
    if all.columns.is_empty() {
        return Ok(DataBlock { columns: vec![], num_rows: perm.len() });
    }
    Ok(all.select_rows(&perm))
}

// ─── Grace-hash JOIN ─────────────────────────────────────────────────────────

fn join_key_hash(data: &ColumnData, row: usize) -> u64 {
    // Mirrors kore_core::JoinKey equality: Int / Bool / Str; everything else (NULL,
    // Float64) is the single "Null" key.  Equal keys MUST hash equally on both sides.
    let mut h = DefaultHasher::new();
    match data {
        ColumnData::Int64(v) => match v[row] {
            Some(i) => { 0u8.hash(&mut h); i.hash(&mut h); }
            None    => 3u8.hash(&mut h),
        },
        ColumnData::Bool(v) => match v[row] {
            Some(b) => { 1u8.hash(&mut h); b.hash(&mut h); }
            None    => 3u8.hash(&mut h),
        },
        ColumnData::Str(_) | ColumnData::StrDict { .. } => match data.get_str(row) {
            Some(s) => { 2u8.hash(&mut h); s.hash(&mut h); }
            None    => 3u8.hash(&mut h),
        },
        ColumnData::Float64(_) => 3u8.hash(&mut h),
    }
    mix(h.finish())
}

/// Partition both inputs by join-key hash, join matching partition pairs with the
/// regular `HashJoin`, concatenate.  Row order of the output is NOT the same as the
/// in-memory join (it is grouped by partition); the multiset of rows is identical.
pub(crate) fn partitioned_join(
    left: &DataBlock,
    right: &DataBlock,
    cfg: &JoinConfig,
    sc: &SpillCtx,
) -> Result<DataBlock, KoreError> {
    let lcol = left.columns.iter().position(|c| c.name == cfg.left_key);
    let rcol = right.columns.iter().position(|c| c.name == cfg.right_key);
    let (lcol, rcol) = match (lcol, rcol) {
        (Some(l), Some(r)) => (l, r),
        _ => return HashJoin::join(left, right, cfg), // preserves the original error behaviour
    };
    sc.stats.join_spills.fetch_add(1, AtOrd::Relaxed);
    let total = estimate_bytes(left) + estimate_bytes(right);
    let nparts = n_partitions(total, sc.limit);
    let mut dir = SpillDir::create(sc.dir, sc.stats.clone())?;

    let np = nparts as u64;
    let lfiles = partition_to_disk(left, nparts, &mut dir, chunk_rows(sc.limit, left), false,
        &|r| (join_key_hash(&left.columns[lcol].data, r) % np) as usize)?;
    let rfiles = partition_to_disk(right, nparts, &mut dir, chunk_rows(sc.limit, right), false,
        &|r| (join_key_hash(&right.columns[rcol].data, r) % np) as usize)?;

    let ltemplate = left.select_rows(&[]);
    let rtemplate = right.select_rows(&[]);
    let mut outs: Vec<DataBlock> = Vec::with_capacity(nparts);
    for (lf, rf) in lfiles.iter().zip(&rfiles) {
        let l = lf.read_all(&ltemplate)?;
        let r = rf.read_all(&rtemplate)?;
        outs.push(HashJoin::join(&l, &r, cfg)?);
    }
    DataBlock::concat(outs)
}
