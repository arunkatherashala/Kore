# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-05-09

### Added - Query Engine & Optimization
- **Query JOINs**: INNER, LEFT, and RIGHT JOIN support with qualified column syntax (table.column)
- **Query Caching**: LRU query plan cache with TTL expiration and cost-based optimization
- **Index Management**: Column indexing system with cardinality estimation and index recommendations
- **Real File Benchmarking**: Benchmark engine integrated with actual KORE file analysis (9 metrics per benchmark)
- **Integration Tests**: 12 comprehensive end-to-end tests spanning all query components
- **Execution Strategy Selection**: Cost-based query optimizer selecting between IndexScan, StreamingScan, HashJoin, DistributedHash

### Changed
- Enhanced benchmarks with detailed metrics: original_size_mb, compressed_size_mb, compression_percentage, read/write times, throughput, memory, row estimates
- Extended query parser to support JOIN clauses and qualified columns

### Test Results
- Total: 62 tests (50 library + 12 integration)
- Pass rate: 100%
- Build time: 6.34s (Release mode)
- Zero unsafe blocks maintained

## [0.1.0] - 2026-04-22
- Build: crate compiles and unit tests pass on Windows (4 tests).
- Included: `kore_fileformat_killer` examples and `tools` scripts.
