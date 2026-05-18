# FOR (Frame-of-Reference) Decompression Spec

**Status:** Specification (Ready for Implementation Week 5)  
**Target:** 150 lines of code  
**Timeline:** June 15-21, 2026  
**Owner:** Implementation Engineer #1

---

## 📋 Overview

**What:** Frame-of-Reference compression (bit-packing for numeric ranges)  
**When to use:** Numeric data (int, float, dates, timestamps)  
**Best for:** Columns with values in narrow range (e.g., ages 0-120, dates in 2020-2026)  
**Expected compression:** 60-75% ratio on numeric data  
**Decompression speed target:** 2000+ MB/s

---

## 🔍 Algorithm Details

### Frame-of-Reference Concept

Instead of storing full 64-bit numbers, store only the bits needed:

```
Example: Column of ages (0-120)
Without FOR: [32, 45, 28, 67, 89, 34] = 6 × 8 bytes = 48 bytes
With FOR:
  - Base: 0 (minimum value)
  - Max-Base: 89 (max value)
  - Bits needed: ⌈log2(89)⌉ = 7 bits

Original:  [32,   45,   28,   67,   89,   34  ]
Offset:    [32,   45,   28,   67,   89,   34  ]  (32=32-0, 45=45-0, etc)
As 7-bits: [100000, 101101, 11100, 1000011, 1011001, 100010]
Packed:    0100000_1 01101_11 100_1000 011_10110 01_100010 = 6 bytes! ✅
Compression: 6 / 48 = 12.5% (88% savings!)
```

### Compression Format

```
FOR Format (Binary):
[bit_width: 8 bits] [base_value: 64 bits] [packed_data: variable]

Example (7 values with 7-bit width):
[bit_width: 0x07] [base: 0x00000000] [packed_bits: 7 bytes]
```

### Bit-Width Calculation

```
bit_width = ⌈log2(max_value - base_value + 1)⌉

Examples:
  max=255, base=0:    bit_width = 8
  max=1000, base=0:   bit_width = 10
  max=120, base=0:    bit_width = 7
  max=100, base=50:   bit_width = 6 (100-50=50, log2(50)=5.6≈6)

Special case:
  max=base (all same): bit_width = 0 (no bits needed!)
```

---

## 💻 Implementation Pseudocode

```rust
pub fn decompress_for(
    input: &[u8],
    bit_width: u8,
    base_value: i64,
    num_values: u32,
) -> Result<Vec<i64>> {
    if bit_width == 0 {
        // All values are the same (base_value)
        return Ok(vec![base_value; num_values as usize]);
    }
    
    let mut output = Vec::with_capacity(num_values as usize);
    let mut bit_cursor = 0;  // Position in bits
    
    for _ in 0..num_values {
        // Extract num_bits from packed data
        let value = extract_bits(input, bit_cursor, bit_width)?;
        bit_cursor += bit_width as usize;
        
        // Add to base value
        output.push(base_value + value as i64);
    }
    
    Ok(output)
}

fn extract_bits(
    data: &[u8],
    bit_offset: usize,
    num_bits: u8,
) -> Result<u64> {
    if num_bits > 64 {
        return Err("Too many bits");
    }
    
    let byte_offset = bit_offset / 8;
    let bit_in_byte = bit_offset % 8;
    
    // Read enough bytes to cover the bits we need
    let bytes_needed = ((bit_in_byte + num_bits as usize + 7) / 8) as usize;
    
    if byte_offset + bytes_needed > data.len() {
        return Err("Not enough data");
    }
    
    let mut result: u64 = 0;
    let mut bit_pos = 0;
    
    for i in 0..bytes_needed {
        let byte = data[byte_offset + i];
        let bits_from_byte = if i == 0 {
            (8 - bit_in_byte) as u8  // First byte: skip initial bits
        } else {
            8
        };
        
        let mask = (1u64 << bits_from_byte) - 1;
        let shift_left = if i == 0 {
            bit_in_byte
        } else {
            0
        };
        
        result |= ((byte as u64 >> shift_left) & mask) << bit_pos;
        bit_pos += bits_from_byte;
    }
    
    // Mask to only keep the bits we want
    let mask = (1u64 << num_bits) - 1;
    Ok(result & mask)
}
```

---

## 🎯 Data Types Supported

| Type | Size | Base Range | Example |
|------|------|-----------|---------|
| int8 | 1 byte | -128 to 127 | Ages |
| int16 | 2 bytes | -32K to 32K | Years since epoch |
| int32 | 4 bytes | -2B to 2B | Unix timestamps (seconds) |
| int64 | 8 bytes | Full range | Unix timestamps (nanoseconds) |
| float32 | 4 bytes | IEEE 754 | Temperature (reinterpret bits) |
| float64 | 8 bytes | IEEE 754 | Precise measurements |
| date32 | 4 bytes | Days since epoch | Dates |
| date64 | 8 bytes | Milliseconds | Timestamps |

---

## 🧪 Test Cases

### Unit Test 1: All Same Value (Bit Width = 0)
```
Input: 1000 values of 42
Bit width: 0
Expected: [42, 42, 42, ...] (1000 times)
Success: Decompresses instantly (no unpacking)
```

### Unit Test 2: Small Range (7-bit values)
```
Input: Ages 0-120
Base: 0
Bit width: 7
Expected: All original ages recovered
Success: 88% compression verified
```

### Unit Test 3: Negative Numbers
```
Input: Temperatures -50 to +50°C
Base: -50
Bit width: 7 (covers 0 to 100)
Expected: Original temperatures recovered
Success: Offset handling works
```

### Unit Test 4: Large Range (32-bit)
```
Input: 1 million Unix timestamps
Base: 1704067200 (Jan 1, 2024)
Bit width: 25 (covers next ~650 years)
Expected: All timestamps correct
Success: Large range handling works
```

### Unit Test 5: Floating Point
```
Input: Float32 values 0.0 to 1.0
Reinterpret as int32 bits
Base: 0
Bit width: 31
Expected: Original float values recovered
Success: Bit manipulation correct
```

### Integration Test: Round-Trip
```
Original: [15, 28, 42, 19, 35, 8, 31, 19]
Compress with FOR
Decompress with FOR
Expected: Matches original
```

---

## ⚡ Performance Targets

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Decompression speed | 2000+ MB/s | Time to decompress 1GB test data |
| Bit extraction | O(1) | Constant time per value |
| Memory overhead | <5% | Peak RAM during decompression |
| FOR compression | 60-75% | Typical numeric column |

---

## 🔒 Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Bit width > 64 | Invalid encoding | Return error |
| Not enough data | Truncated input | Return error + position |
| Invalid base value | Negative bit width | Return error |
| Overflow | Value + base > i64::MAX | Return error + value |

---

## 🔧 Integration Points

### 1. Codec Registry
```rust
// In src/codecs/mod.rs
pub struct FORCodec {
    bit_width: u8,
    base_value: i64,
    num_values: u32,
}

impl CodecTrait for FORCodec {
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        let values = decompress_for(
            input,
            self.bit_width,
            self.base_value,
            self.num_values
        )?;
        Ok(to_bytes(&values))  // Convert back to bytes
    }
}
```

### 2. Numeric Column Detection
```rust
// In src/compression_selector.rs
pub fn should_use_for(column_type: DataType) -> bool {
    matches!(column_type, 
        DataType::Int8 | DataType::Int16 | DataType::Int32 | 
        DataType::Int64 | DataType::Date | DataType::Timestamp
    )
}
```

### 3. Base Value Selection
```rust
// Calculate min/max then select base
let base = column_min;  // OR column_mean for better range
let max_offset = column_max - base;
let bit_width = ceil(log2(max_offset));
```

---

## 📊 Decision Tree for FOR

```
Should I use FOR compression?

1. Check if column is numeric
   ↓ Yes
2. Calculate bit width needed:
   max_value - min_value = range
   bit_width = ⌈log2(range)⌉
   ↓
3. If bit_width ≤ 32:
   ✅ FOR is excellent (50-80% compression)
   ↓
4. If 32 < bit_width ≤ 64:
   ⚠️ FOR is good (30-50% compression)
   ↓
5. If bit_width > 64:
   ❌ Don't use FOR (no savings)
```

---

## 💡 Optimization Tips

### SIMD Vectorization
```
Instead of extracting 1 bit at a time:
Use SIMD to extract 8-16 values in parallel
Target: 2000+ MB/s decompression speed
```

### Bit Width Optimization
```
Instead of ceil(log2(range)):
Use lookup table for common bit widths:
  range ≤ 255:    bit_width = 8
  range ≤ 65535:  bit_width = 16
  range ≤ 2^32-1: bit_width = 32
```

---

## 🎯 Definition of Done

- ✅ Decompress function implemented (150 lines)
- ✅ Bit extraction working correctly
- ✅ All unit tests passing (5+ test cases)
- ✅ Round-trip tests passing
- ✅ Performance benchmark: 2000+ MB/s
- ✅ All numeric types supported
- ✅ Error handling complete
- ✅ Code reviewed by 2+ engineers
- ✅ Documentation complete
- ✅ Integration with codec registry verified

---

## 📚 References

- FOR algorithm: [Apache ORC specification](https://orc.apache.org/specification/)
- Bit packing: Standard compression technique
- SIMD optimization: CPU vectorization for parallel operations

---

**Status:** Ready for implementation  
**Assigned to:** Implementation Engineer #1  
**Review by:** Lead Engineer  
**Deadline:** June 21, 2026
