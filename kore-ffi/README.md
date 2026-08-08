# KORE FileFormat — Rust

**Version 1.6.0** | [crates.io](https://crates.io/crates/kore-store) | [GitHub](https://github.com/arunkatherashala/Kore)

Native Rust implementation — the core engine powering all 8 language bindings.

## Install

```toml
# Cargo.toml
[dependencies]
kore-store = "1.6.0"   # DataBlock read/write
kore-ffi   = "1.6.0"   # C ABI for FFI bindings
```

## Quick Start

```rust
use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{KoreWriter, reader::KoreReader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- Build DataBlock ---
    let block = DataBlock::new(vec![
        Column::float64("price",    vec![Some(10.5), Some(20.0), Some(30.75)]),
        Column::int64("quantity",   vec![Some(100),  Some(200),  Some(300)]),
    ])?;

    // --- Write ---
    KoreWriter::write_file(std::path::Path::new("data.kore"), &block)?;
    println!("Written {} rows", block.num_rows);

    // --- Read ---
    let result = KoreReader::read_file(std::path::Path::new("data.kore"))?;
    println!("{} rows, {} columns", result.num_rows, result.columns.len());

    Ok(())
}
```

## SQL via kore-sql

```rust
use kore_sql::KqlContext;

let mut ctx = KqlContext::new();
ctx.register("orders", block);

// Full SQL: GROUP BY, window functions, JOINs, CTEs, subqueries
let result = ctx.query("
    SELECT status, SUM(price) AS total, COUNT(*) AS n
    FROM orders
    GROUP BY status
    ORDER BY total DESC
")?;
```

## API Reference

| Crate | Type | Description |
|-------|------|-------------|
| `kore-store` | `KoreWriter::write_file(path, block)` | Write .kore binary |
| `kore-store` | `KoreReader::read_file(path)` | Read .kore binary |
| `kore-sql` | `KqlContext::query(sql)` | Full SQL engine |
| `kore-core` | `DataBlock`, `Column`, `Value` | Core types |

## Run Tests

```bash
cargo test --release
cargo test --release -p kore-store
cargo test --release -p kore-sql   # 29 SQL tests
```
