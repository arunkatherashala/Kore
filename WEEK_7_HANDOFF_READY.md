# WEEK 7 HANDOFF: HYBRID COMPRESSION SELECTION - Ready to Start

**From:** Week 6 LZSS Completion  
**To:** Week 7 Hybrid Codec Selection  
**Status:** All prerequisites complete - ready to implement  
**Date:** June 28-July 5, 2026

---

## 🎯 Week 7 Objective

Design and implement automatic codec selection algorithm that:
- Analyzes column data characteristics (cardinality, distribution, patterns)
- Selects optimal decompression codec per column
- Achieves 50% compression ratio (hybrid strategy)
- Maintains 800+ MB/s read speed across all codecs

---

## ✅ Prerequisites Complete

### All 4 Decompression Codecs Ready
```
✅ RLE (20/20 tests)           - 1000+ MB/s
✅ Dictionary (20/20 tests)    - 500+ MB/s
✅ FOR (10/10 tests)           - 2000+ MB/s
✅ LZSS (16/16 tests)          - 800+ MB/s
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
✅ TOTAL: 60/60 tests (100%)   - Avg 1000+ MB/s
```

### Code Structure Ready
- `src/decompression.rs` - All 4 codecs + registry
- `src/kore_reader.rs` - v2.0 format reader
- `CodecRegistry::decompress()` - Dispatcher

### Test Infrastructure Ready
```bash
cargo test decompression:: --lib  # All 60 tests pass
cargo build --lib                 # Clean build
```

---

## 🏗️ Week 7 Implementation Plan

### Phase 1: Cardinality Analysis (2 days)

**Goal:** Detect column characteristics

```rust
pub struct ColumnProfile {
    total_values: u64,
    unique_values: usize,
    cardinality_ratio: f32,  // unique / total
    value_distribution: Distribution,
    numeric_stats: Option<NumericStats>,
    string_stats: Option<StringStats>,
}

enum Distribution {
    LowCardinality,      // <10% unique
    HighCardinality,     // >90% unique
    Categorical,         // 10-100 values
    Numeric,            // Numeric with range
    Text,               // Text/mixed
}
```

**Implementation:**
- Single-pass scan of column data
- Track unique values (use HashSet)
- Compute distribution ratio
- Extract numeric/string stats

**Test Cases:**
- All-identical values (RLE ideal)
- All-unique values (LZSS ideal)
- Limited dictionary (10-100 values)
- Numeric range (FOR ideal)
- Mixed/text data (LZSS ideal)

---

### Phase 2: Codec Selection Algorithm (2 days)

**Goal:** Map characteristics → codec

```rust
pub fn select_codec(profile: &ColumnProfile) -> CodecId {
    match profile {
        // RLE: >90% repeated runs
        if compression_ratio > 0.9 && max_run_length > 100 => CodecId::RLE,
        
        // FOR: Numeric with tight range
        if is_numeric && range < 1_000_000_000 => CodecId::FOR,
        
        // Dictionary: Low cardinality
        if cardinality < 100 => CodecId::Dictionary,
        
        // LZSS: General-purpose fallback
        _ => CodecId::LZSS,
    }
}
```

**Decision Tree:**
1. Check if RLE viable (high run length)
2. Check if numeric FOR viable (tight range)
3. Check if Dictionary viable (low cardinality)
4. Fall back to LZSS

**Test Cases:**
- Pure RLE data (select RLE)
- Numeric ranges (select FOR)
- 50 unique values (select Dictionary)
- JSON text (select LZSS)
- Mixed patterns (auto-select)

---

### Phase 3: Compression Ratio Optimization (2 days)

**Goal:** Achieve 50% compression

```rust
pub struct CompressionStats {
    codec: CodecId,
    original_size: usize,
    compressed_size: usize,
    ratio: f32,  // compressed / original
    speed_mb_per_sec: f32,
}

pub fn optimize_compression(profile: &ColumnProfile) -> CodecId {
    // Try all viable codecs, measure ratio + speed
    let candidates = [CodecId::RLE, CodecId::Dictionary, CodecId::FOR, CodecId::LZSS];
    
    candidates
        .iter()
        .map(|codec| {
            let sample = profile.sample_data();
            let compressed = compress_sample(codec, sample);
            CompressionStats {
                codec,
                ratio: compressed.len() / sample.len(),
                speed: measure_speed(*codec),
            }
        })
        .max_by_key(|stats| score_codec(&stats))  // Ratio + speed trade-off
        .codec
}
```

**Hybrid Strategy:**
- Per-column codec selection
- Measure compression ratio for each codec
- Select best ratio + speed combination
- Target: 50% average ratio

**Compression Targets by Codec:**
- RLE: 2-10x (best for runs)
- Dictionary: 2-5x (good for cardinality)
- FOR: 4-8x (best for numeric)
- LZSS: 1.5-3x (general purpose)

**Test Cases:**
- Measure compression on each codec type
- Verify 50% ratio achievable
- Validate speed remains >800 MB/s

---

### Phase 4: Integration & Validation (1 day)

**Goal:** Full round-trip compression/decompression

```rust
pub fn roundtrip_test(original: &[u8], codec: CodecId) -> Result<()> {
    let profile = analyze_column(original);
    let selected = select_codec(&profile);
    
    // Compress with selected codec
    let compressed = compress(selected, original)?;
    
    // Decompress
    let decompressed = decompress(selected, &compressed)?;
    
    // Verify byte-for-byte match
    assert_eq!(original, decompressed);
    
    Ok(())
}
```

**Integration Points:**
- KoreWriter: Use codec selection for write
- KoreReader: Use CodecRegistry for read (already done)
- Metadata: Store selected codec per column

**Test Coverage:**
- Round-trip for each codec
- Mixed codec file (different codecs per column)
- Real-world data patterns
- Large file validation

---

## 📊 Success Criteria

| Goal | Target | Metric |
|------|--------|--------|
| Cardinality analysis | Accurate | >95% correct classification |
| Codec selection | Optimal | Best or near-best ratio |
| Compression ratio | 50% | Avg across all data |
| Read speed | Maintained | >800 MB/s |
| Test coverage | Complete | 100% pass rate |
| Integration | Seamless | Works with KoreReader |

---

## 📝 Deliverables for Week 7

### Code
- [ ] `src/codec_selector.rs` (200+ lines)
  - `ColumnProfile` struct
  - `analyze_column()` function
  - `select_codec()` algorithm
  - `CompressionStats` tracking
- [ ] `src/compression_optimizer.rs` (150+ lines)
  - Hybrid ratio optimization
  - Per-codec compression testing
  - Speed measurement
- [ ] Integration with `src/decompression.rs`

### Tests
- [ ] 15+ test cases for codec selection
- [ ] Round-trip validation tests (5+)
- [ ] Compression ratio verification (5+)
- [ ] Performance benchmarks (3+)
- [ ] Total: 28+ new tests

### Documentation
- [ ] `WEEK_7_HYBRID_SELECTION.md` - Implementation details
- [ ] Codec selection algorithm spec
- [ ] Compression ratio targets + achievement
- [ ] Performance profiles (before/after)

---

## 🔄 Build & Test Commands

```bash
# Build with all features
cargo build --lib

# Test Week 7 codec selection
cargo test codec_selector:: --lib

# Test round-trip compression
cargo test roundtrip:: --lib

# Benchmark compression
cargo bench --lib

# Full integration
cargo test decompression:: --lib
```

---

## 📚 Reference Materials

### From Week 6
- All 4 decompressors fully tested (60/60 tests)
- Codec registry pattern established
- KoreReader integration ready
- Performance profiles documented:
  - RLE: 1000+ MB/s
  - Dictionary: 500+ MB/s
  - FOR: 2000+ MB/s
  - LZSS: 800+ MB/s

### Key Code Locations
- Decompression: `src/decompression.rs` (lines 1-1100)
- Registry: `CodecRegistry::decompress()` (lines 900-920)
- Reader: `src/kore_reader.rs` (lines 1-350)

### Test Data Available
- RLE patterns: Runs of values (1-10K repeats)
- Dictionary: 10-100 unique values
- FOR: Numeric ranges (1-1B)
- LZSS: Text, JSON, mixed binary

---

## ⏭️ Next Steps (START IMMEDIATELY)

1. **Day 1-2: Analysis**
   - Create `ColumnProfile` struct
   - Implement `analyze_column()`
   - Write analysis tests

2. **Day 2-3: Selection**
   - Build decision tree
   - Implement `select_codec()`
   - Test on synthetic data

3. **Day 3-4: Optimization**
   - Measure compression ratios
   - Implement hybrid selection
   - Target 50% ratio

4. **Day 4-5: Integration**
   - Integrate with KoreWriter
   - Add metadata storage
   - Round-trip validation

5. **Day 5: Documentation**
   - Write WEEK_7_HYBRID_SELECTION.md
   - Create performance report
   - Prepare for Week 8

---

## 🚀 Ready to Ship!

All prerequisites met. Code structure ready. Tests passing.

**Estimated Completion:** July 5, 2026 (on schedule)  
**Expected Compression Ratio:** 50% average  
**Expected Read Speed:** 800-2000 MB/s (codec-dependent)

**PHASE 2 DECOMPRESSION: ✅ COMPLETE**  
**PHASE 3 HYBRID SELECTION: ⏭️ READY TO START**

Let's go! 🎯
