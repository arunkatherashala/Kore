# Phase 1: Azure Blob Storage Full Implementation

**Target Duration**: 1-2 weeks  
**Branch**: `develop-v1.1.0`  
**Status**: Starting May 14, 2026  
**Goal**: Replace all stub implementations in `src/azure_reader.rs` with full functionality

---

## 📋 Implementation Checklist

### Core Methods (Primary Goals)
- [ ] `read_from_azure()` - Full async implementation with streaming
- [ ] `write_to_azure()` - Full async implementation with upload
- [ ] `list_azure_blobs()` - List blobs with filtering
- [ ] `fetch_azure_metadata()` - Extract blob metadata

### Supporting Infrastructure
- [ ] Authentication setup (connection string, account key, SAS token)
- [ ] Error handling and retry logic
- [ ] Connection pooling for multiple operations
- [ ] Logging and debugging support
- [ ] Timeout and rate limiting

### Testing & Validation
- [ ] Azurite emulator setup instructions
- [ ] Integration tests with real Azure SDK
- [ ] Performance benchmarks
- [ ] Error scenario testing
- [ ] Cross-region fallback testing

### Documentation
- [ ] API documentation with examples
- [ ] Azure authentication guide
- [ ] Troubleshooting guide
- [ ] Migration guide (S3 to Azure)

---

## 🔑 Current State (v1.0.0)

### Stub Implementation in `src/azure_reader.rs`

```rust
pub async fn read_from_azure(container: &str, blob_path: &str) -> Result<Vec<u8>> {
    Err("Azure Blob Storage integration available in v1.1".to_string())
}

pub async fn write_to_azure(container: &str, blob_path: &str, data: &[u8]) -> Result<()> {
    Err("Azure Blob Storage integration available in v1.1".to_string())
}

pub async fn list_azure_blobs(container: &str, prefix: &str) -> Result<Vec<BlobItem>> {
    Err("Azure Blob Storage integration available in v1.1".to_string())
}

pub async fn fetch_azure_metadata(container: &str, blob_path: &str) -> Result<AzureBlobMetadata> {
    Err("Azure Blob Storage integration available in v1.1".to_string())
}
```

---

## 🎯 Target Implementation Pattern

### Reference: S3 Implementation (src/s3_reader.rs)

The S3 implementation provides the pattern we'll follow:

```rust
pub async fn read_from_s3(bucket: &str, key: &str) -> Result<Vec<u8>> {
    // 1. Get client config
    let config = aws_config::load_from_env().await;
    let client = Client::new(&config);
    
    // 2. Build request with error handling
    let resp = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| format!("S3 read error: {}", e))?;
    
    // 3. Stream and collect body
    let bytes = resp
        .body
        .collect()
        .await
        .map_err(|e| format!("Body collection error: {}", e))?
        .into_bytes();
    
    Ok(bytes)
}
```

**Apply this pattern to Azure:**
```rust
pub async fn read_from_azure(container: &str, blob_path: &str) -> Result<Vec<u8>> {
    // 1. Get client (using connection string or SAS)
    let client = BlobServiceClient::new(/* credentials */);
    let container_client = client.container_client(container);
    let blob_client = container_client.blob_client(blob_path);
    
    // 2. Download with error handling
    let data = blob_client
        .get()
        .await
        .map_err(|e| format!("Azure read error: {}", e))?;
    
    // 3. Return bytes
    Ok(data.body.into())
}
```

---

## 📦 Dependencies to Add

Update `Cargo.toml` feature flag for Azure:

```toml
[features]
# ... existing features ...
azure = ["azure_storage_blobs", "azure_identity", "tokio"]

[dependencies]
# ... existing deps ...
azure_storage_blobs = { version = "0.20", optional = true }
azure_identity = { version = "0.17", optional = true }
```

---

## 🔐 Authentication Methods to Support

### 1. Connection String (Simplest - Local Testing)
```rust
let connection_string = "DefaultEndpointsProtocol=https;AccountName=myaccount;AccountKey=...";
let client = BlobServiceClient::from_connection_string(connection_string)
    .map_err(|e| format!("Connection error: {}", e))?;
```

### 2. Account Key (Production - Secrets Management)
```rust
let account_name = "myaccount";
let account_key = std::env::var("AZURE_STORAGE_KEY")
    .map_err(|_| "AZURE_STORAGE_KEY not set".to_string())?;
    
let client = BlobServiceClient::new(
    format!("https://{}.blob.core.windows.net", account_name),
    StorageSharedKeyCredential::new(account_name, account_key),
);
```

### 3. SAS Token (Fine-grained Access)
```rust
let account_name = "myaccount";
let sas_token = std::env::var("AZURE_STORAGE_SAS_TOKEN")
    .map_err(|_| "AZURE_STORAGE_SAS_TOKEN not set".to_string())?;
    
let url = format!("https://{}.blob.core.windows.net?{}", account_name, sas_token);
let client = BlobServiceClient::new(url, StorageSharedKeyCredential::new("", ""));
```

### 4. Managed Identity (Azure VM/App Service)
```rust
use azure_identity::ClientSecretCredential;

let credential = ClientSecretCredential::new(
    "tenant-id",
    "client-id",
    "client-secret",
);
let client = BlobServiceClient::new(
    format!("https://{}.blob.core.windows.net", account_name),
    credential,
);
```

---

## 📝 Full Implementation Template

### 1. `read_from_azure()` - Full Implementation

```rust
/// Read blob data from Azure Blob Storage
/// 
/// # Arguments
/// * `container` - Container name
/// * `blob_path` - Path to blob (e.g., "data/file.kore")
/// 
/// # Returns
/// Raw bytes from blob
/// 
/// # Example
/// ```ignore
/// let data = read_from_azure("data-container", "records/data.kore").await?;
/// println!("Read {} bytes", data.len());
/// ```
#[cfg(feature = "azure")]
pub async fn read_from_azure(container: &str, blob_path: &str) -> Result<Vec<u8>> {
    // Get credentials from environment
    let account_name = std::env::var("AZURE_STORAGE_ACCOUNT")
        .unwrap_or_else(|_| "devstoreaccount1".to_string());
    let account_key = std::env::var("AZURE_STORAGE_KEY")
        .or_else(|_| std::env::var("AZURE_STORAGE_ACCOUNT_KEY"))
        .map_err(|_| "AZURE_STORAGE_KEY or AZURE_STORAGE_ACCOUNT_KEY not set".to_string())?;
    
    // Create client with retry logic
    let client = create_azure_client(&account_name, &account_key)?;
    let container_client = client.container_client(container);
    let blob_client = container_client.blob_client(blob_path);
    
    // Download with exponential backoff retry
    let mut attempt = 0;
    const MAX_RETRIES: u32 = 3;
    
    loop {
        match blob_client.get().await {
            Ok(resp) => {
                return Ok(resp.body.into());
            }
            Err(e) if attempt < MAX_RETRIES => {
                attempt += 1;
                log::warn!(
                    "Azure read attempt {} failed for {}/{}: {}. Retrying...",
                    attempt,
                    container,
                    blob_path,
                    e
                );
                tokio::time::sleep(
                    std::time::Duration::from_millis(100 * 2_u64.pow(attempt))
                ).await;
            }
            Err(e) => {
                return Err(format!(
                    "Failed to read blob {}/{} after {} attempts: {}",
                    container, blob_path, MAX_RETRIES, e
                ));
            }
        }
    }
}

/// Helper function to create Azure Blob client
#[cfg(feature = "azure")]
fn create_azure_client(account_name: &str, account_key: &str) -> Result<BlobServiceClient> {
    use azure_storage_blobs::blob::BlobClient;
    
    let url = format!("https://{}.blob.core.windows.net", account_name);
    let credential = azure_storage::StorageCredentials::access_key(
        account_name.to_string(),
        account_key.to_string(),
    );
    
    Ok(BlobServiceClient::new(url, credential))
}
```

### 2. `write_to_azure()` - Full Implementation

```rust
/// Write data to Azure Blob Storage
/// 
/// # Arguments
/// * `container` - Container name
/// * `blob_path` - Path to blob
/// * `data` - Data to write
/// 
/// # Example
/// ```ignore
/// write_to_azure("data-container", "output/file.kore", &bytes).await?;
/// ```
#[cfg(feature = "azure")]
pub async fn write_to_azure(container: &str, blob_path: &str, data: &[u8]) -> Result<()> {
    // Get credentials
    let account_name = std::env::var("AZURE_STORAGE_ACCOUNT")
        .unwrap_or_else(|_| "devstoreaccount1".to_string());
    let account_key = std::env::var("AZURE_STORAGE_KEY")
        .or_else(|_| std::env::var("AZURE_STORAGE_ACCOUNT_KEY"))
        .map_err(|_| "AZURE_STORAGE_KEY or AZURE_STORAGE_ACCOUNT_KEY not set".to_string())?;
    
    // Create client
    let client = create_azure_client(&account_name, &account_key)?;
    let container_client = client.container_client(container);
    let blob_client = container_client.blob_client(blob_path);
    
    // Upload with chunking for large files
    let chunk_size = 4 * 1024 * 1024; // 4MB chunks
    
    if data.len() <= chunk_size {
        // Single upload for small files
        blob_client
            .put_block_blob(data.to_vec())
            .await
            .map_err(|e| format!("Azure write error: {}", e))?;
    } else {
        // Block blob upload for large files
        let mut blocks = Vec::new();
        for (i, chunk) in data.chunks(chunk_size).enumerate() {
            let block_id = format!("{:08x}", i).into_bytes();
            blob_client
                .put_block(block_id.clone(), chunk.to_vec())
                .await
                .map_err(|e| format!("Put block error: {}", e))?;
            blocks.push(block_id);
        }
        
        // Finalize upload
        blob_client
            .put_block_list(blocks)
            .await
            .map_err(|e| format!("Finalize error: {}", e))?;
    }
    
    log::info!("Successfully wrote {} bytes to {}/{}", data.len(), container, blob_path);
    Ok(())
}
```

### 3. `list_azure_blobs()` - Full Implementation

```rust
/// List blobs in an Azure container with optional prefix filtering
/// 
/// # Arguments
/// * `container` - Container name
/// * `prefix` - Filter blobs by prefix (e.g., "data/" returns data/*, empty prefix returns all)
/// 
/// # Example
/// ```ignore
/// let blobs = list_azure_blobs("data-container", "2024/").await?;
/// for blob in blobs {
///     println!("{}: {}", blob.name, blob.size);
/// }
/// ```
#[cfg(feature = "azure")]
pub async fn list_azure_blobs(container: &str, prefix: &str) -> Result<Vec<BlobItem>> {
    let account_name = std::env::var("AZURE_STORAGE_ACCOUNT")
        .unwrap_or_else(|_| "devstoreaccount1".to_string());
    let account_key = std::env::var("AZURE_STORAGE_KEY")
        .or_else(|_| std::env::var("AZURE_STORAGE_ACCOUNT_KEY"))
        .map_err(|_| "AZURE_STORAGE_KEY or AZURE_STORAGE_ACCOUNT_KEY not set".to_string())?;
    
    let client = create_azure_client(&account_name, &account_key)?;
    let container_client = client.container_client(container);
    
    let mut blobs = Vec::new();
    let mut paginated = container_client.list_blobs();
    
    // Apply prefix filter if provided
    if !prefix.is_empty() {
        paginated = paginated.prefix(prefix);
    }
    
    // Iterate through pages
    match paginated.into_stream().next().await {
        Some(Ok(page)) => {
            for blob in page.blobs {
                blobs.push(BlobItem {
                    name: blob.name.clone(),
                    size: blob.properties.content_length,
                    last_modified: blob.properties.last_modified,
                    content_type: blob.properties.content_type.clone(),
                });
            }
        }
        Some(Err(e)) => {
            return Err(format!("List blobs error: {}", e));
        }
        None => {}
    }
    
    log::info!("Listed {} blobs in container {}", blobs.len(), container);
    Ok(blobs)
}
```

### 4. `fetch_azure_metadata()` - Full Implementation

```rust
/// Fetch metadata for an Azure blob
/// 
/// # Arguments
/// * `container` - Container name
/// * `blob_path` - Path to blob
/// 
/// # Returns
/// Metadata including size, content type, last modified, etc.
/// 
/// # Example
/// ```ignore
/// let meta = fetch_azure_metadata("data-container", "file.kore").await?;
/// println!("Size: {}, Type: {}", meta.size, meta.content_type);
/// ```
#[cfg(feature = "azure")]
pub async fn fetch_azure_metadata(container: &str, blob_path: &str) -> Result<AzureBlobMetadata> {
    let account_name = std::env::var("AZURE_STORAGE_ACCOUNT")
        .unwrap_or_else(|_| "devstoreaccount1".to_string());
    let account_key = std::env::var("AZURE_STORAGE_KEY")
        .or_else(|_| std::env::var("AZURE_STORAGE_ACCOUNT_KEY"))
        .map_err(|_| "AZURE_STORAGE_KEY or AZURE_STORAGE_ACCOUNT_KEY not set".to_string())?;
    
    let client = create_azure_client(&account_name, &account_key)?;
    let container_client = client.container_client(container);
    let blob_client = container_client.blob_client(blob_path);
    
    match blob_client.get_properties().await {
        Ok(properties) => {
            Ok(AzureBlobMetadata {
                name: blob_path.to_string(),
                container: container.to_string(),
                size: properties.blob.properties.content_length,
                content_type: properties.blob.properties.content_type.clone().unwrap_or_default(),
                last_modified: properties.blob.properties.last_modified,
                etag: properties.blob.properties.etag.clone(),
                storage_tier: properties.blob.properties.tier.clone(),
            })
        }
        Err(e) => Err(format!("Metadata fetch error: {}", e)),
    }
}
```

---

## 🧪 Testing Strategy

### 1. Azurite Emulator Setup

```bash
# Install Azurite (Azure Storage Emulator)
npm install -g azurite

# Start Azurite in background
azurite --silent &

# Or in Docker
docker run -p 10000:10000 mcr.microsoft.com/azure-storage/azurite
```

### 2. Integration Tests

```rust
#[cfg(all(test, feature = "azure"))]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_azure_read_write() {
        // Set up test environment
        std::env::set_var("AZURE_STORAGE_ACCOUNT", "devstoreaccount1");
        std::env::set_var("AZURE_STORAGE_KEY", "test-key");
        
        // Create container (using client)
        // Write test data
        let test_data = b"test blob content";
        assert!(write_to_azure("test", "test.bin", test_data).await.is_ok());
        
        // Read it back
        let result = read_from_azure("test", "test.bin").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_data);
    }
    
    #[tokio::test]
    async fn test_azure_metadata() {
        // Write test blob
        write_to_azure("test", "metadata.bin", b"data").await.unwrap();
        
        // Fetch metadata
        let meta = fetch_azure_metadata("test", "metadata.bin").await;
        assert!(meta.is_ok());
        let m = meta.unwrap();
        assert_eq!(m.size, 4);
    }
}
```

### 3. Performance Benchmarks

```rust
#[cfg(all(test, feature = "azure"))]
mod bench {
    use super::*;
    
    #[tokio::test]
    async fn bench_read_1mb() {
        let data = vec![0u8; 1024 * 1024]; // 1MB
        write_to_azure("bench", "1mb.bin", &data).await.unwrap();
        
        let start = std::time::Instant::now();
        let _ = read_from_azure("bench", "1mb.bin").await;
        println!("1MB read: {:?}", start.elapsed());
    }
}
```

---

## 📊 Success Criteria

- [ ] All 4 methods (read, write, list, metadata) working
- [ ] Integration tests passing with Azurite
- [ ] Error handling for all failure scenarios
- [ ] Retry logic with exponential backoff
- [ ] Performance within 10% of S3 implementation
- [ ] Full documentation with examples
- [ ] Support for all 4 auth methods

---

## 🚀 Completion Checklist

**By End of Week 1:**
- [ ] Azure SDK dependency added and tested
- [ ] `read_from_azure()` fully implemented and tested
- [ ] `write_to_azure()` fully implemented and tested
- [ ] Basic error handling in place

**By End of Week 2:**
- [ ] `list_azure_blobs()` fully implemented
- [ ] `fetch_azure_metadata()` fully implemented
- [ ] All 4 auth methods supported
- [ ] Integration tests with Azurite passing
- [ ] Performance benchmarks comparable to S3
- [ ] Documentation complete
- [ ] PR ready for review

---

## 📚 Resources

- [Azure SDK for Rust](https://github.com/Azure/azure-sdk-for-rust)
- [Azure Storage Blobs Crate](https://docs.rs/azure_storage_blobs/)
- [Azurite Emulator](https://github.com/Azure/Azurite)
- [S3 Reference Implementation](src/s3_reader.rs)
- [Azure Authentication](https://learn.microsoft.com/en-us/azure/developer/rust/authenticate)

---

**Phase 1 Complete = World's First Cloud-Native Columnar Format! 🎯**
