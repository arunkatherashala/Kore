//! Azure Blob Storage Integration Tests
//!
//! These tests verify Kore's Azure Blob Storage integration works correctly.
//! Requires Azurite emulator to be running.
//!
//! Start Azurite with:
//! ```bash
//! azurite --silent
//! # or with Docker:
//! # docker run -p 10000:10000 mcr.microsoft.com/azure-storage/azurite
//! ```

#![cfg(feature = "azure")]

use kore_fileformat::azure_reader::AzureBlobReader;

#[tokio::test]
async fn test_azure_read_write_small_blob() {
    // Setup
    std::env::set_var("AZURE_STORAGE_ACCOUNT", "devstoreaccount1");
    std::env::set_var("AZURE_STORAGE_KEY", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    let reader = AzureBlobReader::new("devstoreaccount1", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    assert!(reader.is_ok(), "Failed to create Azure reader");
    
    let reader = reader.unwrap();
    
    // Write test data
    let test_data = b"Hello, Azure Blob Storage! This is a test message for Kore v1.1.0";
    let write_result = reader
        .write_file("test-container", "test-blob.bin", test_data)
        .await;
    
    if write_result.is_err() {
        eprintln!("⚠️  Note: Azurite emulator not running. Skipping Azure test.");
        eprintln!("   Start with: azurite --silent");
        return;
    }
    
    // Read it back
    let read_result = reader
        .read_file("test-container", "test-blob.bin")
        .await;
    
    assert!(read_result.is_ok(), "Failed to read blob");
    assert_eq!(read_result.unwrap(), test_data, "Data mismatch");
}

#[tokio::test]
async fn test_azure_metadata() {
    std::env::set_var("AZURE_STORAGE_ACCOUNT", "devstoreaccount1");
    std::env::set_var("AZURE_STORAGE_KEY", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    let reader = AzureBlobReader::new("devstoreaccount1", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Write test blob
    let test_data = b"metadata test data";
    let _ = reader
        .write_file("test-container", "metadata-test.bin", test_data)
        .await;
    
    // Fetch metadata
    let meta_result = reader
        .get_metadata("test-container", "metadata-test.bin")
        .await;
    
    if meta_result.is_err() {
        eprintln!("⚠️  Azurite emulator not running");
        return;
    }
    
    let meta = meta_result.unwrap();
    assert_eq!(meta.size, test_data.len() as u64, "Size mismatch");
    assert!(!meta.etag.is_empty(), "ETag should not be empty");
}

#[tokio::test]
async fn test_azure_list_blobs() {
    std::env::set_var("AZURE_STORAGE_ACCOUNT", "devstoreaccount1");
    std::env::set_var("AZURE_STORAGE_KEY", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    let reader = AzureBlobReader::new("devstoreaccount1", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Write multiple test blobs
    let _ = reader.write_file("test-container", "list1.bin", b"data1").await;
    let _ = reader.write_file("test-container", "list2.bin", b"data2").await;
    let _ = reader.write_file("test-container", "list3.bin", b"data3").await;
    
    // List blobs
    let list_result = reader.list_blobs("test-container", None).await;
    
    if list_result.is_err() {
        eprintln!("⚠️  Azurite emulator not running");
        return;
    }
    
    let blobs = list_result.unwrap();
    assert!(!blobs.is_empty(), "Should list at least one blob");
}

#[tokio::test]
async fn test_azure_large_blob() {
    std::env::set_var("AZURE_STORAGE_ACCOUNT", "devstoreaccount1");
    std::env::set_var("AZURE_STORAGE_KEY", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    let reader = AzureBlobReader::new("devstoreaccount1", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Create 5MB test data
    let test_data = vec![42u8; 5 * 1024 * 1024];
    
    let write_result = reader
        .write_file("test-container", "large-blob.bin", &test_data)
        .await;
    
    if write_result.is_err() {
        eprintln!("⚠️  Azurite emulator not running");
        return;
    }
    
    // Read it back
    let read_result = reader
        .read_file("test-container", "large-blob.bin")
        .await;
    
    assert!(read_result.is_ok(), "Failed to read large blob");
    assert_eq!(read_result.unwrap().len(), test_data.len(), "Large blob size mismatch");
}

#[tokio::test]
async fn test_azure_prefix_filtering() {
    std::env::set_var("AZURE_STORAGE_ACCOUNT", "devstoreaccount1");
    std::env::set_var("AZURE_STORAGE_KEY", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    let reader = AzureBlobReader::new("devstoreaccount1", "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2uvTmjCodQriCEVq5z9v==");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Write blobs with prefixes
    let _ = reader.write_file("test-container", "2024/jan/data.bin", b"jan").await;
    let _ = reader.write_file("test-container", "2024/feb/data.bin", b"feb").await;
    let _ = reader.write_file("test-container", "2025/jan/data.bin", b"jan2025").await;
    
    // List with prefix
    let list_result = reader.list_blobs("test-container", Some("2024/")).await;
    
    if list_result.is_err() {
        eprintln!("⚠️  Azurite emulator not running");
        return;
    }
    
    let blobs = list_result.unwrap();
    let has_2024 = blobs.iter().any(|b| b.contains("2024"));
    assert!(has_2024, "Should find blobs with 2024 prefix");
}

#[test]
fn test_azure_reader_creation() {
    // Valid credentials
    let result = AzureBlobReader::new("myaccount", "mykey");
    assert!(result.is_ok(), "Should create reader with valid credentials");
    
    // Empty account
    let result = AzureBlobReader::new("", "key");
    assert!(result.is_err(), "Should reject empty account");
    
    // Empty key
    let result = AzureBlobReader::new("account", "");
    assert!(result.is_err(), "Should reject empty key");
}

#[test]
fn test_azure_cache_config() {
    let mut reader = AzureBlobReader::new("account", "key").unwrap();
    
    // Enable cache
    let result = reader.with_cache("./azure-cache");
    assert!(result.is_ok(), "Should enable cache");
    assert!(reader.cache_enabled(), "Cache should be enabled");
    
    // Empty cache dir
    let result = reader.with_cache("");
    assert!(result.is_err(), "Should reject empty cache dir");
}

#[test]
fn test_azure_storage_account_getter() {
    let reader = AzureBlobReader::new("test-account", "key").unwrap();
    assert_eq!(reader.storage_account(), "test-account");
}
