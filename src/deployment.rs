/// Production deployment configuration and health monitoring
///
/// Provides:
/// - Service configuration
/// - Health checks
/// - Graceful shutdown
/// - Logging configuration

use std::time::SystemTime;

/// Service configuration
#[derive(Clone, Debug)]
pub struct ServiceConfig {
    pub service_name: String,
    pub version: String,
    pub port: u16,
    pub host: String,
    pub max_connections: usize,
    pub request_timeout_secs: u64,
    pub enable_metrics: bool,
    pub log_level: LogLevel,
}

impl ServiceConfig {
    pub fn production() -> Self {
        Self {
            service_name: "kore-query-engine".to_string(),
            version: "0.3.0".to_string(),
            port: 8080,
            host: "0.0.0.0".to_string(),
            max_connections: 1000,
            request_timeout_secs: 30,
            enable_metrics: true,
            log_level: LogLevel::Info,
        }
    }

    pub fn development() -> Self {
        Self {
            service_name: "kore-query-engine-dev".to_string(),
            version: "0.3.0".to_string(),
            port: 3000,
            host: "127.0.0.1".to_string(),
            max_connections: 100,
            request_timeout_secs: 60,
            enable_metrics: true,
            log_level: LogLevel::Debug,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_host(mut self, host: &str) -> Self {
        self.host = host.to_string();
        self
    }
}

/// Log level
#[derive(Clone, Debug, PartialEq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// Service health status
#[derive(Clone, Debug, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthStatus {
    pub fn as_str(&self) -> &str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Unhealthy => "unhealthy",
        }
    }
}

/// Health check response
#[derive(Clone, Debug)]
pub struct HealthCheck {
    pub status: HealthStatus,
    pub uptime_secs: u64,
    pub queries_processed: u64,
    pub errors_count: u64,
    pub memory_mb: f64,
    pub timestamp: u64,
}

impl HealthCheck {
    pub fn new(
        status: HealthStatus,
        uptime: u64,
        queries: u64,
        errors: u64,
        memory: f64,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            status,
            uptime_secs: uptime,
            queries_processed: queries,
            errors_count: errors,
            memory_mb: memory,
            timestamp: now,
        }
    }

    pub fn calculate_health(
        &self,
    ) -> HealthStatus {
        let error_rate = if self.queries_processed > 0 {
            (self.errors_count as f64) / (self.queries_processed as f64)
        } else {
            0.0
        };

        if error_rate > 0.1 || self.memory_mb > 4000.0 {
            HealthStatus::Unhealthy
        } else if error_rate > 0.05 || self.memory_mb > 3000.0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        }
    }
}

/// Service metrics
#[derive(Clone, Debug)]
pub struct ServiceMetrics {
    pub total_queries: u64,
    pub successful_queries: u64,
    pub failed_queries: u64,
    pub avg_query_time_ms: f64,
    pub p95_query_time_ms: f64,
    pub p99_query_time_ms: f64,
    pub throughput_qps: f64,
}

impl ServiceMetrics {
    pub fn new() -> Self {
        Self {
            total_queries: 0,
            successful_queries: 0,
            failed_queries: 0,
            avg_query_time_ms: 0.0,
            p95_query_time_ms: 0.0,
            p99_query_time_ms: 0.0,
            throughput_qps: 0.0,
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_queries > 0 {
            (self.successful_queries as f64) / (self.total_queries as f64)
                * 100.0
        } else {
            0.0
        }
    }

    pub fn record_query(&mut self, success: bool, duration_ms: f64) {
        self.total_queries += 1;
        if success {
            self.successful_queries += 1;
        } else {
            self.failed_queries += 1;
        }

        // Simple moving average
        self.avg_query_time_ms =
            (self.avg_query_time_ms * 0.99) + (duration_ms * 0.01);
    }
}

impl Default for ServiceMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Deployment target
#[derive(Clone, Debug, PartialEq)]
pub enum DeploymentTarget {
    Development,
    Staging,
    Production,
}

impl DeploymentTarget {
    pub fn as_str(&self) -> &str {
        match self {
            DeploymentTarget::Development => "development",
            DeploymentTarget::Staging => "staging",
            DeploymentTarget::Production => "production",
        }
    }
}

/// Deployment configuration
#[derive(Clone, Debug)]
pub struct DeploymentConfig {
    pub target: DeploymentTarget,
    pub replicas: usize,
    pub health_check_interval_secs: u64,
    pub graceful_shutdown_timeout_secs: u64,
    pub enable_auto_scaling: bool,
    pub min_instances: usize,
    pub max_instances: usize,
}

impl DeploymentConfig {
    pub fn for_target(target: DeploymentTarget) -> Self {
        match target {
            DeploymentTarget::Development => Self {
                target,
                replicas: 1,
                health_check_interval_secs: 10,
                graceful_shutdown_timeout_secs: 5,
                enable_auto_scaling: false,
                min_instances: 1,
                max_instances: 1,
            },
            DeploymentTarget::Staging => Self {
                target,
                replicas: 2,
                health_check_interval_secs: 5,
                graceful_shutdown_timeout_secs: 10,
                enable_auto_scaling: true,
                min_instances: 2,
                max_instances: 5,
            },
            DeploymentTarget::Production => Self {
                target,
                replicas: 3,
                health_check_interval_secs: 3,
                graceful_shutdown_timeout_secs: 30,
                enable_auto_scaling: true,
                min_instances: 3,
                max_instances: 10,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_config_production() {
        let config = ServiceConfig::production();
        assert_eq!(config.port, 8080);
        assert_eq!(config.max_connections, 1000);
    }

    #[test]
    fn test_service_config_development() {
        let config = ServiceConfig::development();
        assert_eq!(config.port, 3000);
        assert_eq!(config.max_connections, 100);
    }

    #[test]
    fn test_log_level() {
        assert_eq!(LogLevel::Debug.as_str(), "DEBUG");
        assert_eq!(LogLevel::Info.as_str(), "INFO");
    }

    #[test]
    fn test_health_check_healthy() {
        let check =
            HealthCheck::new(HealthStatus::Healthy, 3600, 1000, 5, 500.0);
        let status = check.calculate_health();
        assert_eq!(status, HealthStatus::Healthy);
    }

    #[test]
    fn test_health_check_unhealthy() {
        let check =
            HealthCheck::new(HealthStatus::Healthy, 3600, 100, 50, 5000.0);
        let status = check.calculate_health();
        assert_eq!(status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_service_metrics() {
        let mut metrics = ServiceMetrics::new();
        metrics.record_query(true, 10.0);
        metrics.record_query(true, 20.0);
        metrics.record_query(false, 5.0);

        assert_eq!(metrics.total_queries, 3);
        assert_eq!(metrics.successful_queries, 2);
        assert!(metrics.success_rate() < 100.0);
    }

    #[test]
    fn test_deployment_config_production() {
        let config = DeploymentConfig::for_target(
            DeploymentTarget::Production,
        );
        assert_eq!(config.replicas, 3);
        assert!(config.enable_auto_scaling);
        assert_eq!(config.min_instances, 3);
    }

    #[test]
    fn test_deployment_target_str() {
        assert_eq!(
            DeploymentTarget::Development.as_str(),
            "development"
        );
        assert_eq!(
            DeploymentTarget::Production.as_str(),
            "production"
        );
    }

    #[test]
    fn test_health_status_str() {
        assert_eq!(HealthStatus::Healthy.as_str(), "healthy");
        assert_eq!(
            HealthStatus::Unhealthy.as_str(),
            "unhealthy"
        );
    }
}
