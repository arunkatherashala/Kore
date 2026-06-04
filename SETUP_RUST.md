# Rust Setup & Integration Guide for KORE v1.3.3

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Verification](#verification)
4. [KORE Integration](#kore-integration)
5. [Common Tasks](#common-tasks)
6. [Troubleshooting](#troubleshooting)

---

## Prerequisites

| Requirement | Minimum | Recommended | Notes |
|-------------|---------|-------------|-------|
| Rust Version | 1.56+ | Latest Stable | KORE uses Rust Edition 2021 |
| Rustup | Latest | Latest | Rust toolchain manager |
| Cargo | Latest | Latest | Rust package manager |
| OS Support | Windows 10+ | Windows 10, 11 | Also supports Linux/macOS |
| RAM | 2 GB | 4 GB | For compilation |
| Disk Space | 1 GB | 3 GB | Rust + target directory |

---

## Installation

### Step 1: Install Rust

**Official Installation (Recommended):**

```powershell
# Download and run installer
# From: https://rustup.rs/

# Or use automated install:
# Windows: Run rustup-init.exe from https://rustup.rs/

# After download, run:
.\rustup-init.exe

# During installation, select:
# [1] Proceed with installation (default)
# Default host triple: x86_64-pc-windows-msvc
# Profile: default
```

**Using Chocolatey:**
```powershell
choco install rust
```

**Using Windows Package Manager:**
```powershell
winget install Rustlang.Rust.MSVC
```

### Step 2: Verify Installation

```powershell
# Check Rust version
rustc --version

# Check Cargo version
cargo --version

# Check Rustup version
rustup --version

# Expected output:
# rustc 1.75.0 (or newer)
# cargo 1.75.0 (or newer)
# rustup 1.26.0 (or newer)
```

### Step 3: Configure Environment

```powershell
# Update Rust toolchain
rustup update

# Set default toolchain
rustup default stable

# Add build targets (optional)
rustup target add x86_64-pc-windows-gnu
rustup target add wasm32-unknown-unknown
```

---

## Verification

### Quick Check
```powershell
# Test Rust compiler
rustc --version

# Test Cargo
cargo --version

# Create test project
cargo new test_project
cd test_project

# Build and run
cargo run

# Clean up
cd ..
Remove-Item test_project -Recurse
```

### Detailed Environment Check

```powershell
# Show Rust toolchain info
rustup show

# Check installed toolchains
rustup toolchain list

# Check installed targets
rustup target list --installed

# Verify compiler
rustc --print sysroot

# Test cargo
cargo --list
```

---

## KORE Integration (KORE IS RUST!)

### KORE Project Structure

```
kore/
├── Cargo.toml                 # Project manifest
├── Cargo.lock                 # Dependency lock file
├── src/
│   ├── lib.rs                 # Library root
│   ├── main.rs                # Binary root (if applicable)
│   ├── kore_v2.rs             # KORE v2 format implementation
│   ├── decompression.rs       # Codec implementations
│   ├── transactions_v1.rs     # ACID transactions
│   ├── schema_evolution_v1.rs # Schema evolution
│   ├── ai_features.rs         # AI codec selection
│   └── ... (other modules)
├── tests/
│   └── integration_tests.rs
├── examples/
│   └── example_usage.rs
└── target/
    ├── debug/
    └── release/
```

### Building KORE

**Step 1: Ensure You're in KORE Directory**

```powershell
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"
```

**Step 2: Build Variants**

```powershell
# Debug build (fast compilation, slow runtime)
cargo build

# Release build (slow compilation, fast runtime)
cargo build --release

# Clean build
cargo clean

# Check without building
cargo check
```

**Step 3: Verify Build Success**

```powershell
# After successful build, check artifacts
Get-ChildItem target/release -Filter *.exe
```

---

## Common Tasks

### Running KORE Tests

```powershell
# Run all tests (debug mode)
cargo test

# Run all tests (release mode)
cargo test --release

# Run specific test
cargo test test_kore_v2

# Run tests with output
cargo test --release -- --nocapture

# Run with single thread
cargo test --release -- --test-threads=1

# Check test coverage
cargo tarpaulin --out Html --release
```

### Building KORE

```powershell
# Standard build
cargo build --release

# Build and time it
Measure-Command { cargo build --release }

# Build with specific profile
cargo build --release --profile release-lto

# Check compilation warnings
cargo check
```

### Managing Dependencies

```powershell
# Check for outdated dependencies
cargo outdated

# Update dependencies (respecting semver)
cargo update

# Add new dependency
cargo add dependency_name

# Add specific version
cargo add dependency_name@1.0.0

# Update specific dependency
cargo update -p dependency_name

# Remove dependency
# Edit Cargo.toml and remove the line, then:
cargo update
```

### Documentation

```powershell
# Generate documentation
cargo doc

# Generate and open in browser
cargo doc --open

# Generate for dependencies
cargo doc --open --document-private-items
```

### Running Examples

```powershell
# List available examples
cargo run --example list

# Run specific example
cargo run --example benchmark_kore

# Run with arguments
cargo run --example benchmark_kore -- --iterations 1000
```

---

## Troubleshooting

### Issue 1: "rustc not found" or "cargo not found"

**Solution:**
```powershell
# Reinstall Rust
.\rustup-init.exe

# Or update
rustup update

# Verify installation
rustc --version
cargo --version

# Restart PowerShell
```

### Issue 2: "Link error" during compilation

**Solution:**
```powershell
# Clean and rebuild
cargo clean
cargo build --release

# Update Rust
rustup update

# Check toolchain
rustup show
```

### Issue 3: Out of memory during compilation

**Solution:**
```powershell
# Build in debug mode (uses less memory)
cargo build

# Increase system resources
# Or split compilation into modules

# Use incremental compilation
$env:CARGO_INCREMENTAL = "1"
cargo build --release
```

### Issue 4: Tests failing

**Solution:**
```powershell
# Run tests with backtrace
$env:RUST_BACKTRACE = "1"
cargo test --release

# Run single test
cargo test test_name -- --nocapture

# Check for outdated dependencies
cargo outdated
```

### Issue 5: Compilation takes too long

**Solution:**
```powershell
# Use release build for better optimization
cargo build --release

# Parallel compilation (Rust does this by default)
cargo build -j 4  # Use 4 threads

# Check for expensive dependencies
cargo tree

# Consider incremental compilation
cargo build --incremental
```

---

## Best Practices

✅ **DO:**
- Always run `cargo test` before committing
- Use `cargo clippy` for code linting
- Use `cargo fmt` for code formatting
- Keep Cargo.toml organized
- Pin critical dependencies
- Use meaningful test names
- Write documentation comments (///)
- Use `#[must_use]` for important returns
- Test in release mode for performance

❌ **DON'T:**
- Use `unwrap()` in production code
- Ignore compiler warnings
- Commit `target/` directory
- Use `panic!` for error handling
- Ignore test failures
- Use `clone()` excessively
- Hard-code file paths
- Ignore performance warnings

---

## Rust Coding Patterns for KORE

### Error Handling
```rust
use std::io;

pub fn read_file(path: &str) -> Result<String, io::Error> {
    std::fs::read_to_string(path)
}

// Usage
match read_file("data.kore") {
    Ok(data) => println!("Read: {}", data),
    Err(e) => eprintln!("Error: {}", e),
}
```

### Testing
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression() {
        let data = vec![1, 2, 3, 4, 5];
        let result = compress(&data);
        assert!(!result.is_empty());
    }
}
```

### Benchmarking
```rust
// In Cargo.toml:
// [[bench]]
// name = "kore_bench"
// harness = false

// In benches/kore_bench.rs
#[bench]
fn bench_decompress(b: &mut Bencher) {
    let data = get_test_data();
    b.iter(|| decompress(&data));
}
```

---

## Advanced Configuration

### Custom Build Profile

Add to `Cargo.toml`:
```toml
[profile.release-lto]
inherits = "release"
lto = true
codegen-units = 1
opt-level = 3

[profile.bench]
inherits = "release"
debug = true
```

Use it:
```powershell
cargo build --profile release-lto
```

---

## Quick Reference

```powershell
# Project creation
cargo new project_name          # Binary project
cargo new --lib lib_name        # Library project

# Building
cargo build                     # Debug build
cargo build --release           # Release build
cargo clean                     # Clean artifacts
cargo check                     # Check without building

# Testing
cargo test                      # Run all tests
cargo test --release            # Tests in release mode
cargo test specific_test        # Run one test

# Code quality
cargo clippy                    # Linting
cargo fmt                       # Format code
cargo fmt --check               # Check formatting

# Documentation
cargo doc --open                # Generate & open docs

# Information
cargo --version                 # Cargo version
rustc --version                 # Rust version
cargo tree                      # Dependency tree
```

---

## KORE-Specific Commands

```powershell
# Build KORE release
cargo build --release

# Test entire KORE suite
cargo test --lib --release

# Run specific KORE module tests
cargo test --lib --release kore_v2::tests

# Generate KORE documentation
cargo doc --release --open

# Benchmark KORE performance
cargo test --lib --release -- --nocapture --test-threads=1

# Check for issues
cargo clippy --all-targets --release
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | KORE v1.3.3 Rust setup guide |

---

**Status: ✅ Production Ready**

**Note:** KORE v1.3.3 is built entirely in Rust. This is the primary language for KORE development.

**Next:** SQL Setup & Integration Guide (coming next)
