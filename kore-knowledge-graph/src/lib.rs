//! KORE Knowledge Graph — Phase 6A
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Triple {
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

pub struct KnowledgeGraph {
    triples: Vec<Triple>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        KnowledgeGraph { triples: Vec::new() }
    }

    pub fn add_triple(&mut self, triple: Triple) {
        self.triples.push(triple);
    }

    pub fn query(&self, subject: &str) -> Vec<Triple> {
        self.triples
            .iter()
            .filter(|t| t.subject == subject)
            .cloned()
            .collect()
    }
}
