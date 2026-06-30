//! KORE Layer 29 — Distributed Shuffle & Hash Partitioning
//!
//! Implements the core shuffle algorithm that powers distributed GROUP BY,
//! distributed JOIN, and horizontal scale-out:
//!
//! - `HashPartitioner`  — assigns rows to partitions by hash(key) % n
//! - `RangePartitioner` — assigns rows to partitions by sorted value ranges
//! - `RoundRobinPartitioner` — even distribution for load balancing
//! - `distributed_group_by` — two-phase local→global aggregation
//! - `distributed_join`     — partitioned hash join
//!
//! # How it maps to Spark
//! Spark shuffle: repartition → local sort/agg → network transfer → final merge
//! KORE shuffle:  partition()  → local_agg()  → merge_partitions() (in-memory)

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use std::collections::HashMap;

// ── Hash Partitioner ──────────────────────────────────────────────────────────

pub struct HashPartitioner {
    pub n_partitions: usize,
    pub key_cols:     Vec<String>,
}

impl HashPartitioner {
    pub fn new(n_partitions: usize, key_cols: Vec<String>) -> Self {
        Self { n_partitions, key_cols }
    }

    /// Split block into n_partitions smaller DataBlocks.
    /// Rows with the same key always land in the same partition.
    pub fn partition(&self, block: &DataBlock) -> Vec<DataBlock> {
        let n  = block.num_rows;
        let np = self.n_partitions;
        let mut buckets: Vec<Vec<usize>> = vec![vec![]; np];

        for i in 0..n {
            let h = self.row_hash(block, i);
            buckets[h % np].push(i);
        }

        buckets.iter()
            .map(|indices| block.select_rows(indices))
            .collect()
    }

    /// Merge independently processed partition results back into one block.
    pub fn merge(blocks: Vec<DataBlock>) -> Result<DataBlock, KoreError> {
        let non_empty: Vec<DataBlock> = blocks.into_iter()
            .filter(|b| b.num_rows > 0)
            .collect();
        if non_empty.is_empty() { return Ok(DataBlock::empty()); }
        DataBlock::concat(non_empty)
    }

    fn row_hash(&self, block: &DataBlock, row: usize) -> usize {
        let mut h: u64 = 14695981039346656037; // FNV-1a basis
        for col_name in &self.key_cols {
            if let Some(col) = block.column(col_name) {
                let bytes: Vec<u8> = match &col.data {
                    ColumnData::Int64(v)  => v.get(row).and_then(|x| *x)
                        .map(|i| i.to_le_bytes().to_vec()).unwrap_or_default(),
                    ColumnData::Float64(v) => v.get(row).and_then(|x| *x)
                        .map(|f| f.to_bits().to_le_bytes().to_vec()).unwrap_or_default(),
                    ColumnData::Bool(v)    => v.get(row).and_then(|x| *x)
                        .map(|b| vec![b as u8]).unwrap_or_default(),
                    ColumnData::Str(v)     => v.get(row).and_then(|x| x.as_deref())
                        .map(|s| s.as_bytes().to_vec()).unwrap_or_default(),
                    ColumnData::StrDict { codes, dict } => {
                        let c = codes.get(row).copied().unwrap_or(u8::MAX);
                        if c == u8::MAX { vec![] } else { dict.get(c as usize).map(|s| s.as_bytes().to_vec()).unwrap_or_default() }
                    }
                };
                for byte in bytes {
                    h ^= byte as u64;
                    h = h.wrapping_mul(1099511628211); // FNV prime
                }
            }
        }
        h as usize
    }
}

// ── Round-Robin Partitioner ───────────────────────────────────────────────────

pub struct RoundRobinPartitioner {
    pub n_partitions: usize,
}

impl RoundRobinPartitioner {
    pub fn new(n: usize) -> Self { Self { n_partitions: n } }

    pub fn partition(&self, block: &DataBlock) -> Vec<DataBlock> {
        let np = self.n_partitions;
        let n  = block.num_rows;
        let mut buckets: Vec<Vec<usize>> = vec![vec![]; np];
        for i in 0..n { buckets[i % np].push(i); }
        buckets.iter().map(|idx| block.select_rows(idx)).collect()
    }
}

// ── Range Partitioner ─────────────────────────────────────────────────────────

pub struct RangePartitioner {
    pub n_partitions: usize,
    pub sort_col:     String,
    pub ascending:    bool,
}

impl RangePartitioner {
    pub fn new(n: usize, col: impl Into<String>) -> Self {
        Self { n_partitions: n, sort_col: col.into(), ascending: true }
    }

    /// Sort block by sort_col, then split into n equal-size partitions.
    pub fn partition(&self, block: &DataBlock) -> Result<Vec<DataBlock>, KoreError> {
        let sorted = block.sort_by(&self.sort_col, self.ascending)?;
        let chunk  = (sorted.num_rows + self.n_partitions - 1) / self.n_partitions;
        let parts  = (0..self.n_partitions).map(|p| {
            let start = p * chunk;
            let end   = ((p + 1) * chunk).min(sorted.num_rows);
            let indices: Vec<usize> = (start..end).collect();
            sorted.select_rows(&indices)
        }).filter(|b| b.num_rows > 0).collect();
        Ok(parts)
    }
}

// ── Distributed GROUP BY (two-phase aggregation) ─────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum AggOp { Sum, Avg, Count, Min, Max }

pub struct AggSpec {
    pub col:    String,
    pub op:     AggOp,
    pub output: String,
}

/// Two-phase distributed GROUP BY.
///
/// Phase 1: partition by group keys → compute partial aggregates locally per partition.
/// Phase 2: re-group partial aggregates → compute final result.
///
/// This is exactly what Spark's Exchange + HashAgg does.
pub fn distributed_group_by(
    block:      &DataBlock,
    group_keys: &[String],
    aggs:       &[AggSpec],
    n_workers:  usize,
) -> Result<DataBlock, KoreError> {
    // Phase 1: partition by group keys
    let partitioner = HashPartitioner::new(n_workers, group_keys.to_vec());
    let partitions  = partitioner.partition(block);

    // Phase 1b: compute local aggregates in each partition
    let partial: Vec<DataBlock> = partitions.iter()
        .filter(|p| p.num_rows > 0)
        .map(|p| local_group_by(p, group_keys, aggs))
        .collect::<Result<Vec<_>, _>>()?;

    if partial.is_empty() { return Ok(DataBlock::empty()); }

    // Phase 2: merge partial results and re-aggregate.
    // After phase 1, aggregate columns are already named by AggSpec.output,
    // so we aggregate on those names in phase 2.
    let merged = DataBlock::concat(partial)?;
    let phase2_aggs: Vec<AggSpec> = aggs.iter().map(|a| AggSpec {
        col:    a.output.clone(),  // column produced by phase 1
        op:     a.op,
        output: a.output.clone(),
    }).collect();
    local_group_by(&merged, group_keys, &phase2_aggs)
}

fn local_group_by(
    block:      &DataBlock,
    group_keys: &[String],
    aggs:       &[AggSpec],
) -> Result<DataBlock, KoreError> {
    let n = block.num_rows;

    // Build groups: key-string → row indices
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut key_order: Vec<String> = Vec::new();

    for i in 0..n {
        let key = group_keys.iter().map(|c| {
            block.column(c)
                .map(|col| match &col.data {
                    ColumnData::Int64(v)   => v.get(i).and_then(|x| *x).map(|v| v.to_string()).unwrap_or_default(),
                    ColumnData::Float64(v) => v.get(i).and_then(|x| *x).map(|v| format!("{v:.10}")).unwrap_or_default(),
                    ColumnData::Str(v)     => v.get(i).and_then(|x| x.as_deref()).unwrap_or("").to_string(),
                    ColumnData::StrDict { codes, dict } => {
                        let c = codes.get(i).copied().unwrap_or(u8::MAX);
                        if c == u8::MAX { String::new() } else { dict.get(c as usize).cloned().unwrap_or_default() }
                    }
                    ColumnData::Bool(v)    => v.get(i).and_then(|x| *x).map(|b| b.to_string()).unwrap_or_default(),
                })
                .unwrap_or_default()
        }).collect::<Vec<_>>().join("\x00");

        if !groups.contains_key(&key) { key_order.push(key.clone()); }
        groups.entry(key).or_default().push(i);
    }

    // Build output columns
    let ng = key_order.len();
    let mut out_cols: Vec<Column> = Vec::new();

    // Group key columns
    for key_col in group_keys {
        let col = block.column(key_col).ok_or_else(|| KoreError::ColumnNotFound(key_col.clone()))?;
        let data = match &col.data {
            ColumnData::Int64(_) => ColumnData::Int64(
                key_order.iter().map(|k| {
                    let first = groups[k][0];
                    if let ColumnData::Int64(v) = &col.data { v.get(first).copied().flatten() } else { None }
                }).collect()
            ),
            ColumnData::Float64(_) => ColumnData::Float64(
                key_order.iter().map(|k| {
                    let first = groups[k][0];
                    if let ColumnData::Float64(v) = &col.data { v.get(first).copied().flatten() } else { None }
                }).collect()
            ),
            ColumnData::Str(_) => ColumnData::Str(
                key_order.iter().map(|k| {
                    let first = groups[k][0];
                    if let ColumnData::Str(v) = &col.data { v.get(first).cloned().flatten() } else { None }
                }).collect()
            ),
            ColumnData::StrDict { .. } => ColumnData::Str(
                key_order.iter().map(|k| {
                    let first = groups[k][0];
                    if let ColumnData::StrDict { codes, dict } = &col.data {
                        let c = codes.get(first).copied().unwrap_or(u8::MAX);
                        if c == u8::MAX { None } else { dict.get(c as usize).cloned() }
                    } else { None }
                }).collect()
            ),
            ColumnData::Bool(_) => ColumnData::Bool(
                key_order.iter().map(|k| {
                    let first = groups[k][0];
                    if let ColumnData::Bool(v) = &col.data { v.get(first).copied().flatten() } else { None }
                }).collect()
            ),
        };
        out_cols.push(Column { name: key_col.clone(), data });
    }

    // Aggregate columns
    for spec in aggs {
        let agg_col = block.column(&spec.col)
            .ok_or_else(|| KoreError::ColumnNotFound(spec.col.clone()))?;

        let vals: Vec<Option<f64>> = key_order.iter().map(|k| {
            let indices = &groups[k];
            let nums: Vec<f64> = indices.iter().filter_map(|&i| match &agg_col.data {
                ColumnData::Float64(v) => v.get(i).and_then(|x| *x),
                ColumnData::Int64(v)   => v.get(i).and_then(|x| *x).map(|n| n as f64),
                _ => None,
            }).collect();

            if nums.is_empty() { return None; }
            Some(match spec.op {
                AggOp::Sum   => nums.iter().sum(),
                AggOp::Avg   => nums.iter().sum::<f64>() / nums.len() as f64,
                AggOp::Count => nums.len() as f64,
                AggOp::Min   => nums.iter().copied().fold(f64::INFINITY, f64::min),
                AggOp::Max   => nums.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            })
        }).collect();

        out_cols.push(Column {
            name: spec.output.clone(),
            data: ColumnData::Float64(vals),
        });
    }

    Ok(DataBlock { columns: out_cols, num_rows: ng })
}

// ── Distributed JOIN ──────────────────────────────────────────────────────────

/// Partition both sides on the join key, then hash-join matching partitions.
/// Equivalent to Spark's SortMergeJoin with exchange (shuffle) on both sides.
pub fn distributed_join(
    left:       &DataBlock,
    right:      &DataBlock,
    left_key:   &str,
    right_key:  &str,
    n_workers:  usize,
) -> Result<DataBlock, KoreError> {
    use kore_core::JoinType;
    use std::collections::HashMap;

    let lp = HashPartitioner::new(n_workers, vec![left_key.into()]);
    let rp = HashPartitioner::new(n_workers, vec![right_key.into()]);

    let l_parts = lp.partition(left);
    let r_parts = rp.partition(right);

    let results: Vec<DataBlock> = l_parts.iter().zip(r_parts.iter())
        .filter(|(l, r)| l.num_rows > 0 && r.num_rows > 0)
        .map(|(l, r)| {
            // Local hash join on this partition pair
            local_hash_join(l, r, left_key, right_key)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let non_empty: Vec<DataBlock> = results.into_iter().filter(|b| b.num_rows > 0).collect();
    if non_empty.is_empty() { return Ok(DataBlock::empty()); }
    DataBlock::concat(non_empty)
}

fn local_hash_join(
    left:      &DataBlock,
    right:     &DataBlock,
    left_key:  &str,
    right_key: &str,
) -> Result<DataBlock, KoreError> {
    // Build hash table from right side
    let mut ht: HashMap<String, Vec<usize>> = HashMap::new();
    for i in 0..right.num_rows {
        let k = row_key(right, right_key, i);
        ht.entry(k).or_default().push(i);
    }

    // Probe from left side
    let mut l_indices: Vec<usize> = vec![];
    let mut r_indices: Vec<usize> = vec![];

    for i in 0..left.num_rows {
        let k = row_key(left, left_key, i);
        if let Some(matches) = ht.get(&k) {
            for &j in matches {
                l_indices.push(i);
                r_indices.push(j);
            }
        }
    }

    // Build result block
    let mut cols: Vec<Column> = left.columns.iter().map(|c| Column {
        name: c.name.clone(),
        data: match &c.data {
            ColumnData::Int64(v)   => ColumnData::Int64(l_indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
            ColumnData::Float64(v) => ColumnData::Float64(l_indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
            ColumnData::Bool(v)    => ColumnData::Bool(l_indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
            ColumnData::Str(v)     => ColumnData::Str(l_indices.iter().map(|&i| v.get(i).cloned().flatten()).collect()),
            ColumnData::StrDict { codes, dict } => ColumnData::Str(l_indices.iter().map(|&i| {
                let c = codes.get(i).copied().unwrap_or(u8::MAX);
                if c == u8::MAX { None } else { dict.get(c as usize).cloned() }
            }).collect()),
        },
    }).collect();

    for c in right.columns.iter().filter(|c| c.name != right_key) {
        cols.push(Column {
            name: format!("right.{}", c.name),
            data: match &c.data {
                ColumnData::Int64(v)   => ColumnData::Int64(r_indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
                ColumnData::Float64(v) => ColumnData::Float64(r_indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
                ColumnData::Bool(v)    => ColumnData::Bool(r_indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
                ColumnData::Str(v)     => ColumnData::Str(r_indices.iter().map(|&i| v.get(i).cloned().flatten()).collect()),
                ColumnData::StrDict { codes, dict } => ColumnData::Str(r_indices.iter().map(|&i| {
                    let c = codes.get(i).copied().unwrap_or(u8::MAX);
                    if c == u8::MAX { None } else { dict.get(c as usize).cloned() }
                }).collect()),
            },
        });
    }

    let n = l_indices.len();
    Ok(DataBlock { columns: cols, num_rows: n })
}

fn row_key(block: &DataBlock, col_name: &str, row: usize) -> String {
    block.column(col_name).map(|c| match &c.data {
        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(|i| i.to_string()).unwrap_or_default(),
        ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| format!("{f:.10}")).unwrap_or_default(),
        ColumnData::Str(v)     => v.get(row).and_then(|x| x.as_deref()).unwrap_or("").to_string(),
        ColumnData::StrDict { codes, dict } => {
            let c = codes.get(row).copied().unwrap_or(u8::MAX);
            if c == u8::MAX { String::new() } else { dict.get(c as usize).cloned().unwrap_or_default() }
        }
        ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).map(|b| b.to_string()).unwrap_or_default(),
    }).unwrap_or_default()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::Column;

    fn data_block(n: usize, n_groups: usize) -> DataBlock {
        DataBlock::new(vec![
            Column::str_col("cat",
                (0..n).map(|i| Some(format!("g{}", i % n_groups))).collect()),
            Column::float64("val",
                (0..n).map(|i| Some(i as f64)).collect()),
        ]).unwrap()
    }

    #[test]
    fn test_hash_partition_roundtrip() {
        let b = data_block(1000, 10);
        let p = HashPartitioner::new(4, vec!["cat".into()]);
        let parts = p.partition(&b);
        assert_eq!(parts.len(), 4);
        let total: usize = parts.iter().map(|x| x.num_rows).sum();
        assert_eq!(total, 1000);
    }

    #[test]
    fn test_distributed_group_by() {
        let b = data_block(200, 5);
        let result = distributed_group_by(
            &b,
            &["cat".to_string()],
            &[AggSpec { col: "val".into(), op: AggOp::Sum, output: "total".into() }],
            4,
        ).unwrap();
        // 5 groups, each with 40 rows, sum = 0+5+10+...+(200-5) for each group
        assert_eq!(result.num_rows, 5);
    }

    #[test]
    fn test_distributed_join() {
        let left = DataBlock::new(vec![
            Column::int64("id", (0..10).map(|i| Some(i as i64)).collect()),
            Column::float64("lval", (0..10).map(|i| Some(i as f64 * 2.0)).collect()),
        ]).unwrap();
        let right = DataBlock::new(vec![
            Column::int64("id", (5..15).map(|i| Some(i as i64)).collect()),
            Column::float64("rval", (5..15).map(|i| Some(i as f64 * 3.0)).collect()),
        ]).unwrap();
        let joined = distributed_join(&left, &right, "id", "id", 4).unwrap();
        // Overlap: ids 5..10 → 5 rows
        assert_eq!(joined.num_rows, 5);
    }
}
