using Xunit;
using Kore;
using System;
using System.Collections.Generic;

namespace Kore.Tests
{
    public class DataTypesTests
    {
        [Fact]
        public void DataTypeEnumValuesAreCorrect()
        {
            Assert.Equal(1, (byte)KoreFileFormat.DataType.I64);
            Assert.Equal(2, (byte)KoreFileFormat.DataType.F64);
            Assert.Equal(3, (byte)KoreFileFormat.DataType.BOOL);
            Assert.Equal(4, (byte)KoreFileFormat.DataType.STR);
            Assert.Equal(5, (byte)KoreFileFormat.DataType.STR_DICT);
            Assert.Equal(6, (byte)KoreFileFormat.DataType.ARRAY);
            Assert.Equal(7, (byte)KoreFileFormat.DataType.STRUCT);
        }

        [Fact]
        public void CompressionEnumValuesAreCorrect()
        {
            Assert.Equal(0, (byte)KoreFileFormat.Compression.RAW);
            Assert.Equal(1, (byte)KoreFileFormat.Compression.RLE);
            Assert.Equal(2, (byte)KoreFileFormat.Compression.DELTA);
            Assert.Equal(3, (byte)KoreFileFormat.Compression.DICT);
            Assert.Equal(4, (byte)KoreFileFormat.Compression.NAN_RAW);
            Assert.Equal(5, (byte)KoreFileFormat.Compression.DEFLATE);
            Assert.Equal(6, (byte)KoreFileFormat.Compression.ZSTD);
        }

        [Theory]
        [InlineData(KoreFileFormat.DataType.I64, "I64")]
        [InlineData(KoreFileFormat.DataType.F64, "F64")]
        [InlineData(KoreFileFormat.DataType.BOOL, "BOOL")]
        [InlineData(KoreFileFormat.DataType.STR, "STR")]
        public void DataTypeCanBeConvertedToString(KoreFileFormat.DataType type, string expected)
        {
            Assert.Equal(expected, type.ToString());
        }
    }

    public class DataBlockTests
    {
        [Fact]
        public void CreateEmptyDataBlock()
        {
            var block = new KoreFileFormat.DataBlock();

            Assert.NotNull(block);
            Assert.Equal(0, block.NumRows);
            Assert.Equal(0, block.NumColumns);
            Assert.Empty(block.Columns);
        }

        [Fact]
        public void AddSingleColumn()
        {
            var block = new KoreFileFormat.DataBlock();
            var data = new List<long> { 1, 2, 3, 4, 5 };

            block.AddColumn("numbers", KoreFileFormat.DataType.I64, data);

            Assert.Equal(5, block.NumRows);
            Assert.Equal(1, block.NumColumns);
            Assert.NotNull(block.GetColumn("numbers"));
        }

        [Fact]
        public void AddMultipleColumns()
        {
            var block = new KoreFileFormat.DataBlock();
            var nums = new List<long> { 1, 2, 3 };
            var names = new List<string> { "a", "b", "c" };

            block.AddColumn("numbers", KoreFileFormat.DataType.I64, nums);
            block.AddColumn("names", KoreFileFormat.DataType.STR, names);

            Assert.Equal(3, block.NumRows);
            Assert.Equal(2, block.NumColumns);
        }

        [Fact]
        public void AddColumnWithMismatchedRowsThrows()
        {
            var block = new KoreFileFormat.DataBlock();
            var nums = new List<long> { 1, 2, 3 };
            var names = new List<string> { "a", "b" };

            block.AddColumn("numbers", KoreFileFormat.DataType.I64, nums);

            var ex = Assert.Throws<ArgumentException>(() =>
                block.AddColumn("names", KoreFileFormat.DataType.STR, names)
            );

            Assert.Contains("has 2 rows, expected 3", ex.Message);
        }

        [Fact]
        public void GetColumnByName()
        {
            var block = new KoreFileFormat.DataBlock();
            var data = new List<long> { 10, 20, 30 };

            block.AddColumn("test", KoreFileFormat.DataType.I64, data);

            var col = block.GetColumn("test");
            Assert.NotNull(col);
            Assert.Equal("test", col.Name);
            Assert.Equal(KoreFileFormat.DataType.I64, col.Type);
            Assert.Equal(data, col.Data);
        }

        [Fact]
        public void GetNonExistentColumnReturnsNull()
        {
            var block = new KoreFileFormat.DataBlock();
            block.AddColumn("test", KoreFileFormat.DataType.I64, new List<long> { 1, 2, 3 });

            var col = block.GetColumn("nonexistent");
            Assert.Null(col);
        }
    }

    public class ColumnStatsTests
    {
        [Fact]
        public void CreateColumnStats()
        {
            var stats = new KoreFileFormat.ColumnStats
            {
                MinValue = 1,
                MaxValue = 100,
                NullCount = 0,
                Cardinality = 50,
                CRC32 = 0xdeadbeef,
            };

            Assert.Equal(1, stats.MinValue);
            Assert.Equal(100, stats.MaxValue);
            Assert.Equal(0, stats.NullCount);
            Assert.Equal(50, stats.Cardinality);
            Assert.Equal(0xdeadbeef, stats.CRC32);
        }

        [Fact]
        public void ColumnStatsWithNullableValues()
        {
            var stats = new KoreFileFormat.ColumnStats
            {
                MinValueF = 1.5,
                MaxValueF = 99.9,
                NullCount = 5,
            };

            Assert.NotNull(stats.MinValueF);
            Assert.NotNull(stats.MaxValueF);
            Assert.Equal(5, stats.NullCount);
        }
    }

    public class CRC32Tests
    {
        [Fact]
        public void CRC32ComputesNonZero()
        {
            var data = new byte[] { 1, 2, 3, 4, 5 };
            var crc = KoreFileFormat.CRC32(data);

            Assert.NotEqual(0u, crc);
        }

        [Fact]
        public void CRC32IsConsistent()
        {
            var data = new byte[] { 1, 2, 3, 4, 5 };
            var crc1 = KoreFileFormat.CRC32(data);
            var crc2 = KoreFileFormat.CRC32(data);

            Assert.Equal(crc1, crc2);
        }

        [Fact]
        public void CRC32OfEmptyReturnsZero()
        {
            var crc = KoreFileFormat.CRC32(new byte[] { });
            Assert.Equal(0u, crc);
        }

        [Fact]
        public void CRC32OfNullReturnsZero()
        {
            var crc = KoreFileFormat.CRC32(null);
            Assert.Equal(0u, crc);
        }
    }

    public class VersionControlTests
    {
        [Fact]
        public void CreateVersionSnapshot()
        {
            var version = new KoreFileFormat.VersionSnapshot
            {
                VersionId = 1,
                Timestamp = 1234567890,
                BlockOffset = 100,
                RowCount = 1000,
            };

            Assert.Equal(1u, version.VersionId);
            Assert.Equal(1234567890ul, version.Timestamp);
            Assert.Equal(100ul, version.BlockOffset);
            Assert.Equal(1000ul, version.RowCount);
        }

        [Fact]
        public void CreateVersionSnapshotWithPrevious()
        {
            var version = new KoreFileFormat.VersionSnapshot
            {
                VersionId = 2,
                Timestamp = 1234567900,
                PrevVersion = 1,
            };

            Assert.Equal(2u, version.VersionId);
            Assert.Equal(1u, version.PrevVersion);
        }
    }

    public class PartitionSpecTests
    {
        [Fact]
        public void CreatePartitionSpec()
        {
            var spec = new KoreFileFormat.PartitionSpec
            {
                SpecId = 1,
                Columns = new ushort[] { 0, 1 },
                Transforms = new string[] { "year", "month" },
            };

            Assert.Equal(1, spec.SpecId);
            Assert.Equal(2, spec.Columns.Length);
            Assert.Equal(2, spec.Transforms.Length);
        }
    }

    public class DeleteVectorTests
    {
        [Fact]
        public void CreateDeleteVector()
        {
            var bitmap = new byte[] { 0xff, 0x00 };
            var dv = new KoreFileFormat.DeleteVector
            {
                Bitmap = bitmap,
                Cardinality = 8,
                Timestamp = 1234567890,
            };

            Assert.Equal(bitmap, dv.Bitmap);
            Assert.Equal(8u, dv.Cardinality);
            Assert.Equal(1234567890ul, dv.Timestamp);
        }
    }

    public class VersionTests
    {
        [Fact]
        public void VersionIsCorrect()
        {
            Assert.Equal("2.0.0", KoreFileFormat.Version);
        }
    }

    // Phase 3 placeholder tests
    public class PhaseThreeTests
    {
        [Fact(Skip = "Phase 3: Pending FFI integration")]
        public void WriteReadRoundtrip()
        {
            // TODO: Implement after kore-ffi compilation
        }

        [Fact(Skip = "Phase 3: Pending FFI integration")]
        public void EncryptDecryptRoundtrip()
        {
            // TODO: Implement after encryption FFI exposed
        }

        [Fact(Skip = "Phase 3: Pending FFI integration")]
        public void ReadAtVersion()
        {
            // TODO: Implement after version snapshot APIs exposed
        }

        [Fact(Skip = "Phase 3: Pending FFI integration")]
        public void GetBloomFilter()
        {
            // TODO: Implement after bloom filter APIs exposed
        }
    }
}
