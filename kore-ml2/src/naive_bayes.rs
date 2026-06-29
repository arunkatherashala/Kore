//! Gaussian Naive Bayes — per-class Gaussian likelihood in log-space.
//!
//! Prediction: argmax_k  [ log P(k) + Σ_j log N(x_j; μ_kj, σ_kj²) ]

use std::collections::HashMap;
use kore_core::{DataBlock, Estimator, KoreError};

const LOG_2PI: f64 = 1.8378770664093453;   // ln(2π)
const MIN_VAR: f64 = 1e-9;                  // variance floor (numerical stability)

/// Per-class Gaussian parameters for each feature
#[derive(Debug, Clone)]
struct ClassStats {
    log_prior: f64,
    mean:      Vec<f64>,
    var:       Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct GaussianNaiveBayes {
    classes: Vec<i64>,
    stats:   HashMap<i64, ClassStats>,
    n_features: usize,
    feature_cols: Vec<String>,
}

impl GaussianNaiveBayes {
    pub fn new() -> Self {
        Self { classes: vec![], stats: HashMap::new(), n_features: 0, feature_cols: vec![] }
    }

    pub fn fit_raw(&mut self, x: &[Vec<f64>], y: &[f64]) {
        let n = y.len();
        if n == 0 { return; }
        self.n_features = x.first().map_or(0, |r| r.len());

        // Count classes
        let mut class_indices: HashMap<i64, Vec<usize>> = HashMap::new();
        for (i, &yi) in y.iter().enumerate() {
            class_indices.entry(yi as i64).or_default().push(i);
        }

        self.classes = {
            let mut ks: Vec<i64> = class_indices.keys().copied().collect();
            ks.sort_unstable();
            ks
        };

        self.stats.clear();
        for (&cls, idxs) in &class_indices {
            let nc    = idxs.len() as f64;
            let prior = nc / n as f64;

            let mut mean_vec = vec![0.0f64; self.n_features];
            let mut var_vec  = vec![0.0f64; self.n_features];

            // Feature means
            for &i in idxs {
                for j in 0..self.n_features {
                    mean_vec[j] += x[i][j];
                }
            }
            for j in 0..self.n_features { mean_vec[j] /= nc; }

            // Feature variances
            for &i in idxs {
                for j in 0..self.n_features {
                    let d = x[i][j] - mean_vec[j];
                    var_vec[j] += d * d;
                }
            }
            for j in 0..self.n_features {
                var_vec[j] = (var_vec[j] / nc).max(MIN_VAR);
            }

            self.stats.insert(cls, ClassStats {
                log_prior: prior.ln(),
                mean: mean_vec,
                var:  var_vec,
            });
        }
    }

    pub fn predict_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.iter().map(|xi| self.predict_single(xi)).collect()
    }

    fn predict_single(&self, x: &[f64]) -> f64 {
        let mut best_cls   = *self.classes.first().unwrap_or(&0);
        let mut best_score = f64::NEG_INFINITY;

        for &cls in &self.classes {
            let stats = &self.stats[&cls];
            let score = stats.log_prior + log_likelihood(x, &stats.mean, &stats.var);
            if score > best_score {
                best_score = score;
                best_cls   = cls;
            }
        }
        best_cls as f64
    }

    /// Posterior log-probabilities for each class (unnormalised)
    pub fn predict_log_proba(&self, x: &[Vec<f64>]) -> Vec<Vec<(i64, f64)>> {
        x.iter().map(|xi| {
            self.classes.iter().map(|&cls| {
                let stats = &self.stats[&cls];
                let score = stats.log_prior + log_likelihood(xi, &stats.mean, &stats.var);
                (cls, score)
            }).collect()
        }).collect()
    }
}

impl Default for GaussianNaiveBayes {
    fn default() -> Self { Self::new() }
}

fn log_likelihood(x: &[f64], mean: &[f64], var: &[f64]) -> f64 {
    x.iter().zip(mean.iter()).zip(var.iter())
        .map(|((&xi, &m), &v)| {
            -0.5 * (LOG_2PI + v.ln() + (xi - m).powi(2) / v)
        })
        .sum::<f64>()
}

impl Estimator for GaussianNaiveBayes {
    fn name(&self) -> &str { "GaussianNaiveBayes" }

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
    fn gnb_two_gaussians() {
        // Class 0: x ~ N(0,1)   Class 1: x ~ N(5,1)
        let mut x: Vec<Vec<f64>> = Vec::new();
        let mut y: Vec<f64>      = Vec::new();
        for i in 0..50i64 {
            x.push(vec![i as f64 * 0.1]);       y.push(0.0);  // cluster around 0..5
            x.push(vec![5.0 + i as f64 * 0.1]); y.push(1.0); // cluster around 5..10
        }
        let mut gnb = GaussianNaiveBayes::new();
        gnb.fit_raw(&x, &y);
        assert_eq!(gnb.predict_single(&[2.0]) as i64, 0);
        assert_eq!(gnb.predict_single(&[8.0]) as i64, 1);
    }

    #[test]
    fn gnb_three_classes() {
        let data: Vec<(f64, f64)> = vec![
            (0.0,0.0),(0.1,0.0),(0.2,0.0),(0.3,0.0),
            (5.0,1.0),(5.1,1.0),(5.2,1.0),(5.3,1.0),
            (10.0,2.0),(10.1,2.0),(10.2,2.0),(10.3,2.0),
        ];
        let x: Vec<Vec<f64>> = data.iter().map(|(v,_)| vec![*v]).collect();
        let y: Vec<f64>      = data.iter().map(|(_,l)| *l).collect();
        let mut gnb = GaussianNaiveBayes::new();
        gnb.fit_raw(&x, &y);
        assert_eq!(gnb.predict_single(&[0.15]) as i64, 0);
        assert_eq!(gnb.predict_single(&[5.15]) as i64, 1);
        assert_eq!(gnb.predict_single(&[10.15]) as i64, 2);
    }
}
