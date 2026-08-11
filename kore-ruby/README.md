# KORE FileFormat — Ruby

**Version 1.7.14** | [RubyGems](https://rubygems.org/gems/kore-fileformat) | [GitHub](https://github.com/arunkatherashala/Kore)

High-performance columnar format with 11 ACID features. Uses Ruby's built-in `Fiddle` to call Rust `kore_ffi.dll` — no native gem compilation required.

## Install

```bash
gem install kore-fileformat -v 1.7.14
```

Or from source:
```bash
cargo build --release -p kore-ffi
gem build kore-ruby/kore_fileformat.gemspec
gem install kore-fileformat-1.7.14.gem
```

## Quick Start

```ruby
require 'kore_fileformat'

# --- Write ---
block = Kore::DataBlock.new
block.add_column('price',    Kore::FileFormat::DataType::F64, [10.5, 20.0, 30.75])
block.add_column('quantity', Kore::FileFormat::DataType::I64, [100,  200,  300])
Kore.write_file('data.kore', block)

# --- Read ---
result = Kore.read_file('data.kore')
puts "#{result.num_rows} rows, #{result.num_columns} columns"

# --- CRC32 ---
checksum = Kore::FileFormat.crc32('hello kore')
printf "crc32 = 0x%08x\n", checksum   # 0x4b029b4b
```

## API Reference

| Method | Description |
|--------|-------------|
| `Kore.write_file(path, block)` | Write DataBlock to .kore binary |
| `Kore.read_file(path)` | Read .kore → DataBlock |
| `Kore::FileFormat.crc32(data)` | CRC32 checksum |
| `Kore::DataBlock.new` | Create empty block |
| `block.add_column(name, dtype, data)` | Add column |

## Data Types

```ruby
Kore::FileFormat::DataType::F64       # 64-bit float
Kore::FileFormat::DataType::I64       # 64-bit integer
Kore::FileFormat::DataType::STR       # UTF-8 string
Kore::FileFormat::DataType::STR_DICT  # Dictionary-encoded string
Kore::FileFormat::DataType::BOOL      # Boolean
```

## Run Tests

```bash
ruby test_kore_fileformat.rb
```
