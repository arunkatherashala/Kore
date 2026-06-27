# 🌐 Kore AWS S3 Connector

Native AWS S3 integration for Kore columnar file format.

## Features

✅ **Native S3 Integration** — Read/write Kore files directly from S3  
✅ **Async/Await** — Non-blocking async I/O  
✅ **Local Caching** — Optional file caching for performance  
✅ **Error Handling** — Comprehensive error types  
✅ **Metadata Support** — Get file size, ETag, content type  
✅ **Multi-language** — Rust, Python, Java, JavaScript bindings  

## Quick Start

### Rust

**1. Add S3 feature to Cargo.toml:**

```toml
[dependencies]
kore_fileformat = { version = "1.0", features = ["s3"] }
```

**2. Configure AWS credentials:**

```bash
export AWS_ACCESS_KEY_ID=your_key
export AWS_SECRET_ACCESS_KEY=your_secret
export AWS_REGION=us-east-1
```

**3. Use S3Reader:**

```rust
use kore_fileformat::s3_reader::S3Reader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create reader
    let mut reader = S3Reader::new("us-east-1")?;
    
    // Enable caching (optional)
    reader.with_cache("./kore_cache")?;
    
    // Read file from S3
    let data = reader.read_file("my-bucket", "data/records.kore").await?;
    println!("Read {} bytes", data.len());
    
    // Write file to S3
    let payload = vec![1, 2, 3, 4, 5];
    reader.write_file("my-bucket", "output.kore", &payload).await?;
    
    // List files
    let files = reader.list_files("my-bucket", Some("data/")).await?;
    for file in files {
        println!("- {}", file);
    }
    
    // Get metadata
    let meta = reader.get_metadata("my-bucket", "data/records.kore").await?;
    println!("Size: {} bytes", meta.size);
    println!("ETag: {}", meta.etag);
    
    Ok(())
}
```

### Python

**1. Install kore with S3 support:**

```bash
pip install kore-fileformat[s3]
```

**2. Configure AWS credentials:**

```bash
export AWS_ACCESS_KEY_ID=your_key
export AWS_SECRET_ACCESS_KEY=your_secret
export AWS_REGION=us-east-1
```

**3. Use S3Reader:**

```python
import asyncio
from kore_s3 import S3Reader

async def main():
    # Create reader
    reader = S3Reader("us-east-1")
    
    # Enable caching
    await reader.enable_cache("./kore_cache")
    
    # Read file from S3
    data = await reader.read_file("my-bucket", "data/records.kore")
    print(f"Read {len(data)} bytes")
    
    # Write file to S3
    payload = b'\x01\x02\x03\x04\x05'
    await reader.write_file("my-bucket", "output.kore", payload)
    
    # List files
    files = await reader.list_files("my-bucket", prefix="data/")
    for file in files:
        print(f"- {file}")
    
    # Get metadata
    meta = await reader.get_metadata("my-bucket", "data/records.kore")
    print(f"Size: {meta.size} bytes")
    print(f"ETag: {meta.etag}")

asyncio.run(main())
```

## API Reference

### Rust API

```rust
impl S3Reader {
    /// Create reader for AWS region
    pub fn new(region: &str) -> Result<Self, S3Error>;
    
    /// Enable local file caching
    pub fn with_cache(&mut self, cache_dir: &str) -> Result<(), S3Error>;
    
    /// Read file from S3 (checks cache first)
    pub async fn read_file(&self, bucket: &str, key: &str) -> Result<Vec<u8>, S3Error>;
    
    /// Write file to S3 (updates cache)
    pub async fn write_file(&self, bucket: &str, key: &str, data: &[u8]) -> Result<(), S3Error>;
    
    /// List files in bucket with optional prefix
    pub async fn list_files(&self, bucket: &str, prefix: Option<&str>) 
        -> Result<Vec<String>, S3Error>;
    
    /// Get file metadata (size, ETag, content type)
    pub async fn get_metadata(&self, bucket: &str, key: &str) 
        -> Result<S3FileMetadata, S3Error>;
    
    /// Get configured region
    pub fn region(&self) -> &str;
    
    /// Check if caching is enabled
    pub fn cache_enabled(&self) -> bool;
}
```

### Python API

```python
class S3Reader:
    """AWS S3 connector for Kore."""
    
    def __init__(self, region: str) -> None:
        """Create reader for AWS region."""
    
    async def enable_cache(self, cache_dir: str) -> None:
        """Enable local file caching."""
    
    async def read_file(self, bucket: str, key: str) -> bytes:
        """Read file from S3 (checks cache first)."""
    
    async def write_file(self, bucket: str, key: str, data: bytes) -> None:
        """Write file to S3 (updates cache)."""
    
    async def list_files(self, bucket: str, prefix: Optional[str] = None) -> List[str]:
        """List files in bucket with optional prefix."""
    
    async def get_metadata(self, bucket: str, key: str) -> FileMetadata:
        """Get file metadata (size, ETag, content type)."""
```

## Error Handling

### Rust

```rust
use kore_fileformat::s3_reader::{S3Reader, S3Error};

match reader.read_file(bucket, key).await {
    Ok(data) => println!("Read {} bytes", data.len()),
    Err(S3Error::NotFound) => println!("File not found"),
    Err(S3Error::AuthenticationError) => println!("AWS auth failed"),
    Err(S3Error::InvalidPath) => println!("Invalid bucket/key"),
    Err(e) => println!("Error: {}", e),
}
```

### Python

```python
from kore_s3 import S3Reader, NotFound, AuthenticationError, InvalidPath

try:
    data = await reader.read_file(bucket, key)
except NotFound:
    print("File not found")
except AuthenticationError:
    print("AWS auth failed")
except InvalidPath:
    print("Invalid bucket/key")
```

## Authentication

### Environment Variables

```bash
export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY
export AWS_REGION=us-east-1
```

### IAM Policy (Minimal)

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:PutObject",
        "s3:ListBucket"
      ],
      "Resource": [
        "arn:aws:s3:::my-bucket",
        "arn:aws:s3:::my-bucket/*"
      ]
    }
  ]
}
```

## Caching

S3Reader supports optional local caching to reduce API calls:

```rust
let mut reader = S3Reader::new("us-east-1")?;

// Enable caching in ./cache directory
reader.with_cache("./cache")?;

// First call: downloads from S3
let data1 = reader.read_file("bucket", "file.kore").await?;

// Second call: reads from cache
let data2 = reader.read_file("bucket", "file.kore").await?;

println!("Same data: {}", data1 == data2);
```

## Performance Tips

1. **Enable Caching** — Dramatically reduces S3 API calls
2. **Batch Operations** — Use `list_files()` to batch process
3. **Region Selection** — Use same region as data for latency
4. **Concurrent Reads** — Use Tokio for parallel downloads

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
| 1 | AWS S3 Connector | ✅ In Progress |
| 2 | Python Bindings | ⏳ Pending |
| 3 | Java Bindings | ⏳ Pending |
| 4 | JavaScript Bindings | ⏳ Pending |
| 5 | Azure Blob Storage | ⏳ Planned |
| 6 | Google Cloud Storage | ⏳ Planned |
| 7 | Snowflake Connector | ⏳ Planned |

## Examples

See `examples/s3_connector.rs` for a complete runnable example:

```bash
cargo run --example s3_connector --features s3
```

## License

Same as Kore library. See LICENSE file.

## Support

For issues, questions, or feature requests, visit:
https://github.com/arunkatherashala/Kore/issues
