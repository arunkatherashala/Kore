//! Built-in Transformers:
//!   • StandardScaler  — zero-mean / unit-variance normalisation
//!   • MinMaxScaler    — scale each column to [0, 1]
//!   • LabelEncoder    — map string categories → integer indices

use std::collections::HashMap;
use kore_core::{Column, ColumnData, DataBlock, KoreError, Transformer};

// ─── StandardScaler ───────────────────────────────────────────────────────────

/// Standardise numeric columns: x' = (x − μ) / σ
#[derive(Debug, Clone, Default)]
pub struct StandardScaler {
    cols:  Vec<String>,   // columns to scale (empty = all Float64/Int64)
    stats: HashMap<String, (f64, f64)>,  // col → (mean, std)
}

impl StandardScaler {
    pub fn new(cols: Vec<&str>) -> Self {
        Self { cols: cols.into_iter().map(|s| s.into()).collect(), stats: HashMap::new() }
    }

    /// Scale all numeric columns when no explicit list given.
    pub fn all_numeric() -> Self {
        Self { cols: vec![], stats: HashMap::new() }
    }
}

impl Transformer for StandardScaler {
    fn name(&self) -> &str { "StandardScaler" }

    fn fit(&mut self, data: &DataBlock) -> Result<(), KoreError> {
        let target_cols = effective_numeric_cols(data, &self.cols);
        self.stats.clear();
        for col_name in &target_cols {
            let col = data.column(col_name).ok_or_else(|| KoreError::ColumnNotFound(col_name.clone()))?;
            let vals: Vec<f64> = (0..data.num_rows)
                .filter_map(|i| col.data.get_value(i).as_f64())
                .collect();
            let n = vals.len() as f64;
            if n == 0.0 { continue; }
            let mean = vals.iter().sum::<f64>() / n;
            let std  = (vals.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / n).sqrt().max(1e-10);
            self.stats.insert(col_name.clone(), (mean, std));
        }
        Ok(())
    }

    fn transform(&self, data: &DataBlock) -> Result<DataBlock, KoreError> {
        let mut columns: Vec<Column> = Vec::with_capacity(data.columns.len());
        for col in &data.columns {
            if let Some(&(mean, std)) = self.stats.get(&col.name) {
                let scaled: Vec<Option<f64>> = (0..data.num_rows)
                    .map(|i| col.data.get_value(i).as_f64().map(|v| (v - mean) / std))
                    .collect();
                columns.push(Column::float64(&col.name, scaled));
            } else {
                columns.push(col.clone());
            }
        }
        DataBlock::new(columns)
    }
}

// ─── MinMaxScaler ─────────────────────────────────────────────────────────────

/// Scale numeric columns to [0, 1]: x' = (x − min) / (max − min)
#[derive(Debug, Clone, Default)]
pub struct MinMaxScaler {
    cols:  Vec<String>,
    stats: HashMap<String, (f64, f64)>,  // col → (min, max)
}

impl MinMaxScaler {
    pub fn new(cols: Vec<&str>) -> Self {
        Self { cols: cols.into_iter().map(|s| s.into()).collect(), stats: HashMap::new() }
    }

    pub fn all_numeric() -> Self {
        Self { cols: vec![], stats: HashMap::new() }
    }
}

impl Transformer for MinMaxScaler {
    fn name(&self) -> &str { "MinMaxScaler" }

    fn fit(&mut self, data: &DataBlock) -> Result<(), KoreError> {
        let target_cols = effective_numeric_cols(data, &self.cols);
        self.stats.clear();
        for col_name in &target_cols {
            let col = data.column(col_name).ok_or_else(|| KoreError::ColumnNotFound(col_name.clone()))?;
            let vals: Vec<f64> = (0..data.num_rows)
                .filter_map(|i| col.data.get_value(i).as_f64())
                .collect();
            if vals.is_empty() { continue; }
            let min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            self.stats.insert(col_name.clone(), (min, max));
        }
        Ok(())
    }

    fn transform(&self, data: &DataBlock) -> Result<DataBlock, KoreError> {
        let mut columns: Vec<Column> = Vec::with_capacity(data.columns.len());
        for col in &data.columns {
            if let Some(&(min, max)) = self.stats.get(&col.name) {
                let range = (max - min).max(1e-10);
                let scaled: Vec<Option<f64>> = (0..data.num_rows)
                    .map(|i| col.data.get_value(i).as_f64().map(|v| (v - min) / range))
                    .collect();
                columns.push(Column::float64(&col.name, scaled));
            } else {
                columns.push(col.clone());
            }
        }
        DataBlock::new(columns)
    }
}

// ─── LabelEncoder ─────────────────────────────────────────────────────────────

/// Map string categories → consecutive integer indices (0, 1, 2, …).
#[derive(Debug, Clone, Default)]
pub struct LabelEncoder {
    cols:    Vec<String>,
    mapping: HashMap<String, HashMap<String, i64>>,  // col → (label → int)
}

impl LabelEncoder {
    pub fn new(cols: Vec<&str>) -> Self {
        Self { cols: cols.into_iter().map(|s| s.into()).collect(), mapping: HashMap::new() }
    }
}

impl Transformer for LabelEncoder {
    fn name(&self) -> &str { "LabelEncoder" }

    fn fit(&mut self, data: &DataBlock) -> Result<(), KoreError> {
        self.mapping.clear();
        for col_name in &self.cols {
            let col = data.column(col_name).ok_or_else(|| KoreError::ColumnNotFound(col_name.clone()))?;
            if !matches!(col.data, ColumnData::Str(_)) { continue; }
            let mut map: HashMap<String, i64> = HashMap::new();
            let mut idx = 0i64;
            for i in 0..data.num_rows {
                if let kore_core::Value::Str(s) = col.data.get_value(i) {
                    if !map.contains_key(&s) {
                        map.insert(s, idx);
                        idx += 1;
                    }
                }
            }
            self.mapping.insert(col_name.clone(), map);
        }
        Ok(())
    }

    fn transform(&self, data: &DataBlock) -> Result<DataBlock, KoreError> {
        let mut columns: Vec<Column> = Vec::with_capacity(data.columns.len());
        for col in &data.columns {
            if let Some(map) = self.mapping.get(&col.name) {
                let encoded: Vec<Option<i64>> = (0..data.num_rows)
                    .map(|i| match col.data.get_value(i) {
                        kore_core::Value::Str(s) => map.get(&s).copied(),
                        _                        => None,
                    })
                    .collect();
                columns.push(Column::int64(&col.name, encoded));
            } else {
                columns.push(col.clone());
            }
        }
        DataBlock::new(columns)
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn effective_numeric_cols(data: &DataBlock, explicit: &[String]) -> Vec<String> {
    if explicit.is_empty() {
        data.columns.iter()
            .filter(|c| matches!(c.data, ColumnData::Int64(_) | ColumnData::Float64(_)))
            .map(|c| c.name.clone())
            .collect()
    } else {
        explicit.to_vec()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock, Transformer};

    fn numeric_block() -> DataBlock {
        DataBlock::new(vec![
            Column::float64("a", vec![Some(1.0), Some(3.0), Some(5.0)]),
            Column::float64("b", vec![Some(10.0), Some(20.0), Some(30.0)]),
        ]).unwrap()
    }

    #[test]
    fn standard_scaler() {
        let data = numeric_block();
        let mut s = StandardScaler::all_numeric();
        let out = s.fit_transform(&data).unwrap();
        let col = out.column("a").unwrap();
        let v0 = col.data.get_value(1).as_f64().unwrap();
        assert!(v0.abs() < 0.1, "middle value should be ~0 after standardising");
    }

    #[test]
    fn minmax_scaler() {
        let data = numeric_block();
        let mut s = MinMaxScaler::all_numeric();
        let out = s.fit_transform(&data).unwrap();
        let a = out.column("a").unwrap();
        let first = a.data.get_value(0).as_f64().unwrap();
        let last  = a.data.get_value(2).as_f64().unwrap();
        assert!((first - 0.0).abs() < 1e-9);
        assert!((last  - 1.0).abs() < 1e-9);
    }

    #[test]
    fn label_encoder() {
        let data = DataBlock::new(vec![
            Column::str_col("cat", vec![
                Some("dog".into()), Some("cat".into()), Some("dog".into()), Some("bird".into())
            ]),
        ]).unwrap();
        let mut le = LabelEncoder::new(vec!["cat"]);
        let out = le.fit_transform(&data).unwrap();
        let col = out.column("cat").unwrap();
        let a = col.data.get_value(0);
        let b = col.data.get_value(1);
        // dog and cat must get different ids
        assert_ne!(a.as_f64(), b.as_f64());
        // two dogs get same id
        assert_eq!(col.data.get_value(0).as_f64(), col.data.get_value(2).as_f64());
    }
}
