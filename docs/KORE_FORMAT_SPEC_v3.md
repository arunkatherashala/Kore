# KORE File Format Specification v3.0

**Status:** Official  
**Version:** 3.0  
**Date:** 2026-08-15  
**Author:** Arun Katherashala  
**License:** MIT  

---

## 1. Overview

KORE is a columnar binary file format designed for maximum read speed with optional human-readable metadata. It supports two file extensions:

| Extension | Description |
|-----------|-------------|
| `.kore` | Pure binary columnar format (fastest) |
| `.hkore` | Hybrid format: human-readable text header + binary columnar data |

### Design Goals
1. **Speed** — O(1) column access, zero-copy reads via `array.fromfile` / `mmap`
2. **Human readability** — `.hkore` header visible in any text editor
3. **Zero dependencies** — no JVM, no Spark, no external libraries required
4. **Cross-language** — single binary layout readable from any language via FFI

---

## 2. `.kore` Binary Format

### 2.1 File Layout

```
┌─────────────────────────────────────────────────────────┐
│                    HEADER (18 bytes)                     │
├──────────┬──────────┬───────────┬───────────────────────┤
│ Magic    │ Version  │ Num Cols  │ Num Rows              │
│ 4 bytes  │ 2 bytes  │ 4 bytes   │ 8 bytes               │
│ "KORE"   │ LE u16   │ LE u32    │ LE u64                │
├──────────┴──────────┴───────────┴───────────────────────┤
│                    SCHEMA SECTION                        │
│ Repeated num_cols times:                                 │
│   [name_len: LE u16] [name: UTF-8 bytes] [dtype: u8]    │
├─────────────────────────────────────────────────────────┤
│                  COLUMN DATA SECTION                     │
│ Repeated num_cols times:                                 │
│   [compression: u8] [data_len: LE u64] [data: bytes]    │
├─────────────────────────────────────────────────────────┤
│                  STATS SECTION                           │
│   [stats_len: LE u32] [per-column CRC32 + min/max]      │
├─────────────────────────────────────────────────────────┤
│              OPTIONAL: READABLE TRAILER                  │
│              OPTIONAL: KVER (version snapshot)           │
│              OPTIONAL: KDEL (delete vector)              │
│              OPTIONAL: KENC (encrypted marker)           │
└─────────────────────────────────────────────────────────┘
```

### 2.2 Magic Bytes

| Offset | Length | Value | Description |
|--------|--------|-------|-------------|
| 0 | 4 | `0x4B4F5245` | ASCII "KORE" |

### 2.3 Version

| Offset | Length | Type | Value | Description |
|--------|--------|------|-------|-------------|
| 4 | 2 | LE u16 | 2 | Format version (current: 2) |

### 2.4 Data Types (dtype byte)

| Value | Type | Size per element | Description |
|-------|------|------------------|-------------|
| 1 | I64 | 8 bytes | Signed 64-bit integer, little-endian |
| 2 | F64 | 8 bytes | IEEE 754 double, little-endian |
| 3 | Bool | 1 byte | 0 = false, 1 = true |
| 4 | Str | variable | Length-prefixed UTF-8 string |
| 5 | StrDict | variable | Dictionary-encoded string (u8 codes + dict) |
| 6 | Array | variable | Nested array of homogeneous elements |
| 7 | Struct | variable | Named fields (nested columns) |

### 2.5 Compression Codecs

| Value | Codec | Applicable Types | Description |
|-------|-------|-------------------|-------------|
| 0 | Raw | All | No compression |
| 1 | RLE | I64 | Run-length encoding |
| 2 | Delta | I64 | Delta encoding (store differences) |
| 3 | Dict | F64 | Dictionary encoding for floats |
| 4 | NanRaw | F64 | NaN sentinel for NULL values |
| 5 | LZ4 | All | LZ4 block compression |
| 6 | Zstd | All | Zstandard compression |

### 2.6 String Encoding (dtype = 4)

```
[LE u32: byte_length] [UTF-8 bytes: byte_length]
```

Repeated `num_rows` times per string column.

### 2.7 StrDict Encoding (dtype = 5)

```
[LE u16: dict_size]
[dict_size entries: [LE u32: str_len] [UTF-8 bytes: str_len]]
[num_rows × u8 codes]   (0xFF = NULL)
```

Maximum 254 distinct non-NULL values.

### 2.8 CRC32 Checksums

Each column's compressed data has a CRC32 checksum stored in the stats section.

| Algorithm | Polynomial |
|-----------|-----------|
| CRC32 | IEEE 802.3 (0xEDB88320) |

### 2.9 NULL Representation

| Type | NULL Value |
|------|-----------|
| I64 | `Option<i64>` — separate validity in Rust; `i64::MIN` sentinel via FFI |
| F64 | `Option<f64>` — separate validity in Rust; `NaN` sentinel via FFI |
| Bool | `Option<bool>` — `None` in Rust |
| Str | `Option<String>` — `None` in Rust |
| StrDict | code = `0xFF` |

---

## 3. `.hkore` Hybrid Format

### 3.1 Layout

```
┌─────────────────────────────────────────────────────────┐
│  OFFSET LINE (exactly 24 bytes)                          │
│  "KORE2 offset=OOOOOOOOOO\n"                            │
│  O = 10-digit decimal byte offset to binary section      │
├─────────────────────────────────────────────────────────┤
│  TEXT HEADER (human-readable, UTF-8)                     │
│  # KORE Hybrid Format v2.0                               │
│  # Created: YYYY-MM-DD HH:MM:SS                         │
│  # Rows: N  Columns: M                                  │
│  # Schema:                                               │
│  #   column_name          TYPE                           │
│  # Preview (first K rows):                               │
│  #   [col1=val | col2=val | ...]                         │
├─────────────────────────────────────────────────────────┤
│  BINARY MARKER: b"\x00KORE_RAW_V2\x00"                  │
├─────────────────────────────────────────────────────────┤
│  K2RW HEADER (10 bytes)                                  │
│  [magic: "K2RW" 4 bytes] [num_rows: LE u32] [ncols: u16]│
├─────────────────────────────────────────────────────────┤
│  COLUMN METADATA (per column)                            │
│  [dtype: u8] [name_len: LE u16] [name: UTF-8]           │
├─────────────────────────────────────────────────────────┤
│  COLUMN DATA (per column, sequential)                    │
│  I64: num_rows × 8 bytes (LE i64)                        │
│  F64: num_rows × 8 bytes (LE f64, IEEE 754)              │
│  STR: [LE u32: len] [UTF-8 bytes] repeated num_rows      │
│  LIST: [LE u32: count] [elements...] repeated num_rows   │
└─────────────────────────────────────────────────────────┘
```

### 3.2 Offset Line

The first 24 bytes are always:
```
KORE2 offset=OOOOOOOOOO\n
```
Where `OOOOOOOOOO` is a zero-padded 10-digit decimal number indicating the byte offset where binary data begins. This enables O(1) seek to data.

### 3.3 Text Header

Lines starting with `#` are comments. The text header contains:
- Format version
- Creation timestamp
- Row count and column count
- Schema (column names and types)
- Data preview (first N rows)

This section is **purely informational** and ignored during binary reads.

### 3.4 K2RW Binary Header

| Field | Offset | Type | Description |
|-------|--------|------|-------------|
| Magic | 0 | 4 bytes | ASCII "K2RW" |
| Rows | 4 | LE u32 | Number of rows |
| Columns | 8 | LE u16 | Number of columns |

### 3.5 Column dtype tags (`.hkore`)

| Value | Type | Binary layout |
|-------|------|---------------|
| 0 | F64 | num_rows × 8 bytes (IEEE 754 LE) |
| 1 | I64 | num_rows × 8 bytes (LE i64) |
| 2 | STR | [LE u32: len][bytes] repeated |
| 3 | LIST_I64 | [LE u32: count][count × LE i64] repeated |
| 4 | LIST_F64 | [LE u32: count][count × LE f64] repeated |
| 5 | LIST_STR | [LE u32: count][LE u32: len][bytes]... repeated |

### 3.6 Column Pruning

Readers MAY skip unwanted columns by:
1. Reading the K2RW header to get column metadata
2. For numeric columns: `seek(num_rows × 8)` to skip
3. For string columns: read each `[len]` and `seek(len)` to skip
4. Only deserialize columns matching the requested set

---

## 4. Optional Footer Sections

### 4.1 KVER — Version Snapshot (MVCC)

```
[marker: "KVER" 4 bytes]
[version_id: LE u32]
[timestamp_ns: LE u64]
[row_count: LE u64]
[block_offset: LE u64]
```

### 4.2 KDEL — Delete Vector

```
[marker: "KDEL" 4 bytes]
[cardinality: LE u32]
[timestamp: LE u64]
[bitmap: ceil(num_rows/8) bytes]   (bit=1 → row deleted)
```

### 4.3 KENC — Encrypted File

```
[marker: "KENC" 4 bytes]
[salt_len: LE u16] [salt: bytes]
[nonce_len: LE u16] [nonce: bytes]
[ciphertext: remaining bytes]
```

- Algorithm: AES-256-GCM
- KDF: PBKDF2-HMAC-SHA256 (100,000 iterations)
- The ciphertext, when decrypted, yields a standard `.kore` binary

---

## 5. Streaming Format (`.kore` stream)

### 5.1 Stream Header

```
[magic: "KSTR" 4 bytes]
[num_cols: LE u32]
[per column: [dtype: u8] [name_len: LE u16] [name: UTF-8]]
```

### 5.2 Batch Records

```
[marker: "KBAT" 4 bytes]
[num_rows: LE u32]
[timestamp_ms: LE u64]
[column data: sequential, same as .hkore layout]
```

Batches are appended to the file. Readers process batches sequentially.

---

## 6. Type System

### 6.1 Primitive Types

| Type | Rust | Python | Size |
|------|------|--------|------|
| I64 | `i64` | `int` | 8 bytes |
| F64 | `f64` | `float` | 8 bytes |
| Bool | `bool` | `bool` | 1 byte |
| Str | `String` | `str` | variable |

### 6.2 Complex Types

| Type | Description | Encoding |
|------|-------------|----------|
| StrDict | Dictionary-encoded string | u8 codes + string dictionary |
| List\<T\> | Variable-length array | count + elements |
| Struct | Named fields | nested column encoding |

---

## 7. Compatibility

### 7.1 Backward Compatibility

- Version 2 readers MUST be able to read version 1 files
- New optional sections (KVER, KDEL, KENC) MUST be ignored by older readers
- The footer marker pattern ensures forward compatibility

### 7.2 Byte Order

All multi-byte integers are **little-endian**.

### 7.3 Character Encoding

All strings are **UTF-8** encoded.

---

## 8. Reference Implementations

| Language | Location | Status |
|----------|----------|--------|
| Rust | `kore-store/src/` | Primary (reader, writer, compress) |
| Python | `kore-python/kore_fileformat.py` | FFI bridge + .hkore native |
| Node.js | `kore-node/` | FFI bridge |
| Go | `kore-go/` | FFI bridge |
| Java | `maven/` | FFI bridge |
| C# | `csharp/Kore.FileFormat/` | FFI bridge |
| Ruby | `kore-ruby/` | FFI bridge |
| PHP | `kore-php/` | FFI bridge |

---

## 9. Security Considerations

- **CRC32** checksums protect against accidental corruption (not malicious tampering)
- **AES-256-GCM** encryption (KENC) provides authenticated encryption for sensitive data
- **PBKDF2** with 100,000 iterations for key derivation from passwords
- String fields are length-prefixed to prevent buffer overflows
- Readers MUST validate magic bytes and version before processing

---

## 10. MIME Type

| Type | Value |
|------|-------|
| MIME | `application/x-kore` |
| .hkore MIME | `application/x-kore+hybrid` |

---

*KORE File Format Specification v3.0 — Copyright 2026 Arun Katherashala — MIT License*
