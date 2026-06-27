//! GDPR Compliance Example
//!
//! Demonstrates GDPR rights: consent, access, erasure, portability

use kore_security::gdpr::{GdprCompliance, DataSubject, PersonalDataRecord, PersonalDataStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kore GDPR Compliance Example ===\n");

    let store = PersonalDataStore::new();

    // Example 1: Right to be informed (consent)
    println!("Example 1: Right to be Informed & Consent");
    consent_example(&store).await?;

    // Example 2: Right to access
    println!("\nExample 2: Right to Access");
    access_example(&store).await?;

    // Example 3: Right to erasure
    println!("\nExample 3: Right to Erasure");
    erasure_example(&store).await?;

    // Example 4: Right to portability
    println!("\nExample 4: Right to Data Portability");
    portability_example(&store).await?;

    println!("\n✓ GDPR examples completed");
    Ok(())
}

async fn consent_example(store: &PersonalDataStore) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Registering data subject...");

    let mut subject = DataSubject::new(
        "alice@example.com".to_string(),
        "Alice Johnson".to_string(),
    );

    println!("  Subject: {} ({})", subject.name, subject.email);
    println!("  Consent status: {}", subject.consent_given);

    // User gives consent
    println!("  User gives consent to data processing...");
    subject.give_consent();

    let subject_id = store.register_subject(subject).await?;
    println!("  Registered with ID: {}", subject_id);

    // Verify consent
    let has_consent = store.get_consent(&subject_id).await?;
    println!("  Consent verified: {}", has_consent);

    Ok(())
}

async fn access_example(store: &PersonalDataStore) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Demonstrating right to access...");

    // First register and consent
    let mut subject = DataSubject::new(
        "bob@example.com".to_string(),
        "Bob Smith".to_string(),
    );
    subject.give_consent();
    let subject_id = store.register_subject(subject).await?;

    // Store some personal data
    let records_to_store = vec![
        PersonalDataRecord::new(
            subject_id.clone(),
            "email".to_string(),
            b"bob@example.com".to_vec(),
            "account".to_string(),
            365,
        ),
        PersonalDataRecord::new(
            subject_id.clone(),
            "profile".to_string(),
            b"name: Bob Smith, age: 35".to_vec(),
            "profile_management".to_string(),
            365,
        ),
        PersonalDataRecord::new(
            subject_id.clone(),
            "purchase_history".to_string(),
            b"[order1, order2, order3]".to_vec(),
            "order_tracking".to_string(),
            365,
        ),
    ];

    for record in records_to_store {
        store.store_data(record).await?;
    }

    // User requests access to their data
    println!("  User requests access to their personal data...");
    let data = store.get_data(&subject_id).await?;
    println!("  Retrieved {} records:", data.len());

    for record in &data {
        println!("    - Type: {}, Purpose: {}, Expires: {}",
            record.data_type,
            record.purpose,
            record.retention_until.to_rfc3339()
        );
    }

    Ok(())
}

async fn erasure_example(store: &PersonalDataStore) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Demonstrating right to erasure...");

    // Register subject and store data
    let mut subject = DataSubject::new(
        "charlie@example.com".to_string(),
        "Charlie Brown".to_string(),
    );
    subject.give_consent();
    let subject_id = store.register_subject(subject).await?;

    let record = PersonalDataRecord::new(
        subject_id.clone(),
        "account_info".to_string(),
        b"sensitive account data".to_vec(),
        "account_management".to_string(),
        365,
    );

    store.store_data(record).await?;
    println!("  Stored personal data for {}", subject_id);

    // User requests deletion
    println!("  User requests erasure of all personal data...");
    let deleted_count = store.delete_data(&subject_id).await?;
    println!("  ✓ Deleted {} records", deleted_count);

    // Verify deletion
    let remaining = store.get_data(&subject_id).await?;
    println!("  Remaining records: {}", remaining.len());

    Ok(())
}

async fn portability_example(store: &PersonalDataStore) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Demonstrating right to portability...");

    // Register subject and store data
    let mut subject = DataSubject::new(
        "diana@example.com".to_string(),
        "Diana Prince".to_string(),
    );
    subject.give_consent();
    let subject_id = store.register_subject(subject).await?;

    // Store multiple data types
    let data_types = vec![
        ("profile", b"name: Diana, role: admin".to_vec()),
        ("preferences", b"language: en, timezone: UTC".to_vec()),
        ("activity", b"last_login: 2026-05-22".to_vec()),
    ];

    for (data_type, content) in data_types {
        let record = PersonalDataRecord::new(
            subject_id.clone(),
            data_type.to_string(),
            content,
            "profile_management".to_string(),
            365,
        );
        store.store_data(record).await?;
    }

    // User requests data portability
    println!("  User requests data export in portable format...");
    let json_export = store.export_data(&subject_id).await?;

    println!("  Exported data (JSON):");
    let parsed: serde_json::Value = serde_json::from_str(&json_export)?;

    for (i, item) in parsed.as_array().unwrap_or(&vec![]).iter().enumerate() {
        let record_type = item.get("data_type").and_then(|v| v.as_str()).unwrap_or("unknown");
        println!("    [{}] Type: {}", i + 1, record_type);
    }

    println!("  ✓ Data can be transferred to another service");

    Ok(())
}
