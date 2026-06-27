# PROJECT 1: COMPRESSION PHASE 1 - ARCHITECTURE

**Goal:** 86%+ compression, faster than Brotli, beat Parquet + Snappy  
**Timeline:** May 22-31 (10 days)  
**Target:** 1.15 MB from 1.28 MB test dataset

---

## 🎯 COMPRESSION STRATEGY

### Current State
```
Input: 1.28 MB (test dataset)
Current Kore: 1.28 MB (84.7% - no compression yet)
Target: 1.15 MB (86.2% savings)

Competitors:
  Parquet + Snappy: 2.08 MB (75% savings)
  Brotli Level 9: 0.95 MB (88% savings, 5.0s)
  Kore Target: 1.15 MB (86% savings, 1.0s) ← SWEET SPOT
```

### Phase 1 Stack
```
┌─────────────────────────────────────────┐
│ INPUT DATA (CSV/JSON)                   │
└────────────┬────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────┐
│ COLUMNAR FORMAT                         │
│  • Row groups (65K rows/chunk)          │
│  • Dictionary-encoded strings           │
│  • Numeric types as-is                  │
└────────────┬────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────┐
│ ALGORITHM LAYER (NEW - May 22-31)       │
│  1. Dictionary Encoding (strings)       │
│  2. Zstandard Compression (numeric)     │
│  3. Hybrid codec selector               │
│  4. Block-level framing                 │
└────────────┬────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────┐
│ KORE FILE FORMAT (v1.2.2+)              │
│  • Header: Magic, version, metadata     │
│  • Blocks: Compressed chunks            │
│  • Footer: CRC32, offsets               │
└─────────────────────────────────────────┘
```

---

## 🔧 THREE CORE COMPONENTS

### 1. DICTIONARY ENCODING (Strings)

**How it works:**
```
Input strings:
  "customer_1", "customer_2", "customer_1", "customer_3", ...

Step 1: Build dictionary
  Index  | String
  -------|----------------
  0      | "customer_1"
  1      | "customer_2"
  2      | "customer_3"
  ...    | ...

Step 2: Encode column as indices
  [0, 1, 0, 2, 0, 1, 2, 0, 1, ...]  ← Much smaller!

Step 3: Compress indices
  Zstd compresses [0,1,0,2...] very efficiently

Result:
  Original: 12 bytes × 1M rows = 12 MB
  Dictionary: 100 bytes + [0-255 indices] = 1 MB
  After Zstd: ~200 KB ← 98% compression on strings!
```

**Implementation (Rust pseudocode):**
```rust
struct DictionaryEncoder {
    dictionary: HashMap<String, u32>,
    reverse_dict: Vec<String>,
    encoded_values: Vec<u32>,
}

impl DictionaryEncoder {
    fn encode(values: &[String]) -> Self {
        let mut dict = HashMap::new();
        let mut encoded = Vec::new();
        
        for value in values {
            if !dict.contains_key(value) {
                let idx = dict.len() as u32;
                dict.insert(value.clone(), idx);
            }
            encoded.push(dict[value]);
        }
        
        Self {
            dictionary: dict,
            reverse_dict: /* ... */,
            encoded_values: encoded,
        }
    }
    
    fn compress(&self) -> Vec<u8> {
        // 1. Serialize dictionary
        // 2. Zstd compress encoded indices
        // 3. Return [dict_size, dict_bytes, compressed_indices]
    }
}
```

**Savings Potential:**
- String columns: 80-95% (if good cardinality)
- Test dataset estimate: -350 KB

---

### 2. ZSTANDARD COMPRESSION (Numeric + Dictionary Indices)

**Why Zstandard?**
```
Compression Ratio  | Speed (MB/s)  | Library
---|---|---
Zstd (level 3): 2.8x | 185 MB/s | zstd crate (battle-tested)
Brotli (level 9): 3.0x | 25 MB/s | brotli crate (slow)
LZSS (existing): 2.0x | 100 MB/s | Already in Kore
Snappy: 2.2x | 500 MB/s | snap crate (less compression)
```

**Integration Points:**
```rust
// In Cargo.toml
[dependencies]
zstd = "0.13"  // Already added

// In lib.rs - New compression module
mod compression {
    use zstd::Encoder;
    
    pub fn compress_block(data: &[u8], level: i32) -> Vec<u8> {
        let mut encoder = Encoder::new(Vec::new(), level)
            .expect("zstd encoder");
        encoder.write_all(data)
            .expect("write to zstd");
        encoder.finish()
            .expect("finish zstd")
    }
    
    pub fn decompress_block(data: &[u8]) -> Vec<u8> {
        let decoder = Decoder::new(data)
            .expect("zstd decoder");
        let mut result = Vec::new();
        io::copy(&mut decoder, &mut result)
            .expect("decode");
        result
    }
}
```

**Configuration:**
```rust
// Hybrid codec selector (intelligent compression)
fn select_codec(column: &Column) -> CompressionCodec {
    match column.data_type {
        DataType::String => CompressionCodec::Dictionary,  // 80-95%
        DataType::Int64 if column.cardinality < 1000 => 
            CompressionCodec::DictionaryThenZstd,  // Two-stage
        DataType::Float64 => CompressionCodec::ZstdLevel3,  // 2.8x
        DataType::Boolean => CompressionCodec::RLE,  // Single bits
        _ => CompressionCodec::ZstdLevel1,  // Default
    }
}
```

**Savings Potential:**
- Numeric columns: 2.5-3.0x (Zstd level 3)
- Test dataset estimate: -350 KB

---

### 3. HYBRID CODEC SELECTOR

**Decision Tree:**
```
Column Data
    │
    ├─ String type?
    │   ├─ Cardinality < 1000? → Dictionary (save 80-95%)
    │   ├─ Cardinality > 10000? → Zstd Level 3 (save 2.5x)
    │   └─ Medium? → Try both, pick best
    │
    ├─ Numeric type?
    │   ├─ All same value? → RLE (1 byte!)
    │   ├─ Mostly small gaps? → Delta encoding
    │   └─ Random? → Zstd Level 3
    │
    ├─ Boolean? → Bit-pack (1 bit per value)
    │
    └─ Other? → Zstd Level 1 (default)

Optimization: Sample first 1000 rows, try 3 codecs,
pick codec with best compression ratio
```

**Result on Test Data:**
```
Before: 1.28 MB
  Strings (350KB) → Dictionary + Zstd → 70 KB (-280 KB)
  Numerics (800KB) → Zstd Level 3 → 230 KB (-570 KB)
  Metadata (130KB) → Keep as-is → 130 KB
After: ~430 KB → 1.15 MB overall (86% efficient)
```

---

## 📊 COMPRESSION BENCHMARKS (Target)

```
Test Dataset: 1.28 MB CSV file, 1M rows, 12 columns

BEFORE (v1.2.1):
  Size: 1.28 MB
  Compression: 0% (pass-through)
  Speed: N/A

AFTER (v1.2.2 Phase 1 - TARGET):
  Size: 1.15 MB
  Compression: 86.2%
  Speed: 1.0 sec (faster than Brotli's 5.0 sec)
  
VS COMPETITORS:
  Parquet + Snappy: 2.08 MB (worse by 81%)
  Brotli Level 9: 0.95 MB (better by 17%, but takes 5x longer)
  Our Target: Sweet spot!

Breakdown by Column Type:
  Strings (350KB input):
    Dictionary + Zstd → 70 KB (80% compression)
  Numerics (800KB input):
    Zstd Level 3 → 230 KB (71% compression)
  Metadata (130KB):
    No compression → 130 KB (0%)
  Total: 430 KB payload + overhead = 1.15 MB file
```

---

## 🏗️ IMPLEMENTATION PHASES

### Phase 1A: Dictionary Encoding (May 22-24)
```
1. Add StringDictionary struct
2. Implement encode/decode
3. Zstd compress indices
4. Unit tests (10+)
5. Benchmarks on string columns only
```

### Phase 1B: Numeric Compression (May 25-27)
```
1. Integrate Zstd level 3
2. Add delta encoding option
3. RLE for repeated values
4. Hybrid selector logic
5. Full dataset benchmarks
```

### Phase 1C: Polish & Release (May 28-31)
```
1. Performance tuning
2. Documentation
3. Release candidate
4. Final benchmarks
5. Public announcement
```

---

## 🔗 INTEGRATION POINTS

**Existing Kore Code:**
```rust
// src/lib.rs - Add compression module
pub mod compression {
    pub mod dictionary;      // NEW
    pub mod zstandard;       // NEW
    pub mod codec_selector;  // NEW
}

// src/format.rs - Hook into file writer
impl KoreFileWriter {
    fn write_column(&mut self, column: &Column) {
        let codec = select_codec(column);  // NEW!
        
        match codec {
            CompressionCodec::Dictionary => {
                let dict_encoder = DictionaryEncoder::encode(&column.data);
                let compressed = zstd::compress(&dict_encoder.encode());
                self.write_block(compressed);
            }
            CompressionCodec::ZstdLevel3 => {
                let compressed = zstd::compress_level(&column.data, 3);
                self.write_block(compressed);
            }
            // ... other codecs
        }
    }
}
```

**Feature Flags:**
```toml
[features]
default = ["compression-zstd"]
compression-zstd = ["zstd"]
compression-dict = []
compression-all = ["zstd", "brotli"]
```

---

## ✅ SUCCESS CRITERIA

- ✅ Dictionary encoding on strings: 80%+ compression
- ✅ Zstd on numerics: 2.8x compression (71%+)
- ✅ Overall file: 1.15 MB (86%+)
- ✅ Compression speed: < 1.0 sec (faster than Brotli)
- ✅ Decompression: Transparent to users (no API changes)
- ✅ Documentation: Complete with benchmarks
- ✅ Tests: 100% code coverage on compression modules

---

## 📌 RISKS & MITIGATION

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Zstd compression too slow | Miss 1.0s target | Use level 2-3, not level 9 |
| Dictionary encoding doesn't save enough | Miss 86% target | Add delta encoding for numerics |
| File format changes break compatibility | Customers angry | Use new version flag (v2) |
| Memory overhead (large dictionaries) | Out of memory | Chunk data into 64K row blocks |

---

**ARCHITECTURE COMPLETE** ✅  
Ready for implementation starting May 22
