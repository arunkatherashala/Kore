//! AWS S3 Connector Example
//! 
//! This example demonstrates how to use Kore with AWS S3 for reading and writing files.
//! 
//! # Setup
//! 
//! 1. Enable the `s3` feature in Cargo.toml:
//!    ```toml
//!    kore_fileformat = { version = "1.0", features = ["s3"] }
//!    ```
//! 
//! 2. Configure AWS credentials:
//!    ```bash
//!    export AWS_ACCESS_KEY_ID=your_access_key
//!    export AWS_SECRET_ACCESS_KEY=your_secret_key
//!    export AWS_REGION=us-east-1
//!    ```
//! 
//! 3. Run the example:
//!    ```bash
//!    cargo run --example s3_connector --features s3
//!    ```

#[cfg(feature = "s3")]
use kore_fileformat::s3_reader::S3Reader;

#[cfg(feature = "s3")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Kore S3 Connector Example");
    println!("============================\n");

    // 1. Create S3Reader for US East region
    println!("📍 Creating S3Reader for us-east-1...");
    let mut reader = S3Reader::new("us-east-1")?;
    println!("✅ S3Reader created successfully\n");

    // 2. Enable local caching
    println!("💾 Enabling local caching...");
    reader.with_cache("./kore_s3_cache")?;
    println!("✅ Cache enabled at ./kore_s3_cache\n");

    // 3. Example: Read a Kore file from S3
    println!("📖 Example: Reading Kore file from S3");
    println!("   Command: reader.read_file(\"my-bucket\", \"data/records.kore\").await?");
    println!("   This would:");
    println!("     1. Check local cache first");
    println!("     2. Download from S3 if not cached");
    println!("     3. Cache locally for future use\n");

    // 4. Example: Write a Kore file to S3
    println!("✍️  Example: Writing Kore file to S3");
    println!("   Command: reader.write_file(\"my-bucket\", \"output.kore\", &data).await?");
    println!("   This would:");
    println!("     1. Upload file to S3");
    println!("     2. Update local cache\n");

    // 5. Example: List files in S3
    println!("📋 Example: Listing Kore files in S3");
    println!("   Command: reader.list_files(\"my-bucket\", Some(\"data/\")).await?");
    println!("   Returns: Vec<String> of object keys\n");

    // 6. Example: Get file metadata
    println!("ℹ️  Example: Getting file metadata");
    println!("   Command: reader.get_metadata(\"my-bucket\", \"data/records.kore\").await?");
    println!("   Returns: S3FileMetadata with size, etag, content_type\n");

    println!("💡 Features:");
    println!("   ✓ Native S3 integration");
    println!("   ✓ Local caching for performance");
    println!("   ✓ Async/await support");
    println!("   ✓ Error handling");
    println!("   ✓ File metadata retrieval");
    println!("   ✓ List operations\n");

    println!("🌐 Other Cloud Connectors Coming Soon:");
    println!("   • Azure Blob Storage");
    println!("   • Google Cloud Storage (GCS)");
    println!("   • Snowflake Native Connector\n");

    println!("📚 See README.md for more examples");

    Ok(())
}

#[cfg(not(feature = "s3"))]
fn main() {
    println!("❌ S3 feature not enabled!");
    println!("Run with: cargo run --example s3_connector --features s3");
}
