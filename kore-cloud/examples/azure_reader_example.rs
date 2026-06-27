//! Azure Reader Example
//! 
//! Demonstrates reading Kore files from Azure Blob Storage with range requests

use kore_cloud::{CloudReaderBuilder, RangeRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create Azure reader
    let reader = CloudReaderBuilder::azure("my-container", "data/large.kore")
        .with_endpoint("mystorageaccount")
        .build()?;

    // Check if file exists
    let exists = reader.exists().await?;
    println!("File exists: {}", exists);

    if exists {
        // Get total size
        let size = reader.size().await?;
        println!("File size: {} bytes", size);

        // Get metadata
        let metadata = reader.metadata().await?;
        println!("ETag: {}", metadata.etag);
        println!("Modified: {}", metadata.last_modified);

        // Example: Read specific sections for query optimization
        println!("\nReading query-optimized sections...");

        // 1. Read schema metadata (first KB)
        let schema_range = RangeRequest::first(1024);
        let _schema = reader.read_range(schema_range).await?;
        println!("Schema metadata: 1KB");

        // 2. Read row group index (specific offset)
        let index_range = RangeRequest::new(10 * 1024 * 1024, 10 * 1024 * 1024 + 1024)?;
        let _index = reader.read_range(index_range).await?;
        println!("Row group index: 1KB at offset 10MB");

        // 3. Read data sections in parallel
        println!("\nReading 3 row groups in parallel...");
        let ranges = vec![
            RangeRequest::new(20 * 1024 * 1024, 30 * 1024 * 1024)?,
            RangeRequest::new(40 * 1024 * 1024, 50 * 1024 * 1024)?,
            RangeRequest::new(60 * 1024 * 1024, 70 * 1024 * 1024)?,
        ];

        let results = reader.read_ranges(ranges).await?;
        println!("Read {} row groups", results.len());
        for (i, data) in results.iter().enumerate() {
            println!("Row group {}: {} bytes", i, data.len());
        }
    }

    Ok(())
}
