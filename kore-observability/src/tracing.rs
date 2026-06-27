//! OpenTelemetry and Jaeger tracing integration

use opentelemetry::global;
use std::str::FromStr;
use tracing::{Level, Subscriber};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{
    fmt, prelude::*, registry, EnvFilter, Layer, Registry,
};

/// Tracing configuration
#[derive(Debug, Clone)]
pub struct TracingConfig {
    pub service_name: String,
    pub jaeger_endpoint: Option<String>,
    pub log_level: String,
    pub enable_console_output: bool,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            service_name: "kore".to_string(),
            jaeger_endpoint: Some("http://localhost:14268/api/traces".to_string()),
            log_level: "info".to_string(),
            enable_console_output: true,
        }
    }
}

/// Initialize Jaeger tracing
#[cfg(feature = "jaeger")]
pub async fn init_jaeger() -> Result<(), Box<dyn std::error::Error>> {
    let config = TracingConfig::default();

    let tracer = opentelemetry_jaeger::new_agent_pipeline()
        .with_service_name(&config.service_name)
        .with_max_packet_size(4096)
        .with_max_buffer_size(512)
        .install_simple()?;

    let telemetry = OpenTelemetryLayer::new(tracer);

    let log_level = Level::from_str(&config.log_level).unwrap_or(Level::INFO);
    let env_filter = EnvFilter::new(format!(
        "kore={},opentelemetry={},hyper={}",
        config.log_level, "debug", "info"
    ));

    let registry = Registry::default()
        .with(env_filter)
        .with(telemetry);

    if config.enable_console_output {
        let fmt_layer = fmt::layer()
            .with_writer(std::io::stdout)
            .with_level(true)
            .with_target(true)
            .with_thread_ids(true);

        tracing::subscriber::set_default(registry.with(fmt_layer));
    } else {
        tracing::subscriber::set_default(registry);
    }

    Ok(())
}

/// Initialize simple console tracing (without Jaeger)
#[cfg(not(feature = "jaeger"))]
pub fn init_console_tracing(config: TracingConfig) -> Result<(), Box<dyn std::error::Error>> {
    let log_level = Level::from_str(&config.log_level).unwrap_or(Level::INFO);
    let env_filter = EnvFilter::new(format!("kore={},hyper=info", config.log_level));

    let registry = Registry::default()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_writer(std::io::stdout)
                .with_level(true)
                .with_target(true)
                .with_thread_ids(true),
        );

    tracing::subscriber::set_default(registry);

    Ok(())
}

/// Initialize all tracing
pub fn init_tracing() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "jaeger")]
    {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(init_jaeger())?;
    }

    #[cfg(not(feature = "jaeger"))]
    {
        init_console_tracing(TracingConfig::default())?;
    }

    Ok(())
}

/// Trace helper for measuring operation duration
#[macro_export]
macro_rules! trace_operation {
    ($name:expr, $body:block) => {{
        let span = tracing::info_span!($name);
        let _guard = span.enter();
        let start = std::time::Instant::now();
        let result = $body;
        let duration = start.elapsed();
        tracing::info!(duration_ms = duration.as_millis(), "Operation completed");
        result
    }};
}

/// Async trace helper
#[macro_export]
macro_rules! trace_async_operation {
    ($name:expr, $future:expr) => {{
        let span = tracing::info_span!($name);
        let _guard = span.enter();
        let start = std::time::Instant::now();
        let result = $future.await;
        let duration = start.elapsed();
        tracing::info!(duration_ms = duration.as_millis(), "Async operation completed");
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_config_default() {
        let config = TracingConfig::default();
        assert_eq!(config.service_name, "kore");
        assert!(config.jaeger_endpoint.is_some());
    }

    #[test]
    fn test_tracing_config_custom() {
        let config = TracingConfig {
            service_name: "kore-custom".to_string(),
            jaeger_endpoint: Some("http://jaeger:14268".to_string()),
            log_level: "debug".to_string(),
            enable_console_output: false,
        };
        assert_eq!(config.service_name, "kore-custom");
        assert_eq!(config.log_level, "debug");
    }
}
