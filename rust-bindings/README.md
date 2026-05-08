# Phase 2: PyO3 Native Bindings

**Status:** 🚀 In Progress  
**Timeline:** 2-3 weeks  
**Expected Speedup:** 2-5x  

## Overview

Direct Python-Rust FFI using PyO3 to eliminate subprocess overhead and enable true native performance.

## Architecture

```
Python Code
    ↓
PyO3 FFI (zero-copy interface)
    ↓
Rust Core (Kore v0.1.0)
    ↓
Binary format operations
```

## Key Functions

### 1. `kore_read_native(path)`
- Bypass Python layer completely
- Zero-copy data transfer
- Parallel chunk reading with Rayon

### 2. `kore_write_native(path, schema, data)`
- Direct Rust encoding
- Streaming write for large datasets
- Chunked output

### 3. `kore_read_column_native(path, column)`
- Single-column extraction without full read
- Optimized for analytical queries

### 4. `kore_stats_native(path)`
- Fast metadata extraction
- No data reading required

### 5. `kore_process_batch(paths, operation)`
- Parallel multi-file processing
- Rayon work-stealing scheduler

## Build Instructions

```bash
cd rust-bindings

# Install PyO3
pip install maturin

# Build
maturin develop --release

# Test
python -c "from kore_native import *; print(kore_read_native('test.kore'))"
```

## Performance Targets

| Operation | Current (Python) | Target (PyO3) | Speedup |
|-----------|------------------|---------------|---------|
| Read 1GB | 1.0s | 0.2-0.4s | 2.5-5x |
| Write 1GB | 1.2s | 0.3-0.5s | 2.4-4x |
| Single column | 100ms | 10-20ms | 5-10x |
| Batch (10 files) | 10s | 2-3s | 3-5x |

## Integration Path

1. Phase 2A: Basic FFI wrappers (Week 1)
2. Phase 2B: Parallel processing (Week 2)
3. Phase 2C: Zero-copy optimization (Week 3)
4. Phase 2D: Performance testing & tuning

## Dependencies

- PyO3 0.20+
- Rayon 1.7+
- maturin (build)

## Roadmap

- [ ] PyO3 project structure
- [ ] Basic read/write bindings
- [ ] Column-specific operations
- [ ] Batch processing with Rayon
- [ ] Benchmark suite
- [ ] Performance validation
- [ ] Documentation
- [ ] PyPI wheel distribution

## Testing Strategy

```bash
# Unit tests
cargo test

# Python integration tests
pytest tests/

# Performance benchmarks
python examples/benchmark.py
```

## Known Limitations

- Requires Rust compiler on user machine (unless wheels provided)
- PyO3 GIL management needed
- Cross-platform wheel building

## Contributors

Assigned for Phase 2 development.

---

**Next:** Begin with PyO3 project setup and basic FFI wrappers.
