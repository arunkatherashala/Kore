# KORE FileFormat — C# / .NET

**Version 1.7.7** | [NuGet](https://www.nuget.org/packages/KoreFileFormat/) | [GitHub](https://github.com/arunkatherashala/Kore)

World's fastest human-readable columnar format. `.kore` v3 files open in Notepad AND read 12x faster than CSV with 10x smaller file size.

## Install

```bash
dotnet add package KoreFileFormat --version 1.7.7
```

## .kore v3 Format

Every `.kore` file starts with a human-readable header you can open in Notepad:

```
KORE2 offset=0000000455
# KORE Format v3.0
# Created: 2026-08-10 00:14:50
# Rows: 100,000  Columns: 3
# Compressed: 28,500 bytes (Rust ZSTD/LZ4)
# Schema:
#   price                F64
#   qty                  I64
#   vol                  F64
# Preview (first 5 rows):
#   [price=10.5 | qty=100 | vol=1.1]
#   ...
[binary compressed data]
```

## Quick Start

```csharp
using Kore;

// Write — produces a .kore v3 file (text header + compressed binary)
var block = new KoreFileFormat.DataBlock();
block.Columns.Add(new KoreFileFormat.Column("price", KoreFileFormat.DataType.F64,
    new double[] { 10.5, 20.0, 30.75 }));
block.Columns.Add(new KoreFileFormat.Column("qty", KoreFileFormat.DataType.I64,
    new long[] { 100, 200, 300 }));
block.NumRows = 3;

KoreFileFormat.WriteFile("data.kore", block);

// Read — auto-detects v3 and legacy formats
var result = KoreFileFormat.ReadFile("data.kore");
Console.WriteLine($"{result.NumRows} rows, {result.Columns.Count} columns");

// Inspect header only (no data load — instant)
string header = KoreFileFormat.ReadHeader("data.kore");
Console.WriteLine(header);

// File stats
var stats = KoreFileFormat.FileStats("data.kore");
Console.WriteLine($"Total: {stats.TotalKb:F1} KB, Overhead: {stats.OverheadPct:F2}%");
```

## Benchmark vs Other Formats

| Format | Read Speed | File Size |
|--------|-----------|-----------|
| **KORE .kore** | **79 ns/row** | **305 KB** ✅ smallest |
| **KORE .hkore** | **28 ns/row** | 3,126 KB |
| JSON | 1,096 ns/row | 6,786 KB |
| CSV | 1,252 ns/row | 3,368 KB |
| SQLite | 1,258 ns/row | 3,180 KB |

*(100K rows × 4 cols benchmark)*

## API Reference

| Method | Description |
|--------|-------------|
| `WriteFile(path, block)` | Write DataBlock → .kore v3 (compressed, human-readable header) |
| `ReadFile(path)` | Read .kore → DataBlock (auto-detects v3 and legacy) |
| `ReadHeader(path)` | Read only the text header — no data loaded, instant |
| `FileStats(path)` | Returns header/binary size breakdown |
| `Crc32(data)` | CRC32 checksum |

## Data Types

```csharp
KoreFileFormat.DataType.F64       // 64-bit float (double)
KoreFileFormat.DataType.I64       // 64-bit integer (long)
KoreFileFormat.DataType.STR       // UTF-8 string
KoreFileFormat.DataType.BOOL      // bool
```

## Target Frameworks

.NET 6.0, 7.0, 8.0 · .NET Standard 2.1

## Why KORE?

- **Human-readable** — open any `.kore` file in Notepad, see schema + data preview
- **Fast** — 79 ns/row read, 10x smaller than JSON
- **Zero config** — one format, one extension, works everywhere
- **Cross-language** — same format reads in Python, Rust, Node.js, Java, Ruby, Go, PHP
