# PROJECT 2: KORE CLOUD MVP - ARCHITECTURE

**Goal:** Cloud backend for querying Kore files from S3  
**Timeline:** May 22-31 (10 days)  
**Target:** 20 REST endpoints, 1000 QPS, sub-100ms response

---

## 🏗️ CLOUD ARCHITECTURE

```
┌─────────────────────────────────────────────────────────┐
│ CLIENT (Web, Mobile, CLI)                               │
└────────────────────┬────────────────────────────────────┘
                     │ HTTPS REST API
                     ▼
┌─────────────────────────────────────────────────────────┐
│ API GATEWAY (AWS API Gateway or nginx)                  │
│  • Rate limiting (100 req/s per user)                   │
│  • Authentication (JWT)                                 │
│  • Request logging                                      │
└────────────────────┬────────────────────────────────────┘
                     │
        ┌────────────┼────────────┐
        ▼            ▼            ▼
   ┌────────┐  ┌────────┐  ┌────────┐
   │ Query  │  │ Upload │  │ Admin  │
   │ Service│  │ Service│  │ API    │
   └────────┘  └────────┘  └────────┘
        │            │            │
        └────────────┼────────────┘
                     │
        ┌────────────┼────────────┐
        ▼            ▼            ▼
    ┌───────┐   ┌────────┐  ┌──────────┐
    │ S3    │   │ Cache  │  │ Database │
    │(Files)│   │ (Redis)│  │(Postgres)│
    └───────┘   └────────┘  └──────────┘
```

---

## 📡 REST API ENDPOINTS (20+)

### File Operations (6 endpoints)
```
POST   /api/v1/files/upload              → Upload Kore file to S3
GET    /api/v1/files                     → List all files
GET    /api/v1/files/{file_id}           → Get file metadata
DELETE /api/v1/files/{file_id}           → Delete file
GET    /api/v1/files/{file_id}/preview   → Preview first 100 rows
GET    /api/v1/files/{file_id}/schema    → Get column schema
```

### Query Execution (7 endpoints)
```
POST   /api/v1/query/execute             → Execute SQL-like query
POST   /api/v1/query/async               → Start async query
GET    /api/v1/query/{query_id}/status   → Check query status
GET    /api/v1/query/{query_id}/result   → Get query results
POST   /api/v1/query/{query_id}/cancel   → Cancel running query
GET    /api/v1/query/history             → Query execution history
POST   /api/v1/query/analyze             → Analyze query performance
```

### User & Auth (4 endpoints)
```
POST   /api/v1/auth/signup               → Create new account
POST   /api/v1/auth/login                → Login (get JWT)
POST   /api/v1/auth/refresh              → Refresh JWT token
POST   /api/v1/auth/logout               → Revoke token
```

### Admin & Monitoring (4 endpoints)
```
GET    /api/v1/admin/stats               → System statistics
GET    /api/v1/admin/health              → Health check
GET    /api/v1/admin/logs                → System logs
GET    /api/v1/metrics                   → Prometheus metrics
```

---

## 🗄️ DATA MODEL

### PostgreSQL Schema

```sql
-- Users
CREATE TABLE users (
    id UUID PRIMARY KEY,
    email VARCHAR UNIQUE NOT NULL,
    password_hash VARCHAR NOT NULL,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    plan VARCHAR DEFAULT 'free'  -- free, pro, enterprise
);

-- Files (metadata)
CREATE TABLE files (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    s3_key VARCHAR NOT NULL,  -- s3://bucket/path/file.kore
    file_name VARCHAR NOT NULL,
    file_size_bytes INT NOT NULL,
    num_rows INT NOT NULL,
    num_columns INT NOT NULL,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    is_public BOOLEAN DEFAULT FALSE,
    description TEXT
);

-- File Schema (columns)
CREATE TABLE file_columns (
    id UUID PRIMARY KEY,
    file_id UUID NOT NULL REFERENCES files(id),
    column_index INT NOT NULL,
    column_name VARCHAR NOT NULL,
    data_type VARCHAR NOT NULL,  -- int, float, string, etc
    nullable BOOLEAN DEFAULT TRUE
);

-- Query Execution Log
CREATE TABLE query_executions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    file_id UUID NOT NULL REFERENCES files(id),
    query_text TEXT NOT NULL,
    status VARCHAR,  -- running, completed, failed, cancelled
    rows_scanned INT,
    rows_returned INT,
    execution_time_ms INT,
    created_at TIMESTAMP DEFAULT NOW(),
    completed_at TIMESTAMP,
    error_message TEXT
);

-- Rate Limiting
CREATE TABLE rate_limits (
    user_id UUID PRIMARY KEY REFERENCES users(id),
    requests_today INT DEFAULT 0,
    last_reset TIMESTAMP DEFAULT NOW(),
    daily_limit INT DEFAULT 1000  -- free plan
);
```

---

## 🔍 QUERY ENGINE

### Simple SQL Support
```
Query: SELECT col1, col2, col3 FROM file1 WHERE col4 > 100

Parse → Validate → Plan → Execute → Format

1. Parse: Extract SELECT/FROM/WHERE
2. Validate: Check columns exist in schema
3. Plan: 
   - Read only needed columns from S3 (col1, col2, col3, col4)
   - Use Kore format to skip metadata
   - Load into memory as DataFrame
4. Execute:
   - Filter rows where col4 > 100
   - Project only col1, col2, col3
5. Format: Return JSON
```

### Filter Pushdown (Performance)
```
Traditional way (SLOW):
  1. Load entire file (1GB)
  2. Filter in memory
  3. Return 1000 rows

Kore way (FAST):
  1. Read Kore metadata (column offsets)
  2. Read only needed columns
  3. Filter during read (zero-copy)
  4. Return 1000 rows

Result: 90% less data read!
```

### Execution Model
```rust
pub struct QueryEngine {
    s3_client: S3Client,
    cache: Redis,
}

impl QueryEngine {
    pub async fn execute(&self, query: &str) -> QueryResult {
        // 1. Parse
        let ast = parse_sql(query)?;
        
        // 2. Validate
        let schema = self.load_schema(&ast.from)?;
        self.validate_columns(&ast, &schema)?;
        
        // 3. Plan
        let columns_needed = self.extract_columns(&ast);
        
        // 4. Execute
        let mut df = DataFrame::new();
        for column in columns_needed {
            let data = self.read_column(&ast.from, column).await?;
            df.add_column(data);
        }
        
        // 5. Filter
        if let Some(where_clause) = &ast.where_clause {
            df = df.filter(where_clause)?;
        }
        
        Ok(df.to_json())
    }
}
```

---

## 💾 S3 INTEGRATION

### S3 Abstraction Layer
```rust
pub struct S3Storage {
    client: S3Client,
    bucket: String,
}

impl S3Storage {
    // Upload Kore file
    pub async fn upload(&self, file_path: &str, user_id: &str) -> String {
        let key = format!("users/{}/files/{}", user_id, file_path);
        self.client.put_object(&self.bucket, &key, file_data).await?;
        Ok(key)
    }
    
    // Download range (RFC 7233)
    pub async fn read_range(&self, key: &str, offset: u64, length: u64) -> Vec<u8> {
        self.client
            .get_object(&self.bucket, key)
            .range(offset, offset + length)
            .await?
    }
    
    // List files by user
    pub async fn list_files(&self, user_id: &str) -> Vec<String> {
        self.client
            .list_objects(&self.bucket, &format!("users/{}/files/", user_id))
            .await?
    }
}
```

### Connection Pooling
```rust
// Cargo.toml
[dependencies]
aws-sdk-s3 = "0.42"
deadpool = "0.11"  // Connection pooling

// Use pooled connections
let pool = create_s3_pool(10)?;  // 10 concurrent connections
let mut conn = pool.get().await?;
conn.put_object(...).await?;
```

---

## 🚀 PERFORMANCE TARGETS

```
Operation          | Target        | Metric
---|---|---
Query execution    | < 100 ms      | 95th percentile
File upload        | 100 MB/s      | Throughput
S3 read (1MB)      | < 50 ms       | Latency (cold cache)
Schema fetch       | < 10 ms       | Metadata read
Concurrent queries | 1000 QPS      | Throughput
Connection pool    | 10-50 size    | Optimal size
Cache hit ratio    | > 80%         | Redis effectiveness
```

---

## 📊 MONITORING & OBSERVABILITY

### Metrics (Prometheus)
```
kore_cloud_queries_total{status="completed|failed"}
kore_cloud_query_duration_seconds{quantile="0.5|0.95|0.99"}
kore_cloud_s3_operations_seconds{operation="read|write|delete"}
kore_cloud_api_errors_total{endpoint, status_code}
kore_cloud_cache_hit_ratio
kore_cloud_concurrent_queries
```

### Logging
```
INFO: Query execution started (query_id, user_id)
INFO: S3 read complete (key, bytes, latency_ms)
WARN: Cache miss rate > 20%
ERROR: Query failed (query_id, error_message)
```

---

## 🔐 SECURITY LAYER

### Authentication
```rust
// JWT-based authentication
#[derive(Debug, Deserialize)]
struct Claims {
    user_id: String,
    exp: u64,
}

middleware::use_auth(|req| {
    let token = extract_bearer_token(&req)?;
    let claims = verify_jwt(&token)?;
    req.extensions_mut().insert(claims);
});
```

### Rate Limiting
```rust
// Per-user daily limit
// Free: 1,000 queries/day
// Pro: 100,000 queries/day
// Enterprise: unlimited

middleware::rate_limit(|user| {
    get_user_plan(user).daily_limit()
});
```

### S3 Security
```
- Upload: Only to user's own prefix (users/{user_id}/files/)
- Download: Only files user owns
- Delete: Only by file owner or admin
- Public sharing: Explicit opt-in per file
```

---

## ⏱️ IMPLEMENTATION PHASES

### Phase 2A: S3 Layer (May 22-24)
```
1. S3 client setup + connection pool
2. Upload/download/delete operations
3. Range request support (RFC 7233)
4. Integration tests with moto
```

### Phase 2B: REST API (May 25-27)
```
1. Framework setup (Axum + Tokio)
2. 20 endpoints boilerplate
3. Request/response serialization
4. Error handling
```

### Phase 2C: Database & Query (May 28-31)
```
1. PostgreSQL setup
2. Query execution engine
3. Schema inference from Kore
4. Performance optimization
```

---

## ✅ SUCCESS CRITERIA

- ✅ 20+ REST endpoints working
- ✅ 1000 concurrent queries (sub-100ms latency)
- ✅ S3 integration tested
- ✅ Authentication working
- ✅ Rate limiting enforced
- ✅ Complete API documentation (OpenAPI/Swagger)
- ✅ Load testing (1000 QPS demonstrated)

---

**ARCHITECTURE COMPLETE** ✅  
Ready for implementation starting May 22
