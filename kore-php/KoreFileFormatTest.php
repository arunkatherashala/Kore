<?php

namespace Kore\Tests;

use PHPUnit\Framework\TestCase;
use Kore\DataType;
use Kore\Compression;
use Kore\ColumnStats;
use Kore\Column;
use Kore\DataBlock;
use Kore\VersionSnapshot;
use Kore\PartitionSpec;
use Kore\DeleteVector;
use Kore\FileFormat;

class DataTypesTest extends TestCase
{
    public function testDataTypeConstants(): void
    {
        $this->assertEquals(1, DataType::I64);
        $this->assertEquals(2, DataType::F64);
        $this->assertEquals(3, DataType::BOOL);
        $this->assertEquals(4, DataType::STR);
        $this->assertEquals(5, DataType::STR_DICT);
        $this->assertEquals(6, DataType::ARRAY);
        $this->assertEquals(7, DataType::STRUCT);
    }

    public function testCompressionConstants(): void
    {
        $this->assertEquals(0, Compression::RAW);
        $this->assertEquals(1, Compression::RLE);
        $this->assertEquals(2, Compression::DELTA);
        $this->assertEquals(3, Compression::DICT);
        $this->assertEquals(4, Compression::NAN_RAW);
        $this->assertEquals(5, Compression::DEFLATE);
        $this->assertEquals(6, Compression::ZSTD);
    }
}

class DataBlockTest extends TestCase
{
    public function testCreateEmptyDataBlock(): void
    {
        $block = new DataBlock();

        $this->assertEquals(0, $block->numRows);
        $this->assertEquals(0, $block->getNumColumns());
        $this->assertEmpty($block->columns);
    }

    public function testAddSingleColumn(): void
    {
        $block = new DataBlock();
        $data = [1, 2, 3, 4, 5];
        $block->addColumn('numbers', DataType::I64, $data);

        $this->assertEquals(5, $block->numRows);
        $this->assertEquals(1, $block->getNumColumns());
        $this->assertNotNull($block->getColumn('numbers'));
    }

    public function testAddMultipleColumns(): void
    {
        $block = new DataBlock();
        $block->addColumn('numbers', DataType::I64, [1, 2, 3]);
        $block->addColumn('names', DataType::STR, ['a', 'b', 'c']);

        $this->assertEquals(3, $block->numRows);
        $this->assertEquals(2, $block->getNumColumns());
    }

    public function testAddColumnWithMismatchedRowsThrows(): void
    {
        $block = new DataBlock();
        $block->addColumn('numbers', DataType::I64, [1, 2, 3]);

        $this->expectException(\InvalidArgumentException::class);
        $this->expectExceptionMessage('has 2 rows, expected 3');

        $block->addColumn('names', DataType::STR, ['a', 'b']);
    }

    public function testGetColumnByName(): void
    {
        $block = new DataBlock();
        $data = [10, 20, 30];
        $block->addColumn('test', DataType::I64, $data);

        $col = $block->getColumn('test');
        $this->assertNotNull($col);
        $this->assertEquals('test', $col->name);
        $this->assertEquals(DataType::I64, $col->dtype);
        $this->assertEquals($data, $col->data);
    }

    public function testGetNonexistentColumnReturnsNull(): void
    {
        $block = new DataBlock();
        $block->addColumn('test', DataType::I64, [1, 2, 3]);

        $col = $block->getColumn('nonexistent');
        $this->assertNull($col);
    }

    public function testDataBlockToArray(): void
    {
        $block = new DataBlock();
        $block->addColumn('numbers', DataType::I64, [1, 2]);

        $arr = $block->toArray();
        $this->assertEquals(2, $arr['num_rows']);
        $this->assertEquals(1, $arr['num_columns']);
        $this->assertEquals('numbers', $arr['columns'][0]['name']);
    }
}

class ColumnStatsTest extends TestCase
{
    public function testCreateColumnStats(): void
    {
        $stats = new ColumnStats(
            minValue: 1,
            maxValue: 100,
            nullCount: 0,
            cardinality: 50,
            crc32: 0xdeadbeef
        );

        $this->assertEquals(1, $stats->minValue);
        $this->assertEquals(100, $stats->maxValue);
        $this->assertEquals(0, $stats->nullCount);
        $this->assertEquals(50, $stats->cardinality);
        $this->assertEquals(0xdeadbeef, $stats->crc32);
    }

    public function testColumnStatsToArray(): void
    {
        $stats = new ColumnStats(
            minValue: 5,
            maxValue: 95,
            cardinality: 20
        );

        $arr = $stats->toArray();
        $this->assertEquals(5, $arr['min_value']);
        $this->assertEquals(95, $arr['max_value']);
        $this->assertEquals(20, $arr['cardinality']);
    }
}

class ColumnTest extends TestCase
{
    public function testCreateColumn(): void
    {
        $data = [1, 2, 3];
        $col = new Column('test', DataType::I64, $data);

        $this->assertEquals('test', $col->name);
        $this->assertEquals(DataType::I64, $col->dtype);
        $this->assertEquals($data, $col->data);
        $this->assertNull($col->stats);
    }

    public function testColumnWithStats(): void
    {
        $stats = new ColumnStats(minValue: 1, maxValue: 3);
        $col = new Column('test', DataType::I64, [1, 2, 3], $stats);

        $this->assertEquals($stats, $col->stats);
    }

    public function testColumnToArray(): void
    {
        $col = new Column('test', DataType::I64, [1, 2]);

        $arr = $col->toArray();
        $this->assertEquals('test', $arr['name']);
        $this->assertEquals(DataType::I64, $arr['type']);
        $this->assertEquals([1, 2], $arr['data']);
    }
}

class VersionSnapshotTest extends TestCase
{
    public function testCreateVersionSnapshot(): void
    {
        $version = new VersionSnapshot(
            versionId: 1,
            timestamp: 1234567890,
            blockOffset: 100,
            rowCount: 1000
        );

        $this->assertEquals(1, $version->versionId);
        $this->assertEquals(1234567890, $version->timestamp);
        $this->assertEquals(100, $version->blockOffset);
        $this->assertEquals(1000, $version->rowCount);
        $this->assertNull($version->prevVersion);
    }

    public function testCreateVersionSnapshotWithPrevious(): void
    {
        $version = new VersionSnapshot(
            versionId: 2,
            timestamp: 1234567900,
            blockOffset: 200,
            rowCount: 2000,
            prevVersion: 1
        );

        $this->assertEquals(2, $version->versionId);
        $this->assertEquals(1, $version->prevVersion);
    }
}

class PartitionSpecTest extends TestCase
{
    public function testCreatePartitionSpec(): void
    {
        $spec = new PartitionSpec(
            specId: 1,
            columns: [0, 1],
            transforms: ['year', 'month']
        );

        $this->assertEquals(1, $spec->specId);
        $this->assertEquals([0, 1], $spec->columns);
        $this->assertEquals(['year', 'month'], $spec->transforms);
    }
}

class DeleteVectorTest extends TestCase
{
    public function testCreateDeleteVector(): void
    {
        $bitmap = "\xff\x00";
        $dv = new DeleteVector($bitmap, 8, 1234567890);

        $this->assertEquals($bitmap, $dv->bitmap);
        $this->assertEquals(8, $dv->cardinality);
        $this->assertEquals(1234567890, $dv->timestamp);
    }
}

class WriteFileTest extends TestCase
{
    /**
     * @runInSeparateProcess
     */
    public function testWriteFileJsonFallback(): void
    {
        $block = new DataBlock();
        $block->addColumn('numbers', DataType::I64, [1, 2, 3]);

        $tmpFile = tempnam(sys_get_temp_dir(), 'kore_test_');
        try {
            FileFormat::writeFile($tmpFile, $block);

            $this->assertFileExists($tmpFile);
            $content = file_get_contents($tmpFile);
            $data = json_decode($content, true);

            $this->assertEquals(3, $data['num_rows']);
            $this->assertEquals(1, $data['num_columns']);
            $this->assertEquals('numbers', $data['columns'][0]['name']);
        } finally {
            if (file_exists($tmpFile)) {
                unlink($tmpFile);
            }
        }
    }
}

class PhaseThreeTest extends TestCase
{
    public function testCrc32Pending(): void
    {
        $this->expectException(\RuntimeException::class);
        $this->expectExceptionMessage('Phase 3: CRC32 FFI pending');
        FileFormat::crc32('test');
    }

    public function testReadFilePending(): void
    {
        $this->expectException(\RuntimeException::class);
        $this->expectExceptionMessage('Phase 3: Binary format reading pending');
        FileFormat::readFile('/tmp/test.kore');
    }

    public function testEncryptDecryptPending(): void
    {
        $this->expectException(\RuntimeException::class);
        $this->expectExceptionMessage('Phase 3: Encryption API pending');
        FileFormat::encryptAes256('password', 'data');
    }

    public function testGetColumnStatsPending(): void
    {
        $this->expectException(\RuntimeException::class);
        $this->expectExceptionMessage('Phase 3: Stats API pending');
        FileFormat::getColumnStats('', 'column_name');
    }

    public function testGetBloomFilterPending(): void
    {
        $this->expectException(\RuntimeException::class);
        $this->expectExceptionMessage('Phase 3: Bloom filter API pending');
        FileFormat::getBloomFilter('', 'column_name');
    }
}
