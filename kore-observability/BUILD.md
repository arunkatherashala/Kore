# Kore Observability - Build Guide

## Prerequisites

- Rust 1.70+
- Cargo
- Optional: Docker (for Jaeger)

## Quick Build

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# With all features
cargo build --all-features
```

## Feature-Specific Builds

```bash
# Prometheus metrics only
cargo build --no-default-features --features prometheus

# Jaeger tracing only
cargo build --no-default-features --features jaeger

# Both (default)
cargo build --features "prometheus,jaeger"

# Minimal build (no observability)
cargo build --no-default-features
```

## Setup

### 1. Dependencies Installation

```bash
# Automatic with cargo
cargo build

# Verify dependencies
cargo tree
```

### 2. Jaeger Setup (Optional)

```bash
# Start Jaeger with Docker
docker run -d --name jaeger \
  -p 6831:6831/udp \
  -p 16686:16686 \
  jaegertracing/all-in-one

# Verify Jaeger is running
curl http://localhost:16686

# View UI at: http://localhost:16686
```

### 3. Prometheus Setup (Optional)

Create `prometheus.yml`:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'kore'
    scrape_interval: 5s
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: '/metrics'
```

Start Prometheus:

```bash
docker run -d --name prometheus \
  -p 9090:9090 \
  -v $(pwd)/prometheus.yml:/etc/prometheus/prometheus.yml \
  prom/prometheus

# View UI at: http://localhost:9090
```

## Building Examples

### Build All Examples

```bash
cargo build --examples --all-features
```

### Build Specific Examples

```bash
# Prometheus metrics example
cargo build --example prometheus_metrics --features prometheus

# Jaeger tracing example
cargo build --example jaeger_tracing --features jaeger

# Metrics dashboard example
cargo build --example metrics_dashboard
```

## Running Examples

### Example 1: Prometheus Metrics

```bash
# Run
cargo run --example prometheus_metrics --features prometheus

# Output: Prometheus text format with 25+ metrics
```

### Example 2: Jaeger Tracing

Requires Jaeger running (`docker run ...` from Setup section)

```bash
# Run
cargo run --example jaeger_tracing --features jaeger

# Then view traces at: http://localhost:16686
# Service: kore-example
# Operation: query_example, read_operations, distributed_call
```

### Example 3: Metrics Dashboard

```bash
# Run
cargo run --example metrics_dashboard

# Output: ASCII dashboard with real-time metrics and recommendations
```

## Testing

### Run All Tests

```bash
cargo test
```

### Run Specific Tests

```bash
# Metrics tests
cargo test metrics --lib

# Instrumentation tests
cargo test instrumentation --lib

# Tracing tests
cargo test tracing --lib

# Examples (compile check)
cargo test --examples
```

### Test with Output

```bash
# Show println! output
cargo test -- --nocapture --test-threads=1

# With logging
RUST_LOG=debug cargo test -- --nocapture
```

### Integration Testing

```bash
# Prometheus export functionality
cargo test export --lib -- --nocapture

# Instrumentation auto-timing
cargo test instrumentation --lib -- --nocapture
```

## Verification

### Verify Metrics Export

```bash
# Run metrics example and check output
cargo run --example prometheus_metrics --features prometheus 2>&1 | grep "kore_query"
```

### Verify Compilation

```bash
# Check for errors without building
cargo check

# Check with all features
cargo check --all-features

# Check examples
cargo check --examples
```

### Verify Dependencies

```bash
# Show dependency tree
cargo tree

# Show outdated dependencies
cargo outdated

# Audit for vulnerabilities
cargo audit
```

## Code Quality

### Formatting

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt -- --check
```

### Linting

```bash
# Run clippy
cargo clippy

# Run with all targets
cargo clippy --all-targets

# Strict mode
cargo clippy -- -D warnings
```

### Documentation

```bash
# Generate docs
cargo doc --open

# Include private items
cargo doc --document-private-items --open

# Check documentation links
cargo doc --no-deps
```

## Performance

### Release Build Optimization

```bash
# Aggressive optimization
cargo build --release -C opt-level=3 -C lto=fat -C codegen-units=1

# Check size
ls -lh target/release/examples/*
```

### Profiling

```bash
# Build with debug symbols
cargo build --release

# Profile (Linux)
perf record -g ./target/release/examples/prometheus_metrics
perf report

# Flamegraph (requires flamegraph tool)
cargo install flamegraph
cargo flamegraph --example prometheus_metrics
```

### Benchmarking

```bash
# Benchmark metrics collection (requires criterion)
cargo install cargo-criterion

# Would use: cargo criterion
# (When benches/ directory is added)
```

## Troubleshooting

### Jaeger Connection Failed

```
Error: Connection refused (os error 111)
```

**Fix**: Start Jaeger first
```bash
docker run -d --name jaeger -p 16686:16686 -p 14268:14268 jaegertracing/all-in-one
```

### Prometheus Not Found

```
Error: failed to resolve: use of undeclared crate `prometheus`
```

**Fix**: Ensure prometheus feature is enabled
```bash
cargo build --features prometheus
```

### Dependency Conflicts

```
error: found duplicate packages
```

**Fix**: Update dependencies
```bash
cargo update --aggressive
rm Cargo.lock
cargo build
```

### Port Already in Use

```
Error: Address already in use
```

**Fix**: Stop conflicting services
```bash
# Jaeger on 16686
lsof -i :16686
kill -9 <PID>

# Or use different port
# Change examples to use different port
```

## CI/CD Integration

### GitHub Actions

```yaml
name: Build and Test
on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      
      - name: Build
        run: cargo build --all-features
        
      - name: Test
        run: cargo test --all-features
        
      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings
        
      - name: Format
        run: cargo fmt -- --check
```

### Local Pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit

set -e

echo "Running checks..."
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test --lib

echo "✓ All checks passed"
```

Make executable:
```bash
chmod +x .git/hooks/pre-commit
```

## Development Workflow

### 1. Setup Environment

```bash
git clone https://github.com/arunkatherashala/Kore
cd kore-observability

# Install dependencies
rustup update stable

# Start Jaeger (optional)
docker run -d --name jaeger -p 16686:16686 -p 14268:14268 jaegertracing/all-in-one
```

### 2. Build and Test

```bash
# Build with all features
cargo build --all-features

# Run tests
cargo test --all-features -- --nocapture

# Run examples
cargo run --example prometheus_metrics --features prometheus
cargo run --example metrics_dashboard
```

### 3. Make Changes

```bash
# Edit source files
vim src/metrics.rs

# Format automatically
cargo fmt

# Check code quality
cargo clippy

# Run tests
cargo test

# Build release
cargo build --release
```

### 4. Commit and Push

```bash
# Verify everything passes
cargo check && cargo clippy && cargo test

# Commit
git add .
git commit -m "Add feature X"
git push origin branch-name
```

## Documentation

### Generate and View Docs

```bash
# Generate and open in browser
cargo doc --no-deps --open

# Include private documentation
cargo doc --no-deps --document-private-items --open

# Generate for specific crate
cargo doc --package kore-observability --open
```

### Update Documentation

```bash
# Run documentation tests
cargo test --doc

# Check documentation links
cargo doc --no-deps 2>&1 | grep "warning:"
```

## Release

### Version Update

1. Update `Cargo.toml`:
```toml
[package]
version = "1.0.1"
```

2. Build and test:
```bash
cargo build --release
cargo test --all
```

3. Publish:
```bash
cargo publish --dry-run
cargo publish
```

## Getting Help

- **Rust Book**: https://doc.rust-lang.org/book/
- **Cargo Guide**: https://doc.rust-lang.org/cargo/
- **API Docs**: `cargo doc --open`
- **Issues**: GitHub repository
- **Discussions**: GitHub discussions board

## Summary Commands

```bash
# One-liner setup and test
cargo clean && cargo build --all-features && cargo test --all-features && cargo clippy

# Full CI simulation locally
cargo fmt && cargo clippy --all-targets && cargo test --all-features && cargo build --release

# Quick verify
cargo check && cargo test --lib
```
