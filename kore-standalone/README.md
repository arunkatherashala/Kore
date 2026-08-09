# kore_fileformat

**Version 1.6.12** | [crates.io](https://crates.io/crates/kore_fileformat) | [GitHub](https://github.com/arunkatherashala/Kore)

Pure Rust, zero-dependency implementation of the KORE columnar binary format.

## Install

```toml
[dependencies]
kore_fileformat = "1.6.12"
```

## Quick Start

```rust
use kore_fileformat::{DataBlock, DataType, write_file, read_file};

// Write
let mut block = DataBlock::new();
block.add_column("price", DataType::F64, vec![10.5f64.to_bits(), 20.0f64.to_bits()]);
block.add_column("qty",   DataType::I64, vec![100, 200]);
write_file("data.kore", &block).unwrap();

// Read
let result = read_file("data.kore").unwrap();
assert_eq!(result.num_rows(), 2);
let col = result.column("price").unwrap();
assert_eq!(f64::from_bits(col.values[0]), 10.5);
```

## CRC32

```rust
let checksum = kore_fileformat::crc32(b"hello kore");
println!("{checksum:#010x}");
```

## Features
- Read/write `.kore` binary files
- CRC32 integrity verification
- F64 and I64 column types
- Zero external dependencies
- `no_std` compatible (feature planned)
