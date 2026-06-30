//! KORE Layer 73 — Expanded ML: Random Forest, Logistic Regression, K-Means
//!
//! All models work on `&[Vec<f64>]` feature matrices.
//! DataBlock integration helpers convert columnar data to/from that format.

use std::collections::HashMap;
use kore_core::{Column, ColumnData, DataBlock, Value};
use rayon::prelude::*;

// ─── Utilities ────────────────────────────────────────────────────────────────

fn sigmoid(x: f64) -> f64 { 1.0 / (1.0 + (-x).exp()) }

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn euclidean_sq(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum()
}

// ─── Decision Tree ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Node {
    Leaf(f64),
    Split {
        feature:   usize,
        threshold: f64,
        left:      Box<Node>,
        right:     Box<Node>,
    },
}

/// CART decision tree (classification / regression leaf = mean).
#[derive(Debug, Clone)]
pub struct DecisionTree {
    max_depth:   usize,
    min_samples: usize,
    root:        Option<Node>,
}

impl DecisionTree {
    pub fn new(max_depth: usize, min_samples: usize) -> Self {
        Self { max_depth, min_samples, root: None }
    }

    pub fn fit(&mut self, x: &[Vec<f64>], y: &[f64]) {
        self.root = Some(Self::build(x, y, self.max_depth, self.min_samples));
    }

    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<f64> {
        let root = self.root.as_ref().expect("call fit() first");
        x.iter().map(|row| Self::predict_row(root, row)).collect()
    }

    fn predict_row(node: &Node, row: &[f64]) -> f64 {
        match node {
            Node::Leaf(v) => *v,
            Node::Split { feature, threshold, left, right } => {
                if row[*feature] <= *threshold {
                    Self::predict_row(left, row)
                } else {
                    Self::predict_row(right, row)
                }
            }
        }
    }

    fn build(x: &[Vec<f64>], y: &[f64], depth: usize, min_samples: usize) -> Node {
        if depth == 0 || y.len() <= min_samples {
            return Node::Leaf(mean(y));
        }
        let (feat, thresh, _gain) = Self::best_split(x, y);
        if feat == usize::MAX {
            return Node::Leaf(mean(y));
        }
        let (lx, ly, rx, ry) = split_data(x, y, feat, thresh);
        if lx.is_empty() || rx.is_empty() {
            return Node::Leaf(mean(y));
        }
        Node::Split {
            feature:   feat,
            threshold: thresh,
            left:  Box::new(Self::build(&lx, &ly, depth - 1, min_samples)),
            right: Box::new(Self::build(&rx, &ry, depth - 1, min_samples)),
        }
    }

    /// Returns (feature, threshold, gain).  `feature == usize::MAX` means no improvement.
    pub fn best_split(x: &[Vec<f64>], y: &[f64]) -> (usize, f64, f64) {
        let n_features = x.first().map(|r| r.len()).unwrap_or(0);
        let base_gini  = Self::gini_impurity(y);
        let mut best_feat  = usize::MAX;
        let mut best_thr   = 0.0f64;
        let mut best_gain  = f64::NEG_INFINITY;

        for fi in 0..n_features {
            let mut vals: Vec<f64> = x.iter().map(|r| r[fi]).collect();
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            vals.dedup();

            for &thr in vals.iter().take(vals.len().saturating_sub(1)) {
                let (lx, ly, rx, ry) = split_data(x, y, fi, thr);
                if lx.is_empty() || rx.is_empty() { continue; }
                let n = y.len() as f64;
                let gain = base_gini
                    - (ly.len() as f64 / n) * Self::gini_impurity(&ly)
                    - (ry.len() as f64 / n) * Self::gini_impurity(&ry);
                if gain > best_gain {
                    best_gain = gain;
                    best_feat = fi;
                    best_thr  = thr;
                }
            }
        }
        (best_feat, best_thr, best_gain)
    }

    pub fn gini_impurity(y: &[f64]) -> f64 {
        if y.is_empty() { return 0.0; }
        let mut counts: HashMap<i64, usize> = HashMap::new();
        for &v in y { *counts.entry(v as i64).or_insert(0) += 1; }
        let n = y.len() as f64;
        1.0 - counts.values().map(|&c| (c as f64 / n).powi(2)).sum::<f64>()
    }
}

fn mean(y: &[f64]) -> f64 {
    if y.is_empty() { return 0.0; }
    y.iter().sum::<f64>() / y.len() as f64
}

fn split_data(
    x: &[Vec<f64>], y: &[f64], feat: usize, thresh: f64,
) -> (Vec<Vec<f64>>, Vec<f64>, Vec<Vec<f64>>, Vec<f64>) {
    let mut lx = Vec::new(); let mut ly = Vec::new();
    let mut rx = Vec::new(); let mut ry = Vec::new();
    for (row, &label) in x.iter().zip(y.iter()) {
        if row[feat] <= thresh { lx.push(row.clone()); ly.push(label); }
        else                   { rx.push(row.clone()); ry.push(label); }
    }
    (lx, ly, rx, ry)
}

// ─── Random Forest ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RandomForest {
    pub n_trees:       usize,
    pub max_depth:     usize,
    pub trees:         Vec<DecisionTree>,
    pub feature_subset: usize,
}

impl RandomForest {
    pub fn new(n_trees: usize, max_depth: usize) -> Self {
        Self { n_trees, max_depth, trees: Vec::new(), feature_subset: 0 }
    }

    pub fn fit(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n       = x.len();
        let n_feats = x.first().map(|r| r.len()).unwrap_or(1);
        self.feature_subset = ((n_feats as f64).sqrt() as usize).max(1);

        // Generate bootstrap indices per tree (deterministic via simple LCG)
        let mut seeds: Vec<u64> = (0..self.n_trees as u64)
            .map(|i| 6364136223846793005u64.wrapping_mul(i).wrapping_add(1442695040888963407))
            .collect();

        self.trees = (0..self.n_trees)
            .into_par_iter()
            .map(|ti| {
                let mut rng = seeds[ti];
                // Draw bootstrap indices (x and y must share the same indices)
                let boot_idx: Vec<usize> = (0..n).map(|_| {
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (rng >> 33) as usize % n
                }).collect();
                let boot_x: Vec<Vec<f64>> = boot_idx.iter().map(|&i| x[i].clone()).collect();
                let boot_y: Vec<f64>       = boot_idx.iter().map(|&i| y[i]).collect();
                let mut tree = DecisionTree::new(self.max_depth, 2);
                tree.fit(&boot_x, &boot_y);
                tree
            })
            .collect();
    }

    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<f64> {
        let n_rows = x.len();
        let all_preds: Vec<Vec<f64>> = self.trees.par_iter()
            .map(|t| t.predict(x))
            .collect();

        // Majority vote
        (0..n_rows).map(|r| {
            let votes: Vec<f64> = all_preds.iter().map(|p| p[r]).collect();
            let mut cnt: HashMap<i64, usize> = HashMap::new();
            for &v in &votes { *cnt.entry(v as i64).or_insert(0) += 1; }
            let best = cnt.into_iter().max_by_key(|&(_, c)| c).map(|(k, _)| k).unwrap_or(0);
            best as f64
        }).collect()
    }

    pub fn feature_importance(&self) -> Vec<f64> {
        let n_feats = self.trees.first()
            .and_then(|t| t.root.as_ref())
            .map(|_| {
                // Walk tree to find max feature index used
                fn max_feat(n: &Node) -> usize {
                    match n {
                        Node::Leaf(_) => 0,
                        Node::Split { feature, left, right, .. } =>
                            *feature.max(&max_feat(left)).max(&max_feat(right)),
                    }
                }
                self.trees.iter().filter_map(|t| t.root.as_ref())
                    .map(max_feat).max().unwrap_or(0) + 1
            })
            .unwrap_or(0);

        let mut importance = vec![0.0f64; n_feats];
        let mut counts     = vec![0usize;  n_feats];

        fn accumulate(n: &Node, imp: &mut Vec<f64>, cnt: &mut Vec<usize>) {
            match n {
                Node::Leaf(_) => {}
                Node::Split { feature, left, right, .. } => {
                    if *feature < imp.len() {
                        imp[*feature] += 1.0;
                        cnt[*feature] += 1;
                    }
                    accumulate(left,  imp, cnt);
                    accumulate(right, imp, cnt);
                }
            }
        }

        for tree in &self.trees {
            if let Some(root) = &tree.root {
                accumulate(root, &mut importance, &mut counts);
            }
        }

        let total: f64 = importance.iter().sum();
        if total > 0.0 { importance.iter_mut().for_each(|v| *v /= total); }
        importance
    }
}

// ─── Logistic Regression ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LogisticRegression {
    pub weights: Vec<f64>,
    pub bias:    f64,
    pub lr:      f64,
    pub epochs:  usize,
}

impl LogisticRegression {
    pub fn new(lr: f64, epochs: usize) -> Self {
        Self { weights: Vec::new(), bias: 0.0, lr, epochs }
    }

    pub fn fit(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n_feats = x.first().map(|r| r.len()).unwrap_or(0);
        self.weights = vec![0.0; n_feats];
        self.bias    = 0.0;
        let n = x.len() as f64;

        for _ in 0..self.epochs {
            let mut dw = vec![0.0f64; n_feats];
            let mut db = 0.0f64;

            for (row, &label) in x.iter().zip(y.iter()) {
                let pred  = sigmoid(dot(&self.weights, row) + self.bias);
                let delta = pred - label;
                for (j, &xj) in row.iter().enumerate() {
                    dw[j] += delta * xj;
                }
                db += delta;
            }

            for (j, w) in self.weights.iter_mut().enumerate() {
                *w -= self.lr * dw[j] / n;
            }
            self.bias -= self.lr * db / n;
        }
    }

    pub fn predict_proba(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter()
            .map(|row| sigmoid(dot(&self.weights, row) + self.bias))
            .collect()
    }

    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<f64> {
        self.predict_proba(x)
            .into_iter()
            .map(|p| if p >= 0.5 { 1.0 } else { 0.0 })
            .collect()
    }

    pub fn accuracy(y_true: &[f64], y_pred: &[f64]) -> f64 {
        let correct = y_true.iter().zip(y_pred.iter())
            .filter(|(a, b)| (*a - *b).abs() < 0.5)
            .count();
        correct as f64 / y_true.len() as f64
    }
}

// ─── K-Means ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct KMeans {
    pub k:         usize,
    pub max_iter:  usize,
    pub centroids: Vec<Vec<f64>>,
}

impl KMeans {
    pub fn new(k: usize, max_iter: usize) -> Self {
        Self { k, max_iter, centroids: Vec::new() }
    }

    /// Lloyd's algorithm with k-means++ initialisation (deterministic).
    pub fn fit(&mut self, x: &[Vec<f64>]) {
        let n      = x.len();
        let n_feat = x.first().map(|r| r.len()).unwrap_or(0);
        if n == 0 || self.k == 0 { return; }

        // Simple k-means++ style init: pick k spread-out points
        self.centroids = vec![x[0].clone()];
        let mut rng = 12345u64;
        while self.centroids.len() < self.k {
            let dists: Vec<f64> = x.iter().map(|row| {
                self.centroids.iter()
                    .map(|c| euclidean_sq(row, c))
                    .fold(f64::INFINITY, f64::min)
            }).collect();
            let total: f64 = dists.iter().sum();
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mut pick = (rng as f64 / u64::MAX as f64) * total;
            let mut chosen = n - 1;
            for (i, &d) in dists.iter().enumerate() {
                pick -= d;
                if pick <= 0.0 { chosen = i; break; }
            }
            self.centroids.push(x[chosen].clone());
        }

        for _ in 0..self.max_iter {
            let assignments = self.predict(x);
            let mut new_centroids: Vec<Vec<f64>> = vec![vec![0.0; n_feat]; self.k];
            let mut counts = vec![0usize; self.k];
            for (row, &cl) in x.iter().zip(assignments.iter()) {
                for (j, &xj) in row.iter().enumerate() {
                    new_centroids[cl][j] += xj;
                }
                counts[cl] += 1;
            }
            let mut converged = true;
            for ci in 0..self.k {
                if counts[ci] == 0 { continue; }
                let new: Vec<f64> = new_centroids[ci].iter()
                    .map(|&s| s / counts[ci] as f64)
                    .collect();
                if euclidean_sq(&new, &self.centroids[ci]) > 1e-10 { converged = false; }
                self.centroids[ci] = new;
            }
            if converged { break; }
        }
    }

    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<usize> {
        x.iter().map(|row| {
            self.centroids.iter().enumerate()
                .min_by(|(_, a), (_, b)| {
                    euclidean_sq(row, a).partial_cmp(&euclidean_sq(row, b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        }).collect()
    }

    pub fn inertia(&self, x: &[Vec<f64>]) -> f64 {
        let assignments = self.predict(x);
        x.iter().zip(assignments.iter())
            .map(|(row, &cl)| euclidean_sq(row, &self.centroids[cl]))
            .sum()
    }
}

// ─── DataBlock integration ────────────────────────────────────────────────────

fn extract_features(block: &DataBlock, cols: &[&str]) -> Result<Vec<Vec<f64>>, String> {
    let n = block.num_rows;
    let mut result: Vec<Vec<f64>> = vec![Vec::with_capacity(cols.len()); n];
    for &col_name in cols {
        let col = block.column(col_name)
            .ok_or_else(|| format!("feature column '{}' not found", col_name))?;
        for r in 0..n {
            let v = match col.data.get_value(r) {
                Value::Float(f) => f,
                Value::Int(i)   => i as f64,
                Value::Bool(b)  => b as i32 as f64,
                _               => 0.0,
            };
            result[r].push(v);
        }
    }
    Ok(result)
}

fn extract_target(block: &DataBlock, col_name: &str) -> Result<Vec<f64>, String> {
    let col = block.column(col_name)
        .ok_or_else(|| format!("target column '{}' not found", col_name))?;
    Ok((0..block.num_rows).map(|r| match col.data.get_value(r) {
        Value::Float(f) => f,
        Value::Int(i)   => i as f64,
        Value::Bool(b)  => b as i32 as f64,
        _               => 0.0,
    }).collect())
}

/// Fit a `RandomForest` directly from a `DataBlock`.
pub fn fit_from_block(
    model:        &mut RandomForest,
    block:        &DataBlock,
    feature_cols: &[&str],
    target_col:   &str,
) -> Result<(), String> {
    let x = extract_features(block, feature_cols)?;
    let y = extract_target(block, target_col)?;
    model.fit(&x, &y);
    Ok(())
}

/// Run `RandomForest::predict` and append predictions as a new column.
pub fn predict_to_block(
    model:        &RandomForest,
    block:        &DataBlock,
    feature_cols: &[&str],
    output_col:   &str,
) -> Result<DataBlock, String> {
    let x    = extract_features(block, feature_cols)?;
    let preds = model.predict(&x);
    let mut columns = block.columns.clone();
    columns.push(Column::float64(output_col,
        preds.into_iter().map(Some).collect(),
    ));
    DataBlock::new(columns).map_err(|e| e.to_string())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// XOR data: (0,0)→0, (0,1)→1, (1,0)→1, (1,1)→0 — repeated many times.
    fn xor_data(n: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
        let patterns = [(0.0_f64, 0.0_f64, 0.0_f64), (0.0, 1.0, 1.0), (1.0, 0.0, 1.0), (1.0, 1.0, 0.0)];
        let mut x = Vec::with_capacity(n);
        let mut y = Vec::with_capacity(n);
        // Add a little noise so trees don't over-fit perfectly (makes test
        // more realistic) by repeating the same 4 patterns.
        for i in 0..n {
            let (a, b, label) = patterns[i % 4];
            x.push(vec![a, b]);
            y.push(label);
        }
        (x, y)
    }

    #[test]
    fn decision_tree_xor() {
        let (x, y) = xor_data(200);
        let mut tree = DecisionTree::new(4, 2);
        tree.fit(&x, &y);
        let preds = tree.predict(&x);
        let acc = LogisticRegression::accuracy(&y, &preds);
        assert!(acc > 0.80, "DT accuracy {acc:.2} < 0.80");
    }

    #[test]
    fn random_forest_xor() {
        let (x, y) = xor_data(400);
        let mut rf = RandomForest::new(10, 5);
        rf.fit(&x, &y);
        let preds = rf.predict(&x);
        let acc = LogisticRegression::accuracy(&y, &preds);
        assert!(acc > 0.80, "RF accuracy {acc:.2} < 0.80");
    }

    #[test]
    fn feature_importance_sums_to_one() {
        let (x, y) = xor_data(200);
        let mut rf = RandomForest::new(5, 4);
        rf.fit(&x, &y);
        let imp = rf.feature_importance();
        let total: f64 = imp.iter().sum();
        assert!((total - 1.0).abs() < 1e-9, "importance sum {total}");
    }

    #[test]
    fn logistic_regression_linearly_separable() {
        // y = 1 if x[0] > 0.5
        let n = 200;
        let x: Vec<Vec<f64>> = (0..n).map(|i| vec![i as f64 / n as f64]).collect();
        let y: Vec<f64>       = (0..n).map(|i| if i > n / 2 { 1.0 } else { 0.0 }).collect();
        let mut lr = LogisticRegression::new(0.1, 500);
        lr.fit(&x, &y);
        let preds = lr.predict(&x);
        let acc = LogisticRegression::accuracy(&y, &preds);
        assert!(acc > 0.85, "LR accuracy {acc:.2} < 0.85");
    }

    #[test]
    fn kmeans_three_clusters() {
        // Three well-separated clusters
        let mut x: Vec<Vec<f64>> = Vec::new();
        for i in 0..50 { x.push(vec![i as f64 * 0.01, 0.0]); }      // cluster A: near x=0
        for i in 0..50 { x.push(vec![10.0 + i as f64 * 0.01, 0.0]); } // cluster B: near x=10
        for i in 0..50 { x.push(vec![20.0 + i as f64 * 0.01, 0.0]); } // cluster C: near x=20

        let mut km = KMeans::new(3, 100);
        km.fit(&x);
        let assignments = km.predict(&x);

        // Each cluster should have exactly one label used
        let a: std::collections::HashSet<_> = assignments[0..50].iter().collect();
        let b: std::collections::HashSet<_> = assignments[50..100].iter().collect();
        let c: std::collections::HashSet<_> = assignments[100..150].iter().collect();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(c.len(), 1);
        // All three labels should be distinct
        assert_ne!(a, b); assert_ne!(b, c); assert_ne!(a, c);
    }

    #[test]
    fn datablock_integration() {
        use kore_core::Column;
        let (x_data, y_data) = xor_data(200);
        let x0: Vec<Option<f64>> = x_data.iter().map(|r| Some(r[0])).collect();
        let x1: Vec<Option<f64>> = x_data.iter().map(|r| Some(r[1])).collect();
        let yy: Vec<Option<f64>> = y_data.iter().map(|&v| Some(v)).collect();
        let block = DataBlock::new(vec![
            Column::float64("x0", x0),
            Column::float64("x1", x1),
            Column::float64("label", yy),
        ]).unwrap();

        let mut rf = RandomForest::new(5, 4);
        fit_from_block(&mut rf, &block, &["x0", "x1"], "label").unwrap();
        let out = predict_to_block(&rf, &block, &["x0", "x1"], "pred").unwrap();
        assert_eq!(out.num_rows, 200);
        assert!(out.column("pred").is_some());
    }
}
