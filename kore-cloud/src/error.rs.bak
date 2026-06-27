//! Error types for cloud operations

use thiserror::Error;

/// Result type for cloud operations
pub type Result<T> = std::result::Result<T, CloudError>;

/// Cloud operation errors
#[derive(Debug, Error)]
pub enum CloudError {
    #[error("S3 error: {0}")]
    S3Error(String),

    #[error("GCS error: {0}")]
    GCSError(String),

    #[error("Azure error: {0}")]
    AzureError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Invalid range: {0}")]
    InvalidRange(String),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Access denied: {0}")]
    AccessDenied(String),

    #[error("Feature not enabled: {0}")]
    FeatureNotEnabled(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl CloudError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            CloudError::NetworkError(_) => true,
            CloudError::S3Error(s) => s.contains("ServiceUnavailable"),
            CloudError::GCSError(s) => s.contains("503"),
            CloudError::AzureError(s) => s.contains("throttled"),
            _ => false,
        }
    }
}
