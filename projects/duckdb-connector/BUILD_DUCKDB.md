# DuckDB Connector Build Instructions

## Overview
The DuckDB connector extends DuckDB with support for reading Kore compressed files through a table function: `read_kore()`.

## Prerequisites

### System Requirements
- **OS**: Linux, macOS, or Windows with MSVC/MinGW
- **Compiler**: GCC 9+, Clang 10+, or MSVC 2019+
- **CMake**: 3.15+
- **DuckDB**: Development headers for 0.8.0+
- **Kore Library**: Compiled libkore_fileformat

### Installation on Linux

```bash
# Install build tools
sudo apt-get install build-essential cmake git

# Install DuckDB development headers
sudo apt-get install duckdb-dev

# Or compile DuckDB from source:
git clone https://github.com/duckdb/duckdb.git
cd duckdb && make
export DUCKDB_DIR=$(pwd)
```

### Installation on macOS

```bash
# Using Homebrew
brew install cmake duckdb

# Set environment variable
export DUCKDB_DIR=$(brew --prefix duckdb)
```

### Installation on Windows (MSVC)

```powershell
# Install Visual Studio 2019+ with C++ workload
# Install CMake 3.15+
# Download DuckDB precompiled headers or build from source

# Set environment variables in PowerShell:
$env:DUCKDB_DIR = "C:\path\to\duckdb"
$env:CMAKE_PREFIX_PATH = "C:\path\to\duckdb"
```

## Build Instructions

### Standard Release Build

```bash
cd projects/duckdb-connector
mkdir build && cd build
cmake .. -DDUCKDB_DIR=$DUCKDB_DIR -DKORE_LIB_DIR=../../target/release -DCMAKE_BUILD_TYPE=Release
make -j$(nproc)  # Linux/macOS
# OR
cmake --build . --config Release -j  # Windows
```

### Output Files
- **Linux/macOS**: `lib/kore_extension.so`
- **Windows**: `bin/Release/kore_extension.dll`

### Installation

```bash
# Copy to DuckDB extensions directory
mkdir -p ~/.duckdb/extensions
cp build/lib/kore_extension.* ~/.duckdb/extensions/

# Or set DuckDB_CUSTOM_EXTENSIONS environment variable:
export DuckDB_CUSTOM_EXTENSIONS="./build/lib"
```

## Usage in DuckDB

```sql
-- Load the extension (auto-loads from extensions directory)
LOAD 'kore';

-- Read a Kore file
SELECT * FROM read_kore('data/file.kore');

-- With filtering (pushes down to Kore reader)
SELECT id, name FROM read_kore('data/file.kore') 
WHERE id > 1000;

-- Schema inference
DESCRIBE read_kore('data/file.kore');
```

## Architecture

### Components

1. **CMakeLists.txt**: Finds DuckDB and Kore libraries, configures C++ 17 compilation
2. **kore_extension.cpp**: Main extension entry point, registers read_kore() function
3. **kore_reader.cpp**: Reads Kore file headers, metadata, and row data
4. **kore_file_parser.cpp**: Utility functions for byte parsing and decompression

### Data Flow

```
DuckDB Query
    ↓
read_kore() TableFunction
    ↓
KoreBind() → Opens file, validates format, extracts schema
    ↓
KoreRead() → Reads partition of rows into DataChunk
    ↓
Decompressor (Codecs 0-6) → Reconstructs original values
    ↓
DataChunk returned to DuckDB
```

### Supported Data Types

| Kore Type | DuckDB Type |
|-----------|------------|
| i64 (type 0) | BIGINT |
| f64 (type 1) | DOUBLE |
| string (type 2) | VARCHAR |
| bool (type 3) | BOOLEAN |
| bytes (type 4) | BLOB |

### Supported Codecs

| ID | Name | Details |
|----|------|---------|
| 0 | None | No compression |
| 1 | RLE | Run-length encoding |
| 2 | Dictionary | Dictionary encoding |
| 3 | FOR | Frame-of-reference encoding |
| 4 | LZSS | LZSS compression |
| 5 | EnhancedDictionary | Multi-level dictionary |
| 6 | DoubleDelta | Sorted numeric delta |

## Troubleshooting

### CMake Issues

```
CMake Error: Could not find DuckDB
```
**Solution**: Set `DUCKDB_DIR` to DuckDB installation root:
```bash
cmake .. -DDUCKDB_DIR=/usr/include/duckdb -DKORE_LIB_DIR=../../target/release
```

### Compilation Errors

```
error: undefined reference to 'kore_reader_open'
```
**Solution**: Ensure Kore library is built:
```bash
cd ../.. && cargo build --release
```

### Extension Loading Fails

```
Error: Extension 'kore' not found
```
**Solution**: Verify extension location:
```bash
ls -la ~/.duckdb/extensions/
# OR copy build output
cp build/lib/kore_extension.* ~/.duckdb/extensions/
```

## Development

### Adding Compression Codec Support

1. Update `kore_file_parser.cpp::Decompress()` with new codec case
2. Add codec ID mapping in header
3. Test with benchmark suite

### Performance Optimization

- Use `PRAGMA threads=4` for parallel execution
- Enable filter pushdown in `KoreBind()`
- Profile with `PRAGMA explain_output='all'`

## Integration with CI/CD

### GitHub Actions Example

```yaml
- name: Build DuckDB Connector
  run: |
    sudo apt-get install cmake duckdb-dev
    cd projects/duckdb-connector
    mkdir build && cd build
    cmake .. -DCMAKE_BUILD_TYPE=Release
    make -j$(nproc)
    make test
```

## License
Apache License 2.0 (matching Kore main project)
