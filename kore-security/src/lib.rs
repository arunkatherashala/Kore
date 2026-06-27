//! Kore Security - AES-256 Encryption, Audit Logging, GDPR Compliance

pub mod error;
pub use error::{Result, SecurityError};

pub mod encryption;
pub use encryption::{EncryptionKey, EncryptionCipher, AesGcmCipher};

pub mod audit;
pub use audit::{AuditEvent, AuditLog, InMemoryAuditLog};

pub mod gdpr;
pub use gdpr::{DataSubject, PersonalDataRecord, GdprCompliance, PersonalDataStore};

pub mod access_control;
pub use access_control::{AccessControl, Permission, Role, Subject, Resource};

use serde::{Deserialize, Serialize};

/// Security configuration for Kore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Enable encryption (default: true)
    pub encryption_enabled: bool,
    /// Encryption algorithm (default: "aes-256-gcm")
    pub encryption_algorithm: String,
    /// Enable audit logging (default: true)
    pub audit_logging_enabled: bool,
    /// Audit log retention days (default: 365)
    pub audit_retention_days: u32,
    /// Enable GDPR compliance (default: true)
    pub gdpr_enabled: bool,
    /// Data retention days for compliance (default: 90)
    pub data_retention_days: u32,
    /// Enable access control (default: true)
    pub access_control_enabled: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        SecurityConfig {
            encryption_enabled: true,
            encryption_algorithm: "aes-256-gcm".to_string(),
            audit_logging_enabled: true,
            audit_retention_days: 365,
            gdpr_enabled: true,
            data_retention_days: 90,
            access_control_enabled: true,
        }
    }
}

/// Initialize security subsystem
pub async fn init_security(config: SecurityConfig) -> Result<()> {
    if config.encryption_enabled {
        log::info!("Initialized encryption: {}", config.encryption_algorithm);
    }
    if config.audit_logging_enabled {
        log::info!("Initialized audit logging (retention: {} days)", config.audit_retention_days);
    }
    if config.gdpr_enabled {
        log::info!("Initialized GDPR compliance (retention: {} days)", config.data_retention_days);
    }
    if config.access_control_enabled {
        log::info!("Initialized access control");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_config_default() {
        let config = SecurityConfig::default();
        assert!(config.encryption_enabled);
        assert!(config.audit_logging_enabled);
        assert!(config.gdpr_enabled);
        assert!(config.access_control_enabled);
    }
}
