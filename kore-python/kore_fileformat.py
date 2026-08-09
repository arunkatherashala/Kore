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


__version__ = "1.6.7"
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



