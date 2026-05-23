# Cloud MVP: Implementation Plan

## Phase 1: Foundation (Week 1)

### 1.1 Project Setup
- [ ] Initialize Node.js/Rust project
- [ ] Setup Express.js or Actix framework
- [ ] Configure PostgreSQL connection
- [ ] Setup S3 SDK integration

### 1.2 Database Schema
```sql
CREATE TABLE kore_files (
  id UUID PRIMARY KEY,
  bucket VARCHAR,
  key VARCHAR,
  size BIGINT,
  compressed_ratio DECIMAL,
  row_count BIGINT,
  column_count INT,
  created_at TIMESTAMP,
  updated_at TIMESTAMP
);

CREATE TABLE query_cache (
  id UUID PRIMARY KEY,
  file_id UUID REFERENCES kore_files,
  query_hash VARCHAR UNIQUE,
  result BYTEA,
  ttl INT,
  created_at TIMESTAMP
);
```

### 1.3 Environment Setup
- [ ] Docker Compose (PostgreSQL + API + S3 mock)
- [ ] .env configuration
- [ ] Logging setup
- [ ] Health check endpoints

---

## Phase 2: Core API (Week 2)

### 2.1 REST Endpoints (20+)

**File Operations:**
- `POST /files` - Upload Kore file to S3
- `GET /files` - List all files
- `GET /files/{id}` - Get file metadata
- `DELETE /files/{id}` - Delete file
- `PUT /files/{id}` - Update metadata

**Query Operations:**
- `POST /query` - Execute query
- `GET /query/{id}` - Get query result
- `POST /query/batch` - Batch queries

**S3 Operations:**
- `GET /s3/signed-url` - Generate signed download URL
- `POST /s3/multipart` - Start multipart upload
- `POST /s3/multipart/{uploadId}/complete` - Complete upload

**Statistics:**
- `GET /stats/compression` - Compression metrics
- `GET /stats/queries` - Query performance
- `GET /health` - Service health

### 2.2 Request/Response Models
- FileUploadRequest, FileMetadata
- QueryRequest, QueryResult
- BatchQueryRequest, BatchQueryResponse
- ErrorResponse (standard error format)

### 2.3 Error Handling
- [ ] Validation errors (400)
- [ ] Authentication errors (401)
- [ ] Not found (404)
- [ ] Rate limiting (429)
- [ ] Server errors (500)

---

## Phase 3: S3 Integration (Week 3)

### 3.1 Upload Pipeline
- [ ] Stream upload from client
- [ ] Multipart upload for large files
- [ ] Verify Kore format before S3 write
- [ ] Store metadata in PostgreSQL

### 3.2 Download Pipeline
- [ ] Signed URL generation (1-hour expiry)
- [ ] Stream download from S3
- [ ] Verify file integrity (checksum)

### 3.3 Streaming
- [ ] Backpressure handling
- [ ] Memory-efficient reads
- [ ] Connection timeout handling

---

## Phase 4: Query Engine (Week 4)

### 4.1 Column Projection
- [ ] Parse column list from query
- [ ] Skip unnecessary columns
- [ ] Reduce I/O overhead

### 4.2 Predicate Pushdown
- [ ] Parse WHERE clause
- [ ] Push filters to S3 Select (if supported)
- [ ] Fallback to in-memory filtering

### 4.3 Optimization
- [ ] Query plan caching
- [ ] Result caching (PostgreSQL)
- [ ] Index metadata on column types

---

## Phase 5: Performance & Testing (Week 5)

### 5.1 Benchmarks
- [ ] 1MB file upload/download
- [ ] 10MB query execution
- [ ] 100K row scan
- [ ] Batch query throughput

### 5.2 Integration Tests
- [ ] End-to-end upload → query → download
- [ ] Error scenarios
- [ ] Concurrent requests
- [ ] Load testing (1000+ concurrent)

### 5.3 Documentation
- [ ] API reference (OpenAPI/Swagger)
- [ ] Architecture diagram
- [ ] Deployment guide
- [ ] Performance tuning guide

---

## Acceptance Criteria
- ✅ All 20+ endpoints working
- ✅ S3 integration stable
- ✅ Queries execute in <100ms p95
- ✅ Throughput >10k req/sec
- ✅ Zero data loss in roundtrip
- ✅ 95% uptime in load testing

## Status: Ready for Phase 1
