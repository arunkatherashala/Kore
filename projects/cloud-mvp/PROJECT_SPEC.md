# Project 2: Cloud MVP (S3 + REST API + Query Engine)

## 📋 Specification

### Core Components
1. **REST API** (20+ endpoints)
   - List/Create/Delete/Update/Batch operations
   - Filter pushdown to S3 Select
   - Query execution engine

2. **S3 Integration**
   - Read/write Kore files to S3
   - Multipart upload support
   - Signed URL generation

3. **Query Engine**
   - PostgreSQL metadata layer
   - Column pruning
   - Predicate pushdown
   - Scan optimization

4. **Security**
   - IAM authentication
   - TLS encryption
   - Rate limiting
   - API key management

## 🎯 Deliverables
- ✅ Production-ready REST API
- ✅ S3 connector with streaming
- ✅ Query optimizer
- ✅ Performance benchmarks
- ✅ Docker deployment
- ✅ Kubernetes manifests

## 📊 Metrics
- Throughput: >10k queries/sec
- Latency: <100ms p95
- S3 cost: <5% overhead

## Status
- Created: May 23, 2026
- Ready for implementation
