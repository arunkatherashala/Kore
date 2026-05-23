//! CDC (Change Data Capture) Streaming Example
//!
//! Demonstrates real-time change streaming for replication and analytics

use kore_streaming::cdc::{ChangeType, ChangeRecord, CDCStream, InMemoryCDCStream};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kore CDC Streaming Example ===\n");

    let stream = InMemoryCDCStream::new();

    // Example 1: Simulate data changes
    println!("Example 1: Publishing Changes");
    publish_changes(&stream).await?;

    // Example 2: Subscribe and consume
    println!("\nExample 2: Subscribing to Changes");
    subscribe_changes(&stream).await?;

    // Example 3: CDC for replication
    println!("\nExample 3: CDC-based Replication");
    cdc_replication(&stream).await?;

    // Example 4: Stream statistics
    println!("\nExample 4: Stream Statistics");
    stream_statistics(&stream).await?;

    println!("\n✓ CDC examples completed");
    Ok(())
}

async fn publish_changes(stream: &InMemoryCDCStream) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Publishing changes...");

    // Simulate data warehouse updates
    let changes = vec![
        ChangeRecord::insert(0, b"Customer: Alice (ID: 1)".to_vec()),
        ChangeRecord::insert(1, b"Customer: Bob (ID: 2)".to_vec()),
        ChangeRecord::update(
            2,
            b"Customer: Alice (ID: 1)".to_vec(),
            b"Customer: Alice Smith (ID: 1)".to_vec(),
        ),
        ChangeRecord::insert(3, b"Customer: Charlie (ID: 3)".to_vec()),
        ChangeRecord::delete(4, b"Customer: Bob (ID: 2)".to_vec()),
    ];

    stream.publish_batch(changes).await?;

    println!("  Published {} changes", stream.latest_sequence());

    Ok(())
}

async fn subscribe_changes(stream: &InMemoryCDCStream) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Subscribing from sequence 0...");

    let changes = stream.subscribe(0).await?;

    println!("  Received {} changes:", changes.len());
    for change in &changes {
        let data = String::from_utf8(change.after.clone().or(change.before.clone()).unwrap_or_default())?;
        println!(
            "    [{}] {} - {}",
            change.sequence, change.change_type, data
        );
    }

    Ok(())
}

async fn cdc_replication(stream: &InMemoryCDCStream) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Simulating replica database...");

    // Replica 1: read from sequence 0
    let replica1_changes = stream.subscribe(0).await?;
    println!("  Replica 1: replicated {} changes", replica1_changes.len());

    // Replica 2: read from sequence 2 (late join)
    let replica2_changes = stream.subscribe(2).await?;
    println!("  Replica 2: caught up with {} changes", replica2_changes.len());

    // Track state
    let mut replica_state: std::collections::HashMap<u64, String> = std::collections::HashMap::new();

    for change in replica1_changes {
        match change.change_type {
            ChangeType::Insert | ChangeType::Update => {
                if let Some(data) = change.after {
                    replica_state.insert(
                        change.sequence,
                        String::from_utf8(data).unwrap_or_default(),
                    );
                }
            }
            ChangeType::Delete => {
                replica_state.remove(&change.sequence);
            }
        }
    }

    println!("  Replica state: {} records", replica_state.len());

    Ok(())
}

async fn stream_statistics(stream: &InMemoryCDCStream) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Stream Statistics:");
    println!("    Latest sequence: {}", stream.latest_sequence());
    println!("    Subscribers: {}", stream.subscriber_count());

    // Count changes by type
    let changes = stream.subscribe(0).await?;

    let mut insert_count = 0;
    let mut update_count = 0;
    let mut delete_count = 0;
    let mut total_bytes = 0;

    for change in changes {
        total_bytes += change.size();
        match change.change_type {
            ChangeType::Insert => insert_count += 1,
            ChangeType::Update => update_count += 1,
            ChangeType::Delete => delete_count += 1,
        }
    }

    println!("    Inserts: {}", insert_count);
    println!("    Updates: {}", update_count);
    println!("    Deletes: {}", delete_count);
    println!("    Total bytes: {}", total_bytes);

    Ok(())
}
