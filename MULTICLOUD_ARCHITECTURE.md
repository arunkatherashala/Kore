# Multi-Cloud Architecture & Integration Guide

## 🏗️ System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       Kore Cloud API                            │
│  (Rust + Tokio + Axum)                                          │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ REST Endpoints:                                          │   │
│  │ POST /api/v1/upload                                      │   │
│  │ GET /api/v1/files/{id}                                   │   │
│  │ DELETE /api/v1/files/{id}                                │   │
│  │ GET /api/v1/status                                       │   │
│  └──────────────────────────────────────────────────────────┘   │
└──────┬──────────────────────────────┬──────────────────────────┘
       │ AppState contains:           │
       │ - StorageBackend (Arc)      │
       │ - Database (Arc, Optional)  │
       │                              │
┌──────▼──────┐  ┌──────────────────▼─────┐
│  Storage    │  │  PostgreSQL             │
│ Abstraction │  │  Database               │
│  (Trait)    │  │  - File records         │
└──────┬──────┘  │  - Upload sessions      │
       │         │  - Statistics           │
   ┌───┼────┐    │  - Audit logs           │
   │   │    │    └────────────────────────┘
┌──▼──▼───▼──┐
│            │
│ S3 (AWS)   │
│ ↓          │
│ Blob       │
│ Storage    │
│ (Azure)    │
│ ↓          │
│ Cloud      │
│ Storage    │
│ (GCP)      │
└────────────┘
```

## 🔄 Request Flow

### 1. Upload File Request

```
Client Request (Multipart)
    ↓
Axum Handler (src/main.rs)
    ↓
StorageBackend::upload_file()
    ├─ AWS: rusoto_s3 client
    ├─ Azure: azure_storage client
    └─ GCP: google_cloud_storage client
    ↓
Cloud Provider (S3/Blob/GCS)
    ↓
Database::insert_file() [Optional]
    ├─ Insert file record
    ├─ Update statistics
    └─ Audit log entry
    ↓
Response to Client (200 OK + file ID)
```

### 2. Failover Flow (if primary fails)

```
CloudProvider::AWS fails
    ↓ (connection timeout/unauthorized)
Retry with CloudProvider::Azure
    ↓ (also fails)
Retry with CloudProvider::GCP
    ↓ (succeeds)
Log failover event
    ↓
Resume normal operation on GCP
    ↓
Monitor and switch back when AWS recovers
```

## 📦 Module Organization

### kore-cloud/src/

```
src/
├── main.rs
│   ├── AppState struct
│   ├── REST endpoints
│   ├── Health check
│   └── Server initialization
│
├── storage.rs
│   ├── StorageBackend trait
│   ├── S3StorageBackend
│   ├── LocalStorageBackend
│   └── from_env() factory
│
├── cloud_providers.rs
│   ├── CloudProvider enum
│   ├── AzureBlobStorageBackend (feature: "azure")
│   ├── GCPCloudStorageBackend (feature: "gcp")
│   ├── MultiCloudConfig
│   └── create_cloud_storage() factory
│
├── db.rs (feature: "postgres")
│   ├── Database struct
│   ├── FileRecord struct
│   ├── UploadSession struct
│   ├── Stats struct
│   ├── All CRUD operations
│   └── Migration logic
│
└── lib.rs
    └── Module declarations
```

## ⚙️ Feature Gates

### Cargo.toml Configuration

```toml
[features]
default = []

# AWS Support (always enabled for backward compatibility)
s3 = ["rusoto_s3", "chrono"]

# Database Support
postgres = ["sqlx", "async-trait"]

# Azure Support
azure = ["azure_storage"]

# GCP Support
gcp = ["google_cloud_storage"]

# Full Features
full = ["s3", "postgres", "azure", "gcp"]
```

### Build Configurations

```bash
# S3 only (legacy)
cargo build --features s3

# PostgreSQL only
cargo build --features postgres

# AWS + PostgreSQL
cargo build --features "s3,postgres"

# Multi-cloud (all providers)
cargo build --features "s3,postgres,azure,gcp"

# Full production build
cargo build --release --features "s3,postgres,azure,gcp"
```

## 🌍 Environment Variables

### Core Configuration

```bash
# Server
PORT=8000
HOST=0.0.0.0

# Logging
RUST_LOG=info  # debug, info, warn, error

# Primary Storage Provider
STORAGE_BACKEND=s3  # s3, azure, gcp, local

# Failover Strategy
FAILOVER_PROVIDERS=azure,gcp
CROSS_REGION_REPLICATION=true
DATA_RESIDENCY=us
```

### AWS Configuration (S3)

```bash
AWS_REGION=us-east-1
AWS_S3_BUCKET=kore-storage-prod
# AWS_ACCESS_KEY_ID (from AWS CLI config)
# AWS_SECRET_ACCESS_KEY (from AWS CLI config)
```

### Azure Configuration (Blob Storage)

```bash
AZURE_STORAGE_ACCOUNT=korestgprodxxxxxxxx
AZURE_STORAGE_KEY=xxxxxxxxxxxxx
AZURE_STORAGE_CONTAINER=kore-files
```

### GCP Configuration (Cloud Storage)

```bash
GCP_BUCKET_NAME=kore-storage-project-prod
GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
```

### PostgreSQL Configuration

```bash
DATABASE_URL=postgresql://koremaster:password@host:5432/kore
DATABASE_POOL_SIZE=5  # Connection pool
```

## 🔗 Integration Steps

### Step 1: Update Cargo.toml

```toml
[dependencies]
# ... existing dependencies ...

# Cloud providers
azure_storage = { version = "0.20", optional = true }
google_cloud_storage = { version = "0.10", optional = true }

# Database
sqlx = { version = "0.7", features = [...], optional = true }

[features]
# ... existing features ...
azure = ["azure_storage"]
gcp = ["google_cloud_storage"]
postgres = ["sqlx"]
```

### Step 2: Update main.rs

```rust
// Add module declarations
mod storage;
mod cloud_providers;
#[cfg(feature = "postgres")]
mod db;

use cloud_providers::create_cloud_storage;

// In initialization
let storage = create_cloud_storage(primary_provider).await?;

// Initialize database (optional)
#[cfg(feature = "postgres")]
let database = db::Database::new(&db_url).await?;
```

### Step 3: Update AppState

```rust
pub struct AppState {
    storage: Arc<dyn StorageBackend>,
    #[cfg(feature = "postgres")]
    database: Option<Arc<db::Database>>,
}

impl AppState {
    pub async fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            storage,
            #[cfg(feature = "postgres")]
            database: None,
        }
    }

    #[cfg(feature = "postgres")]
    pub async fn with_database(
        storage: Arc<dyn StorageBackend>,
        database: db::Database,
    ) -> Self {
        Self {
            storage,
            database: Some(Arc::new(database)),
        }
    }
}
```

### Step 4: Update Request Handlers

```rust
// Example: /api/v1/upload handler
async fn upload_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, AppError> {
    // Extract file from multipart
    let file = extract_file(&mut multipart).await?;
    
    // Upload to storage (handles failover automatically)
    let etag = state.storage.upload_file(&file).await?;
    
    // Save to database (if enabled)
    #[cfg(feature = "postgres")]
    if let Some(db) = &state.database {
        db.insert_file(&file, &etag).await?;
    }
    
    Ok(Json(UploadResponse { 
        file_id: file.id, 
        etag,
    }))
}
```

## 🧪 Testing Multi-Cloud Setup

### 1. Local Testing with Docker Compose

```bash
# Start local environment (PostgreSQL, MinIO, etc.)
docker-compose up -d

# Set environment variables
export STORAGE_BACKEND=local
export DATABASE_URL=postgresql://kore:kore_password@localhost:5432/kore
export RUST_LOG=debug

# Run locally
cargo run --features "postgres"

# Test endpoints
curl -F "file=@test.txt" http://localhost:8000/api/v1/upload
```

### 2. Cloud Testing with Terraform

```bash
# Deploy to AWS
cd terraform/aws
terraform apply

# Test from cloud
ALB_URL=$(terraform output -raw alb_dns_name)
curl -F "file=@test.txt" http://$ALB_URL/api/v1/upload

# Check database
aws rds describe-db-clusters --query 'DBClusters[0].Endpoint'

# Check storage
aws s3 ls s3://kore-storage-prod/

# Monitor logs
aws logs tail /ecs/kore-cloud-prod --follow
```

### 3. Failover Testing

```bash
# Simulate AWS failure (block port 443)
# Set FAILOVER_PROVIDERS=azure,gcp

# API should automatically use Azure
curl http://api.example.com/api/v1/upload

# Check logs for failover event
aws logs filter-log-events \
  --log-group-name /ecs/kore-cloud-prod \
  --filter-pattern "failover"
```

## 📊 Performance Considerations

### Database Connection Pooling
- Pool size: 5 connections (configurable)
- Min idle: 1 connection
- Max lifetime: 30 minutes
- Connection timeout: 5 seconds

### Storage Upload Optimization
- Multi-part upload for files > 5MB
- Concurrent chunk uploads (4 parallel)
- Automatic retry with exponential backoff
- Circuit breaker for provider failures

### Caching Strategy
- File metadata cached for 5 minutes
- Statistics cache with 1-hour TTL
- Last-resort: local in-memory cache

## 🔒 Security Architecture

### Authentication & Authorization
```
┌─ Client Request
├─ Optional: API Key validation
├─ Optional: JWT token validation
└─ Access to storage/database
```

### Data Protection
- Encryption at rest: Cloud provider defaults
- Encryption in transit: HTTPS/TLS 1.2+
- Secrets management: Vault/KeyVault/KMS
- Database password: 32-byte random generated

### Network Security
- VPC/VNet isolation
- Security groups / firewalls
- No public database access
- Private subnets for compute

## 📈 Monitoring & Observability

### Application Metrics
- Request count per endpoint
- Response time percentiles (p50, p95, p99)
- Error rates by type
- Storage backend health

### Cloud Provider Monitoring
- **AWS**: CloudWatch dashboards
- **Azure**: Application Insights
- **GCP**: Cloud Monitoring

### Logging Strategy
- Application logs → Cloud provider logs
- 30-day retention
- Debug level for development
- Info level for production

## 🚀 Deployment Workflow

### Manual Deployment
```bash
cd terraform/[aws|azure|gcp]
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars with values
terraform init
terraform plan
terraform apply
```

### Automated Deployment
```bash
cd kore-cloud
./deploy_[aws|azure|gcp].sh [environment] [region] [email]
# Fully automated with validation
```

### CI/CD Integration (GitHub Actions - Future)
```yaml
on: [push: tags]
jobs:
  deploy:
    - Build Docker image
    - Push to all registries
    - Deploy to all 3 clouds
    - Run health checks
```

## 🔄 Configuration Matrix

| Setting | Dev | Staging | Prod |
|---------|-----|---------|------|
| Pool Size | 2 | 3 | 5 |
| Log Level | debug | info | warn |
| Replica Count | 1 | 2 | 3 |
| Backup Days | 7 | 14 | 30 |
| Monitoring | Basic | Standard | Premium |
| HA Enabled | No | Yes | Yes |

## 📚 File Dependencies

```
main.rs
├── depends on storage.rs (StorageBackend trait)
├── depends on cloud_providers.rs (AWS/Azure/GCP)
└── depends on db.rs (Database operations)

cloud_providers.rs
├── implements StorageBackend trait
├── imports storage.rs (for trait)
└── uses Azure/GCP SDKs (feature-gated)

storage.rs
├── defines StorageBackend trait
├── implements S3StorageBackend
└── implements LocalStorageBackend

db.rs (feature: postgres)
├── PostgreSQL connection management
├── CRUD operations
└── schema migrations
```

## ✅ Integration Checklist

- [ ] Update Cargo.toml with cloud provider features
- [ ] Add module declarations to main.rs
- [ ] Update AppState structure
- [ ] Integrate cloud_providers.rs factory
- [ ] Update request handlers for multi-cloud
- [ ] Add environment variable support
- [ ] Test local with Docker Compose
- [ ] Deploy to AWS test environment
- [ ] Deploy to Azure test environment
- [ ] Deploy to GCP test environment
- [ ] Test failover scenario
- [ ] Load test across providers
- [ ] Performance benchmark
- [ ] Production deployment

## 🎯 Next Steps

1. **Code Integration**: Integrate cloud_providers.rs into main.rs handlers
2. **Testing**: Comprehensive multi-cloud testing
3. **Documentation**: Update API docs for multi-cloud
4. **Monitoring**: Set up cross-cloud monitoring dashboard
5. **Automation**: GitHub Actions for CI/CD
6. **Optimization**: Performance tuning per provider

---

**Status**: ✅ Architecture Ready for Integration  
**Next Phase**: Application Code Integration  
**Timeline**: 2-3 hours for complete integration and testing
