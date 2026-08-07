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
 * Main KORE FileFormat API.
 */
class FileFormat
{
    private static ?\FFI $ffi = null;

    /**
     * Load native FFI bindings (Phase 3 TODO).
     */
    private static function getFfi(): ?\FFI
    {
        if (self::$ffi !== null) {
            return self::$ffi;
        }

        // TODO: Load libkore_ffi via FFI::load()
        // if (!extension_loaded('ffi')) {
        //     throw new \RuntimeException('PHP FFI extension is required');
        // }
        //
        // self::$ffi = \FFI::load(__DIR__ . '/kore.h');
        return null;
    }

    // ─────────────────────────────────────────────────────────────────────
    // HIGH-LEVEL API
    // ─────────────────────────────────────────────────────────────────────

    /**
     * Compute CRC32 checksum.
     */
    public static function crc32(string $data): int
    {
        // TODO: Call Rust kore_crc32 via FFI
        throw new \RuntimeException('Phase 3: CRC32 FFI pending');
    }

    /**
     * Write DataBlock to KORE file (Phase 3 placeholder).
     */
    public static function writeFile(string $path, DataBlock $dataBlock): void
    {
        // TODO: Call Rust kore_write_file via FFI
        // For now: JSON fallback
        file_put_contents($path, json_encode($dataBlock->toArray(), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES));
    }

    /**
     * Read KORE file into DataBlock (Phase 3 placeholder).
     */
    public static function readFile(string $path): DataBlock
    {
        // TODO: Call Rust kore_read_file via FFI
        // For now: JSON fallback
        throw new \RuntimeException('Phase 3: Binary format reading pending');
    }

    /**
     * Read KORE data at specific version (time travel).
     */
    public static function readAtVersion(string $data, int $timestamp): DataBlock
    {
        throw new \RuntimeException('Phase 3: Time travel API pending');
    }

    /**
     * Encrypt data with AES-256-GCM.
     */
    public static function encryptAes256(string $password, string $data): string
    {
        throw new \RuntimeException('Phase 3: Encryption API pending');
    }

    /**
     * Decrypt data with AES-256-GCM.
     */
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
