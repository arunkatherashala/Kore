# 🌐 Kore Azure Blob Storage Connector

Native Azure Blob Storage integration for Kore columnar file format.

## Features

✅ **Native Azure Integration** — Read/write Kore files directly from Azure Blob Storage  
✅ **Async/Await** — Non-blocking async I/O  
✅ **Local Caching** — Optional file caching for performance  
✅ **Error Handling** — Comprehensive error types  
✅ **Metadata Support** — Get blob size, ETag, content type  
✅ **Multi-language** — Python, Java, JavaScript bindings (planned)  

## Quick Start

### Rust

**1. Add Azure feature to Cargo.toml:**

```toml
[dependencies]
kore_fileformat = { version = "1.0", features = ["azure"] }
```

**2. Configure Azure credentials:**

```bash
export AZURE_STORAGE_ACCOUNT_NAME=mystorageaccount
export AZURE_STORAGE_ACCOUNT_KEY=your_account_key
```

**3. Use AzureBlobReader:**

```rust
use kore_fileformat::azure_reader::AzureBlobReader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create reader with storage account credentials
    let mut reader = AzureBlobReader::new("mystorageaccount", "account_key")?;
    
    // Enable caching (optional)
    reader.with_cache("./azure_cache")?;
    
    // Read blob from container
    let data = reader.read_file("mycontainer", "data/records.kore").await?;
    println!("Read {} bytes", data.len());
    
    // Write blob to container
    let payload = vec![1, 2, 3, 4, 5];
    reader.write_file("mycontainer", "output.kore", &payload).await?;
    
    // List blobs in container
    let blobs = reader.list_blobs("mycontainer", Some("data/")).await?;
    for blob in blobs {
        println!("- {}", blob);
    }
    
    // Get blob metadata
    let meta = reader.get_metadata("mycontainer", "data/records.kore").await?;
    println!("Size: {} bytes", meta.size);
    println!("ETag: {}", meta.etag);
    
    Ok(())
}
```

## API Reference

```rust
impl AzureBlobReader {
    /// Create reader with storage account credentials
    pub fn new(storage_account: &str, account_key: &str) -> Result<Self, AzureError>;
    
    /// Enable local file caching
    pub fn with_cache(&mut self, cache_dir: &str) -> Result<(), AzureError>;
    
    /// Read blob from container (checks cache first)
    pub async fn read_file(&self, container: &str, blob_path: &str) 
        -> Result<Vec<u8>, AzureError>;
    
    /// Write blob to container (updates cache)
    pub async fn write_file(&self, container: &str, blob_path: &str, data: &[u8]) 
        -> Result<(), AzureError>;
    
    /// List blobs in container with optional prefix
    pub async fn list_blobs(&self, container: &str, prefix: Option<&str>) 
        -> Result<Vec<String>, AzureError>;
    
    /// Get blob metadata (size, ETag, content type)
    pub async fn get_metadata(&self, container: &str, blob_path: &str) 
        -> Result<AzureBlobMetadata, AzureError>;
    
    /// Get storage account name
    pub fn storage_account(&self) -> &str;
    
    /// Check if caching is enabled
    pub fn cache_enabled(&self) -> bool;
}
```

## Error Handling

```rust
use kore_fileformat::azure_reader::{AzureBlobReader, AzureError};

match reader.read_file(container, blob_path).await {
    Ok(data) => println!("Read {} bytes", data.len()),
    Err(AzureError::NotFound) => println!("Blob not found"),
    Err(AzureError::AuthenticationError) => println!("Azure auth failed"),
    Err(AzureError::InvalidPath) => println!("Invalid container/path"),
    Err(e) => println!("Error: {}", e),
}
```

## Authentication

### Connection String

```bash
export AZURE_STORAGE_CONNECTION_STRING="DefaultEndpointsProtocol=https;AccountName=...;AccountKey=...;EndpointSuffix=core.windows.net"
```

### Storage Account Key

```bash
export AZURE_STORAGE_ACCOUNT_NAME=mystorageaccount
export AZURE_STORAGE_ACCOUNT_KEY=your_account_key
```

### Managed Identity (Azure VMs)

```rust
// Automatically uses VM's managed identity
let reader = AzureBlobReader::with_managed_identity("mystorageaccount")?;
```

## Roadmap

| Phase | Feature | Status |
|-------|---------|--------|
| 1 | Azure SDK Integration | ⏳ Pending |
| 2 | Python Bindings | ⏳ Pending |
| 3 | Java Bindings | ⏳ Pending |
| 4 | JavaScript Bindings | ⏳ Pending |
| 5 | Managed Identity Support | ⏳ Pending |

## License

Same as Kore library. See LICENSE file.

## Support

For issues, questions, or feature requests, visit:
https://github.com/arunkatherashala/Kore/issues
