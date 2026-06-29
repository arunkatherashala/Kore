use thiserror::Error;

#[derive(Debug, Error)]
pub enum KoreError {
    #[error("column not found: {0}")]
    ColumnNotFound(String),

    #[error("schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("type mismatch: expected {expected}, got {got}")]
    TypeMismatch { expected: String, got: String },

    #[error("index out of bounds: {0}")]
    IndexOutOfBounds(usize),

    #[error("model not fitted")]
    NotFitted,

    #[error("empty dataset")]
    EmptyDataset,

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("cluster error: {0}")]
    Cluster(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}
