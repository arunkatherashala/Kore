# Kore Streaming - Build & Development Guide

## Build Setup

### Prerequisites

- Rust 1.75+
- Cargo 1.75+
- tokio 1.35+ runtime
- Linux, macOS, or Windows (PowerShell)

### Development Build

```bash
cd kore-streaming

# Build with debug symbols
cargo build

# Build optimized for testing
cargo build --release

# Check compilation without linking
cargo check
```

### Build Targets

```bash
# Full build with all features
cargo build --features "all"

# Build without optional features
cargo build --no-default-features

# Build specific mode only
cargo build --features "append-only"
cargo build --features "acid"
cargo build --features "cdc"

# Include Kafka support
cargo build --features "kafka,all"
```

## Testing

### Run All Tests

```bash
# Full test suite
cargo test

# Test with output
cargo test -- --nocapture

# Single test
cargo test append_only_example
```

### Test Organization

```
src/
  append_only.rs        → 4 unit tests
  acid.rs               → 4 unit tests
  cdc.rs                → 4 unit tests
  transaction.rs        → 4 unit tests
  error.rs              → (compile-time validation)
  
examples/
  append_only_example.rs     → Real-world event stream
  acid_transactions.rs       → Concurrent ACID demo
  cdc_streaming.rs           → Change replication
```

### Run Examples

```bash
# Append-only event streaming
cargo run --example append_only_example

# ACID transactions with snapshot isolation
cargo run --example acid_transactions

# Change data capture
cargo run --example cdc_streaming

# All examples
cargo run --example append_only_example && \
cargo run --example acid_transactions && \
cargo run --example cdc_streaming
```

## Benchmarking

### Performance Verification

```bash
# Build for benchmarking
cargo build --release

# Run with timing output
time cargo test --release

# Measure throughput
cargo test append_batch --release -- --nocapture

# Profile with flamegraph (requires flamegraph tool)
cargo install flamegraph
cargo flamegraph --example append_only_example
```

### Expected Performance

```
Append operations:      100K+ ops/sec
ACID commit:            10K+ ops/sec
CDC publish:            100K+ ops/sec
Average latency:        <100μs
```

## CI/CD Integration

### GitHub Actions Workflow

```yaml
name: Kore Streaming Tests
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Build
        run: cargo build -p kore-streaming --release
      - name: Test
        run: cargo test -p kore-streaming
      - name: Examples
        run: |
          cargo run --example append_only_example
          cargo run --example acid_transactions
          cargo run --example cdc_streaming
```

### Local Pre-commit Check

```bash
#!/bin/bash
# .git/hooks/pre-commit

cargo check
if [ $? -ne 0 ]; then
  echo "Compilation failed"
  exit 1
fi

cargo test --lib
if [ $? -ne 0 ]; then
  echo "Tests failed"
  exit 1
fi
```

## Dependency Management

### Core Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1.35 | Async runtime |
| async-trait | 0.1 | Async trait support |
| serde | 1.0 | Serialization |
| serde_json | 1.0 | JSON encoding |
| chrono | 0.4 | Timestamps |
| uuid | 1.6 | Transaction IDs |
| dashmap | 5.5 | Concurrent hashmap |
| thiserror | 1.0 | Error handling |
| parking_lot | 0.12 | Efficient locks |

### Optional Dependencies

| Feature | Crate | Version | Purpose |
|---------|-------|---------|---------|
| kafka | rdkafka | 0.35 | Kafka producer |

### Update Dependencies

```bash
# Check for outdated packages
cargo outdated

# Update to latest compatible versions
cargo update

# Update specific package
cargo update -p tokio --aggressive

# Security audit
cargo audit
```

## Troubleshooting

### Common Build Issues

| Issue | Cause | Solution |
|-------|-------|----------|
| `cargo build` fails with "cannot find" | Missing dependencies | Run `cargo fetch` |
| Compilation hangs | Large workspace | Use `cargo build -p kore-streaming` |
| Tests timeout | Async deadlock | Check mutex usage in tests |
| Memory errors in tests | Unbounded growth | Check Arc/Mutex cleanup |

### Debug Logging

```rust
// Enable debug output in tests
RUST_LOG=debug cargo test -- --nocapture

// In code
use tracing::{debug, info, warn};
debug!("Event: {:?}", event);
```

### Performance Issues

```bash
# Profile execution
cargo flamegraph --example append_only_example -o profile.svg

# Memory profiling
VALGRIND_LOG_FD=2 valgrind --leak-check=full \
  ./target/debug/examples/append_only_example

# Check for allocations
RUST_BACKTRACE=1 cargo run --example append_only_example
```

## Integration Testing

### Test Append-Only with Spark Connector

```bash
# Build both modules
cargo build -p kore-streaming
cargo build -p spark-kore

# Run integration test
cargo test --test integration_append_only
```

### Test ACID with Observability

```bash
# Build with metrics
cargo build -p kore-streaming -p kore-observability

# Run ACID example with metrics collection
RUST_LOG=info cargo run --example acid_transactions
```

### End-to-End Testing

```bash
# Build all weeks
cd ../spark-kore && cargo build
cd ../kore-cloud && cargo build
cd ../kore-observability && cargo build
cd ../kore-streaming && cargo build

# Run all examples
for example in append_only_example acid_transactions cdc_streaming; do
  echo "Running $example..."
  cargo run --example $example
done
```

## Documentation

### Generate API Docs

```bash
# Build documentation
cargo doc --open --no-deps

# Include private items
cargo doc --open --no-deps --document-private-items
```

### Markdown Examples

- See [README.md](README.md) for:
  - Quick start guides
  - API reference
  - Architecture overview
  - Performance characteristics

### Code Comments

```rust
/// Top-level documentation for module/function
///
/// # Examples
/// ```
/// use kore_streaming::append_only::*;
/// # async fn demo() -> Result<()> {
/// let store = InMemoryAppendOnlyStore::new();
/// let record = AppendRecord::new(0, vec![1,2,3]);
/// store.append(record).await?;
/// # Ok(())
/// # }
/// ```
```

## Release Process

### Version Bumping

```bash
# Update Cargo.toml version
sed -i 's/version = "4.*/version = "4.1.0"/' Cargo.toml

# Rebuild to verify
cargo build

# Run full test suite
cargo test

# Tag release
git tag v4.1.0
git push origin v4.1.0
```

### Publishing to crates.io

```bash
# Publish to registry
cargo publish -p kore-streaming

# Verify on crates.io
open https://crates.io/crates/kore-fileformat-streaming
```

## Development Workflow

### Feature Branch Development

```bash
# Create feature branch
git checkout -b feature/kafka-integration

# Develop with tests
cargo test

# Commit changes
git add .
git commit -m "Add Kafka support"

# Create pull request
gh pr create
```

### Code Quality Checks

```bash
# Format code
cargo fmt --all

# Lint warnings
cargo clippy -- -D warnings

# Security audit
cargo audit

# Full check
cargo check && cargo fmt --check && cargo clippy
```

### Continuous Integration

```bash
# Simulate CI locally
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
cargo doc --no-deps
```

## Performance Optimization

### Profile Guide

```bash
# Identify hotspots
cargo install cargo-flamegraph
cargo flamegraph --example append_only_example

# Memory analysis
cargo install cargo-heaptrack
cargo heaptrack run target/debug/examples/append_only_example

# Benchmark comparison
cargo bench
```

### Optimization Checklist

- [x] Lock-free append with atomic compare-and-swap
- [x] Batch operations to reduce contention
- [x] Use DashMap for concurrent access
- [x] Arc<Mutex> for shared state
- [ ] Custom allocators (future)
- [ ] SIMD vectorization (future)

## Support & Debugging

### Get Build Information

```bash
cargo --version
rustc --version
uname -a

# Full environment
cargo build --verbose 2>&1 | head -20
```

### Report Issues

Include in issue report:
- Output of `cargo --version`
- Output of `rustc --version`
- Full error message with backtrace
- Minimal reproduction code

### Quick Help

```bash
# Clean rebuild
cargo clean && cargo build

# Fresh tests
cargo clean && cargo test

# Verbose output
RUST_LOG=trace cargo run --example append_only_example

# Check specific features
cargo check --features "kafka"
```

---

**Last Updated**: June 2026
**Status**: Production Ready (Week 4)
