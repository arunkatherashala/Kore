"""
KORE Python FFI Integration Tests
===================================

Tests for Python ctypes FFI wrapper around Rust kore-ffi library.
"""

import pytest
import tempfile
from pathlib import Path

import kore_fileformat as kore


class TestDataTypes:
    """Test data type enum values."""

    def test_data_type_values(self):
        """Verify DataType enum matches Rust DType codes."""
        assert kore.DataType.I64 == 1
        assert kore.DataType.F64 == 2
        assert kore.DataType.BOOL == 3
        assert kore.DataType.STR == 4
        assert kore.DataType.STR_DICT == 5
        assert kore.DataType.ARRAY == 6
        assert kore.DataType.STRUCT == 7

    def test_compression_values(self):
        """Verify Compression enum matches Rust Compression codes."""
        assert kore.Compression.RAW == 0
        assert kore.Compression.RLE == 1
        assert kore.Compression.DELTA == 2
        assert kore.Compression.DICT == 3
        assert kore.Compression.NAN_RAW == 4
        assert kore.Compression.DEFLATE == 5
        assert kore.Compression.ZSTD == 6


class TestDataBlock:
    """Test DataBlock construction and operations."""

    def test_create_empty_block(self):
        """Create empty data block."""
        block = kore.DataBlock()
        assert block.num_rows == 0
        assert block.num_columns == 0
        assert block.columns == []

    def test_add_column(self):
        """Add column to data block."""
        block = kore.DataBlock()
        block.add_column('numbers', kore.DataType.I64, [1, 2, 3, 4, 5])
        
        assert block.num_rows == 5
        assert block.num_columns == 1
        assert block.get_column('numbers') is not None

    def test_add_multiple_columns(self):
        """Add multiple columns with same row count."""
        block = kore.DataBlock()
        block.add_column('numbers', kore.DataType.I64, [1, 2, 3])
        block.add_column('names', kore.DataType.STR, ['a', 'b', 'c'])
        
        assert block.num_rows == 3
        assert block.num_columns == 2

    def test_add_column_mismatched_rows(self):
        """Adding column with wrong row count raises error."""
        block = kore.DataBlock()
        block.add_column('numbers', kore.DataType.I64, [1, 2, 3])
        
        with pytest.raises(ValueError):
            block.add_column('names', kore.DataType.STR, ['a', 'b'])  # 2 rows != 3

    def test_get_column(self):
        """Get column by name."""
        block = kore.DataBlock()
        block.add_column('test', kore.DataType.I64, [10, 20, 30])
        
        col = block.get_column('test')
        assert col is not None
        assert col.name == 'test'
        assert col.dtype == kore.DataType.I64
        assert col.data == [10, 20, 30]

    def test_get_column_not_found(self):
        """Get non-existent column returns None."""
        block = kore.DataBlock()
        block.add_column('test', kore.DataType.I64, [1, 2, 3])
        
        assert block.get_column('nonexistent') is None


class TestColumnStats:
    """Test column statistics."""

    def test_stats_creation(self):
        """Create column statistics."""
        stats = kore.ColumnStats(
            min_value=1,
            max_value=100,
            null_count=0,
            cardinality=50,
            crc32=0xdeadbeef,
        )
        
        assert stats.min_value == 1
        assert stats.max_value == 100
        assert stats.null_count == 0
        assert stats.cardinality == 50
        assert stats.crc32 == 0xdeadbeef


class TestRoundtrip:
    """Test read/write roundtrip (Phase 3 placeholder)."""

    def test_write_read_roundtrip(self):
        """Write and read data block (Phase 3: via JSON placeholder)."""
        block = kore.DataBlock()
        block.add_column('numbers', kore.DataType.I64, [1, 2, 3, 4, 5])
        block.add_column('names', kore.DataType.STR, ['a', 'b', 'c', 'd', 'e'])
        
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'test.kore'
            
            # Write
            kore.write_file(path, block)
            assert path.exists()
            
            # Read
            restored = kore.read_file(path)
            
            assert restored.num_rows == 5
            assert restored.num_columns == 2
            
            # Verify columns
            numbers_col = restored.get_column('numbers')
            assert numbers_col is not None
            assert numbers_col.data == [1, 2, 3, 4, 5]
            
            names_col = restored.get_column('names')
            assert names_col is not None
            assert names_col.data == ['a', 'b', 'c', 'd', 'e']

    def test_write_read_float_column(self):
        """Write and read float column."""
        block = kore.DataBlock()
        block.add_column('decimals', kore.DataType.F64, [1.1, 2.2, 3.3])
        
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'test_float.kore'
            
            kore.write_file(path, block)
            restored = kore.read_file(path)
            
            col = restored.get_column('decimals')
            assert col is not None
            assert len(col.data) == 3


class TestCRC32:
    """Test CRC32 checksum (Phase 3: FFI wrapper)."""

    def test_crc32_basic(self):
        """Compute CRC32 of bytes (Phase 3: pending FFI)."""
        # Phase 3: Once kore-ffi is compiled, this will call Rust crc32()
        # For now: placeholder
        pass


class TestVersionControl:
    """Test MVCC and time travel APIs (Phase 3)."""

    def test_read_at_version(self):
        """Read data at specific timestamp (Phase 3: pending)."""
        # Phase 3: Implement once version snapshots integrated
        pass


class TestEncryption:
    """Test AES-256-GCM encryption (Phase 3)."""

    def test_encrypt_decrypt_roundtrip(self):
        """Encrypt and decrypt data (Phase 3: pending)."""
        # Phase 3: Implement once crypto FFI exposed
        pass


class TestBloomFilters:
    """Test Bloom filter APIs (Phase 3)."""

    def test_get_bloom_filter(self):
        """Retrieve Bloom filter for column (Phase 3: pending)."""
        # Phase 3: Implement once filter APIs exposed
        pass


if __name__ == '__main__':
    pytest.main([__file__, '-v'])
