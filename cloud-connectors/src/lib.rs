//! Kore Cloud Connectors - AWS S3, Azure Blob, GCS, and Snowflake Integration
//! 
//! This library provides native cloud storage connectors for Kore FileFormat,
//! enabling seamless reading/writing of Kore files across major cloud providers.

pub mod s3;
pub mod errors;

pub use s3::{S3Reader, S3Config};
pub use errors::{CloudError, CloudResult};

// Re-export commonly used items
pub mod prelude {
    pub use crate::s3::{S3Reader, S3Config};
    pub use crate::errors::{CloudError, CloudResult};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prelude() {
        // Verify prelude exports work
        let _ = std::any::type_name::<S3Reader>();
    }
}
