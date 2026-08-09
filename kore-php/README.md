# KORE FileFormat — PHP

**Version 1.6.23** | [Packagist](https://packagist.org/packages/arunkatherashala/kore-fileformat) | [GitHub](https://github.com/arunkatherashala/Kore)

High-performance columnar format with 11 ACID features. Uses PHP's built-in `FFI` extension to call Rust `kore_ffi.dll`.

## Requirements

- PHP 7.4+ with `ext-ffi` enabled
- Enable in `php.ini`: `ffi.enable = "preloaded"` (or `true`)

## Install

```bash
composer require arunkatherashala/kore-fileformat:1.6.23
```

Or from source:
```bash
cargo build --release -p kore-ffi
```

## Quick Start

```php
<?php
require_once 'KoreFileFormat.php';
use Kore\{FileFormat, DataBlock, DataType};

// --- Write ---
$block = new DataBlock();
$block->addColumn('price',    DataType::F64, [10.5, 20.0, 30.75]);
$block->addColumn('quantity', DataType::I64, [100,  200,  300]);
FileFormat::writeFile('data.kore', $block);

// --- Read ---
$result = FileFormat::readFile('data.kore');
echo "{$result->numRows} rows, {$result->numColumns} columns\n";

// --- CRC32 ---
$checksum = FileFormat::crc32('hello kore');
printf("crc32 = 0x%08x\n", $checksum);   // 0x4b029b4b
```

## API Reference

| Method | Description |
|--------|-------------|
| `FileFormat::writeFile($path, $block)` | Write DataBlock to .kore |
| `FileFormat::readFile($path)` | Read .kore → DataBlock |
| `FileFormat::crc32($data)` | CRC32 checksum |
| `new DataBlock()` | Create empty block |
| `$block->addColumn($name, $type, $data)` | Add column |

## Data Types

```php
DataType::F64       // 64-bit float
DataType::I64       // 64-bit integer
DataType::STR       // UTF-8 string
DataType::STR_DICT  // Dictionary-encoded string
DataType::BOOL      // Boolean
```

## Run Tests

```bash
php KoreFileFormatTest.php
# or with PHPUnit:
phpunit KoreFileFormatTest.php
```
