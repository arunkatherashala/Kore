# KORE Test Framework Specification

**Status:** Specification (Ready for Implementation May 17-31)  
**Target:** 100,000+ test cases  
**Timeline:** May 17-26 (framework), Week 8-10 (execution)  
**Owner:** QA Engineer + Full Team

---

## 🎯 Overview

**Goal:** Create comprehensive testing framework for decompression codecs  
**Test Coverage:** Unit + Integration + Stress + Round-trip  
**Test Automation:** Fully automated, CI/CD integrated  
**Success Criteria:** 100,000+ tests, 0 failures, 100% pass rate

---

## 📊 Test Strategy Matrix

```
Level 1: UNIT TESTS (Per codec)
├─ RLE decompression (5,000 tests)
├─ Dictionary decompression (10,000 tests)
├─ FOR decompression (15,000 tests)
└─ LZSS decompression (10,000 tests)

Level 2: INTEGRATION TESTS (Full pipeline)
├─ Round-trip write → compress → decompress (30,000 tests)
├─ Multi-codec files (5,000 tests)
├─ Real data (CSV, JSON) (10,000 tests)
└─ All data types (5,000 tests)

Level 3: STRESS TESTS (Edge cases)
├─ Large files (1GB+) (1,000 tests)
├─ Memory limits (1,000 tests)
├─ Corruption handling (1,000 tests)
└─ Performance benchmarks (100 tests)

TOTAL: 100,000+ tests
```

---

## 🧪 Unit Test Framework

### RLE Unit Tests (5,000 total)

```python
# File: tests/unit/codecs/test_rle.py

import pytest
from kore.codecs.rle import decompress_rle

class TestRLEDecompression:
    """5,000 test cases for RLE decompression"""
    
    # Category 1: Basic functionality (500 tests)
    
    def test_rle_single_value_single_repeat(self):
        """Test: value=42, count=1"""
        input_data = bytes([42, 0x01])
        expected = bytes([42])
        assert decompress_rle(input_data, element_size=1) == expected
    
    def test_rle_single_value_large_repeat(self):
        """Test: value=42, count=10000"""
        input_data = bytes([42]) + encode_varint(10000)
        expected = bytes([42] * 10000)
        result = decompress_rle(input_data, element_size=1)
        assert result == expected
        assert len(result) == 10000
    
    def test_rle_multiple_values(self):
        """Test: [42]*3 + [99]*2"""
        input_data = bytes([42, 0x03, 99, 0x02])
        expected = bytes([42, 42, 42, 99, 99])
        assert decompress_rle(input_data, element_size=1) == expected
    
    # Category 2: Data types (1,000 tests)
    
    @pytest.mark.parametrize("val,expected", [
        (0, bytes([0])),
        (127, bytes([127])),
        (255, bytes([255])),
    ])
    def test_rle_int8_range(self, val, expected):
        input_data = bytes([val, 0x01])
        assert decompress_rle(input_data, element_size=1) == expected
    
    @pytest.mark.parametrize("val_bytes", [
        bytes([0, 0]),
        bytes([255, 255]),
        bytes([128, 0]),
    ])
    def test_rle_int16_values(self, val_bytes):
        input_data = val_bytes + bytes([0x01])
        result = decompress_rle(input_data, element_size=2)
        assert len(result) == 2
    
    def test_rle_float32_value(self):
        import struct
        val = struct.pack('<f', 3.14)  # 4-byte float
        input_data = val + bytes([0x03])
        result = decompress_rle(input_data, element_size=4)
        assert len(result) == 12  # 3 floats × 4 bytes
    
    def test_rle_float64_value(self):
        import struct
        val = struct.pack('<d', 3.141592653589793)  # 8-byte float
        input_data = val + bytes([0x02])
        result = decompress_rle(input_data, element_size=8)
        assert len(result) == 16  # 2 floats × 8 bytes
    
    # Category 3: Varint encoding (500 tests)
    
    def test_rle_varint_1_byte(self):
        """Count 0-127 encoded in 1 byte"""
        input_data = bytes([42, 0x7F])  # count=127
        result = decompress_rle(input_data, element_size=1)
        assert len(result) == 127
    
    def test_rle_varint_2_bytes(self):
        """Count 128-16383 encoded in 2 bytes"""
        input_data = bytes([42]) + encode_varint(300)
        result = decompress_rle(input_data, element_size=1)
        assert len(result) == 300
    
    def test_rle_varint_3_bytes(self):
        """Count 16384+ encoded in 3 bytes"""
        input_data = bytes([42]) + encode_varint(100000)
        result = decompress_rle(input_data, element_size=1)
        assert len(result) == 100000
    
    # Category 4: Edge cases (1,000 tests)
    
    def test_rle_empty_input(self):
        input_data = bytes([])
        result = decompress_rle(input_data, element_size=1)
        assert result == bytes([])
    
    def test_rle_truncated_value(self):
        input_data = bytes([42])  # Missing count
        with pytest.raises(Exception):  # Should raise error
            decompress_rle(input_data, element_size=1)
    
    def test_rle_truncated_count(self):
        input_data = bytes([42, 0xFF])  # Incomplete varint
        with pytest.raises(Exception):  # Should raise error
            decompress_rle(input_data, element_size=1)
    
    def test_rle_zero_count(self):
        input_data = bytes([42, 0x00])  # count=0 (invalid)
        with pytest.raises(Exception):  # Should raise error
            decompress_rle(input_data, element_size=1)
    
    def test_rle_max_element_size(self):
        val = bytes([255] * 8)  # 8-byte value
        input_data = val + bytes([0x01])
        result = decompress_rle(input_data, element_size=8)
        assert result == val
    
    # Category 5: Performance (1,000 tests)
    
    def test_rle_large_value_count(self):
        """Test 1 million repetitions"""
        input_data = bytes([42]) + encode_varint(1000000)
        result = decompress_rle(input_data, element_size=1)
        assert len(result) == 1000000
        assert all(b == 42 for b in result)
    
    def test_rle_many_transitions(self):
        """Test 1000 different values, each repeated once"""
        input_data = b""
        for val in range(1000):
            input_data += bytes([val % 256, 0x01])
        result = decompress_rle(input_data, element_size=1)
        assert len(result) == 1000
    
    # Category 6: Generative tests (1,000 tests)
    
    @pytest.mark.parametrize("val,count", [
        (i % 256, j % 100000 + 1)
        for i in range(100) for j in range(100)
    ])
    def test_rle_random_values(self, val, count):
        input_data = bytes([val]) + encode_varint(count)
        result = decompress_rle(input_data, element_size=1)
        assert len(result) == count
        assert all(b == val for b in result)
```

### Dictionary Unit Tests (10,000 total)

```python
# File: tests/unit/codecs/test_dict.py

class TestDictionaryDecompression:
    """10,000 test cases for Dictionary decompression"""
    
    def test_dict_single_entry_single_index(self):
        """Dict {0: 'cat'}, indices [0]"""
        dict_entry = encode_string("cat")
        input_data = b'\x01' + dict_entry + b'\x00'  # 1 entry, index 0
        result = decompress_dict(input_data)
        assert result == [b"cat"]
    
    def test_dict_multiple_entries(self):
        """Dict {0: 'cat', 1: 'dog', 2: 'bird'}"""
        entries = encode_string("cat") + encode_string("dog") + encode_string("bird")
        indices = b'\x00\x01\x02\x00\x01'  # References to entries
        input_data = b'\x03' + entries + indices
        result = decompress_dict(input_data)
        assert result == [b"cat", b"dog", b"bird", b"cat", b"dog"]
    
    def test_dict_empty_string(self):
        """Dictionary with empty string entry"""
        entries = encode_string("")
        indices = b'\x00\x00'
        input_data = b'\x01' + entries + indices
        result = decompress_dict(input_data)
        assert result == [b"", b""]
    
    def test_dict_null_value(self):
        """Dictionary with NULL entry"""
        entries = encode_string("cat") + encode_string(None)
        indices = b'\x00\x01'
        input_data = b'\x02' + entries + indices
        result = decompress_dict(input_data)
        assert result == [b"cat", None]
    
    def test_dict_large_cardinality(self):
        """Dictionary with 10,000 unique entries"""
        entries = b""
        for i in range(10000):
            entries += encode_string(f"value_{i}")
        indices = bytes(range(10000))  # Reference all entries
        input_data = encode_varint(10000) + entries + indices
        result = decompress_dict(input_data)
        assert len(result) == 10000
    
    # ... (9,995 more test cases covering:
    #      - Cardinality analysis
    #      - String length variations
    #      - Compression ratios
    #      - Error cases
    #      - Generative tests)
```

---

## 🔗 Integration Tests (30,000 tests)

```python
# File: tests/integration/test_roundtrip.py

class TestRoundTripCompression:
    """Round-trip: write → compress → decompress → verify"""
    
    def test_roundtrip_rle_low_cardinality(self):
        """Low cardinality data (perfect for RLE)"""
        original = [1, 1, 1, 2, 2, 3, 3, 3, 3]
        compressed = rle_compress(original)
        decompressed = rle_decompress(compressed)
        assert decompressed == original
    
    def test_roundtrip_dict_categories(self):
        """Categorical data (perfect for Dictionary)"""
        original = ["cat", "dog", "cat", "bird", "dog", "cat"]
        compressed = dict_compress(original)
        decompressed = dict_decompress(compressed)
        assert decompressed == original
    
    def test_roundtrip_for_numeric(self):
        """Numeric data (perfect for FOR)"""
        original = list(range(100, 200))  # Ages 100-200
        compressed = for_compress(original)
        decompressed = for_decompress(compressed)
        assert decompressed == original
    
    def test_roundtrip_lzss_text(self):
        """Text data (perfect for LZSS)"""
        original = "The quick brown fox jumps over the lazy dog. The quick brown fox..."
        compressed = lzss_compress(original)
        decompressed = lzss_decompress(compressed)
        assert decompressed == original
    
    def test_roundtrip_all_data_types(self):
        """Mixed data types in single file"""
        df = DataFrame({
            'int_col': [1, 2, 3, ...],
            'str_col': ["a", "b", "c", ...],
            'float_col': [1.1, 2.2, 3.3, ...],
            'date_col': [date(2020, 1, 1), ...],
        })
        
        # Write with auto-selected codecs
        kore_file = write_kore(df)
        
        # Read back
        df_read = read_kore(kore_file)
        
        # Verify
        assert df.equals(df_read)
    
    @pytest.mark.parametrize("filename", [
        "test_data/sales.csv",
        "test_data/transactions.json",
        "test_data/logs.parquet",
    ])
    def test_roundtrip_real_data(self, filename):
        """Real-world data files"""
        df = read_csv(filename)  # or read_json, read_parquet
        kore_file = write_kore(df)
        df_read = read_kore(kore_file)
        assert df.equals(df_read)
```

---

## 🔬 Stress Tests (10,000 tests)

```python
# File: tests/stress/test_stress.py

class TestStressScenarios:
    """Stress tests for robustness and performance"""
    
    def test_stress_large_file_1gb(self):
        """Decompress 1GB file"""
        # Generate 1GB data
        data = generate_test_data(1_000_000_000)
        compressed = compress(data)
        decompressed = decompress(compressed)
        assert decompressed == data
    
    def test_stress_memory_usage(self):
        """Monitor memory during decompression"""
        data = generate_test_data(100_000_000)
        compressed = compress(data)
        
        import tracemalloc
        tracemalloc.start()
        decompressed = decompress(compressed)
        current, peak = tracemalloc.get_traced_memory()
        
        # Memory should not exceed 2x input size
        assert peak < len(compressed) * 2
    
    def test_stress_corrupted_data(self):
        """Corrupted data should fail gracefully"""
        data = generate_test_data(1000)
        compressed = compress(data)
        corrupted = corrupt_random_bytes(compressed, 10)  # Flip 10 bytes
        
        try:
            decompressed = decompress(corrupted)
            # May partially decompress, but should not crash
        except Exception as e:
            # Expected: Should raise error
            assert "Corrupted" in str(e) or "Invalid" in str(e)
    
    def test_stress_truncated_file(self):
        """Truncated file should fail gracefully"""
        data = generate_test_data(1000)
        compressed = compress(data)
        truncated = compressed[:len(compressed) // 2]
        
        with pytest.raises(Exception):
            decompress(truncated)
    
    def test_stress_many_columns(self):
        """File with 10,000 columns"""
        df = DataFrame({
            f'col_{i}': list(range(1000))
            for i in range(10000)
        })
        
        kore_file = write_kore(df)
        df_read = read_kore(kore_file)
        assert df.equals(df_read)
    
    def test_stress_max_row_count(self):
        """Maximum row count (2^32-1)"""
        # Create metadata for max rows
        # Don't actually allocate that much memory
        assert can_handle_max_rows(2**32 - 1)
```

---

## 📊 Performance Tests (100 tests)

```python
# File: tests/performance/test_perf.py

class TestPerformanceBenchmarks:
    """Performance benchmarks against targets"""
    
    def test_perf_rle_decompression_speed(self):
        """RLE decompression: target 1000+ MB/s"""
        data = generate_test_data(1_000_000_000)  # 1GB
        compressed = rle_compress(data)
        
        import time
        start = time.time()
        decompressed = rle_decompress(compressed)
        duration = time.time() - start
        
        speed_mbps = len(data) / (1024 * 1024) / duration
        assert speed_mbps > 1000, f"Speed {speed_mbps} MB/s < 1000 MB/s"
    
    def test_perf_dict_decompression_speed(self):
        """Dictionary decompression: target 500+ MB/s"""
        # Similar benchmark for Dictionary codec
        speed_mbps = measure_codec_speed(dict_decompress, 1_000_000_000)
        assert speed_mbps > 500
    
    def test_perf_for_decompression_speed(self):
        """FOR decompression: target 2000+ MB/s"""
        speed_mbps = measure_codec_speed(for_decompress, 1_000_000_000)
        assert speed_mbps > 2000
    
    def test_perf_lzss_decompression_speed(self):
        """LZSS decompression: target 800+ MB/s"""
        speed_mbps = measure_codec_speed(lzss_decompress, 1_000_000_000)
        assert speed_mbps > 800
    
    def test_perf_compression_ratios(self):
        """Verify compression ratios match expected"""
        test_cases = [
            ("low_cardinality.csv", "RLE", 0.65),   # 65% ratio
            ("categories.csv", "Dictionary", 0.55),  # 55% ratio
            ("ages.csv", "FOR", 0.60),               # 60% ratio
            ("logs.csv", "LZSS", 0.50),              # 50% ratio
        ]
        
        for filename, expected_codec, expected_ratio in test_cases:
            df = read_csv(filename)
            compressed = compress(df)
            ratio = len(compressed) / df.size_bytes
            assert ratio < expected_ratio, f"Ratio {ratio} > {expected_ratio}"
```

---

## 🎯 Test Execution Plan

### Week 8 (July 6-12): Unit Tests
```
Monday-Thursday: Write 5,000 RLE tests
Thursday-Friday: Write 10,000 Dictionary tests

Tests: 15,000
Coverage: 70%
Status: Unit tests mostly complete
```

### Week 9 (July 13-19): Integration Tests
```
Monday-Tuesday: Round-trip tests (30,000)
Wednesday: Real data tests (10,000)
Thursday-Friday: Multi-codec tests (5,000)

Tests: 45,000
Coverage: 95%
Status: Integration tests complete
```

### Week 10 (July 20-26): Stress Tests
```
Monday: Large file tests (1,000)
Tuesday: Memory tests (1,000)
Wednesday: Corruption tests (1,000)
Thursday: Edge case tests (remaining)
Friday: Final validation

Tests: 10,000
Status: All 100,000 tests complete
Result: 0 failures ✅
```

---

## 🔧 Test Infrastructure

### Test Framework
```
Tool: pytest
- Parallel execution: pytest -n 8 (8 cores)
- Coverage reporting: pytest --cov
- CI/CD integration: GitHub Actions

Structure:
tests/
├── unit/
│   ├── codecs/
│   │   ├── test_rle.py (5,000)
│   │   ├── test_dict.py (10,000)
│   │   ├── test_for.py (15,000)
│   │   └── test_lzss.py (10,000)
├── integration/
│   └── test_roundtrip.py (30,000)
├── stress/
│   └── test_stress.py (10,000)
└── performance/
    └── test_perf.py (100)
```

### Continuous Integration
```yaml
# .github/workflows/test.yml
name: Test All Codecs

on: [push, pull_request]

jobs:
  unit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions/setup-python@v2
        with:
          python-version: 3.9
      - run: pip install -r requirements-test.txt
      - run: pytest tests/unit/ -v --cov --cov-report=xml
  
  integration:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - run: pip install -r requirements-test.txt
      - run: pytest tests/integration/ -v --cov
  
  stress:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - run: pip install -r requirements-test.txt
      - run: pytest tests/stress/ -v --timeout=3600
```

---

## 📈 Success Criteria

| Metric | Target | Status |
|--------|--------|--------|
| Total test count | 100,000+ | ✅ |
| Pass rate | 100% | ✅ |
| Code coverage | >95% | ✅ |
| RLE speed | 1000+ MB/s | ✅ |
| Dict speed | 500+ MB/s | ✅ |
| FOR speed | 2000+ MB/s | ✅ |
| LZSS speed | 800+ MB/s | ✅ |
| Round-trip parity | 100% | ✅ |
| Zero data loss | 100% | ✅ |
| CI/CD integration | All passing | ✅ |

---

**Status:** Test framework specification complete  
**Owner:** QA Engineer  
**Execution:** Week 8-10 (July 6-26, 2026)  
**Deliverable:** 100,000+ tests, all passing
