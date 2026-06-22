# QUICK REFERENCE: File Format Comparison Chart

## Performance Scoreboard (Lower is Better for Speed, Higher is Better for Compression)

```
╔════════════════════════════════════════════════════════════════════════════╗
║                      WRITE SPEED COMPARISON (seconds)                     ║
╠════════════════════════════════════════════════════════════════════════════╣
║ Arrow/Feather ████ 0.113s     [FASTEST - 11x faster than CSV]             ║
║ Parquet       ███████ 0.370s                                              ║
║ SQLite        █████████ 0.578s                                            ║
║ JSON          ████████ 0.408s                                             ║
║ NDJSON        ██████████ 0.623s                                           ║
║ CSV           ███████████████ 1.194s [SLOWEST]                            ║
╚════════════════════════════════════════════════════════════════════════════╝

╔════════════════════════════════════════════════════════════════════════════╗
║                      READ SPEED COMPARISON (seconds)                      ║
╠════════════════════════════════════════════════════════════════════════════╣
║ Arrow/Feather ███ 0.076s      [FASTEST - 15x faster than JSON]            ║
║ Parquet       ████ 0.140s                                                 ║
║ CSV           █████ 0.343s                                                ║
║ NDJSON        ██████████ 1.473s                                           ║
║ JSON          ███████████ 1.193s                                          ║
║ SQLite        ████████████ 1.084s                                         ║
╚════════════════════════════════════════════════════════════════════════════╝

╔════════════════════════════════════════════════════════════════════════════╗
║                    COMPRESSION RATIO COMPARISON (%)                       ║
╠════════════════════════════════════════════════════════════════════════════╣
║ Parquet       ██████████████████ 82.7%  [BEST COMPRESSION]                ║
║ SQLite        ██████████████████ 79.7%                                    ║
║ Arrow/Feather █████████████████ 76.8%                                     ║
║ CSV           ████████████████ 67.6%                                      ║
║ NDJSON        ████████ 42.1%                                              ║
║ JSON          ████████ 42.1%                                              ║
╚════════════════════════════════════════════════════════════════════════════╝

╔════════════════════════════════════════════════════════════════════════════╗
║                      AVERAGE FILE SIZE (MB)                               ║
╠════════════════════════════════════════════════════════════════════════════╣
║ Parquet       ████ 6.0 MB      [SMALLEST - 4.4x smaller than JSON]        ║
║ SQLite        █████ 8.7 MB                                                ║
║ Arrow/Feather █████ 10.6 MB                                               ║
║ CSV           ██████ 13.4 MB                                              ║
║ NDJSON        ███████████████████ 26.4 MB                                 ║
║ JSON          ███████████████████ 26.4 MB                                 ║
╚════════════════════════════════════════════════════════════════════════════╝
```

---

## Format Selection Decision Tree

```
START: Choosing a File Format
│
├─→ Do you need ACID transactions?
│   ├─ YES → SQLite (embedded) or KORE (distributed)
│   └─ NO  → Continue...
│
├─→ Is this for real-time operations?
│   ├─ YES → Arrow/Feather (0.076s read, 0.113s write)
│   └─ NO  → Continue...
│
├─→ Is compression/storage cost critical?
│   ├─ YES → Parquet (82.7% compression, 6.0 MB files)
│   └─ NO  → Continue...
│
├─→ Do you need flexible/nested schemas?
│   ├─ YES → JSON or HDF5 (if scientific)
│   └─ NO  → Continue...
│
├─→ Do you need universal compatibility?
│   ├─ YES → CSV (every tool reads it)
│   └─ NO  → Continue...
│
└─→ Are you in Hadoop ecosystem?
    ├─ YES → Parquet or ORC
    └─ NO  → Parquet (safe default)
```

---

## Use Case Selector

| Scenario | Best Format | Why | Secondary Option |
|----------|-------------|-----|-------------------|
| 📊 Data Warehouse | **Parquet** | 82.7% compression, Spark native, cost optimization | Arrow |
| ⚡ Real-time Dashboard | **Arrow/Feather** | 0.076s reads, minimal overhead, fast updates | Parquet |
| 💾 Mobile App | **SQLite** | ACID, embedded, no external deps | CSV |
| 📈 Machine Learning | **Parquet** | Standard in ML ecosystems (TensorFlow, PyTorch) | HDF5 |
| 📱 REST API / microservices | **JSON** | Web-native, flexible schemas, self-describing | NDJSON |
| 📜 Reporting / Excel | **CSV** | Universal export, human-readable | Parquet |
| 🔬 Scientific Computing | **HDF5** | NumPy native, multidimensional arrays | Parquet |
| 🔐 Compliance Auditing | **KORE** | WAL for forensic trails, atomic commits | SQLite |
| ⏰ Time-series Analytics | **KORE** | FOR codec optimal for sequential data | Parquet |
| 🌍 Multi-cloud Data Lake | **KORE** | Native Azure/GCS/S3 connectors | Parquet |
| 🔄 Data Migration | **Parquet** | Widest ecosystem support | CSV |
| 🚀 Hadoop/Hive | **ORC** | Optimized for Hadoop, ACID v2 | Parquet |

---

## Quick Wins: Best-of-Class by Metric

### 🏃 Speed Champions
- **Fastest Write**: Arrow/Feather (0.113s) — 11x faster than CSV
- **Fastest Read**: Arrow/Feather (0.076s) — 15x faster than JSON
- **Best for**: Real-time analytics, dashboards, streaming

### 💎 Storage Champions
- **Best Compression**: Parquet (82.7%) — saves 3.4 MB vs CSV per 100K rows
- **Smallest Files**: Parquet (6.0 MB) — 4.4x smaller than JSON
- **Best for**: Cloud storage costs, long-term archival

### 🔒 Reliability Champions
- **ACID Transactions**: SQLite, KORE (full ACID)
- **Audit Trail**: KORE (WAL + manifest snapshots)
- **Best for**: Financial data, compliance, immutable logs

### 🌐 Compatibility Champions
- **Most Widely Supported**: CSV (literally everywhere)
- **Best Ecosystem**: Parquet (Spark, Polars, DuckDB, Pandas, etc.)
- **Most Flexible**: JSON (schema-free, self-describing)

---

## Performance Tiers (Ranked)

### Tier 1: Production Data Warehouses
1. **Parquet** — compression + ecosystem
2. **Arrow** — speed + simplicity
3. **ORC** — Hadoop-specific ecosystems

### Tier 2: Real-time Analytics
1. **Arrow/Feather** — ultra-fast
2. **Parquet** — good balance
3. **KORE** (pending) — ACID + speed

### Tier 3: Application Data
1. **SQLite** — transactions, embedded
2. **JSON** — web services, APIs
3. **NDJSON** — streaming logs

### Tier 4: Universal Exchange
1. **CSV** — broadest compatibility
2. **JSON** — structured, flexible
3. **Parquet** — if compression matters

### Tier 5: Scientific/Specialty
1. **HDF5** — multidimensional, NumPy
2. **Arrow** — modern scientific Python
3. **Parquet** — general scientific

---

## KORE's Competitive Moat

KORE doesn't compete on **compression** (Parquet wins) or **speed** (Arrow wins).

KORE wins on:

```
┌─────────────────────────────────────────────────────────────┐
│  KORE's Unique Value Propositions                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  1. ACID Transactions in Distributed Systems               │
│     └─ Like SQLite + Cloud ✓                               │
│                                                             │
│  2. Block-aware Compaction                                 │
│     └─ Delete rows without full rewrite ✓                  │
│                                                             │
│  3. WAL-based Audit Trail                                  │
│     └─ Forensic recovery for compliance ✓                  │
│                                                             │
│  4. Multi-cloud Native (Azure/GCS/S3)                      │
│     └─ Not a bolt-on, native connectors ✓                  │
│                                                             │
│  5. Advanced Codecs (FOR, RLE, Packed)                     │
│     └─ Optimized for domain-specific data ✓                │
│                                                             │
│  6. Manifest Streaming                                     │
│     └─ Incremental reads without full scans ✓              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## Migration Paths

### From CSV → Recommended Target
- **If you need speed**: Arrow/Feather
- **If you need compression**: Parquet
- **If you need transactions**: KORE or SQLite
- **If you need ease**: Keep CSV for exchange, use Parquet for storage

### From Parquet → When to Switch
- **To Arrow**: If query latency is critical (0.076s reads)
- **To SQLite**: If you need single-file ACID
- **To KORE**: If you need distributed transactions + cloud
- **To CSV**: If you need universal export

### From SQLite → Recommended Next Step
- **If scaling**: Parquet (data warehouse) + Spark
- **If real-time**: Arrow/Feather
- **If keeping ACID**: KORE (cloud-native transactions)

---

## System Architecture Guide

```
┌──────────────────────────────────────────────────────────────┐
│  Typical Analytics Stack by Size                             │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  SMALL (< 1GB)                                               │
│  ├─ Format: CSV or SQLite                                    │
│  ├─ Tools: Excel, Python, SQL                                │
│  └─ Strategy: Fast prototyping                               │
│                                                              │
│  MEDIUM (1GB - 100GB)                                        │
│  ├─ Format: Parquet or Arrow                                 │
│  ├─ Tools: DuckDB, Polars, Pandas                            │
│  └─ Strategy: Efficient local analytics                      │
│                                                              │
│  LARGE (100GB - 1TB)                                         │
│  ├─ Format: Parquet (preferred) or KORE                      │
│  ├─ Tools: Spark, DuckDB, specialized warehouses             │
│  └─ Strategy: Distributed processing, cloud storage          │
│                                                              │
│  MASSIVE (> 1TB)                                             │
│  ├─ Format: Parquet + ORC + KORE (multi-format)              │
│  ├─ Tools: Spark, Hadoop, Kubernetes, cloud services        │
│  └─ Strategy: Distributed lake houses, federated queries    │
│                                                              │
│  REALTIME (Continuous Streaming)                            │
│  ├─ Format: Arrow (in-flight) + Parquet (at-rest)            │
│  ├─ Tools: Kafka, Flink, Spark Streaming, kdb+              │
│  └─ Strategy: Fast ingestion, efficient storage              │
│                                                              │
│  COMPLIANCE / AUDIT (Immutable Historical)                  │
│  ├─ Format: KORE (with WAL) or Parquet + WAL                │
│  ├─ Tools: Data lake, HDFS, object storage + audit logs      │
│  └─ Strategy: Point-in-time recovery, forensic trails       │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

---

## Cheat Sheet: Pick Your Format in 10 Seconds

```
I want:          → Choose:
─────────────────────────────────────────────────────────
Speed            → Arrow/Feather
Compression      → Parquet
ACID             → SQLite or KORE
Simplicity       → CSV
Flexibility      → JSON
Compatibility    → CSV
Cloud storage    → Parquet
Transactions     → KORE
Real-time        → Arrow
Hadoop           → Parquet or ORC
Scientific       → HDF5
Mobile           → SQLite
API/Web          → JSON
```

---

## Final Score Card

| Format | ⭐ | Use It For | Avoid If |
|--------|----|-----------|---------
| **Parquet** | ⭐⭐⭐⭐⭐ | Data lakes, compression matters | Speed critical |
| **Arrow** | ⭐⭐⭐⭐⭐ | Real-time, speed critical | Compression matters |
| **CSV** | ⭐⭐⭐⭐ | Reporting, exchange, compatibility | Performance critical |
| **SQLite** | ⭐⭐⭐⭐⭐ | Embedded apps, ACID needed, single file | Distributed systems |
| **JSON** | ⭐⭐⭐⭐ | APIs, flexible schemas, web | Large datasets, storage cost |
| **ORC** | ⭐⭐⭐⭐ | Hadoop/Hive, Hadoop ecosystem | Outside Hadoop world |
| **HDF5** | ⭐⭐⭐⭐ | Scientific, ML, multidimensional | Web services |
| **KORE** | ⭐⭐⭐⭐ | Distributed ACID, cloud, time-series | Established Parquet ecosystem |

---

**Last Updated**: June 22, 2026  
**Data Source**: Live Benchmark Suite (KORE_VS_ALL_COMPETITORS.py)  
**Status**: ✅ Ready for Production Decisions

Print this page and keep it handy! 🚀
