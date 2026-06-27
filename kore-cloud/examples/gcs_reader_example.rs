//! GCS Reader Example
//! 
//! Demonstrates reading Kore files from Google Cloud Storage with range requests

use kore_cloud::{CloudReaderBuilder, RangeRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create GCS reader
    let reader = CloudReaderBuilder::gcs("my-kore-bucket", "data/large.kore")
        .with_endpoint("my-project-id")
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

        // Streaming pattern: read file in chunks
        println!("\nStreaming file in 10MB chunks...");
        let chunk_size = 10 * 1024 * 1024;
        let mut offset = 0;

        while offset < size {
            let end = std::cmp::min(offset + chunk_size - 1, size - 1);
            let range = RangeRequest::new(offset, end)?;

            println!(
                "Reading bytes {}-{} ({} bytes)",
                offset,
                end,
                range.size()
            );

            let _data = reader.read_range(range).await?;
            offset = end + 1;
        }

        println!("\nStream complete!");
    }

    Ok(())
}
