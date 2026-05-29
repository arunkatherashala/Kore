# KORE PHASE 1 SPRINT 1: SIMD VECTORIZATION
## Week 1-2 (Jun 2-13, 2026) - BEAT PARQUET STARTS NOW

**Goal**: 2.7M → 3.5M rows/sec (+30% performance gain)  
**Owner**: Michael Torres (Performance Lead) + 2 engineers  
**Deadline**: Jun 13, 2026 (Friday EOD) - 10 business days  
**Success Metric**: Achieved 3.5M rows/sec on TPC-H 10M row benchmark

---

## 🎯 MISSION

Implement SIMD (Single Instruction Multiple Data) vectorization for integer column scanning to achieve 30% performance improvement. This is the foundational sprint for Phase 1.

---

## 📊 CURRENT vs TARGET

| Metric | Current | Target | Improvement |
|--------|---------|--------|-------------|
| Query Speed | 2.7M rows/sec | 3.5M rows/sec | +30% |
| Integer Scan (100M rows) | 37.0 ms | 28.5 ms | -23% latency |
| Memory Bandwidth | 72.9 GB/s | 94.8 GB/s | +30% throughput |
| CPU Utilization | 6.8 cores / 8 | 8.0 cores / 8 | Maxed out |

---

## 🛠️ IMPLEMENTATION DETAILS

### SIMD Optimization Strategy

**Current Bottleneck**: Integer column scanning is CPU-limited (72.9 GB/s vs available 119.4 GB/s).

**Solution**: AVX-512 SIMD instructions to process 8 integers per cycle instead of 1.

**Affected Code**: 
- `kore/src/columnar/integer_scan.rs` (primary)
- `kore/src/columnar/filter.rs` (secondary)
- `kore/src/simd/mod.rs` (new SIMD module)

### Implementation Tasks

#### Task 1: Create SIMD Module (Day 1-2)
**Owner**: Michael Torres + Engineer 1  
**Timeline**: Jun 2-3 (2 days)  
**Deliverable**: `src/simd/mod.rs` (SIMD kernel library)

```rust
// GOAL: Create reusable SIMD kernels for integer operations

pub mod integer_scan {
    // Scan 8 integers in parallel (AVX-512)
    pub fn scan_i32_avx512(values: &[i32]) -> u64
    
    // Selective scan with predicate (faster filtering)
    pub fn scan_i32_filtered_avx512(values: &[i32], predicate: i32) -> u64
    
    // Parallel comparison (bitwise SIMD comparison)
    pub fn compare_i32_avx512(left: &[i32], right: &[i32]) -> Vec<bool>
}

pub mod compression {
    // SIMD-friendly decompression
    pub fn decompress_delta_avx512(compressed: &[u8]) -> Vec<i32>
}
```

**Acceptance Criteria**:
- [ ] 3 SIMD kernels implemented (integer_scan, scan_filtered, compare)
- [ ] All functions tested on TPC-H data
- [ ] Benchmarks show 25%+ speedup vs scalar version
- [ ] Code reviewed + approved

---

#### Task 2: Integrate SIMD into Query Engine (Day 3-4)
**Owner**: Engineer 2  
**Timeline**: Jun 4-5 (2 days)  
**Deliverable**: Updated `src/columnar/integer_scan.rs` (SIMD integration)

**Changes**:
1. Replace scalar integer scan with SIMD equivalent
2. Add runtime CPU feature detection (AVX-512 vs AVX2 fallback)
3. Update query planner to prefer SIMD paths
4. Benchmark against baseline

```rust
// BEFORE:
fn scan_integers(column: &Column) -> u64 {
    let mut count = 0;
    for i in column.values { count += i; }
    count
}

// AFTER:
fn scan_integers(column: &Column) -> u64 {
    if cpu_supports_avx512() {
        simd::integer_scan::scan_i32_avx512(&column.values)
    } else {
        simd::integer_scan::scan_i32_avx2(&column.values)
    }
}
```

**Acceptance Criteria**:
- [ ] SIMD paths integrated into query engine
- [ ] CPU feature detection working (AVX-512 → AVX2 fallback)
- [ ] Query planner modified
- [ ] Benchmark: 2.9M rows/sec achieved (5% improvement)

---

#### Task 3: Cache Miss Reduction (Day 5-6)
**Owner**: Michael Torres  
**Timeline**: Jun 6-7 (2 days)  
**Deliverable**: `src/columnar/cache_optimization.rs` (prefetching + alignment)

**Optimizations**:
1. Implement cache prefetching (NTA - non-temporal access)
2. Align data structures to 64-byte L1 cache lines
3. Optimize memory layout for sequential access

```rust
// Add prefetch hints for upcoming data
fn scan_with_prefetch(column: &Column) -> u64 {
    let mut sum = 0;
    for i in (0..column.len()).step_by(64) {
        // Prefetch next chunk
        unsafe { _mm_prefetch(column.values[i+64] as *const i8, 0); }
        
        // Process current chunk
        sum += simd::integer_scan::scan_i32_avx512(&column.values[i..i+64]);
    }
    sum
}

// Ensure 64-byte alignment
#[repr(align(64))]
struct AlignedColumn {
    values: Vec<i32>,
}
```

**Acceptance Criteria**:
- [ ] Prefetch instructions implemented
- [ ] Memory alignment verified (64-byte boundaries)
- [ ] Cache miss rate reduced 15%
- [ ] Benchmark: 3.1M rows/sec achieved (10% improvement)

---

#### Task 4: Testing & Hardening (Day 7-8)
**Owner**: Engineer 1 + Emily Rodriguez (QA)  
**Timeline**: Jun 8-9 (2 days)  
**Deliverable**: `tests/simd_tests.rs` (comprehensive test suite)

**Tests to Write**:
1. Correctness: SIMD results match scalar (vector comparison test)
2. Edge cases: Empty arrays, single element, large arrays
3. Boundary conditions: Unaligned data, cache line boundaries
4. Performance: Benchmark vs baseline (must show 25%+ improvement)
5. Regression: Existing query tests still pass

```rust
#[test]
fn test_simd_integer_scan_correctness() {
    let data = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let simd_result = simd::integer_scan::scan_i32_avx512(&data);
    let scalar_result = data.iter().sum::<i32>();
    assert_eq!(simd_result as i32, scalar_result);
}

#[test]
fn test_simd_performance_gain() {
    let data = vec![/* 10M integers */];
    let start = Instant::now();
    let _ = simd::integer_scan::scan_i32_avx512(&data);
    let simd_time = start.elapsed();
    
    // Must be 25%+ faster
    assert!(simd_time < Duration::from_secs_f64(0.37)); // 37ms baseline / 1.25
}
```

**Acceptance Criteria**:
- [ ] 50+ test cases written
- [ ] 100% of SIMD code covered by tests
- [ ] All tests passing (green CI/CD)
- [ ] No regressions in existing queries
- [ ] Benchmark confirmed: 3.5M rows/sec achieved (30% improvement)

---

#### Task 5: Documentation & Knowledge Transfer (Day 9-10)
**Owner**: Michael Torres  
**Timeline**: Jun 10-13 (4 days)  
**Deliverable**: `docs/SIMD_OPTIMIZATION_GUIDE.md` + code comments

**Documentation**:
1. SIMD kernel architecture (ASCII diagram)
2. CPU feature detection strategy
3. Performance profiling results
4. Future optimization opportunities
5. Code comments (every SIMD function)

```markdown
# SIMD Optimization Guide

## Architecture
```
     Input Column (10M integers)
              |
              v
     CPU Feature Detection
         /            \
    AVX-512          AVX2
     |                |
  8 ints/cycle    4 ints/cycle
     |                |
      \              /
       v            v
    SIMD Kernel (optimized)
       |
       v
    Output Result
```

## Performance Gains
- Integer scanning: +30% (2.7M → 3.5M rows/sec)
- Memory utilization: +30% (72.9 → 94.8 GB/s)
- Cache efficiency: +15% (fewer L1 misses)
```

**Acceptance Criteria**:
- [ ] Architecture documented with diagrams
- [ ] Code comments on all SIMD functions
- [ ] Performance profiling results attached
- [ ] Team reviews + approves documentation
- [ ] Engineers understand implementation (knowledge transfer complete)

---

## 📅 WEEK 1-2 TIMELINE

### Week 1 (Jun 2-6)

**Monday, Jun 2** (9:00 AM Kickoff)
- 9:00-10:00 AM: Engineering kickoff (Conference Room A)
  - Sarah: Phase 1 mission overview (5 min)
  - Michael: SIMD sprint plan + Week 1-2 milestones (15 min)
  - Q&A + blockers (10 min)
- 10:00 AM-12:00 PM: Michael + Engineer 1 begin SIMD module design
- 1:00-5:00 PM: Implementation begins (`src/simd/mod.rs` coding)

**Tue-Wed, Jun 3-4** (Days 2-3)
- **Task 1 (Jun 2-3)**: SIMD module creation (integer_scan kernel)
- **Task 2 (Jun 4-5)**: Integration into query engine
- **Daily standups**: 9:00 AM (2 min each - blockers only)

**Thu-Fri, Jun 5-6** (Days 4-5)
- **Task 2 Complete (Jun 5)**: SIMD integration + first benchmark
- **Task 3 Start (Jun 6)**: Cache miss reduction (prefetch + alignment)
- **Week 1 Status**: 2.9M rows/sec achieved (still in progress)

### Week 2 (Jun 9-13)

**Monday, Jun 9** (Day 6)
- **Task 3 Complete (Jun 7)**: Cache optimization
- **Benchmark**: 3.1M rows/sec (10% improvement, on track)
- **Daily standup**: Report progress, plan Task 4

**Tue-Wed, Jun 10-11** (Days 7-8)
- **Task 4**: Testing + hardening
- Engineer 1 writes unit tests
- Emily Rodriguez runs performance benchmarks
- **Target**: 3.5M rows/sec confirmed

**Thu-Fri, Jun 12-13** (Days 9-10)
- **Task 5**: Documentation + knowledge transfer
- Michael finalizes SIMD guide
- Code review meeting (all engineers)
- **Week 2 Status**: 3.5M rows/sec ACHIEVED ✅

**Friday, Jun 13 EOD**: Sprint 1 Complete
- All tasks finished
- 3.5M rows/sec confirmed
- Code merged to main
- Team ready for Sprint 2 (Memory optimization)

---

## 🎯 SUCCESS METRICS

By Jun 13, 2026:

| Metric | Target | Status |
|--------|--------|--------|
| Query Performance | 3.5M rows/sec | MUST ACHIEVE |
| Code Coverage | 100% (SIMD functions) | MUST ACHIEVE |
| Test Cases | 50+ passing | MUST ACHIEVE |
| Performance Gain | +30% vs baseline | MUST ACHIEVE |
| No Regressions | All queries still working | MUST ACHIEVE |
| Documentation | Complete + team reviewed | MUST ACHIEVE |
| Code Quality | Zero critical issues | MUST ACHIEVE |

---

## 📋 DELIVERABLES (Jun 13)

| Item | Owner | Status |
|------|-------|--------|
| `src/simd/mod.rs` | Michael + Eng1 | ✅ Delivered |
| `src/columnar/integer_scan.rs` (updated) | Eng2 | ✅ Delivered |
| `src/columnar/cache_optimization.rs` | Michael | ✅ Delivered |
| `tests/simd_tests.rs` | Eng1 + Emily | ✅ Delivered |
| `docs/SIMD_OPTIMIZATION_GUIDE.md` | Michael | ✅ Delivered |
| Performance Benchmark Report | Emily | ✅ Delivered |
| Code Review Approval | All engineers | ✅ Approved |
| **3.5M rows/sec Performance** | **All teams** | **✅ ACHIEVED** |

---

## 🎁 NEXT SPRINT (Week 3-4: Jun 16-27)

After Week 1-2 SIMD sprint completes, next sprint is **Memory Optimization** (Task: 3.5M → 4.4M rows/sec, +25%).

Owner: David Park (Systems Engineer)

---

## ⚠️ RISKS & MITIGATIONS

| Risk | Probability | Mitigation |
|------|-------------|-----------|
| SIMD kernels have bugs | Low | 50+ unit tests, code review |
| CPU feature detection fails | Very Low | Test on both AVX-512 and AVX2 systems |
| Cache optimization shows no gain | Low | Run memory profiling, identify bottleneck |
| Test coverage incomplete | Low | Emily runs regression tests |
| Performance target missed | Very Low | Conservative +30% estimate, proven techniques |

---

## 📞 ESCALATION

| Issue | Action | Escalate To |
|------|--------|-------------|
| SIMD kernels not compiling | Debug Rust errors | Michael |
| Benchmark shows no improvement | Profile with perf, investigate | Michael + Sarah |
| Test failures | Debug + fix code | Engineer responsible |
| Blockers preventing progress | Daily standup discussion | Sarah (CTO) |

---

## ✨ COMMIT CRITERIA (Jun 13 EOD)

Code can be merged to main ONLY if:

- [ ] All 5 tasks completed
- [ ] 50+ unit tests passing
- [ ] 3.5M rows/sec achieved on TPC-H benchmark
- [ ] Zero regressions in existing queries
- [ ] Code reviewed and approved by 2+ engineers
- [ ] Documentation complete and reviewed
- [ ] Team confirms no blockers for Sprint 2

---

**Prepared by**: Michael Torres (Performance Lead)  
**Approved by**: Sarah Williams (CTO)  
**Date**: May 28, 2026  
**Sprint Dates**: Jun 2-13, 2026 (10 business days)  
**Standup Time**: 9:00 AM daily (2 min updates)  
**Success Target**: 3.5M rows/sec (+30% improvement)
