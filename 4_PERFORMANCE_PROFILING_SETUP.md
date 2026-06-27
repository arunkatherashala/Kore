# KORE v1.2.1 Performance Profiling Setup Guide

**Purpose**: Establish baseline and profiling environment for v1.2.1 performance optimization  
**Timeline**: Week 1 of v1.2.1 (June 1-7, 2026)  
**Output**: Baseline metrics, profiling tools configured, benchmarks ready  
**Owner**: Performance Engineering Team

---

## 📊 Profiling Objectives

### Primary Goals

1. **Establish Baseline** (v1.2.0)
   - Throughput: 19.1 GB/s (verify)
   - Compression ratio: 42.1% (verify)
   - Latency: 0.05-0.12ms (verify)

2. **Identify Hotspots**
   - CPU-intensive functions
   - Memory allocation patterns
   - Cache efficiency issues
   - Branch prediction failures

3. **Create Benchmark Suite**
   - Small files (1MB)
   - Medium files (100MB)
   - Large files (1GB)
   - Huge files (10GB+)

4. **Set Up Continuous Profiling**
   - Automated benchmark runs
   - Performance regression detection
   - Historical tracking

---

## 🛠️ Tool Installation

### Prerequisites

```bash
# Rust (already installed)
rustup --version

# Git
git --version

# Required system packages (Ubuntu/Debian)
sudo apt-get update
sudo apt-get install -y build-essential linux-tools-generic \
                       valgrind massif-visualizer pkg-config

# macOS (if needed)
brew install massif-visualizer

# Windows (WSL recommended)
# Use Ubuntu subsystem with above commands
```

### Performance Profiling Tools

#### 1. Cargo Flamegraph (CPU profiling)

```bash
# Install
cargo install flamegraph

# Usage
cargo flamegraph --bench benchmark_suite
# Output: flamegraph.svg
```

#### 2. Cargo Bench (Benchmarking)

```bash
# Already included with Rust
cargo bench --bench benchmark_suite --verbose
```

#### 3. Valgrind + Massif (Memory profiling)

```bash
# Install
cargo install valgrind
# Then use: valgrind --tool=massif

# Alternative: cargo-valgrind
cargo install cargo-valgrind
```

#### 4. perf (Linux CPU profiling)

```bash
# Install (Linux only)
sudo apt-get install linux-tools-generic

# Usage
perf record -F 100 ./target/release/benchmark_suite
perf report
```

#### 5. Criterion.rs (Statistical benchmarking)

```bash
# Already in Cargo.toml (dev-dependency)
# Provides automatic statistical analysis
```

---

## 📁 Benchmark Suite Setup

### 1. Create Benchmark Directory

```bash
cd /path/to/kore
mkdir -p benches
mkdir -p data/benchmarks/{1mb,100mb,1gb,10gb}
```

### 2. Generate Test Data

**benches/generate_test_data.rs**:

```rust
use std::io::Write;
use std::fs::File;

fn main() {
    generate_random_data("data/benchmarks/1mb/random.bin", 1_000_000);
    generate_repetitive_data("data/benchmarks/1mb/repetitive.bin", 1_000_000);
    generate_json_data("data/benchmarks/1mb/data.json", 1_000_000);
    
    println!("Generated test data files");
}

fn generate_random_data(path: &str, size: usize) {
    let mut file = File::create(path).unwrap();
    for _ in 0..size {
        file.write_all(&[(rand::random::<u8>())]).unwrap();
    }
}

fn generate_repetitive_data(path: &str, size: usize) {
    let mut file = File::create(path).unwrap();
    let pattern = b"ABCDEFGHIJ".repeat(1000);
    for _ in 0..(size / pattern.len()) {
        file.write_all(&pattern).unwrap();
    }
}

fn generate_json_data(path: &str, size: usize) {
    let mut file = File::create(path).unwrap();
    let record = r#"{"id":123,"name":"test","value":456.78,"timestamp":"2026-05-21T12:00:00Z"}"#;
    for i in 0..(size / record.len()) {
        let json = format!(r#"{{"id":{},"name":"test{}","value":{}.00}}"#, i, i, i % 1000);
        file.write_all(json.as_bytes()).unwrap();
    }
}
```

### 3. Main Benchmark Suite (benches/benchmark_suite.rs)

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use kore_fileformat::*;
use std::fs;

fn benchmark_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression");
    group.measurement_time(std::time::Duration::from_secs(30));
    
    // 1MB test
    let data_1mb = fs::read("data/benchmarks/1mb/random.bin").unwrap();
    group.bench_with_input(BenchmarkId::new("1mb_random", "compress"), 
        &data_1mb, |b, data| {
        b.iter(|| compress(black_box(data)))
    });
    
    // 100MB test
    let data_100mb = fs::read("data/benchmarks/100mb/random.bin").unwrap();
    group.bench_with_input(BenchmarkId::new("100mb_random", "compress"),
        &data_100mb, |b, data| {
        b.iter(|| compress(black_box(data)))
    });
    
    group.finish();
}

fn benchmark_decompression(c: &mut Criterion) {
    let data = fs::read("data/benchmarks/100mb/random.bin").unwrap();
    let compressed = compress(&data);
    
    let mut group = c.benchmark_group("decompression");
    group.bench_function("decompress_100mb", |b| {
        b.iter(|| decompress(black_box(&compressed)))
    });
    group.finish();
}

criterion_group!(benches, 
    benchmark_compression,
    benchmark_decompression
);
criterion_main!(benches);
```

### 4. Cargo.toml Benchmark Configuration

```toml
[[bench]]
name = "benchmark_suite"
harness = false

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
rand = "0.8"
```

---

## 🔍 Baseline Measurement Process

### Step 1: Build Optimized Binary

```bash
# Release build with optimizations
cargo build --release --bench benchmark_suite

# Alternative: With profiling-guided optimization (PGO)
RUSTFLAGS="-Cllvm-args=-pgo-warn-missing-function" \
cargo build --release --bench benchmark_suite
```

### Step 2: Run Baseline Benchmarks

```bash
# Run criterion benchmarks
cargo bench --bench benchmark_suite -- --verbose --output-format bencher

# Save results
cargo bench --bench benchmark_suite > baseline_results_v1.2.0.txt
```

### Step 3: Generate Flamegraph

```bash
# Profile with flamegraph
CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph \
    --bench benchmark_suite \
    -- data/benchmarks/1gb/random.bin

# View result
open flamegraph.svg  # macOS
xdg-open flamegraph.svg  # Linux
start flamegraph.svg  # Windows
```

### Step 4: Memory Profiling

```bash
# Build with symbols
cargo build --release

# Run with Valgrind
valgrind --tool=massif \
    ./target/release/deps/benchmark_suite-* \
    data/benchmarks/100mb/random.bin

# Visualize
massif-visualizer massif.out.*
```

### Step 5: CPU Profiling (perf)

```bash
# Record
perf record -F 100 -o perf.data \
    ./target/release/deps/benchmark_suite-* \
    data/benchmarks/100mb/random.bin

# Analyze
perf report -i perf.data
perf annotate -i perf.data
```

---

## 📋 Baseline Results Template

**Create `BASELINE_v1.2.0.md`**:

```markdown
# KORE v1.2.0 Baseline Metrics

## Hardware Environment

- CPU: [model]
- RAM: [size]
- Storage: [type - SSD/HDD]
- OS: [name and version]
- Date: May 21, 2026

## Throughput Benchmarks

### Compression Speed

| Data Type | Size | Throughput | Ratio |
|-----------|------|-----------|-------|
| Random | 1MB | [GB/s] | [%] |
| Random | 100MB | [GB/s] | [%] |
| Random | 1GB | [GB/s] | [%] |
| Repetitive | 1MB | [GB/s] | [%] |
| Repetitive | 100MB | [GB/s] | [%] |
| JSON | 1MB | [GB/s] | [%] |
| JSON | 100MB | [GB/s] | [%] |

### Decompression Speed

| Data Type | Size | Throughput |
|-----------|------|-----------|
| Random | 1MB | [GB/s] |
| Random | 100MB | [GB/s] |
| Random | 1GB | [GB/s] |

## Latency Metrics

- Metadata extraction: [ms]
- Dictionary building: [ms]
- Pattern matching: [ms]

## Memory Usage

- Compression 1MB: [MB]
- Compression 100MB: [MB]
- Decompression 100MB: [MB]
- Peak allocation: [MB]

## CPU Efficiency

- L1 cache hit rate: [%]
- L2 cache hit rate: [%]
- L3 cache hit rate: [%]
- Branch prediction accuracy: [%]
- IPC (Instructions Per Cycle): [#]

## Hot Functions (Top 5)

1. [Function name] - [% of time]
2. [Function name] - [% of time]
3. [Function name] - [% of time]
4. [Function name] - [% of time]
5. [Function name] - [% of time]

## Analysis & Notes

[Observations from profiling]
```

---

## 🔄 Continuous Profiling Setup

### Automated Benchmark Runs

**GitHub Actions Workflow (.github/workflows/benchmark.yml)**:

```yaml
name: Performance Benchmarks

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]
  schedule:
    - cron: '0 0 * * 0'  # Weekly Sunday

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - uses: rust-lang/rust-toolchain@v1
        with:
          toolchain: stable
      
      - name: Generate test data
        run: cargo run --release --bin generate_test_data
      
      - name: Run benchmarks
        run: cargo bench --bench benchmark_suite -- --output-format bencher
      
      - name: Store benchmark
        uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: 'cargo'
          output-file-path: target/criterion/output.txt
          github-token: ${{ secrets.GITHUB_TOKEN }}
          auto-push: true
```

### Performance Regression Detection

```yaml
      - name: Check performance
        run: |
          if (benchmark_score < baseline - 5%); then
            echo "WARNING: Performance regression detected!"
            exit 1
          fi
```

---

## 📊 Profiling Checklist

### Week 1: Initial Profiling

- [ ] Install all profiling tools
- [ ] Generate test data (1MB, 100MB, 1GB)
- [ ] Run baseline benchmarks
- [ ] Generate flamegraph
- [ ] Profile memory usage
- [ ] Document baseline metrics
- [ ] Identify top 5 hotspots
- [ ] Create comparison baseline

### Ongoing: Continuous Monitoring

- [ ] Weekly benchmark runs
- [ ] Automatic regression detection
- [ ] Track improvements per optimization
- [ ] Compare optimizations against baseline

---

## 🎯 Key Metrics to Track

**Throughput**:
- GB/s for compression
- GB/s for decompression
- Ratio vs previous version

**Performance**:
- Latency (milliseconds)
- Peak memory (MB)
- Cache efficiency (%)

**Quality**:
- 100% data integrity
- Zero regressions
- IPC improvement

---

## 📞 Troubleshooting

### Issue: Flamegraph not generating

```bash
# Solution: Install dependencies
cargo install flamegraph
# Ensure perf is installed
which perf || sudo apt-get install linux-tools-generic
```

### Issue: Benchmark results too noisy

```bash
# Solution: Increase measurement time
cargo bench --bench benchmark_suite -- --measurement-time 60

# Or run multiple times
for i in {1..5}; do cargo bench --bench benchmark_suite; done
```

### Issue: Memory profiling crashes

```bash
# Solution: Build with symbols
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release
valgrind --leak-check=full ./target/release/binary
```

---

## ✅ Success Criteria

- ✅ All tools installed and working
- ✅ Test data generated (all sizes)
- ✅ Baseline benchmarks completed
- ✅ Flamegraph generated & analyzed
- ✅ Hotspots identified
- ✅ Continuous profiling configured
- ✅ Regression detection active
- ✅ Documentation complete

---

**Last Updated**: May 21, 2026  
**Owner**: Performance Engineering  
**Status**: Ready for Week 1 execution (June 1)  
**Expected Completion**: June 7, 2026
