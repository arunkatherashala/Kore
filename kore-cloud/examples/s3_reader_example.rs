//! S3 Reader Example
//! 
//! Demonstrates reading Kore files from Amazon S3 with range requests

use kore_cloud::{CloudReaderBuilder, RangeRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create S3 reader
    let reader = CloudReaderBuilder::s3("my-kore-bucket", "data/large.kore")
        .with_region("us-west-2")
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

        // Read first 1MB efficiently (range request)
        println!("\nReading first 1MB with range request...");
        let range = RangeRequest::first(1024 * 1024);
        let data = reader.read_range(range).await?;
        println!("Read {} bytes", data.len());

        // Read multiple ranges in parallel (header + footer)
        println!("\nReading header and footer in parallel...");
        let ranges = vec![
            RangeRequest::first(4096),               // First 4KB
            RangeRequest::last(size, 4096),          // Last 4KB
        ];
        let chunks = reader.read_ranges(ranges).await?;
        for (i, chunk) in chunks.iter().enumerate() {
            println!("Chunk {}: {} bytes", i, chunk.len());
        }

        // Read entire file (for smaller files)
        if size < 100 * 1024 * 1024 {
            // Only if < 100MB
            println!("\nReading entire file...");
            let all_data = reader.read_all().await?;
            println!("Total read: {} bytes", all_data.len());
        }
    }

    Ok(())
}
