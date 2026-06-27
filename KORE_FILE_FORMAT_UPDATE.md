# Kore File Format Update Spec

**Status:** Specification (Ready for Implementation May 17-31)  
**Impact:** Core file format changes  
**Backward Compatibility:** Full (v1.0.0 can read old files)  
**Timeline:** May 17-24 (design phase)

---

## 🎯 Overview

**Goal:** Update Kore binary format to support multiple decompression codecs  
**Current State:** Single codec (write-only, no decompression)  
**New State:** Multi-codec with codec selection + hybrid compression  
**Compatibility:** Files from Phase 1-7 remain readable (backward compatible)

---

## 📊 Current File Format (Phase 1-7)

```
Kore File Structure:
┌─ HEADER ──────────────────┐
│ Magic: 0x4B4F5245         │  4 bytes  "KORE"
│ Version: 0x01             │  1 byte   (v1)
│ Flags: 0x00               │  1 byte   (reserved)
│ Columns: 1                │  1 byte   (number of columns)
├─ COLUMN METADATA ─────────┤
│ Name: "data"              │  varint + bytes
│ Type: Int32               │  1 byte
│ Compression: None         │  1 byte   (0=none)
│ Row count: 1000000        │  varint
│ Data offset: 1024         │  varint
├─ COLUMN DATA ─────────────┤
│ [1000000 int32 values]    │  4MB uncompressed
└────────────────────────────┘
```

---

## 🆕 NEW File Format (v2.0 for decompression)

**Decision:** Extend v1 format, don't break it

```
Kore File Structure (v2.0):
┌─ HEADER ──────────────────────┐
│ Magic: 0x4B4F5245             │  4 bytes  "KORE"
│ Version: 0x02                 │  1 byte   (v2 = with decompression)
│ Flags: [codec_bits]           │  1 byte   
│ Columns: 2                    │  1 byte   (2 columns in example)
├─ COLUMN 1 METADATA ───────────┤
│ Name: "id"                    │  varint + bytes
│ Type: Int32                   │  1 byte   
│ Compression: RLE              │  1 byte   (1=RLE)
│ Row count: 1000000            │  varint
│ Data offset: 2048             │  varint
│ Element size: 4               │  1 byte   (for RLE)
├─ COLUMN 2 METADATA ───────────┤
│ Name: "category"              │  varint + bytes
│ Type: String                  │  1 byte
│ Compression: Dictionary       │  1 byte   (2=Dictionary)
│ Row count: 1000000            │  varint
│ Data offset: 3072             │  varint
│ Dict size: 500                │  varint   (for Dictionary)
├─ COLUMN DATA ─────────────────┤
│ Column 1: [RLE compressed]    │  Variable
│ Column 2: [Dict compressed]   │  Variable
└────────────────────────────────┘
```

---

## 🔢 Compression Codec Identifiers

```
Codec ID  Name         Introduced  Use Case
────────────────────────────────────────────
0         None         Phase 1      No compression
1         RLE          Phase 2      Low cardinality
2         Dictionary   Phase 2      Medium cardinality
3         FOR          Phase 2      Numeric ranges
4         LZSS         Phase 2      Text/JSON
5         Reserved     Future       TBD
6         Reserved     Future       TBD
7         Reserved     Future       TBD
```

---

## 📝 Column Metadata Format (Updated)

### Version 1 (Phase 1-7)
```
Column Metadata v1:
[name_length: varint]
[name: bytes]
[type: 1 byte]
[compression_codec: 1 byte]  ← 0 only (no compression)
[row_count: varint]
[data_offset: varint]
```

### Version 2 (Phase 2+, with decompression)
```
Column Metadata v2:
[name_length: varint]
[name: bytes]
[type: 1 byte]
[compression_codec: 1 byte]  ← 0-4 (can be any codec)
[row_count: varint]
[data_offset: varint]
[codec_params: codec-specific]

Codec-specific params:
  RLE:        [element_size: 1 byte]
  Dictionary: [dict_size: varint]
  FOR:        [bit_width: 1 byte] [base_value: 8 bytes]
  LZSS:       [uncompressed_size: varint]
  None:       (no params)
```

---

## 🔄 Backward Compatibility

### Reading Old Files (v1)
```rust
// Old file (Phase 1-7)
// Version: 0x01, Compression: 0x00 (None)

fn read_v1_file(data: &[u8]) -> Result<DataFrame> {
    let version = data[4];
    
    match version {
        0x01 => {
            // Old format: No compression, read directly
            // Codec=0 means raw data, no decompression needed ✅
        },
        0x02 => {
            // New format: Codec can be 1-4, needs decompression
        },
    }
}
```

### Writing New Files (v2)
```rust
// New file can use any codec
// Version: 0x02, Compression: 1-4 (with decompression support)

fn write_v2_file(data: DataFrame) -> Result<Vec<u8>> {
    let version = 0x02;  // v2 indicates codec support
    
    // Choose best codec for each column
    for column in &data.columns {
        let codec = select_best_codec(column)?;
        // Write with codec_id and codec_params
    }
}
```

**Result:** ✅ All old files readable, new files use codecs

---

## 🎯 Codec Selection Algorithm

When writing a file, choose the best codec per column:

```
Algorithm: Choose Best Codec
Input: Column data, Data type
Output: Codec ID, codec_params

1. If uncompressed size < 1KB:
   → CompressionCodec::None (not worth it)

2. If Data Type is Numeric:
   → Try FOR (bit-packing)
   → If compression < 50%, fallback to None

3. If Data Type is String/Text:
   → Try Dictionary (if cardinality < 20%)
   → Else try LZSS
   → If compression < 50%, fallback to KORE+Bzip2

4. If Data Type is Category/Enum:
   → Use Dictionary (always good for categories)

5. If All Values Same:
   → Use RLE (best case: 99% compression!)

6. If All Else Fails:
   → Use None (no compression)

7. Hybrid Fallback:
   → If compression > 65%, try KORE+Bzip2
   → Can achieve 50% ratio (matches ORC)
```

---

## 📋 File Format Version History

| Version | Phase | Codecs | Decompression | Introduced |
|---------|-------|--------|---------------|------------|
| v0.1 | 1 | — | ❌ | Phase 1 (write-only demo) |
| v1.0 | 1-7 | None | ❌ | Phase 7 (complete) |
| v2.0 | 2 | RLE, Dict, FOR, LZSS | ✅ | Phase 2 (Aug 31) |
| v2.1 | 3 | + Streaming | ✅ | Phase 3 (Oct 31) |
| v3.0 | 4 | + Indexing | ✅ | Phase 4 (Dec 31) |

---

## 🔍 Reading Algorithm (Updated)

```rust
pub fn read_kore_file(path: &str) -> Result<DataFrame> {
    let data = std::fs::read(path)?;
    
    // Read header
    let magic = &data[0..4];
    if magic != b"KORE" {
        return Err("Invalid magic");
    }
    
    let version = data[4];
    match version {
        0x01 => read_v1_format(&data),  // Phase 1-7: No decompression
        0x02 => read_v2_format(&data),  // Phase 2+: With decompression
        0x03 => read_v3_format(&data),  // Future: Streaming + indexing
        _ => Err("Unsupported version"),
    }
}

fn read_v2_format(data: &[u8]) -> Result<DataFrame> {
    let mut df = DataFrame::new();
    let column_count = data[6];
    
    let mut offset = 7;
    
    for _ in 0..column_count {
        // Read column metadata
        let (name, col_offset) = read_string(&data[offset..])?;
        offset += col_offset;
        
        let col_type = data[offset];
        offset += 1;
        
        let codec_id = data[offset];
        offset += 1;
        
        let (row_count, vc) = read_varint(&data[offset..])?;
        offset += vc;
        
        let (data_offset, doff) = read_varint(&data[offset..])?;
        offset += doff;
        
        // Read codec-specific params
        let codec = match codec_id {
            0 => CompressionCodec::None,
            1 => {
                let elem_size = data[offset];
                offset += 1;
                CompressionCodec::RLE(elem_size)
            },
            2 => {
                let (dict_size, vc) = read_varint(&data[offset..])?;
                offset += vc;
                CompressionCodec::Dictionary(dict_size)
            },
            3 => {
                let bit_width = data[offset];
                offset += 1;
                let base = i64::from_le_bytes([
                    data[offset], data[offset+1], /* ... */
                ]);
                offset += 8;
                CompressionCodec::FOR { bit_width, base }
            },
            4 => CompressionCodec::LZSS,
            _ => return Err("Unknown codec"),
        };
        
        // Read compressed column data
        let compressed_data = &data[data_offset..];  // To end of file or next column
        
        // Decompress based on codec
        let decompressed = match codec {
            CompressionCodec::None => compressed_data.to_vec(),
            CompressionCodec::RLE(size) => rle_decompress(compressed_data, size)?,
            CompressionCodec::Dictionary(_) => dict_decompress(compressed_data)?,
            CompressionCodec::FOR { bit_width, base } => for_decompress(compressed_data, bit_width, base)?,
            CompressionCodec::LZSS => lzss_decompress(compressed_data)?,
        };
        
        // Convert to column
        let column = create_column(name, col_type, &decompressed, row_count)?;
        df.add_column(column);
    }
    
    Ok(df)
}
```

---

## 🧪 Testing the Format Changes

### Test 1: Read Old File (Phase 1-7)
```
Load file from Phase 1-7 (no decompression)
Expected: File loads correctly, compression codec = None
Success: Backward compatibility verified ✅
```

### Test 2: Write New File (Phase 2+)
```
Create DataFrame
Write with v2.0 format
Choose codec for each column
Expected: File written with proper metadata
Success: New format works ✅
```

### Test 3: Round-Trip with Codecs
```
Create DataFrame
Compress with RLE/Dict/FOR/LZSS
Write v2.0 file
Read v2.0 file
Decompress
Expected: Data matches original
Success: Codec round-trip verified ✅
```

### Test 4: Mixed Codec File
```
Create DataFrame with 4 columns
Column 1: RLE (low cardinality)
Column 2: Dictionary (categories)
Column 3: FOR (numeric)
Column 4: LZSS (text)
Expected: Each column uses best codec
Success: Multi-codec file works ✅
```

---

## 🔒 Data Integrity

### Checksums (Optional for Phase 2, Required for Phase 3)
```
Format: MD5 or SHA256 of each column data

Location in file:
[header][metadata][column_data][checksums]

Verification on read:
For each column:
  1. Decompress data
  2. Calculate checksum
  3. Compare with stored checksum
  4. If mismatch → Error (data corruption)
```

---

## 📐 File Size Impact

### Example: 1MB CSV File

| Compression | Size | Ratio | Read Speed | Encode Overhead |
|-------------|------|-------|-----------|-----------------|
| None (v1) | 1.0 MB | 100% | Instant | ~0% |
| RLE (v2) | 0.65 MB | 65% | 1000+ MB/s | +10 bytes metadata |
| Dictionary (v2) | 0.55 MB | 55% | 500+ MB/s | +20 bytes metadata |
| FOR (v2) | 0.60 MB | 60% | 2000+ MB/s | +15 bytes metadata |
| LZSS (v2) | 0.50 MB | 50% | 800+ MB/s | +10 bytes metadata |
| Hybrid v2 | 0.50 MB | 50% | Varies | +10 bytes metadata |

**Total overhead:** < 100 bytes per file (negligible)

---

## 🎯 Implementation Tasks

### Task 1: Update Header (May 17-18)
- [ ] Change version byte to 0x02
- [ ] Update flags for codec support
- [ ] Add codec metadata parsing

### Task 2: Update Column Metadata (May 18-19)
- [ ] Add codec_params to metadata
- [ ] Handle codec-specific fields
- [ ] Maintain backward compatibility

### Task 3: Update Compression Selection (May 19-20)
- [ ] Implement codec selection algorithm
- [ ] Add cardinality analysis
- [ ] Add compression estimation

### Task 4: Update Write Path (May 20-21)
- [ ] Write v2.0 format
- [ ] Include codec metadata
- [ ] Store codec_params

### Task 5: Update Read Path (May 21-22)
- [ ] Read v2.0 format
- [ ] Parse codec metadata
- [ ] Route to correct decompressor

### Task 6: Testing (May 22-24)
- [ ] Test backward compatibility
- [ ] Test round-trip for each codec
- [ ] Test mixed-codec files
- [ ] Test format edge cases

---

## 🚀 Rollout Plan

### Phase 2.0 (Aug 31, 2026)
- ✅ Write v2.0 with all codecs
- ✅ Read v1.0 files (backward compat)
- ✅ Read v2.0 files (with decompression)

### Phase 2.1 (Oct 31, 2026)
- ✅ Add streaming support (v2.1)
- ✅ Add checksums for integrity

### Phase 3.0 (Dec 31, 2026)
- ✅ Add indexing (v3.0)
- ✅ Add enterprise features

---

## 📚 Documentation

### For Users
```markdown
# Kore File Format Guide

## Backward Compatibility
Kore v2.0 can read files from v1.0 (Phase 1-7).
All old files continue to work.

## New Codecs
v2.0 introduces 4 new compression codecs:
- RLE: For low-cardinality data
- Dictionary: For categorical data
- FOR: For numeric data
- LZSS: For text/JSON data

## Auto-Selection
By default, Kore chooses the best codec for each column.
You don't need to configure anything.
```

### For Developers
```markdown
# File Format Reference

## Version 2.0 Specification
See KORE_FILE_FORMAT_UPDATE.md for complete specification.

## Adding New Codecs
1. Add codec ID to enum
2. Define codec_params in metadata
3. Implement decompress function
4. Update codec selection algorithm
5. Add tests
```

---

**Status:** Ready for implementation (May 17-24)  
**Owner:** Lead Engineer  
**Review by:** Lead Engineer + 1 other  
**Deadline:** May 24, 2026 (design review)  
**Implementation:** May 25 - June 15 (parallel with codec specs)
