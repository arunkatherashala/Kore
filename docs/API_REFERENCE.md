# 🔧 API Reference - KORE v1.1.4

Complete API documentation for all KORE functions and classes.

---

## Table of Contents
1. [Core Functions](#core-functions)
2. [KoreWriter Class](#korewriter-class)
3. [KoreReader Class](#korereader-class)
4. [Data Types](#data-types)
5. [Error Handling](#error-handling)
6. [Type Hints](#type-hints)

---

## Core Functions

### compress_csv()

Compress a CSV file to KORE format.

#### Signature
```python
def compress_csv(csv_path: str, kore_path: str) -> Tuple[int, int, float]
```

#### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `csv_path` | `str` | Path to input CSV file |
| `kore_path` | `str` | Path to output KORE file |

#### Returns
```python
Tuple[int, int, float]
  # (original_size, compressed_size, compression_ratio)
  # Example: (1000000, 650000, 0.65)
```

#### Examples

**Basic Usage:**
```python
from kore_fileformat import compress_csv

original, compressed, ratio = compress_csv("input.csv", "output.kore")
print(f"Compression: {ratio:.1%}")
```

**With Error Handling:**
```python
try:
    original, compressed, ratio = compress_csv("data.csv", "data.kore")
    print(f"✅ Compressed: {ratio:.1%}")
except FileNotFoundError:
    print("❌ CSV file not found")
except Exception as e:
    print(f"❌ Error: {e}")
```

---

### get_kore_info()

Get metadata about a KORE file.

#### Signature
```python
def get_kore_info(kore_path: str) -> Dict[str, Any]
```

#### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `kore_path` | `str` | Path to KORE file |

#### Returns
```python
Dict[str, Any]:
  {
    "total_records": int,       # Number of records
    "version": str,             # KORE version
    "is_compressed": bool,      # Compression status
    "columns": List[str],       # Column names (if available)
    "file_size": int,           # File size in bytes
    "created_at": str,          # Creation timestamp
  }
```

#### Examples

**Basic Usage:**
```python
from kore_fileformat import get_kore_info

info = get_kore_info("data.kore")
print(f"Records: {info['total_records']}")
print(f"Version: {info['version']}")
print(f"Compressed: {info['is_compressed']}")
```

**With Detailed Analysis:**
```python
info = get_kore_info("data.kore")

print("📊 File Statistics:")
print(f"  Total records:  {info['total_records']:,}")
print(f"  Compressed:     {info['is_compressed']}")
print(f"  KORE version:   {info['version']}")

if 'columns' in info:
    print(f"  Columns:        {', '.join(info['columns'])}")
```

---

## KoreWriter Class

Write data to KORE files.

### Constructor

```python
def __init__(self, output_path: str)
```

#### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `output_path` | `str` | Path to output KORE file |

#### Example
```python
from kore_fileformat import KoreWriter

writer = KoreWriter("output.kore")
```

---

### write()

Write a record to the KORE file.

#### Signature
```python
def write(self, record: Dict[str, Any]) -> None
```

#### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `record` | `Dict[str, Any]` | Data record as dictionary |

#### Returns
- None

#### Example
```python
writer = KoreWriter("data.kore")

# Write single record
writer.write({"id": 1, "name": "Alice", "age": 30})

# Write multiple records
for record in data:
    writer.write(record)
```

---

### close()

Finalize and close the KORE file.

#### Signature
```python
def close(self) -> None
```

#### Example
```python
writer = KoreWriter("data.kore")
writer.write({"id": 1, "value": 100})
writer.close()  # Important: Always close!
```

---

### Context Manager (Recommended)

Use KoreWriter as context manager for automatic cleanup.

#### Signature
```python
with KoreWriter(path) as writer:
    # Write data
    writer.write(record)
# Automatically closed
```

#### Example
```python
from kore_fileformat import KoreWriter

with KoreWriter("data.kore") as writer:
    for record in data:
        writer.write(record)
# File automatically closed and finalized
```

---

## KoreReader Class

Read data from KORE files.

### Constructor

```python
def __init__(self, input_path: str)
```

#### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `input_path` | `str` | Path to KORE file |

#### Example
```python
from kore_fileformat import KoreReader

reader = KoreReader("data.kore")
```

---

### count()

Get total number of records in file.

#### Signature
```python
def count(self) -> int
```

#### Returns
- `int`: Total record count

#### Example
```python
reader = KoreReader("data.kore")
total = reader.count()
print(f"Total records: {total:,}")
```

---

### read()

Read records from the file.

#### Signature
```python
def read(self, limit: Optional[int] = None, offset: int = 0) -> List[Dict[str, Any]]
```

#### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `limit` | `int \| None` | Maximum records to read (None = all) |
| `offset` | `int` | Starting position (default: 0) |

#### Returns
- `List[Dict[str, Any]]`: List of records

#### Examples

**Read All Records:**
```python
reader = KoreReader("data.kore")
all_records = reader.read()
print(f"Read {len(all_records)} records")
```

**Read With Limit:**
```python
reader = KoreReader("data.kore")
first_100 = reader.read(limit=100)
print(f"First 100 records: {len(first_100)}")
```

**Read With Offset:**
```python
reader = KoreReader("data.kore")
records = reader.read(limit=50, offset=100)
print(f"Records 100-150: {len(records)}")
```

**Pagination Pattern:**
```python
reader = KoreReader("data.kore")
page_size = 1000
total = reader.count()

for page in range(0, total, page_size):
    records = reader.read(limit=page_size, offset=page)
    process_batch(records)
```

---

### close()

Close the KORE reader.

#### Signature
```python
def close(self) -> None
```

#### Example
```python
reader = KoreReader("data.kore")
records = reader.read()
reader.close()
```

---

### Context Manager (Recommended)

Use KoreReader as context manager.

#### Example
```python
from kore_fileformat import KoreReader

with KoreReader("data.kore") as reader:
    total = reader.count()
    records = reader.read(limit=100)
# Automatically closed
```

---

## Data Types

### Supported Types

KORE supports the following Python data types:

| Python Type | KORE Type | Notes |
|------------|-----------|-------|
| `int` | Integer | 64-bit signed |
| `float` | Float | 64-bit IEEE 754 |
| `str` | String | UTF-8 encoded |
| `bool` | Boolean | True/False |
| `None` | Null | Missing values |
| `dict` | Struct | Nested objects |
| `list` | Array | Homogeneous lists |

### Type Conversion Examples

```python
# Integer
{"count": 100}           # → int64

# Float
{"temperature": 23.5}    # → float64

# String
{"name": "Alice"}        # → utf8

# Boolean
{"is_active": True}      # → bool

# Null
{"optional_field": None} # → null

# Nested dict
{"metadata": {"key": "value"}}

# List
{"tags": ["a", "b", "c"]}
```

---

## Error Handling

### Common Exceptions

#### FileNotFoundError
```python
from kore_fileformat import compress_csv

try:
    compress_csv("nonexistent.csv", "output.kore")
except FileNotFoundError:
    print("CSV file not found")
```

#### PermissionError
```python
try:
    reader = KoreReader("/root/protected.kore")
except PermissionError:
    print("Access denied")
```

#### ValueError
```python
try:
    writer = KoreWriter("")  # Invalid path
except ValueError:
    print("Invalid output path")
```

### Error Handling Best Practices

```python
from kore_fileformat import compress_csv, KoreReader

# Pattern 1: Try-Except
try:
    original, compressed, ratio = compress_csv("data.csv", "data.kore")
except FileNotFoundError:
    print("Input file not found")
except IOError as e:
    print(f"IO Error: {e}")
except Exception as e:
    print(f"Unexpected error: {e}")

# Pattern 2: Context manager (auto-cleanup)
try:
    with KoreReader("data.kore") as reader:
        records = reader.read(limit=100)
except FileNotFoundError:
    print("KORE file not found")
finally:
    print("Cleanup complete")

# Pattern 3: Validation before operation
import os

if not os.path.exists("data.csv"):
    print("File doesn't exist")
elif not os.path.isfile("data.csv"):
    print("Not a file")
elif os.path.getsize("data.csv") == 0:
    print("File is empty")
else:
    compress_csv("data.csv", "data.kore")
```

---

## Type Hints

All KORE functions use proper type hints for IDE support.

```python
from typing import Dict, List, Tuple, Optional, Any
from kore_fileformat import compress_csv, KoreWriter, KoreReader

# Type hints in action
def process_kore_file(path: str) -> Dict[str, Any]:
    """Process KORE file and return stats."""
    with KoreReader(path) as reader:
        total: int = reader.count()
        records: List[Dict[str, Any]] = reader.read(limit=100)
    
    return {
        "total_records": total,
        "sample_size": len(records),
    }

# Calling with hints
stats: Dict[str, Any] = process_kore_file("data.kore")
```

---

## Complete Example

```python
"""
Complete example using all KORE APIs
"""
from kore_fileformat import compress_csv, get_kore_info, KoreWriter, KoreReader

# 1. Create sample data
print("📝 Step 1: Creating sample data...")
with open("sample.csv", "w") as f:
    f.write("id,name,age,salary\n")
    f.write("1,Alice,30,50000\n")
    f.write("2,Bob,25,45000\n")
    f.write("3,Charlie,35,60000\n")

# 2. Compress CSV
print("🔨 Step 2: Compressing CSV...")
original, compressed, ratio = compress_csv("sample.csv", "sample.kore")
print(f"   Ratio: {ratio:.1%}")

# 3. Get file info
print("📊 Step 3: Reading file info...")
info = get_kore_info("sample.kore")
print(f"   Records: {info['total_records']}")

# 4. Read KORE file
print("📖 Step 4: Reading KORE file...")
with KoreReader("sample.kore") as reader:
    records = reader.read()
    for record in records:
        print(f"   {record}")

# 5. Write new KORE file
print("✍️  Step 5: Writing new KORE file...")
with KoreWriter("sample_output.kore") as writer:
    for record in records:
        record["salary"] *= 1.1  # 10% raise
        writer.write(record)

print("✅ All operations complete!")
```

---

## Performance Notes

### Compression Time
- **Small files** (< 1 MB): ~10-50 ms
- **Medium files** (1-100 MB): ~100-500 ms
- **Large files** (> 100 MB): 1-5 seconds

### Memory Usage
- Typically: 2-3x the file size
- Streaming mode: ~constant memory

### Throughput
- **Writing**: 500-2000 MB/s
- **Reading**: 1000-3000 MB/s
- **Compression**: 500-2000 MB/s

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.1.4 | 2026-05-17 | Current release |
| 1.1.3 | 2026-05-10 | Bug fixes |
| 1.1.0 | 2026-05-01 | Initial release |

---

## Getting Help

### 📚 Documentation
- [Installation Guide](INSTALLATION.md)
- [User Guide](USER_GUIDE.md)
- [Examples](EXAMPLES.md)

### 💬 Support
- GitHub Issues: https://github.com/arunkatherashala/Kore/issues
- Email: arunkatherashala@gmail.com

**Happy coding! 🚀**
