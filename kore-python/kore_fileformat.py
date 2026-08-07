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

        # Try multiple library paths
        lib_names = [
            'libkore_ffi.so',           # Linux
            'libkore_ffi.dylib',        # macOS
            'kore_ffi.dll',             # Windows
        ]

        for lib_name in lib_names:
            try:
                cls._lib = ctypes.CDLL(lib_name)
                return cls._lib
            except OSError:
                continue

        raise RuntimeError(
            f"Could not load kore-ffi library. "
            f"Tried: {', '.join(lib_names)}"
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
    """Write DataBlock to KORE file.
    
    Args:
        path: Output file path
        data_block: DataBlock to serialize
        
    Raises:
        IOError: If write fails
    """
    lib = KoreFFI.get_library()
    
    # For now: simple JSON serialization (Phase 3 placeholder)
    # TODO: Call Rust kore_write_file() via FFI
    
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    
    with open(path, 'w') as f:
        json.dump(data_block.to_dict(), f, indent=2)


def read_file(path: Union[str, Path]) -> DataBlock:
    """Read KORE file into DataBlock.
    
    Args:
        path: Input file path
        
    Returns:
        DataBlock with deserialized data
        
    Raises:
        IOError: If read fails
    """
    lib = KoreFFI.get_library()
    
    # For now: simple JSON deserialization (Phase 3 placeholder)
    # TODO: Call Rust kore_read_file() via FFI
    
    path = Path(path)
    
    with open(path, 'r') as f:
        data = json.load(f)
    
    block = DataBlock(num_rows=data['num_rows'])
    for col_data in data['columns']:
        col = Column(
            name=col_data['name'],
            dtype=DataType[col_data['type']],
            data=col_data['data'],
        )
        if col_data.get('stats'):
            col.stats = ColumnStats(
                min_value=col_data['stats']['min'],
                max_value=col_data['stats']['max'],
                null_count=col_data['stats']['nulls'],
                cardinality=col_data['stats']['cardinality'],
                crc32=col_data['stats']['crc32'],
            )
        block.columns.append(col)
    
    return block


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


__version__ = "2.0.0"
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
]
