# Kore Cloud Integration - Build Guide

## Prerequisites

- Rust 1.70+
- Cargo
- For S3: AWS credentials (environment variables or ~/.aws/credentials)
- For GCS: Google credentials (GOOGLE_APPLICATION_CREDENTIALS)
- For Azure: Azure Storage credentials (environment variables)

## Build Commands

### Standard Build

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

### Feature-Specific Builds

```bash
# S3 only
cargo build --no-default-features --features s3

# GCS only
cargo build --no-default-features --features gcs

# Azure only
cargo build --no-default-features --features azure

# All features
cargo build --features "s3,gcs,azure"
```

## Testing

### Unit Tests

```bash
# Run all tests
cargo test

# Run with logging
RUST_LOG=debug cargo test -- --nocapture

# Test specific module
cargo test s3_reader
cargo test gcs_reader
cargo test azure_reader

# Single test
cargo test test_range_request_new
```

### Integration Tests

Requires valid cloud credentials:

```bash
# S3 tests
cargo test s3 -- --test-threads=1

# GCS tests
cargo test gcs -- --test-threads=1

# Azure tests
cargo test azure -- --test-threads=1
```

## Running Examples

### S3 Example

Requires AWS credentials and valid S3 bucket:

```bash
export AWS_REGION=us-west-2
cargo run --example s3_reader_example --features s3
```

### GCS Example

Requires Google Cloud credentials:

```bash
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/credentials.json
cargo run --example gcs_reader_example --features gcs
```

### Azure Example

Requires Azure Storage credentials:

```bash
export AZURE_STORAGE_ACCOUNT_NAME=myaccount
export AZURE_STORAGE_ACCOUNT_KEY=xxxxx
cargo run --example azure_reader_example --features azure
```

## Development Workflow

### 1. Setup Development Environment

```bash
# Clone repository
git clone https://github.com/arunkatherashala/Kore
cd kore-cloud

# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version
cargo --version
```

### 2. Configure Cloud Credentials

**AWS S3**:
```bash
# Option 1: Environment variables
export AWS_ACCESS_KEY_ID=xxxxx
export AWS_SECRET_ACCESS_KEY=xxxxx
export AWS_REGION=us-west-2

# Option 2: Credentials file
mkdir -p ~/.aws
cat > ~/.aws/credentials << EOF
[default]
aws_access_key_id = xxxxx
aws_secret_access_key = xxxxx
EOF

# Option 3: Temporary credentials
export AWS_SESSION_TOKEN=xxxxx
```

**Google Cloud Storage**:
```bash
# Option 1: Service account JSON
export GOOGLE_APPLICATION_CREDENTIALS=~/gcp-key.json

# Option 2: Application default credentials
gcloud auth application-default login
```

**Azure Blob Storage**:
```bash
# Option 1: Account name and key
export AZURE_STORAGE_ACCOUNT_NAME=myaccount
export AZURE_STORAGE_ACCOUNT_KEY=xxxxx

# Option 2: Connection string
export AZURE_STORAGE_CONNECTION_STRING="DefaultEndpointsProtocol=https;AccountName=...;AccountKey=...;EndpointSuffix=core.windows.net"

# Option 3: Managed identity (Azure-hosted)
export AZURE_AUTHORITY_HOST=https://login.microsoftonline.com
export AZURE_TENANT_ID=xxxxx
export AZURE_CLIENT_ID=xxxxx
export AZURE_CLIENT_SECRET=xxxxx
```

### 3. Development Build

```bash
# Build with all features
cargo build --release

# Check for errors
cargo check

# Run linter
cargo clippy

# Format code
cargo fmt
```

### 4. Testing Before Commit

```bash
# Run all tests
cargo test --all

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_s3_reader_path

# Check code coverage (requires tarpaulin)
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```

### 5. Documentation

```bash
# Generate documentation
cargo doc --open

# Check documentation links
cargo doc --document-private-items
```

## Troubleshooting

### "AWS credentials not found"

```bash
# Check credentials
aws configure

# Or set environment variables
export AWS_ACCESS_KEY_ID=xxxxx
export AWS_SECRET_ACCESS_KEY=xxxxx
export AWS_REGION=us-west-2

# Verify
aws s3 ls
```

### "GCS authentication failed"

```bash
# Check credentials file
ls -la $GOOGLE_APPLICATION_CREDENTIALS

# Test credentials
gcloud auth list

# Or set default credentials
gcloud auth application-default login
```

### "Azure credentials missing"

```bash
# Check environment variables
echo $AZURE_STORAGE_ACCOUNT_NAME
echo $AZURE_STORAGE_ACCOUNT_KEY

# Or use connection string
export AZURE_STORAGE_CONNECTION_STRING="..."

# Test connection
az storage account show -n myaccount
```

### Build Errors

**"failed to resolve: use of undeclared crate"**
```bash
# Ensure features are enabled
cargo build --features "s3,gcs,azure"

# Update dependencies
cargo update
```

**"error: failed to fetch repository"**
```bash
# Check network connection
cargo update --aggressive

# Or use offline mode if available
cargo build --offline
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
      - run: cargo build --release
      - run: cargo test
```

### Local Pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit

cargo check
if [ $? -ne 0 ]; then
  echo "cargo check failed"
  exit 1
fi

cargo fmt -- --check
if [ $? -ne 0 ]; then
  echo "cargo fmt failed - run: cargo fmt"
  exit 1
fi

cargo clippy
if [ $? -ne 0 ]; then
  echo "cargo clippy failed"
  exit 1
fi
```

## Performance Profiling

```bash
# Build with debug symbols
cargo build --release

# Profile with perf (Linux)
perf record -g target/release/examples/s3_reader_example
perf report

# Profile with Instruments (macOS)
cargo instruments -t "System Trace"

# Profile with flamegraph
cargo install flamegraph
cargo flamegraph --example s3_reader_example
```

## Dependency Management

### Update Dependencies

```bash
# Check for updates
cargo update --dry-run

# Update all dependencies
cargo update

# Update specific dependency
cargo update -p rusoto_s3
```

### Security Audit

```bash
# Install audit tool
cargo install cargo-audit

# Check for vulnerabilities
cargo audit

# Fix vulnerabilities
cargo audit fix
```

### Size Optimization

```bash
# Check binary size
ls -lh target/release/

# Optimize for size
cargo build --release -C opt-level=z -C lto=fat

# Strip binary
strip target/release/examples/*
```

## Platform-Specific Notes

### Windows

```bash
# Install Visual Studio Build Tools (if needed)
# Then build normally
cargo build --release

# Or use pre-built MSVC toolchain
rustup install stable-msvc
rustup default stable-msvc
```

### macOS

```bash
# Ensure Xcode command line tools installed
xcode-select --install

# Build as normal
cargo build --release
```

### Linux

```bash
# Install build essentials
sudo apt-get install build-essential

# Build
cargo build --release
```

## Documentation Generation

```bash
# Generate and open docs
cargo doc --no-deps --open

# Include private items
cargo doc --no-deps --document-private-items --open

# Build with default-target
cargo doc --target x86_64-unknown-linux-gnu
```

## Release Process

1. Update version in `Cargo.toml`
2. Run full test suite: `cargo test --all`
3. Build release: `cargo build --release`
4. Tag release: `git tag vX.Y.Z`
5. Push to registry: `cargo publish`

```bash
# Example
cargo publish --dry-run
cargo publish
```

## Getting Help

- Check [Rust Book](https://doc.rust-lang.org/book/)
- Refer to [Cargo documentation](https://doc.rust-lang.org/cargo/)
- Visit [Rust by Example](https://doc.rust-lang.org/rust-by-example/)
- Ask on [Stack Overflow](https://stackoverflow.com/questions/tagged/rust)
