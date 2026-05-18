# RLE (Run-Length Encoding) Decompression Spec

**Status:** Specification (Ready for Implementation Week 3)  
**Target:** 150 lines of code  
**Timeline:** June 1-7, 2026  
**Owner:** Implementation Engineer #1

---

## 📋 Overview

**What:** Run-Length Encoding decompression codec  
**When to use:** Low cardinality data (repeated values, e.g., gender, region, status)  
**Expected compression:** 65-90% ratio on low-cardinality data  
**Decompression speed target:** 1000+ MB/s

---

## 🔍 Algorithm Details

### Compression Format

```
RLE Format (Binary):
[header: 1 byte] [value1: 1-8 bytes] [count1: varint] 
                 [value2: 1-8 bytes] [count2: varint] ...

Example (Low cardinality):
Value: 42 (int8), repeated 1000 times
Encoded: [42] [varint(1000)]
         = 3 bytes (vs 1000 bytes original) = 99.7% compression!
```

### Data Type Handling

| Data Type | Size | Storage | Example |
|-----------|------|---------|---------|
| int8 | 1 byte | Direct | [42] |
| int16 | 2 bytes | Direct | [42, 0] |
| int32 | 4 bytes | Direct | [42, 0, 0, 0] |
| int64 | 8 bytes | Direct | [42, 0, 0, 0, 0, 0, 0, 0] |
| float32 | 4 bytes | Direct | IEEE 754 encoding |
| float64 | 8 bytes | Direct | IEEE 754 encoding |
| string | Variable | Length-prefixed | [len_varint][bytes] |
| null | 0 bytes | Special marker | [0xFF] = null repeat |

### Varint Encoding (Count)

Why varint? Compression ratios on counts:
- 1-127 repeats: 1 byte
- 128-16K repeats: 2 bytes
- 16K-2M repeats: 3 bytes
- 2M+ repeats: 4 bytes

```rust
// Pseudocode: Encode count as varint
fn encode_varint(n: u32) -> Vec<u8> {
    let mut result = vec![];
    let mut val = n;
    
    while val >= 128 {
        result.push((val & 0x7F | 0x80) as u8);  // Set MSB
        val >>= 7;
    }
    result.push((val & 0x7F) as u8);  // Final byte (no MSB)
    result
}

// Example: count=1000
// 1000 = 0x3E8 = 0b0011_1110_1000
// Split into 7-bit chunks: 111 | 1110_1000
// Encoded: [0xE8, 0x07] = [(1000 & 0x7F | 0x80), (1000 >> 7)]
```

---

## 💻 Implementation Pseudocode

```rust
pub fn decompress_rle(
    input: &[u8],
    element_size: usize,  // Size of each value (1-8 bytes)
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut cursor = 0;
    
    while cursor < input.len() {
        // Read value
        if cursor + element_size > input.len() {
            return Err("Truncated value");
        }
        let value = &input[cursor..cursor + element_size];
        cursor += element_size;
        
        // Read varint count
        let (count, varint_len) = read_varint(&input[cursor..])?;
        if count == 0 {
            return Err("Invalid count (zero)");
        }
        cursor += varint_len;
        
        // Repeat value count times
        for _ in 0..count {
            output.extend_from_slice(value);
        }
    }
    
    Ok(output)
}

fn read_varint(input: &[u8]) -> Result<(u32, usize)> {
    let mut result: u32 = 0;
    let mut shift = 0;
    let mut pos = 0;
    
    for byte in input.iter() {
        result |= ((*byte & 0x7F) as u32) << shift;
        shift += 7;
        pos += 1;
        
        if *byte < 128 {
            return Ok((result, pos));
        }
        
        if shift > 32 {
            return Err("Varint overflow");
        }
    }
    
    Err("Unterminated varint")
}
```

---

## 🧪 Test Cases

### Unit Test 1: Single Value, Single Repeat
```
Input: value=42 (1 byte), count=1
Expected: [42]
Success: Decompress([42, 0x01]) == [42]
```

### Unit Test 2: Single Value, Large Repeat
```
Input: value=42 (1 byte), count=10000
Expected: 10000x [42]
Success: Output length == 10000, all bytes == 42
```

### Unit Test 3: Multiple Values
```
Input: [42, 0x03] [99, 0x02]
Expected: [42, 42, 42, 99, 99]
Success: Decompress correctly identifies transitions
```

### Unit Test 4: All Data Types
```
int8:   [127, 0xFF, 0x01]    → [127] repeated 255 times
int32:  [0xFF, 0, 0, 0, 0x02] → [0xFF, 0, 0, 0] repeated 2 times
float64: [8-byte IEEE754, varint] → [float_value] repeated N times
string: [len_varint][bytes][count_varint]
```

### Unit Test 5: Edge Cases
```
Empty:   [] → []
One:     [42, 0x01] → [42]
Max U32: count = 4,294,967,295
Null:    [0xFF, 0x01] → null repeated 1 time
```

### Integration Test: Round-Trip
```
Original data: [1, 1, 1, 2, 2, 3, 3, 3, 3]
Compress:      [1, 0x03, 2, 0x02, 3, 0x04]
Decompress:    [1, 1, 1, 2, 2, 3, 3, 3, 3]
Success: Matches original
```

---

## ⚡ Performance Targets

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Decompression speed | 1000+ MB/s | Time to decompress 1GB test data |
| Compression ratio | 65-90% | Size after compression / before |
| Memory overhead | <10% | Peak RAM during decompression |
| Codec detection | O(1) | Instant (no analysis) |

---

## 🔒 Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Truncated value | Input too short | Return error + partial output |
| Truncated varint | No terminator | Return error + position |
| Zero count | Invalid data | Return error + position |
| Varint overflow | Count > 2^32 | Return error |
| Invalid element size | Size mismatch | Return error at start |

---

## 🔧 Integration Points

### 1. Codec Registry
```rust
// In src/codecs/mod.rs
pub enum CompressionCodec {
    None = 0,
    RLE = 1,
    Dictionary = 2,
    FOR = 3,
    LZSS = 4,
}

// In src/codecs/rle.rs
impl CodecTrait for RLECodec {
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        decompress_rle(input, self.element_size)
    }
}
```

### 2. Column Decompression
```rust
// In src/column_reader.rs
match column.compression_codec {
    CompressionCodec::RLE => {
        let decompressed = rle_codec.decompress(column_data)?;
        Ok(decompressed)
    },
    // ... other codecs
}
```

### 3. File Header
```
Kore file header:
[magic: 4 bytes] [version: 1 byte] [columns: metadata] 
Column metadata:
  [codec_id: 1 byte] [element_size: 1 byte] ...
```

---

## 📊 Decision Tree for RLE

```
Should I use RLE?

1. Check cardinality (% unique values)
   ↓
2. If cardinality < 10% → RLE is good
   ↓
3. Estimate compression:
   - Single value: 99% compression
   - 10 unique: 80-90% compression
   - 100 unique: 70-80% compression
   - 1000+ unique: <50% compression → use other codec
   ↓
4. Decision: Use RLE if compression > 50%
```

---

## 🎯 Definition of Done

- ✅ Decompress function implemented (150 lines)
- ✅ All unit tests passing (5+ test cases)
- ✅ Round-trip tests passing (write → compress → decompress → verify)
- ✅ Performance benchmark: 1000+ MB/s
- ✅ Error handling complete
- ✅ Code reviewed by 2+ engineers
- ✅ Documentation complete
- ✅ Integration with codec registry verified

---

## 📚 References

- Varint encoding: [Protocol Buffers encoding](https://developers.google.com/protocol-buffers/docs/encoding)
- RLE algorithm: Standard (appears in Parquet, ORC, Avro)
- Test data: `/test-data/low-cardinality-*.csv`

---

**Status:** Ready for implementation  
**Assigned to:** Implementation Engineer #1  
**Review by:** Lead Engineer  
**Deadline:** June 7, 2026
