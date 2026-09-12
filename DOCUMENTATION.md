# KORE Engine v1.8.0 — Complete Documentation Index

> **All documentation you need to install, use, and master KORE Engine.**

---

## 📚 Documentation Roadmap

### **🟢 Level 1: Getting Started (Beginners)**
Start here if you're new to KORE Engine.

1. **[README.md](README.md)** — Project Overview
   - What is KORE? (75x faster than Spark)
   - Benchmark results (TPC-H, feature matrix)
   - Architecture overview (75 crates, 7 layers)
   - Quick start (`cargo run`)

2. **[USER_GUIDE.md](USER_GUIDE.md)** — Installation & Basic Usage
   - Installation: pip, conda, Docker, source
   - Quick Start: 5-minute example
   - File formats: CSV, Parquet, .kore, Delta Lake
   - Common patterns: ETL, aggregations, joins
   - Troubleshooting guide
   - **Time:** 30 minutes to productive
   - **After this:** You can run SQL queries on any dataset

---

### **🟡 Level 2: Core Concepts (Intermediate)**
Understand KORE's data model and how it works.

3. **[ADVANCED_GUIDE.md](ADVANCED_GUIDE.md)** — Deep Dive into KORE
   - Core Concepts: DataBlock, Column, KoreSession
   - Data Models: DataBlock vs DataFrame vs Dataset
   - **Transformations** (7 types with examples):
     - Selection & Projection
     - Filtering (compiled predicates, GPU acceleration)
     - Aggregation (GROUP BY optimization)
     - Joins (broadcast, shuffle, sort-merge strategies)
     - Window Functions (RANK, LAG, LEAD, etc.)
     - Set Operations (UNION, INTERSECT, EXCEPT)
     - Subqueries & CTEs
   - **Actions** (5 types with examples):
     - Collection operations (collect, take, head)
     - Aggregation actions (count, max, min)
     - Persistence (CSV, Parquet, .kore, Delta)
     - Show/Display (explain, show)
     - Write operations (INSERT, UPDATE, DELETE, MERGE)
   - What's Special in KORE (5 features)
   - KORE vs Spark comparison
   - Query Execution Model (10-phase pipeline)
   - Advanced Features (8 optimization techniques)
   - **Time:** 1-2 hours to mastery
   - **After this:** You understand KORE's execution model and optimization

---

### **🔴 Level 3: Production Deployment (Advanced)**
Deploy KORE at scale with clustering, monitoring, and security.

4. **[DISTRIBUTION.md](DISTRIBUTION.md)** — Cluster Setup & Distributed Computing
   - Cluster architecture: Coordinator + N Workers
   - Setup: Local, Docker, Kubernetes, Cloud
   - Configuration: Nodes, shuffle, partitions
   - Monitoring: Prometheus metrics, EXPLAIN ANALYZE
   - Fault tolerance: Lineage tracking, automatic retry
   - Performance tuning: Shuffle optimization, memory management
   - Multi-tenancy: Isolate workloads, resource limits
   - **Time:** 2-3 hours for production setup
   - **After this:** You can run KORE at enterprise scale

5. **[FILE_FORMAT_GUIDE.md](FILE_FORMAT_GUIDE.md)** — File I/O & Format Utilities
   - When to use `kore-fileformat` (optional companion package)
   - Schema management & inference
   - File formats: Parquet, .kore, Delta, CSV, ORC, Arrow IPC
   - Compression: LZ4, Zstd, Gzip comparisons
   - Batch processing: Chunked reading/writing
   - Conversion pipelines: CSV → Parquet → .kore → Delta
   - Production patterns: Incremental imports, archiving, validation
   - **Time:** 1 hour for production file handling
   - **After this:** You can optimize file I/O and formats

6. **[GPU_SUPPORT_DESIGN.md](GPU_SUPPORT_DESIGN.md)** — GPU Acceleration (Intel Arc, NVIDIA, AMD)
   - GPU setup: Detect, verify, enable
   - WGSL compute kernels (WebGPU)
   - Supported operations: Filter, Aggregate
   - Performance metrics: 339x vs Spark on TPC-H
   - Coming soon: GROUP BY, JOIN, SORT GPU kernels
   - **Time:** 30 minutes to enable GPU
   - **After this:** Queries run 100x faster on massive datasets

7. **[DEPLOYMENT.md](DEPLOYMENT.md)** — Production Deployment
   - Docker image building
   - Kubernetes manifests (Helm charts)
   - Cloud deployment: AWS, GCP, Azure
   - Authentication: API tokens, TLS certificates
   - High availability: Load balancing, failover
   - CI/CD integration: GitHub Actions, GitLab CI
   - **Time:** 2-3 hours for full deployment
   - **After this:** KORE runs in production with monitoring

---

### **📋 Reference Documentation**

8. **[RELEASE_v1_8_0.md](RELEASE_v1_8_0.md)** — v1.8.0 Release Notes
   - Seven-Pillar Mission (Correctness, Performance, Scalability, Reliability, Usability, Transparency, Production Quality)
   - v1.8.0 features: GPU acceleration, WebGPU kernel, Intel Arc validation
   - Benchmark results: TPC-H SF-1 (339x Spark), SF-5 (75x Spark)
   - Installation methods
   - Q4 2026 roadmap: Phase 2A-2E deliverables
   - Scorecards: Current ratings and targets

9. **[RELEASE_NOTES.md](RELEASE_NOTES.md)** — All Releases
   - Version history (v1.0.0 → v1.8.0)
   - Breaking changes
   - Migration guides
   - Deprecation notices

10. **[STRESS_TEST_PLAN.md](STRESS_TEST_PLAN.md)** — Testing & Validation
    - Correctness testing (TPC-H, edge cases)
    - Performance testing (benchmarks, scalability)
    - Reliability testing (failures, recovery)
    - Stress testing (memory, CPU, network limits)

---

## 🎯 Quick Navigation by Use Case

### "I want to..."

| Goal | Start With | Then Read | Time |
|------|-----------|-----------|------|
| **Learn KORE basics** | [USER_GUIDE.md](USER_GUIDE.md) | None | 30 min |
| **Run my first query** | [USER_GUIDE.md](USER_GUIDE.md#-quick-start--5-minutes) | None | 5 min |
| **Understand transformations** | [ADVANCED_GUIDE.md](ADVANCED_GUIDE.md#transformations) | [ADVANCED_GUIDE.md](ADVANCED_GUIDE.md#actions) | 1 hour |
| **Setup a cluster** | [DISTRIBUTION.md](DISTRIBUTION.md) | [DEPLOYMENT.md](DEPLOYMENT.md) | 3 hours |
| **Optimize file I/O** | [FILE_FORMAT_GUIDE.md](FILE_FORMAT_GUIDE.md) | None | 1 hour |
| **Enable GPU acceleration** | [GPU_SUPPORT_DESIGN.md](GPU_SUPPORT_DESIGN.md) | [USER_GUIDE.md](USER_GUIDE.md#-gpu-acceleration-optional) | 30 min |
| **Deploy to production** | [DEPLOYMENT.md](DEPLOYMENT.md) | [DISTRIBUTION.md](DISTRIBUTION.md) | 3 hours |
| **Compare with Spark** | [ADVANCED_GUIDE.md](ADVANCED_GUIDE.md#kore-vs-spark) | [README.md](README.md) | 30 min |
| **Understand execution plans** | [ADVANCED_GUIDE.md](ADVANCED_GUIDE.md#query-execution-model) | [DISTRIBUTION.md](DISTRIBUTION.md#distributed-execution) | 1 hour |
| **Migrate from Spark** | [USER_GUIDE.md](USER_GUIDE.md) | [ADVANCED_GUIDE.md](ADVANCED_GUIDE.md) | 2 hours |

---

## 📖 Documentation by Topic

### Core Concepts
- **DataBlock, Column, KoreSession** → [ADVANCED_GUIDE.md § Core Concepts](ADVANCED_GUIDE.md#core-concepts)
- **Data Models comparison** → [ADVANCED_GUIDE.md § Data Models](ADVANCED_GUIDE.md#data-models)
- **Query types (OLAP vs OLTP)** → [ADVANCED_GUIDE.md § Query Execution Model](ADVANCED_GUIDE.md#query-execution-model)

### SQL Queries
- **SELECT, WHERE, GROUP BY** → [USER_GUIDE.md § Quick Start](USER_GUIDE.md#-quick-start--5-minutes)
- **JOINs (INNER, LEFT, FULL)** → [ADVANCED_GUIDE.md § Joins](ADVANCED_GUIDE.md#4-joins)
- **Window Functions** → [ADVANCED_GUIDE.md § Window Functions](ADVANCED_GUIDE.md#5-window-functions)
- **Subqueries & CTEs** → [ADVANCED_GUIDE.md § Subqueries & CTEs](ADVANCED_GUIDE.md#7-subqueries--ctes)

### Data Formats
- **CSV, Parquet, .kore, Delta** → [USER_GUIDE.md § File Formats](USER_GUIDE.md#-working-with-file-formats)
- **Format conversion** → [FILE_FORMAT_GUIDE.md § Conversion Pipelines](FILE_FORMAT_GUIDE.md#-conversion-pipelines)
- **Schema management** → [FILE_FORMAT_GUIDE.md § Schema Management](FILE_FORMAT_GUIDE.md#-schema-management)
- **Compression** → [FILE_FORMAT_GUIDE.md § Compression](FILE_FORMAT_GUIDE.md#-compression)

### GPU Acceleration
- **Setup & enable** → [USER_GUIDE.md § GPU Acceleration](USER_GUIDE.md#-gpu-acceleration-optional)
- **Supported operations** → [GPU_SUPPORT_DESIGN.md](GPU_SUPPORT_DESIGN.md)
- **Performance tuning** → [ADVANCED_GUIDE.md § GPU Acceleration](ADVANCED_GUIDE.md#1-gpu-acceleration-unique-to-kore)

### Distributed Computing
- **Cluster setup** → [DISTRIBUTION.md](DISTRIBUTION.md)
- **Worker coordination** → [DISTRIBUTION.md § Architecture](DISTRIBUTION.md#-architecture)
- **Shuffle optimization** → [ADVANCED_GUIDE.md § Broadcast Join](ADVANCED_GUIDE.md#2-broadcast-join-optimization-avoid-shuffle)
- **Fault tolerance** → [ADVANCED_GUIDE.md § Query Execution Model](ADVANCED_GUIDE.md#phase-by-phase-execution)

### Production Deployment
- **Docker & Kubernetes** → [DEPLOYMENT.md](DEPLOYMENT.md)
- **Monitoring & metrics** → [DISTRIBUTION.md § Monitoring](DISTRIBUTION.md#-monitoring)
- **Authentication & security** → [USER_GUIDE.md § Production Setup](USER_GUIDE.md#-production-setup)

### Performance & Optimization
- **Query optimization** → [ADVANCED_GUIDE.md § Catalyst Query Optimizer](ADVANCED_GUIDE.md#3-catalyst-query-optimizer-production-grade)
- **Partition pruning** → [ADVANCED_GUIDE.md § Partition Pruning](ADVANCED_GUIDE.md#1-partition-pruning-skip-unnecessary-data)
- **Skew handling** → [ADVANCED_GUIDE.md § Skew Handling](ADVANCED_GUIDE.md#4-skew-handling-uneven-data-distribution)
- **Speculative execution** → [ADVANCED_GUIDE.md § Speculative Execution](ADVANCED_GUIDE.md#3-speculative-execution-handle-stragglers)

---

## 🎓 Learning Paths

### Path 1: Data Analyst (SQL-Focused)
1. [USER_GUIDE.md](USER_GUIDE.md) — 30 min
2. [ADVANCED_GUIDE.md § Transformations](ADVANCED_GUIDE.md#transformations) — 1 hour
3. Start writing SQL queries!

**Outcome:** Run analytical queries, generate reports, explore data

---

### Path 2: Data Engineer (ETL-Focused)
1. [USER_GUIDE.md](USER_GUIDE.md) — 30 min
2. [FILE_FORMAT_GUIDE.md](FILE_FORMAT_GUIDE.md) — 1 hour
3. [DISTRIBUTION.md](DISTRIBUTION.md) — 2 hours
4. [DEPLOYMENT.md](DEPLOYMENT.md) — 2 hours

**Outcome:** Build production ETL pipelines, manage data warehouses, cluster administration

---

### Path 3: Data Scientist (ML-Focused)
1. [USER_GUIDE.md](USER_GUIDE.md) — 30 min
2. [ADVANCED_GUIDE.md § Feature Engineering Pattern](ADVANCED_GUIDE.md#pattern-3-machine-learning-feature-engineering) — 1 hour
3. [GPU_SUPPORT_DESIGN.md](GPU_SUPPORT_DESIGN.md) — 30 min

**Outcome:** Feature engineering at scale, GPU-accelerated preprocessing, ML feature pipelines

---

### Path 4: DevOps/SRE (Infrastructure-Focused)
1. [DEPLOYMENT.md](DEPLOYMENT.md) — 2 hours
2. [DISTRIBUTION.md](DISTRIBUTION.md) — 2 hours
3. [USER_GUIDE.md § Production Setup](USER_GUIDE.md#-production-setup) — 1 hour

**Outcome:** Cluster deployment, monitoring, high availability, disaster recovery

---

### Path 5: Database Engineer (Advanced)
1. [ADVANCED_GUIDE.md](ADVANCED_GUIDE.md) — 2 hours
2. [DISTRIBUTION.md](DISTRIBUTION.md) — 2 hours
3. [GPU_SUPPORT_DESIGN.md](GPU_SUPPORT_DESIGN.md) — 1 hour
4. Source code review: `kore-catalyst`, `kore-distributed`

**Outcome:** Query optimization, performance tuning, cluster resource management, kernel development

---

## 📊 Documentation Statistics

| Document | Lines | Topics | Difficulty |
|----------|-------|--------|-----------|
| [README.md](README.md) | 400 | Overview, benchmarks, features | ⭐ Easy |
| [USER_GUIDE.md](USER_GUIDE.md) | 650 | Installation, basics, troubleshooting | ⭐ Easy |
| [ADVANCED_GUIDE.md](ADVANCED_GUIDE.md) | 1018 | Transformations, actions, optimization | ⭐⭐⭐ Hard |
| [FILE_FORMAT_GUIDE.md](FILE_FORMAT_GUIDE.md) | 700 | File I/O, formats, compression | ⭐⭐ Medium |
| [DISTRIBUTION.md](DISTRIBUTION.md) | 600 | Cluster setup, distributed computing | ⭐⭐ Medium |
| [DEPLOYMENT.md](DEPLOYMENT.md) | 500 | Docker, Kubernetes, production | ⭐⭐ Medium |
| [GPU_SUPPORT_DESIGN.md](GPU_SUPPORT_DESIGN.md) | 400 | GPU setup, WebGPU, acceleration | ⭐⭐ Medium |
| **Total** | **4268** | **30+ topics** | **Comprehensive** |

---

## 🔍 Search by Keyword

### Performance
- "339x faster than Spark" → [README.md](README.md)
- "GPU acceleration" → [GPU_SUPPORT_DESIGN.md](GPU_SUPPORT_DESIGN.md)
- "Query optimization" → [ADVANCED_GUIDE.md § Catalyst](ADVANCED_GUIDE.md#3-catalyst-query-optimizer-production-grade)
- "Broadcast join" → [ADVANCED_GUIDE.md § Broadcast Join](ADVANCED_GUIDE.md#2-broadcast-join-optimization-avoid-shuffle)

### Transformations
- "GROUP BY" → [ADVANCED_GUIDE.md § Aggregation](ADVANCED_GUIDE.md#3-aggregation-group-by)
- "Joins" → [ADVANCED_GUIDE.md § Joins](ADVANCED_GUIDE.md#4-joins)
- "Window Functions" → [ADVANCED_GUIDE.md § Window Functions](ADVANCED_GUIDE.md#5-window-functions)
- "Subqueries" → [ADVANCED_GUIDE.md § Subqueries](ADVANCED_GUIDE.md#7-subqueries--ctes)

### Actions
- "collect()" → [ADVANCED_GUIDE.md § Collection Actions](ADVANCED_GUIDE.md#1-collection-actions-bring-data-to-driver)
- "to_csv()" → [ADVANCED_GUIDE.md § Persistence Actions](ADVANCED_GUIDE.md#3-persistence-actions-save-to-storage)
- "to_delta()" → [ADVANCED_GUIDE.md § Persistence Actions](ADVANCED_GUIDE.md#3-persistence-actions-save-to-storage)

### Data Formats
- "Parquet" → [FILE_FORMAT_GUIDE.md § Parquet](FILE_FORMAT_GUIDE.md#2-parquet-format)
- ".kore format" → [FILE_FORMAT_GUIDE.md § Native .kore](FILE_FORMAT_GUIDE.md#1-native-kore-format-recommended)
- "Delta Lake" → [FILE_FORMAT_GUIDE.md § Delta Lake](FILE_FORMAT_GUIDE.md#3-delta-lake-format)
- "CSV import" → [FILE_FORMAT_GUIDE.md § CSV Format](FILE_FORMAT_GUIDE.md#4-csv-format)

### Cluster & Deployment
- "Setup cluster" → [DISTRIBUTION.md](DISTRIBUTION.md)
- "Docker" → [DEPLOYMENT.md](DEPLOYMENT.md)
- "Kubernetes" → [DEPLOYMENT.md](DEPLOYMENT.md)
- "High availability" → [DEPLOYMENT.md](DEPLOYMENT.md)

### Troubleshooting
- "Out of memory" → [USER_GUIDE.md § Troubleshooting](USER_GUIDE.md#-troubleshooting)
- "Query is slow" → [USER_GUIDE.md § Troubleshooting](USER_GUIDE.md#-troubleshooting)
- "ModuleNotFoundError" → [USER_GUIDE.md § Troubleshooting](USER_GUIDE.md#-troubleshooting)

---

## 🚀 What's Next?

### Phase 2A (Q4 2026): Transparency & Multi-Engine Benchmarking
- Formal benchmark methodology documentation
- Public dashboard: kore-benchmarks.com
- DuckDB, Polars, ClickHouse, DataFusion comparison
- Reproducible benchmark scripts

### Phase 2B: GPU Acceleration Completion
- GPU GROUP BY kernel (hash table + atomics)
- GPU hash join kernel
- GPU radix sort kernel
- GPU aggregate function kernels

### Phase 2C: Correctness & Reliability
- Expanded SQL-Conformance test suite (1000+ edge cases)
- Crash/deadlock/memory leak testing
- Error recovery validation

### Phase 2D: Production Hardening
- JDBC binding security audit
- REST API authentication and TLS
- CVE tracking and dependency remediation

### Phase 2E: Monitoring & Documentation
- Prometheus metrics exporter
- EXPLAIN query plan formatter
- Deployment playbooks
- API stability guarantees

---

## 📞 Getting Help

- **GitHub Issues:** https://github.com/arunkatherashala/Kore/issues
- **Discussions:** https://github.com/arunkatherashala/Kore/discussions
- **Email:** team@kore.dev
- **Documentation:** This guide
- **Examples:** See [kore/examples](examples/) directory

---

**Version:** v1.8.0  
**Documentation Last Updated:** Sept 11, 2026  
**Total Documentation:** 4268 lines, 30+ topics, 7 guides  
**Status:** Production Ready ✅
