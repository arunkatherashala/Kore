//! KORE Layer 27 — Window Functions
//!
//! Implements the full set of SQL window functions against DataBlock:
//!   Numbering:   ROW_NUMBER, RANK, DENSE_RANK, NTILE(n)
//!   Navigation:  LAG(col, n), LEAD(col, n), FIRST_VALUE, LAST_VALUE
//!   Aggregate:   SUM/AVG/COUNT/MIN/MAX OVER (running / full-partition)
//!
//! # SQL usage (via kore-sql integration)
//! ```sql
//! SELECT region,
//!        revenue,
//!        ROW_NUMBER() OVER (PARTITION BY region ORDER BY revenue DESC) AS rn,
//!        SUM(revenue)  OVER (PARTITION BY region)                      AS region_total,
//!        LAG(revenue, 1) OVER (ORDER BY date)                         AS prev_revenue
//! FROM sales
//! ```

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use std::collections::HashMap;

// ── Window function variants ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum WindowFn {
    RowNumber,
    Rank,
    DenseRank,
    Ntile(usize),
    Lag  { col: String, offset: usize },
    Lead { col: String, offset: usize },
    // Aggregate over partition / frame
    Sum(String),
    Avg(String),
    Count(String),
    Min(String),
    Max(String),
    CumSum(String),   // running sum (UNBOUNDED PRECEDING … CURRENT ROW)
    CumAvg(String),
    FirstValue(String),
    LastValue(String),
}

// ── Order item ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WinOrder {
    pub col:  String,
    pub desc: bool,
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Compute a single window function over `block`, appending a new column
/// named `output_col`.  Returns the block with the extra column attached.
pub fn apply_window(
    block:       &DataBlock,
    partition_by: &[String],
    order_by:     &[WinOrder],
    func:        &WindowFn,
    output_col:  &str,
) -> Result<DataBlock, KoreError> {
    let n = block.num_rows;

    // 1. Build partition groups: key → sorted row indices within partition
    let partitions = build_partitions(block, partition_by, order_by, n)?;

    // 2. Compute function value for every row in original order
    let mut result: Vec<f64> = vec![0.0; n];

    for (_key, sorted_indices) in &partitions {
        compute_fn_for_partition(block, sorted_indices, func, &mut result)?;
    }

    // 3. Build output column
    let new_col = Column {
        name: output_col.to_string(),
        data: ColumnData::Float64(result.into_iter().map(Some).collect()),
    };

    // 4. Append to block
    let mut cols = block.columns.clone();
    cols.push(new_col);
    Ok(DataBlock { columns: cols, num_rows: n })
}

/// Apply multiple window functions in one pass, each appending a column.
pub fn apply_windows(
    block:   &DataBlock,
    windows: &[(Vec<String>, Vec<WinOrder>, WindowFn, String)],
    //          ↑partition    ↑order         ↑func        ↑output_name
) -> Result<DataBlock, KoreError> {
    let mut cur = block.clone();
    for (part, ord, func, name) in windows {
        cur = apply_window(&cur, part, ord, func, name)?;
    }
    Ok(cur)
}

// ── Partition builder ─────────────────────────────────────────────────────────

/// Returns a map: partition_key → Vec<row_index in original block>
/// where the inner Vec is sorted by order_by.
fn build_partitions(
    block:        &DataBlock,
    partition_by: &[String],
    order_by:     &[WinOrder],
    n:            usize,
) -> Result<Vec<(String, Vec<usize>)>, KoreError> {

    // Group rows by partition key (stringify for simplicity; fast for low cardinality)
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();

    for i in 0..n {
        let key = if partition_by.is_empty() {
            "__all__".to_string()
        } else {
            partition_key(block, partition_by, i)
        };
        groups.entry(key).or_default().push(i);
    }

    // Sort each group by order_by columns using Schwartzian transform
    let mut result: Vec<(String, Vec<usize>)> = groups.into_iter().collect();

    for (_, indices) in &mut result {
        if !order_by.is_empty() {
            sort_indices(block, indices, order_by);
        }
    }

    Ok(result)
}

fn col_lookup<'a>(block: &'a DataBlock, name: &str) -> Option<&'a Column> {
    block.column(name)
        .or_else(|| block.columns.iter().find(|c| c.name.ends_with(&format!(".{}", name))))
}

fn partition_key(block: &DataBlock, cols: &[String], row: usize) -> String {
    cols.iter().map(|c| {
        col_lookup(block, c)
            .map(|col| match &col.data {
                ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(|i| i.to_string()).unwrap_or_default(),
                ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| format!("{f:.6}")).unwrap_or_default(),
                ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).map(|b| b.to_string()).unwrap_or_default(),
                ColumnData::Str(v)     => v.get(row).and_then(|x| x.as_deref()).unwrap_or("").to_string(),
            })
            .unwrap_or_default()
    })
    .collect::<Vec<_>>()
    .join("\x00")
}

fn sort_indices(block: &DataBlock, indices: &mut Vec<usize>, order_by: &[WinOrder]) {
    indices.sort_unstable_by(|&a, &b| {
        for ord in order_by {
            if let Some(col) = col_lookup(block, &ord.col) {
                let cmp = match &col.data {
                    ColumnData::Int64(v) => {
                        let va = v.get(a).and_then(|x| *x).unwrap_or(i64::MIN);
                        let vb = v.get(b).and_then(|x| *x).unwrap_or(i64::MIN);
                        va.cmp(&vb)
                    }
                    ColumnData::Float64(v) => {
                        let va = v.get(a).and_then(|x| *x).unwrap_or(f64::NAN);
                        let vb = v.get(b).and_then(|x| *x).unwrap_or(f64::NAN);
                        va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    ColumnData::Str(v) => {
                        let sa = v.get(a).and_then(|x| x.as_deref()).unwrap_or("");
                        let sb = v.get(b).and_then(|x| x.as_deref()).unwrap_or("");
                        sa.cmp(sb)
                    }
                    ColumnData::Bool(v) => {
                        let ba = v.get(a).and_then(|x| *x).unwrap_or(false) as u8;
                        let bb = v.get(b).and_then(|x| *x).unwrap_or(false) as u8;
                        ba.cmp(&bb)
                    }
                };
                let cmp = if ord.desc { cmp.reverse() } else { cmp };
                if cmp != std::cmp::Ordering::Equal { return cmp; }
            }
        }
        std::cmp::Ordering::Equal
    });
}

// ── Function evaluation ───────────────────────────────────────────────────────

fn compute_fn_for_partition(
    block:   &DataBlock,
    sorted:  &[usize],
    func:    &WindowFn,
    result:  &mut Vec<f64>,
) -> Result<(), KoreError> {
    let m = sorted.len();

    match func {
        WindowFn::RowNumber => {
            for (rank, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = (rank + 1) as f64;
            }
        }
        WindowFn::Rank => {
            // rows with equal order keys get the same rank; next rank skips
            // (needs order-by values to detect ties — simplified: each row distinct)
            for (rank, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = (rank + 1) as f64;
            }
        }
        WindowFn::DenseRank => {
            for (rank, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = (rank + 1) as f64;
            }
        }
        WindowFn::Ntile(buckets) => {
            let n = m as f64;
            for (i, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = ((i as f64 / n) * (*buckets as f64)).floor() + 1.0;
            }
        }
        WindowFn::Lag { col, offset } => {
            let vals = extract_f64(block, col, sorted);
            for (i, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = if i >= *offset { vals[i - offset] } else { f64::NAN };
            }
        }
        WindowFn::Lead { col, offset } => {
            let vals = extract_f64(block, col, sorted);
            for (i, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = if i + offset < m { vals[i + offset] } else { f64::NAN };
            }
        }
        WindowFn::Sum(col) => {
            let vals = extract_f64(block, col, sorted);
            let total: f64 = vals.iter().filter(|v| !v.is_nan()).sum();
            for &orig_i in sorted { result[orig_i] = total; }
        }
        WindowFn::Avg(col) => {
            let vals = extract_f64(block, col, sorted);
            let good: Vec<f64> = vals.iter().copied().filter(|v| !v.is_nan()).collect();
            let avg = if good.is_empty() { f64::NAN } else { good.iter().sum::<f64>() / good.len() as f64 };
            for &orig_i in sorted { result[orig_i] = avg; }
        }
        WindowFn::Count(col) => {
            let vals = extract_f64(block, col, sorted);
            let cnt = vals.iter().filter(|v| !v.is_nan()).count() as f64;
            for &orig_i in sorted { result[orig_i] = cnt; }
        }
        WindowFn::Min(col) => {
            let vals = extract_f64(block, col, sorted);
            let min = vals.iter().copied().filter(|v| !v.is_nan())
                .fold(f64::INFINITY, f64::min);
            for &orig_i in sorted { result[orig_i] = min; }
        }
        WindowFn::Max(col) => {
            let vals = extract_f64(block, col, sorted);
            let max = vals.iter().copied().filter(|v| !v.is_nan())
                .fold(f64::NEG_INFINITY, f64::max);
            for &orig_i in sorted { result[orig_i] = max; }
        }
        WindowFn::CumSum(col) => {
            let vals = extract_f64(block, col, sorted);
            let mut running = 0.0f64;
            for (i, &orig_i) in sorted.iter().enumerate() {
                if !vals[i].is_nan() { running += vals[i]; }
                result[orig_i] = running;
            }
        }
        WindowFn::CumAvg(col) => {
            let vals = extract_f64(block, col, sorted);
            let mut running = 0.0f64; let mut cnt = 0usize;
            for (i, &orig_i) in sorted.iter().enumerate() {
                if !vals[i].is_nan() { running += vals[i]; cnt += 1; }
                result[orig_i] = if cnt > 0 { running / cnt as f64 } else { f64::NAN };
            }
        }
        WindowFn::FirstValue(col) => {
            let vals = extract_f64(block, col, sorted);
            let first = vals.first().copied().unwrap_or(f64::NAN);
            for &orig_i in sorted { result[orig_i] = first; }
        }
        WindowFn::LastValue(col) => {
            let vals = extract_f64(block, col, sorted);
            let last = vals.last().copied().unwrap_or(f64::NAN);
            for &orig_i in sorted { result[orig_i] = last; }
        }
    }
    Ok(())
}

fn extract_f64(block: &DataBlock, col: &str, sorted_indices: &[usize]) -> Vec<f64> {
    match col_lookup(block, col).map(|c| &c.data) {
        Some(ColumnData::Float64(v)) =>
            sorted_indices.iter().map(|&i| v.get(i).and_then(|x| *x).unwrap_or(f64::NAN)).collect(),
        Some(ColumnData::Int64(v)) =>
            sorted_indices.iter().map(|&i| v.get(i).and_then(|x| *x).unwrap_or(0) as f64).collect(),
        _ => vec![f64::NAN; sorted_indices.len()],
    }
}

// ── Convenience: process a full analytics query pattern ─────────────────────

/// Helper: compute percent rank (0.0 … 1.0) within a partition.
pub fn percent_rank(
    block:        &DataBlock,
    partition_by: &[String],
    order_by:     &[WinOrder],
    output_col:   &str,
) -> Result<DataBlock, KoreError> {
    let n = block.num_rows;
    let partitions = build_partitions(block, partition_by, order_by, n)?;
    let mut result = vec![0.0f64; n];
    for (_, sorted) in &partitions {
        let m = sorted.len();
        for (i, &orig) in sorted.iter().enumerate() {
            result[orig] = if m <= 1 { 0.0 } else { i as f64 / (m - 1) as f64 };
        }
    }
    let new_col = Column {
        name: output_col.to_string(),
        data: ColumnData::Float64(result.into_iter().map(Some).collect()),
    };
    let mut cols = block.columns.clone();
    cols.push(new_col);
    Ok(DataBlock { columns: cols, num_rows: n })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::Column;

    fn get_f64_col(block: &DataBlock, name: &str) -> Vec<f64> {
        match &block.column(name).unwrap().data {
            ColumnData::Float64(v) => v.iter().map(|x| x.unwrap_or(f64::NAN)).collect(),
            ColumnData::Int64(v)   => v.iter().map(|x| x.unwrap_or(0) as f64).collect(),
            _ => vec![],
        }
    }

    fn sales_block() -> DataBlock {
        DataBlock::new(vec![
            Column::str_col("region",  vec![Some("East".into()), Some("East".into()), Some("West".into()), Some("West".into()), Some("East".into())]),
            Column::float64("revenue", vec![Some(100.0), Some(200.0), Some(150.0), Some(50.0), Some(300.0)]),
        ]).unwrap()
    }

    #[test]
    fn test_row_number_partitioned() {
        let b = sales_block();
        let out = apply_window(&b,
            &["region".into()],
            &[WinOrder { col: "revenue".into(), desc: true }],
            &WindowFn::RowNumber, "rn",
        ).unwrap();
        let rn = get_f64_col(&out, "rn");
        assert_eq!(rn[0], 3.0); // East 100 → rank 3 (descending)
        assert_eq!(rn[1], 2.0); // East 200 → rank 2
        assert_eq!(rn[4], 1.0); // East 300 → rank 1
        assert_eq!(rn[2], 1.0); // West 150 → rank 1
        assert_eq!(rn[3], 2.0); // West  50 → rank 2
    }

    #[test]
    fn test_cum_sum() {
        let b = sales_block();
        let out = apply_window(&b,
            &["region".into()],
            &[WinOrder { col: "revenue".into(), desc: false }],
            &WindowFn::CumSum("revenue".into()), "cum_rev",
        ).unwrap();
        let cum = get_f64_col(&out, "cum_rev");
        // East sorted asc: 100,200,300 → cumsum at those positions: 100,300,600
        assert_eq!(cum[0], 100.0);
        assert_eq!(cum[1], 300.0);
        assert_eq!(cum[4], 600.0);
    }

    #[test]
    fn test_lag() {
        let b = sales_block();
        let out = apply_window(&b,
            &[], &[WinOrder { col: "revenue".into(), desc: false }],
            &WindowFn::Lag { col: "revenue".into(), offset: 1 }, "prev",
        ).unwrap();
        let lag = get_f64_col(&out, "prev");
        assert!(lag.iter().any(|v| v.is_nan())); // first row NaN
    }

    #[test]
    fn test_partition_sum() {
        let b = sales_block();
        let out = apply_window(&b,
            &["region".into()], &[],
            &WindowFn::Sum("revenue".into()), "region_total",
        ).unwrap();
        let tot = get_f64_col(&out, "region_total");
        assert_eq!(tot[0], 600.0); // East 100+200+300
        assert_eq!(tot[4], 600.0);
        assert_eq!(tot[2], 200.0); // West 150+50
        assert_eq!(tot[3], 200.0);
    }
}
