use crate::{DataBlock, KoreError};

/// Stateful data transformation stage (fit then transform).
pub trait Transformer: Send + Sync {
    fn name(&self) -> &str;
    fn fit(&mut self, data: &DataBlock) -> Result<(), KoreError>;
    fn transform(&self, data: &DataBlock) -> Result<DataBlock, KoreError>;
    fn fit_transform(&mut self, data: &DataBlock) -> Result<DataBlock, KoreError> {
        self.fit(data)?;
        self.transform(data)
    }
}

/// Supervised ML estimator: fit on labelled data, predict on new data.
pub trait Estimator: Send + Sync {
    fn name(&self) -> &str;
    fn fit(&mut self, data: &DataBlock, target_col: &str) -> Result<(), KoreError>;
    fn predict(&self, data: &DataBlock) -> Result<Vec<f64>, KoreError>;
}
