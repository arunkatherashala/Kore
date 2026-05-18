# Dictionary Compression Decompression Spec

**Status:** Specification (Ready for Implementation Week 4)  
**Target:** 150 lines of code  
**Timeline:** June 8-14, 2026  
**Owner:** Implementation Engineer #2

---

## 📋 Overview

**What:** Dictionary-based compression (lookup table)  
**When to use:** Medium cardinality data (10-10K unique values)  
**Best for:** Strings, categories, enums, status codes  
**Expected compression:** 50-80% ratio  
**Decompression speed target:** 500+ MB/s

---

## 🔍 Algorithm Details

### Compression Format

```
Dictionary Format (Binary):
[dict_size: varint] [dict_entry1: len + bytes] ... [dict_entryN]
[indices: varint_array]

Example (Categorical data):
Values: ["cat", "dog", "cat", "bird", "dog", "cat"]

Dictionary:
  0 → "cat"
  1 → "dog"
  2 → "bird"

Indices: [0, 1, 0, 2, 1, 0]

Encoded:
  [dict_size: 3]
  [3]["cat"] [3]["dog"] [3]["bird"]
  [0, 1, 0, 2, 1, 0]  (as varints)
```

### Dictionary Entry Format

```
For each dictionary entry:
[value_length: varint] [value_bytes: raw] [null_flag: 1 bit]

Example:
Entry "cat":
  [0x03] [0x63, 0x61, 0x74]  (length=3, bytes="cat")

Entry NULL:
  [0x00] [no bytes]  (length=0)
```

### Index Encoding

Indices point to dictionary entries. Use varint for compact storage:

```
Index 0:   [0x00]     (1 byte)
Index 127: [0x7F]     (1 byte)
Index 128: [0x80, 0x01] (2 bytes)
Index 10K: [0x80, 0x4E] (2 bytes)
```

---

## 💻 Implementation Pseudocode

```rust
pub struct DictionaryCodec {
    dictionary: HashMap<Vec<u8>, u32>,  // Value → Index
    indices: Vec<u32>,                  // Index array
}

pub fn decompress_dict(
    input: &[u8],
    dict_size: u32,
) -> Result<Vec<Vec<u8>>> {
    let mut cursor = 0;
    let mut dictionary: Vec<Vec<u8>> = Vec::new();
    
    // Read dictionary
    for _ in 0..dict_size {
        let (len, len_bytes) = read_varint(&input[cursor..])?;
        cursor += len_bytes;
        
        if cursor + len as usize > input.len() {
            return Err("Truncated dictionary");
        }
        
        let value = input[cursor..cursor + len as usize].to_vec();
        cursor += len as usize;
        
        dictionary.push(value);
    }
    
    // Read indices
    let mut output: Vec<Vec<u8>> = Vec::new();
    while cursor < input.len() {
        let (index, index_bytes) = read_varint(&input[cursor..])?;
        cursor += index_bytes;
        
        if index >= dict_size {
            return Err("Invalid dictionary index");
        }
        
        output.push(dictionary[index as usize].clone());
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

## 🎯 When to Use Dictionary Compression

### Cardinality Analysis

```
Cardinality = (unique values) / (total values) * 100%

0-5%:   Excellent for dictionary (use RLE instead)
5-20%:  Great for dictionary ✅
20-50%: Good for dictionary
50%+:   Poor for dictionary (use FOR or LZSS)
```

### Compression Benefit Calculation

```
Original size: N values × value_size bytes
Dictionary size: D entries × avg_entry_size bytes
Index size: N indices × varint_size bytes (avg 1-2 bytes)

Total compressed = D × entry_size + N × 1.5 (avg varint)
Compression ratio = compressed / original

Example (10K values, 100 unique strings):
Original: 10,000 × 50 (avg str len) = 500 KB
Dictionary: 100 × 50 = 5 KB
Indices: 10,000 × 1.5 = 15 KB
Total: 20 KB
Ratio: 20 / 500 = 4% ✅ EXCELLENT
```

---

## 🧪 Test Cases

### Unit Test 1: Single Value Dictionary
```
Dictionary: {0: "cat"}
Indices: [0, 0, 0]
Expected: ["cat", "cat", "cat"]
Success: All match
```

### Unit Test 2: Multiple Values
```
Dictionary: {0: "cat", 1: "dog", 2: "bird"}
Indices: [0, 1, 2, 0, 1]
Expected: ["cat", "dog", "bird", "cat", "dog"]
Success: All match in order
```

### Unit Test 3: Large Dictionary (10K entries)
```
Dictionary: 10,000 unique strings
Indices: References to all 10K entries
Expected: All lookups succeed, no collisions
Success: O(1) lookup time maintained
```

### Unit Test 4: String Length Variations
```
Entries: ["a", "ab", "abc", "abcd", ...]
Expected: All lengths encoded correctly
Success: Varint length prefix works for all
```

### Unit Test 5: NULL values
```
Dictionary: {0: "cat", 1: NULL}
Indices: [0, 1, 0]
Expected: ["cat", null, "cat"]
Success: NULL handling works
```

### Unit Test 6: Edge Cases
```
Empty dictionary: {0: ""}
Expected: Empty string lookups work
```

### Integration Test: Round-Trip
```
Original: ["cat", "dog", "cat", "bird", "dog"]
Compress with dict
Decompress with dict
Expected: Matches original
```

---

## ⚡ Performance Targets

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Decompression speed | 500+ MB/s | Time to decompress 1GB test data |
| Dictionary lookup | O(1) | Constant time index access |
| Memory overhead | <20% | Peak RAM during decompression |
| Dictionary compression | 50-80% | Size after compression / before |

---

## 🔒 Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Truncated dictionary | Input too short | Return error + partial dict |
| Invalid index | Index >= dict_size | Return error + position |
| Truncated entry | Value bytes missing | Return error + position |
| Truncated indices | Indices incomplete | Return error + position |
| Varint overflow | Index > 2^32 | Return error |

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

impl CodecTrait for DictionaryCodec {
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        decompress_dict(input, self.dict_size)
    }
}
```

### 2. Cardinality Detection
```rust
// In src/compression_selector.rs
pub fn should_use_dict(cardinality: f32) -> bool {
    cardinality >= 0.05 && cardinality <= 0.5
}
```

### 3. Column Decompression
```rust
match column.compression_codec {
    CompressionCodec::Dictionary => {
        let decompressed = dict_codec.decompress(column_data)?;
        Ok(decompressed)
    },
}
```

---

## 📊 Decision Tree for Dictionary

```
Should I use Dictionary compression?

1. Analyze column for unique values
   ↓
2. Calculate cardinality = unique / total
   ↓
3. If 5% < cardinality < 50%:
   ✅ Dictionary might be good
   ↓
4. Build sample dictionary (1000 values)
   ↓
5. Measure compression ratio:
   - If ratio < 50% → Use Dictionary ✅
   - If ratio > 50% → Use RLE or skip ❌
```

---

## 🎯 Definition of Done

- ✅ Decompress function implemented (150 lines)
- ✅ All unit tests passing (6+ test cases)
- ✅ Round-trip tests passing
- ✅ Performance benchmark: 500+ MB/s
- ✅ Cardinality detection working
- ✅ Error handling complete
- ✅ Code reviewed by 2+ engineers
- ✅ Documentation complete
- ✅ Integration with codec registry verified

---

## 📚 References

- Dictionary encoding: [Apache Parquet spec](https://parquet.apache.org/docs/file-format/)
- Varint encoding: Protocol Buffers
- Cardinality analysis: Standard database technique

---

**Status:** Ready for implementation  
**Assigned to:** Implementation Engineer #2  
**Review by:** Lead Engineer  
**Deadline:** June 14, 2026
