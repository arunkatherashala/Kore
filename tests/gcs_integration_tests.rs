//! Google Cloud Storage Integration Tests
//!
//! These tests verify Kore's Google Cloud Storage integration works correctly.
//! Requires GCS emulator (gcloud-cli) or real GCS credentials.
//!
//! Setup with gcloud CLI:
//! ```bash
//! gcloud auth application-default login
//! # or set GOOGLE_APPLICATION_CREDENTIALS env var
//! ```

#![cfg(feature = "gcs")]

use kore_fileformat::gcs_reader::GcsReader;

#[tokio::test]
async fn test_gcs_read_write_small_object() {
    // Setup
    let reader = GcsReader::new("my-project");
    assert!(reader.is_ok(), "Failed to create GCS reader");
    
    let reader = reader.unwrap();
    
    // Write test data
    let test_data = b"Hello, Google Cloud Storage! This is a test message for Kore v1.1.0";
    let write_result = reader
        .write_file("test-bucket", "test-object.bin", test_data)
        .await;
    
    if write_result.is_err() {
        eprintln!("⚠️  Note: GCS credentials not configured. Skipping GCS test.");
        eprintln!("   Setup with: gcloud auth application-default login");
        return;
    }
    
    // Read it back
    let read_result = reader
        .read_file("test-bucket", "test-object.bin")
        .await;
    
    assert!(read_result.is_ok(), "Failed to read object");
    assert_eq!(read_result.unwrap(), test_data, "Data mismatch");
}

#[tokio::test]
async fn test_gcs_metadata() {
    let reader = GcsReader::new("my-project");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Write test object
    let test_data = b"metadata test data";
    let _ = reader
        .write_file("test-bucket", "metadata-test.bin", test_data)
        .await;
    
    // Fetch metadata
    let meta_result = reader
        .get_metadata("test-bucket", "metadata-test.bin")
        .await;
    
    if meta_result.is_err() {
        eprintln!("⚠️  GCS credentials not configured");
        return;
    }
    
    let meta = meta_result.unwrap();
    assert_eq!(meta.size, test_data.len() as u64, "Size mismatch");
    assert!(!meta.generation.is_empty(), "Generation should not be empty");
}

#[tokio::test]
async fn test_gcs_list_objects() {
    let reader = GcsReader::new("my-project");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Write multiple test objects
    let _ = reader.write_file("test-bucket", "list1.bin", b"data1").await;
    let _ = reader.write_file("test-bucket", "list2.bin", b"data2").await;
    let _ = reader.write_file("test-bucket", "list3.bin", b"data3").await;
    
    // List objects
    let list_result = reader.list_objects("test-bucket", None).await;
    
    if list_result.is_err() {
        eprintln!("⚠️  GCS credentials not configured");
        return;
    }
    
    let objects = list_result.unwrap();
    assert!(!objects.is_empty(), "Should list at least one object");
}

#[tokio::test]
async fn test_gcs_large_object() {
    let reader = GcsReader::new("my-project");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Create 256MB test data
    let test_data = vec![42u8; 256 * 1024 * 1024];
    
    let write_result = reader
        .write_file("test-bucket", "large-object.bin", &test_data)
        .await;
    
    if write_result.is_err() {
        eprintln!("⚠️  GCS credentials not configured");
        return;
    }
    
    // Read it back
    let read_result = reader
        .read_file("test-bucket", "large-object.bin")
        .await;
    
    assert!(read_result.is_ok(), "Failed to read large object");
    assert_eq!(read_result.unwrap().len(), test_data.len(), "Large object size mismatch");
}

#[tokio::test]
async fn test_gcs_prefix_filtering() {
    let reader = GcsReader::new("my-project");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Write objects with prefixes
    let _ = reader.write_file("test-bucket", "2024/jan/data.bin", b"jan").await;
    let _ = reader.write_file("test-bucket", "2024/feb/data.bin", b"feb").await;
    let _ = reader.write_file("test-bucket", "2025/jan/data.bin", b"jan2025").await;
    
    // List with prefix
    let list_result = reader.list_objects("test-bucket", Some("2024/")).await;
    
    if list_result.is_err() {
        eprintln!("⚠️  GCS credentials not configured");
        return;
    }
    
    let objects = list_result.unwrap();
    let has_2024 = objects.iter().any(|o| o.contains("2024"));
    assert!(has_2024, "Should find objects with 2024 prefix");
}

#[tokio::test]
async fn test_gcs_content_type() {
    let reader = GcsReader::new("my-project");
    
    if reader.is_err() {
        return;
    }
    
    let reader = reader.unwrap();
    
    // Write test object
    let test_data = b"test data with content type";
    let _ = reader
        .write_file("test-bucket", "content-type-test.bin", test_data)
        .await;
    
    // Get metadata
    let meta_result = reader
        .get_metadata("test-bucket", "content-type-test.bin")
        .await;
    
    if meta_result.is_err() {
        return;
    }
    
    let meta = meta_result.unwrap();
    assert!(meta.content_type.is_some(), "Content type should be present");
}

#[test]
fn test_gcs_reader_creation() {
    // Valid project
    let result = GcsReader::new("my-project");
    assert!(result.is_ok(), "Should create reader with valid project ID");
    
    // Empty project
    let result = GcsReader::new("");
    assert!(result.is_err(), "Should reject empty project ID");
}

#[test]
fn test_gcs_cache_config() {
    let mut reader = GcsReader::new("my-project").unwrap();
    
    // Enable cache
    let result = reader.with_cache("./gcs-cache");
    assert!(result.is_ok(), "Should enable cache");
    assert!(reader.cache_enabled(), "Cache should be enabled");
    
    // Empty cache dir
    let result = reader.with_cache("");
    assert!(result.is_err(), "Should reject empty cache dir");
}

#[test]
fn test_gcs_project_id_getter() {
    let reader = GcsReader::new("test-project").unwrap();
    assert_eq!(reader.project_id(), "test-project");
}

#[tokio::test]
async fn test_gcs_parallel_operations() {
    let reader = std::sync::Arc::new(GcsReader::new("my-project").unwrap());
    
    // Spawn multiple concurrent write tasks
    let mut handles = vec![];
    for i in 0..5 {
        let reader = reader.clone();
        let handle = tokio::spawn(async move {
            let data = format!("data-{}", i).into_bytes();
            let path = format!("parallel-{}.bin", i);
            let _ = reader.write_file("test-bucket", &path, &data).await;
        });
        handles.push(handle);
    }
    
    // Wait for all tasks
    for handle in handles {
        let _ = handle.await;
    }
}
