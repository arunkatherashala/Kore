//! LogisticRegressor — mini-batch SGD with cross-entropy loss.

use kore_core::{DataBlock, Estimator, KoreError};

#[derive(Debug, Clone)]
pub struct LogisticRegressor {
    pub lr:          f64,      // learning rate
    pub epochs:      usize,
    pub batch_size:  usize,
    pub lambda:      f64,      // L2 reg
    weights:         Vec<f64>, // includes bias at index 0
    feature_cols:    Vec<String>,
}

impl Default for LogisticRegressor {
    fn default() -> Self { Self::new(0.1, 100, 32, 1e-4) }
}

impl LogisticRegressor {
    pub fn new(lr: f64, epochs: usize, batch_size: usize, lambda: f64) -> Self {
        Self { lr, epochs, batch_size, lambda, weights: vec![], feature_cols: vec![] }
    }

    pub fn fit_raw(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n = x.len();
        let d = x.first().map_or(0, |r| r.len()) + 1;
        self.weights = vec![0.0f64; d];

        for epoch in 0..self.epochs {
            let offset = (epoch * self.batch_size) % n.max(1);
            let end    = (offset + self.batch_size).min(n);
            let batch_x = &x[offset..end];
            let batch_y = &y[offset..end];

            let mut grad = vec![0.0f64; d];
            for (xi, &yi) in batch_x.iter().zip(batch_y.iter()) {
                let aug: Vec<f64> = std::iter::once(1.0).chain(xi.iter().copied()).collect();
                let score = dot(&self.weights, &aug);
                let prob  = sigmoid(score);
                let err   = prob - yi;
                for j in 0..d { grad[j] += err * aug[j]; }
            }

            let m = batch_x.len().max(1) as f64;
            for j in 0..d {
                let reg = if j == 0 { 0.0 } else { self.lambda * self.weights[j] };
                self.weights[j] -= self.lr * (grad[j] / m + reg);
            }
        }
    }

    /// Returns probability of class 1.
    pub fn predict_proba(&self, x: &[f64]) -> f64 {
        let aug: Vec<f64> = std::iter::once(1.0).chain(x.iter().copied()).collect();
        sigmoid(dot(&self.weights, &aug))
    }

    pub fn predict_single(&self, x: &[f64]) -> f64 {
        if self.predict_proba(x) >= 0.5 { 1.0 } else { 0.0 }
    }

    pub fn predict_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| self.predict_single(xi)).collect()
    }

    pub fn predict_proba_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| self.predict_proba(xi)).collect()
    }
}

fn sigmoid(x: f64) -> f64 { 1.0 / (1.0 + (-x).exp()) }
fn dot(a: &[f64], b: &[f64]) -> f64 { a.iter().zip(b.iter()).map(|(x, y)| x * y).sum() }

impl Estimator for LogisticRegressor {
    fn name(&self) -> &str { "LogisticRegressor" }

    fn fit(&mut self, data: &DataBlock, target_col: &str) -> Result<(), KoreError> {
        self.feature_cols = data.columns.iter()
            .filter(|c| c.name != target_col).map(|c| c.name.clone()).collect();
        let feat: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        self.fit_raw(&data.to_feature_matrix(&feat)?, &data.to_target_vector(target_col)?);
        Ok(())
    }

    fn predict(&self, data: &DataBlock) -> Result<Vec<f64>, KoreError> {
        let feat: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        Ok(self.predict_raw(&data.to_feature_matrix(&feat)?))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logistic_binary() {
        let x: Vec<Vec<f64>> = (0..40).map(|i| vec![i as f64 - 20.0]).collect();
        let y: Vec<f64>      = x.iter().map(|r| if r[0] < 0.0 { 0.0 } else { 1.0 }).collect();
        let mut lr = LogisticRegressor::new(0.05, 300, 20, 1e-4);
        lr.fit_raw(&x, &y);
        assert_eq!(lr.predict_single(&[-10.0]) as i32, 0);
        assert_eq!(lr.predict_single(&[ 10.0]) as i32, 1);
    }
}
