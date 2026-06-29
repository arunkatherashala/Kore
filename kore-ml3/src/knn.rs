//! K-Nearest Neighbors — brute force (exact), regression + classification.

use kore_core::{DataBlock, Estimator, KoreError};
use rayon::prelude::*;

#[derive(Debug, Clone)]
pub struct KNearestNeighbors {
    pub k:            usize,
    pub task:         KnnTask,
    train_x:          Vec<Vec<f64>>,
    train_y:          Vec<f64>,
    feature_cols:     Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnnTask { Regression, Classification }

impl KNearestNeighbors {
    pub fn new_regressor(k: usize)     -> Self { Self::new(k, KnnTask::Regression) }
    pub fn new_classifier(k: usize)    -> Self { Self::new(k, KnnTask::Classification) }
    pub fn new(k: usize, task: KnnTask) -> Self {
        Self { k, task, train_x: vec![], train_y: vec![], feature_cols: vec![] }
    }

    pub fn fit_raw(&mut self, x: &[Vec<f64>], y: &[f64]) {
        self.train_x = x.to_vec();
        self.train_y = y.to_vec();
    }

    pub fn predict_single(&self, xi: &[f64]) -> f64 {
        let k = self.k.min(self.train_x.len());
        if k == 0 { return 0.0; }

        // Compute distances to all training points
        let mut dists: Vec<(f64, f64)> = self.train_x.iter().zip(self.train_y.iter())
            .map(|(tx, &ty)| (euclidean_sq(xi, tx), ty))
            .collect();

        // Partial sort: keep k smallest
        dists.select_nth_unstable_by(k - 1, |a, b| a.0.partial_cmp(&b.0).unwrap());
        let neighbors = &dists[..k];

        match self.task {
            KnnTask::Regression => {
                neighbors.iter().map(|(_, y)| y).sum::<f64>() / k as f64
            }
            KnnTask::Classification => {
                // Majority vote
                let mut counts = std::collections::HashMap::<i64, usize>::new();
                for (_, y) in neighbors.iter() {
                    *counts.entry(*y as i64).or_insert(0) += 1;
                }
                counts.into_iter().max_by_key(|(_, c)| *c).map(|(k, _)| k as f64).unwrap_or(0.0)
            }
        }
    }

    pub fn predict_raw(&self, x: &[Vec<f64>]) -> Vec<f64> {
        x.par_iter().map(|xi| self.predict_single(xi)).collect()
    }
}

fn euclidean_sq(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum()
}

impl Estimator for KNearestNeighbors {
    fn name(&self) -> &str { "KNN" }

    fn fit(&mut self, data: &DataBlock, target_col: &str) -> Result<(), KoreError> {
        self.feature_cols = data.columns.iter()
            .filter(|c| c.name != target_col)
            .map(|c| c.name.clone())
            .collect();
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
    fn knn_regression() {
        let x: Vec<Vec<f64>> = (0..20).map(|i| vec![i as f64]).collect();
        let y: Vec<f64>      = x.iter().map(|r| r[0] * 2.0).collect();
        let mut knn = KNearestNeighbors::new_regressor(3);
        knn.fit_raw(&x, &y);
        let pred = knn.predict_single(&[10.0]);
        assert!((pred - 20.0).abs() < 4.0, "pred={pred}");
    }

    #[test]
    fn knn_classification() {
        let x: Vec<Vec<f64>> = (0..20).map(|i| vec![i as f64]).collect();
        let y: Vec<f64>      = (0..20).map(|i| if i < 10 { 0.0 } else { 1.0 }).collect();
        let mut knn = KNearestNeighbors::new_classifier(3);
        knn.fit_raw(&x, &y);
        assert_eq!(knn.predict_single(&[3.0])  as i32, 0);
        assert_eq!(knn.predict_single(&[15.0]) as i32, 1);
    }
}
