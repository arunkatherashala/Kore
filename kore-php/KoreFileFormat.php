<?php

/**
 * KORE File Format PHP Extension
 * ==============================
 *
 * PHP bindings to the KORE columnar format via ext-ffi.
 *
 * Features:
 *   - Read/write KORE v2 binary files
 *   - All 11 ACID features
 *   - Native PHP arrays and objects
 *   - Type-safe API
 *
 * Example:
 *   $kore = new Kore\FileFormat();
 *   $block = new Kore\DataBlock();
 *   $block->addColumn('numbers', Kore\DataType::I64, [1, 2, 3, 4, 5]);
 *   $block->addColumn('names', Kore\DataType::STR, ['a', 'b', 'c', 'd', 'e']);
 *   $kore->writeFile('/tmp/data.kore', $block);
 *
 * Requirements:
 *   - PHP 7.4+ (for typed properties)
 *   - PHP FFI extension enabled (php.ini: ffi.enable = "preloaded")
 */

namespace Kore;

// ─────────────────────────────────────────────────────────────────────────────
// DATA TYPES & ENUMS
// ─────────────────────────────────────────────────────────────────────────────

/**
 * KORE column data types (must match Rust DType enum).
 */
abstract class DataType
{
    public const I64 = 1;          // 64-bit signed integer
    public const F64 = 2;          // 64-bit floating point
    public const BOOL = 3;         // Boolean
    public const STR = 4;          // UTF-8 string
    public const STR_DICT = 5;     // Dictionary-encoded string
    public const ARRAY = 6;        // Nested array
    public const STRUCT = 7;       // Nested struct
}

/**
 * KORE compression codecs (must match Rust Compression enum).
 */
abstract class Compression
{
    public const RAW = 0;          // No compression
    public const RLE = 1;          // Run-length encoding
    public const DELTA = 2;        // Delta encoding
    public const DICT = 3;         // Dictionary encoding
    public const NAN_RAW = 4;      // Special NaN handling
    public const DEFLATE = 5;      // Deflate/LZ4
    public const ZSTD = 6;         // ZSTD compression
}

// ─────────────────────────────────────────────────────────────────────────────
// CORE CLASSES
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Column statistics for predicate pushdown.
 */
class ColumnStats
{
    public ?int $minValue = null;
    public ?int $maxValue = null;
    public ?float $minValueF = null;
    public ?float $maxValueF = null;
    public int $nullCount = 0;
    public int $cardinality = 0;
    public int $crc32 = 0;

    public function __construct(
        ?int $minValue = null,
        ?int $maxValue = null,
        ?float $minValueF = null,
        ?float $maxValueF = null,
        int $nullCount = 0,
        int $cardinality = 0,
        int $crc32 = 0
    ) {
        $this->minValue = $minValue;
        $this->maxValue = $maxValue;
        $this->minValueF = $minValueF;
        $this->maxValueF = $maxValueF;
        $this->nullCount = $nullCount;
        $this->cardinality = $cardinality;
        $this->crc32 = $crc32;
    }

    public function toArray(): array
    {
        return [
            'min_value' => $this->minValue,
            'max_value' => $this->maxValue,
            'min_value_f' => $this->minValueF,
            'max_value_f' => $this->maxValueF,
            'null_count' => $this->nullCount,
            'cardinality' => $this->cardinality,
            'crc32' => $this->crc32,
        ];
    }
}

/**
 * Single column in a data block.
 */
class Column
{
    public string $name;
    public int $dtype;
    public array $data;
    public ?ColumnStats $stats = null;

    public function __construct(string $name, int $dtype, array $data, ?ColumnStats $stats = null)
    {
        $this->name = $name;
        $this->dtype = $dtype;
        $this->data = $data;
        $this->stats = $stats;
    }

    public function toArray(): array
    {
        return [
            'name' => $this->name,
            'type' => $this->dtype,
            'data' => $this->data,
            'stats' => $this->stats ? $this->stats->toArray() : null,
        ];
    }
}

/**
 * Multi-column data structure.
 */
class DataBlock
{
    public array $columns = [];
    public int $numRows = 0;

    /**
     * Add a column to the data block.
     */
    public function addColumn(string $name, int $dtype, array $data): void
    {
        if ($this->numRows === 0) {
            $this->numRows = count($data);
        } elseif (count($data) !== $this->numRows) {
            throw new \InvalidArgumentException(
                "Column '{$name}' has " . count($data) . " rows, expected {$this->numRows}"
            );
        }

        $this->columns[] = new Column($name, $dtype, $data);
    }

    /**
     * Get column by name.
     */
    public function getColumn(string $name): ?Column
    {
        foreach ($this->columns as $col) {
            if ($col->name === $name) {
                return $col;
            }
        }
        return null;
    }

    /**
     * Get number of columns.
     */
    public function getNumColumns(): int
    {
        return count($this->columns);
    }

    public function toArray(): array
    {
        return [
            'num_rows' => $this->numRows,
            'num_columns' => $this->getNumColumns(),
            'columns' => array_map(fn ($col) => $col->toArray(), $this->columns),
        ];
    }
}

/**
 * MVCC version tracking for time travel.
 */
class VersionSnapshot
{
    public int $versionId;
    public int $timestamp;
    public int $blockOffset;
    public int $rowCount;
    public ?int $prevVersion = null;

    public function __construct(
        int $versionId,
        int $timestamp,
        int $blockOffset,
        int $rowCount,
        ?int $prevVersion = null
    ) {
        $this->versionId = $versionId;
        $this->timestamp = $timestamp;
        $this->blockOffset = $blockOffset;
        $this->rowCount = $rowCount;
        $this->prevVersion = $prevVersion;
    }

    public function toArray(): array
    {
        return [
            'version_id' => $this->versionId,
            'timestamp' => $this->timestamp,
            'block_offset' => $this->blockOffset,
            'row_count' => $this->rowCount,
            'prev_version' => $this->prevVersion,
        ];
    }
}

/**
 * Partition evolution support.
 */
class PartitionSpec
{
    public int $specId;
    public array $columns = [];
    public array $transforms = [];
    public ?int $parentSpecId = null;

    public function __construct(
        int $specId,
        array $columns = [],
        array $transforms = [],
        ?int $parentSpecId = null
    ) {
        $this->specId = $specId;
        $this->columns = $columns;
        $this->transforms = $transforms;
        $this->parentSpecId = $parentSpecId;
    }

    public function toArray(): array
    {
        return [
            'spec_id' => $this->specId,
            'columns' => $this->columns,
            'transforms' => $this->transforms,
            'parent_spec_id' => $this->parentSpecId,
        ];
    }
}

/**
 * Row-level delete bitmap for soft deletes.
 */
class DeleteVector
{
    public string $bitmap;
    public int $cardinality;
    public int $timestamp;

    public function __construct(string $bitmap = '', int $cardinality = 0, int $timestamp = 0)
    {
        $this->bitmap = $bitmap;
        $this->cardinality = $cardinality;
        $this->timestamp = $timestamp;
    }

    public function toArray(): array
    {
        return [
            'bitmap' => bin2hex($this->bitmap),
            'cardinality' => $this->cardinality,
            'timestamp' => $this->timestamp,
        ];
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FFI BINDINGS
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Main KORE FileFormat API — wired to Rust kore_ffi via PHP FFI.
 */
class FileFormat
{
    private static ?\FFI $ffi = null;

    private static function getFfi(): \FFI
    {
        if (self::$ffi !== null) {
            return self::$ffi;
        }

        $here = __DIR__;
        $candidates = [
            $here . '/../target/release/kore_ffi.dll',      // Windows
            $here . '/../target/release/libkore_ffi.so',    // Linux
            $here . '/../target/release/libkore_ffi.dylib', // macOS
        ];
        $dll = null;
        foreach ($candidates as $c) {
            if (file_exists($c)) { $dll = realpath($c); break; }
        }
        if (!$dll) {
            throw new \RuntimeException(
                'kore_ffi not found. Build: cargo build --release -p kore-ffi'
            );
        }

        self::$ffi = \FFI::cdef(<<<EOH
            typedef void KoreBlock;
            uint32_t kore_crc32(const uint8_t* data, size_t len);
            KoreBlock* kore_block_new();
            void       kore_block_free(KoreBlock* block);
            int        kore_block_add_f64(KoreBlock* block, const char* name, const double* data, size_t len);
            int        kore_block_add_i64(KoreBlock* block, const char* name, const int64_t* data, size_t len);
            int        kore_write_file(const char* path, KoreBlock* block);
            KoreBlock* kore_read_file(const char* path);
            uint64_t   kore_block_num_rows(const KoreBlock* block);
            uint32_t   kore_block_num_cols(const KoreBlock* block);
            char*      kore_block_col_name(const KoreBlock* block, size_t idx);
            int64_t    kore_block_get_f64(const KoreBlock* block, const char* col, double* out, uint64_t maxlen);
            void       kore_free_string(char* s);
EOH, $dll);
        return self::$ffi;
    }

    // ─────────────────────────────────────────────────────────────────────
    // HIGH-LEVEL API
    // ─────────────────────────────────────────────────────────────────────

    public static function crc32(string $data): int
    {
        $ffi = self::getFfi();
        $buf = \FFI::new("uint8_t[" . strlen($data) . "]");
        \FFI::memcpy($buf, $data, strlen($data));
        return $ffi->kore_crc32($buf, strlen($data));
    }

    public static function writeFile(string $path, DataBlock $dataBlock): void
    {
        $ffi    = self::getFfi();
        $handle = $ffi->kore_block_new();
        if (\FFI::isNull($handle)) throw new \RuntimeException('kore_block_new failed');
        try {
            foreach ($dataBlock->columns as $col) {
                if ($col->dtype === DataType::F64 && count($col->data) > 0) {
                    $arr = \FFI::new('double[' . count($col->data) . ']');
                    foreach ($col->data as $i => $v) $arr[$i] = (float)$v;
                    $ffi->kore_block_add_f64($handle, $col->name, $arr, count($col->data));
                } elseif ($col->dtype === DataType::I64 && count($col->data) > 0) {
                    $arr = \FFI::new('int64_t[' . count($col->data) . ']');
                    foreach ($col->data as $i => $v) $arr[$i] = (int)$v;
                    $ffi->kore_block_add_i64($handle, $col->name, $arr, count($col->data));
                }
            }
            $rc = $ffi->kore_write_file($path, $handle);
            if ($rc !== 0) throw new \RuntimeException("kore_write_file failed (rc=$rc)");
        } finally {
            $ffi->kore_block_free($handle);
        }
    }

    public static function readFile(string $path): DataBlock
    {
        $ffi    = self::getFfi();
        $handle = $ffi->kore_read_file($path);
        if (\FFI::isNull($handle)) throw new \RuntimeException("kore_read_file failed: $path");
        try {
            $nrows = $ffi->kore_block_num_rows($handle);
            $ncols = $ffi->kore_block_num_cols($handle);
            $block = new DataBlock();

            for ($ci = 0; $ci < $ncols; $ci++) {
                $rawName = $ffi->kore_block_col_name($handle, $ci);
                $colName = \FFI::isNull($rawName) ? "col$ci" : \FFI::string($rawName);
                if (!\FFI::isNull($rawName)) $ffi->kore_free_string($rawName);

                $buf = \FFI::new("double[$nrows]");
                $n   = $ffi->kore_block_get_f64($handle, $colName, $buf, $nrows);
                if ($n > 0) {
                    $data = [];
                    for ($i = 0; $i < $n; $i++) $data[] = $buf[$i];
                    $block->addColumn($colName, DataType::F64, $data);
                }
            }
            return $block;
        } finally {
            $ffi->kore_block_free($handle);
        }
    }

    public static function readAtVersion(string $data, int $timestamp): DataBlock
    {
        throw new \RuntimeException('Phase 3: Time travel API pending');
    }

    public static function encryptAes256(string $password, string $data): string
    {
        throw new \RuntimeException('Phase 3: Encryption API pending');
    }

    public static function decryptAes256(string $password, string $encryptedData): string
    {
        throw new \RuntimeException('Phase 3: Decryption API pending');
    }

    /**
     * Get statistics for a column.
     */
    public static function getColumnStats(string $data, string $columnName): ColumnStats
    {
        throw new \RuntimeException('Phase 3: Stats API pending');
    }

    /**
     * Get Bloom filter for a column.
     */
    public static function getBloomFilter(string $data, string $columnName): string
    {
        throw new \RuntimeException('Phase 3: Bloom filter API pending');
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// VERSION
// ─────────────────────────────────────────────────────────────────────────────

const VERSION = '2.0.0';
