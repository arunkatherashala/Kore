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
            assert list(numbers_col.data) == [1, 2, 3, 4, 5]
            
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


class TestEncryptionAndStats:
    def test_encrypt_roundtrip(self):
        blob = kore.encrypt_aes256("pw", b"secret" * 50)
        assert kore.decrypt_aes256("pw", blob) == b"secret" * 50

    def test_decrypt_rejects_bad_input(self):
        blob = kore.encrypt_aes256("pw", b"secret")
        for pw, data in (("wrong", blob), ("pw", blob[:10]), ("pw", b"junk")):
            with pytest.raises(ValueError):
                kore.decrypt_aes256(pw, data)

    def test_column_stats(self):
        block = kore.DataBlock()
        block.add_column('x', kore.DataType.I64, [3, 1, None, 2])
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'stats.kore'
            kore.write_file(path, block)
            data = path.read_bytes()
        st = kore.get_column_stats(data, 'x')
        assert (st.min_value, st.max_value, st.null_count) == (1, 3, 1)
        assert list(kore.column_stats_from_bytes(data)) == ['x']


class TestStringEscaping:
    def test_special_characters_roundtrip(self):
        vals = ['plain', 'a"b', 'x\ny', 'back' + chr(92) + 'slash', 'caf\u00e9,\U0001f600', '', 'a","b']
        block = kore.DataBlock()
        block.add_column('s', kore.DataType.STR, vals)
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'esc.kore'
            kore.write_file(path, block)
            assert list(kore.read_file(path).get_column('s').data) == vals


class TestFloatNulls:
    def test_none_survives_roundtrip(self):
        block = kore.DataBlock()
        block.add_column('f', kore.DataType.F64, [1.5, None, float('nan'), 2.5])
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'f.kore'
            kore.write_file(path, block)
            got = list(kore.read_file(path).get_column('f').data)
        assert got[0] == 1.5 and got[1] is None and got[3] == 2.5
        assert got[2] is not None and got[2] != got[2]


class TestTimeTravel:
    def _block(self, vals):
        b = kore.DataBlock()
        b.add_column('n', kore.DataType.I64, vals)
        b.add_column('s', kore.DataType.STR, [f'r{v}' for v in vals])
        return b

    def test_read_at_version(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'log.kore'
            kore.append_version(path, self._block([1]), timestamp=100)
            kore.append_version(path, self._block([1, 2]), timestamp=200)
            kore.append_version(path, self._block([1, 2, 3]), timestamp=300)
            data = path.read_bytes()
        assert kore.list_versions(data) == [100, 200, 300]
        assert kore.read_at_version(data, 250).num_rows == 2
        assert list(kore.read_at_version(data, 300).get_column('s').data) == ['r1', 'r2', 'r3']
        with pytest.raises(ValueError):
            kore.read_at_version(data, 99)

    def test_rejects_non_increasing_timestamp(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'log.kore'
            kore.append_version(path, self._block([1]), timestamp=10)
            with pytest.raises(ValueError):
                kore.append_version(path, self._block([2]), timestamp=10)


class TestArrow:
    def test_to_arrow_with_nulls_and_zero_copy(self):
        pa = pytest.importorskip("pyarrow")
        block = kore.DataBlock()
        block.add_column('i', kore.DataType.I64, [1, None, 3])
        block.add_column('f', kore.DataType.F64, [1.5, 2.5, None])
        block.add_column('s', kore.DataType.STR, ['a', None, 'c'])
        t = kore.to_arrow(block)
        assert t.column('i').to_pylist() == [1, None, 3]
        assert t.column('f').to_pylist() == [1.5, 2.5, None]
        assert t.column('s').to_pylist() == ['a', None, 'c']
        import array
        b2 = kore.DataBlock()
        b2.add_column('x', kore.DataType.I64, array.array('q', [4, 5, 6]))
        assert kore.to_arrow(b2).column('x').to_pylist() == [4, 5, 6]


class TestArrowFileRead:
    def test_file_to_arrow_matches_read_file(self):
        pytest.importorskip("pyarrow")
        block = kore.DataBlock()
        block.add_column('i', kore.DataType.I64, [1, None, 3, 4])
        block.add_column('f', kore.DataType.F64, [1.5, 2.5, None, 4.5])
        block.add_column('s', kore.DataType.STR, ['a', 'b"q', None, 'a'])
        block.add_column('b', kore.DataType.BOOL, [True, False, None, True])
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'a.kore'
            kore.write_file(path, block)
            t = kore.to_arrow(path)
            ref = kore.read_file(path)
        for name in ('i', 'f', 'b'):
            assert t.column(name).to_pylist() == list(ref.get_column(name).data)
        assert t.column('s').to_pylist() == list(ref.get_column('s').data)
        assert t.column('b').to_pylist() == [True, False, None, True]


class TestShuffleCodec:
    def test_opt_in_shuffle_roundtrip_and_smaller(self, monkeypatch):
        import array, random
        random.seed(1)
        n = 20000
        block = kore.DataBlock()
        block.add_column('f', kore.DataType.F64, array.array('d', [random.uniform(1, 1000) for _ in range(n)]))
        block.add_column('q', kore.DataType.I64, array.array('q', [random.randint(1, 10000) for _ in range(n)]))
        with tempfile.TemporaryDirectory() as tmpdir:
            plain = Path(tmpdir) / 'plain.kore'
            shuf = Path(tmpdir) / 'shuf.kore'
            monkeypatch.delenv('KORE_SHUFFLE', raising=False)
            kore.write_file(plain, block)
            monkeypatch.setenv('KORE_SHUFFLE', '1')
            kore.write_file(shuf, block)
            monkeypatch.delenv('KORE_SHUFFLE', raising=False)
            assert shuf.stat().st_size < plain.stat().st_size
            got = kore.read_file(shuf)
        assert list(got.get_column('f').data) == list(block.get_column('f').data)
        assert list(got.get_column('q').data) == list(block.get_column('q').data)


class TestNativeLayout:
    FIXTURE = Path(__file__).parent / 'fixtures' / 'legacy_v3.kore'

    @staticmethod
    def _cols(block):
        return {c.name: list(c.data) for c in block.columns}

    def test_reads_files_written_by_the_legacy_layout(self):
        got = self._cols(kore.read_file(self.FIXTURE))
        assert got['i'] == [1, None, 3, 4, 5]
        # the legacy layout never stored string nulls: None came back as ''
        assert got['s'] == ['plain', 'a"b', 'x' + chr(10) + 'y', '', 'caf\u00e9,\U0001f600']
        assert got['b'] == [True, False, None, True, False]
        f = got['f']
        assert f[:3] == [1.5, 2.5, None] and f[3] != f[3] and f[4] == 5.5

    def test_legacy_writer_still_available(self, monkeypatch):
        block = kore.DataBlock()
        block.add_column('s', kore.DataType.STR, ['a', None, 'b'])
        monkeypatch.setenv('KORE_LEGACY_LAYOUT', '1')
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'l.kore'
            kore.write_file(path, block)
            assert b'StringDictJ' in path.read_bytes()[:400]
            assert list(kore.read_file(path).get_column('s').data) == ['a', '', 'b']

    def test_native_values_roundtrip(self):
        big = -(2 ** 63)
        block = kore.DataBlock()
        block.add_column('i', kore.DataType.I64, [big, None, 7, 2 ** 63 - 1])
        block.add_column('f', kore.DataType.F64, [None, float('nan'), -0.0, 1e308])
        block.add_column('b', kore.DataType.BOOL, [None, True, False, True])
        block.add_column('s', kore.DataType.STR, ['', None, 'x' * 1000, 'caf\u00e9\U0001f600'])
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'n.kore'
            kore.write_file(path, block)
            assert b'Layout: native-1' in path.read_bytes()[:400]
            got = self._cols(kore.read_file(path))
        assert got['i'] == [big, None, 7, 2 ** 63 - 1]
        assert got['f'][0] is None and got['f'][1] != got['f'][1] and got['f'][3] == 1e308
        assert got['b'] == [None, True, False, True]
        assert got['s'] == ['', None, 'x' * 1000, 'caf\u00e9\U0001f600']

    def test_empty_block_and_unique_strings(self):
        empty = kore.DataBlock()
        empty.add_column('s', kore.DataType.STR, [])
        empty.add_column('i', kore.DataType.I64, [])
        n = 5000
        uniq = kore.DataBlock()
        uniq.add_column('s', kore.DataType.STR, [f'user-{i:08d}@example.com' for i in range(n)])
        with tempfile.TemporaryDirectory() as tmpdir:
            p1, p2 = Path(tmpdir) / 'e.kore', Path(tmpdir) / 'u.kore'
            kore.write_file(p1, empty)
            kore.write_file(p2, uniq)
            assert kore.read_file(p1).num_rows == 0
            assert list(kore.read_file(p2).get_column('s').data)[4999] == 'user-00004999@example.com'
            header_len = int(p2.read_bytes()[13:23])
            assert header_len < 1000  # strings are in the binary section, not the text header

    def test_arrow_from_native(self):
        pa = pytest.importorskip("pyarrow")
        block = kore.DataBlock()
        block.add_column('i', kore.DataType.I64, [1, None, 3])
        block.add_column('f', kore.DataType.F64, [1.5, None, float('nan')])
        block.add_column('b', kore.DataType.BOOL, [True, None, False])
        block.add_column('s', kore.DataType.STR, ['a', None, 'caf\u00e9'])
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'a.kore'
            kore.write_file(path, block)
            t = kore.to_arrow(path)
        assert t.column('i').to_pylist() == [1, None, 3]
        assert t.column('b').to_pylist() == [True, None, False]
        assert t.column('s').to_pylist() == ['a', None, 'caf\u00e9']
        f = t.column('f').to_pylist()
        assert f[0] == 1.5 and f[1] is None and f[2] != f[2]
        assert str(t.schema.field('s').type) == 'string'


class TestStringDictionaryPath:
    @staticmethod
    def _roundtrip(values):
        block = kore.DataBlock()
        block.add_column('s', kore.DataType.STR, values)
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / 'd.kore'
            kore.write_file(path, block)
            plain = list(kore.read_file(path).get_column('s').data)
            pa = pytest.importorskip("pyarrow")
            arrow = kore.to_arrow(path).column('s').to_pylist()
        return plain, arrow

    def test_low_cardinality_with_nulls_and_empty(self):
        vals = [['US', None, '', 'EU', 'caf\u00e9'][i % 5] for i in range(3000)]
        plain, arrow = self._roundtrip(vals)
        assert plain == vals and arrow == vals

    def test_dictionary_boundary_254_vs_255_distinct(self):
        for distinct in (254, 255, 256):
            vals = [f'v{i % distinct}' for i in range(2000)]
            plain, arrow = self._roundtrip(vals)
            assert plain == vals and arrow == vals, distinct

    def test_all_null_and_non_string_values(self):
        plain, arrow = self._roundtrip([None, None, None])
        assert plain == [None] * 3 and arrow == [None] * 3
        plain, arrow = self._roundtrip([1, 'a', None, 2.5])
        assert plain == ['1', 'a', None, '2.5'] and arrow == plain
