# KORE FileFormat — User Guide v1.8.0

## Quick Start

### Python
```bash
pip install kore-fileformat
```
```python
import kore_fileformat as kore

# Write
block = kore.DataBlock()
block.add_column("price", kore.DataType.F64, [10.5, 20.0, 30.75])
block.add_column("qty",   kore.DataType.I64, [100, 200, 300])
kore.write_file("data.kore", block)

# Read
result = kore.read_file("data.kore")
print(result.column_names())
print(result.get_column("price").data)
```

### Node.js
```bash
npm install kore-fileformat
```
```javascript
const kore = require('kore-fileformat');
kore.write('data.kore', { price: [10.5, 20.0], qty: [100, 200] });
const data = kore.read('data.kore');
```

### Rust
```bash
cargo add kore_fileformat
```
```rust
use kore_store::{KoreWriter, KoreReader};
use kore_core::{DataBlock, Column, ColumnData};

let block = DataBlock::new(vec![
    Column { name: "price".into(), data: ColumnData::Float64(vec![Some(10.5), Some(20.0)]) },
]);
KoreWriter::write_file(Path::new("data.kore"), &block).unwrap();
let result = KoreReader::read_file(Path::new("data.kore")).unwrap();
```

### Go
```bash
go get github.com/arunkatherashala/Kore/kore-go
```

### Java (Maven)
```xml
<dependency>
    <groupId>com.github.arunkatherashala</groupId>
    <artifactId>kore-fileformat</artifactId>
    <version>1.8.0</version>
</dependency>
```

### C# (NuGet)
```bash
dotnet add package KoreFileFormat
```

### Ruby
```bash
gem install kore-fileformat
```

### PHP
```bash
composer require arunkatherashala/kore-fileformat
```

---

## API Reference

### Python SDK

| Function | Description |
|----------|-------------|
| `DataBlock()` | Create empty data block |
| `block.add_column(name, type, data)` | Add a column |
| `write_file(path, block)` | Write .kore file |
| `read_file(path)` | Read .kore file |
| `KoreWriter(path).write_csv(csv_path)` | Convert CSV to .kore |
| `KoreReader(path).read_columns()` | Read as dict |
| `KoreReader(path).column_names()` | Get column names |
| `KoreReader(path).shape()` | Get (rows, cols) |

### Data Types

| Type | Python | Rust | Description |
|------|--------|------|-------------|
| I64 | `int` | `i64` | 64-bit integer |
| F64 | `float` | `f64` | 64-bit float |
| STR | `str` | `String` | UTF-8 string |
| BOOL | `bool` | `bool` | Boolean |

---

## Performance

### Benchmark Results (10M rows x 30 cols)

| Format | Read | Size | Human Readable |
|--------|------|------|----------------|
| **KORE** | **2,225ms** | **1,423MB** | Schema header |
| Parquet (zstd) | 2,646ms | 1,505MB | No |
| ORC | 3,735ms | 1,665MB | No |

### Stress Test Results

| Scale | Status | Speed |
|-------|--------|-------|
| 1M rows | PASS | 34ms read |
| 10M rows | PASS | 2.2s read |
| 33M rows (single file) | PASS | Linear |
| 100M rows (streaming) | PASS | 8M rows/sec |
| 1 BILLION rows | PASS | 8M rows/sec |

---

## File Format

### .kore Binary Layout
```
[MAGIC: "KORE" 4 bytes]
[VERSION: u16]
[NUM_COLS: u32]
[NUM_ROWS: u64]
[SCHEMA: per column name + type]
[COLUMN DATA: compressed binary per column]
[STATS: CRC32 checksums]
```

### Compression
- Automatic codec selection (LZ4 vs Zstd)
- Dictionary encoding for low-cardinality strings
- Delta encoding for sorted integers
- RLE for repeated values

### Features
- Column pruning (read only needed columns)
- NULL support (Option<T>)
- CRC32 integrity checksums
- Schema evolution (via kore-iceberg)
- Time travel (via kore-delta)
- ACID transactions
- AES-256-GCM encryption

---

## Tools

### CLI
```bash
python kore_convert.py input.csv output.kore        # Convert CSV → .kore
python kore_convert.py input.kore output.json       # Convert .kore → JSON
```

### Inspect
```python
import kore_py
d = kore_py.read_kore("data.kore")
print(d.num_rows(), d.num_columns())
print(d.column_names())
print(d.get_f64_column("price")[:10])
```

### Web Viewer
Open `docs/viewer.html` in browser, drag-drop .kore file.

### MCP Server (AI Integration)
```bash
python kore_mcp_server.py
```
AI agents can query .kore files via Model Context Protocol.

---

## Architecture

```
User Application
    ↓
kore_py (PyO3 Rust binding)  ← Python users
    ↓
kore-store (Rust)            ← Core read/write/compress
    ↓
kore-core (Rust)             ← DataBlock, Column, Types
```

All processing happens in Rust. Python/Node/Go/Java are thin FFI wrappers.

---

## License

MIT License — free to use, modify, distribute.

## Links

- GitHub: https://github.com/arunkatherashala/Kore
- PyPI: https://pypi.org/project/kore-fileformat/
- npm: https://www.npmjs.com/package/kore-fileformat
- crates.io: https://crates.io/crates/kore_fileformat
- Format Spec: docs/KORE_FORMAT_SPEC_v3.md
