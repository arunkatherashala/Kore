//! Audit Logging Example
//!
//! Demonstrates audit trail for security events and compliance

use kore_security::audit::{AuditLog, AuditEvent, AuditEventType, InMemoryAuditLog};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kore Audit Logging Example ===\n");

    let audit = InMemoryAuditLog::new();

    // Example 1: Log various audit events
    println!("Example 1: Logging Security Events");
    log_security_events(&audit).await?;

    // Example 2: Query audit trail
    println!("\nExample 2: Querying Audit Trail");
    query_audit_trail(&audit).await?;

    // Example 3: Security incident investigation
    println!("\nExample 3: Security Investigation");
    investigate_incident(&audit).await?;

    println!("\n✓ Audit examples completed");
    Ok(())
}

async fn log_security_events(audit: &InMemoryAuditLog) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Logging audit events...");

    // Authentication
    let mut auth_event = AuditEvent::new(
        AuditEventType::Authentication,
        "user:alice".to_string(),
        "system:auth".to_string(),
        "LOGIN".to_string(),
        true,
    );
    auth_event = auth_event.with_detail("ip", serde_json::json!("192.168.1.100"));
    audit.log(auth_event).await?;

    // Data read
    let mut read_event = AuditEvent::new(
        AuditEventType::DataRead,
        "user:bob".to_string(),
        "file:customer_data.csv".to_string(),
        "READ".to_string(),
        true,
    );
    read_event = read_event.with_detail("rows", serde_json::json!(1000));
    audit.log(read_event).await?;

    // Data write
    let mut write_event = AuditEvent::new(
        AuditEventType::DataWrite,
        "user:alice".to_string(),
        "database:accounts".to_string(),
        "INSERT".to_string(),
        true,
    );
    write_event = write_event.with_detail("table", serde_json::json!("transactions"));
    audit.log(write_event).await?;

    // Failed access attempt
    let mut failed_event = AuditEvent::new(
        AuditEventType::Authorization,
        "user:charlie".to_string(),
        "file:secret.key".to_string(),
        "READ".to_string(),
        false,
    );
    failed_event = failed_event.with_detail("reason", serde_json::json!("Permission denied"));
    audit.log(failed_event).await?;

    // Encryption operation
    let mut encrypt_event = AuditEvent::new(
        AuditEventType::Encryption,
        "system:backup".to_string(),
        "data:sensitive".to_string(),
        "ENCRYPT_AES256".to_string(),
        true,
    );
    encrypt_event = encrypt_event.with_detail("algorithm", serde_json::json!("AES-256-GCM"));
    audit.log(encrypt_event).await?;

    println!("  Logged {} events", 5);

    Ok(())
}

async fn query_audit_trail(audit: &InMemoryAuditLog) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Querying audit trail...");

    // Get all events
    let all_events = audit.get_all_events().await?;
    println!("  Total events: {}", all_events.len());

    // Get events by subject
    let alice_events = audit.get_events_by_subject("user:alice").await?;
    println!("  Events by user:alice: {}", alice_events.len());
    for event in &alice_events {
        println!("    - [{}] {} on {}", 
            event.event_type, event.action, event.resource);
    }

    // Get failed events
    let failed_events = audit.get_failed_events().await?;
    println!("  Failed events (security issues): {}", failed_events.len());
    for event in &failed_events {
        println!("    - User {} failed to {} on {}",
            event.subject, event.action, event.resource);
    }

    // Get events by type
    let auth_events = audit.get_events_by_type(AuditEventType::DataRead).await?;
    println!("  DataRead events: {}", auth_events.len());

    Ok(())
}

async fn investigate_incident(audit: &InMemoryAuditLog) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Investigating security incident...");

    // Check for suspicious patterns
    let all_events = audit.get_all_events().await?;

    // Count failed authentication attempts per user
    let mut failed_auth: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for event in &all_events {
        if event.event_type == AuditEventType::Authorization && !event.result {
            *failed_auth.entry(event.subject.clone()).or_insert(0) += 1;
        }
    }

    println!("  Failed authorization attempts:");
    for (subject, count) in failed_auth {
        if count > 0 {
            println!("    - {}: {} attempts", subject, count);
        }
    }

    // Get events by resource
    let sensitive_access = audit.get_events_by_resource("file:secret.key").await?;
    println!("  Access to secret.key: {} events", sensitive_access.len());

    for event in &sensitive_access {
        println!("    - {} by {} at {}", 
            if event.result { "✓ ALLOWED" } else { "✗ BLOCKED" },
            event.subject,
            event.timestamp.to_rfc3339()
        );
    }

    // Compliance report
    println!("\n  Compliance Summary:");
    println!("    - Total events logged: {}", all_events.len());
    println!("    - Successful operations: {}", all_events.iter().filter(|e| e.result).count());
    println!("    - Failed operations: {}", all_events.iter().filter(|e| !e.result).count());

    Ok(())
}
