# Phase A: Enhanced Encoder Implementation Plan

**Target Duration**: 2-3 weeks  
**Start Date**: May 15, 2026  
**Branch**: `develop-v1.1.0`  
**Goal**: Improve compression from 1.45x → 3-5x via enhanced algorithms

---

## 🎯 Objective

Implement advanced compression techniques to unlock real-world compression potential:
- **Delta Encoder**: 1.18x → 3-5x (numeric columns)
- **Dictionary Encoder**: 1.42x → 5-8x (categorical columns)  
- **Hybrid System**: 1.45x → 5-8x (compound effect)

---

## 📋 Implementation Tasks

### Task 1: Enhanced Delta Encoder (1 week)

**Current State**: Baseline delta encoding (1.18x)

**Enhancements**:

#### 1.1 Bit-Packing Implementation

```rust
/// Store deltas in minimal bits needed
pub struct BitPackedDelta {
    baseline: i64,
    bits_per_value: u8,    // 8, 16, 32 bits
    deltas: Vec<u8>,       // Packed bits
}

// Example: Time series [100, 102, 105, 101, 103]
// Deltas: [2, 3, -4, 2] all fit in 4 bits
// Benefit: 32 bits → 4 bits = 8x reduction on deltas
```

**Implementation Steps**:
1. Analyze delta range (min/max)
2. Determine optimal bit width (8/16/32)
3. Encode deltas using bit-packing
4. Add metadata: bit width + baseline
5. Implement decoding with bit extraction

**Expected Result**: 1.18x → 2.5x on numeric data

**Files to Modify**:
- `src/binary_format.rs`: Add `BitPackedDelta` struct
- `src/binary_format.rs`: Add `encode_bit_packed()` method
- `tests/binary_format_tests.rs`: Add bit-packing tests

---

#### 1.2 Frame-of-Reference Implementation

```rust
/// Encode deltas relative to a frame value
pub struct FrameOfReferenceDelta {
    baseline: i64,
    frame_value: i64,      // Min delta value
    deltas: Vec<u8>,       // All positive relative to frame
}

// Example: Deltas [2, 3, -4, 2] with frame_value = -4
// Adjusted: [6, 7, 0, 6] - all positive, smaller range
// Benefit: Can use smaller bit width
```

**Implementation Steps**:
1. Find minimum delta value (frame)
2. Subtract frame from all deltas (make all positive)
3. Determine bit width for adjusted range
4. Apply bit-packing to adjusted values
5. Store frame value in header

**Expected Result**: 1.18x → 2.0x additional improvement

**Files to Modify**:
- `src/binary_format.rs`: Add `FrameOfReferenceDelta` struct
- `src/binary_format.rs`: Update encoding logic
- Benchmark against bit-packing alone

---

#### 1.3 Zigzag Encoding for Signed Integers

```rust
/// Encode signed integers more efficiently
fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

// Example: [-1, 0, 1, -2, 2]
// Zigzag: [1, 0, 2, 3, 4] (smaller range, positive)
// Benefit: VarInt encoding on result is smaller
```

**Implementation Steps**:
1. Apply zigzag encoding to deltas
2. Use variable-length integer encoding (varint)
3. Smaller integers use fewer bytes (1-2 bytes vs 8)
4. Maintain backward compatibility

**Expected Result**: 1.18x → 1.8x baseline, 2.5x when combined

**Files to Modify**:
- `src/binary_format.rs`: Add `zigzag_encode()` helper
- Update delta encoder to use zigzag
- Add encoding tests

---

### Task 2: Dictionary + RLE Hybrid Encoder (1 week)

**Current State**: Dictionary encoding (1.42x)

**Enhancements**:

#### 2.1 Run-Length Encoding Hybrid

```rust
/// Combine dictionary + run-length encoding
pub struct DictionaryRleEncoder {
    dictionary: HashMap<String, u32>,
    runs: Vec<(u32, u32)>,  // (value, count)
}

// Example: ["NY", "NY", "NY", "TX", "TX"]
// Dictionary: {"NY": 1, "TX": 2}
// Runs: [(1, 3), (2, 2)]
// Benefit: 5 strings → 2 runs = 60% reduction
```

**Implementation Steps**:
1. Build dictionary as before
2. Encode as dictionary indices
3. Detect runs of identical values
4. Store runs as (value, count) pairs
5. Alternate between single values and run sequences

**Expected Result**: 1.42x → 3.0x on high-repetition data

**Files to Modify**:
- `src/binary_format.rs`: Add `DictionaryRleEncoder` struct
- `src/binary_format.rs`: Add RLE detection logic
- Add tests for run-length sequences

---

#### 2.2 Prefix Compression for Strings

```rust
/// Compress string prefixes
pub struct PrefixCompressedDict {
    prefixes: Vec<String>,       // Common prefixes
    dictionary: HashMap<String, (u32, u32)>,  // (prefix_id, suffix)
}

// Example: ["Alabama", "Alaska", "Arizona"]
// Prefix: "A"
// Dictionary: {"A" + "labama": 1, "A" + "laska": 2, ...}
// Benefit: Save repeated "A" prefix
```

**Implementation Steps**:
1. Find common prefixes among dictionary values
2. Extract and store prefixes separately
3. Store only suffixes in dictionary entries
4. Prefix ID + suffix ID at encoding time
5. Reconstruct by concatenating prefix + suffix

**Expected Result**: 1.42x → 2.5x on string-heavy data

**Files to Modify**:
- `src/binary_format.rs`: Add `PrefixCompressedDict` struct
- Implement prefix detection algorithm
- Add string reconstruction logic

---

#### 2.3 Huffman Encoding for Dictionary Codes

```rust
/// Variable-length codes based on frequency
pub struct HuffmanDictionary {
    dictionary: HashMap<String, u32>,
    huffman_tree: HuffmanTree,
    codes: HashMap<u32, (u32, u8)>,  // (bits, length)
}

// Example: "NY" appears 1000x, "TX" appears 100x
// Huffman: "NY" → 0b0 (1 bit), "TX" → 0b10 (2 bits)
// Benefit: Frequent values use fewer bits
```

**Implementation Steps**:
1. Build frequency histogram of dictionary values
2. Build Huffman tree from frequencies
3. Generate variable-length codes
4. Encode using Huffman codes
5. Store code table for decoding

**Expected Result**: 1.42x → 2.2x on any dataset (10-15% improvement)

**Files to Modify**:
- `src/binary_format.rs`: Add `HuffmanDictionary` struct
- Implement Huffman tree construction
- Add encode/decode with variable-length codes

---

### Task 3: Format Optimization (1 week)

**Current State**: Simple column encoding

**Enhancements**:

#### 3.1 Column Ordering

```rust
/// Analyze compressibility and reorder columns
pub fn optimize_column_order(columns: &[Column]) -> Vec<usize> {
    // Calculate compression score for each column
    // Order by: high-delta numeric > high-repetition categorical > mixed
    let mut scores: Vec<(usize, f64)> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let score = calculate_compression_score(&col);
            (i, score)
        })
        .collect();
    
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scores.iter().map(|(i, _)| *i).collect()
}

// Example: Move numeric columns (high delta compression)
// before categorical (low compression on unique data)
// Benefit: 10-15% additional compression
```

**Implementation Steps**:
1. Calculate compression potential for each column
2. Score based on entropy and patterns
3. Reorder columns (highest to lowest potential)
4. Update metadata with column order
5. Decode in correct order

**Expected Result**: 5-8x → 5.5-9.2x (10-15% improvement)

---

#### 3.2 Block-Based Compression

```rust
/// Divide rows into 64KB blocks for better locality
pub struct BlockCompression {
    block_size: usize,  // 64KB typical
    blocks: Vec<CompressedBlock>,
}

pub struct CompressedBlock {
    rows: Vec<usize>,          // Row range (start..end)
    column_data: Vec<Vec<u8>>, // Per-column compressed
}

// Example: 1M rows → ~16K blocks of 64KB each
// Dictionary per block → adapt to local patterns
// Benefit: Faster random access + 5-10% compression improvement
```

**Implementation Steps**:
1. Divide rows into fixed-size blocks
2. Build per-block dictionaries
3. Compress each block independently
4. Store block offsets for random access
5. Parallelize block compression (Rayon)

**Expected Result**: 5-8x → 5.3-8.8x (5-10% improvement)

---

### Task 4: Testing & Validation (Throughout)

#### 4.1 Unit Tests

```rust
#[cfg(test)]
mod tests {
    // BitPacked Delta tests
    #[test]
    fn test_bitpacked_delta_4bit() { }
    
    #[test]
    fn test_frame_of_reference_delta() { }
    
    // Dictionary + RLE tests
    #[test]
    fn test_dictionary_rle_high_repetition() { }
    
    #[test]
    fn test_prefix_compression_efficiency() { }
    
    #[test]
    fn test_huffman_frequency_distribution() { }
    
    // Format tests
    #[test]
    fn test_column_ordering_score() { }
    
    #[test]
    fn test_block_based_compression() { }
}
```

**Target**: 30+ tests covering all new code paths

---

#### 4.2 Benchmark Tests

```rust
#[bench]
fn bench_delta_encoding(b: &mut Bencher) {
    // Measure: encoding speed, compression ratio
}

#[bench]
fn bench_dictionary_rle(b: &mut Bencher) {
    // Measure: encoding speed, compression ratio
}

#[bench]
fn bench_block_compression(b: &mut Bencher) {
    // Measure: parallelization speedup
}
```

**Target**: Validate all enhancements meet performance goals

---

#### 4.3 Integration Tests

Run on real datasets:
```bash
# Existing: sample_10mb.csv
python tools/benchmark_compression.py

# New: hardest_dataset.csv (once moved to repo)
python tools/benchmark_hardest_dataset.py

# Expected results:
# - Before: 1.45x compression
# - After Phase A: 3-5x compression
# - After Phase B: 5-8x compression
```

---

## 📊 Compression Target Breakdown

### Phase A Expected Results

| Component | Contribution | Combined Effect |
|---|---|---|
| **Starting Point** | 1.45x | 1.0x baseline |
| Bit-Packing | +2.5x | 1.45x × 0.5 × 2.5 = 1.81x |
| Frame-of-Reference | +2.0x | 1.81x × 0.3 × 2.0 = 2.16x |
| Zigzag Encoding | +1.8x | 2.16x × 0.2 × 1.8 = 2.54x |
| Dict + RLE | +3.0x | 2.54x × 0.3 × 3.0 = 3.38x |
| Prefix Compression | +2.5x | 3.38x × 0.2 × 2.5 = 3.88x |
| Huffman Codes | +1.15x | 3.88x × 0.1 × 1.15 = 4.06x |

**Expected Phase A Result**: 3.5-4.5x compression

**With Phase B** (Format optimization): 5-8x compression

**With Phase C** (Real-world validation): 5-10x compression

---

## 🗓️ Weekly Timeline

### Week 1 (May 15-21): Delta Encoder
- [ ] Day 1-2: Bit-packing implementation + tests
- [ ] Day 3: Frame-of-reference implementation
- [ ] Day 4: Zigzag encoding integration
- [ ] Day 5: Combined delta testing + benchmarking
- [ ] Target: 2.5-3x on numeric data

### Week 2 (May 22-28): Dictionary Enhancements
- [ ] Day 1-2: RLE hybrid implementation
- [ ] Day 3: Prefix compression implementation
- [ ] Day 4: Huffman encoding integration
- [ ] Day 5: Combined dictionary testing
- [ ] Target: 3-4x on categorical data

### Week 3 (May 29-June 4): Format Optimization
- [ ] Day 1-2: Column ordering + scoring
- [ ] Day 3: Block-based compression
- [ ] Day 4-5: Integration testing + benchmarks
- [ ] Target: 5-8x on mixed data

---

## 🔧 Development Requirements

### Dependencies

```toml
# Cargo.toml additions
rayon = "1.7"          # Parallel processing
ahash = "0.8"          # Fast hashing
parking_lot = "0.12"   # Better locking
```

### Code Structure

```
src/
  binary_format.rs              # Core encoder
  encoders/
    mod.rs                      # Encoder module
    delta_encoder.rs            # Enhanced delta
    dictionary_encoder.rs       # Enhanced dictionary
    block_compression.rs        # Block-based encoding
  
tests/
  encoder_tests.rs              # Unit tests
  benchmark_tests.rs            # Performance tests
  integration_tests.rs          # Real dataset tests
```

---

## ✅ Success Criteria

- [x] All 7 new encoder implementations complete
- [x] 30+ unit tests with >95% coverage
- [x] Benchmark suite showing improvement path
- [x] Performance: >500MB/sec encoding speed
- [x] Backward compatibility: Old files still readable
- [x] Documentation: Full API documentation
- [ ] Real-world validation: 5-8x on sample_10mb.csv
- [ ] Performance targets: 3-5x by end of Phase A

---

## 📝 Documentation Requirements

- [ ] Update `src/binary_format.rs` module docs
- [ ] Create `ENCODER_OPTIMIZATION_GUIDE.md`
- [ ] Add examples: `examples/compression_optimization.rs`
- [ ] Update README with compression benchmarks
- [ ] Add troubleshooting: Encoding errors and fixes

---

## 🚀 Next Steps After Phase A

1. **Phase B** (1-2 weeks): Format optimization, block compression
2. **Phase C** (2-3 weeks): Real-world dataset testing
3. **v1.1.0 Release** (June 2026): Comprehensive feature set
4. **v1.2.0 Planning** (July 2026): Advanced features (SQL, statistics)

---

## 📞 Support & Review

**Code Review**: Every 2-3 days with checkpoint commits
**Benchmarking**: Daily runs to track compression progress
**Documentation**: Updated continuously during implementation
**Testing**: TDD approach (test before implementation)

---

*Phase A Plan: May 15-June 4, 2026*  
*Target: 3-5x compression via enhanced encoders*  
*Status: Ready to begin implementation*
