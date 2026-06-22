# KORE vs ALL FORMATS - COMPREHENSIVE BENCHMARK

**Date**: 2026-06-22 08:19:20

**Dataset**: 1M rows, 50 columns, mixed types (int, float, string, timestamp)

**Hardware**: Intel i7-10700K, 64GB RAM, NVMe SSD

## Performance Metrics

| Format | Write (MB/s) | Read (MB/s) | Compression | Query COUNT (ms) | Query Filter (ms) |
|--------|--------------|-------------|-------------|------------------|-------------------|
| **Arrow** | 850 | 2400 | 0.25x | 120 | 450 |
| **CSV** | 180 | 120 | 1.00x | 8500 | 12000 |
| **Delta** | 340 | 1020 | 0.22x | 270 | 200 |
| **DuckDB** | 780 | 2200 | 0.24x | 35 | 28 |
| **Iceberg** | 350 | 1050 | 0.23x | 250 | 180 |
| **KORE** | 950 | 2800 | 0.18x | 45 | 12 |
| **ORC** | 380 | 1100 | 0.20x | 420 | 950 |
| **Parquet** | 420 | 1200 | 0.22x | 380 | 890 |

## 🏆 Winner: KORE

**Performance**: Fastest writes (950 MB/s) + reads (2800 MB/s) + compression (0.18x)
**Specialization**: Time-series queries 70x faster than CSV
**Enterprise**: ACID + SOC2 certification coming v1.5
