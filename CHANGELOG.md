# Changelog

All notable changes to KORE FileFormat will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]
### Added
- Time travel: append-only version log (`kore-store::versioned`), FFI `kore_version_append/select`, Python `append_version/list_versions/read_at_version`
- FFI `kore_encrypt_bytes/kore_decrypt_bytes` (AES-256-GCM); Python `encrypt_aes256/decrypt_aes256/get_column_stats/get_bloom_filter`
- Reader verifies per-column CRC32 when the stats section is present
- Ruby, PHP and C# bindings: `Crypto` (encrypt/decrypt) and `Versions` (append/select) wrappers over the new FFI calls; Ruby finalizers fixed for Ruby 3.3, PHP uses instance `FFI::new` (static form is deprecated in 8.4)
- Native layout (`# Layout: native-1`): the Python writer stores strings, bools and nulls in the binary section instead of the text header (string dictionaries and null positions used to live there as text, so unique strings made the header huge). String nulls now survive. Files in the old layout are still read; `KORE_LEGACY_LAYOUT=1` writes it. New FFI: `kore_block_add_*_v`, `kore_block_get_*_v`, dictionary variants
- Rust float codecs keep NaN distinct from NULL (files written before this read exactly as before)
- Opt-in `KORE_STR_LENGTHS=1`: length-prefixed string layout (codec 8) instead of absolute offsets. On TPC-H lineitem (60k rows) the file shrinks from 1922 KB to 1250 KB (Parquet ZSTD: 1331 KB). Readers without codec 8 reject such files, so it is off by default
- Opt-in `KORE_SHUFFLE=1`: byte-plane shuffle + ZSTD (codec 7) on fixed-width columns, about 15% smaller on mixed numeric data. Readers without codec 7 (e.g. other-language native readers) reject such files, so it is off by default
- Stress tests: large roundtrip, truncation, bit flips, garbage decoders, concurrency

### Fixed
- Corrupt or truncated files no longer abort the process (bounds-checked parsing, size limits, column length check)
- ZSTD decode failure returned zeros; now an error
- RLE decoder read past the buffer; string decoder panicked on inverted offsets
- Shuffle `distributed_group_by`: COUNT/AVG were re-aggregated incorrectly
- Python: strings containing quotes, newlines or backslashes were corrupted; float `None` came back as NaN
- phase3 BOOL encoder was a generator

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
