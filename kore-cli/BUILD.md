# Kore CLI - Build & Development Guide

## Build Setup

### Prerequisites

- Rust 1.75+
- Cargo 1.75+
- Linux, macOS, or Windows (PowerShell/CMD)
- Optional: Docker for containerized builds

### Development Build

```bash
cd kore-cli

# Debug build (faster compilation, slower runtime)
cargo build

# Release build (optimized)
cargo build --release

# Check compilation without linking
cargo check
```

### Build Targets

```bash
# Build binary only
cargo build --bin kore

# Build with optimizations
cargo build --release

# LTO enabled (slower build, faster runtime)
cargo build --release -C lto=true
```

## Testing

### Run Tests

```bash
# Full test suite
cargo test

# Tests with output
cargo test -- --nocapture

# Specific test
cargo test validate

# Unit tests only
cargo test --lib

# Integration tests only
cargo test --test '*'
```

### Test Organization

```
src/
  commands/
    inspect.rs   - File inspection tests
    validate.rs  - Validation tests
    convert.rs   - Format conversion tests
    analyze.rs   - Analysis tests

examples/
  basic_usage.rs         - Basic examples
  advanced_workflows.rs  - Workflow demonstrations
  scripting_automation.rs - CI/CD patterns
```

### Run Examples

```bash
# Basic usage
cargo run --example basic_usage

# Advanced workflows
cargo run --example advanced_workflows

# Scripting patterns
cargo run --example scripting_automation

# All examples
for example in basic_usage advanced_workflows scripting_automation; do
  cargo run --example $example
done
```

## Benchmarking

### Performance Profiling

```bash
# Build release binary
cargo build --release

# Time individual operations
time cargo run --release -- inspect data.kore
time cargo run --release -- validate data.kore
time cargo run --release -- analyze data.kore

# Profile with flamegraph
cargo install flamegraph
cargo flamegraph --bin kore -- inspect data.kore
```

### Load Testing

```bash
# Generate test data
dd if=/dev/urandom of=testfile.bin bs=1M count=100

# Benchmark inspect
time for i in {1..100}; do
  cargo run --release -- inspect testfile.bin > /dev/null
done

# Benchmark validation
time cargo run --release -- validate testfile.bin --checksum
```

## CI/CD Integration

### GitHub Actions Workflow

```yaml
name: Kore CLI Tests
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
        run: cargo build -p kore-cli --release
        
      - name: Test
        run: cargo test -p kore-cli
        
      - name: Lint
        run: cargo clippy -p kore-cli -- -D warnings
        
      - name: Format Check
        run: cargo fmt -p kore-cli -- --check
        
      - name: Security Audit
        run: cargo audit
        
      - name: Run Examples
        run: |
          cargo run --example basic_usage
          cargo run --example advanced_workflows
          cargo run --example scripting_automation

  release:
    runs-on: ${{ matrix.os }}
    if: startsWith(github.ref, 'refs/tags/')
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
          - os: macos-latest
            target: x86_64-apple-darwin
          - os: windows-latest
            target: x86_64-pc-windows-msvc
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          target: ${{ matrix.target }}
      - run: cargo build --release --target ${{ matrix.target }}
      - uses: softprops/action-gh-release@v1
        with:
          files: target/${{ matrix.target }}/release/kore*
```

### Pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit

echo "Running Kore CLI pre-commit checks..."

# Format check
cargo fmt -p kore-cli -- --check
if [ $? -ne 0 ]; then
  echo "❌ Format check failed. Run: cargo fmt -p kore-cli"
  exit 1
fi

# Lint check
cargo clippy -p kore-cli -- -D warnings
if [ $? -ne 0 ]; then
  echo "❌ Lint check failed"
  exit 1
fi

# Test
cargo test -p kore-cli --lib
if [ $? -ne 0 ]; then
  echo "❌ Tests failed"
  exit 1
fi

echo "✓ All checks passed"
```

Make executable:
```bash
chmod +x .git/hooks/pre-commit
```

## Dependency Management

### Core Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| clap | 4.4 | CLI argument parsing |
| tokio | 1.35 | Async runtime |
| serde | 1.0 | Serialization |
| comfy-table | 7.0 | Table formatting |
| sha2 | 0.10 | Hash verification |
| flate2 | 1.0 | Gzip compression |
| zstd | 0.13 | Zstd compression |
| indicatif | 0.17 | Progress bars |

### Update Dependencies

```bash
# Check outdated packages
cargo outdated

# Update to latest compatible
cargo update

# Update specific package
cargo update -p tokio --aggressive

# Security audit
cargo audit

# Fix vulnerabilities
cargo audit --fix
```

## Code Quality

### Format Code

```bash
cargo fmt --all

# Check without modifying
cargo fmt --all -- --check
```

### Lint with Clippy

```bash
# Run all lints
cargo clippy -- -D warnings

# Specific lint
cargo clippy -- -W clippy::all

# Against nightly features
cargo +nightly clippy --all-targets --all-features
```

### Documentation

```bash
# Generate docs
cargo doc --open --no-deps

# Check doc links
cargo test --doc
```

## Building Releases

### Local Release Build

```bash
# Clean build
cargo clean

# Build optimized binary
cargo build --release

# Binary location
ls -lh target/release/kore

# Test release binary
./target/release/kore --version
./target/release/kore inspect --help
```

### Cross-Platform Build

```bash
# Install cross
cargo install cross

# Build for Linux
cross build --target x86_64-unknown-linux-gnu --release

# Build for macOS
cross build --target x86_64-apple-darwin --release

# Build for Windows
cross build --target x86_64-pc-windows-gnu --release
```

### Distribution

```bash
# Create release tarball
tar -czf kore-cli-v1.0.0.tar.gz target/release/kore

# Create zip (Windows)
cd target/release
zip -q kore-cli-v1.0.0.zip kore.exe
```

## Docker

### Dockerfile

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p kore-cli

FROM ubuntu:22.04
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/kore /usr/local/bin/
ENTRYPOINT ["kore"]
```

Build:
```bash
docker build -t kore-cli:latest .
```

Run:
```bash
docker run -v $(pwd):/data kore-cli:latest inspect /data/file.kore
```

## Integration Testing

### Test Against Other Modules

```bash
# Build all related modules
cd ../kore-security && cargo build --release
cd ../kore-streaming && cargo build --release
cd ../kore-cli && cargo build --release

# Integration test
cargo test --release

# Cross-module test
# Test: CLI can validate files encrypted by kore-security
```

## Performance Benchmarks

### Expected Performance

```
Inspect (1MB file):        < 10ms
Validate (checksum):       < 50ms
Analyze (compression):     < 100ms
Convert (with zstd):       < 500ms
Batch (8 parallel files):  ~200ms per file
```

### Profile Memory Usage

```bash
# Memory profiling
/usr/bin/time -v cargo run --release -- analyze large.kore

# Valgrind (Linux)
valgrind --leak-check=full ./target/release/kore inspect file.kore

# Instruments (macOS)
instruments -t "Allocations" ./target/release/kore inspect file.kore
```

## Troubleshooting

### Build Issues

| Issue | Solution |
|-------|----------|
| "error: could not compile" | Run `cargo clean && cargo build` |
| "dependency not found" | Run `cargo fetch` |
| "linker error" | Install build tools: `apt-get install build-essential` |
| "out of memory" | Use `cargo build -j 1` (single thread) |

### Runtime Issues

| Issue | Solution |
|-------|----------|
| File not found | Check path: `ls -la file.kore` |
| Permission denied | Fix permissions: `chmod +r file.kore` |
| Timeout on large files | Increase timeout: `KORE_TIMEOUT=600` |
| Memory usage high | Reduce sample size: `--samples 1000` |

### Debugging

```bash
# Enable debug output
RUST_LOG=debug cargo run -- inspect data.kore

# Full stack trace
RUST_BACKTRACE=1 cargo run -- validate data.kore

# With all logging
RUST_LOG=trace RUST_BACKTRACE=full cargo run -- analyze data.kore
```

## Optimization Tips

### Compilation Speed

```bash
# Incremental compilation
cargo build -j 4

# Parallel linking
export RUSTFLAGS="-C link-arg=-fuse-ld=lld"
cargo build --release

# Use mold linker (fastest)
cargo install mold
RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo build --release
```

### Runtime Performance

```bash
# LTO for binary size/speed
cargo build --release -C lto=true

# Optimize for speed (not size)
cargo build --release -C opt-level=3

# Strip binary for distribution
strip target/release/kore
```

## Release Checklist

- [ ] Update version in Cargo.toml
- [ ] Run `cargo test --release`
- [ ] Run `cargo fmt` and `cargo clippy`
- [ ] Run `cargo audit`
- [ ] Update CHANGELOG.md
- [ ] Create git tag: `git tag v1.0.0`
- [ ] Build release binaries for all platforms
- [ ] Create GitHub release with binaries
- [ ] Update crates.io: `cargo publish`

## Advanced

### Custom Allocators

```rust
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

Add to Cargo.toml:
```toml
[dependencies]
mimalloc = "0.1"
```

### Feature Flags

```bash
# Build with specific features
cargo build --release --features "compression,encryption"

# Default features
cargo build --release --no-default-features
```

### Vendoring Dependencies

```bash
# Create vendor directory
cargo vendor

# Build from vendor
cargo build --offline
```

---

**Last Updated**: June 2026
**Status**: Production Ready (Week 6)
