using System;
using Xunit;
using KoreFileFormat;

namespace KoreFileFormat.Tests
{
    public class CompressorTests
    {
        [Fact]
        public void Compress_WithValidData_ReturnsCompressedData()
        {
            // Arrange
            var data = new byte[] { 1, 2, 3, 4, 5, 1, 2, 3, 4, 5 };

            // Act
            var result = Kore.Compress(data);

            // Assert
            Assert.NotNull(result);
            Assert.NotEmpty(result);
        }

        [Fact]
        public void Compress_WithNullData_ThrowsArgumentNullException()
        {
            // Act & Assert
            Assert.Throws<ArgumentNullException>(() => Kore.Compress(null!));
        }

        [Fact]
        public void Compress_WithEmptyArray_ReturnsResult()
        {
            // Arrange
            var data = Array.Empty<byte>();

            // Act
            var result = Kore.Compress(data);

            // Assert
            Assert.NotNull(result);
        }

        [Fact]
        public void Compress_WithAllCompressionLevels_Succeeds()
        {
            // Arrange
            var data = new byte[1024];
            for (int i = 0; i < data.Length; i++)
            {
                data[i] = (byte)(i % 256);
            }

            // Act & Assert
            foreach (CompressionLevel level in Enum.GetValues(typeof(CompressionLevel)))
            {
                var result = Kore.Compress(data, level);
                Assert.NotNull(result);
                Assert.NotEmpty(result);
            }
        }

        [Fact]
        public void Compress_BalancedVsFast_BalancedProducesSmaller()
        {
            // Arrange
            var data = new byte[10000];
            for (int i = 0; i < data.Length; i++)
            {
                data[i] = (byte)('A' + (i % 26));
            }

            // Act
            var fast = Kore.Compress(data, CompressionLevel.Fast);
            var balanced = Kore.Compress(data, CompressionLevel.Balanced);

            // Assert
            Assert.True(balanced.Length <= fast.Length,
                $"Balanced ({balanced.Length}) should compress better than or equal to Fast ({fast.Length})");
        }
    }
}
