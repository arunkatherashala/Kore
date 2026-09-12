# KORE File Format (kore-fileformat) — Complete Guide

> **Optional utility library for working with .kore native format, Parquet, Delta Lake, and other columnar formats.**

---

## 📋 Overview

**`kore-fileformat`** is a companion library to `kore-engine` that specializes in:
- ✅ Native `.kore` columnar format (50% smaller, faster than Parquet)
- ✅ Schema inference and management
- ✅ Format conversion (CSV ↔ Parquet ↔ .kore ↔ Delta)
- ✅ Compression utilities (LZ4, Snappy, Zstd, Gzip)
- ✅ Delta Lake metadata handling
- ✅ Streaming I/O for large files

**Use ONLY if you need:** File format conversion, schema management, native .kore optimization

**You DON'T need it for:** Simple SQL queries (just use `kore-engine`)

---

## 🎯 Installation

### Quick Install
```bash
pip install kore-fileformat
```

### With Dependencies
```bash
# For Delta Lake support
pip install kore-fileformat[delta]

# For all compression formats
pip install kore-fileformat[compress]

# Everything
pip install kore-fileformat[all]
```

### Verify
```bash
python3 -c "from kore_fileformat import KoreFileReader; print('✅ Ready')"
```

---

## 🚀 Quick Start

### Read Any File Format
```python
from kore_fileformat import KoreFileReader

# Automatically detects format by extension
reader = KoreFileReader('data.parquet')
data = reader.read_all()

# Or explicit format
reader = KoreFileReader('data.csv', format='csv', delimiter=',')
data = reader.read_all()
```

### Write Any File Format
```python
from kore_fileformat import KoreFileWriter

# Automatically detects format by extension
writer = KoreFileWriter('output.kore')
writer.write(data)

# Or explicit format + compression
writer = KoreFileWriter('output.kore', compression='lz4')
writer.write(data)
```

### Convert Between Formats
```python
from kore_fileformat import KoreFileReader, KoreFileWriter

# Parquet → .kore
reader = KoreFileReader('input.parquet')
writer = KoreFileWriter('output.kore')
writer.write(reader.read_all())

# CSV → Parquet
reader = KoreFileReader('input.csv')
writer = KoreFileWriter('output.parquet')
writer.write(reader.read_all())

# .kore → Delta Lake
reader = KoreFileReader('input.kore')
writer = KoreFileWriter('output_delta', format='delta')
writer.write(reader.read_all())
```

---

## 📁 File Format Reference

### 1. Native .kore Format (Recommended)
**Best for:** Maximum performance, smallest file size

```python
from kore_fileformat import KoreFileWriter

data = get_your_data()  # Your DataBlock

# Write
writer = KoreFileWriter('data.kore')
writer.write(data)

# With compression
writer = KoreFileWriter('data.kore.lz4', compression='lz4')
writer.write(data)

# Read back
from kore_fileformat import KoreFileReader
reader = KoreFileReader('data.kore')
restored = reader.read_all()
```

**Characteristics:**
- Native columnar format (100% compatible with KORE)
- ~50% smaller than Parquet
- ~2x faster reads than Parquet
- Preserves all data types and metadata
- File size: `original_parquet_size / 2`

**Example:**
```
Input:  lineitem.parquet (4.2 GB)
Output: lineitem.kore (2.1 GB)
Speed:  Read 2.1GB in 450ms (4.7 GB/s)
```

### 2. Parquet Format
**Best for:** Compatibility with Spark, Pandas, DuckDB

```python
from kore_fileformat import KoreFileWriter, ParquetSchema

# Write Parquet
writer = KoreFileWriter('data.parquet', 
                       compression='snappy',  # or gzip, zstd
                       row_group_size=65536)
writer.write(data)

# Read Parquet
reader = KoreFileReader('data.parquet')
schema = reader.infer_schema()
data = reader.read_all()
```

**Supported Compressions:**
- `snappy` - Fast, moderate compression
- `gzip` - Slow, high compression
- `zstd` - Balanced (recommended)
- `none` - No compression

### 3. Delta Lake Format
**Best for:** ACID transactions, time travel, production reliability

```python
from kore_fileformat import DeltaTableWriter, DeltaTableReader

# Create Delta table
writer = DeltaTableWriter('warehouse/users')
writer.write(data, mode='create')

# Append to Delta table
writer = DeltaTableWriter('warehouse/users')
writer.write(new_data, mode='append')

# Update Delta table
writer = DeltaTableWriter('warehouse/users')
writer.write(updated_data, mode='overwrite')

# Read current version
reader = DeltaTableReader('warehouse/users')
data = reader.read_all()

# Read historical version
reader = DeltaTableReader('warehouse/users')
data = reader.read_version(version=5)  # Fifth version

# Read as of timestamp
data = reader.read_timestamp('2026-09-10 12:00:00')
```

**Features:**
- ACID guarantees
- Full history tracking
- Time travel queries
- Automatic schema evolution
- Concurrent writes (MVCC)

### 4. CSV Format
**Best for:** Simple data, easy sharing

```python
from kore_fileformat import CsvReader, CsvWriter

# Read CSV
reader = CsvReader('data.csv',
                   delimiter=',',
                   quote_char='"',
                   has_header=True,
                   encoding='utf-8')
schema = reader.infer_schema()
data = reader.read_all()

# Write CSV
writer = CsvWriter('output.csv',
                   delimiter=',',
                   quote_char='"',
                   include_header=True)
writer.write(data)
```

### 5. ORC Format
**Best for:** Hive/Spark compatibility, compression

```python
from kore_fileformat import OrcReader, OrcWriter

# Read ORC
reader = OrcReader('data.orc')
data = reader.read_all()

# Write ORC
writer = OrcWriter('output.orc', compression='zstd')
writer.write(data)
```

### 6. Arrow IPC Format
**Best for:** Zero-copy inter-process communication

```python
from kore_fileformat import ArrowIPCReader, ArrowIPCWriter

# Write Arrow IPC (binary)
writer = ArrowIPCWriter('data.arrow')
writer.write(data)

# Read Arrow IPC
reader = ArrowIPCReader('data.arrow')
data = reader.read_all()
```

---

## 🔧 Schema Management

### Infer Schema
```python
from kore_fileformat import SchemaInferrer

inferrer = SchemaInferrer('data.csv', sample_rows=1000)
schema = inferrer.infer()

print(schema)
# Output:
# Column 'id': INT64
# Column 'name': STRING
# Column 'price': FLOAT64
# Column 'date': DATE
```

### Define Schema Explicitly
```python
from kore_fileformat import Schema, DataType, Field

schema = Schema([
    Field('id', DataType.INT64, nullable=False),
    Field('name', DataType.STRING),
    Field('price', DataType.DECIMAL(10, 2)),
    Field('date', DataType.DATE),
    Field('tags', DataType.LIST(DataType.STRING)),
])

# Use with reader
reader = KoreFileReader('data.csv', schema=schema)
data = reader.read_all()
```

### Evolve Schema
```python
from kore_fileformat import DeltaTableWriter

# Add new column
writer = DeltaTableWriter('warehouse/users')
new_schema = old_schema.add_column(
    Field('email', DataType.STRING, nullable=True)
)
writer.write(data, schema=new_schema)
```

---

## 🗜️ Compression

### Supported Algorithms

| Format | Speed | Compression | Best For |
|--------|-------|-------------|----------|
| `none` | ⚡⚡⚡ | 0% | Uncompressed (fastest) |
| `lz4` | ⚡⚡ | 40-50% | .kore format |
| `snappy` | ⚡ | 40-50% | Parquet |
| `zstd` | ⚡ | 50-60% | Balanced (recommended) |
| `gzip` | 🐢 | 60-70% | Maximum compression |

### Apply Compression
```python
from kore_fileformat import KoreFileWriter

data = get_your_data()

# LZ4 (default for .kore, 40-50% compression)
writer = KoreFileWriter('data.kore.lz4', compression='lz4')
writer.write(data)

# Zstd (best balanced, 50-60% compression)
writer = KoreFileWriter('data.kore.zst', compression='zstd')
writer.write(data)

# Gzip (maximum, slowest)
writer = KoreFileWriter('data.kore.gz', compression='gzip')
writer.write(data)
```

### Compare Compression
```python
import os
from kore_fileformat import KoreFileWriter

data = get_your_data()

# Test all compressions
for algo in ['none', 'lz4', 'zstd', 'gzip']:
    writer = KoreFileWriter(f'data_{algo}.kore', 
                           compression=algo if algo != 'none' else None)
    writer.write(data)
    
    size = os.path.getsize(f'data_{algo}.kore')
    print(f"{algo:8} : {size:12,} bytes")

# Output example:
# none     :     4,294,967,296 bytes (4.3 GB)
# lz4      :     2,097,152,000 bytes (2.1 GB, 51% compression)
# zstd     :     1,912,602,624 bytes (1.9 GB, 55% compression)
# gzip     :     1,288,490,189 bytes (1.3 GB, 70% compression)
```

---

## 📊 Batch Processing

### Process Large Files in Chunks
```python
from kore_fileformat import KoreFileReader

reader = KoreFileReader('huge_file.parquet')

# Read in 1M row chunks
for i, chunk in enumerate(reader.read_chunks(chunk_size=1_000_000)):
    print(f"Processing chunk {i}: {len(chunk)} rows")
    # Process chunk
    process_data(chunk)
```

### Stream Write
```python
from kore_fileformat import KoreFileWriter

writer = KoreFileWriter('output.kore', buffered=True, buffer_size=10_000_000)

# Write in streaming fashion
for batch in data_generator():
    writer.write_batch(batch)

writer.close()  # Finalize file
```

---

## 🔗 Conversion Pipelines

### ETL: CSV → Parquet → .kore
```python
from kore_fileformat import KoreFileReader, KoreFileWriter

# Step 1: Read CSV
reader = KoreFileReader('raw.csv', has_header=True)
schema = reader.infer_schema()
print(f"Inferred schema: {schema}")

# Step 2: Convert to Parquet (for compatibility)
writer = KoreFileWriter('intermediate.parquet')
writer.write(reader.read_all())

# Step 3: Convert to .kore (for performance)
reader2 = KoreFileReader('intermediate.parquet')
writer2 = KoreFileWriter('final.kore.lz4', compression='lz4')
writer2.write(reader2.read_all())

print("✅ Conversion complete")
print(f"  CSV:      raw.csv")
print(f"  Parquet:  intermediate.parquet")
print(f"  .kore:    final.kore.lz4")
```

### Deduplicate and Save
```python
from kore_fileformat import KoreFileReader, KoreFileWriter

reader = KoreFileReader('data.csv')
data = reader.read_all()

# Remove duplicates (using kore-engine)
unique_data = deduplicate(data, key=['id'])

# Save in .kore format
writer = KoreFileWriter('data_unique.kore.zst', compression='zstd')
writer.write(unique_data)
```

---

## 💾 Production Patterns

### Pattern 1: Incremental Imports to Delta
```python
from kore_fileformat import DeltaTableWriter, KoreFileReader
import os

input_dir = '/data/csv_imports'

for csv_file in os.listdir(input_dir):
    if not csv_file.endswith('.csv'):
        continue
    
    # Read CSV
    reader = KoreFileReader(f'{input_dir}/{csv_file}')
    data = reader.read_all()
    
    # Append to Delta table
    writer = DeltaTableWriter('/warehouse/events')
    writer.write(data, mode='append')
    
    print(f"✅ Imported {csv_file}")
```

### Pattern 2: Archive Old Data
```python
from kore_fileformat import DeltaTableReader, KoreFileWriter
from datetime import datetime, timedelta

# Read old data (>30 days)
reader = DeltaTableReader('/warehouse/events')
cutoff = datetime.now() - timedelta(days=30)
old_data = reader.read_before_timestamp(cutoff)

# Archive to compressed .kore
writer = KoreFileWriter(
    f'/archive/events_{cutoff.date()}.kore.gzip',
    compression='gzip'
)
writer.write(old_data)

print(f"✅ Archived {len(old_data)} rows")
```

### Pattern 3: Schema Validation
```python
from kore_fileformat import SchemaValidator, Schema, DataType, Field

# Define expected schema
expected_schema = Schema([
    Field('id', DataType.INT64, nullable=False),
    Field('email', DataType.STRING, nullable=False),
    Field('created_at', DataType.TIMESTAMP, nullable=False),
])

# Validate file
validator = SchemaValidator('data.parquet', expected_schema)
if not validator.validate():
    print("❌ Schema mismatch!")
    print(validator.differences())
else:
    print("✅ Schema valid")
```

---

## 🐛 Troubleshooting

### Problem: "Cannot read .kore file (corrupted?)"
```python
from kore_fileformat import KoreFileValidator

# Validate file integrity
validator = KoreFileValidator('data.kore')
if not validator.is_valid():
    print(f"❌ Corruption detected: {validator.errors()}")
else:
    print("✅ File is valid")
```

### Problem: Out of Memory on Large Files
```python
from kore_fileformat import KoreFileReader

# Process in chunks instead of loading all
reader = KoreFileReader('huge_file.kore')

total_rows = 0
for chunk in reader.read_chunks(chunk_size=1_000_000):
    total_rows += len(chunk)
    process_chunk(chunk)  # Process one chunk at a time

print(f"Processed {total_rows} rows without loading all into RAM")
```

### Problem: Slow Reads from Parquet
```python
from kore_fileformat import KoreFileReader, KoreFileWriter

# Convert to faster .kore format
reader = KoreFileReader('slow.parquet')
writer = KoreFileWriter('fast.kore.lz4', compression='lz4')
writer.write(reader.read_all())

# Now reads are 2x faster
reader2 = KoreFileReader('fast.kore.lz4')
# Read speed: ~4.7 GB/s vs ~2.3 GB/s for Parquet
```

---

## 📖 API Reference

### KoreFileReader
```python
from kore_fileformat import KoreFileReader

reader = KoreFileReader(
    path: str,
    format: str = 'auto',  # auto, csv, parquet, kore, orc, arrow
    schema: Schema = None,  # Optional: enforce schema
    # CSV options
    delimiter: str = ',',
    has_header: bool = True,
    # Reading options
    batch_size: int = 65536,
)

data = reader.read_all()  # Load all
chunks = reader.read_chunks(chunk_size=1_000_000)  # Stream
```

### KoreFileWriter
```python
from kore_fileformat import KoreFileWriter

writer = KoreFileWriter(
    path: str,
    format: str = 'auto',  # auto-detect from extension
    compression: str = None,  # none, lz4, snappy, zstd, gzip
    # Parquet options
    row_group_size: int = 65536,
    # .kore options
    buffered: bool = True,
    buffer_size: int = 10_000_000,
)

writer.write(data)
```

### DeltaTableReader / DeltaTableWriter
```python
from kore_fileformat import DeltaTableReader, DeltaTableWriter

# Reader
reader = DeltaTableReader(path: str)
data = reader.read_all()
data = reader.read_version(version: int)
data = reader.read_timestamp(timestamp: str)

# Writer
writer = DeltaTableWriter(path: str)
writer.write(data, mode='create' | 'append' | 'overwrite')
```

---

## 🎓 Integration with kore-engine

### Use Together
```python
# File utilities for I/O
from kore_fileformat import KoreFileReader, KoreFileWriter

# Engine for queries
from kore_engine import KoreSession

# Workflow:
# 1. Read data with kore-fileformat
reader = KoreFileReader('data.csv')
data = reader.read_all()

# 2. Query with kore-engine
session = KoreSession()
session.load_data('mydata', data)
result = session.query("SELECT * FROM mydata WHERE x > 10")

# 3. Save with kore-fileformat
writer = KoreFileWriter('result.kore.lz4')
writer.write(result)
```

---

## 📞 Support

| Question | Answer |
|----------|--------|
| **Do I need kore-fileformat?** | Only if you do format conversions or need schema management |
| **Can I use it without kore-engine?** | Yes, standalone I/O library |
| **What's the .kore format compatibility?** | 100% with kore-engine v1.8.0+ |
| **Can it read Spark Parquet?** | Yes, fully compatible |
| **Does it support streaming?** | Yes, chunked reading and writing |

---

**Version:** v1.8.0  
**Last Updated:** Sept 11, 2026  
**Companion Package:** `kore-engine`
