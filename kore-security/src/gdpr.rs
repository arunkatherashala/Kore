//! GDPR compliance and personal data management

use crate::error::{Result, SecurityError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Data subject (person)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSubject {
    /// Unique identifier
    pub id: String,
    /// Email address
    pub email: String,
    /// Name
    pub name: String,
    /// GDPR consent status
    pub consent_given: bool,
    /// Consent date
    pub consent_date: Option<DateTime<Utc>>,
}

impl DataSubject {
    /// Create new data subject
    pub fn new(email: String, name: String) -> Self {
        DataSubject {
            id: Uuid::new_v4().to_string(),
            email,
            name,
            consent_given: false,
            consent_date: None,
        }
    }

    /// Give GDPR consent
    pub fn give_consent(&mut self) {
        self.consent_given = true;
        self.consent_date = Some(Utc::now());
    }

    /// Revoke GDPR consent
    pub fn revoke_consent(&mut self) {
        self.consent_given = false;
    }
}

/// Personal data record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalDataRecord {
    /// Record ID
    pub id: String,
    /// Data subject ID
    pub subject_id: String,
    /// Data type
    pub data_type: String,
    /// Data (encrypted when stored)
    pub data: Vec<u8>,
    /// Created date
    pub created_at: DateTime<Utc>,
    /// Last accessed date
    pub last_accessed: DateTime<Utc>,
    /// Purpose of processing
    pub purpose: String,
    /// Retention until date
    pub retention_until: DateTime<Utc>,
}

impl PersonalDataRecord {
    /// Create new personal data record
    pub fn new(
        subject_id: String,
        data_type: String,
        data: Vec<u8>,
        purpose: String,
        retention_days: u32,
    ) -> Self {
        let now = Utc::now();
        PersonalDataRecord {
            id: Uuid::new_v4().to_string(),
            subject_id,
            data_type,
            data,
            created_at: now,
            last_accessed: now,
            purpose,
            retention_until: now + chrono::Duration::days(retention_days as i64),
        }
    }

    /// Check if retention period has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.retention_until
    }

    /// Mark as accessed
    pub fn mark_accessed(&mut self) {
        self.last_accessed = Utc::now();
    }
}

/// GDPR compliance trait
#[async_trait]
pub trait GdprCompliance: Send + Sync {
    /// Register data subject
    async fn register_subject(&self, subject: DataSubject) -> Result<String>;

    /// Get data subject consent status
    async fn get_consent(&self, subject_id: &str) -> Result<bool>;

    /// Give consent
    async fn give_consent(&self, subject_id: &str) -> Result<()>;

    /// Revoke consent
    async fn revoke_consent(&self, subject_id: &str) -> Result<()>;

    /// Store personal data
    async fn store_data(&self, record: PersonalDataRecord) -> Result<String>;

    /// Retrieve personal data (right to access)
    async fn get_data(&self, subject_id: &str) -> Result<Vec<PersonalDataRecord>>;

    /// Delete personal data (right to erasure)
    async fn delete_data(&self, subject_id: &str) -> Result<usize>;

    /// Cleanup expired records (data retention)
    async fn cleanup_expired_data(&self) -> Result<usize>;

    /// Export data for subject (right to portability)
    async fn export_data(&self, subject_id: &str) -> Result<String>;

    /// Get data processing records
    async fn get_processing_records(&self, subject_id: &str) -> Result<Vec<PersonalDataRecord>>;
}

/// In-memory GDPR compliance store
pub struct PersonalDataStore {
    subjects: Arc<parking_lot::Mutex<std::collections::HashMap<String, DataSubject>>>,
    records: Arc<parking_lot::Mutex<Vec<PersonalDataRecord>>>,
}

impl PersonalDataStore {
    /// Create new store
    pub fn new() -> Self {
        PersonalDataStore {
            subjects: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
            records: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    /// Get subject count
    pub fn subject_count(&self) -> usize {
        self.subjects.lock().len()
    }

    /// Get record count
    pub fn record_count(&self) -> usize {
        self.records.lock().len()
    }
}

impl Default for PersonalDataStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GdprCompliance for PersonalDataStore {
    async fn register_subject(&self, subject: DataSubject) -> Result<String> {
        let id = subject.id.clone();
        self.subjects.lock().insert(id.clone(), subject);
        Ok(id)
    }

    async fn get_consent(&self, subject_id: &str) -> Result<bool> {
        let subjects = self.subjects.lock();
        subjects
            .get(subject_id)
            .map(|s| s.consent_given)
            .ok_or_else(|| SecurityError::GdprViolation("Subject not found".to_string()))
    }

    async fn give_consent(&self, subject_id: &str) -> Result<()> {
        let mut subjects = self.subjects.lock();
        subjects
            .get_mut(subject_id)
            .ok_or_else(|| SecurityError::GdprViolation("Subject not found".to_string()))?
            .give_consent();
        Ok(())
    }

    async fn revoke_consent(&self, subject_id: &str) -> Result<()> {
        let mut subjects = self.subjects.lock();
        subjects
            .get_mut(subject_id)
            .ok_or_else(|| SecurityError::GdprViolation("Subject not found".to_string()))?
            .revoke_consent();
        Ok(())
    }

    async fn store_data(&self, record: PersonalDataRecord) -> Result<String> {
        // Check consent
        let subjects = self.subjects.lock();
        let subject = subjects
            .get(&record.subject_id)
            .ok_or_else(|| SecurityError::GdprViolation("Subject not found".to_string()))?;

        if !subject.consent_given {
            return Err(SecurityError::GdprViolation(
                "No consent for data processing".to_string(),
            ));
        }

        drop(subjects);

        let id = record.id.clone();
        self.records.lock().push(record);
        Ok(id)
    }

    async fn get_data(&self, subject_id: &str) -> Result<Vec<PersonalDataRecord>> {
        Ok(self
            .records
            .lock()
            .iter()
            .filter(|r| r.subject_id == subject_id)
            .cloned()
            .collect())
    }

    async fn delete_data(&self, subject_id: &str) -> Result<usize> {
        let mut records = self.records.lock();
        let before_count = records.len();
        records.retain(|r| r.subject_id != subject_id);
        Ok(before_count - records.len())
    }

    async fn cleanup_expired_data(&self) -> Result<usize> {
        let mut records = self.records.lock();
        let before_count = records.len();
        records.retain(|r| !r.is_expired());
        Ok(before_count - records.len())
    }

    async fn export_data(&self, subject_id: &str) -> Result<String> {
        let records = self.get_data(subject_id).await?;
        serde_json::to_string(&records)
            .map_err(|e| SecurityError::SerializationError(e))
    }

    async fn get_processing_records(&self, subject_id: &str) -> Result<Vec<PersonalDataRecord>> {
        self.get_data(subject_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_subject_creation() {
        let subject = DataSubject::new("user@example.com".to_string(), "John Doe".to_string());
        assert_eq!(subject.email, "user@example.com");
        assert!(!subject.consent_given);
    }

    #[test]
    fn test_data_subject_consent() {
        let mut subject = DataSubject::new("user@example.com".to_string(), "John Doe".to_string());
        subject.give_consent();
        assert!(subject.consent_given);
        assert!(subject.consent_date.is_some());

        subject.revoke_consent();
        assert!(!subject.consent_given);
    }

    #[tokio::test]
    async fn test_personal_data_store() {
        let store = PersonalDataStore::new();

        let subject = DataSubject::new("user@example.com".to_string(), "John Doe".to_string());
        store.register_subject(subject).await.unwrap();

        assert_eq!(store.subject_count(), 1);
    }

    #[tokio::test]
    async fn test_gdpr_consent_enforcement() {
        let store = PersonalDataStore::new();

        let mut subject = DataSubject::new("user@example.com".to_string(), "John Doe".to_string());
        let subject_id = subject.id.clone();
        subject.give_consent();

        store.register_subject(subject).await.unwrap();

        let record = PersonalDataRecord::new(
            subject_id,
            "email".to_string(),
            b"user@example.com".to_vec(),
            "newsletter".to_string(),
            30,
        );

        let result = store.store_data(record).await;
        assert!(result.is_ok());
    }
}
