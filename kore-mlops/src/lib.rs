//! KORE MLOps — Phase 5A: Model Management
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVersion {
    pub id: String,
    pub name: String,
    pub version: String,
    pub created_at: i64,
    pub metrics: HashMap<String, f32>,
}

pub struct ModelRegistry {
    models: HashMap<String, Vec<ModelVersion>>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        ModelRegistry {
            models: HashMap::new(),
        }
    }

    pub fn register(&mut self, model: ModelVersion) {
        self.models
            .entry(model.name.clone())
            .or_insert_with(Vec::new)
            .push(model);
    }

    pub fn get_latest(&self, name: &str) -> Option<ModelVersion> {
        self.models.get(name).and_then(|versions| versions.last().cloned())
    }
}
