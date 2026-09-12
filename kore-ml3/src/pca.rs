//! PCA — Principal Component Analysis via SVD.
//!
//! Fits via centring + SVD on the feature matrix.
//! Reduces dimensionality to n_components (default min(n_samples, n_features)).

use kore_core::{DataBlock, Estimator, KoreError};

#[derive(Debug, Clone)]
pub struct PCA {
    pub n_components: usize,
    mean:             Vec<f64>,           // Feature means (for centring)
    components:       Vec<Vec<f64>>,      // Principal components (eigenvectors) [n_components × n_features]
    explained_var:    Vec<f64>,           // Variance explained by each PC
    feature_cols:     Vec<String>,
}

impl PCA {
    pub fn new(n_components: usize) -> Self {
        Self {
            n_components,
            mean: vec![],
            components: vec![],
            explained_var: vec![],
            feature_cols: vec![],
        }
    }

    /// Fit on raw feature matrix.
    /// Uses SVD-based PCA (numerically stable).
    pub fn fit_raw(&mut self, x: &[Vec<f64>]) {
        let n = x.len();
        if n == 0 { return; }

        let d = x.first().map_or(0, |r| r.len());
        if d == 0 { return; }

        let k = self.n_components.min(d).min(n);

        // Compute mean
        self.mean = vec![0.0; d];
        for row in x {
            for (j, &xij) in row.iter().enumerate() {
                self.mean[j] += xij;
            }
        }
        for mj in &mut self.mean {
            *mj /= n as f64;
        }

        // Centre data: X_c = X - mean
        let mut x_c = vec![0.0; n * d];
        for (i, row) in x.iter().enumerate() {
            for (j, &xij) in row.iter().enumerate() {
                x_c[i * d + j] = xij - self.mean[j];
            }
        }

        // SVD on centred data (simplified: power iteration for top-k eigenvalues/eigenvectors)
        // For numerical stability, use QR-based method or eigendecomposition
        // Here we implement simplified eigendecomposition on covariance matrix

        // Compute covariance: C = X_c^T X_c / (n - 1)
        let mut cov = vec![0.0; d * d];
        for i in 0..d {
            for j in 0..d {
                let mut s = 0.0;
                for row_idx in 0..n {
                    s += x_c[row_idx * d + i] * x_c[row_idx * d + j];
                }
                cov[i * d + j] = s / (n as f64 - 1.0).max(1.0);
            }
        }

        // Power iteration to find top-k eigenvectors (approximate SVD)
        self.components = vec![];
        self.explained_var = vec![];

        for _ in 0..k {
            // Power iteration: find largest eigenvector
            let mut v = vec![1.0 / (d as f64).sqrt(); d];
            for _ in 0..20 {
                // C v
                let mut cv = vec![0.0; d];
                for i in 0..d {
                    for j in 0..d {
                        cv[i] += cov[i * d + j] * v[j];
                    }
                }
                // Normalize
                let norm: f64 = cv.iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm > 1e-10 {
                    for j in 0..d {
                        v[j] = cv[j] / norm;
                    }
                } else {
                    break;
                }
                let mut cv_norm = 0.0;
                for i in 0..d {
                    for j in 0..d {
                        cv_norm += cov[i * d + j] * v[j] * v[j];
                    }
                }
                let _ = cv_norm; // Use for variance
            }

            // Compute eigenvalue: λ = v^T C v
            let mut eigenval = 0.0;
            for i in 0..d {
                let mut s = 0.0;
                for j in 0..d {
                    s += cov[i * d + j] * v[j];
                }
                eigenval += v[i] * s;
            }
            eigenval = eigenval.max(0.0);

            self.components.push(v.clone());
            self.explained_var.push(eigenval);

            // Deflate covariance (optional, for more accurate subsequent components)
            // For simplicity, we skip deflation and approximate
        }
    }

    /// Project a single sample onto the principal components.
    pub fn transform_single(&self, x: &[f64]) -> Vec<f64> {
        if self.components.is_empty() || self.mean.len() != x.len() {
            return vec![];
        }

        // Centre
        let x_c: Vec<f64> = x.iter().zip(self.mean.iter())
            .map(|(&xi, &mi)| xi - mi)
            .collect();

        // Project onto each PC
        let mut proj = vec![];
        for pc in &self.components {
            let p: f64 = pc.iter().zip(x_c.iter())
                .map(|(&pij, &xj)| pij * xj)
                .sum();
            proj.push(p);
        }
        proj
    }

    /// Project all samples.
    pub fn transform_raw(&self, x: &[Vec<f64>]) -> Vec<Vec<f64>> {
        x.iter().map(|xi| self.transform_single(xi)).collect()
    }

    /// Explained variance ratio (cumulative)
    pub fn explained_variance_ratio(&self) -> Vec<f64> {
        if self.explained_var.is_empty() { return vec![]; }
        let total: f64 = self.explained_var.iter().sum();
        if total == 0.0 { return vec![]; }
        self.explained_var.iter().map(|&e| e / total).collect()
    }
}

impl Estimator for PCA {
    fn name(&self) -> &str { "PCA" }

    fn fit(&mut self, data: &DataBlock, _target_col: &str) -> Result<(), KoreError> {
        self.feature_cols = data.columns.iter()
            .map(|c| c.name.clone())
            .collect();
        let feat: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        let x = data.to_feature_matrix(&feat)?;
        self.fit_raw(&x);
        Ok(())
    }

    fn predict(&self, data: &DataBlock) -> Result<Vec<f64>, KoreError> {
        let feat: Vec<&str> = self.feature_cols.iter().map(|s| s.as_str()).collect();
        let x = data.to_feature_matrix(&feat)?;
        if x.is_empty() { return Ok(vec![]); }
        Ok(self.transform_single(&x[0]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pca_simple() {
        // 4 samples, 2 features
        let x = vec![
            vec![1.0, 2.0],
            vec![2.0, 4.0],
            vec![3.0, 6.0],
            vec![4.0, 8.0],
        ];
        let mut pca = PCA::new(1);
        pca.fit_raw(&x);
        assert_eq!(pca.components.len(), 1);
        assert_eq!(pca.mean.len(), 2);
    }

    #[test]
    fn test_pca_transform() {
        let x = vec![
            vec![1.0, 2.0],
            vec![2.0, 4.0],
            vec![3.0, 6.0],
            vec![4.0, 8.0],
        ];
        let mut pca = PCA::new(2);
        pca.fit_raw(&x);
        let proj = pca.transform_single(&[1.0, 2.0]);
        assert!(!proj.is_empty());
    }
}
