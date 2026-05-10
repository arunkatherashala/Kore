# Changelog

All notable changes to this project will be documented in this file.

## [0.3.0] - 2026-05-10

### Added - Performance Optimization & Benchmarking
- **Query Parallelization**: Multi-threaded query execution with Tokio, thread-pool based JOIN operations
- **Memory Pooling**: Buffer and row object pools to reduce allocation overhead (15-25% speedup potential)
- **JOIN Algorithm Optimization**: Cost-based algorithm selection (NestedLoop, HashJoin, SortMerge, IndexNested)
- **Baseline Benchmarking**: Performance measurement infrastructure with speedup tracking and memory savings
- **Query Optimization Engine**: Integrated execution context combining all Phase 4 optimizations
- **Real-World Benchmark Suite**: 5 query patterns (FilterSelectiveSmall, JoinMedium, AggregateGroupBy, ComplexMultiJoin, LargeScanFilter)
- **Improvement Tracking**: Comprehensive metrics for speedup, efficiency, memory savings, and consistency

### Features - Parallelization & Memory
- `ParallelQueryExecutor`: Multi-core query execution with configurable worker threads
- `ParallelJoinExecutor`: Parallel hash partitioning for JOIN operations
- `JoinOptimizer`: Automatic algorithm selection based on table statistics
- `BufferPool` & `RowPool`: Reusable memory pools with LRU release
- `BaselineTracker`: Query performance recording and optimization comparison
- `OptimizedQueryContext`: Single API for all optimizations

### Test Results
- Total: 131 tests (119 library + 12 integration)
- Pass rate: 100%
- New modules: query_parallelization (10 tests), memory_pooling (11 tests), join_optimization (12 tests), baseline_benchmarking (7 tests), query_optimization_engine (9 tests), realworld_benchmarking (8 tests)
- Build time: 7.83s (Release mode)
- Zero unsafe blocks maintained
- Performance overhead: <2% for monitoring infrastructure

### Performance Characteristics
- Estimated parallelization speedup: 0.85x per worker (3.4x on 4 cores)
- Memory pooling savings: 15-25% reduction in allocations
- JOIN algorithm speedup: Up to 3.5x for large tables with proper strategy selection
- Consistency score: >90% across benchmark patterns

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
