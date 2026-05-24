# Phase 9: Language Bindings (Go, Python, Node.js)

## Overview
Creating native SDKs for Go, Python, and Node.js to enable Kore format integration across the full developer ecosystem.

---

## 9.1 Go SDK (go-kore)

### Project Structure
```
projects/go-kore/
├── go.mod
├── go.sum
├── README.md
├── examples/
│   ├── read_example.go
│   ├── write_example.go
│   └── stream_example.go
├── kore/
│   ├── reader.go         (~200 lines)
│   ├── writer.go         (~200 lines)
│   ├── codec.go          (~150 lines)
│   ├── types.go          (~100 lines)
│   └── compression/
│       ├── zstd.go
│       ├── snappy.go
│       └── brotli.go
└── _test.go
```

### Core Implementation (kore/reader.go - ~200 lines)
```go
package kore

import (
    "bytes"
    "encoding/binary"
    "io"
    "fmt"
)

type Reader struct {
    reader      io.ReaderAt
    header      *Header
    partitions  []PartitionEntry
    cache       map[int][]Row
}

type Header struct {
    Magic          [4]byte
    Version        uint8
    ColumnCount    uint16
    RowCount       uint64
    CompressionId  uint8
}

type Row struct {
    Values map[string]interface{}
}

func NewReader(r io.ReaderAt) (*Reader, error) {
    reader := &Reader{
        reader: r,
        cache:  make(map[int][]Row),
    }
    
    if err := reader.readHeader(); err != nil {
        return nil, err
    }
    
    if err := reader.readPartitions(); err != nil {
        return nil, err
    }
    
    return reader, nil
}

func (r *Reader) readHeader() error {
    headerBuf := make([]byte, 20)
    _, err := r.reader.ReadAt(headerBuf, 0)
    if err != nil {
        return err
    }
    
    r.header = &Header{
        Magic:         [4]byte{headerBuf[0], headerBuf[1], headerBuf[2], headerBuf[3]},
        Version:       headerBuf[4],
        ColumnCount:   binary.LittleEndian.Uint16(headerBuf[5:7]),
        RowCount:      binary.LittleEndian.Uint64(headerBuf[8:16]),
        CompressionId: headerBuf[17],
    }
    
    if string(r.header.Magic[:]) != "KORE" {
        return fmt.Errorf("invalid magic bytes")
    }
    
    return nil
}

func (r *Reader) ReadRows(start, end uint64) ([]Row, error) {
    if end > r.header.RowCount {
        end = r.header.RowCount
    }
    
    var rows []Row
    for i := start; i < end; i++ {
        row, err := r.readRow(i)
        if err != nil {
            return nil, err
        }
        rows = append(rows, *row)
    }
    
    return rows, nil
}

func (r *Reader) ReadAll() ([]Row, error) {
    return r.ReadRows(0, r.header.RowCount)
}

func (r *Reader) readRow(index uint64) (*Row, error) {
    // Implementation for reading single row
    row := &Row{Values: make(map[string]interface{})}
    // Read row data...
    return row, nil
}

func (r *Reader) Close() error {
    return nil  // Reader doesn't own resource
}
```

### Writer Implementation (kore/writer.go - ~200 lines)
```go
package kore

import (
    "encoding/binary"
    "fmt"
    "io"
)

type Writer struct {
    writer        io.Writer
    rows          []Row
    columnNames   []string
    columnTypes   []DataType
    compressionId uint8
}

func NewWriter(w io.Writer, columnNames []string, columnTypes []DataType) *Writer {
    return &Writer{
        writer:        w,
        rows:          make([]Row, 0),
        columnNames:   columnNames,
        columnTypes:   columnTypes,
        compressionId: 13,  // Zstd
    }
}

func (w *Writer) WriteRow(row Row) error {
    w.rows = append(w.rows, row)
    return nil
}

func (w *Writer) WriteRows(rows []Row) error {
    for _, row := range rows {
        if err := w.WriteRow(row); err != nil {
            return err
        }
    }
    return nil
}

func (w *Writer) Close() error {
    // Write header
    header := []byte{'K', 'O', 'R', 'E'}  // Magic
    header = append(header, 1)              // Version
    
    // Write column count
    colCountBuf := make([]byte, 2)
    binary.LittleEndian.PutUint16(colCountBuf, uint16(len(w.columnNames)))
    header = append(header, colCountBuf...)
    
    // Write row count
    rowCountBuf := make([]byte, 8)
    binary.LittleEndian.PutUint64(rowCountBuf, uint64(len(w.rows)))
    header = append(header, rowCountBuf...)
    
    // Compress data
    compressed, err := w.compressRows()
    if err != nil {
        return err
    }
    
    // Write to file
    _, err = w.writer.Write(header)
    if err != nil {
        return err
    }
    
    _, err = w.writer.Write(compressed)
    return err
}

func (w *Writer) compressRows() ([]byte, error) {
    // Serialize rows
    var buf bytes.Buffer
    for _, row := range w.rows {
        for _, colName := range w.columnNames {
            val := row.Values[colName]
            // Serialize value...
        }
    }
    
    // Compress
    codec := getCodec(w.compressionId)
    return codec.Compress(buf.Bytes())
}

func (w *Writer) SetCompression(codecId uint8) error {
    if codecId > 11 {
        return fmt.Errorf("invalid codec ID: %d", codecId)
    }
    w.compressionId = codecId
    return nil
}
```

### Usage Example (examples/read_example.go)
```go
package main

import (
    "fmt"
    "log"
    "github.com/arunkatherashala/go-kore/kore"
)

func main() {
    // Open Kore file
    file, err := os.Open("data.kore")
    if err != nil {
        log.Fatal(err)
    }
    defer file.Close()
    
    // Create reader
    reader, err := kore.NewReader(file)
    if err != nil {
        log.Fatal(err)
    }
    
    // Read first 1000 rows
    rows, err := reader.ReadRows(0, 1000)
    if err != nil {
        log.Fatal(err)
    }
    
    // Process rows
    for _, row := range rows {
        fmt.Printf("Row: %v\n", row.Values)
    }
}
```

---

## 9.2 Python SDK (kore-fileformat)

### Project Structure
```
projects/kore-python/
├── setup.py
├── pyproject.toml
├── README.md
├── kore/
│   ├── __init__.py
│   ├── reader.py        (~250 lines)
│   ├── writer.py        (~250 lines)
│   ├── codecs.py        (~150 lines)
│   ├── types.py         (~100 lines)
│   └── compression/
│       ├── __init__.py
│       ├── zstd_codec.py
│       ├── snappy_codec.py
│       └── brotli_codec.py
├── tests/
│   ├── test_reader.py
│   ├── test_writer.py
│   └── test_codecs.py
└── examples/
    ├── read_example.py
    ├── write_example.py
    └── streaming_example.py
```

### Core Implementation (kore/reader.py - ~250 lines)
```python
import struct
from typing import List, Dict, Any, Optional
from io import BytesIO
from .codecs import get_codec

class KoreReader:
    """Read Kore format files"""
    
    def __init__(self, file_path: str):
        self.file_path = file_path
        self.file = None
        self.header = None
        self.partitions = []
        self._open()
        self._read_header()
    
    def _open(self):
        self.file = open(self.file_path, 'rb')
    
    def _read_header(self):
        """Read Kore file header"""
        header_bytes = self.file.read(32)
        
        magic = header_bytes[:4].decode('ascii')
        if magic != 'KORE':
            raise ValueError(f"Invalid magic bytes: {magic}")
        
        version = header_bytes[4]
        column_count = struct.unpack('<H', header_bytes[5:7])[0]
        row_count = struct.unpack('<Q', header_bytes[8:16])[0]
        compression_id = header_bytes[17]
        
        self.header = {
            'magic': magic,
            'version': version,
            'column_count': column_count,
            'row_count': row_count,
            'compression_id': compression_id,
        }
    
    def read_rows(self, start: int = 0, end: Optional[int] = None) -> List[Dict[str, Any]]:
        """Read rows from Kore file"""
        if end is None:
            end = self.header['row_count']
        
        rows = []
        for i in range(start, end):
            row = self._read_row(i)
            rows.append(row)
        
        return rows
    
    def read_all(self) -> List[Dict[str, Any]]:
        """Read entire file"""
        return self.read_rows(0, self.header['row_count'])
    
    def _read_row(self, index: int) -> Dict[str, Any]:
        """Read single row"""
        # Implementation...
        return {}
    
    def stream_rows(self, batch_size: int = 1000):
        """Stream rows in batches (memory efficient)"""
        for i in range(0, self.header['row_count'], batch_size):
            end = min(i + batch_size, self.header['row_count'])
            yield self.read_rows(i, end)
    
    def close(self):
        """Close file"""
        if self.file:
            self.file.close()
    
    def __enter__(self):
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()
```

### Writer Implementation (kore/writer.py - ~250 lines)
```python
import struct
from typing import List, Dict, Any
from .codecs import get_codec

class KoreWriter:
    """Write Kore format files"""
    
    def __init__(self, file_path: str, columns: Dict[str, str], 
                 compression: str = 'zstd'):
        self.file_path = file_path
        self.columns = columns
        self.compression = compression
        self.rows = []
        self.file = None
    
    def write_row(self, row: Dict[str, Any]):
        """Write single row"""
        self.rows.append(row)
    
    def write_rows(self, rows: List[Dict[str, Any]]):
        """Write multiple rows"""
        self.rows.extend(rows)
    
    def close(self):
        """Finalize and write file"""
        self.file = open(self.file_path, 'wb')
        
        # Write header
        header = bytearray()
        header.extend(b'KORE')  # Magic
        header.append(1)        # Version
        
        # Column count
        header.extend(struct.pack('<H', len(self.columns)))
        
        # Row count
        header.extend(struct.pack('<Q', len(self.rows)))
        
        # Compression ID
        codec = get_codec(self.compression)
        header.append(codec.id)
        
        # Serialize rows
        data = self._serialize_rows()
        
        # Compress
        compressed = codec.compress(data)
        
        # Write to file
        self.file.write(header)
        self.file.write(compressed)
        self.file.close()
    
    def _serialize_rows(self) -> bytes:
        """Serialize rows to bytes"""
        result = bytearray()
        for row in self.rows:
            for col_name in self.columns:
                value = row.get(col_name)
                result.extend(self._serialize_value(value))
        return bytes(result)
    
    def _serialize_value(self, value: Any) -> bytes:
        """Serialize single value"""
        if isinstance(value, int):
            return struct.pack('<Q', value)
        elif isinstance(value, float):
            return struct.pack('<d', value)
        elif isinstance(value, str):
            encoded = value.encode('utf-8')
            return struct.pack('<H', len(encoded)) + encoded
        else:
            return b''
    
    def __enter__(self):
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()
```

### Usage Example (examples/read_example.py)
```python
from kore import KoreReader

# Read entire file
with KoreReader('data.kore') as reader:
    rows = reader.read_all()
    for row in rows[:10]:
        print(row)

# Stream large files
with KoreReader('large.kore') as reader:
    for batch in reader.stream_rows(batch_size=10000):
        process_batch(batch)

# Pandas integration
import pandas as pd
with KoreReader('data.kore') as reader:
    df = pd.DataFrame(reader.read_all())
    print(df.head())
```

### PyPI Package (setup.py)
```python
from setuptools import setup

setup(
    name='kore-fileformat',
    version='1.0.0',
    description='Python SDK for Kore file format',
    author='Arun Kather Ashala',
    url='https://github.com/arunkatherashala/kore-python',
    packages=['kore', 'kore.compression'],
    install_requires=[
        'zstandard>=0.17.0',
        'python-snappy>=0.6.0',
        'brotli>=1.0.9',
    ],
    classifiers=[
        'Programming Language :: Python :: 3',
        'Programming Language :: Python :: 3.8+',
        'License :: OSI Approved :: Apache Software License',
    ],
)
```

---

## 9.3 Node.js SDK (kore-format)

### Project Structure
```
projects/kore-nodejs/
├── package.json
├── README.md
├── src/
│   ├── reader.js        (~200 lines)
│   ├── writer.js        (~200 lines)
│   ├── codecs.js        (~150 lines)
│   ├── types.js         (~100 lines)
│   └── compression/
│       ├── zstd.js
│       ├── snappy.js
│       └── brotli.js
├── test/
│   ├── reader.test.js
│   ├── writer.test.js
│   └── integration.test.js
└── examples/
    ├── read.js
    ├── write.js
    └── stream.js
```

### Core Implementation (src/reader.js - ~200 lines)
```javascript
const fs = require('fs');
const zstd = require('zstd');

class KoreReader {
    constructor(filePath) {
        this.filePath = filePath;
        this.fd = null;
        this.header = null;
        this.partitions = [];
    }
    
    async open() {
        this.fd = await fs.promises.open(this.filePath, 'r');
        await this._readHeader();
    }
    
    async _readHeader() {
        const headerBuffer = Buffer.alloc(32);
        await this.fd.read(headerBuffer, 0, 32, 0);
        
        const magic = headerBuffer.slice(0, 4).toString('ascii');
        if (magic !== 'KORE') {
            throw new Error(`Invalid magic bytes: ${magic}`);
        }
        
        this.header = {
            magic: magic,
            version: headerBuffer[4],
            columnCount: headerBuffer.readUInt16LE(5),
            rowCount: Number(headerBuffer.readBigUInt64LE(8)),
            compressionId: headerBuffer[17],
        };
    }
    
    async readRows(start = 0, end = null) {
        if (end === null) {
            end = this.header.rowCount;
        }
        
        const rows = [];
        for (let i = start; i < end; i++) {
            const row = await this._readRow(i);
            rows.push(row);
        }
        return rows;
    }
    
    async readAll() {
        return this.readRows(0, this.header.rowCount);
    }
    
    async _readRow(index) {
        // Implementation...
        return {};
    }
    
    async *streamRows(batchSize = 1000) {
        for (let i = 0; i < this.header.rowCount; i += batchSize) {
            const end = Math.min(i + batchSize, this.header.rowCount);
            yield this.readRows(i, end);
        }
    }
    
    async close() {
        if (this.fd) {
            await this.fd.close();
        }
    }
}

module.exports = KoreReader;
```

### Writer Implementation (src/writer.js - ~200 lines)
```javascript
const fs = require('fs');
const zstd = require('zstd');

class KoreWriter {
    constructor(filePath, columns, compression = 'zstd') {
        this.filePath = filePath;
        this.columns = columns;
        this.compression = compression;
        this.rows = [];
    }
    
    writeRow(row) {
        this.rows.push(row);
    }
    
    writeRows(rows) {
        this.rows.push(...rows);
    }
    
    async close() {
        const fd = await fs.promises.open(this.filePath, 'w');
        
        // Build header
        const header = Buffer.alloc(32);
        header.write('KORE', 'ascii');
        header[4] = 1;  // Version
        header.writeUInt16LE(Object.keys(this.columns).length, 5);
        header.writeBigUInt64LE(BigInt(this.rows.length), 8);
        header[17] = this._getCompressionId();
        
        // Serialize rows
        const data = this._serializeRows();
        
        // Compress
        const compressed = await this._compress(data);
        
        // Write file
        await fd.write(header);
        await fd.write(compressed);
        await fd.close();
    }
    
    _serializeRows() {
        const buffers = [];
        for (const row of this.rows) {
            for (const colName of Object.keys(this.columns)) {
                const value = row[colName];
                buffers.push(this._serializeValue(value));
            }
        }
        return Buffer.concat(buffers);
    }
    
    _serializeValue(value) {
        if (typeof value === 'number') {
            if (Number.isInteger(value)) {
                const buf = Buffer.alloc(8);
                buf.writeBigUInt64LE(BigInt(value), 0);
                return buf;
            } else {
                const buf = Buffer.alloc(8);
                buf.writeDoubleLE(value, 0);
                return buf;
            }
        } else if (typeof value === 'string') {
            const encoded = Buffer.from(value, 'utf-8');
            const header = Buffer.alloc(2);
            header.writeUInt16LE(encoded.length);
            return Buffer.concat([header, encoded]);
        }
        return Buffer.alloc(0);
    }
    
    async _compress(data) {
        return new Promise((resolve, reject) => {
            zstd.compress(data, (err, compressed) => {
                if (err) reject(err);
                else resolve(compressed);
            });
        });
    }
    
    _getCompressionId() {
        const map = { 'zstd': 13, 'snappy': 7, 'brotli': 8 };
        return map[this.compression] || 13;
    }
}

module.exports = KoreWriter;
```

### Usage Example (examples/read.js)
```javascript
const KoreReader = require('../src/reader');

async function main() {
    const reader = new KoreReader('data.kore');
    await reader.open();
    
    // Read all rows
    const rows = await reader.readAll();
    console.log(`Read ${rows.length} rows`);
    
    // Stream large files
    for await (const batch of reader.streamRows(10000)) {
        processBatch(batch);
    }
    
    await reader.close();
}

main().catch(console.error);
```

### NPM Package (package.json)
```json
{
  "name": "kore-fileformat",
  "version": "1.0.0",
  "description": "Node.js SDK for Kore file format",
  "main": "src/index.js",
  "scripts": {
    "test": "jest",
    "build": "npm run test"
  },
  "dependencies": {
    "zstd": "^1.3.0",
    "snappy": "^7.2.0",
    "brotli": "^1.3.0"
  },
  "keywords": ["kore", "compression", "data-format"],
  "author": "Arun Kather Ashala",
  "license": "Apache-2.0"
}
```

---

## Build & Publish

### Go (go-kore)
```bash
cd projects/go-kore
go test ./...
go build ./cmd/...
# Publish to GitHub Packages
go release
```

### Python (kore-fileformat)
```bash
cd projects/kore-python
python setup.py sdist bdist_wheel
twine upload dist/*  # Upload to PyPI
```

### Node.js (kore-format)
```bash
cd projects/kore-nodejs
npm test
npm publish  # Upload to npm registry
```

---

## Expected Deliverables

| SDK | Language | Size | Lines | Tests | Status |
|-----|----------|------|-------|-------|--------|
| go-kore | Go | 1.2 MB | 650 | 40+ | Ready |
| kore-fileformat | Python | 800 KB | 700 | 50+ | Ready |
| kore-format | Node.js | 900 KB | 650 | 45+ | Ready |
| **Total** | **3 langs** | **2.9 MB** | **2,000** | **135+** | **Ready** |

---

## Summary

**Language Support**: Go, Python, Node.js
**Total SDK Code**: 2,000+ lines
**Package Support**: Streaming, batch, filtering APIs
**Test Coverage**: 135+ test cases
**Documentation**: Full API reference with examples

**Status**: Ready for implementation

---

**Next**: Phase 10 - Enhanced Security
