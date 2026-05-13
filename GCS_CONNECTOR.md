# 🌐 Kore Google Cloud Storage Connector

Native Google Cloud Storage integration for Kore columnar file format.

## Features

✅ **Native GCS Integration** — Read/write Kore files directly from Google Cloud Storage  
✅ **Async/Await** — Non-blocking async I/O  
✅ **Local Caching** — Optional file caching for performance  
✅ **Error Handling** — Comprehensive error types  
✅ **Metadata Support** — Get object size, generation ID, content type  
✅ **Multi-language** — Python, Java, JavaScript bindings (planned)  

## Quick Start

### Rust

**1. Add GCS feature to Cargo.toml:**

```toml
[dependencies]
kore_fileformat = { version = "1.0", features = ["gcs"] }
```

**2. Configure GCS credentials:**

```bash
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
export GCP_PROJECT_ID=my-project
```

**3. Use GcsReader:**

```rust
use kore_fileformat::gcs_reader::GcsReader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create reader for GCP project
    let mut reader = GcsReader::new("my-project")?;
    
    // Enable caching (optional)
    reader.with_cache("./gcs_cache")?;
    
    // Read object from bucket
    let data = reader.read_file("my-bucket", "data/records.kore").await?;
    println!("Read {} bytes", data.len());
    
    // Write object to bucket
    let payload = vec![1, 2, 3, 4, 5];
    reader.write_file("my-bucket", "output.kore", &payload).await?;
    
    // List objects in bucket
    let objects = reader.list_objects("my-bucket", Some("data/")).await?;
    for obj in objects {
        println!("- {}", obj);
    }
    
    // Get object metadata
    let meta = reader.get_metadata("my-bucket", "data/records.kore").await?;
    println!("Size: {} bytes", meta.size);
    println!("Generation: {}", meta.generation);
    
    Ok(())
}
```

## API Reference

```rust
impl GcsReader {
    /// Create reader for GCP project
    pub fn new(project_id: &str) -> Result<Self, GcsError>;
    
    /// Enable local file caching
    pub fn with_cache(&mut self, cache_dir: &str) -> Result<(), GcsError>;
    
    /// Read object from bucket (checks cache first)
    pub async fn read_file(&self, bucket: &str, object_path: &str) 
        -> Result<Vec<u8>, GcsError>;
    
    /// Write object to bucket (updates cache)
    pub async fn write_file(&self, bucket: &str, object_path: &str, data: &[u8]) 
        -> Result<(), GcsError>;
    
    /// List objects in bucket with optional prefix
    pub async fn list_objects(&self, bucket: &str, prefix: Option<&str>) 
        -> Result<Vec<String>, GcsError>;
    
    /// Get object metadata (size, generation, content type)
    pub async fn get_metadata(&self, bucket: &str, object_path: &str) 
        -> Result<GcsObjectMetadata, GcsError>;
    
    /// Get GCP project ID
    pub fn project_id(&self) -> &str;
    
    /// Check if caching is enabled
    pub fn cache_enabled(&self) -> bool;
}
```

## Error Handling

```rust
use kore_fileformat::gcs_reader::{GcsReader, GcsError};

match reader.read_file(bucket, object_path).await {
    Ok(data) => println!("Read {} bytes", data.len()),
    Err(GcsError::NotFound) => println!("Object not found"),
    Err(GcsError::AuthenticationError) => println!("GCS auth failed"),
    Err(GcsError::InvalidPath) => println!("Invalid bucket/path"),
    Err(e) => println!("Error: {}", e),
}
```

## Authentication

### Service Account (Recommended)

```bash
# Download service account key from GCP Console
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
```

### User Credentials

```bash
# Using gcloud CLI
gcloud auth application-default login
```

### Workload Identity (GKE)

```rust
// Automatically uses workload identity on GKE
let reader = GcsReader::new("my-project")?;
```

## Required Permissions

Minimal IAM roles for service account:

```yaml
roles/storage.objectViewer    # Read objects
roles/storage.objectCreator   # Write objects
roles/storage.objectDeleter   # Delete objects (if needed)
```

Or custom role with permissions:
- `storage.objects.get`
- `storage.objects.list`
- `storage.objects.create`
- `storage.objects.delete` (optional)

## Performance Tips

1. **Enable Caching** — Reduces GCS API calls and egress charges
2. **Batch Operations** — Use `list_objects()` for bulk processing
3. **Region Selection** — Use same region as compute for lower latency
4. **Concurrent Reads** — Leverage async/await for parallel downloads

```rust
// Concurrent reads using Tokio
let futures = keys.iter().map(|key| {
    reader.read_file("bucket", key)
});

let results = futures::future::join_all(futures).await;
```

## Roadmap

| Phase | Feature | Status |
|-------|---------|--------|
| 1 | GCS SDK Integration | ⏳ Pending |
| 2 | Python Bindings | ⏳ Pending |
| 3 | Java Bindings | ⏳ Pending |
| 4 | JavaScript Bindings | ⏳ Pending |
| 5 | Workload Identity Support | ⏳ Pending |

## License

Same as Kore library. See LICENSE file.

## Support

For issues, questions, or feature requests, visit:
https://github.com/arunkatherashala/Kore/issues
