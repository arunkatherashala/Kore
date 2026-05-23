//! Audit logging system

use crate::error::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Audit event types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventType {
    /// Authentication event
    Authentication,
    /// Authorization event
    Authorization,
    /// Data read
    DataRead,
    /// Data write
    DataWrite,
    /// Data delete
    DataDelete,
    /// Encryption operation
    Encryption,
    /// Access control change
    AccessControlChange,
    /// Configuration change
    ConfigChange,
    /// Error/security issue
    SecurityEvent,
}

impl std::fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditEventType::Authentication => write!(f, "AUTH"),
            AuditEventType::Authorization => write!(f, "AUTHZ"),
            AuditEventType::DataRead => write!(f, "READ"),
            AuditEventType::DataWrite => write!(f, "WRITE"),
            AuditEventType::DataDelete => write!(f, "DELETE"),
            AuditEventType::Encryption => write!(f, "ENCRYPT"),
            AuditEventType::AccessControlChange => write!(f, "ACL_CHANGE"),
            AuditEventType::ConfigChange => write!(f, "CONFIG_CHANGE"),
            AuditEventType::SecurityEvent => write!(f, "SECURITY"),
        }
    }
}

/// Single audit event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event ID
    pub id: String,
    /// Event timestamp
    pub timestamp: DateTime<Utc>,
    /// Event type
    pub event_type: AuditEventType,
    /// Subject performing action
    pub subject: String,
    /// Resource being accessed
    pub resource: String,
    /// Action performed
    pub action: String,
    /// Result (success/failure)
    pub result: bool,
    /// Additional details
    pub details: serde_json::Value,
}

impl AuditEvent {
    /// Create new audit event
    pub fn new(
        event_type: AuditEventType,
        subject: String,
        resource: String,
        action: String,
        result: bool,
    ) -> Self {
        AuditEvent {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type,
            subject,
            resource,
            action,
            result,
            details: serde_json::json!({}),
        }
    }

    /// Add detail to event
    pub fn with_detail(mut self, key: &str, value: serde_json::Value) -> Self {
        if let serde_json::Value::Object(ref mut obj) = self.details {
            obj.insert(key.to_string(), value);
        }
        self
    }
}

/// Audit log trait
#[async_trait]
pub trait AuditLog: Send + Sync {
    /// Log an audit event
    async fn log(&self, event: AuditEvent) -> Result<()>;

    /// Log multiple events
    async fn log_batch(&self, events: Vec<AuditEvent>) -> Result<()>;

    /// Retrieve events by subject
    async fn get_events_by_subject(&self, subject: &str) -> Result<Vec<AuditEvent>>;

    /// Retrieve events by resource
    async fn get_events_by_resource(&self, resource: &str) -> Result<Vec<AuditEvent>>;

    /// Retrieve events by type
    async fn get_events_by_type(&self, event_type: AuditEventType) -> Result<Vec<AuditEvent>>;

    /// Get failed events (security issues)
    async fn get_failed_events(&self) -> Result<Vec<AuditEvent>>;

    /// Get all events
    async fn get_all_events(&self) -> Result<Vec<AuditEvent>>;

    /// Clear old events (retention policy)
    async fn clear_old_events(&self, retention_days: u32) -> Result<usize>;
}

/// In-memory audit log implementation
pub struct InMemoryAuditLog {
    events: Arc<parking_lot::Mutex<Vec<AuditEvent>>>,
}

impl InMemoryAuditLog {
    /// Create new audit log
    pub fn new() -> Self {
        InMemoryAuditLog {
            events: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    /// Get event count
    pub fn event_count(&self) -> usize {
        self.events.lock().len()
    }
}

impl Default for InMemoryAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AuditLog for InMemoryAuditLog {
    async fn log(&self, event: AuditEvent) -> Result<()> {
        self.events.lock().push(event);
        Ok(())
    }

    async fn log_batch(&self, events: Vec<AuditEvent>) -> Result<()> {
        let mut log = self.events.lock();
        log.extend(events);
        Ok(())
    }

    async fn get_events_by_subject(&self, subject: &str) -> Result<Vec<AuditEvent>> {
        let log = self.events.lock();
        Ok(log
            .iter()
            .filter(|e| e.subject == subject)
            .cloned()
            .collect())
    }

    async fn get_events_by_resource(&self, resource: &str) -> Result<Vec<AuditEvent>> {
        let log = self.events.lock();
        Ok(log
            .iter()
            .filter(|e| e.resource == resource)
            .cloned()
            .collect())
    }

    async fn get_events_by_type(&self, event_type: AuditEventType) -> Result<Vec<AuditEvent>> {
        let log = self.events.lock();
        Ok(log
            .iter()
            .filter(|e| e.event_type == event_type)
            .cloned()
            .collect())
    }

    async fn get_failed_events(&self) -> Result<Vec<AuditEvent>> {
        let log = self.events.lock();
        Ok(log
            .iter()
            .filter(|e| !e.result)
            .cloned()
            .collect())
    }

    async fn get_all_events(&self) -> Result<Vec<AuditEvent>> {
        Ok(self.events.lock().clone())
    }

    async fn clear_old_events(&self, retention_days: u32) -> Result<usize> {
        let mut log = self.events.lock();
        let before_count = log.len();

        let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
        log.retain(|e| e.timestamp > cutoff);

        Ok(before_count - log.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_creation() {
        let event = AuditEvent::new(
            AuditEventType::DataRead,
            "user_123".to_string(),
            "file_456".to_string(),
            "READ".to_string(),
            true,
        );

        assert_eq!(event.subject, "user_123");
        assert_eq!(event.resource, "file_456");
        assert_eq!(event.event_type, AuditEventType::DataRead);
        assert!(event.result);
    }

    #[tokio::test]
    async fn test_audit_log_basic() {
        let log = InMemoryAuditLog::new();

        let event = AuditEvent::new(
            AuditEventType::DataRead,
            "user_1".to_string(),
            "file_1".to_string(),
            "READ".to_string(),
            true,
        );

        log.log(event).await.unwrap();
        assert_eq!(log.event_count(), 1);
    }

    #[tokio::test]
    async fn test_audit_log_batch() {
        let log = InMemoryAuditLog::new();

        let events = vec![
            AuditEvent::new(
                AuditEventType::DataWrite,
                "user_1".to_string(),
                "file_1".to_string(),
                "WRITE".to_string(),
                true,
            ),
            AuditEvent::new(
                AuditEventType::DataRead,
                "user_2".to_string(),
                "file_2".to_string(),
                "READ".to_string(),
                false,
            ),
        ];

        log.log_batch(events).await.unwrap();
        assert_eq!(log.event_count(), 2);
    }

    #[tokio::test]
    async fn test_audit_log_query() {
        let log = InMemoryAuditLog::new();

        log.log(AuditEvent::new(
            AuditEventType::DataRead,
            "user_1".to_string(),
            "file_1".to_string(),
            "READ".to_string(),
            true,
        ))
        .await
        .unwrap();

        let events = log.get_events_by_subject("user_1").await.unwrap();
        assert_eq!(events.len(), 1);
    }
}
