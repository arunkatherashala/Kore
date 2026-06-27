//! Append-Only Streaming Example
//!
//! Demonstrates immutable append-only streaming for event logs and time-series data

use kore_streaming::append_only::{AppendOnlyWriter, AppendOnlyReader, AppendRecord, InMemoryAppendOnlyStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kore Append-Only Streaming Example ===\n");

    let store = InMemoryAppendOnlyStore::new();

    // Simulate event stream (e.g., IoT sensor data)
    println!("Simulating event stream...");
    simulate_event_stream(&store).await?;

    // Read records
    println!("\n=== Reading Events ===");
    read_events(&store).await?;

    // Stream processing
    println!("\n=== Stream Processing ===");
    process_stream(&store).await?;

    println!("\n✓ Example completed");
    Ok(())
}

async fn simulate_event_stream(
    store: &InMemoryAppendOnlyStore,
) -> Result<(), Box<dyn std::error::Error>> {
    // Simulate time-series data from IoT sensors
    let events = vec![
        ("temperature", 22.5),
        ("temperature", 23.1),
        ("humidity", 45.2),
        ("temperature", 22.8),
        ("pressure", 1013.25),
        ("humidity", 46.1),
        ("temperature", 23.5),
        ("humidity", 45.8),
    ];

    // Batch write events
    let mut records = Vec::new();
    for (i, (sensor, value)) in events.iter().enumerate() {
        let data = format!("{{\"sensor\":\"{}\",\"value\":{}}}", sensor, value);
        let record = AppendRecord::new(i as u64, data.into_bytes());
        records.push(record);
    }

    store.append_batch(records).await?;

    println!("Events written: {}", store.total_records());
    println!("Total bytes: {} bytes", store.total_bytes());

    Ok(())
}

async fn read_events(
    store: &InMemoryAppendOnlyStore,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read all events
    let all_events = store.read_from(0).await?;
    println!("All events ({} total):", all_events.len());
    for event in &all_events {
        let data = String::from_utf8(event.data.clone())?;
        println!(
            "  [{}] {} ({})",
            event.sequence,
            data,
            event.timestamp.to_rfc3339()
        );
    }

    // Read latest 3 events
    let latest = store.read_latest(3).await?;
    println!("\nLatest 3 events:");
    for event in latest {
        let data = String::from_utf8(event.data)?;
        println!("  [{}] {}", event.sequence, data);
    }

    // Stream from specific position
    let from_seq_3 = store.read_from(3).await?;
    println!("\nEvents from sequence 3:");
    for event in from_seq_3 {
        let data = String::from_utf8(event.data)?;
        println!("  [{}] {}", event.sequence, data);
    }

    Ok(())
}

async fn process_stream(
    store: &InMemoryAppendOnlyStore,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Processing stream for analytics...");

    let events = store.read_from(0).await?;

    let mut sensor_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for event in events {
        let data = String::from_utf8(event.data)?;

        // Extract sensor name from JSON
        if let Some(start) = data.find("\"sensor\":\"") {
            if let Some(end) = data[start + 10..].find("\"") {
                let sensor_name = data[start + 10..start + 10 + end].to_string();
                *sensor_counts.entry(sensor_name).or_insert(0) += 1;
            }
        }
    }

    println!("\nSensor event counts:");
    for (sensor, count) in sensor_counts {
        println!("  {}: {} events", sensor, count);
    }

    println!("\nStream statistics:");
    println!("  Total events: {}", store.total_records());
    println!("  Total bytes: {} bytes", store.total_bytes());
    println!("  Current sequence: {}", store.current_sequence());
    println!("  Average event size: {:.0} bytes", 
        store.total_bytes() as f64 / store.total_records() as f64);

    Ok(())
}
