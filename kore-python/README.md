# KORE FileFormat — Python

**Version 1.6.0** | [PyPI](https://pypi.org/project/kore-fileformat/) | [GitHub](https://github.com/arunkatherashala/Kore)

High-performance columnar format with 11 ACID features. Reads/writes via Rust `kore_ffi.dll` through `ctypes`.

## Install

```bash
pip install kore-fileformat==1.6.0
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
