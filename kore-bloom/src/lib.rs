//! KORE Layer 36 — Bloom Filter Joins
//!
//! A Bloom filter is a probabilistic set membership structure that lets us
//! skip rows during joins with zero false negatives.  Used as a pre-filter
//! before the expensive hash-join probe phase.
//!
//! Architecture:
//!   1. **Build phase** — scan the smaller (build) side and insert all join-key
//!      values into a Bloom filter.
//!   2. **Probe phase** — scan the larger (probe) side; any row whose key is
//!      definitely NOT in the filter is discarded immediately without touching
//!      the hash table.
//!   3. **Hash join** — remaining rows go through the normal hash join.
//!
//! This mirrors Apache Spark's DynamicBroadcastHashJoin strategy.

use kore_core::{Column, ColumnData, DataBlock, KoreError, JoinType};
use kore_core::Value;

// ─── Bloom filter ─────────────────────────────────────────────────────────────

/// A simple Bloom filter backed by a bit-array.
/// Uses `k = 7` independent hash functions (FNV-based double-hashing).
pub struct BloomFilter {
    bits:     Vec<u64>,   // packed bit-array
    num_bits: usize,
    k:        usize,      // number of hash functions
}

impl BloomFilter {
    /// Create a filter sized for `expected_items` at `false_positive_rate`.
    pub fn new(expected_items: usize, false_positive_rate: f64) -> Self {
        // m = -n * ln(p) / (ln(2)^2)
        let m = (-(expected_items as f64) * false_positive_rate.ln()
                  / std::f64::consts::LN_2.powi(2))
                  .ceil() as usize;
        let m = m.max(64);
        // k = (m/n) * ln(2)
        let k = ((m as f64 / expected_items as f64) * std::f64::consts::LN_2)
                  .round() as usize;
        let k = k.max(1).min(15);
        BloomFilter {
            bits: vec![0u64; (m + 63) / 64],
            num_bits: m,
            k,
        }
    }

    fn hashes(&self, key: u64) -> impl Iterator<Item = usize> + '_ {
        // Double-hashing: h_i(x) = h1(x) + i * h2(x)
        let h1 = fnv1a(key);
        let h2 = fnv1a(key.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        (0..self.k).map(move |i| {
            (h1.wrapping_add((i as u64).wrapping_mul(h2)) % self.num_bits as u64) as usize
        })
    }

    pub fn insert(&mut self, key: u64) {
        for bit in self.hashes(key).collect::<Vec<_>>() {
            self.bits[bit / 64] |= 1u64 << (bit % 64);
        }
    }

    pub fn may_contain(&self, key: u64) -> bool {
        self.hashes(key).all(|bit| (self.bits[bit / 64] >> (bit % 64)) & 1 == 1)
    }

    /// Insert a string key (hashed).
    pub fn insert_str(&mut self, s: &str) { self.insert(fnv1a_str(s)); }
    /// Query a string key.
    pub fn may_contain_str(&self, s: &str) -> bool { self.may_contain(fnv1a_str(s)); false || self.may_contain(fnv1a_str(s)) }

    /// Estimated false positive rate given the number of insertions.
    pub fn false_positive_rate(&self, n: usize) -> f64 {
        let m = self.num_bits as f64;
        let k = self.k as f64;
        let n = n as f64;
        (1.0 - (-k * n / m).exp()).powf(k)
    }
}

// ─── FNV-1a hash ─────────────────────────────────────────────────────────────

fn fnv1a(val: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in val.to_le_bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn fnv1a_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ─── Cell → u64 key ──────────────────────────────────────────────────────────

fn cell_key(data: &ColumnData, row: usize) -> Option<u64> {
    match data {
        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(|i| i as u64),
        ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| f.to_bits()),
        ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).map(|b| b as u64),
        ColumnData::Str(v)     => v.get(row).and_then(|x| x.as_deref()).map(fnv1a_str),
        ColumnData::StrDict { codes, dict } => codes.get(row).and_then(|&c| if c == u8::MAX { None } else { dict.get(c as usize) }).map(|s| fnv1a_str(s)),
    }
}

fn str_key(data: &ColumnData, row: usize) -> Option<String> {
    match data {
        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(|i| i.to_string()),
        ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| format!("{f:.10}")),
        ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).map(|b| b.to_string()),
        ColumnData::Str(v)     => v.get(row).and_then(|x| x.clone()),
        ColumnData::StrDict { codes, dict } => codes.get(row).and_then(|&c| if c == u8::MAX { None } else { dict.get(c as usize).cloned() }),
    }
}

// ─── Bloom-filter inner join ──────────────────────────────────────────────────

/// Inner join with a Bloom filter pre-filter.
///
/// - `build` side: smaller table → its join keys populate the filter.
/// - `probe` side: larger table → rows that fail the filter are skipped.
///
/// Returns a new block containing matched rows from both sides (columns
/// from probe first, then build — same convention as kore-join).
pub fn bloom_join(
    build: &DataBlock,
    probe: &DataBlock,
    build_key: &str,
    probe_key: &str,
) -> Result<DataBlock, KoreError> {
    // 1. Find key columns
    let bk_col = find_col(build, build_key)
        .ok_or_else(|| KoreError::InvalidArgument(format!("build key not found: {build_key}")))?;
    let pk_col = find_col(probe, probe_key)
        .ok_or_else(|| KoreError::InvalidArgument(format!("probe key not found: {probe_key}")))?;

    // 2. Build Bloom filter from build side
    let mut filter = BloomFilter::new(build.num_rows.max(16), 0.01);
    for r in 0..build.num_rows {
        if let Some(k) = cell_key(&bk_col.data, r) { filter.insert(k); }
    }

    // 3. Pre-filter probe side
    let probe_candidates: Vec<usize> = (0..probe.num_rows)
        .filter(|&r| {
            cell_key(&pk_col.data, r)
                .map(|k| filter.may_contain(k))
                .unwrap_or(false)
        })
        .collect();
    let probe_filtered = probe.select_rows(&probe_candidates);

    // 4. Hash join on filtered probe side
    hash_join_inner(&probe_filtered, build, probe_key, build_key)
}

/// Simple in-memory hash join (inner).
fn hash_join_inner(
    left: &DataBlock,
    right: &DataBlock,
    left_key: &str,
    right_key: &str,
) -> Result<DataBlock, KoreError> {
    use std::collections::HashMap;

    let lk = find_col(left,  left_key)
        .ok_or_else(|| KoreError::InvalidArgument(format!("left key not found: {left_key}")))?;
    let rk = find_col(right, right_key)
        .ok_or_else(|| KoreError::InvalidArgument(format!("right key not found: {right_key}")))?;

    // Build hash map from right side: key_str → Vec<row>
    let mut ht: HashMap<String, Vec<usize>> = HashMap::new();
    for r in 0..right.num_rows {
        if let Some(k) = str_key(&rk.data, r) {
            ht.entry(k).or_default().push(r);
        }
    }

    // Probe from left side
    let mut left_rows:  Vec<usize> = Vec::new();
    let mut right_rows: Vec<usize> = Vec::new();

    for l in 0..left.num_rows {
        if let Some(k) = str_key(&lk.data, l) {
            if let Some(matches) = ht.get(&k) {
                for &r in matches {
                    left_rows.push(l);
                    right_rows.push(r);
                }
            }
        }
    }

    // Build output columns
    let mut cols: Vec<Column> = Vec::new();
    for c in &left.columns {
        cols.push(select_col(c, &left_rows));
    }
    for c in &right.columns {
        let mut nc = select_col(c, &right_rows);
        // Avoid duplicate key column name
        if nc.name == right_key { nc.name = format!("{}.right", nc.name); }
        cols.push(nc);
    }

    let num_rows = left_rows.len();
    Ok(DataBlock { columns: cols, num_rows })
}

fn find_col<'a>(block: &'a DataBlock, name: &str) -> Option<&'a Column> {
    block.columns.iter().find(|c| c.name == name || c.name.ends_with(&format!(".{}", name)))
}

fn select_col(col: &Column, indices: &[usize]) -> Column {
    let data = match &col.data {
        ColumnData::Int64(v)   => ColumnData::Int64  (indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
        ColumnData::Float64(v) => ColumnData::Float64(indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
        ColumnData::Bool(v)    => ColumnData::Bool   (indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
        ColumnData::Str(v)     => ColumnData::Str    (indices.iter().map(|&i| v.get(i).cloned().flatten()).collect()),
        ColumnData::StrDict { codes, dict } => ColumnData::StrDict {
            codes: indices.iter().map(|&i| codes.get(i).copied().unwrap_or(u8::MAX)).collect(),
            dict: dict.clone(),
        },
    };
    Column { name: col.name.clone(), data }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    #[test]
    fn test_bloom_filter_basics() {
        let mut bf = BloomFilter::new(1000, 0.01);
        bf.insert(42);
        bf.insert(100);
        assert!(bf.may_contain(42));
        assert!(bf.may_contain(100));
        // Very unlikely to have a false positive for completely unrelated value
        // (we can't assert !may_contain because of FP, but 99.9% it won't match)
        let fpr = bf.false_positive_rate(2);
        assert!(fpr < 0.05);  // < 5% FPR with only 2 items in a 1000-item filter
    }

    #[test]
    fn test_bloom_join() {
        let build = DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(),   data: ColumnData::Int64(vec![Some(1),Some(2),Some(3)]) },
                Column { name: "name".into(), data: ColumnData::Str(vec![
                    Some("Alice".into()), Some("Bob".into()), Some("Carol".into())]) },
            ],
        };
        let probe = DataBlock {
            num_rows: 5,
            columns: vec![
                Column { name: "user_id".into(), data: ColumnData::Int64(vec![
                    Some(1), Some(2), Some(99), Some(3), Some(99)]) },
                Column { name: "score".into(),   data: ColumnData::Float64(vec![
                    Some(90.0), Some(80.0), Some(70.0), Some(95.0), Some(60.0)]) },
            ],
        };
        let result = bloom_join(&build, &probe, "id", "user_id").unwrap();
        // user_id 1, 2, 3 match; 99 does not → 3 output rows
        assert_eq!(result.num_rows, 3);
    }

    #[test]
    fn test_bloom_no_matches() {
        let build = DataBlock {
            num_rows: 2,
            columns: vec![
                Column { name: "k".into(), data: ColumnData::Int64(vec![Some(1), Some(2)]) },
            ],
        };
        let probe = DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "k".into(), data: ColumnData::Int64(vec![Some(10), Some(20), Some(30)]) },
                Column { name: "v".into(), data: ColumnData::Float64(vec![Some(1.0), Some(2.0), Some(3.0)]) },
            ],
        };
        let result = bloom_join(&build, &probe, "k", "k").unwrap();
        assert_eq!(result.num_rows, 0);
    }
}
