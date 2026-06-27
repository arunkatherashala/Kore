# Kore Go Bindings

Pure Go implementation of the **Kore columnar file format** with zero external dependencies.

## Features

- ✅ **Read/Write Kore files** - Direct file I/O operations
- ✅ **Column-based access** - Efficient columnar data retrieval
- ✅ **Zero dependencies** - Pure Go standard library
- ✅ **Production-ready** - Handles multi-chunk files, encoding detection
- ✅ **Type-safe** - Go type system with error handling

## Installation

```bash
go get github.com/arunkatherashala/kore-go
```

## Quick Start

### Reading a Kore File

```go
package main

import (
	"fmt"
	"log"
	"github.com/arunkatherashala/kore-go/kore"
)

func main() {
	// Open and read
	reader, err := kore.NewReader("data.kore")
	if err != nil {
		log.Fatal(err)
	}
	defer reader.Close()

	// Read all data ([][]string - columns)
	data, err := reader.Read()
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Columns: %d, Rows: %d\n", len(data), len(data[0]))
	for colIdx, column := range data {
		fmt.Printf("Column %d: %v\n", colIdx, column)
	}
}
```

### Writing a Kore File

```go
import "github.com/arunkatherashala/kore-go/kore"

// Prepare columnar data
data := [][]string{
	{"Alice", "Bob", "Charlie"},        // Column 1: names
	{"25", "30", "35"},                 // Column 2: ages
	{"Engineer", "Designer", "Manager"}, // Column 3: roles
}

// Write to file
err := kore.WriteFile("output.kore", data)
if err != nil {
	log.Fatal(err)
}
```

### Reading a Specific Column

```go
reader, err := kore.NewReader("data.kore")
if err != nil {
	log.Fatal(err)
}
defer reader.Close()

// Read column 0
column, err := reader.ReadColumn(0)
if err != nil {
	log.Fatal(err)
}

fmt.Printf("Column has %d rows: %v\n", len(column), column)
```

## API Reference

### Reader

- `NewReader(path string)` - Open a Kore file for reading
- `Read() ([][]string, error)` - Read all data as columns
- `ReadColumn(colIdx int) ([]string, error)` - Read a specific column
- `Header() *Header` - Get file header information
- `Columns() []Column` - Get column metadata
- `Close() error` - Close the file

### Writer

- `NewWriter(path string)` - Create a Kore file for writing
- `WriteData(columns [][]string) error` - Write column-oriented data
- `Close() error` - Close the file

### Convenience Functions

- `ReadFile(path string) ([][]string, error)` - Read entire file
- `WriteFile(path, data) error` - Write entire file
- `Version() string` - Get library version

## Examples

See the `example/` directory for complete, runnable examples including:
- Reading Kore files
- Writing Kore files
- Column-level access
- Error handling

## Format Specification

The Kore format is a columnar file format optimized for analytics:

- **Magic**: `KORE` (4 bytes)
- **Version**: 2 (1 byte)
- **Columns**: Number of columns (2 bytes)
- **Rows**: Total row count (8 bytes)
- **Data**: Column-oriented storage with optional compression

For full specification, see [KORE format docs](https://github.com/arunkatherashala/Kore#format).

## Performance

Benchmarks on typical analytics workloads:

- **Read**: ~50MB/sec
- **Write**: ~40MB/sec
- **Memory**: O(n) where n = number of rows per column

## Error Handling

All operations return Go-style `error` values:

```go
if reader, err := kore.NewReader("data.kore"); err != nil {
	// ErrInvalidFormat, ErrInvalidHeader, or file I/O errors
	log.Printf("Failed to open Kore file: %v", err)
}
```

## Contributing

We welcome contributions! See [CONTRIBUTING.md](https://github.com/arunkatherashala/Kore/blob/main/CONTRIBUTING.md)

## License

Licensed under KUOPL. See [LICENSE](https://github.com/arunkatherashala/Kore/blob/main/KUOPL-LICENSE)

## Related Packages

- **Python**: `pip install kore-fileformat`
- **Java**: `io.github.arunkatherashala:kore-fileformat`
- **JavaScript/Node.js**: `npm install kore-fileformat`
- **Rust**: `cargo add kore_fileformat`

## Support

- 📚 [Documentation](https://pkg.go.dev/github.com/arunkatherashala/kore-go)
- 🐛 [Issue Tracker](https://github.com/arunkatherashala/Kore/issues)
- 💬 [Discussions](https://github.com/arunkatherashala/Kore/discussions)
