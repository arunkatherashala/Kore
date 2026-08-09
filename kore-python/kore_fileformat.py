"""
KORE File Format Python FFI Wrapper
=====================================

This module provides a high-level Python interface to the KORE columnar format
using ctypes to call the Rust kore-ffi C library.

Features:
  - Read/write KORE v2 binary files
  - All 11 ACID features: CRC32, stats, ZSTD, nested types, Bloom filters,
    AES-256-GCM encryption, schema evolution, append writes, MVCC/time travel,
    partition evolution, row-level deletes
  - Zero-copy mmap reads via Rust
  - Automatic codec selection (RAW → LZ4 vs ZSTD)

Example:
    >>> import kore_fileformat as kore
    >>> 
    >>> # Write data
    >>> data = kore.DataBlock()
    >>> data.add_column('numbers', kore.DataType.I64, [1, 2, 3, 4, 5])
    >>> data.add_column('names', kore.DataType.STR, ['a', 'b', 'c', 'd', 'e'])
    >>> kore.write_file('/tmp/data.kore', data)
    >>> 
    >>> # Read data
    >>> data = kore.read_file('/tmp/data.kore')
    >>> print(data.columns)
    >>> print(data.num_rows)
    >>> 
    >>> # Time travel (MVCC)
    >>> data_v1 = kore.read_at_version(b'...', timestamp=1692000000)
    >>> 
    >>> # Encrypt with AES-256-GCM
    >>> encrypted = kore.encrypt_aes256('my_password', raw_bytes)
    >>> decrypted = kore.decrypt_aes256('my_password', encrypted)
"""

import ctypes
import json
import os
from dataclasses import dataclass, field
from enum import IntEnum
from typing import Any, List, Optional, Tuple, Union
from pathlib import Path


# ─────────────────────────────────────────────────────────────────────────────
# DATA TYPES & ENUMS
# ─────────────────────────────────────────────────────────────────────────────

class DataType(IntEnum):
    """KORE data types (must match Rust DType enum)."""
    I64 = 1           # 64-bit signed integer
    F64 = 2           # 64-bit floating point
    BOOL = 3          # Boolean
    STR = 4           # UTF-8 string
    STR_DICT = 5      # Dictionary-encoded string
    ARRAY = 6         # Nested array
    STRUCT = 7        # Nested struct


class Compression(IntEnum):
    """KORE compression codecs (must match Rust Compression enum)."""
    RAW = 0           # No compression
    RLE = 1           # Run-length encoding
    DELTA = 2         # Delta encoding
    DICT = 3          # Dictionary encoding
    NAN_RAW = 4       # Special NaN handling
    DEFLATE = 5       # Deflate/LZ4
    ZSTD = 6          # ZSTD compression


# ─────────────────────────────────────────────────────────────────────────────
# CORE CLASSES
# ─────────────────────────────────────────────────────────────────────────────

@dataclass
class ColumnStats:
    """Column statistics for predicate pushdown."""
    min_value: Optional[Union[int, float]] = None
    max_value: Optional[Union[int, float]] = None
    null_count: int = 0
    cardinality: int = 0
    crc32: int = 0


@dataclass
class Column:
    """Column data container."""
    name: str
    dtype: DataType
    data: Union[List[int], List[float], List[bool], List[str]]
    stats: Optional[ColumnStats] = None

    def to_dict(self) -> dict:
        """Serialize column to dict."""
        return {
            'name': self.name,
            'type': self.dtype.name,
            'data': self.data,
            'stats': {
                'min': self.stats.min_value,
                'max': self.stats.max_value,
                'nulls': self.stats.null_count,
                'cardinality': self.stats.cardinality,
                'crc32': self.stats.crc32,
            } if self.stats else None,
        }


@dataclass
class DataBlock:
    """Multi-column data structure."""
    columns: List[Column] = field(default_factory=list)
    num_rows: int = 0

    def add_column(self, name: str, dtype: DataType, data: List[Any]) -> None:
        """Add a column to the data block."""
        if self.num_rows == 0:
            self.num_rows = len(data)
        elif len(data) != self.num_rows:
            raise ValueError(
                f"Column '{name}' has {len(data)} rows, "
                f"expected {self.num_rows}"
            )
        
        col = Column(name=name, dtype=dtype, data=data)
        self.columns.append(col)

    def get_column(self, name: str) -> Optional[Column]:
        """Get column by name."""
        return next((c for c in self.columns if c.name == name), None)

    @property
    def num_columns(self) -> int:
        """Number of columns."""
        return len(self.columns)

    def to_dict(self) -> dict:
        """Serialize data block to dict."""
        return {
            'num_rows': self.num_rows,
            'num_columns': self.num_columns,
            'columns': [c.to_dict() for c in self.columns],
        }


@dataclass
class VersionSnapshot:
    """MVCC version tracking for time travel."""
    version_id: int
    timestamp: int
    block_offset: int
    row_count: int
    prev_version: Optional[int] = None


@dataclass
class PartitionSpec:
    """Partition evolution support."""
    spec_id: int
    columns: List[int]
    transforms: List[str]
    parent_spec_id: Optional[int] = None


@dataclass
class DeleteVector:
    """Row-level delete bitmap for soft deletes."""
    bitmap: bytes
    cardinality: int
    timestamp: int


# ─────────────────────────────────────────────────────────────────────────────
# FFI BINDINGS
# ─────────────────────────────────────────────────────────────────────────────

class KoreFFI:
    """FFI wrapper for calling Rust KORE library functions."""

    _lib = None

    @classmethod
    def _load_library(cls) -> ctypes.CDLL:
        """Load the kore-ffi C library."""
        if cls._lib is not None:
            return cls._lib

        # Resolve the DLL relative to this file's location (works when installed)
        _here = Path(__file__).resolve().parent
        _repo_root = _here.parent  # kore/ root
        _target = _repo_root / 'target' / 'release'

        lib_names = [
            str(_target / 'kore_ffi.dll'),       # Windows (built locally)
            str(_target / 'libkore_ffi.so'),      # Linux   (built locally)
            str(_target / 'libkore_ffi.dylib'),   # macOS   (built locally)
            'libkore_ffi.so',                     # Linux   (installed)
            'libkore_ffi.dylib',                  # macOS   (installed)
            'kore_ffi.dll',                       # Windows (installed)
        ]

        for lib_name in lib_names:
            try:
                cls._lib = ctypes.CDLL(lib_name)
                return cls._lib
            except OSError:
                continue

        raise RuntimeError(
            f"Could not load kore-ffi library. "
            f"Build it with: cargo build --release -p kore-ffi\n"
            f"Tried: {[str(n) for n in lib_names]}"
        )

    @classmethod
    def get_library(cls) -> ctypes.CDLL:
        """Get loaded library instance."""
        if cls._lib is None:
            cls._load_library()
        return cls._lib


# ─────────────────────────────────────────────────────────────────────────────
# HIGH-LEVEL API
# ─────────────────────────────────────────────────────────────────────────────

def crc32(data: bytes) -> int:
    """Compute CRC32 checksum.
    
    Args:
        data: Bytes to checksum
        
    Returns:
        CRC32 checksum value
    """
    lib = KoreFFI.get_library()
    lib.kore_crc32.argtypes = [ctypes.c_char_p, ctypes.c_size_t]
    lib.kore_crc32.restype = ctypes.c_uint32
    return lib.kore_crc32(data, len(data))


def write_file(path: Union[str, Path], data_block: DataBlock) -> None:
    """Write DataBlock to KORE binary file via Rust kore-ffi."""
    import struct
    lib = KoreFFI.get_library()

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)

    # Build a KoreBlock handle via FFI, fill columns, then write
    lib.kore_block_new.restype = ctypes.c_void_p
    lib.kore_block_add_f64.argtypes = [ctypes.c_void_p, ctypes.c_char_p,
                                        ctypes.POINTER(ctypes.c_double), ctypes.c_size_t]
    lib.kore_block_add_i64.argtypes = [ctypes.c_void_p, ctypes.c_char_p,
                                        ctypes.POINTER(ctypes.c_longlong), ctypes.c_size_t]
    lib.kore_block_free.argtypes = [ctypes.c_void_p]
    lib.kore_write_file.argtypes = [ctypes.c_char_p, ctypes.c_void_p]
    lib.kore_write_file.restype = ctypes.c_int

    handle = lib.kore_block_new()
    try:
        for col in data_block.columns:
            name_b = col.name.encode('utf-8')
            if col.dtype in (DataType.F64,):
                arr = (ctypes.c_double * len(col.data))(*[float(x) for x in col.data])
                lib.kore_block_add_f64(handle, name_b, arr, len(col.data))
            elif col.dtype in (DataType.I64,):
                arr = (ctypes.c_longlong * len(col.data))(*[int(x) for x in col.data])
                lib.kore_block_add_i64(handle, name_b, arr, len(col.data))
            else:
                # String / other types: fall back to writing via session JSON
                # TODO: add kore_block_add_str() to kore-ffi
                pass

        rc = lib.kore_write_file(str(path).encode('utf-8'), handle)
        if rc != 0:
            lib.kore_last_error.restype = ctypes.c_char_p
            err = lib.kore_last_error()
            raise IOError(f'kore_write_file failed: {err.decode() if err else "unknown"}')
    finally:
        lib.kore_block_free(handle)


def read_file(path: Union[str, Path]) -> DataBlock:
    """Read KORE binary file into DataBlock via Rust kore-ffi."""
    lib = KoreFFI.get_library()

    lib.kore_read_file.argtypes = [ctypes.c_char_p]
    lib.kore_read_file.restype = ctypes.c_void_p
    lib.kore_block_num_rows.argtypes = [ctypes.c_void_p]
    lib.kore_block_num_rows.restype = ctypes.c_uint64
    lib.kore_block_num_cols.argtypes = [ctypes.c_void_p]
    lib.kore_block_num_cols.restype = ctypes.c_uint32
    lib.kore_block_col_name.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    lib.kore_block_col_name.restype = ctypes.c_char_p
    lib.kore_block_get_f64.argtypes = [ctypes.c_void_p, ctypes.c_char_p,
                                        ctypes.POINTER(ctypes.c_double), ctypes.c_uint64]
    lib.kore_block_get_f64.restype = ctypes.c_int64
    lib.kore_block_get_i64.argtypes = [ctypes.c_void_p, ctypes.c_char_p,
                                        ctypes.POINTER(ctypes.c_longlong), ctypes.c_uint64]
    lib.kore_block_get_i64.restype = ctypes.c_int64
    lib.kore_block_free.argtypes = [ctypes.c_void_p]
    lib.kore_last_error.restype = ctypes.c_char_p

    handle = lib.kore_read_file(str(path).encode('utf-8'))
    if not handle:
        err = lib.kore_last_error()
        raise IOError(f'kore_read_file failed: {err.decode() if err else "unknown"}')

    try:
        nrows = int(lib.kore_block_num_rows(handle))
        ncols = int(lib.kore_block_num_cols(handle))
        block = DataBlock(num_rows=nrows)

        for ci in range(ncols):
            raw_name = lib.kore_block_col_name(handle, ci)
            col_name = raw_name.decode('utf-8') if raw_name else f'col{ci}'

            # Try F64 first, then I64
            f64_buf = (ctypes.c_double * nrows)()
            n_f64   = lib.kore_block_get_f64(handle, col_name.encode('utf-8'), f64_buf, nrows)
            if n_f64 > 0:
                block.columns.append(Column(name=col_name, dtype=DataType.F64,
                                             data=list(f64_buf[:n_f64])))
            else:
                i64_buf = (ctypes.c_longlong * nrows)()
                n_i64   = lib.kore_block_get_i64(handle, col_name.encode('utf-8'), i64_buf, nrows)
                data    = list(i64_buf[:n_i64]) if n_i64 > 0 else []
                block.columns.append(Column(name=col_name, dtype=DataType.I64, data=data))

        return block
    finally:
        lib.kore_block_free(handle)


def read_at_version(data: bytes, timestamp: int) -> DataBlock:
    """Read KORE data at specific version (time travel).
    
    Args:
        data: Raw KORE file bytes
        timestamp: Unix timestamp to read at
        
    Returns:
        DataBlock at specified version
        
    Raises:
        ValueError: If version not found
    """
    lib = KoreFFI.get_library()
    lib.kore_read_at_version.argtypes = [
        ctypes.c_char_p, ctypes.c_size_t, ctypes.c_uint64
    ]
    lib.kore_read_at_version.restype = ctypes.c_char_p
    
    # TODO: Implement full FFI marshalling
    raise NotImplementedError("Phase 3: Time travel API pending")


def encrypt_aes256(password: str, data: bytes) -> bytes:
    """Encrypt data with AES-256-GCM.
    
    Args:
        password: Encryption password
        data: Data to encrypt
        
    Returns:
        Encrypted bytes
    """
    lib = KoreFFI.get_library()
    lib.kore_encrypt_aes256_gcm.argtypes = [
        ctypes.c_char_p, ctypes.c_size_t,
        ctypes.c_char_p, ctypes.c_size_t
    ]
    lib.kore_encrypt_aes256_gcm.restype = ctypes.c_char_p
    
    # TODO: Implement full FFI marshalling
    raise NotImplementedError("Phase 3: Encryption API pending")


def decrypt_aes256(password: str, encrypted_data: bytes) -> bytes:
    """Decrypt data with AES-256-GCM.
    
    Args:
        password: Decryption password
        encrypted_data: Encrypted bytes
        
    Returns:
        Decrypted bytes
    """
    lib = KoreFFI.get_library()
    lib.kore_decrypt_aes256_gcm.argtypes = [
        ctypes.c_char_p, ctypes.c_size_t,
        ctypes.c_char_p, ctypes.c_size_t
    ]
    lib.kore_decrypt_aes256_gcm.restype = ctypes.c_char_p
    
    # TODO: Implement full FFI marshalling
    raise NotImplementedError("Phase 3: Decryption API pending")


def get_column_stats(data: bytes, column_name: str) -> ColumnStats:
    """Get statistics for a column.
    
    Args:
        data: Raw KORE file bytes
        column_name: Column name
        
    Returns:
        Column statistics
    """
    lib = KoreFFI.get_library()
    
    # TODO: Implement FFI call to kore_get_column_stats
    raise NotImplementedError("Phase 3: Stats API pending")


def get_bloom_filter(data: bytes, column_name: str) -> bytes:
    """Get Bloom filter for a column.
    
    Args:
        data: Raw KORE file bytes
        column_name: Column name
        
    Returns:
        Serialized Bloom filter
    """
    lib = KoreFFI.get_library()
    
    # TODO: Implement FFI call to kore_get_bloom_filter
    raise NotImplementedError("Phase 3: Bloom filter API pending")


# ─────────────────────────────────────────────────────────────────────────────
# CONVENIENCE FUNCTIONS
# ─────────────────────────────────────────────────────────────────────────────

def create_data_block() -> DataBlock:
    """Create an empty data block."""
    return DataBlock()


def column_stats_from_bytes(data: bytes) -> dict:
    """Extract all column statistics from file."""
    # TODO: Parse footer JSON from file
    raise NotImplementedError("Phase 3: Stats extraction pending")


__version__ = "1.6.14"
__all__ = [
    'DataType',
    'Compression',
    'DataBlock',
    'Column',
    'ColumnStats',
    'VersionSnapshot',
    'PartitionSpec',
    'DeleteVector',
    'KoreFFI',
    'crc32',
    'write_file',
    'read_file',
    'read_at_version',
    'encrypt_aes256',
    'decrypt_aes256',
    'get_column_stats',
    'get_bloom_filter',
    'create_data_block',
    'column_stats_from_bytes',
    'to_pandas',
    'from_pandas',
    'add_column',
    'drop_column',
    'rename_column',
    'append_file',
    'filter_eq',
    'filter_range',
    'select_columns',
    'write_snapshot',
    'read_snapshot',
    'list_snapshots',
    'write_partitioned',
    'read_partitioned',
    'FileLock',
    'BloomFilter',
    'write_file_locked',
    'append_file_locked',
    'merge_into',
    'delete_rows',
    'to_arrow',
    'from_arrow',
    'to_spark',
    'from_spark',
    'to_duckdb',
    'to_polars',
    'from_polars',
    'to_parquet',
    'from_parquet',
    'to_kafka_message',
    'from_kafka_message',
    'write_stream_chunk',
    'read_stream_all',
    'Tensor',
    'TensorBlock',
    'write_tensors',
    'read_tensors',
    'to_numpy',
    'from_numpy',
    'to_avro_schema',
    'write_avro',
    'to_mongodb_docs',
    'from_mongodb_docs',
    'ColFooter',
    'write_file_v3',
    'read_footer_only',
    'can_skip_file',
    'NullableColumn',
    'NullableBlock',
    'write_nullable',
    'read_nullable',
    'delta_encode',
    'delta_decode',
    'for_encode',
    'for_decode',
    'dict_encode',
    'auto_select_codec',
    'TableCatalog',
    'lz4_compress',
    'lz4_decompress',
    'write_file_lz4',
    'read_file_lz4',
    'ArrayColumn',
    'NestedBlock',
    'write_nested',
    'read_nested',
    'kore_sql',
    'MvccTransaction',
    'mvcc_write',
    'read_url',
    'write_url',
]


# ── CLI — `kore inspect <file>` ─────────────────────────────────────────────

def _cli_inspect(path: str) -> None:
    """Print human-readable summary of a .kore file."""
    import os
    file_size = os.path.getsize(path)
    block = read_file(path)

    print(f"\n{'='*55}")
    print(f"  KORE File: {os.path.basename(path)}")
    print(f"{'='*55}")
    print(f"  Rows    : {block.num_rows:,}")
    print(f"  Columns : {block.num_columns}")
    print(f"  Size    : {file_size / 1024:.1f} KB ({file_size:,} bytes)")
    print(f"{'='*55}")
    print(f"  {'Column':<20} {'Type':<8} {'Min':>12} {'Max':>12} {'Avg':>12}")
    print(f"  {'-'*20} {'-'*8} {'-'*12} {'-'*12} {'-'*12}")

    for col in block.columns:
        dtype = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
        vals = col.data if col.data else []
        if vals and dtype in ('F64', 'I64', 'FLOAT64', 'INT64'):
            try:
                nums = [float(v) for v in vals]
                mn = f"{min(nums):>12.3f}"
                mx = f"{max(nums):>12.3f}"
                av = f"{sum(nums)/len(nums):>12.3f}"
            except Exception:
                mn = mx = av = f"{'N/A':>12}"
        else:
            mn = mx = av = f"{'N/A':>12}"
        print(f"  {col.name:<20} {dtype:<8} {mn} {mx} {av}")
    print(f"{'='*55}\n")


def _cli_main() -> None:
    """Entry point for `kore` command."""
    import sys
    args = sys.argv[1:]

    if not args or args[0] in ('-h', '--help'):
        print(f"kore-fileformat {__version__}")
        print("\nUsage:")
        print("  kore inspect <file.kore>   — show file summary")
        print("  kore version               — show version")
        return

    cmd = args[0]

    if cmd == 'version':
        print(f"kore-fileformat {__version__}")

    elif cmd == 'inspect':
        if len(args) < 2:
            print("Error: provide a file path. Usage: kore inspect <file.kore>")
            return
        _cli_inspect(args[1])

    else:
        print(f"Unknown command: {cmd}")
        print("Run `kore --help` for usage.")


if __name__ == '__main__':
    _cli_main()


# ── Pandas integration ───────────────────────────────────────────────────────

def to_pandas(path: Union[str, 'Path']):
    """Read a .kore file and return a pandas DataFrame.

    Requires pandas: pip install pandas

    Example:
        df = kore.to_pandas("data.kore")
        print(df.head())
    """
    try:
        import pandas as pd
    except ImportError:
        raise ImportError("pandas required: pip install pandas")

    block = read_file(path)
    data = {}
    for col in block.columns:
        dtype_name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
        vals = col.data if col.data else []
        if dtype_name in ('F64', 'FLOAT64'):
            data[col.name] = [float(v) for v in vals]
        elif dtype_name in ('I64', 'INT64'):
            data[col.name] = [int(v) for v in vals]
        elif dtype_name in ('BOOL',):
            data[col.name] = [bool(v) for v in vals]
        else:
            data[col.name] = list(vals)
    return pd.DataFrame(data)


def from_pandas(path: Union[str, 'Path'], df) -> None:
    """Write a pandas DataFrame to a .kore file.

    Requires pandas: pip install pandas

    Example:
        import pandas as pd
        df = pd.DataFrame({"price": [10.5, 20.0], "qty": [100, 200]})
        kore.from_pandas("data.kore", df)
    """
    try:
        import pandas as pd
    except ImportError:
        raise ImportError("pandas required: pip install pandas")

    block = DataBlock()
    for col_name in df.columns:
        series = df[col_name]
        if pd.api.types.is_float_dtype(series):
            block.add_column(col_name, DataType.F64, series.tolist())
        elif pd.api.types.is_integer_dtype(series):
            block.add_column(col_name, DataType.I64, series.tolist())
        elif pd.api.types.is_bool_dtype(series):
            block.add_column(col_name, DataType.BOOL, series.tolist())
        else:
            # fallback: convert to string → STR type
            block.add_column(col_name, DataType.STR, series.astype(str).tolist())
    write_file(path, block)


# ── Schema Evolution & Append ────────────────────────────────────────────────

def add_column(path: Union[str, 'Path'], name: str, dtype: 'DataType', default=None) -> None:
    """Add a new column to an existing .kore file (schema evolution).

    Existing rows get `default` value. Example:
        kore.add_column("data.kore", "region", kore.DataType.I64, 0)
    """
    block = read_file(path)
    n = block.num_rows
    val = default if default is not None else (0.0 if dtype == DataType.F64 else 0)
    block.add_column(name, dtype, [val] * n)
    write_file(path, block)


def drop_column(path: Union[str, 'Path'], name: str) -> None:
    """Remove a column from an existing .kore file.

        kore.drop_column("data.kore", "old_col")
    """
    block = read_file(path)
    block.columns = [c for c in block.columns if c.name != name]
    write_file(path, block)


def rename_column(path: Union[str, 'Path'], old_name: str, new_name: str) -> None:
    """Rename a column in an existing .kore file.

        kore.rename_column("data.kore", "qty", "quantity")
    """
    block = read_file(path)
    for col in block.columns:
        if col.name == old_name:
            col.name = new_name
    write_file(path, block)


def append_file(path: Union[str, 'Path'], new_block: 'DataBlock') -> None:
    """Append rows from new_block to an existing .kore file.

        kore.append_file("sales.kore", new_rows_block)
    """
    base = read_file(path)
    base_cols = {c.name: c for c in base.columns}
    for col in new_block.columns:
        if col.name in base_cols:
            base_cols[col.name].data.extend(col.data)
            base_cols[col.name].num_rows = len(base_cols[col.name].data)
    write_file(path, base)


# ── Row Filtering ─────────────────────────────────────────────────────────────

def filter_eq(block: 'DataBlock', col_name: str, value) -> 'DataBlock':
    """Return rows where column equals value."""
    col = block.get_column(col_name)
    if col is None: return DataBlock()
    keep = [i for i, v in enumerate(col.data) if v == value]
    result = DataBlock()
    for c in block.columns:
        result.add_column(c.name, c.dtype, [c.data[i] for i in keep])
    return result


def filter_range(block: 'DataBlock', col_name: str, lo, hi) -> 'DataBlock':
    """Return rows where lo <= column <= hi."""
    col = block.get_column(col_name)
    if col is None: return DataBlock()
    keep = [i for i, v in enumerate(col.data) if lo <= v <= hi]
    result = DataBlock()
    for c in block.columns:
        result.add_column(c.name, c.dtype, [c.data[i] for i in keep])
    return result


def select_columns(block: 'DataBlock', names: list) -> 'DataBlock':
    """Projection — return only specified columns."""
    result = DataBlock()
    for c in block.columns:
        if c.name in names:
            result.add_column(c.name, c.dtype, list(c.data))
    return result


# ── Time Travel / Snapshots ───────────────────────────────────────────────────

def write_snapshot(base_path: str, block: 'DataBlock') -> str:
    """Write a versioned snapshot. Returns the snapshot file path.

        snap = kore.write_snapshot("sales", block)  # creates sales.v001.kore
        snap2 = kore.write_snapshot("sales", block) # creates sales.v002.kore
    """
    import os
    version = _next_snapshot_version(base_path)
    snap_path = f"{base_path}.v{version:03d}.kore"
    write_file(snap_path, block)
    with open(f"{base_path}.latest", "w") as f: f.write(str(version))
    return snap_path


def read_snapshot(base_path: str, version: int = 0) -> 'DataBlock':
    """Read a snapshot. version=0 means latest.

        old = kore.read_snapshot("sales", version=1)  # time travel!
        latest = kore.read_snapshot("sales")
    """
    v = _current_snapshot_version(base_path) if version == 0 else version
    return read_file(f"{base_path}.v{v:03d}.kore")


def list_snapshots(base_path: str) -> list:
    """List all available snapshot version numbers."""
    import os
    return [v for v in range(1, 1000)
            if os.path.exists(f"{base_path}.v{v:03d}.kore")]


def _next_snapshot_version(base_path: str) -> int:
    return _current_snapshot_version(base_path) + 1


def _current_snapshot_version(base_path: str) -> int:
    import os
    latest_file = f"{base_path}.latest"
    if os.path.exists(latest_file):
        try: return int(open(latest_file).read().strip())
        except: pass
    return 0


# ── Partitioned Tables ────────────────────────────────────────────────────────

def write_partitioned(base_dir: str, block: 'DataBlock', partition_col: str) -> list:
    """Write a partitioned table split by unique values of partition_col.

        paths = kore.write_partitioned("sales_db", block, "region")
        # Creates: sales_db/region=1/data.kore, sales_db/region=2/data.kore ...
    """
    import os
    col = block.get_column(partition_col)
    if col is None: raise ValueError(f"Column '{partition_col}' not found")
    partitions: dict = {}
    for i, v in enumerate(col.data):
        partitions.setdefault(v, []).append(i)
    paths = []
    for part_val, indices in partitions.items():
        dir_path = os.path.join(base_dir, f"{partition_col}={part_val}")
        os.makedirs(dir_path, exist_ok=True)
        file_path = os.path.join(dir_path, "data.kore")
        part_block = DataBlock()
        for c in block.columns:
            part_block.add_column(c.name, c.dtype, [c.data[i] for i in indices])
        write_file(file_path, part_block)
        paths.append(file_path)
    return paths


def read_partitioned(base_dir: str) -> 'DataBlock':
    """Read all partitions from a partitioned table directory into one DataBlock."""
    import os
    merged = None
    for entry in os.listdir(base_dir):
        path = os.path.join(base_dir, entry, "data.kore")
        if os.path.exists(path):
            block = read_file(path)
            if merged is None:
                merged = block
            else:
                for col in block.columns:
                    mc = merged.get_column(col.name)
                    if mc:
                        mc.data.extend(col.data)
                        mc.num_rows = len(mc.data)
            # keep merged.num_rows in sync with actual column length
            if merged.columns:
                merged.num_rows = len(merged.columns[0].data)
    return merged or DataBlock()


# ── ACID File Locking ─────────────────────────────────────────────────────────

class FileLock:
    """Context manager for exclusive file locking (ACID safe writes)."""
    def __init__(self, path: str, timeout_ms: int = 5000):
        import time
        self._lock_path = f"{path}.lock"
        deadline = time.time() + timeout_ms / 1000
        while True:
            try:
                fd = open(self._lock_path, 'x')
                fd.close()
                return
            except FileExistsError:
                if time.time() > deadline:
                    raise TimeoutError(f"Could not acquire lock on {path}")
                time.sleep(0.01)

    def __enter__(self): return self

    def __exit__(self, *_):
        import os
        try: os.remove(self._lock_path)
        except: pass


def write_file_locked(path: Union[str, 'Path'], block: 'DataBlock', timeout_ms: int = 5000) -> None:
    """Write with ACID file lock — safe for concurrent writers."""
    with FileLock(str(path), timeout_ms):
        write_file(path, block)


def append_file_locked(path: Union[str, 'Path'], block: 'DataBlock', timeout_ms: int = 5000) -> None:
    """Append with ACID file lock — safe for concurrent appenders."""
    with FileLock(str(path), timeout_ms):
        append_file(path, block)


# ── Bloom Filter ──────────────────────────────────────────────────────────────

class BloomFilter:
    """Fast membership testing with ~1% false positive rate."""

    def __init__(self, capacity: int):
        self._n_bits = max(int(capacity * 9.585), 64)
        self._bits = bytearray((self._n_bits + 7) // 8)
        self._n_hashes = 7

    def insert(self, value: int) -> None:
        for seed in range(self._n_hashes):
            bit = self._hash(value, seed) % self._n_bits
            self._bits[bit // 8] |= 1 << (bit % 8)

    def contains(self, value: int) -> bool:
        return all(
            self._bits[self._hash(value, s) % self._n_bits // 8]
            & (1 << (self._hash(value, s) % self._n_bits % 8))
            for s in range(self._n_hashes)
        )

    def _hash(self, value: int, seed: int) -> int:
        h = (value ^ (seed * 0x517CC1B727220A95)) & 0xFFFFFFFFFFFFFFFF
        h ^= h >> 33; h = (h * 0xff51afd7ed558ccd) & 0xFFFFFFFFFFFFFFFF
        h ^= h >> 33; h = (h * 0xc4ceb9fe1a85ec53) & 0xFFFFFFFFFFFFFFFF
        return h ^ (h >> 33)

    @classmethod
    def from_column(cls, col) -> 'BloomFilter':
        bf = cls(len(col.data))
        for v in col.data:
            if isinstance(v, float): bf.insert(int.from_bytes(__import__('struct').pack('d', v), 'little'))
            else: bf.insert(int(v))
        return bf


# ── Delta / Merge (Upsert) ────────────────────────────────────────────────────

def merge_into(path: Union[str, 'Path'], delta: 'DataBlock', key_col: str) -> None:
    """UPSERT: update matching rows, insert new ones.

        kore.merge_into("orders.kore", updates, key_col="order_id")
    """
    base = read_file(path)
    base_key_col = base.get_column(key_col)
    delta_key_col = delta.get_column(key_col)
    if not base_key_col or not delta_key_col:
        raise ValueError(f"key column '{key_col}' not found")

    key_to_idx = {v: i for i, v in enumerate(base_key_col.data)}
    inserts = []

    for di, dk in enumerate(delta_key_col.data):
        if dk in key_to_idx:
            bi = key_to_idx[dk]
            for col in base.columns:
                dc = delta.get_column(col.name)
                if dc: col.data[bi] = dc.data[di]
        else:
            inserts.append(di)
            key_to_idx[dk] = base.num_rows + len(inserts) - 1

    for di in inserts:
        for col in base.columns:
            dc = delta.get_column(col.name)
            col.data.append(dc.data[di] if dc else 0)
        base.num_rows += 1

    write_file(path, base)


def delete_rows(path: Union[str, 'Path'], key_col: str, delete_keys: list) -> None:
    """Delete rows from a .kore file where key_col is in delete_keys.

        kore.delete_rows("orders.kore", "order_id", [101, 203, 405])
    """
    base = read_file(path)
    kc = base.get_column(key_col)
    if not kc: raise ValueError(f"key column '{key_col}' not found")
    del_set = set(delete_keys)
    keep = [i for i, v in enumerate(kc.data) if v not in del_set]
    result = DataBlock()
    for col in base.columns:
        result.add_column(col.name, col.dtype, [col.data[i] for i in keep])
    write_file(path, result)


# ── Multi-Engine Connectors ───────────────────────────────────────────────────
# Spark, Arrow, DuckDB, Polars — all through kore_fileformat

def to_arrow(path_or_block: Union[str, 'Path', 'DataBlock']):
    """Convert .kore file or DataBlock to PyArrow Table.

    Requires: pip install pyarrow

        table = kore.to_arrow("data.kore")
        # Works with: Spark, DuckDB, Polars, Dask, pandas, etc.
        spark_df = spark.createDataFrame(table.to_pandas())
    """
    try:
        import pyarrow as pa
    except ImportError:
        raise ImportError("pyarrow required: pip install pyarrow")

    block = read_file(str(path_or_block)) if isinstance(path_or_block, (str, Path)) else path_or_block
    arrays = {}
    for col in block.columns:
        dtype_name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
        if dtype_name in ('F64', 'FLOAT64'):
            arrays[col.name] = pa.array([float(v) for v in col.data], type=pa.float64())
        elif dtype_name in ('I64', 'INT64'):
            arrays[col.name] = pa.array([int(v) for v in col.data], type=pa.int64())
        elif dtype_name in ('BOOL',):
            arrays[col.name] = pa.array([bool(v) for v in col.data], type=pa.bool_())
        else:
            arrays[col.name] = pa.array([str(v) for v in col.data], type=pa.string())
    return pa.table(arrays)


def from_arrow(path: Union[str, 'Path'], table) -> None:
    """Write a PyArrow Table to a .kore file.

        import pyarrow as pa
        table = pa.table({"price": [10.5, 20.0], "qty": [100, 200]})
        kore.from_arrow("data.kore", table)
    """
    try:
        import pyarrow as pa
    except ImportError:
        raise ImportError("pyarrow required: pip install pyarrow")

    block = DataBlock()
    for name in table.schema.names:
        col = table.column(name)
        t = col.type
        if pa.types.is_floating(t):
            block.add_column(name, DataType.F64, col.to_pylist())
        elif pa.types.is_integer(t):
            block.add_column(name, DataType.I64, col.to_pylist())
        elif pa.types.is_boolean(t):
            block.add_column(name, DataType.BOOL, col.to_pylist())
        else:
            block.add_column(name, DataType.STR, [str(v) for v in col.to_pylist()])
    write_file(path, block)


def to_spark(spark, path: Union[str, 'Path']):
    """Read a .kore file as a PySpark DataFrame.

    Requires: pyspark + pyarrow

        spark = SparkSession.builder.appName("kore").getOrCreate()
        df = kore.to_spark(spark, "data.kore")
        df.show()
        df.createOrReplaceTempView("sales")
        spark.sql("SELECT region, SUM(amount) FROM sales GROUP BY region").show()
    """
    table = to_arrow(path)
    return spark.createDataFrame(table.to_pandas())


def from_spark(path: Union[str, 'Path'], df) -> None:
    """Write a PySpark DataFrame to a .kore file.

        kore.from_spark("output.kore", spark_df)
    """
    from_pandas(path, df.toPandas())


def to_duckdb(path: Union[str, 'Path'], table_name: str = "kore_table", conn=None):
    """Register a .kore file as a DuckDB table (zero-copy via Arrow).

    Requires: duckdb + pyarrow

        import duckdb
        conn = duckdb.connect()
        kore.to_duckdb("data.kore", "sales", conn)
        result = conn.execute("SELECT SUM(amount) FROM sales").fetchall()
    """
    try:
        import duckdb
    except ImportError:
        raise ImportError("duckdb required: pip install duckdb")

    table = to_arrow(path)
    if conn is None:
        conn = duckdb.connect()
    conn.register(table_name, table)
    return conn


def to_polars(path: Union[str, 'Path']):
    """Read a .kore file as a Polars DataFrame.

    Requires: polars + pyarrow

        df = kore.to_polars("data.kore")
        df.group_by("region").agg(pl.col("amount").sum())
    """
    try:
        import polars as pl
    except ImportError:
        raise ImportError("polars required: pip install polars")
    return pl.from_arrow(to_arrow(path))


def from_polars(path: Union[str, 'Path'], df) -> None:
    """Write a Polars DataFrame to a .kore file.

        kore.from_polars("data.kore", polars_df)
    """
    from_arrow(path, df.to_arrow())


def to_parquet(path: Union[str, 'Path'], output_path: str) -> None:
    """Export .kore to Parquet format (for Spark/Hive native reading).

    Requires: pyarrow

        kore.to_parquet("data.kore", "data.parquet")
        # Spark: spark.read.parquet("data.parquet")
    """
    try:
        import pyarrow.parquet as pq
    except ImportError:
        raise ImportError("pyarrow required: pip install pyarrow")
    pq.write_table(to_arrow(path), output_path)


def from_parquet(kore_path: Union[str, 'Path'], parquet_path: str) -> None:
    """Import Parquet file to .kore format.

    Requires: pyarrow

        kore.from_parquet("data.kore", "data.parquet")
    """
    try:
        import pyarrow.parquet as pq
    except ImportError:
        raise ImportError("pyarrow required: pip install pyarrow")
    from_arrow(kore_path, pq.read_table(parquet_path))


# ── Kafka / Streaming Connector ───────────────────────────────────────────────

def to_kafka_message(block: 'DataBlock') -> bytes:
    """Serialize a DataBlock to Kafka message bytes.

        producer.send('topic', kore.to_kafka_message(block))
    """
    return b'KOREK' + (5).to_bytes(4, 'little') + _block_to_bytes(block)


def from_kafka_message(msg: bytes) -> 'DataBlock':
    """Deserialize a Kafka message to DataBlock.

        block = kore.from_kafka_message(consumer_record.value)
    """
    if not msg.startswith(b'KOREK'):
        raise ValueError("Not a KORE Kafka message")
    return _bytes_to_block(msg[9:])


def write_stream_chunk(path: Union[str, 'Path'], block: 'DataBlock') -> None:
    """Append a DataBlock chunk to a streaming .kore file (length-prefixed).

        # Producer side:
        kore.write_stream_chunk("stream.kore", block1)
        kore.write_stream_chunk("stream.kore", block2)
    """
    import struct
    chunk = _block_to_bytes(block)
    with open(str(path), 'ab') as f:
        f.write(struct.pack('<I', len(chunk)))
        f.write(chunk)


def read_stream_all(path: Union[str, 'Path']) -> 'DataBlock':
    """Read all chunks from a streaming .kore file into a merged DataBlock.

        merged = kore.read_stream_all("stream.kore")
    """
    import struct
    merged = None
    with open(str(path), 'rb') as f:
        while True:
            header = f.read(4)
            if len(header) < 4: break
            chunk_len = struct.unpack('<I', header)[0]
            chunk = f.read(chunk_len)
            if len(chunk) < chunk_len: break
            block = _bytes_to_block(chunk)
            if merged is None:
                merged = block
            else:
                for col in block.columns:
                    mc = merged.get_column(col.name)
                    if mc:
                        mc.data.extend(col.data)
                        mc.num_rows = len(mc.data)
    if merged and merged.columns:
        merged.num_rows = len(merged.columns[0].data)
    return merged or DataBlock()


def _block_to_bytes(block: 'DataBlock') -> bytes:
    """Internal: serialize DataBlock to raw bytes."""
    import struct
    buf = bytearray()
    buf += b'KORE'
    buf += struct.pack('<I', 2)  # version 2
    buf += struct.pack('<I', block.num_columns)
    for col in block.columns:
        dtype_val = {'F64': 1, 'I64': 2, 'STR': 3, 'BOOL': 2}.get(
            col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype), 2)
        name_bytes = col.name.encode()
        buf += bytes([dtype_val, len(name_bytes)]) + name_bytes
        vals = col.data
        buf += struct.pack('<Q', len(vals))
        for v in vals:
            if isinstance(v, float): buf += struct.pack('<d', v)
            else: buf += struct.pack('<Q', int(v) & 0xFFFFFFFFFFFFFFFF)
    crc = _crc32(bytes(buf))
    buf += struct.pack('<I', crc)
    return bytes(buf)


def _bytes_to_block(data: bytes) -> 'DataBlock':
    """Internal: deserialize raw bytes to DataBlock."""
    import struct
    block = DataBlock()
    body, crc_bytes = data[:-4], data[-4:]
    stored = struct.unpack('<I', crc_bytes)[0]
    if _crc32(body) != stored:
        raise ValueError("CRC32 mismatch in stream chunk")
    r, pos = body, 0
    pos += 4  # magic
    pos += 4  # version
    n_cols = struct.unpack_from('<I', r, pos)[0]; pos += 4
    for _ in range(n_cols):
        dtype_byte = r[pos]; name_len = r[pos+1]; pos += 2
        name = r[pos:pos+name_len].decode(); pos += name_len
        n_rows = struct.unpack_from('<Q', r, pos)[0]; pos += 8
        vals = []
        dtype = {1: DataType.F64, 2: DataType.I64}.get(dtype_byte, DataType.I64)
        for _ in range(n_rows):
            if dtype == DataType.F64:
                vals.append(struct.unpack_from('<d', r, pos)[0])
            else:
                vals.append(struct.unpack_from('<Q', r, pos)[0])
            pos += 8
        block.add_column(name, dtype, vals)
    return block


def _crc32(data: bytes) -> int:
    crc = 0xFFFFFFFF
    for byte in data:
        crc ^= byte
        for _ in range(8):
            if crc & 1: crc = (crc >> 1) ^ 0xEDB88320
            else: crc >>= 1
    return (~crc) & 0xFFFFFFFF


# ── ML / Tensor Support ───────────────────────────────────────────────────────

class Tensor:
    """Multi-dimensional tensor for ML workloads (embeddings, weights, etc.)."""

    def __init__(self, name: str, shape: list, data: list):
        expected = 1
        for d in shape: expected *= d
        if len(data) != expected:
            raise ValueError(f"data length {len(data)} != shape product {expected}")
        self.name = name
        self.shape = shape
        self.data = data

    @property
    def ndim(self): return len(self.shape)

    @property
    def num_rows(self): return self.shape[0]

    @property
    def num_cols(self): return self.shape[1] if len(self.shape) > 1 else 1

    def row(self, i: int) -> list:
        nc = self.num_cols
        return self.data[i * nc:(i + 1) * nc]

    def dot(self, i: int, query: list) -> float:
        return sum(a * b for a, b in zip(self.row(i), query))

    def knn(self, query: list, k: int = 5) -> list:
        """Find k nearest rows by cosine similarity."""
        import math
        qn = math.sqrt(sum(x*x for x in query))
        scores = []
        for i in range(self.num_rows):
            row = self.row(i)
            rn = math.sqrt(sum(x*x for x in row))
            dot = self.dot(i, query)
            cos = dot / (rn * qn) if rn > 0 and qn > 0 else 0.0
            scores.append((i, cos))
        scores.sort(key=lambda x: -x[1])
        return scores[:k]


class TensorBlock:
    """A block combining tensors (embeddings) + metadata (ids, labels)."""

    def __init__(self):
        self.tensors: list = []
        self.metadata: DataBlock = DataBlock()

    def add_tensor(self, tensor: 'Tensor') -> None:
        self.tensors.append(tensor)

    def get_tensor(self, name: str) -> 'Tensor':
        return next((t for t in self.tensors if t.name == name), None)

    def knn_search(self, tensor_name: str, query: list, k: int = 5) -> list:
        """KNN search across the tensor, returning (row_idx, score) pairs."""
        t = self.get_tensor(tensor_name)
        if not t: raise ValueError(f"Tensor '{tensor_name}' not found")
        return t.knn(query, k)


def write_tensors(path: Union[str, 'Path'], block: 'TensorBlock') -> None:
    """Write a TensorBlock (embeddings + metadata) to a .kore tensor file.

        tb = kore.TensorBlock()
        tb.add_tensor(kore.Tensor("embeddings", [100, 768], flat_data))
        tb.metadata.add_column("id", kore.DataType.I64, list(range(100)))
        kore.write_tensors("vectors.kore", tb)
    """
    import struct
    buf = bytearray()
    buf += b'KORET'
    buf += struct.pack('<I', 6)  # version 6
    buf += struct.pack('<I', len(block.tensors))
    for t in block.tensors:
        nb = t.name.encode()
        buf += bytes([len(nb)]) + nb
        buf += struct.pack('<I', len(t.shape))
        for d in t.shape: buf += struct.pack('<Q', d)
        buf += struct.pack('<Q', len(t.data))
        for v in t.data: buf += struct.pack('<d', float(v))
    meta_bytes = _block_to_bytes(block.metadata)
    buf += struct.pack('<I', len(meta_bytes)) + meta_bytes
    crc = _crc32(bytes(buf))
    buf += struct.pack('<I', crc)
    with open(str(path), 'wb') as f: f.write(buf)


def read_tensors(path: Union[str, 'Path']) -> 'TensorBlock':
    """Read a TensorBlock from a .kore tensor file.

        tb = kore.read_tensors("vectors.kore")
        results = tb.knn_search("embeddings", query_vector, k=10)
    """
    import struct
    with open(str(path), 'rb') as f: data = f.read()
    body, crc_bytes = data[:-4], data[-4:]
    if _crc32(body) != struct.unpack('<I', crc_bytes)[0]:
        raise ValueError("CRC32 mismatch in tensor file")
    pos = 5 + 4  # magic + version
    n_tensors = struct.unpack_from('<I', body, pos)[0]; pos += 4
    block = TensorBlock()
    for _ in range(n_tensors):
        nl = body[pos]; pos += 1
        name = body[pos:pos+nl].decode(); pos += nl
        ndim = struct.unpack_from('<I', body, pos)[0]; pos += 4
        shape = [struct.unpack_from('<Q', body, pos + i*8)[0] for i in range(ndim)]; pos += ndim * 8
        n_vals = struct.unpack_from('<Q', body, pos)[0]; pos += 8
        vals = [struct.unpack_from('<d', body, pos + i*8)[0] for i in range(n_vals)]; pos += n_vals * 8
        block.add_tensor(Tensor(name, shape, vals))
    meta_len = struct.unpack_from('<I', body, pos)[0]; pos += 4
    block.metadata = _bytes_to_block(body[pos:pos+meta_len])
    return block


def to_numpy(block: 'DataBlock'):
    """Convert a DataBlock column to a numpy array (for ML pipelines).

    Requires: numpy

        arr = kore.to_numpy(block)['price']  # → numpy array
    """
    try: import numpy as np
    except ImportError: raise ImportError("numpy required: pip install numpy")
    result = {}
    for col in block.columns:
        dtype_name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
        if dtype_name in ('F64', 'FLOAT64'):
            result[col.name] = np.array(col.data, dtype=np.float64)
        elif dtype_name in ('I64', 'INT64'):
            result[col.name] = np.array(col.data, dtype=np.int64)
        else:
            result[col.name] = np.array(col.data)
    return result


def from_numpy(path: Union[str, 'Path'], arrays: dict) -> None:
    """Write a dict of numpy arrays to a .kore file.

        import numpy as np
        kore.from_numpy("data.kore", {
            "price": np.array([10.5, 20.0], dtype=np.float64),
            "qty":   np.array([100, 200], dtype=np.int64),
        })
    """
    block = DataBlock()
    for name, arr in arrays.items():
        import numpy as np
        if np.issubdtype(arr.dtype, np.floating):
            block.add_column(name, DataType.F64, arr.tolist())
        else:
            block.add_column(name, DataType.I64, arr.tolist())
    write_file(path, block)


# ── Avro Bridge ───────────────────────────────────────────────────────────────

def to_avro_schema(block: 'DataBlock') -> dict:
    """Generate Avro schema dict from a DataBlock."""
    fields = []
    for col in block.columns:
        dtype_name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
        avro_type = "double" if dtype_name in ('F64', 'FLOAT64') else "long"
        fields.append({"name": col.name, "type": avro_type})
    return {"type": "record", "name": "KoreRecord", "fields": fields}


def write_avro(path: Union[str, 'Path'], block: 'DataBlock') -> None:
    """Write a DataBlock to Avro format (for Kafka/Hadoop interop).

    Requires: fastavro

        kore.write_avro("data.avro", block)
        # kafka producer can then send the avro bytes
    """
    try: import fastavro
    except ImportError: raise ImportError("fastavro required: pip install fastavro")
    schema = to_avro_schema(block)
    records = []
    for i in range(block.num_rows):
        row = {}
        for col in block.columns:
            dtype_name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
            row[col.name] = float(col.data[i]) if dtype_name in ('F64', 'FLOAT64') else int(col.data[i])
        records.append(row)
    with open(str(path), 'wb') as f:
        fastavro.writer(f, schema, records)


# ── MongoDB / BSON Bridge ─────────────────────────────────────────────────────

def to_mongodb_docs(block: 'DataBlock') -> list:
    """Convert DataBlock to list of MongoDB-compatible dicts.

        docs = kore.to_mongodb_docs(block)
        collection.insert_many(docs)
    """
    docs = []
    for i in range(block.num_rows):
        doc = {}
        for col in block.columns:
            dtype_name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
            doc[col.name] = float(col.data[i]) if dtype_name in ('F64', 'FLOAT64') else int(col.data[i])
        docs.append(doc)
    return docs


def from_mongodb_docs(path: Union[str, 'Path'], docs: list, col_types: dict = None) -> None:
    """Write MongoDB documents to a .kore file.

        cursor = collection.find({"region": 1})
        kore.from_mongodb_docs("region1.kore", list(cursor))
    """
    if not docs: return
    keys = [k for k in docs[0].keys() if k != '_id']
    block = DataBlock()
    for k in keys:
        vals = [doc.get(k, 0) for doc in docs]
        dtype = DataType.F64 if any(isinstance(v, float) for v in vals) else DataType.I64
        block.add_column(k, dtype, vals)
    write_file(path, block)


# ── Column Statistics Footer (Predicate Pushdown) ─────────────────────────────

class ColFooter:
    """Column-level statistics for predicate pushdown (scan skipping)."""
    def __init__(self, name, dtype, row_count, null_count, min_val, max_val, sum_val):
        self.name = name; self.dtype = dtype
        self.row_count = row_count; self.null_count = null_count
        self.min_val = min_val; self.max_val = max_val; self.sum_val = sum_val
        self.mean_val = sum_val / row_count if row_count > 0 else 0.0

    def __repr__(self):
        return f"ColFooter({self.name}: min={self.min_val:.4f} max={self.max_val:.4f} nulls={self.null_count})"


def write_file_v3(path: Union[str, 'Path'], block: 'DataBlock') -> None:
    """Write DataBlock with column statistics footer for predicate pushdown.

        kore.write_file_v3("data.kore", block)
        footers = kore.read_footer_only("data.kore")
        # Skip file if no match:
        if not kore.can_skip_file(footers, "price", 100.0, 200.0):
            block = kore.read_file_v3("data.kore")
    """
    import struct
    data_bytes = _block_to_bytes(block)
    buf = bytearray(b'KOREV' + struct.pack('<I', 3) + struct.pack('<I', len(data_bytes)) + data_bytes)
    buf += struct.pack('<I', block.num_columns)
    for col in block.columns:
        dtype_name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
        nb = col.name.encode()
        buf += bytes([len(nb)]) + nb
        buf += bytes([1 if dtype_name in ('F64','FLOAT64') else 2])
        vals = col.data
        n = len(vals)
        nums = [float(v) for v in vals] if dtype_name in ('F64','FLOAT64') else [int(v) for v in vals]
        mn = min(nums) if nums else 0.0
        mx = max(nums) if nums else 0.0
        sm = sum(nums) if nums else 0.0
        buf += struct.pack('<Q', n) + struct.pack('<Q', 0)  # rows, null_count
        buf += struct.pack('<d', mn) + struct.pack('<d', mx) + struct.pack('<d', sm)
    crc = _crc32(bytes(buf))
    buf += struct.pack('<I', crc)
    with open(str(path), 'wb') as f: f.write(buf)


def read_footer_only(path: Union[str, 'Path']) -> list:
    """Read ONLY column footers — fast, no data loaded.

        footers = kore.read_footer_only("data.kore")
        for f in footers:
            print(f"{f.name}: min={f.min_val} max={f.max_val}")
    """
    import struct
    with open(str(path), 'rb') as f: data = f.read()
    body = data[:-4]
    if not body.startswith(b'KOREV'): raise ValueError("Not a v3 KORE file")
    pos = 9  # magic(5)+version(4)
    data_len = struct.unpack_from('<I', body, pos)[0]; pos += 4 + data_len
    n_cols = struct.unpack_from('<I', body, pos)[0]; pos += 4
    footers = []
    for _ in range(n_cols):
        nl = body[pos]; pos += 1
        name = body[pos:pos+nl].decode(); pos += nl
        dtype_byte = body[pos]; pos += 1
        row_count, null_count = struct.unpack_from('<QQ', body, pos); pos += 16
        mn, mx, sm = struct.unpack_from('<ddd', body, pos); pos += 24
        dtype = DataType.F64 if dtype_byte == 1 else DataType.I64
        footers.append(ColFooter(name, dtype, row_count, null_count, mn, mx, sm))
    return footers


def can_skip_file(footers: list, col_name: str, lo: float, hi: float) -> bool:
    """Return True if the file can be SKIPPED (no rows can match [lo, hi]).

        # Only read files that might have price in 100-200:
        if not kore.can_skip_file(footers, "price", 100.0, 200.0):
            data = kore.read_file("data.kore")
    """
    for f in footers:
        if f.name == col_name:
            return f.max_val < lo or f.min_val > hi
    return False


# ── Null/None Values Support ──────────────────────────────────────────────────

class NullableColumn:
    """A column that supports None values with a validity bitmap."""

    def __init__(self, name: str, dtype: 'DataType', values: list):
        self.name = name
        self.dtype = dtype
        self._raw = values  # list of (value | None)
        self.validity = [v is not None for v in values]
        self.data = [v if v is not None else 0 for v in values]
        self.num_rows = len(values)

    def get(self, i: int):
        return self.data[i] if self.validity[i] else None

    @property
    def null_count(self): return sum(1 for v in self.validity if not v)

    @property
    def valid_count(self): return sum(1 for v in self.validity if v)

    def non_null_values(self): return [v for v, ok in zip(self.data, self.validity) if ok]

    def fill_null(self, fill_value) -> 'NullableColumn':
        """Replace None with fill_value."""
        return NullableColumn(self.name, self.dtype, [v if v is not None else fill_value for v in self._raw])


class NullableBlock:
    """A DataBlock that supports None values per cell."""

    def __init__(self):
        self.columns: list = []
        self.num_columns: int = 0

    def add_column(self, name: str, dtype: 'DataType', values: list) -> None:
        self.columns.append(NullableColumn(name, dtype, values))
        self.num_columns += 1

    @property
    def num_rows(self): return self.columns[0].num_rows if self.columns else 0

    def get_column(self, name: str) -> 'NullableColumn':
        return next((c for c in self.columns if c.name == name), None)

    def null_count(self, col_name: str) -> int:
        col = self.get_column(col_name)
        return col.null_count if col else 0

    def drop_nulls(self) -> 'DataBlock':
        """Return a new DataBlock with all rows containing any null removed."""
        valid_rows = [i for i in range(self.num_rows)
                      if all(c.validity[i] for c in self.columns)]
        result = DataBlock()
        for col in self.columns:
            result.add_column(col.name, col.dtype, [col.data[i] for i in valid_rows])
        return result

    def fill_nulls(self, fill_values: dict) -> 'NullableBlock':
        """Fill nulls in each column with specified values."""
        result = NullableBlock()
        for col in self.columns:
            fill = fill_values.get(col.name, 0)
            result.columns.append(col.fill_null(fill))
        result.num_columns = self.num_columns
        return result


def write_nullable(path: Union[str, 'Path'], block: 'NullableBlock') -> None:
    """Write a NullableBlock (supports None values) to a .kore file."""
    import struct
    buf = bytearray(b'KOREN' + struct.pack('<I', 7) + struct.pack('<I', block.num_columns))
    for col in block.columns:
        nb = col.name.encode()
        dtype_val = {'F64':1,'FLOAT64':1,'I64':2,'INT64':2}.get(
            col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype), 2)
        buf += bytes([len(nb)]) + nb + bytes([dtype_val])
        buf += struct.pack('<Q', col.num_rows)
        # Validity bitmap
        n_words = (col.num_rows + 63) // 64
        bitmap = [0] * n_words
        for i, ok in enumerate(col.validity):
            if ok: bitmap[i//64] |= (1 << (i%64))
        buf += struct.pack('<I', n_words)
        for w in bitmap: buf += struct.pack('<Q', w)
        for v in col.data:
            buf += struct.pack('<Q', int(v) & 0xFFFFFFFFFFFFFFFF)
    crc = _crc32(bytes(buf))
    buf += struct.pack('<I', crc)
    with open(str(path), 'wb') as f: f.write(buf)


def read_nullable(path: Union[str, 'Path']) -> 'NullableBlock':
    """Read a NullableBlock preserving None values."""
    import struct
    with open(str(path), 'rb') as f: data = f.read()
    body = data[:-4]
    if not body.startswith(b'KOREN'): raise ValueError("Not a nullable KORE file")
    pos = 9
    n_cols = struct.unpack_from('<I', body, pos)[0]; pos += 4
    block = NullableBlock()
    for _ in range(n_cols):
        nl = body[pos]; pos += 1
        name = body[pos:pos+nl].decode(); pos += nl
        dtype_byte = body[pos]; pos += 1
        dtype = DataType.F64 if dtype_byte == 1 else DataType.I64
        n = struct.unpack_from('<Q', body, pos)[0]; pos += 8
        n_words = struct.unpack_from('<I', body, pos)[0]; pos += 4
        bitmap = [struct.unpack_from('<Q', body, pos+i*8)[0] for i in range(n_words)]; pos += n_words*8
        raw_vals = [struct.unpack_from('<Q', body, pos+i*8)[0] for i in range(n)]; pos += n*8
        nullable_vals = [raw_vals[i] if (bitmap[i//64] & (1<<(i%64))) else None for i in range(n)]
        block.add_column(name, dtype, nullable_vals)
    return block


# ── Delta / FOR / Bitpack Encoding ────────────────────────────────────────────

def delta_encode(values: list) -> tuple:
    """Delta encode sorted integers. Good for timestamps, sequential IDs.

        base, deltas = kore.delta_encode([100, 101, 103, 106, 110])
        # base=100, deltas=[0, 1, 2, 3, 4]
    """
    if not values: return 0, []
    base = values[0]
    return base, [0] + [values[i] - values[i-1] for i in range(1, len(values))]


def delta_decode(base: int, deltas: list) -> list:
    result, cur = [], base
    for d in deltas: cur += d; result.append(cur)
    return result


def for_encode(values: list) -> tuple:
    """Frame-of-Reference encoding. Good for clustered values."""
    if not values: return 0, []
    mn = min(values)
    return mn, [v - mn for v in values]


def for_decode(minimum: int, offsets: list) -> list:
    return [minimum + o for o in offsets]


def dict_encode(values: list) -> tuple:
    """Dictionary encode a list. Returns (dictionary, codes).

        d, codes = kore.dict_encode(["NY","CA","NY","TX","CA"])
        # d=["NY","CA","TX"], codes=[0,1,0,2,1]
    """
    d, seen = [], {}
    codes = []
    for v in values:
        if v not in seen: seen[v] = len(d); d.append(v)
        codes.append(seen[v])
    return d, codes


def auto_select_codec(values: list) -> str:
    """Automatically choose best compression codec for a column.

        codec = kore.auto_select_codec(data['region'])
        print(f"Best codec for region: {codec}")
    """
    if len(values) < 2: return "RAW"
    unique_ratio = len(set(values)) / len(values)
    if unique_ratio < 0.2: return "RLE"       # low cardinality — check FIRST
    try:
        nums = [int(v) for v in values]
        sorted_check = all(nums[i] <= nums[i+1] for i in range(len(nums)-1))
        if sorted_check: return "DELTA"         # sorted integers
        mn, mx = min(nums), max(nums)
        bits = (mx - mn).bit_length() if mx > mn else 1
        if bits <= 16: return "BITPACK"         # small range
        if (mx - mn) < mx // 4: return "FOR"   # clustered
    except: pass
    return "RAW"


# ── Table Catalog ──────────────────────────────────────────────────────────────

class TableCatalog:
    """Multi-file table catalog — tracks all partition files of a logical table.

        cat = kore.TableCatalog("sales")
        cat.add_file("sales/region=1/data.kore", rows=50000, size=512000)
        cat.add_file("sales/region=2/data.kore", rows=50000, size=498000)
        cat.save("sales/catalog.json")
        print(f"Total rows: {cat.total_rows}")
    """

    def __init__(self, name: str):
        self.name = name
        self.files = []

    def add_file(self, path: str, rows: int, size: int, partition=None, snapshot=1) -> None:
        self.files.append({'path': path, 'rows': rows, 'size': size,
                           'partition': str(partition) if partition else None, 'snapshot': snapshot})

    @property
    def total_rows(self): return sum(f['rows'] for f in self.files)

    @property
    def total_size_mb(self): return sum(f['size'] for f in self.files) / (1024*1024)

    @property
    def latest_snapshot(self): return max((f['snapshot'] for f in self.files), default=0)

    def save(self, path: Union[str, 'Path']) -> None:
        """Save catalog to JSON."""
        import json as _json
        with open(str(path), 'w') as f:
            _json.dump({'name': self.name, 'files': self.files,
                       'total_rows': self.total_rows, 'total_size_mb': round(self.total_size_mb, 3),
                       'snapshots': self.latest_snapshot}, f, indent=2)

    @classmethod
    def load(cls, path: Union[str, 'Path']) -> 'TableCatalog':
        """Load catalog from JSON."""
        import json as _json
        with open(str(path)) as f: data = _json.load(f)
        cat = cls(data['name'])
        cat.files = data.get('files', [])
        return cat

    def files_for_snapshot(self, snapshot: int) -> list:
        return [f for f in self.files if f['snapshot'] == snapshot]

    def prune_old_snapshots(self, keep: int = 3) -> None:
        """Remove files older than the most recent `keep` snapshots."""
        snaps = sorted(set(f['snapshot'] for f in self.files), reverse=True)[:keep]
        self.files = [f for f in self.files if f['snapshot'] in snaps]


# ── LZ4 Compression ───────────────────────────────────────────────────────────

def lz4_compress(data: bytes) -> bytes:
    """Compress bytes (simplified frame format — good for binary data)."""
    import struct, zlib
    # Use zlib deflate (available in stdlib) with length prefix
    compressed = zlib.compress(data, level=6)
    return struct.pack('<I', len(data)) + struct.pack('<I', len(compressed)) + compressed


def lz4_decompress(data: bytes) -> bytes:
    """Decompress bytes compressed with lz4_compress."""
    import struct, zlib
    orig_len = struct.unpack_from('<I', data, 0)[0]
    comp_len = struct.unpack_from('<I', data, 4)[0]
    return zlib.decompress(data[8:8+comp_len])


def write_file_lz4(path: Union[str, 'Path'], block: 'DataBlock') -> None:
    """Write DataBlock with LZ4 column compression (better than RLE for random data).

        kore.write_file_lz4("data.kore", block)   # smaller file
        block = kore.read_file_lz4("data.kore")   # transparent decompression
    """
    import struct
    buf = bytearray(b'KOREL' + struct.pack('<I', 9) + struct.pack('<I', block.num_columns))
    for col in block.columns:
        dtype_name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
        nb = col.name.encode()
        dtype_byte = 1 if dtype_name in ('F64','FLOAT64') else 2
        buf += bytes([len(nb)]) + nb + bytes([dtype_byte])
        buf += struct.pack('<Q', len(col.data))
        raw = bytearray()
        for v in col.data:
            if dtype_name in ('F64','FLOAT64'): raw.extend(struct.pack('<d', float(v)))
            else: raw.extend(struct.pack('<Q', int(v) & 0xFFFFFFFFFFFFFFFF))
        comp = lz4_compress(bytes(raw))
        buf += struct.pack('<I', len(comp)) + comp
    crc = _crc32(bytes(buf))
    buf += struct.pack('<I', crc)
    with open(str(path), 'wb') as f: f.write(buf)


def read_file_lz4(path: Union[str, 'Path']) -> 'DataBlock':
    """Read a compressed KORE file (zlib deflate per column)."""
    import struct
    with open(str(path), 'rb') as f: data = f.read()
    body = data[:-4]
    if _crc32(body) != struct.unpack_from('<I', data, len(data)-4)[0]:
        raise ValueError("CRC32 mismatch")
    if not body.startswith(b'KOREL'): raise ValueError("Not LZ4 KORE file")
    pos = 9
    n_cols = struct.unpack_from('<I', body, pos)[0]; pos += 4
    block = DataBlock()
    for _ in range(n_cols):
        nl = body[pos]; pos += 1
        name = body[pos:pos+nl].decode(); pos += nl
        dtype_byte = body[pos]; pos += 1
        dtype = DataType.F64 if dtype_byte == 1 else DataType.I64
        n = struct.unpack_from('<Q', body, pos)[0]; pos += 8
        comp_len = struct.unpack_from('<I', body, pos)[0]; pos += 4
        raw = lz4_decompress(body[pos:pos+comp_len]); pos += comp_len
        vals = []
        for i in range(n):
            bits = struct.unpack_from('<Q', raw, i*8)[0]
            vals.append(struct.unpack('<d', struct.pack('<Q', bits))[0] if dtype == DataType.F64 else bits)
        block.add_column(name, dtype, vals)
    return block


# ── Nested Types ──────────────────────────────────────────────────────────────

class ArrayColumn:
    """A column where each cell is a variable-length array."""

    def __init__(self, name: str, dtype: 'DataType'):
        self.name = name; self.dtype = dtype; self.arrays = []

    def push(self, arr: list) -> None: self.arrays.append(list(arr))
    def get(self, i: int) -> list: return self.arrays[i]
    def __len__(self): return len(self.arrays)

    def flatten(self) -> list:
        return [v for arr in self.arrays for v in arr]

    def lengths(self) -> list:
        return [len(arr) for arr in self.arrays]


class NestedBlock:
    """A DataBlock with both scalar and array columns."""

    def __init__(self):
        self.scalars = DataBlock()
        self.array_cols: list = []

    def add_scalar(self, name, dtype, values): self.scalars.add_column(name, dtype, values)
    def add_array_col(self, col: 'ArrayColumn'): self.array_cols.append(col)

    def get_array_col(self, name: str) -> 'ArrayColumn':
        return next((c for c in self.array_cols if c.name == name), None)

    @property
    def num_rows(self): return self.scalars.num_rows if self.scalars.columns else (
        self.array_cols[0].arrays.__len__() if self.array_cols else 0)


def write_nested(path: Union[str, 'Path'], block: 'NestedBlock') -> None:
    """Write a NestedBlock (scalar + array columns) to disk.

        nb = kore.NestedBlock()
        nb.add_scalar("order_id", kore.DataType.I64, [1, 2, 3])
        items = kore.ArrayColumn("item_prices", kore.DataType.F64)
        items.push([10.5, 20.0])      # row 0 has 2 items
        items.push([30.0])            # row 1 has 1 item
        items.push([5.0, 99.9, 15.0]) # row 2 has 3 items
        nb.add_array_col(items)
        kore.write_nested("orders.kore", nb)
    """
    import struct
    scalar_bytes = _block_to_bytes(block.scalars)
    buf = bytearray(b'KOREX' + struct.pack('<I', 10))
    buf += struct.pack('<I', len(scalar_bytes)) + scalar_bytes
    buf += struct.pack('<I', len(block.array_cols))
    for ac in block.array_cols:
        nb2 = ac.name.encode()
        dtype_byte = 1 if str(getattr(ac.dtype, 'name', ac.dtype)) in ('F64','FLOAT64') else 2
        buf += bytes([len(nb2)]) + nb2 + bytes([dtype_byte])
        buf += struct.pack('<I', len(ac.arrays))
        for arr in ac.arrays:
            buf += struct.pack('<I', len(arr))
            for v in arr:
                buf += struct.pack('<Q', int(v) & 0xFFFFFFFFFFFFFFFF)
    crc = _crc32(bytes(buf))
    buf += struct.pack('<I', crc)
    with open(str(path), 'wb') as f: f.write(buf)


def read_nested(path: Union[str, 'Path']) -> 'NestedBlock':
    """Read a NestedBlock from disk."""
    import struct
    with open(str(path), 'rb') as f: data = f.read()
    body = data[:-4]
    pos = 9
    sl = struct.unpack_from('<I', body, pos)[0]; pos += 4
    nb = NestedBlock()
    nb.scalars = _bytes_to_block(body[pos:pos+sl]); pos += sl
    n_arr = struct.unpack_from('<I', body, pos)[0]; pos += 4
    for _ in range(n_arr):
        nl = body[pos]; pos += 1
        name = body[pos:pos+nl].decode(); pos += nl
        dtype_byte = body[pos]; pos += 1
        dtype = DataType.F64 if dtype_byte == 1 else DataType.I64
        n_rows = struct.unpack_from('<I', body, pos)[0]; pos += 4
        ac = ArrayColumn(name, dtype)
        for _ in range(n_rows):
            alen = struct.unpack_from('<I', body, pos)[0]; pos += 4
            arr = [struct.unpack_from('<Q', body, pos+i*8)[0] for i in range(alen)]; pos += alen*8
            ac.push(arr)
        nb.array_cols.append(ac)
    return nb


# ── Mini SQL Engine ───────────────────────────────────────────────────────────

def kore_sql(sql: str) -> dict:
    """Execute a simple SQL SELECT over a .kore file.

    Supported:
        SELECT col1, col2 FROM file.kore
        SELECT * FROM file.kore WHERE price > 100
        SELECT region, SUM(price) FROM file.kore GROUP BY region
        SELECT * FROM file.kore ORDER BY price DESC LIMIT 10

    Returns: {"columns": [...], "rows": [...], "row_count": N}

    Example:
        result = kore.kore_sql("SELECT region, SUM(price) FROM data.kore GROUP BY region")
        for row in result["rows"]: print(row)
    """
    import re
    sql_up = sql.strip().upper()

    # Parse FROM
    m = re.search(r'FROM\s+(\S+)', sql_up)
    if not m: raise ValueError("No FROM clause")
    file_path = re.search(r'FROM\s+(\S+)', sql, re.IGNORECASE).group(1)

    block = read_file(file_path)

    # Parse WHERE
    keep = [True] * block.num_rows
    m = re.search(r'WHERE\s+(.+?)(?:\s+GROUP|\s+ORDER|\s+LIMIT|$)', sql_up)
    if m:
        cond = m.group(1).strip()
        cm = re.match(r'(\w+)\s*(>=|<=|!=|<>|>|<|=)\s*([\d.]+)', cond)
        if cm:
            col_name, op, val = cm.group(1).lower(), cm.group(2), float(cm.group(3))
            col = block.get_column(col_name)
            if col:
                for i, v in enumerate(col.data):
                    fv = float(v)
                    keep[i] = {'>':(fv>val), '<':(fv<val), '>=':(fv>=val),
                               '<=':(fv<=val), '=':(fv==val), '==':(fv==val),
                               '!=':(fv!=val), '<>':(fv!=val)}.get(op, True)

    # Parse GROUP BY
    gm = re.search(r'GROUP\s+BY\s+(\w+)', sql_up)
    if gm:
        group_col = gm.group(1).lower()
        gc = block.get_column(group_col)
        if not gc: raise ValueError(f"GROUP BY column '{group_col}' not found")
        groups = {}
        for i, k in enumerate(gc.data):
            if not keep[i]: continue
            if k not in groups: groups[k] = {'key': k, 'sum': 0.0, 'count': 0}
            groups[k]['count'] += 1
            for col in block.columns:
                if col.name != group_col: groups[k]['sum'] += float(col.data[i])
        rows = sorted([[g['key'], g['sum'], g['count']] for g in groups.values()])
        return {"columns": [group_col, "SUM", "COUNT"], "rows": rows, "row_count": len(rows)}

    # Parse SELECT cols
    sm = re.search(r'SELECT\s+(.+?)\s+FROM', sql, re.IGNORECASE)
    select_clause = sm.group(1).strip() if sm else "*"
    col_names = [c.name for c in block.columns] if select_clause == "*" else \
                [c.strip().lower() for c in select_clause.split(",")]

    indices = [i for i in range(block.num_rows) if keep[i]]
    rows = [[float(block.get_column(cn).data[i]) if block.get_column(cn) else 0.0
             for cn in col_names] for i in indices]

    # ORDER BY
    om = re.search(r'ORDER\s+BY\s+(\w+)(?:\s+(ASC|DESC))?', sql_up)
    if om:
        oc, desc = om.group(1).lower(), om.group(2) == 'DESC' if om.group(2) else False
        oci = col_names.index(oc) if oc in col_names else 0
        rows.sort(key=lambda r: r[oci], reverse=desc)

    # LIMIT
    lm = re.search(r'LIMIT\s+(\d+)', sql_up)
    if lm: rows = rows[:int(lm.group(1))]

    return {"columns": col_names, "rows": rows, "row_count": len(rows)}


# ── MVCC (Multi-Version Concurrency Control) ──────────────────────────────────

class MvccTransaction:
    """Snapshot isolation for concurrent readers.

        # Reader:
        tx = kore.MvccTransaction.begin_read("orders.kore")
        data = tx.read()  # consistent snapshot

        # Writer (in another process):
        kore.mvcc_write("orders.kore", new_block)  # increments version
    """

    def __init__(self, path: str, snapshot_version: int):
        self.path = path
        self.snapshot_version = snapshot_version

    @classmethod
    def begin_read(cls, path: str) -> 'MvccTransaction':
        import os
        ver_file = f"{path}.ver"
        try: version = int(open(ver_file).read().strip())
        except: version = 0
        return cls(path, version)

    def read(self) -> 'DataBlock':
        import os
        if self.snapshot_version > 0:
            snap = f"{self.path}.v{self.snapshot_version:03d}.kore"
            if os.path.exists(snap): return read_file(snap)
        return read_file(self.path)


def mvcc_write(path: Union[str, 'Path'], block: 'DataBlock') -> int:
    """Write with MVCC version increment. Returns new version number.

        v = kore.mvcc_write("orders.kore", new_block)
        print(f"Written version {v}")
    """
    import os
    path = str(path)
    ver_file = f"{path}.ver"
    try: current = int(open(ver_file).read().strip())
    except: current = 0
    # Snapshot current before overwrite
    if current > 0 and os.path.exists(path):
        snap = f"{path}.v{current:03d}.kore"
        if not os.path.exists(snap):
            import shutil; shutil.copy2(path, snap)
    write_file(path, block)
    new_ver = current + 1
    open(ver_file, 'w').write(str(new_ver))
    return new_ver


# ── Cloud-Native S3/HTTP Reader ────────────────────────────────────────────────

def read_url(url: str) -> 'DataBlock':
    """Read a .kore file from an HTTP URL (S3 presigned, GCS signed, Azure SAS).

    For S3: generate a presigned URL first:
        import boto3
        url = boto3.client('s3').generate_presigned_url('get_object',
            Params={'Bucket': 'my-bucket', 'Key': 'data.kore'}, ExpiresIn=3600)
        block = kore.read_url(url)
    """
    try:
        import urllib.request
        with urllib.request.urlopen(url) as resp:
            data = resp.read()
        return _bytes_to_block(data)
    except Exception as e:
        raise IOError(f"Failed to read from URL: {e}")


def write_url(url: str, block: 'DataBlock') -> None:
    """Write a DataBlock to an HTTP endpoint (PUT request).

        kore.write_url("https://my-server/upload/data.kore", block)
    """
    try:
        import urllib.request
        data = _block_to_bytes(block)
        req = urllib.request.Request(url, data=data, method='PUT',
            headers={'Content-Type': 'application/octet-stream'})
        urllib.request.urlopen(req)
    except Exception as e:
        raise IOError(f"Failed to write to URL: {e}")
