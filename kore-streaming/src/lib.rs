//! Kore Streaming - Append-only, ACID Transactions, CDC for Kore Format
//!
//! Provides production-grade streaming capabilities:
//! - Append-only mode for immutable data
//! - ACID transactions with snapshot isolation
//! - Change Data Capture (CDC) format
//! - Kafka integration for distributed streaming

pub mod append_only;
pub mod acid;
pub mod cdc;
pub mod transaction;
pub mod error;

pub use append_only::{AppendOnlyWriter, AppendOnlyReader};
pub use acid::{AcidWriter, AcidReader};
pub use cdc::{ChangeType, ChangeRecord, CDCStream};
pub use transaction::{Transaction, TransactionId, TransactionState};
pub use error::{StreamingError, Result};

/// Streaming mode enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingMode {
    /// Append-only mode: new data appended without overwrites
    AppendOnly,
    /// ACID mode: transactions with snapshot isolation
    Acid,
    /// CDC mode: capture and stream changes
    CDC,
}

/// Streaming configuration
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    pub mode: StreamingMode,
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub max_transaction_size: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            mode: StreamingMode::AppendOnly,
            batch_size: 1024,
            flush_interval_ms: 1000,
            max_transaction_size: 100_000_000, // 100MB
        }
    }
}

impl StreamingConfig {
    /// Create config for append-only mode
    pub fn append_only() -> Self {
        Self {
            mode: StreamingMode::AppendOnly,
            ..Default::default()
        }
    }

    /// Create config for ACID mode
    pub fn acid() -> Self {
        Self {
            mode: StreamingMode::Acid,
            batch_size: 512, // Smaller batches for transactions
            ..Default::default()
        }
    }

    /// Create config for CDC mode
    pub fn cdc() -> Self {
        Self {
            mode: StreamingMode::CDC,
            batch_size: 256, // Small batches for real-time updates
            flush_interval_ms: 100, // Fast flush for CDC
            ..Default::default()
        }
    }
}

/// Initialize streaming infrastructure
pub async fn init_streaming(config: StreamingConfig) -> Result<()> {
    log::info!("Initializing Kore streaming: mode={:?}", config.mode);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_config_default() {
        let config = StreamingConfig::default();
        assert_eq!(config.mode, StreamingMode::AppendOnly);
        assert_eq!(config.batch_size, 1024);
    }

    #[test]
    fn test_streaming_config_append_only() {
        let config = StreamingConfig::append_only();
        assert_eq!(config.mode, StreamingMode::AppendOnly);
    }

    #[test]
    fn test_streaming_config_acid() {
        let config = StreamingConfig::acid();
        assert_eq!(config.mode, StreamingMode::Acid);
        assert_eq!(config.batch_size, 512);
    }

    #[test]
    fn test_streaming_config_cdc() {
        let config = StreamingConfig::cdc();
        assert_eq!(config.mode, StreamingMode::CDC);
        assert_eq!(config.batch_size, 256);
        assert_eq!(config.flush_interval_ms, 100);
    }
}
