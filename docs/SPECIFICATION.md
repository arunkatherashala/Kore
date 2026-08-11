# KORE File Format Specification v3.0

## 1. Overview

KORE is a columnar binary file format with built-in ACID transactions, encryption, and human-readable metadata. A single `.kore` file replaces Parquet + Delta Lake/Iceberg.

## 2. File Layout

```
┌─────────────────────────────────────┐
│  Human-Readable Header (UTF-8 text) │  ← Opens in Notepad
├─────────────────────────────────────┤
│  Binary Header                      │
│    Magic: "KORE" (4 bytes)          │
│    Version: u16                     │
│    NumColumns: u32                  │
│    NumRows: u64                     │
│    Flags: u32                       │
├─────────────────────────────────────┤
│  Schema Section                     │
│    Per column:                      │
│      ColumnID: u32                  │
│      NameLen: u16 + Name: UTF-8     │
│      DataType: u8                   │
│      Compression: u8               │
│      StatsOffset: u64              │
├─────────────────────────────────────┤
│  Column Data (compressed)           │
│    Per column:                      │
│      CompressedLen: u32             │
│      RawLen: u32                    │
│      CRC32: u32                     │
│      Data: [u8]                     │
├─────────────────────────────────────┤
│  Column Statistics                  │
│    Per column:                      │
│      Min, Max, NullCount            │
│      Cardinality, BloomFilter       │
├─────────────────────────────────────┤
│  MVCC Version Log (optional)        │
│    VersionID, Timestamp             │
│    BlockOffset, RowCount            │
├─────────────────────────────────────┤
│  Delete Vector (optional)           │
│    Bitmap: [u8]                     │
│    Cardinality: u32                 │
├─────────────────────────────────────┤
│  Footer                             │
│    SchemaOffset: u64                │
│    StatsOffset: u64                 │
│    MVCCOffset: u64                  │
│    TotalSize: u64                   │
│    Magic: "KORE" (4 bytes)          │
└─────────────────────────────────────┘
```

## 3. Data Types

| ID | Name     | Size     | Description                    |
|----|----------|----------|--------------------------------|
| 1  | I64      | 8 bytes  | 64-bit signed integer          |
| 2  | F64      | 8 bytes  | 64-bit IEEE 754 float          |
| 3  | BOOL     | 1 byte   | Boolean (0 or 1)               |
| 4  | STR      | variable | Length-prefixed UTF-8 string    |
| 5  | STR_DICT | 1 byte   | Dictionary-encoded string (u8 code → dict entry, 0xFF = NULL) |
| 6  | ARRAY    | variable | Nested array of any type        |
| 7  | STRUCT   | variable | Nested struct (named fields)    |

## 4. Compression Codecs

| ID | Name    | Algorithm                |
|----|---------|--------------------------|
| 0  | RAW     | No compression           |
| 1  | RLE     | Run-length encoding      |
| 2  | DELTA   | Delta encoding           |
| 3  | DICT    | Dictionary encoding      |
| 4  | NAN_RAW | Special NaN handling     |
| 5  | DEFLATE | LZ4 compression          |
| 6  | ZSTD    | Zstandard compression    |

## 5. ACID Features

### 5.1 CRC32 Checksums
Every column block has a CRC32 checksum for data integrity verification.

### 5.2 Column Statistics
Min, max, null count, cardinality per column. Enables predicate pushdown.

### 5.3 Bloom Filters
Per-column Bloom filter for probabilistic membership testing. Reduces I/O.

### 5.4 AES-256-GCM Encryption
Column-level encryption with password-derived keys (PBKDF2-SHA256).

### 5.5 Schema Evolution
Columns identified by ColumnID (not position). Add/drop columns without rewriting data.

### 5.6 Append Writes
Multiple DataBlocks can be appended to a single file. Each block has its own schema section.

### 5.7 MVCC + Time Travel
Version log tracks snapshots with timestamps. `read_at_version(data, timestamp)` returns historical state.

### 5.8 Partition Evolution
Partition spec tracks column-based partitioning with transforms (identity, year, month, bucket).

### 5.9 Row-Level Deletes
Bitmap-based delete vector for soft deletes without rewriting data.

## 6. Human-Readable Header

Every `.kore` file starts with a UTF-8 text header (before binary data):

```
KORE2 offset=0000000455
# KORE Format v3.0
# Rows: 100,000  Columns: 3
# Compressed: 28,500 bytes (ZSTD)
# Schema:
#   price                F64
#   qty                  I64
#   region               STR
# Preview (first 5 rows):
#   [price=10.5 | qty=100 | region=US]
#   [price=20.0 | qty=200 | region=EU]
```

The `offset=` value points to where binary data starts. Any text editor can display the header.

## 7. Ecosystem Connectors

| System | Integration |
|--------|------------|
| Apache Spark | DataSourceV2 (`spark.read.format("kore")`) |
| Trino/Presto | Connector SPI (`SELECT * FROM kore.default.table`) |
| DuckDB | Arrow bridge (`kore.to_duckdb()`) |
| Polars | Arrow bridge (`kore.to_polars()`) |
| Apache Arrow | RecordBatch ↔ DataBlock (Rust + Python) |
| Pandas | `kore.to_pandas()` |
| Kafka | `kore.to_kafka_message()` / `kore.from_kafka_message()` |

## 8. Language Bindings

Python (PyPI), Rust (crates.io), Java (Maven), Node.js (npm), Ruby (RubyGems), C# (NuGet), Go (pkg.go.dev)

## 9. Wire Format Details

### 9.1 Integer Encoding
All integers are little-endian.

### 9.2 String Encoding
Strings are length-prefixed: `u32 length` followed by UTF-8 bytes. NULL strings have length `0xFFFFFFFF`.

### 9.3 Dictionary Encoding (STR_DICT)
- Dictionary: array of unique strings (length-prefixed)
- Codes: `Vec<u8>` where each byte indexes into the dictionary
- `0xFF` = NULL
- Maximum 254 distinct non-NULL values per column

## 10. Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-06 | Initial format |
| 2.0 | 2026-07 | ACID features, compression |
| 3.0 | 2026-08 | Human-readable header, Spark/Trino connectors, Arrow bridge |
