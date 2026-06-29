//! LinearRegressor — OLS via Gaussian elimination (ridge-regularised).
//!
//! Fits β = (XᵀX + λI)⁻¹ Xᵀy  where λ=1e-8 by default (numerical stability).

use kore_core::{DataBlock, Estimator, KoreError};

#[derive(Debug, Clone)]
pub struct LinearRegressor {
    pub lambda:      f64,           // L2 regularisation (default 1e-8)
    weights:         Vec<f64>,      // β coefficients (includes intercept)
    feature_cols:    Vec<String>,
}

impl Default for LinearRegressor {
    fn default() -> Self { Self::new(1e-8) }
}

impl LinearRegressor {
    pub fn new(lambda: f64) -> Self {
        Self { lambda, weights: vec![], feature_cols: vec![] }
    }

    /// Fit on raw feature matrix (no intercept column needed — added internally).
    pub fn fit_raw(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n = x.len();
        let d = x.first().map_or(0, |r| r.len());
        let p = d + 1; // +1 for intercept

        // Build X_aug (n × p)  with a leading column of 1s
        // Compute XᵀX (p×p) and Xᵀy (p) directly
        let mut xtx = vec![0.0f64; p * p];
        let mut xty = vec![0.0f64; p];

        for (i, (row, &yi)) in x.iter().zip(y.iter()).enumerate() {
            let xi: Vec<f64> = std::iter::once(1.0).chain(row.iter().copied()).collect();
            for j in 0..p {
                xty[j] += xi[j] * yi;
                for k in 0..p {
                    xtx[j * p + k] += xi[j] * xi[k];
                }
            }
            let _ = i;
        }

        // Add ridge regularisation (skip intercept term j=0)
        for j in 1..p {
            xtx[j * p + j] += self.lambda;
        }

        // Solve XᵀX β = Xᵀy via Gaussian elimination with partial pivoting
        self.weights = gauss_solve(&mut xtx, &mut xty, p)
            .unwrap_or_else(|| vec![0.0; p]);
    }

    pub fn predict_single(&self, x: &[f64]) -> f64 {
        if self.weights.is_empty() { return 0.0; }
        let mut s = self.weights[0]; // intercept
        for (j, &xj) in x.iter().enumerate() {
            s += self.weights.get(j + 1).copied().unwrap_or(0.0) * xj;
        }
        s
    }

    pub fn predict_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| self.predict_single(xi)).collect()
    }

    /// R² score on test data.
    pub fn r2(&self, x: &[Vec<f64>], y: &[f64]) -> f64 {
        let preds = self.predict_raw(x);
        crate::metrics::r2(y, &preds)
    }
}

impl Estimator for LinearRegressor {
    fn name(&self) -> &str { "LinearRegressor" }

    fn fit(&mut self, data: &DataBlock, target_col: &str) -> Result<(), KoreError> {
        self.feature_cols = data.columns.iter()
            .filter(|c| c.name != target_col)
            .map(|c| c.name.clone())
            .collect();
        let feat: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        let x = data.to_feature_matrix(&feat)?;
        let y = data.to_target_vector(target_col)?;
        self.fit_raw(&x, &y);
        Ok(())
    }

    fn predict(&self, data: &DataBlock) -> Result<Vec<f64>, KoreError> {
        let feat: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        let x = data.to_feature_matrix(&feat)?;
        Ok(self.predict_raw(&x))
    }
}

// ─── Gaussian elimination with partial pivoting ───────────────────────────────

/// Solve Ax = b in-place using partial pivoting.  Returns None if singular.
pub fn gauss_solve(a: &mut [f64], b: &mut [f64], n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        // find pivot
        let pivot_row = (col..n)
            .max_by(|&r1, &r2| a[r1*n+col].abs().partial_cmp(&a[r2*n+col].abs()).unwrap())?;
        if a[pivot_row * n + col].abs() < 1e-14 { return None; }
        // swap rows
        for k in 0..n { a.swap(col * n + k, pivot_row * n + k); }
        b.swap(col, pivot_row);
        // eliminate below
        let diag = a[col * n + col];
        for row in (col + 1)..n {
            let factor = a[row * n + col] / diag;
            for k in col..n { a[row * n + k] -= factor * a[col * n + k]; }
            b[row] -= factor * b[col];
        }
    }
    // back-substitution
    let mut x = vec![0.0f64; n];
    for i in (0..n).rev() {
        x[i] = b[i];
        for j in (i + 1)..n { x[i] -= a[i * n + j] * x[j]; }
        x[i] /= a[i * n + i];
    }
    Some(x)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_perfect_fit() {
        // y = 2x + 3
        let x: Vec<Vec<f64>> = (0..20).map(|i| vec![i as f64]).collect();
        let y: Vec<f64>      = x.iter().map(|r| 2.0 * r[0] + 3.0).collect();
        let mut lr = LinearRegressor::new(1e-8);
        lr.fit_raw(&x, &y);
        let pred = lr.predict_single(&[10.0]);
        assert!((pred - 23.0).abs() < 0.01, "pred={pred}");
        let r2 = lr.r2(&x, &y);
        assert!(r2 > 0.999, "r2={r2}");
    }

    #[test]
    fn multivariate_regression() {
        // y = x0 + 2*x1 - x2 + 5
        let x: Vec<Vec<f64>> = (0..50).map(|i| vec![i as f64, (i % 7) as f64, (i % 3) as f64]).collect();
        let y: Vec<f64>      = x.iter().map(|r| r[0] + 2.0*r[1] - r[2] + 5.0).collect();
        let mut lr = LinearRegressor::new(1e-8);
        lr.fit_raw(&x, &y);
        let r2 = lr.r2(&x, &y);
        assert!(r2 > 0.99, "r2={r2}");
    }
}
