# KORE FileFormat — C# / .NET

**Version 1.6.31** | [NuGet](https://www.nuget.org/packages/KoreFileFormat/) | [GitHub](https://github.com/arunkatherashala/Kore)

High-performance columnar format with 11 ACID features. Uses P/Invoke to call Rust `kore_ffi.dll`.

## Install

```bash
dotnet add package KoreFileFormat --version 1.6.31
```

Or from source:
```bash
cargo build --release -p kore-ffi
# Copy kore_ffi.dll to your project output directory
```

## Quick Start

```csharp
using Kore;

// --- Write ---
var block = new KoreFileFormat.DataBlock();
block.Columns.Add(new KoreFileFormat.Column("price",    KoreFileFormat.DataType.F64, new double[] { 10.5, 20.0, 30.75 }));
block.Columns.Add(new KoreFileFormat.Column("quantity", KoreFileFormat.DataType.I64, new long[]   { 100,  200,  300   }));
block.NumRows = 3;

KoreFileFormat.WriteFile("data.kore", block);

// --- Read ---
var result = KoreFileFormat.ReadFile("data.kore");
Console.WriteLine($"{result.NumRows} rows, {result.Columns.Count} columns");

// --- CRC32 ---
uint checksum = KoreFileFormat.Crc32(System.Text.Encoding.UTF8.GetBytes("hello kore"));
Console.WriteLine($"crc32 = 0x{checksum:x8}");   // 0x4b029b4b
```

## API Reference

| Method | Description |
|--------|-------------|
| `KoreFileFormat.WriteFile(path, block)` | Write DataBlock to .kore binary |
| `KoreFileFormat.ReadFile(path)` | Read .kore → DataBlock |
| `KoreFileFormat.Crc32(data)` | CRC32 checksum |

## Data Types

```csharp
KoreFileFormat.DataType.F64       // double
KoreFileFormat.DataType.I64       // long
KoreFileFormat.DataType.STR       // string
KoreFileFormat.DataType.STR_DICT  // dictionary-encoded string
KoreFileFormat.DataType.BOOL      // bool
```

## Target Frameworks

- .NET 6.0, 7.0, 8.0
- .NET Standard 2.1

## Run Tests

```bash
dotnet test
```
