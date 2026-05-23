use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Conversion error: {0}")]
    ConversionError(String),

    #[error("Analysis error: {0}")]
    AnalysisError(String),

    #[error("Unknown error")]
    Unknown,
}

impl CliError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, CliError::IoError(_))
    }
}
