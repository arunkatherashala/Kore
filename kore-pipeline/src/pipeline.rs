//! Pipeline — chain Transformers then an Estimator (fit → transform → predict).

use kore_core::{DataBlock, Estimator, KoreError, Transformer};

pub struct Pipeline {
    transformers: Vec<Box<dyn Transformer>>,
    estimator:    Option<Box<dyn Estimator>>,
    target_col:   String,
}

impl Pipeline {
    pub fn new(target_col: &str) -> Self {
        Self { transformers: vec![], estimator: None, target_col: target_col.into() }
    }

    pub fn add_transformer(&mut self, t: Box<dyn Transformer>) -> &mut Self {
        self.transformers.push(t);
        self
    }

    pub fn set_estimator(&mut self, e: Box<dyn Estimator>) -> &mut Self {
        self.estimator = Some(e);
        self
    }

    /// Fit all transformers sequentially, then fit the estimator.
    pub fn fit(&mut self, data: &DataBlock) -> Result<(), KoreError> {
        let mut current = data.clone();
        for t in &mut self.transformers {
            t.fit(&current)?;
            current = t.transform(&current)?;
        }
        if let Some(est) = &mut self.estimator {
            est.fit(&current, &self.target_col)?;
        }
        Ok(())
    }

    /// Apply all transformers (no refitting).
    pub fn transform(&self, data: &DataBlock) -> Result<DataBlock, KoreError> {
        let mut current = data.clone();
        for t in &self.transformers {
            current = t.transform(&current)?;
        }
        Ok(current)
    }

    /// Transform then predict with the estimator.
    pub fn predict(&self, data: &DataBlock) -> Result<Vec<f64>, KoreError> {
        let transformed = self.transform(data)?;
        self.estimator
            .as_ref()
            .ok_or(KoreError::NotFitted)?
            .predict(&transformed)
    }

    /// Names of all stages for introspection.
    pub fn stage_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.transformers.iter().map(|t| t.name()).collect();
        if let Some(e) = &self.estimator { names.push(e.name()); }
        names
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock};
    use kore_ml2::GradientBoostingRegressor;
    use crate::transformer::StandardScaler;

    fn make_data(n: usize) -> DataBlock {
        DataBlock::new(vec![
            Column::float64("x1", (0..n).map(|i| Some(i as f64)).collect()),
            Column::float64("x2", (0..n).map(|i| Some(i as f64 * 0.5)).collect()),
            Column::float64("y",  (0..n).map(|i| Some(i as f64 * 2.0 + 1.0)).collect()),
        ]).unwrap()
    }

    #[test]
    fn pipeline_fit_predict() {
        let data = make_data(50);
        let mut pipe = Pipeline::new("y");
        pipe.add_transformer(Box::new(StandardScaler::all_numeric()));
        pipe.set_estimator(Box::new(GradientBoostingRegressor::new(30, 0.1, 3)));
        pipe.fit(&data).unwrap();
        let preds = pipe.predict(&data).unwrap();
        assert_eq!(preds.len(), 50);
        // Rough sanity: predictions near the target range [1, 99]
        assert!(preds[0] > -50.0 && preds[49] < 200.0);
    }

    #[test]
    fn stage_names() {
        let mut pipe = Pipeline::new("target");
        pipe.add_transformer(Box::new(StandardScaler::all_numeric()));
        pipe.set_estimator(Box::new(GradientBoostingRegressor::new(10, 0.1, 2)));
        assert_eq!(pipe.stage_names(), &["StandardScaler", "GradientBoostingRegressor"]);
    }
}
