# Phase 5: Cloud Storage Connectors

**Status:** 🚀 In Progress  
**Timeline:** 2-3 weeks  
**Target:** S3, GCS, Azure Blob Storage support  

## Overview

Seamless cloud storage integration enables:
- ✅ Read/write directly from cloud buckets
- ✅ Streaming uploads/downloads
- ✅ Automatic credential handling
- ✅ Multi-cloud support

## Architecture

```
Application Code
    ↓
Kore Cloud Connectors
    ├─ S3 Reader/Writer
    ├─ GCS Reader/Writer
    └─ Azure Reader/Writer
    ↓
Cloud Storage Service
```

## Cloud Providers

### AWS S3
```python
from cloud_connectors import KoreS3Reader, KoreS3Writer

reader = KoreS3Reader(bucket="my-bucket", region="us-west-2")
df = reader.read("s3://my-bucket/data.kore")

writer = KoreS3Writer(bucket="my-bucket")
writer.write(df, "s3://my-bucket/output.kore")
```

### Google Cloud Storage
```python
from cloud_connectors import KoreGCSReader, KoreGCSWriter

reader = KoreGCSReader(project="my-project", bucket="my-bucket")
df = reader.read("gs://my-bucket/data.kore")

writer = KoreGCSWriter(project="my-project", bucket="my-bucket")
writer.write(df, "gs://my-bucket/output.kore")
```

### Azure Blob Storage
```python
from cloud_connectors import KoreAzureReader, KoreAzureWriter

conn_str = "DefaultEndpointsProtocol=https;..."
reader = KoreAzureReader(connection_string=conn_str, container="data")
df = reader.read("data.kore")

writer = KoreAzureWriter(connection_string=conn_str, container="output")
writer.write(df, "output.kore")
```

## Implementation Phases

### Phase 5A: AWS S3 (Week 1)
- [ ] S3 client setup
- [ ] Multipart download
- [ ] Multipart upload
- [ ] IAM/STS integration
- [ ] Testing with real S3

### Phase 5B: Google Cloud Storage (Week 2)
- [ ] GCS client setup
- [ ] Streaming operations
- [ ] Signed URLs
- [ ] Testing with real GCS

### Phase 5C: Azure Blob Storage (Week 2)
- [ ] Azure SDK setup
- [ ] Blob operations
- [ ] SAS token support
- [ ] Testing with real Azure

## Performance Targets

| Operation | Size | Latency | Throughput |
|-----------|------|---------|-----------|
| S3 Read | 100MB | 2-3s | 30-50 MB/s |
| S3 Write | 100MB | 2-3s | 30-50 MB/s |
| GCS Read | 100MB | 1-2s | 50-100 MB/s |
| GCS Write | 100MB | 1-2s | 50-100 MB/s |

## Dependencies

- boto3 (AWS)
- google-cloud-storage (GCS)
- azure-storage-blob (Azure)

## Roadmap

- [ ] Project structure
- [ ] S3 reader/writer
- [ ] GCS reader/writer
- [ ] Azure reader/writer
- [ ] Credential management
- [ ] Multipart handling
- [ ] Error recovery
- [ ] Unit tests
- [ ] Integration tests
- [ ] Benchmarks
- [ ] Documentation

## Testing

```bash
# Local testing
pytest tests/

# Real cloud testing (requires credentials)
pytest tests/ -m cloud
```

## Known Limitations

- Requires cloud credentials
- Network-dependent performance
- Large file handling requires streaming

## Contributors

Assigned for Phase 5 development.

---

**Next:** Begin with S3 reader/writer implementation.
