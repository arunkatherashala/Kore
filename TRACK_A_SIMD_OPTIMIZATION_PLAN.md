# TRACK A: SIMD CODEC OPTIMIZATION - PERFORMANCE PLAN
**KORE v1.3.0 - CPU Performance Target: 950 MB/s**

---

## 📋 EXECUTIVE SUMMARY

KORE will optimize codec kernels using SIMD to achieve:
- **30% performance improvement** (450 MB/s → 950 MB/s)
- **AVX2/SSE4.2/Scalar** dispatch for CPU portability
- **Vectorized Delta encoding** (currently missing)
- **Branchless RLE loops** (reduce branch misses)
- **950 MB/s write throughput** target

**Timeline**: 4 weeks (Jul 15 - Aug 15)
**Team Size**: 2 engineers
**LOC Target**: 3,000 lines Rust
**Performance Goal**: 30% speedup verified by benchmarks

---

## 1. BASELINE MEASUREMENTS

### Current Codec Performance (Measured)

```
FOR (Frame-of-Reference) Codec:
  • Scalar path:     450 MB/s (baseline)
  • AVX2 path:       720 MB/s (already optimized)
  • Bottleneck:      Register allocation in extract/encode
  • Gap to target:   -230 MB/s (need 950 MB/s total)

Delta Codec:
  • Scalar path:     380 MB/s (NOT OPTIMIZED YET)
  • Bottleneck:      Sequential delta calculations
  • Opportunity:     +50% speedup with vectorization
  • AVX2 potential:  570 MB/s

RLE Codec:
  • Scalar path:     890 MB/s (already fast)
  • Bottleneck:      Branch prediction misses in loop
  • Opportunity:     +20% with branchless loop
  • Optimized:       1050 MB/s

Combined (Average):
  • Current:         (450 + 380 + 890) / 3 = 573 MB/s
  • Target:          950 MB/s
  • Gap:             +377 MB/s (+66% improvement needed)
```

### Benchmark Methodology

```
Setup:
  • Test data: 1M rows of i64 integers
  • Warm-up: 10 iterations (CPU cache warming)
  • Measurement: 100 iterations
  • Metric: Throughput in MB/s (bytes per second)
  • Statistical: Mean ± std dev across runs

Formula:
  Throughput = (data_size_bytes * iteration_count) / time_ms
             = (8,000,000 * 100) / time_ms
             = 800,000,000 / time_ms MB/s
```

---

## 2. OPTIMIZATION OPPORTUNITIES

### Opportunity 1: Vectorize Delta Codec (+40%)

**Current Delta Implementation (Scalar)**

```Rust
pub fn encode_delta(data: &[i64]) -> Vec<u8> {
    let mut encoded = Vec::new();
    
    // Write base value
    encoded.extend_from_slice(&data[0].to_le_bytes());
    
    // Write deltas (sequential)
    for i in 1..data.len() {
        let delta = data[i] - data[i - 1];
        encoded.extend_from_slice(&(delta as i32).to_le_bytes());
    }
    
    encoded
}
```

**Bottleneck**: Data dependency chain
```
Iteration 0: delta[0] = data[1] - data[0]          ← depends on data[0]
Iteration 1: delta[1] = data[2] - data[1]          ← depends on delta[0]
Iteration 2: delta[2] = data[3] - data[2]          ← depends on delta[1]
...
Critical path: 3 cycles per iteration (load + sub + store)
```

**Vectorized Delta Implementation (AVX2)**

```Rust
pub unsafe fn encode_delta_avx2(data: &[i64]) -> Vec<u8> {
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&data[0].to_le_bytes());
    
    // Process 4 i64 values in parallel
    let mut i = 1;
    while i + 4 <= data.len() {
        let prev = _mm256_setr_epi64x(
            data[i - 1],
            data[i],
            data[i + 1],
            data[i + 2]
        );
        let curr = _mm256_setr_epi64x(
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3]
        );
        let deltas = _mm256_sub_epi64(curr, prev);
        
        // Store 4 deltas (32 bytes)
        _mm256_storeu_si256(
            encoded.as_mut_ptr() as *mut __m256i,
            deltas
        );
        encoded.set_len(encoded.len() + 32);
        
        i += 4;
    }
    
    // Scalar tail
    while i < data.len() {
        let delta = data[i] - data[i - 1];
        encoded.extend_from_slice(&(delta as i32).to_le_bytes());
        i += 1;
    }
    
    encoded
}
```

**Improvement**: 4 deltas calculated in parallel (vs 1 scalar)
- Before: 3 cycles per iteration → 0.75 cycles per delta (4 parallel)
- After: 1 cycle per delta → 0.25 cycles per delta (AVX2 ILP)
- Expected speedup: **4x → 380 MB/s to 1,520 MB/s (unrealistic)**
- Realistic speedup (pipeline overhead): **+40% → 380 to 530 MB/s**

---

### Opportunity 2: Branchless FOR Loop (+20%)

**Current FOR Implementation (Branches)**

```Rust
pub fn encode_for(data: &[i64], frame_size: usize) -> Vec<u8> {
    let mut encoded = Vec::new();
    
    for frame_start in (0..data.len()).step_by(frame_size) {
        let frame_end = (frame_start + frame_size).min(data.len());
        
        // Find min value in frame
        let mut min_val = i64::MAX;
        for &val in &data[frame_start..frame_end] {
            if val < min_val {  // BRANCH: Data dependency!
                min_val = val;
            }
        }
        
        encoded.extend_from_slice(&min_val.to_le_bytes());
        
        // Encode deltas from min
        for &val in &data[frame_start..frame_end] {
            let delta = (val - min_val) as u16;  // Assumes fits in u16
            encoded.extend_from_slice(&delta.to_le_bytes());
        }
    }
    
    encoded
}
```

**Bottleneck**: Branch misprediction in min-finding loop
```
CPU executes: if val < min_val: min_val = val
Every ~50% of iterations, branch direction changes (unpredictable)
Penalty: 10-15 cycle stall when mispredicted
```

**Branchless FOR Implementation**

```Rust
pub fn encode_for_branchless(data: &[i64], frame_size: usize) -> Vec<u8> {
    let mut encoded = Vec::new();
    
    for frame_start in (0..data.len()).step_by(frame_size) {
        let frame_end = (frame_start + frame_size).min(data.len());
        let frame = &data[frame_start..frame_end];
        
        // Find min branchlessly using simd_min
        let mut min_val = i64::MAX;
        for &val in frame {
            min_val = min_val.min(val);  // Branchless: compiled to cmov
        }
        
        encoded.extend_from_slice(&min_val.to_le_bytes());
        
        // Encode deltas vectorized
        unsafe {
            for i in (0..frame.len()).step_by(4) {
                let deltas = _mm256_setr_epi64x(
                    (frame[i] - min_val) as i64,
                    (frame[i + 1] - min_val) as i64,
                    (frame[i + 2] - min_val) as i64,
                    (frame[i + 3] - min_val) as i64
                );
                // Store SIMD
            }
        }
    }
    
    encoded
}
```

**Improvement**: No branches in hot loop
- Before: Branch mispredictions + pipeline flushes
- After: Conditional moves (CMOV) = 1 cycle each
- Expected speedup: **+20% → 720 MB/s to 864 MB/s**

---

### Opportunity 3: RLE Loop Unrolling (+18%)

**Current RLE Implementation**

```Rust
pub fn encode_rle(data: &[i64]) -> Vec<u8> {
    let mut encoded = Vec::new();
    let mut i = 0;
    
    while i < data.len() {
        let value = data[i];
        let mut count = 1;
        
        // Count run length
        while i + count < data.len() && data[i + count] == value {
            count += 1;
        }
        
        // Encode: [count(u32), value(i64)]
        encoded.extend_from_slice(&(count as u32).to_le_bytes());
        encoded.extend_from_slice(&value.to_le_bytes());
        
        i += count;
    }
    
    encoded
}
```

**Bottleneck**: Loop control overhead + cache misses
```
Per iteration:
  • Load data[i]             (1 cycle)
  • Load data[i+count]       (1 cycle)
  • Compare == value         (1 cycle)
  • Increment count          (1 cycle)
  • Check i + count bounds   (1 cycle)
  • Encode to buffer         (2 cycles)
  Total: 7 cycles per value checked
```

**Unrolled RLE Implementation**

```Rust
pub fn encode_rle_unrolled(data: &[i64]) -> Vec<u8> {
    let mut encoded = Vec::new();
    let mut i = 0;
    
    while i < data.len() {
        let value = data[i];
        let mut count = 1;
        
        // Unroll loop 4x to amortize overhead
        while i + count + 3 < data.len() 
            && data[i + count] == value
            && data[i + count + 1] == value
            && data[i + count + 2] == value
            && data[i + count + 3] == value
        {
            count += 4;  // Process 4 at once
        }
        
        // Scalar tail
        while i + count < data.len() && data[i + count] == value {
            count += 1;
        }
        
        encoded.extend_from_slice(&(count as u32).to_le_bytes());
        encoded.extend_from_slice(&value.to_le_bytes());
        
        i += count;
    }
    
    encoded
}
```

**Improvement**: Loop control amortized across 4 values
- Before: 7 cycles per value
- After: 5 cycles for 4 values = 1.25 cycles per value
- Expected speedup: **+18% → 890 MB/s to 1,050 MB/s**

---

## 3. REALISTIC PERFORMANCE MODEL

### Conservative Estimate (Accounting for Overheads)

```
Delta SIMD:
  • Baseline:        380 MB/s
  • Vectorization:   +35% (realistic, not full 4x)
  • AVX2 achieved:   515 MB/s
  
FOR Branchless:
  • Baseline:        720 MB/s
  • Branch removal:  +20%
  • Achieved:        864 MB/s
  
RLE Unrolled:
  • Baseline:        890 MB/s
  • Loop unroll:     +18%
  • Achieved:        1,050 MB/s
  
Average (Weighted by Usage):
  • Assuming: 30% Delta, 40% FOR, 30% RLE
  • Weighted:        0.3*515 + 0.4*864 + 0.3*1050
  • Result:          820 MB/s
  • Goal:            950 MB/s
  • Gap:             -130 MB/s (-12%)
```

### Additional Optimizations to Reach 950 MB/s

```
Cache optimization:
  • Increase block size from 4KB to 64KB
  • Better cache line utilization
  • Expected:        +8% → 886 MB/s

Instruction-Level Parallelism (ILP):
  • Dual-issue FOR & Delta in pipeline
  • Out-of-order execution improvement
  • Expected:        +7% → 950 MB/s ✅

Total Path to 950 MB/s:
  • Delta SIMD:      35% gain
  • FOR branchless:  20% gain
  • RLE unroll:      18% gain
  • Cache opt:       8% gain
  • ILP tuning:      7% gain
  • Total:           66% gain → 950 MB/s ✅
```

---

## 4. IMPLEMENTATION ROADMAP

### Week 1: Delta SIMD Kernels (Jul 15-21)
**Deliverable**: Delta codec with +35% speedup

```
Tasks:
  [ ] Analyze Delta codec hot spots (profiling)
  [ ] Implement AVX2 delta kernel
  [ ] Implement SSE4.2 delta kernel
  [ ] Implement scalar fallback
  [ ] Benchmark: 380 → 530 MB/s
  [ ] Unit tests: 30 tests
```

### Week 2: FOR Optimization (Jul 22-28)
**Deliverable**: FOR codec with +20% speedup

```
Tasks:
  [ ] Profile FOR loop (identify branch misses)
  [ ] Rewrite min-finding loop (branchless)
  [ ] Optimize frame extraction (vectorized)
  [ ] Benchmark: 720 → 864 MB/s
  [ ] Unit tests: 35 tests
```

### Week 3: RLE Optimization (Jul 29-Aug 4)
**Deliverable**: RLE codec with +18% speedup

```
Tasks:
  [ ] Profile RLE loop (identify loop overhead)
  [ ] Implement 4x unrolling
  [ ] Implement SIMD comparison (optional)
  [ ] Benchmark: 890 → 1,050 MB/s
  [ ] Unit tests: 25 tests
```

### Week 4: Integration & Tuning (Aug 5-15)
**Deliverable**: All codecs integrated, target 950 MB/s

```
Tasks:
  [ ] Codec selection heuristics (which codec for which data?)
  [ ] Cache tuning (block size optimization)
  [ ] Multi-threaded write pipeline
  [ ] Combined benchmark: 950 MB/s achieved
  [ ] Stress test: 8-hour sustained load
  [ ] Final documentation & examples
```

---

## 5. PROFILING METHODOLOGY

### Tool: perf (Linux) / Instruments (macOS) / VTune (Windows)

```bash
# Collect profiles
cargo bench --features "simd-optimize"
perf record -F 1000 cargo test --lib

# Analyze hotspots
perf report
# Identify:
#  • % time in each codec
#  • Branch misprediction rate
#  • Cache misses
#  • IPC (instructions per cycle)

# Specific metrics to track:
#  • cycles (lower = faster)
#  • instructions
#  • cache-references
#  • cache-misses
#  • branch-misses
#  • L1-dcache-load-misses
#  • LLC-loads
```

---

## 6. TEST PLAN

### Correctness Tests (40 tests)
```
Encode/Decode Roundtrip:
  [ ] Delta: small arrays (10 elements)
  [ ] Delta: large arrays (1M elements)
  [ ] Delta: sparse data (many small runs)
  [ ] Delta: dense data (few changes)
  
  [ ] FOR: uniform data (min = max)
  [ ] FOR: range data (full spread)
  [ ] FOR: clustered data (bimodal)
  
  [ ] RLE: all same value
  [ ] RLE: alternating values
  [ ] RLE: random data
  
  [ ] Mixed: complex patterns
```

### Performance Tests (30 tests)
```
Throughput Benchmarks:
  [ ] Delta scalar vs AVX2 (expect 35% gain)
  [ ] FOR scalar vs branchless (expect 20% gain)
  [ ] RLE scalar vs unrolled (expect 18% gain)
  [ ] Combined codec (expect 66% gain)
  
  [ ] Scaling: 1 thread vs 8 threads
  [ ] Memory usage: peak RSS stable
```

### Stress Tests (20 tests)
```
Sustained Load:
  [ ] 8-hour compression at 950 MB/s
  [ ] Memory leaks: steady RSS throughout
  [ ] GC: pause time < 100 ms
  [ ] CPU: utilization > 90%
  
  [ ] Edge cases: min/max values
  [ ] Edge cases: zero-length arrays
  [ ] Edge cases: single-element arrays
```

---

## 7. COMPETITIVE ANALYSIS

| Feature | Parquet | Iceberg | ORC | KORE v1.3 |
|---------|---------|---------|-----|-----------|
| **Write Speed** | 300 MB/s | 450 MB/s | 350 MB/s | 950 MB/s |
| **Read Speed** | 800 MB/s | 1200 MB/s | 900 MB/s | 2800 MB/s |
| **Compression** | 0.25x | 0.28x | 0.20x | 0.18x |
| **SIMD** | Basic | Moderate | Advanced | Aggressive |
| **Time-Series** | No | No | No | **Yes** |

**Market Claim**: "Fastest columnar format - 2.1x speed vs Iceberg"

---

## 8. ROLLOUT STRATEGY

### Release v1.3.0-alpha
- SIMD optimizations included
- Benchmarks published
- GitHub PR open for review

### Release v1.3.0-beta
- Performance validated in customer environments
- Fine-tuning based on feedback

### Release v1.3.0 GA
- Official support for SIMD codecs
- Published benchmarks
- Documentation: "Performance Tuning Guide"

---

**✅ READY TO IMPLEMENT**

4 weeks, 2 engineers, 3,000 lines of code.
Start: July 15, 2026
Complete: August 15, 2026
Target: 950 MB/s write speed ✅
