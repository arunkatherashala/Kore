//! Error types for streaming operations

use thiserror::Error;

/// Result type for streaming operations
pub type Result<T> = std::result::Result<T, StreamingError>;

/// Streaming operation errors
#[derive(Debug, Error)]
pub enum StreamingError {
    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Write error: {0}")]
    WriteError(String),

    #[error("Read error: {0}")]
    ReadError(String),

    #[error("Conflict: {0}")]
    ConflictError(String),

    #[error("Timeout: {0}")]
    TimeoutError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    #[error("Other error: {0}")]
    Other(String),
}

impl StreamingError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            StreamingError::TimeoutError(_)
                | StreamingError::ConflictError(_)
                | StreamingError::ResourceExhausted(_)
        )
    }

    /// Check if error is fatal
    pub fn is_fatal(&self) -> bool {
        !self.is_retryable()
    }
}
