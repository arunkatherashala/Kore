//! Gradient Boosting Regressor — additive model trained on residuals.
//!
//! Algorithm (squared-loss):
//!   F₀(x) = mean(y)
//!   for m in 1..n_estimators:
//!     r_i = y_i − F_{m-1}(x_i)          (pseudo-residuals)
//!     fit tree T_m to (x, r)
//!     F_m(x) = F_{m-1}(x) + η · T_m(x)  (η = learning_rate)

use kore_core::{DataBlock, Estimator, KoreError};
use crate::{
    decision_tree::{mean, DecisionTree, Task},
};

#[derive(Debug, Clone)]
pub struct GradientBoostingRegressor {
    pub n_estimators:      usize,
    pub learning_rate:     f64,
    pub max_depth:         usize,
    pub min_samples_split: usize,
    f0:                    f64,
    trees:                 Vec<DecisionTree>,
    feature_cols:          Vec<String>,
}

impl GradientBoostingRegressor {
    pub fn new(n_estimators: usize, learning_rate: f64, max_depth: usize) -> Self {
        Self {
            n_estimators,
            learning_rate,
            max_depth,
            min_samples_split: 2,
            f0: 0.0,
            trees: vec![],
            feature_cols: vec![],
        }
    }

    pub fn fit_raw(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n = x.len();
        if n == 0 { return; }

        self.f0 = mean(y);
        let mut f: Vec<f64> = vec![self.f0; n];
        self.trees.clear();

        for m in 0..self.n_estimators {
            // Pseudo-residuals (negative gradient of squared loss)
            let r: Vec<f64> = y.iter().zip(f.iter()).map(|(&yi, &fi)| yi - fi).collect();

            let mut tree = DecisionTree::new_with_params(
                self.max_depth, self.min_samples_split, None, Task::Regression,
                42u64.wrapping_add(m as u64), 255,
            );
            tree.fit_raw(x, &r);

            // Update F
            for (i, xi) in x.iter().enumerate() {
                f[i] += self.learning_rate * tree.predict_single(xi);
            }
            self.trees.push(tree);
        }
    }

    pub fn predict_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| {
            self.f0 + self.trees.iter()
                .map(|t| self.learning_rate * t.predict_single(xi))
                .sum::<f64>()
        }).collect()
    }
}

impl Estimator for GradientBoostingRegressor {
    fn name(&self) -> &str { "GradientBoostingRegressor" }

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

    #[test]
    fn gbm_sine_approx() {
        use std::f64::consts::PI;
        let x: Vec<Vec<f64>> = (0..60).map(|i| vec![(i as f64) / 60.0 * 2.0 * PI]).collect();
        let y: Vec<f64>      = x.iter().map(|r| r[0].sin()).collect();
        let mut gbm = GradientBoostingRegressor::new(80, 0.1, 3);
        gbm.fit_raw(&x, &y);
        let preds = gbm.predict_raw(&x);
        let rmse = (preds.iter().zip(y.iter())
            .map(|(&p, &t)| (p - t).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
        assert!(rmse < 0.15, "RMSE={:.4}", rmse);
    }

    #[test]
    fn gbm_linear() {
        let x: Vec<Vec<f64>> = (0..50).map(|i| vec![i as f64]).collect();
        let y: Vec<f64>      = x.iter().map(|r| 2.0 * r[0] + 5.0).collect();
        let mut gbm = GradientBoostingRegressor::new(50, 0.1, 2);
        gbm.fit_raw(&x, &y);
        let p = gbm.predict_raw(&[vec![25.0]])[0];
        assert!((p - 55.0).abs() < 5.0, "p={}", p);
    }
}
