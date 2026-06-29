//! Random Forest — bagging ensemble of Decision Trees.
//!
//! Both Regressor (mean) and Classifier (majority vote) variants.
//! Uses rayon for parallel tree training.

use rayon::prelude::*;
use kore_core::{DataBlock, Estimator, KoreError};
use crate::{
    decision_tree::{DecisionTree, Task},
    Rng,
};

// ─── Regressor ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RandomForestRegressor {
    pub n_estimators:       usize,
    pub max_depth:          usize,
    pub min_samples_split:  usize,
    pub max_features:       Option<usize>,   // None = sqrt(n_features)
    pub seed:               u64,
    trees:                  Vec<DecisionTree>,
    feature_cols:           Vec<String>,
}

impl RandomForestRegressor {
    pub fn new(n_estimators: usize, max_depth: usize) -> Self {
        Self {
            n_estimators,
            max_depth,
            min_samples_split: 2,
            max_features: None,
            seed: 42,
            trees: vec![],
            feature_cols: vec![],
        }
    }

    pub fn fit_raw(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n = x.len();
        if n == 0 { return; }
        let n_feat = x[0].len();
        let mf = self.max_features.unwrap_or(((n_feat as f64).sqrt() as usize).max(1));

        // Build (bootstrap_x, bootstrap_y, tree_seed) for each tree in parallel
        let configs: Vec<(Vec<Vec<f64>>, Vec<f64>, u64)> = {
            let mut rng = Rng::new(self.seed);
            (0..self.n_estimators).map(|_| {
                let idxs   = rng.bootstrap(n, n);
                let bx: Vec<Vec<f64>> = idxs.iter().map(|&i| x[i].clone()).collect();
                let by: Vec<f64>      = idxs.iter().map(|&i| y[i]).collect();
                let tree_seed = rng.next_u64();
                (bx, by, tree_seed)
            }).collect()
        };

        self.trees = configs
            .into_par_iter()
            .map(|(bx, by, ts)| {
                let mut tree = DecisionTree::new_with_params(
                    self.max_depth, self.min_samples_split, Some(mf), Task::Regression, ts, 255,
                );
                tree.fit_raw(&bx, &by);
                tree
            })
            .collect();
    }

    pub fn predict_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        let n = x.len();
        let mut sums = vec![0.0f64; n];
        for tree in &self.trees {
            for (i, xi) in x.iter().enumerate() {
                sums[i] += tree.predict_single(xi);
            }
        }
        let k = self.trees.len() as f64;
        sums.iter().map(|&s| s / k).collect()
    }
}

impl Estimator for RandomForestRegressor {
    fn name(&self) -> &str { "RandomForestRegressor" }

    fn fit(&mut self, data: &DataBlock, target_col: &str) -> Result<(), KoreError> {
        self.feature_cols = data.columns.iter()
            .filter(|c| c.name != target_col)
            .map(|c| c.name.clone())
            .collect();
        let feat_refs: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        let x = data.to_feature_matrix(&feat_refs)?;
        let y = data.to_target_vector(target_col)?;
        self.fit_raw(&x, &y);
        Ok(())
    }

    fn predict(&self, data: &DataBlock) -> Result<Vec<f64>, KoreError> {
        let feat_refs: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        let x = data.to_feature_matrix(&feat_refs)?;
        Ok(self.predict_raw(&x))
    }
}

// ─── Classifier ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RandomForestClassifier {
    pub n_estimators:      usize,
    pub max_depth:         usize,
    pub min_samples_split: usize,
    pub max_features:      Option<usize>,
    pub seed:              u64,
    trees:                 Vec<DecisionTree>,
    feature_cols:          Vec<String>,
}

impl RandomForestClassifier {
    pub fn new(n_estimators: usize, max_depth: usize) -> Self {
        Self {
            n_estimators, max_depth, min_samples_split: 2,
            max_features: None, seed: 42, trees: vec![], feature_cols: vec![],
        }
    }

    pub fn fit_raw(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n = x.len();
        if n == 0 { return; }
        let n_feat = x[0].len();
        let mf = self.max_features.unwrap_or(((n_feat as f64).sqrt() as usize).max(1));

        let configs: Vec<(Vec<Vec<f64>>, Vec<f64>, u64)> = {
            let mut rng = Rng::new(self.seed);
            (0..self.n_estimators).map(|_| {
                let idxs  = rng.bootstrap(n, n);
                let bx    = idxs.iter().map(|&i| x[i].clone()).collect();
                let by    = idxs.iter().map(|&i| y[i]).collect();
                (bx, by, rng.next_u64())
            }).collect()
        };

        self.trees = configs
            .into_par_iter()
            .map(|(bx, by, ts)| {
                let mut t = DecisionTree::new_with_params(
                    self.max_depth, self.min_samples_split, Some(mf), Task::Classification, ts, 255,
                );
                t.fit_raw(&bx, &by);
                t
            })
            .collect();
    }

    /// Returns predicted class labels (majority vote).
    pub fn predict_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| {
            let mut votes: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
            for tree in &self.trees {
                *votes.entry(tree.predict_single(xi) as i64).or_insert(0) += 1;
            }
            votes.into_iter().max_by_key(|(_, c)| *c).map(|(k, _)| k as f64).unwrap_or(0.0)
        }).collect()
    }
}

impl Estimator for RandomForestClassifier {
    fn name(&self) -> &str { "RandomForestClassifier" }

    fn fit(&mut self, data: &DataBlock, target_col: &str) -> Result<(), KoreError> {
        self.feature_cols = data.columns.iter()
            .filter(|c| c.name != target_col)
            .map(|c| c.name.clone())
            .collect();
        let feat_refs: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        let x = data.to_feature_matrix(&feat_refs)?;
        let y = data.to_target_vector(target_col)?;
        self.fit_raw(&x, &y);
        Ok(())
    }

    fn predict(&self, data: &DataBlock) -> Result<Vec<f64>, KoreError> {
        let feat_refs: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        let x = data.to_feature_matrix(&feat_refs)?;
        Ok(self.predict_raw(&x))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn xor_dataset(n: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
        let mut rng = Rng::new(1);
        let x: Vec<Vec<f64>> = (0..n).map(|_| vec![
            if rng.next_u64() % 2 == 0 { 0.0 } else { 1.0 },
            if rng.next_u64() % 2 == 0 { 0.0 } else { 1.0 },
        ]).collect();
        let y: Vec<f64> = x.iter().map(|r| if r[0] as i32 ^ r[1] as i32 == 1 { 1.0 } else { 0.0 }).collect();
        (x, y)
    }

    #[test]
    fn rf_classifier_xor() {
        let (x, y) = xor_dataset(200);
        let mut rf = RandomForestClassifier::new(20, 4);
        rf.fit_raw(&x, &y);
        let preds = rf.predict_raw(&x);
        let acc = preds.iter().zip(y.iter())
            .filter(|(&p, &t)| (p - t).abs() < 0.5)
            .count() as f64 / x.len() as f64;
        assert!(acc > 0.70, "accuracy={}", acc);
    }

    #[test]
    fn rf_regressor_linear() {
        let x: Vec<Vec<f64>> = (0..100).map(|i| vec![i as f64]).collect();
        let y: Vec<f64>      = (0..100).map(|i| i as f64 * 3.0 + 1.0).collect();
        let mut rf = RandomForestRegressor::new(50, 5);
        rf.fit_raw(&x, &y);
        let pred = rf.predict_raw(&[vec![50.0]])[0];
        assert!((pred - 151.0).abs() < 15.0, "pred={}", pred);
    }
}
