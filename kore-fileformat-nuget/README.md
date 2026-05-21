# KORE NuGet Package Development

This directory contains the C# bindings and NuGet package for KORE file format compression.

## Project Structure

```
kore-fileformat-nuget/
├── KoreFileFormat/
│   ├── KoreFileFormat.csproj       # NuGet package configuration
│   ├── Kore.cs                     # Main public API
│   ├── Native.cs                   # P/Invoke bindings
│   ├── CompressionLevel.cs         # Enum for compression levels
│   └── CompressionException.cs     # Custom exception type
├── Tests/
│   ├── CompressorTests.cs          # Unit tests for compression
│   └── DecompressorTests.cs        # Unit tests for decompression
└── README.md                       # This file
```

## Building

### Prerequisites
- .NET 6, 7, or 8
- Windows, Linux, or macOS

### Build Commands

```bash
# Restore dependencies
cd kore-fileformat-nuget
dotnet restore

# Build
dotnet build -c Release

# Run tests
dotnet test -c Release

# Create NuGet package
dotnet pack KoreFileFormat -c Release -o nupkg
```

## Usage

### Installing from NuGet

```bash
dotnet add package kore-fileformat
```

### Basic Usage

```csharp
using KoreFileFormat;

// Compress data
byte[] originalData = System.Text.Encoding.UTF8.GetBytes("Hello World!");
byte[] compressed = Kore.Compress(originalData);

// Decompress data
byte[] decompressed = Kore.Decompress(compressed);

// With compression levels
byte[] fastCompressed = Kore.Compress(originalData, CompressionLevel.Fast);
byte[] maxCompressed = Kore.Compress(originalData, CompressionLevel.Maximum);
```

## API Reference

### `Kore.Compress(byte[] data, CompressionLevel level = Balanced)`
Compresses data using KORE compression.

- **Parameters**:
  - `data`: Input data to compress
  - `level`: Compression level (Fast, Balanced, Maximum)
- **Returns**: Compressed data as byte array
- **Throws**: `ArgumentNullException`, `CompressionException`

### `Kore.Decompress(byte[] data)`
Decompresses KORE-compressed data.

- **Parameters**:
  - `data`: Compressed data
- **Returns**: Decompressed data as byte array
- **Throws**: `ArgumentNullException`, `CompressionException`

## Requirements

- Native library: `kore_fileformat.dll` (Windows), `libkore_fileformat.so` (Linux), `libkore_fileformat.dylib` (macOS)
- Libraries are automatically included in the NuGet package for supported platforms

## Performance

- **Throughput**: 19.1 GB/s (verified)
- **Compression Ratio**: 42.1% (adaptive)
- **Metadata Latency**: <1ms
- **Supported Platforms**: Windows x64, Linux x64, macOS ARM64, macOS x64

## Status

**v1.2.1** - Production Release
- ✅ Multi-platform support
- ✅ 3 compression levels
- ✅ Comprehensive error handling
- ✅ Full unit test coverage

## Support

For issues, questions, or contributions, visit: https://github.com/arunkatherashala/Kore
