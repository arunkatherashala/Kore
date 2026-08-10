# KORE FileFormat — Go

**Version 1.6.28** | [pkg.go.dev](https://pkg.go.dev/github.com/arunkatherashala/kore/kore-go) | [GitHub](https://github.com/arunkatherashala/Kore)

High-performance columnar format with 11 ACID features. Uses CGo to call Rust `kore_ffi.dll`/`libkore_ffi.so`.

## Install

```bash
go get github.com/arunkatherashala/kore/kore-go@v1.6.28
```

Requires `kore_ffi` library in `LD_LIBRARY_PATH` (Linux) or same directory (Windows):
```bash
cargo build --release -p kore-ffi
export LD_LIBRARY_PATH=$PWD/target/release:$LD_LIBRARY_PATH
```

## Quick Start

```go
package main

import (
    "fmt"
    "log"
    kore "github.com/arunkatherashala/kore/kore-go"
)

func main() {
    // --- Write ---
    block := kore.NewDataBlock()
    block.AddColumn("price",    kore.F64, []float64{10.5, 20.0, 30.75})
    block.AddColumn("quantity", kore.I64, []int64{100, 200, 300})

    if err := kore.WriteFile("data.kore", block); err != nil {
        log.Fatal(err)
    }

    // --- Read ---
    result, err := kore.ReadFile("data.kore")
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("%d rows, %d columns\n", result.NumRows, result.NumColumns())

    // --- CRC32 ---
    checksum := kore.CRC32([]byte("hello kore"))
    fmt.Printf("crc32 = 0x%08x\n", checksum)   // 0x4b029b4b
}
```

## API Reference

| Function | Description |
|----------|-------------|
| `WriteFile(path, block)` | Write DataBlock to .kore binary |
| `ReadFile(path)` | Read .kore → DataBlock |
| `CRC32(data []byte)` | CRC32 checksum |
| `NewDataBlock()` | Create empty block |
| `block.AddColumn(name, dtype, data)` | Add column |
| `block.GetColumn(name)` | Get column by name |

## Data Types

```go
kore.F64      // float64
kore.I64      // int64
kore.STR      // string
kore.STR_DICT // dictionary-encoded string
kore.BOOL     // bool
```

## Run Tests

```bash
go test ./...
```
