# Phase 7: Advanced Algorithm Optimization

## Overview
Implementing 5 additional compression algorithms to expand codec support from 7 to 12, targeting specialized data patterns and use cases.

---

## 7.1 Snappy Compression Codec

### Algorithm Profile
- **Type**: Fast, general-purpose compression
- **Compression Ratio**: ~40-50% (moderate)
- **Speed**: 250+ MB/s (fastest)
- **Best For**: Real-time streaming, latency-sensitive workloads
- **Use Case**: Time-series data, logs

### Implementation (src/compression/snappy_codec.rs - ~120 lines)
```rust
use snap::read::FrameDecoder;
use snap::write::FrameEncoder;
use std::io::{Read, Write};

pub struct SnappyCodec;

impl SnappyCodec {
    pub fn compress(data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut encoder = FrameEncoder::new(Vec::new());
        encoder.write_all(data)?;
        Ok(encoder.into_inner()?)
    }
    
    pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut decoder = FrameDecoder::new(compressed);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }
    
    pub fn compression_ratio(data: &[u8]) -> f32 {
        if let Ok(compressed) = Self::compress(data) {
            compressed.len() as f32 / data.len() as f32
        } else {
            1.0
        }
    }
}
```

### Cargo.toml Update
```toml
[dependencies]
snap = "1.1"  # Snappy implementation
```

### Integration
- CodecId: 7
- Selection rule: Real-time data (timestamp columns < 1 hour old)
- Fallback: LZSS if compression ratio > 0.65

---

## 7.2 Brotli Compression Codec

### Algorithm Profile
- **Type**: High-compression, general-purpose
- **Compression Ratio**: 30-45% (excellent)
- **Speed**: 50-100 MB/s (slower)
- **Best For**: Storage optimization, batch processing
- **Use Case**: Archive storage, cold data

### Implementation (src/compression/brotli_codec.rs - ~130 lines)
```rust
use brotli::{enc, dec};
use std::io::{Read, Write};

pub struct BrotliCodec {
    quality: u32,  // 0-11, default 11
}

impl BrotliCodec {
    pub fn new() -> Self {
        Self { quality: 8 }  // Balance speed/compression
    }
    
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut compressed = Vec::new();
        let mut cursor = &data[..];
        
        enc::BrotliEncoderOperation::Finish;
        enc::copy_from_to(
            &mut cursor,
            &mut compressed,
            &mut CompressorMem::new(4096, 16, 15, self.quality, 0)
        )?;
        
        Ok(compressed)
    }
    
    pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut decompressed = Vec::new();
        let mut cursor = &compressed[..];
        
        dec::copy_from_to(
            &mut cursor,
            &mut decompressed,
            &mut DecompressorMem::new(4096, 16, 15)
        )?;
        
        Ok(decompressed)
    }
}
```

### Cargo.toml Update
```toml
[dependencies]
brotli = "2.3"
```

### Integration
- CodecId: 8
- Selection rule: Cold data (file age > 7 days) or archive flag
- Fallback: LZSS if memory budget exceeded

---

## 7.3 LZ4 Compression Codec

### Algorithm Profile
- **Type**: Extremely fast, moderate compression
- **Compression Ratio**: 45-55% (good)
- **Speed**: 400+ MB/s (second fastest)
- **Best For**: Medium-latency workloads
- **Use Case**: Distributed cache, message queues

### Implementation (src/compression/lz4_codec.rs - ~100 lines)
```rust
use lz4::codec::Decoder;
use lz4_flex::{compress, decompress};

pub struct Lz4Codec;

impl Lz4Codec {
    pub fn compress(data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        Ok(compress(data))
    }
    
    pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        decompress(compressed)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
    
    pub fn quick_compress(data: &[u8], max_size: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let compressed = Self::compress(data)?;
        if compressed.len() > max_size {
            Err("Compressed size exceeds limit".into())
        } else {
            Ok(compressed)
        }
    }
}
```

### Cargo.toml Update
```toml
[dependencies]
lz4-flex = "0.10"
```

### Integration
- CodecId: 9
- Selection rule: Real-time but medium-latency (1-10 seconds)
- Fallback: Snappy if speed > 400 MB/s

---

## 7.4 Deflate/GZIP Codec

### Algorithm Profile
- **Type**: Standard compression (zip format)
- **Compression Ratio**: 35-50% (good)
- **Speed**: 80-120 MB/s (moderate)
- **Best For**: Web compatibility, standard formats
- **Use Case**: REST API responses, web services

### Implementation (src/compression/deflate_codec.rs - ~110 lines)
```rust
use flate2::Compression;
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use std::io::{Read, Write};

pub struct DeflateCodec {
    compression_level: Compression,
}

impl DeflateCodec {
    pub fn new() -> Self {
        Self {
            compression_level: Compression::default(),
        }
    }
    
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut encoder = GzEncoder::new(Vec::new(), self.compression_level);
        encoder.write_all(data)?;
        Ok(encoder.finish()?)
    }
    
    pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut decoder = GzDecoder::new(compressed);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }
}
```

### Cargo.toml Update
```toml
[dependencies]
flate2 = "1.0"
```

### Integration
- CodecId: 10
- Selection rule: Data with HTTP transport or external API integration
- Fallback: LZSS for compatibility

---

## 7.5 Dictionary-Based Compression (Specialized)

### Algorithm Profile
- **Type**: Hybrid (Dictionary + Delta)
- **Compression Ratio**: 25-40% (excellent for specific patterns)
- **Speed**: 100-150 MB/s (moderate)
- **Best For**: Highly repetitive text, config files
- **Use Case**: Log aggregation, configuration storage

### Implementation (src/compression/specialized_dict.rs - ~150 lines)
```rust
pub struct SpecializedDictCodec {
    dict_entries: Vec<Vec<u8>>,
    dict_size: usize,
    min_frequency: usize,
}

impl SpecializedDictCodec {
    pub fn new(min_frequency: usize) -> Self {
        Self {
            dict_entries: Vec::new(),
            dict_size: 0,
            min_frequency,
        }
    }
    
    pub fn build_dictionary(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        // Analyze data for repeated patterns
        let mut pattern_freq = std::collections::HashMap::new();
        
        for window in data.windows(16) {  // 16-byte patterns
            *pattern_freq.entry(window.to_vec()).or_insert(0) += 1;
        }
        
        // Select top patterns by frequency
        let mut patterns: Vec<_> = pattern_freq
            .into_iter()
            .filter(|(_, freq)| freq >= &self.min_frequency)
            .collect();
        
        patterns.sort_by_key(|(_, freq)| std::cmp::Reverse(*freq));
        
        for (pattern, _) in patterns.iter().take(256) {
            self.dict_entries.push(pattern.clone());
        }
        
        self.dict_size = self.dict_entries.len();
        Ok(())
    }
    
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut compressed = Vec::new();
        
        // Encode dictionary
        compressed.push(self.dict_size as u8);
        for entry in &self.dict_entries {
            compressed.push(entry.len() as u8);
            compressed.extend_from_slice(entry);
        }
        
        // Encode data using dictionary
        let mut i = 0;
        while i < data.len() {
            let mut found = false;
            
            for (idx, entry) in self.dict_entries.iter().enumerate() {
                if data[i..].starts_with(entry) {
                    compressed.push(255);  // Dictionary marker
                    compressed.push(idx as u8);
                    i += entry.len();
                    found = true;
                    break;
                }
            }
            
            if !found {
                compressed.push(data[i]);
                i += 1;
            }
        }
        
        Ok(compressed)
    }
}
```

### Integration
- CodecId: 11
- Selection rule: Text with >50% repeated patterns
- Fallback: EnhancedDictionary for general use

---

## Codec Selection Logic Update

### Enhanced Selector (src/compression/codec_selector.rs - additions)

```rust
pub fn select_advanced_codec(
    data: &[u8],
    data_type: &DataType,
    profile: &ColumnProfile,
) -> CompressionCodec {
    // Existing logic (codecs 0-6)
    match data_type {
        DataType::String => {
            // NEW: Check for real-time data
            if profile.is_recent() {  // < 1 hour old
                return CompressionCodec::Snappy;
            }
            
            // NEW: Check for repetition
            if profile.repetition_ratio() > 0.5 {
                return CompressionCodec::SpecializedDict;
            }
            
            // Existing: Dictionary encoding
            if profile.cardinality_ratio < 0.001 {
                return CompressionCodec::EnhancedDictionary;
            }
        },
        DataType::Numeric => {
            // NEW: LZ4 for medium latency
            if !profile.is_sorted() && profile.entropy() < 0.6 {
                return CompressionCodec::Lz4;
            }
            
            // Existing: Double Delta for sorted
            if matches!(profile.distribution, DataDistribution::NumericRange) {
                return CompressionCodec::DoubleDelta;
            }
        },
        _ => {}
    }
    
    // NEW: Archive/cold data optimization
    if profile.age_days > 7 {
        return CompressionCodec::Brotli;
    }
    
    // NEW: Web/API optimization
    if profile.transport_type == Some(TransportType::Http) {
        return CompressionCodec::Deflate;
    }
    
    // Default: Zstd
    CompressionCodec::Zstd
}
```

---

## Performance Comparison Matrix

| Codec | Compression | Speed | Use Case | CodecId |
|-------|-------------|-------|----------|---------|
| None | 100% | ∞ | Uncompressed | 0 |
| RLE | 10-90% | 500MB/s | Repeating values | 1 |
| Dictionary | 20-60% | 150MB/s | Low cardinality | 2 |
| FOR | 30-50% | 100MB/s | Numeric | 3 |
| LZSS | 40-60% | 100MB/s | General | 4 |
| EnhancedDict | 30-50% | 100MB/s | High-card strings | 5 |
| DoubleDelta | 20-40% | 80MB/s | Sorted numeric | 6 |
| **Snappy** | **40-50%** | **250MB/s** | **Real-time** | **7** |
| **Brotli** | **30-45%** | **80MB/s** | **Archive** | **8** |
| **LZ4** | **45-55%** | **400MB/s** | **Medium-latency** | **9** |
| **Deflate** | **35-50%** | **100MB/s** | **Web** | **10** |
| **SpecializedDict** | **25-40%** | **100MB/s** | **Repetitive** | **11** |

---

## Expected Improvements

### Compression Ratio Gains

| Data Type | Old Best | New Best | Improvement |
|-----------|----------|----------|-------------|
| Real-time logs | 52% (LZSS) | 42% (Snappy) | **+10pp** |
| Archive data | 50.8% (Task 2) | 35% (Brotli) | **+15.8pp** |
| Sorted numeric | 50% (DD) | 35% (Brotli+DD) | **+15pp** |
| Text documents | 48% (Dict) | 28% (SpecDict) | **+20pp** |
| **Overall Average** | **50.8%** | **38-42%** | **+8.8-12.8pp** |

### Throughput by Use Case

| Use Case | Throughput | Codec |
|----------|-----------|-------|
| Real-time streaming | 250 MB/s | Snappy |
| High-speed cache | 400 MB/s | LZ4 |
| Standard processing | 125 MB/s | Brotli/LZSS |
| Web API | 100 MB/s | Deflate |
| Text analytics | 100 MB/s | SpecializedDict |

---

## Testing Strategy

### Unit Tests (per codec)
```bash
for codec in snappy brotli lz4 deflate specialized_dict; do
  cargo test --lib compression::${codec}_codec
done
```

### Integration Tests
```bash
# Test codec transitions
cargo test test_codec_fallback

# Test performance
cargo test --release benchmark_codecs

# Test data recovery
cargo test round_trip_all_codecs
```

### Benchmarking
```bash
# Compare all 12 codecs
cargo run --release --bin bench-all-codecs \
  --test-data /data/samples/*.kore

# Expected output:
# Codec 0-11: Compression ratio, speed, memory usage
```

---

## Rollout Strategy

### Phase 7a: Development & Testing
- Implement 5 codecs (~600 lines Rust)
- Unit & integration tests
- Benchmark validation

### Phase 7b: Soft Rollout
- Default to new Snappy for real-time
- Maintain backward compatibility (codecs 0-6)
- Monitor compression ratios

### Phase 7c: Full Rollout
- Enable all 12 codecs
- Auto-selection optimizer
- Performance tracking

---

## Summary

**New Codecs Added**: 5 (Snappy, Brotli, LZ4, Deflate, SpecializedDict)
**Total Codec Support**: 7 → 12
**Code Added**: ~600 lines Rust
**Expected Compression Improvement**: +8.8-12.8pp average
**Use Case Coverage**: Real-time, Archive, Cache, Web, Text Analytics

**Status**: Ready for implementation

---

**Next**: Phase 8 - Performance Scaling
