# ALGORITHM RESEARCH: Beat Parquet's 46.2%

**Goal:** Understand how Parquet achieves better compression, then implement improvements to Kore

---

## 🔍 **INVESTIGATION 1: Parquet Compression Pipeline**

### Known Facts
```
Test Data: 10MB mixed analytics
Parquet Ratio: 46.2%
Kore Ratio: 56.4%
Gap: 10.2% (need to close this)
```

### Research Questions

**Q1: Which codec does Parquet use for each column type?**
```
Numeric columns:
  ├─ Integer: [Delta-of-Delta encoding + RLE + Zstd?]
  ├─ Float: [Bit packing + RLE + Zstd?]
  └─ Unknown compression ratio contribution

Categorical columns:
  ├─ Dictionary: [Adaptive dictionary + Zstd?]
  └─ High cardinality: [Direct Zstd without dictionary?]

String columns:
  ├─ Small strings: [Dictionary + RLE?]
  ├─ Large text: [Direct Zstd?]
  └─ Unknown strategy
```

**Q2: Zstd Parameters**
```
Kore: Using standard Zstd (level 3)
Parquet: Likely using:
  ├─ Higher compression level (6-19)?
  ├─ Larger dictionary window?
  ├─ Custom training data?
  └─ Multiple pass optimization?
```

**Q3: Format-Level Optimizations**
```
Parquet specific:
  ├─ Stripe-level metadata (reduces repeated info?)
  ├─ Run-length encoding at page level
  ├─ Null suppression
  ├─ Bit-packing for integers
  └─ Delta encoding for sorted columns
```

### Action Items

- [ ] Read Parquet spec: https://parquet.apache.org/docs/file-format/
- [ ] Extract sample Parquet files
- [ ] Measure compression ratio per-column
- [ ] Reverse engineer codec selection logic
- [ ] Document findings in ALGORITHM_ANALYSIS.md

---

## 🔍 **INVESTIGATION 2: Enhanced Dictionary Encoding**

### Current Kore Dictionary Implementation
```rust
// Simple dictionary:
[unique_values] + [indices into dict]
Overhead: Dictionary size + type info
```

### Potential Improvements

**1. Multi-Level Dictionary (Hierarchical)**
```
Level 1: Top 256 most common values → 1 byte index
Level 2: Next 65K values → 2 byte index  
Level 3: Remaining rare values → full encoding

Result: Smaller indices for common values
Expected gain: 2-3% compression improvement
```

**2. Adaptive Code Lengths**
```
If 80% of values fit in 256 dictionary:
  Use 1-byte indices (not full dictionary)
If 99% fit in 64K:
  Use 2-byte indices

Result: Smaller index size
Expected gain: 1-2% compression improvement
```

**3. Context-Aware Dictionary**
```
Example: "status" column
  ├─ Active: 45%
  ├─ Inactive: 35%
  ├─ Pending: 19%
  └─ Rare: 1%

Split dictionary into "hot" and "cold"
Expected gain: 2-3% improvement
```

### Implementation Plan
- Week 1: Implement multi-level dictionary
- Test on benchmark data
- Measure compression ratio improvement
- If successful (>2%), integrate into Kore

---

## 🔍 **INVESTIGATION 3: Variable Zstd Parameters**

### Current Approach
```rust
// Kore: One-size-fits-all
zstd::Compressor::new(3) // Level 3
```

### Enhanced Approach
```rust
match column_type {
    DataType::Numeric => {
        // Numeric data compresses well
        Compressor::new(9)  // Level 9
    }
    DataType::String => {
        // Strings need more passes
        Compressor::new(6)  // Level 6
    }
    DataType::Categorical => {
        // Should use Dictionary instead
        // Skip Zstd
    }
}
```

### Alternative: Custom Zstd Training
```
Create dictionary from first 10K rows
Zstd learns patterns from data
Better compression on remaining rows
```

### Expected Improvement
- 1-2% better compression (column-specific tuning)
- Minimal speed impact (faster on some, slower on others)

---

## 🔍 **INVESTIGATION 4: Delta Encoding**

### How It Works
```
Original: [100, 103, 105, 108, 110]
Delta:    [100, 3, 2, 3, 2]     ← much smaller numbers!
RLE:      [100, 3×1, 2×1, 3×1, 2×1]
Zstd:     Compresses tiny numbers efficiently
```

### Where It Helps
- Time-series data (timestamps)
- Sequential IDs
- Monotonic numeric columns
- Measurements that change slowly

### Expected Improvement
- 3-5% on time-series data
- 0% on random data (skip)

### Implementation
```rust
detect_if_monotonic_or_near_monotonic(column)
if is_monotonic {
    apply_delta_encoding()
    compress_with_zstd()
} else {
    skip_delta, use_zstd_directly()
}
```

---

## 📊 **COMPETITIVE COMPARISON**

| Format | Strategy | Ratio | Notes |
|--------|----------|-------|-------|
| **Parquet** | Multi-codec + advanced encoding | 46.2% | Industry standard |
| **ORC** | Stripe + block compression | 58.3% | Good balance |
| **Kore v1.0** | Per-column codec selection | 56.4% | Current (good) |
| **Kore v2.0 (target)** | + Delta + Multi-level dict | 40%? | Best possible |

---

## 🎯 **Phase 1 Research Output (by Jun 6)**

### Deliverable 1: ALGORITHM_ANALYSIS.md
```
Summary of:
- Parquet compression techniques
- Codec selection logic
- Zstd parameter impact
- Delta encoding effectiveness
- Dictionary optimization strategies
```

### Deliverable 2: ALGORITHM_PROTOTYPES.md
```
Code + results for:
- Enhanced dictionary encoder
- Variable Zstd parameters
- Delta encoding test
- Performance impact measurements
```

### Deliverable 3: IMPROVEMENT_ESTIMATES.md
```
Projected improvements per technique:
- Multi-level dict: +2-3%
- Context-aware dict: +2-3%
- Variable Zstd: +1-2%
- Delta encoding: +3-5% (time-series only)

Total potential: 8-13% improvement → 43-48% ratio (beating Parquet!)
```

---

## 🚀 **If Breakthrough Found (Jun 16 Decision)**

**Fast-track implementation:**
1. Integrate best technique(s) into src/compression/mod.rs
2. Run full test suite (600+ tests must pass)
3. Benchmark on real data
4. If successful: Ready for Phase 2 integration
5. If fails: Revert, keep 56.4%, focus on projects

---

## ⚠️ **Risks & Mitigations**

| Risk | Mitigation |
|------|-----------|
| New algorithms break tests | Maintain fallback to 56.4% always |
| Complex codecs slower | Profile, optimize hotpaths |
| Diminishing returns | Cap research at 3 weeks |
| Parquet already optimal | Still gain from being 2nd place with ecosystem |

---

## 📞 **Research Checkpoints**

- **May 29:** Parquet analysis complete
- **Jun 3:** First prototype coded + tested
- **Jun 6:** All 4 prototypes ready
- **Jun 16:** Decision on which (if any) to ship

---

**Status:** Research scaffolding ready, start immediately!
