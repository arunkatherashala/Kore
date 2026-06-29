//! In-memory model registry for kore-api.

use std::collections::HashMap;
use kore_ml2::{GradientBoostingRegressor, RandomForestClassifier, RandomForestRegressor};
use kore_ml3::{KNearestNeighbors, LinearRegressor, LinearSVM, LogisticRegressor};
use serde_json::{json, Value};

pub enum ModelEntry {
    RfReg(RandomForestRegressor),
    RfClf(RandomForestClassifier),
    Gbm(GradientBoostingRegressor),
    LinReg(LinearRegressor),
    Logistic(LogisticRegressor),
    KnnReg(KNearestNeighbors),
    KnnClf(KNearestNeighbors),
    Svm(LinearSVM),
}

impl ModelEntry {
    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<f64> {
        match self {
            ModelEntry::RfReg(m)   => m.predict_raw(x),
            ModelEntry::RfClf(m)   => m.predict_raw(x),
            ModelEntry::Gbm(m)     => m.predict_raw(x),
            ModelEntry::LinReg(m)  => m.predict_raw(x),
            ModelEntry::Logistic(m)=> m.predict_raw(x),
            ModelEntry::KnnReg(m)  => m.predict_raw(x),
            ModelEntry::KnnClf(m)  => m.predict_raw(x),
            ModelEntry::Svm(m)     => m.predict_raw(x),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            ModelEntry::RfReg(_)   => "rf_reg",
            ModelEntry::RfClf(_)   => "rf_clf",
            ModelEntry::Gbm(_)     => "gbm",
            ModelEntry::LinReg(_)  => "linreg",
            ModelEntry::Logistic(_)=> "logistic",
            ModelEntry::KnnReg(_)  => "knn_reg",
            ModelEntry::KnnClf(_)  => "knn_clf",
            ModelEntry::Svm(_)     => "svm",
        }
    }
}

#[derive(Default)]
pub struct ModelRegistry {
    models:  HashMap<usize, ModelEntry>,
    next_id: usize,
}

impl ModelRegistry {
    pub fn insert(&mut self, _type_name: String, entry: ModelEntry) -> usize {
        let id = self.next_id;
        self.models.insert(id, entry);
        self.next_id += 1;
        id
    }

    pub fn predict(&self, id: usize, x: &[Vec<f64>]) -> Result<Vec<f64>, String> {
        let m = self.models.get(&id)
            .ok_or_else(|| format!("model {id} not found"))?;
        Ok(m.predict(x))
    }

    pub fn remove(&mut self, id: usize) -> Result<(), String> {
        self.models.remove(&id)
            .ok_or_else(|| format!("model {id} not found"))?;
        Ok(())
    }

    pub fn list(&self) -> Vec<Value> {
        let mut v: Vec<_> = self.models.iter()
            .map(|(id, m)| json!({ "id": id, "type": m.type_name() }))
            .collect();
        v.sort_by_key(|x| x["id"].as_u64().unwrap_or(0));
        v
    }
}
