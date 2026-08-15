# Changelog

All notable changes to KORE FileFormat will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).

## [1.7.16] - 2026-08-11
### Changed
- Auto-bump version 1.7.15 → 1.7.16

## [1.7.15] - 2026-08-11
### Added
- Hive SerDe + Athena Lambda integration
- Spark write support + column pruning
- Docker deployment support
- DuckDB scanner extension
- Website + format spec v3.0 + benchmark suite
- Trino Connector SPI — native .kore file read
- Go publish support
- Spark DataSourceV2 connector
- kore-arrow — Apache Arrow RecordBatch bridge
- kore-store — 11 features for 100% Iceberg parity
- ClickHouse MergeTree-style storage engine

### Fixed
- Go publish — skip vet for CGo types

## [1.6.x] - 2026-06
### Added
- 8 language SDKs (Python, Node.js, Rust, Ruby, Java, C#, Go, PHP)
- CRC32 integrity checks
- SIMD-accelerated columnar operations
- SQL query engine
- Kafka connector
- GPU acceleration support
- Distributed processing framework
- JIT compilation support
