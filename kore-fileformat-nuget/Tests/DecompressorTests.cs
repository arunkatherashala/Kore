using System;
using Xunit;
using KoreFileFormat;

namespace KoreFileFormat.Tests
{
    public class DecompressorTests
    {
        [Fact]
        public void Decompress_WithValidCompressedData_ReturnsOriginal()
        {
            // Arrange
            var original = new byte[] { 1, 2, 3, 4, 5 };
            var compressed = Kore.Compress(original);

            // Act
            var decompressed = Kore.Decompress(compressed);

            // Assert
            Assert.Equal(original, decompressed);
        }

        [Fact]
        public void Decompress_WithNullData_ThrowsArgumentNullException()
        {
            // Act & Assert
            Assert.Throws<ArgumentNullException>(() => Kore.Decompress(null!));
        }

        [Fact]
        public void RoundTrip_CompressDecompress_MaintainsDataIntegrity()
        {
            // Arrange
            var original = new byte[1000];
            for (int i = 0; i < original.Length; i++)
            {
                original[i] = (byte)(i % 256);
            }

            // Act
            var compressed = Kore.Compress(original, CompressionLevel.Balanced);
            var decompressed = Kore.Decompress(compressed);

            // Assert
            Assert.Equal(original, decompressed);
        }

        [Fact]
        public void RoundTrip_LargeData_MaintainsDataIntegrity()
        {
            // Arrange - Create 1MB test data
            var original = new byte[1024 * 1024];
            var random = new Random(42); // Fixed seed for reproducibility
            random.NextBytes(original);

            // Act
            var compressed = Kore.Compress(original, CompressionLevel.Balanced);
            var decompressed = Kore.Decompress(compressed);

            // Assert
            Assert.Equal(original.Length, decompressed.Length);
            for (int i = 0; i < original.Length; i++)
            {
                Assert.Equal(original[i], decompressed[i]);
            }
        }

        [Fact]
        public void RoundTrip_TextData_MaintainsDataIntegrity()
        {
            // Arrange
            var text = "The quick brown fox jumps over the lazy dog. " +
                      "The quick brown fox jumps over the lazy dog. " +
                      "The quick brown fox jumps over the lazy dog.";
            var original = System.Text.Encoding.UTF8.GetBytes(text);

            // Act
            var compressed = Kore.Compress(original);
            var decompressed = Kore.Decompress(compressed);

            // Assert
            Assert.Equal(original, decompressed);
        }
    }
}
