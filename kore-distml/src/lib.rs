//! KORE Layer 52 — Distributed ML
//!
//! Distributed training across kore-worker nodes — mirrors Spark MLlib's
//! parameter-server / AllReduce architecture.
//!
//! Algorithms:
//! - **Distributed Linear Regression** — gradient aggregation across workers
//! - **Distributed K-Means** — centroid update via two-phase reduce
//! - **Distributed Gradient Boosting** — feature-parallel tree building

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use serde::{Deserialize, Serialize};

// ─── Distributed Linear Regression ───────────────────────────────────────────

/// Gradient update from one worker partition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradientUpdate {
    pub grad:  Vec<f64>,   // ∂Loss/∂w for each weight
    pub n:     usize,      // number of rows in this partition
}

/// Distributed Linear Regression via mini-batch gradient descent.
///
/// Each iteration:
/// 1. Broadcast current weights to all workers.
/// 2. Each worker computes local gradient on its partition.
/// 3. Coordinator aggregates gradients (weighted average).
/// 4. Coordinator updates weights with the aggregated gradient.
pub struct DistributedLinearRegressor {
    pub learning_rate: f64,
    pub epochs:        usize,
    pub lambda:        f64,   // L2 regularization
    pub weights:       Vec<f64>,
}

impl DistributedLinearRegressor {
    pub fn new(learning_rate: f64, epochs: usize, lambda: f64) -> Self {
        Self { learning_rate, epochs, lambda, weights: vec![] }
    }

    /// Simulate distributed training: split data into `n_workers` partitions
    /// and run gradient aggregation locally (same logic as real distributed version).
    pub fn fit(&mut self, x: &[Vec<f64>], y: &[f64], n_workers: usize) {
        let n = x.len();
        if n == 0 { return; }
        let d = x[0].len() + 1;  // +1 for bias
        self.weights = vec![0.0f64; d];

        let chunk = (n + n_workers - 1) / n_workers;

        for _epoch in 0..self.epochs {
            // Simulate gradient aggregation from each worker
            let mut agg_grad = vec![0.0f64; d];
            let mut total_n  = 0usize;

            for wid in 0..n_workers {
                let start = wid * chunk;
                let end   = n.min(start + chunk);
                if start >= n { break; }

                let update = self.compute_gradient(&x[start..end], &y[start..end]);
                let wn = update.n as f64;
                for j in 0..d { agg_grad[j] += update.grad[j] * wn; }
                total_n += update.n;
            }

            // Average gradient
            if total_n > 0 {
                for j in 0..d { agg_grad[j] /= total_n as f64; }
            }

            // Update weights (SGD + L2)
            for j in 0..d {
                let reg = if j == 0 { 0.0 } else { self.lambda * self.weights[j] };
                self.weights[j] -= self.learning_rate * (agg_grad[j] + reg);
            }
        }
    }

    fn compute_gradient(&self, x: &[Vec<f64>], y: &[f64]) -> GradientUpdate {
        let n = x.len();
        let d = self.weights.len();
        let mut grad = vec![0.0f64; d];
        for (xi, &yi) in x.iter().zip(y.iter()) {
            let aug: Vec<f64> = std::iter::once(1.0).chain(xi.iter().copied()).collect();
            let pred: f64 = aug.iter().zip(self.weights.iter()).map(|(a, w)| a * w).sum();
            let err = pred - yi;
            for j in 0..d { grad[j] += err * aug[j]; }
        }
        for g in &mut grad { *g /= n as f64; }
        GradientUpdate { grad, n }
    }

    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| {
            let aug: Vec<f64> = std::iter::once(1.0).chain(xi.iter().copied()).collect();
            aug.iter().zip(self.weights.iter()).map(|(a, w)| a * w).sum()
        }).collect()
    }

    pub fn r2(&self, x: &[Vec<f64>], y: &[f64]) -> f64 {
        let preds = self.predict(x);
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        let ss_tot: f64 = y.iter().map(|&v| (v - mean).powi(2)).sum();
        let ss_res: f64 = y.iter().zip(preds.iter()).map(|(&t, &p)| (t-p).powi(2)).sum();
        if ss_tot == 0.0 { 1.0 } else { 1.0 - ss_res / ss_tot }
    }
}

// ─── Distributed K-Means ──────────────────────────────────────────────────────

/// K-Means with distributed centroid computation.
///
/// Each worker computes local partial sums and counts per centroid.
/// Coordinator aggregates to get global centroids.
pub struct DistributedKMeans {
    pub k:         usize,
    pub max_iters: usize,
    pub tol:       f64,
    pub centroids: Vec<Vec<f64>>,
}

impl DistributedKMeans {
    pub fn new(k: usize, max_iters: usize) -> Self {
        Self { k, max_iters, tol: 1e-4, centroids: vec![] }
    }

    pub fn fit(&mut self, x: &[Vec<f64>], n_workers: usize) {
        if x.is_empty() { return; }
        let n = x.len();
        let d = x[0].len();
        let chunk = (n + n_workers - 1) / n_workers;

        // Initialize centroids with first k points
        self.centroids = x.iter().take(self.k).cloned().collect();
        if self.centroids.len() < self.k {
            while self.centroids.len() < self.k {
                self.centroids.push(vec![0.0; d]);
            }
        }

        for _iter in 0..self.max_iters {
            let mut new_sums  = vec![vec![0.0f64; d]; self.k];
            let mut new_counts = vec![0usize; self.k];

            // Each worker computes local partial sums
            for wid in 0..n_workers {
                let start = wid * chunk;
                let end   = n.min(start + chunk);
                if start >= n { break; }
                let (sums, counts) = self.local_assign(&x[start..end]);
                for k in 0..self.k {
                    for j in 0..d { new_sums[k][j] += sums[k][j]; }
                    new_counts[k] += counts[k];
                }
            }

            // Update centroids (global average)
            let old = self.centroids.clone();
            for k in 0..self.k {
                if new_counts[k] > 0 {
                    for j in 0..d { self.centroids[k][j] = new_sums[k][j] / new_counts[k] as f64; }
                }
            }

            // Check convergence
            let shift: f64 = old.iter().zip(self.centroids.iter())
                .map(|(a, b)| a.iter().zip(b.iter()).map(|(x, y)| (x-y).powi(2)).sum::<f64>().sqrt())
                .sum();
            if shift < self.tol { break; }
        }
    }

    fn local_assign(&self, x: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<usize>) {
        let d = if x.is_empty() { 0 } else { x[0].len() };
        let mut sums   = vec![vec![0.0f64; d]; self.k];
        let mut counts = vec![0usize; self.k];
        for xi in x {
            let c = self.nearest_centroid(xi);
            for j in 0..d { sums[c][j] += xi[j]; }
            counts[c] += 1;
        }
        (sums, counts)
    }

    fn nearest_centroid(&self, x: &[f64]) -> usize {
        self.centroids.iter().enumerate()
            .map(|(i, c)| (i, c.iter().zip(x.iter()).map(|(a, b)| (a-b).powi(2)).sum::<f64>()))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i).unwrap_or(0)
    }

    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<usize> {
        x.iter().map(|xi| self.nearest_centroid(xi)).collect()
    }

    /// Inertia (within-cluster sum of squared distances).
    pub fn inertia(&self, x: &[Vec<f64>]) -> f64 {
        x.iter().map(|xi| {
            let c = self.nearest_centroid(xi);
            xi.iter().zip(self.centroids[c].iter()).map(|(a, b)| (a-b).powi(2)).sum::<f64>()
        }).sum()
    }
}

// ─── Distributed Gradient Boosting ───────────────────────────────────────────

/// Feature-parallel GBM: each worker trains on a subset of features per tree.
/// This matches XGBoost's `colsample_bytree` strategy.
pub struct DistributedGBM {
    pub n_estimators:    usize,
    pub learning_rate:   f64,
    pub max_depth:       usize,
    pub col_sample:      f64,    // fraction of features per tree (like colsample_bytree)
    trees:               Vec<SimpleTree>,
    f0:                  f64,
}

#[derive(Debug, Clone)]
struct SimpleTree {
    feature: usize,
    threshold: f64,
    left_val: f64,
    right_val: f64,
}

impl SimpleTree {
    fn predict(&self, x: &[f64]) -> f64 {
        if x.get(self.feature).copied().unwrap_or(0.0) <= self.threshold {
            self.left_val
        } else {
            self.right_val
        }
    }
}

impl DistributedGBM {
    pub fn new(n_estimators: usize, learning_rate: f64, col_sample: f64) -> Self {
        Self { n_estimators, learning_rate, col_sample, max_depth: 3, trees: vec![], f0: 0.0 }
    }

    pub fn fit(&mut self, x: &[Vec<f64>], y: &[f64], n_workers: usize) {
        let n = x.len();
        if n == 0 { return; }
        let n_feats = x[0].len();
        self.f0 = y.iter().sum::<f64>() / n as f64;
        let mut f: Vec<f64> = vec![self.f0; n];
        self.trees.clear();

        for m in 0..self.n_estimators {
            let residuals: Vec<f64> = y.iter().zip(f.iter()).map(|(&yi, &fi)| yi - fi).collect();

            // Feature-parallel: each worker handles a subset of features
            let cols_per_worker = ((n_feats as f64 * self.col_sample) as usize).max(1);
            let best_tree = self.find_best_split(x, &residuals, cols_per_worker, m, n_workers);

            for (i, xi) in x.iter().enumerate() {
                f[i] += self.learning_rate * best_tree.predict(xi);
            }
            self.trees.push(best_tree);
        }
    }

    fn find_best_split(
        &self, x: &[Vec<f64>], r: &[f64], cols: usize, seed: usize, n_workers: usize
    ) -> SimpleTree {
        let n_feats = x.first().map(|r| r.len()).unwrap_or(1);
        let mut best_gain = f64::NEG_INFINITY;
        let mut best_tree = SimpleTree { feature: 0, threshold: 0.0, left_val: 0.0, right_val: 0.0 };

        // Each worker tries different feature subsets
        for w in 0..n_workers {
            let feat_start = (w * cols) % n_feats;
            let feat_end   = (feat_start + cols).min(n_feats);
            for fi in feat_start..feat_end {
                // Find best threshold for this feature
                let mut vals: Vec<(f64, f64)> = x.iter().zip(r.iter())
                    .filter_map(|(xi, &ri)| xi.get(fi).map(|&v| (v, ri)))
                    .collect();
                vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

                let total_sum: f64 = vals.iter().map(|(_, r)| r).sum();
                let mut left_sum = 0.0f64;

                for (i, &(threshold, ri)) in vals.iter().enumerate() {
                    left_sum += ri;
                    let left_n  = i + 1;
                    let right_n = vals.len() - left_n;
                    if right_n == 0 { continue; }
                    let right_sum = total_sum - left_sum;
                    let gain = left_sum * left_sum / left_n as f64
                             + right_sum * right_sum / right_n as f64;
                    if gain > best_gain {
                        best_gain = gain;
                        let left_val  = left_sum / left_n as f64;
                        let right_val = right_sum / right_n as f64;
                        best_tree = SimpleTree { feature: fi, threshold, left_val, right_val };
                    }
                }
            }
        }
        best_tree
    }

    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| {
            self.f0 + self.trees.iter().map(|t| self.learning_rate * t.predict(xi)).sum::<f64>()
        }).collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_data(n: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
        let x: Vec<Vec<f64>> = (0..n).map(|i| vec![i as f64, (i % 5) as f64]).collect();
        let y: Vec<f64>      = x.iter().map(|r| 2.0 * r[0] + r[1] + 3.0).collect();
        (x, y)
    }

    #[test]
    fn test_dist_linear_regression() {
        // Use small-scale data to avoid gradient explosion
        let x: Vec<Vec<f64>> = (0..50).map(|i| vec![(i as f64) / 50.0]).collect();
        let y: Vec<f64>      = x.iter().map(|r| 3.0 * r[0] + 1.0).collect();
        let mut model = DistributedLinearRegressor::new(0.5, 300, 1e-6);
        model.fit(&x, &y, 4);
        let preds = model.predict(&x);
        let mse: f64 = preds.iter().zip(y.iter()).map(|(&p,&t)|(p-t).powi(2)).sum::<f64>() / x.len() as f64;
        assert!(mse < 1.0, "MSE={mse:.4}");
    }

    #[test]
    fn test_dist_kmeans() {
        // Two clear clusters
        let mut x: Vec<Vec<f64>> = (0..50).map(|_| vec![0.0, 0.0]).collect();
        x.extend((0..50).map(|_| vec![10.0, 10.0]));
        let mut km = DistributedKMeans::new(2, 20);
        km.fit(&x, 4);
        assert_eq!(km.centroids.len(), 2);
        let preds = km.predict(&x);
        // All first 50 should be in the same cluster
        let c0 = preds[0];
        assert!(preds[..50].iter().all(|&c| c == c0));
        assert!(preds[50..].iter().all(|&c| c != c0));
    }

    #[test]
    fn test_dist_gbm() {
        let (x, y) = linear_data(80);
        let mut gbm = DistributedGBM::new(30, 0.1, 0.8);
        gbm.fit(&x, &y, 4);
        let preds = gbm.predict(&x);
        let mse: f64 = preds.iter().zip(y.iter()).map(|(&p, &t)| (p-t).powi(2)).sum::<f64>() / x.len() as f64;
        assert!(mse < 100.0, "MSE={mse:.2}");
    }
}
