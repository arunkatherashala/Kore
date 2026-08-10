# KORE FileFormat — Python

**Version 1.6.32** | [PyPI](https://pypi.org/project/kore-fileformat/) | [GitHub](https://github.com/arunkatherashala/Kore)

World's fastest human-readable columnar format. `.kore` v3 opens in Notepad AND reads 12x faster than CSV.

## Install

```bash
pip install kore-fileformat
```

## .kore v3 — One Format, Everything

```
KORE2 offset=0000000455      ← jump straight to data
# KORE Format v3.0
# Rows: 100,000  Columns: 3
# Compressed: 28,500 bytes (Rust ZSTD/LZ4)
# Schema:
#   price                F64
#   qty                  I64
# Preview (first 5 rows):
#   [price=10.5 | qty=100]
[binary compressed data — 10x smaller than JSON]
```

## Quick Start

```python
import kore_fileformat as kore

# Write — human-readable header + compressed binary
block = kore.DataBlock()
block.add_column('price', kore.DataType.F64, [10.5, 20.0, 30.75])
block.add_column('qty',   kore.DataType.I64, [100,  200,  300])
kore.write_file('data.kore', block)

# Read — returns array.array (no Python object overhead)
result = kore.read_file('data.kore')
print(result.num_rows, result.num_columns)

# Inspect without loading data
kore.inspect_kore('data.kore')           # prints header
header = kore.kore_header('data.kore')   # returns string
stats  = kore.kore_stats('data.kore')    # {'total_kb', 'overhead_pct', ...}
```

## CLI (installed automatically)

```bash
kore inspect data.kore            # show schema + preview (no full read)
kore stats   data.kore            # file size breakdown
kore convert src.kore dst.hkore   # convert formats
kore bench                        # write/read speed benchmark
kore version                      # version string
```

## Benchmark

| Format | Read | Write | Size |
|--------|------|-------|------|
| **KORE .kore** | **79 ns/row** | 255 ns/row | **305 KB** |
| **KORE .hkore** | **28 ns/row** | 154 ns/row | 3,126 KB |
| JSON | 1,096 ns/row | 9,576 ns/row | 6,786 KB |
| CSV | 1,252 ns/row | 3,447 ns/row | 3,368 KB |
| SQLite | 1,258 ns/row | 1,256 ns/row | 3,180 KB |

*(100K rows × 4 cols, warm OS cache)*

## API Reference

| Function | Description |
|----------|-------------|
| `write_file(path, block)` | Write .kore v3 (compressed + human header) |
| `read_file(path)` | Read .kore → DataBlock (returns array.array) |
| `write_hybrid(path, block)` | Write .hkore (raw binary, 28 ns/row read) |
| `read_hybrid(path)` | Read .hkore → DataBlock |
| `inspect_kore(path)` | Print text header (no data load) |
| `kore_header(path)` | Get text header as string |
| `kore_stats(path)` | Dict: total_kb, header_kb, binary_kb, overhead_pct |
| `DataBlock()` | Create empty block |
| `block.add_column(name, dtype, data)` | Add column |
| `block.get_column(name)` | Get column by name |

## Data Types

```python
kore.DataType.F64   # 64-bit float
kore.DataType.I64   # 64-bit integer
kore.DataType.STR   # UTF-8 string
kore.DataType.BOOL  # Boolean
```

## Install

```bash
pip install kore-fileformat==1.6.32
```

Or from source (requires Rust):
```bash
cargo build --release -p kore-ffi
pip install -e .
```

## Quick Start

```python
import kore_fileformat as kore

# --- Write ---
block = kore.DataBlock()
block.add_column('price',    kore.DataType.F64, [10.5, 20.0, 30.75])
block.add_column('quantity', kore.DataType.I64, [100,  200,  300])
kore.write_file('data.kore', block)

# --- Read ---
result = kore.read_file('data.kore')
print(f'{result.num_rows} rows, {result.num_columns} columns')
price_col = result.get_column('price')
print(price_col.data)   # [10.5, 20.0, 30.75]

# --- CRC32 checksum ---
checksum = kore.crc32(b'hello kore')
print(f'crc32 = {checksum:#010x}')   # 0x4b029b4b
```

## API Reference

| Function | Description |
|----------|-------------|
| `write_file(path, block)` | Write DataBlock to .kore binary |
| `read_file(path)` | Read .kore binary into DataBlock |
| `crc32(data: bytes)` | CRC32 checksum |
| `DataBlock()` | Create empty block |
| `block.add_column(name, dtype, data)` | Add a column |
| `block.get_column(name)` | Get column by name |

## Data Types

```python
kore.DataType.I64       # 64-bit integer
kore.DataType.F64       # 64-bit float
kore.DataType.STR       # UTF-8 string
kore.DataType.STR_DICT  # Dictionary-encoded string (compressed)
kore.DataType.BOOL      # Boolean
```

## Run Tests

```bash
python -m pytest test_kore_fileformat.py -v
python test_phase3.py
```
