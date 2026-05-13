//! Cloud connector error types

use thiserror::Error;

/// Result type for cloud operations
pub type CloudResult<T> = Result<T, CloudError>;

/// Cloud connector errors
#[derive(Error, Debug)]
pub enum CloudError {
    #[error("AWS S3 error: {0}")]
    S3Error(String),

    #[error("Authentication failed: {0}")]
    AuthenticationError(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::get_object::GetObjectError>> for CloudError {
    fn from(err: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::get_object::GetObjectError>) -> Self {
        CloudError::S3Error(err.to_string())
    }
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::put_object::PutObjectError>> for CloudError {
    fn from(err: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::put_object::PutObjectError>) -> Self {
        CloudError::S3Error(err.to_string())
    }
}
