# Kore Security - Build & Development Guide

## Build Setup

### Prerequisites

- Rust 1.75+
- Cargo 1.75+
- tokio 1.35+ runtime
- Linux, macOS, or Windows (PowerShell)

### Development Build

```bash
cd kore-security

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

# Build specific feature only
cargo build --features "encryption"
cargo build --features "audit"
cargo build --features "gdpr"
cargo build --features "access-control"
```

## Testing

### Run All Tests

```bash
# Full test suite
cargo test

# Test with output
cargo test -- --nocapture

# Single test
cargo test encryption_key

# Specific module
cargo test encryption::tests
```

### Test Organization

```
src/
  encryption.rs       → 4 unit tests (key generation, encryption, AAD)
  audit.rs            → 4 unit tests (event logging, querying)
  gdpr.rs             → 4 unit tests (consent, data storage, erasure)
  access_control.rs   → 4 unit tests (roles, permissions)
  error.rs            → (compile-time validation)

examples/
  encryption_example.rs      → Random key, password derivation, AAD
  audit_logging.rs           → Event logging, querying, investigation
  gdpr_compliance.rs         → Consent, access, erasure, portability
```

### Run Examples

```bash
# AES-256 encryption demo
cargo run --example encryption_example

# Audit trail and compliance logging
cargo run --example audit_logging

# GDPR data subject rights
cargo run --example gdpr_compliance

# All examples
cargo run --example encryption_example && \
cargo run --example audit_logging && \
cargo run --example gdpr_compliance
```

## Benchmarking

### Performance Verification

```bash
# Build for benchmarking
cargo build --release

# Run with timing output
time cargo test --release

# Measure encryption throughput
cargo test encrypt --release -- --nocapture

# Profile with flamegraph (requires flamegraph tool)
cargo install flamegraph
cargo flamegraph --example encryption_example
```

### Expected Performance

```
Encryption throughput:    10K+ ops/sec (1KB blocks)
Audit logging:            100K+ ops/sec
Key derivation:           1000 ops/sec (Argon2)
Key generation:           1M+ ops/sec (random)
```

## CI/CD Integration

### GitHub Actions Workflow

```yaml
name: Kore Security Tests
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
        run: cargo build -p kore-security --release
      - name: Test
        run: cargo test -p kore-security
      - name: Security Audit
        run: cargo audit
      - name: Examples
        run: |
          cargo run --example encryption_example
          cargo run --example audit_logging
          cargo run --example gdpr_compliance
```

### Pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit

cargo check -p kore-security
if [ $? -ne 0 ]; then
  echo "Compilation failed"
  exit 1
fi

cargo test -p kore-security --lib
if [ $? -ne 0 ]; then
  echo "Tests failed"
  exit 1
fi

cargo clippy -p kore-security -- -D warnings
if [ $? -ne 0 ]; then
  echo "Clippy warnings"
  exit 1
fi
```

## Dependency Management

### Core Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1.35 | Async runtime |
| async-trait | 0.1 | Async trait support |
| aes-gcm | 0.10 | AES-256-GCM encryption |
| argon2 | 0.5 | Password hashing |
| sha2 | 0.10 | SHA-256 hashing |
| rand | 0.8 | Random number generation |
| uuid | 1.6 | Unique identifiers |
| chrono | 0.4 | Timestamps |
| serde | 1.0 | Serialization |
| thiserror | 1.0 | Error handling |

### Update Dependencies

```bash
# Check for outdated packages
cargo outdated

# Update to latest compatible versions
cargo update

# Security audit
cargo audit

# Fix vulnerabilities
cargo audit --fix
```

## Code Quality

### Format Code

```bash
cargo fmt --all
cargo fmt --check
```

### Lint Warnings

```bash
cargo clippy -- -D warnings
```

### Security Analysis

```bash
# Check for known vulnerabilities
cargo audit

# MISRA-C style rules
cargo clippy --all-targets -- -W clippy::all
```

## Troubleshooting

### Common Build Issues

| Issue | Cause | Solution |
|-------|-------|----------|
| `cargo build` fails | Missing dependencies | Run `cargo fetch` |
| Compilation hangs | Large workspace | Use `cargo build -p kore-security` |
| Async runtime error | Missing tokio features | Check Cargo.toml features |
| Tests hang | Mutex deadlock | Check Arc/Mutex usage |
| Memory errors | Unbounded growth | Check encryption loop cleanup |

### Debug Logging

```rust
// Enable debug output in tests
RUST_LOG=debug cargo test -- --nocapture

// In code
log::debug!("Encryption key: {} bytes", key.key_material.len());
```

### Performance Issues

```bash
# Profile execution
cargo flamegraph --example encryption_example -o profile.svg

# Check allocations
perf record -g cargo run --example encryption_example
perf report

# Memory profiling
valgrind --leak-check=full ./target/debug/examples/encryption_example
```

## Integration Testing

### Test with Week 4 (Streaming)

```bash
# Build both modules
cargo build -p kore-security
cargo build -p kore-streaming

# Verify streaming can use encrypted data
cargo test --test integration_encryption_streaming
```

### Test with Week 3 (Observability)

```bash
# Build with metrics
cargo build -p kore-security -p kore-observability

# Run encryption with metrics collection
RUST_LOG=info cargo run --example encryption_example
```

### End-to-End Test

```bash
# Build all weeks
cd ../spark-kore && cargo build
cd ../kore-cloud && cargo build
cd ../kore-observability && cargo build
cd ../kore-streaming && cargo build
cd ../kore-security && cargo build

# Run all examples
for example in encryption_example audit_logging gdpr_compliance; do
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
  - Security best practices
  - Compliance standards

## Release Process

### Version Bumping

```bash
# Update Cargo.toml version
sed -i 's/version = "1.*/version = "1.1.0"/' Cargo.toml

# Rebuild to verify
cargo build

# Run full test suite
cargo test

# Security audit before release
cargo audit

# Tag release
git tag v1.1.0
git push origin v1.1.0
```

## Development Workflow

### Feature Branch Development

```bash
# Create feature branch
git checkout -b feature/hardware-security-module

# Develop with tests
cargo test

# Format and lint
cargo fmt
cargo clippy -- -D warnings

# Commit changes
git add .
git commit -m "Add HSM support"

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
cargo check && cargo fmt --check && cargo clippy && cargo test
```

## Security Testing

### Cryptographic Validation

```bash
# Test encryption round-trips
cargo test encrypt_decrypt

# Test key derivation consistency
cargo test key_derivation

# Test authentication (AAD)
cargo test aad
```

### GDPR Compliance Testing

```bash
# Test consent enforcement
cargo test gdpr_consent

# Test data deletion
cargo test gdpr_erasure

# Test retention policies
cargo test retention
```

## Performance Optimization

### Profile Guide

```bash
# Identify hotspots
cargo install cargo-flamegraph
cargo flamegraph --example encryption_example

# Memory analysis
cargo install cargo-heaptrack
cargo heaptrack run target/debug/examples/encryption_example

# Benchmark comparison
cargo bench
```

### Optimization Checklist

- [x] Use AES-NI hardware acceleration (via aes-gcm)
- [x] Minimize allocations in encryption loop
- [x] Use thread-safe Arc/Mutex for key storage
- [x] Async I/O for audit logging
- [ ] SIMD vectorization (future)
- [ ] GPU acceleration (future)

## Support & Debugging

### Get Build Information

```bash
cargo --version
rustc --version
uname -a

# Full environment
cargo build --verbose 2>&1 | head -20
```

### Report Security Issues

Include in security report:
- Specific vulnerability description
- Steps to reproduce
- Affected versions
- Proposed fix (if available)

**Important**: Send security reports to security@kore.dev, not GitHub issues

### Quick Help

```bash
# Clean rebuild
cargo clean && cargo build

# Fresh tests
cargo clean && cargo test

# Verbose output
RUST_LOG=trace cargo run --example encryption_example

# Check dependencies
cargo tree -p kore-security

# Audit dependencies
cargo audit --deny warnings
```

## Advanced Topics

### Custom Allocators

```rust
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;
```

### Hardware Security Module (HSM)

Future support for:
- PKCS#11
- Hardware key storage
- Secure key derivation
- Compliance certifications

### Key Rotation

```rust
// Future API for key rotation
let rotated_key = cipher.rotate_key(new_key)?;
```

---

**Last Updated**: June 2026
**Status**: Production Ready (Week 5)
