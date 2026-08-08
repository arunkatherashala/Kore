//! Shared result type for world solver branches.

#[derive(Debug, Clone)]
pub struct WorldAnswer {
    pub method: String,
    pub answer: String,
    pub confidence: f64,
}

impl WorldAnswer {
    pub fn new(method: &str, answer: impl Into<String>, confidence: f64) -> Self {
        Self {
            method: method.to_string(),
            answer: answer.into(),
            confidence,
        }
    }
}
