//! Kore Observability - Prometheus Metrics + OpenTelemetry Tracing
//!
//! Provides comprehensive observability for Kore operations including:
//! - Query execution metrics
//! - Compression efficiency tracking
//! - Cache hit rates
//! - Distributed tracing with Jaeger
//! - Performance profiling

pub mod metrics;
pub mod tracing;
pub mod instrumentation;

pub use metrics::{KoreMetrics, MetricsRegistry};
pub use tracing::{TracingConfig, init_tracing};
pub use instrumentation::{Instrumented, instrument};

/// Initialize all observability infrastructure
pub async fn init_observability() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing (with Jaeger if configured)
    #[cfg(feature = "jaeger")]
    tracing::init_jaeger().await?;

    // Initialize metrics (Prometheus)
    #[cfg(feature = "prometheus")]
    metrics::init_prometheus()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observability_module_loads() {
        // Verify module compiles and loads
        assert!(true);
    }
}
