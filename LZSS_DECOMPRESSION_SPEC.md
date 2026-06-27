# LZSS (Lempel-Ziv-Storer-Szymanski) Decompression Spec

**Status:** Specification (Ready for Implementation Week 6)  
**Target:** 150 lines of code  
**Timeline:** June 22-28, 2026  
**Owner:** Implementation Engineer #2

---

## 📋 Overview

**What:** LZSS compression (sliding window with backreferences)  
**When to use:** Text data (JSON, CSV, logs), unstructured data  
**Best for:** Highly repetitive text, strings, nested structures  
**Expected compression:** 50-75% ratio on text/JSON  
**Decompression speed target:** 800+ MB/s

---

## 🔍 Algorithm Details

### LZSS Concept

Instead of storing repeated text, store a reference back to earlier occurrence:

```
Example: "The quick brown fox jumps over the quick brown dog"

Naive encoding: Every word stored = ~50 bytes
LZSS encoding:
  - "The quick brown fox jumps over "  (literal 31 bytes)
  - [distance: 32, length: 17]         (backreference: "The quick brown ")
  - "dog"                              (literal 3 bytes)
  Total: ~15 bytes! ✅ 70% compression

How backreference works:
  distance = 32 (go back 32 bytes in history)
  length = 17 (copy 17 bytes from that position)
```

### Compression Format

```
LZSS Format (Binary):
[block_type: 1 bit] [data]

Block type 0 (Literal):  Store raw bytes
Block type 1 (Backref):  Store distance + length

Full format:
[flag_byte: 8 bits] [block1] [block2] ... [block8]

Flag byte:
  bit 0 = block 1 type (0=literal, 1=backref)
  bit 1 = block 2 type
  ...
  bit 7 = block 8 type

Each literal block: [length: 8 bits] [data: length bytes]
Each backref block: [distance: 16 bits] [length: 8 bits]

Example:
Input: "abcabcabc"
Flag: 0b00000001 (1 literal, rest are backrefs)
Block 0: [length:3] [a][b][c]         (literal "abc")
Block 1: [dist:3, len:3]              (backref: distance 3, length 3 = "abc")
Block 2: [dist:6, len:3]              (backref: distance 6, length 3 = "abc")
Output: "abcabcabc" ✅
```

---

## 💻 Implementation Pseudocode

```rust
pub fn decompress_lzss(input: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut cursor = 0;
    
    while cursor < input.len() {
        // Read flag byte
        let flag_byte = input[cursor];
        cursor += 1;
        
        // Process 8 blocks (one per bit in flag byte)
        for bit in 0..8 {
            if cursor >= input.len() {
                break;
            }
            
            let is_backref = (flag_byte >> bit) & 1 == 1;
            
            if is_backref {
                // Read backreference: distance (2 bytes) + length (1 byte)
                if cursor + 3 > input.len() {
                    return Err("Truncated backref");
                }
                
                let distance = u16::from_le_bytes([
                    input[cursor],
                    input[cursor + 1]
                ]) as usize;
                cursor += 2;
                
                let length = input[cursor] as usize;
                cursor += 1;
                
                // Validate distance
                if distance == 0 || distance > output.len() {
                    return Err("Invalid distance");
                }
                
                // Copy from history
                let start = output.len() - distance;
                for i in 0..length {
                    let byte = output[start + i];
                    output.push(byte);
                }
            } else {
                // Read literal: length (1 byte) + data
                if cursor >= input.len() {
                    return Err("Truncated literal");
                }
                
                let length = input[cursor] as usize;
                cursor += 1;
                
                if cursor + length > input.len() {
                    return Err("Truncated data");
                }
                
                output.extend_from_slice(&input[cursor..cursor + length]);
                cursor += length;
            }
        }
    }
    
    Ok(output)
}
```

---

## 🎯 Distance and Length Encoding

### Distance (16-bit)
```
Represents how far back in history to look:
  1 = 1 byte back
  32 = 32 bytes back
  65535 = 65KB back (typical window size)

Why 16-bit? 
  Standard: 32KB sliding window = 2^15 distance
  Extended: 64KB sliding window = 2^16 distance
```

### Length (8-bit)
```
How many bytes to copy from history:
  1 = copy 1 byte
  255 = copy 255 bytes

Why 8-bit?
  Typical repeated patterns are 3-50 bytes
  Max of 255 covers most practical cases
  Longer matches: use multiple blocks
```

---

## 🧪 Test Cases

### Unit Test 1: Literal Only (No Compression)
```
Input: "Hello world"
Expected: Copy all literally
Success: Output == input (0% compression, valid case)
```

### Unit Test 2: Simple Backreference
```
Input: "abcabc"
Blocks:
  - Literal: "abc"
  - Backref: distance=3, length=3
Expected: "abcabc"
Success: Backreference resolves correctly
```

### Unit Test 3: Multiple Backreferences
```
Input: "The quick brown fox... The quick brown dog"
Multiple backreferences to "The quick brown"
Expected: Full text reconstructed
Success: Multi-block decompression works
```

### Unit Test 4: Overlapping Backreference
```
Input: "aaaa" (4 'a's)
Could be encoded as:
  - Literal: "a"
  - Backref: distance=1, length=3 (copy last byte 3 times)
Expected: "aaaa"
Success: Overlapping copy works (distance < length)
```

### Unit Test 5: Long Copy
```
Input: 100+ byte repeated pattern
Single backref with distance + length
Expected: Entire pattern decompresses
Success: Long length values work
```

### Unit Test 6: Text Data (CSV)
```
Input: CSV with repeated column names
Expected: Text decompresses correctly
Success: Real-world compression works
```

### Integration Test: Round-Trip
```
Original: Large JSON file (1MB)
Compress with LZSS
Decompress with LZSS
Expected: Byte-for-byte match
```

---

## ⚡ Performance Targets

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Decompression speed | 800+ MB/s | Time to decompress 1GB test data |
| History lookup | O(1) | Constant time copy |
| Memory overhead | <10% | Peak RAM during decompression |
| LZSS compression | 50-75% | Typical text/JSON ratio |

---

## 🔒 Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Invalid distance | Distance > output length | Return error + position |
| Zero distance | Distance = 0 (impossible) | Return error |
| Truncated backref | Missing distance/length | Return error + position |
| Truncated literal | Missing data bytes | Return error + position |
| Invalid length | Weird values | Return error |

---

## 🔧 Integration Points

### 1. Codec Registry
```rust
// In src/codecs/mod.rs
pub struct LZSSCodec;

impl CodecTrait for LZSSCodec {
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        decompress_lzss(input)
    }
}
```

### 2. Text Column Detection
```rust
// In src/compression_selector.rs
pub fn should_use_lzss(column_type: DataType) -> bool {
    matches!(column_type, DataType::String | DataType::Binary)
}
```

### 3. Fallback Strategy
```rust
// If LZSS doesn't compress well (> 90% ratio)
// Fall back to no compression or KORE+Bzip2
if compressed_size > original_size * 0.9 {
    return CompressionCodec::None;  // Don't compress
}
```

---

## 📊 Decision Tree for LZSS

```
Should I use LZSS?

1. Check if column is text/string data
   ↓ Yes
2. Sample 1000 values (or 1MB if large)
   ↓
3. Estimate compression with LZSS:
   - Repetition analysis
   - Pattern frequency
   ↓
4. If compression > 50%:
   ✅ Use LZSS
   ↓
5. If compression < 50%:
   Try other codecs (Dict, FOR) or skip
```

---

## 💡 Optimization Tips

### Window Management
```
Instead of storing full output:
Use circular buffer for 64KB window
This limits memory to constant size
```

### Bulk Copying
```
Instead of byte-by-byte copy:
Use memcpy for large copies (length > 16)
Target: Overlapping copies (distance < length)
```

### Literal Grouping
```
Instead of 1 literal per block:
Group adjacent literals
Reduce flag bytes overhead
```

---

## 🎯 Definition of Done

- ✅ Decompress function implemented (150 lines)
- ✅ Flag byte processing correct
- ✅ Backreference resolution working
- ✅ All unit tests passing (6+ test cases)
- ✅ Round-trip tests passing
- ✅ Performance benchmark: 800+ MB/s
- ✅ Text data compression verified
- ✅ Error handling complete
- ✅ Overlapping copy handling verified
- ✅ Code reviewed by 2+ engineers
- ✅ Documentation complete
- ✅ Integration with codec registry verified

---

## 📚 References

- LZSS algorithm: [Wikipedia LZSS](https://en.wikipedia.org/wiki/Lempel%E2%80%93Ziv%E2%80%93Storer%E2%80%93Szymanski)
- Sliding window: Standard compression technique
- Backreferences: LZ77 family compression

---

**Status:** Ready for implementation  
**Assigned to:** Implementation Engineer #2  
**Review by:** Lead Engineer  
**Deadline:** June 28, 2026
