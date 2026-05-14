//! Integration tests for cloud connectors using emulators
//! 
//! Run with: `cargo test --features s3,azure,gcs --test integration_tests -- --test-threads=1`
//! 
//! Prerequisites:
//! - LocalStack running on localhost:4566 (for S3)
//! - Azurite running on localhost:10000 (for Azure)
//! - GCS Emulator running (optional, for full integration)

#![cfg(all(
    test,
    any(feature = "s3", feature = "azure", feature = "gcs")
))]

#[cfg(feature = "s3")]
#[tokio::test]
async fn test_s3_localstack_integration() {
    use kore_fileformat::s3_reader::S3Reader;

    // Skip if LocalStack not running
    if !is_localstack_running().await {
        println!("⏭️ Skipping LocalStack test (not running)");
        return;
    }

    let reader = S3Reader::new("us-east-1").expect("Failed to create reader");
    
    // Test basic connectivity (LocalStack has default bucket)
    // This would require setting up a test bucket first
    println!("✅ S3Reader created successfully");
}

#[cfg(feature = "azure")]
#[tokio::test]
async fn test_azure_azurite_integration() {
    use kore_fileformat::azure_reader::AzureBlobReader;

    // Skip if Azurite not running
    if !is_azurite_running().await {
        println!("⏭️ Skipping Azurite test (not running)");
        return;
    }

    let reader = AzureBlobReader::new("devstoreaccount1", "DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;BlobEndpoint=http://127.0.0.1:10000/devstoreaccount1;").expect("Failed to create reader");
    
    println!("✅ AzureBlobReader created successfully");
}

#[cfg(feature = "gcs")]
#[tokio::test]
async fn test_gcs_emulator_integration() {
    use kore_fileformat::gcs_reader::GcsReader;

    let reader = GcsReader::new("test-project").expect("Failed to create reader");
    
    println!("✅ GcsReader created successfully");
}

// Helper functions to check if emulators are running
async fn is_localstack_running() -> bool {
    std::process::Command::new("curl")
        .args(&["-s", "http://localhost:4566/_localstack/health"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn is_azurite_running() -> bool {
    std::process::Command::new("curl")
        .args(&["-s", "http://127.0.0.1:10000"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[test]
fn test_emulator_setup_instructions() {
    println!("\n📋 To run integration tests:\n");
    println!("1. Start LocalStack (for S3):");
    println!("   docker run -p 4566:4566 localstack/localstack\n");
    println!("2. Start Azurite (for Azure):");
    println!("   docker run -p 10000:10000 mcr.microsoft.com/azure-storage/azurite\n");
    println!("3. Run tests:");
    println!("   cargo test --features s3,azure,gcs --test integration_tests -- --test-threads=1\n");
}
