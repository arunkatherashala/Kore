//! LinearSVM — Pegasos SGD (Shalev-Shwartz et al. 2007).
//!
//! Binary classification: labels must be encoded as +1 / -1 (or 1.0 / 0.0).
//! 0.0 labels are converted to -1 internally.
//!
//! Update rule (averaged Pegasos):
//!   η_t = 1 / (λ t)
//!   if y_i (w · x_i) < 1 :  w ← (1 - η_t λ) w + η_t y_i x_i

use kore_core::{DataBlock, Estimator, KoreError};

#[derive(Debug, Clone)]
pub struct LinearSVM {
    pub lambda:     f64,        // regularisation strength (default 0.01)
    pub epochs:     usize,      // SGD passes (default 50)
    pub tol:        f64,        // convergence tolerance
    weights:        Vec<f64>,   // w (no bias term — prepend 1 to x for bias)
    feature_cols:   Vec<String>,
}

impl Default for LinearSVM {
    fn default() -> Self { Self::new(0.01, 100) }
}

impl LinearSVM {
    pub fn new(lambda: f64, epochs: usize) -> Self {
        Self { lambda, epochs, tol: 1e-5, weights: vec![], feature_cols: vec![] }
    }

    pub fn fit_raw(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n = x.len();
        let d = x.first().map_or(0, |r| r.len()) + 1; // +1 for bias

        let labels: Vec<f64> = y.iter().map(|&v| if v <= 0.5 { -1.0 } else { 1.0 }).collect();
        let mut w = vec![0.0f64; d];
        let mut t = 1usize;

        for _ in 0..self.epochs {
            for i in 0..n {
                let xi: Vec<f64> = std::iter::once(1.0).chain(x[i].iter().copied()).collect();
                let margin = dot(&w, &xi) * labels[i];
                let eta = 1.0 / (self.lambda * t as f64);
                // decay
                for wj in &mut w { *wj *= 1.0 - eta * self.lambda; }
                // hinge gradient
                if margin < 1.0 {
                    for (j, &xj) in xi.iter().enumerate() {
                        w[j] += eta * labels[i] * xj;
                    }
                }
                // project onto ‖w‖ ≤ 1/√λ
                let norm2: f64 = w.iter().map(|&wj| wj * wj).sum();
                let max_norm = 1.0 / self.lambda.sqrt();
                if norm2.sqrt() > max_norm {
                    let scale = max_norm / norm2.sqrt();
                    for wj in &mut w { *wj *= scale; }
                }
                t += 1;
            }
        }
        self.weights = w;
    }

    pub fn predict_single(&self, x: &[f64]) -> f64 {
        let xi: Vec<f64> = std::iter::once(1.0).chain(x.iter().copied()).collect();
        let score = dot(&self.weights, &xi);
        if score >= 0.0 { 1.0 } else { 0.0 }
    }

    pub fn predict_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| self.predict_single(xi)).collect()
    }

    pub fn decision_value(&self, x: &[f64]) -> f64 {
        let xi: Vec<f64> = std::iter::once(1.0).chain(x.iter().copied()).collect();
        dot(&self.weights, &xi)
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

impl Estimator for LinearSVM {
    fn name(&self) -> &str { "LinearSVM" }

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
    fn svm_linearly_separable() {
        // x < 0 → class 0,  x > 0 → class 1
        let x: Vec<Vec<f64>> = (-20i32..=20).filter(|&i| i != 0)
            .map(|i| vec![i as f64]).collect();
        let y: Vec<f64>      = x.iter().map(|r| if r[0] < 0.0 { 0.0 } else { 1.0 }).collect();
        let mut svm = LinearSVM::new(0.001, 200);
        svm.fit_raw(&x, &y);
        assert_eq!(svm.predict_single(&[-5.0]) as i32, 0);
        assert_eq!(svm.predict_single(&[ 5.0]) as i32, 1);
    }
}
