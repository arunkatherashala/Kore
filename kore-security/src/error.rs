//! Security error types

use thiserror::Error;

/// Result type for security operations
pub type Result<T> = std::result::Result<T, SecurityError>;

/// Security operation errors
#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Decryption error: {0}")]
    DecryptionError(String),

    #[error("Key derivation error: {0}")]
    KeyDerivationError(String),

    #[error("Invalid key: {0}")]
    InvalidKey(String),

    #[error("Audit error: {0}")]
    AuditError(String),

    #[error("GDPR violation: {0}")]
    GdprViolation(String),

    #[error("Access denied: {0}")]
    AccessDenied(String),

    #[error("Invalid permission: {0}")]
    InvalidPermission(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl SecurityError {
    /// Check if error should be retried
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SecurityError::KeyDerivationError(_)
                | SecurityError::EncryptionError(_)
                | SecurityError::IoError(_)
        )
    }

    /// Check if error is fatal
    pub fn is_fatal(&self) -> bool {
        !self.is_retryable()
    }
}
