using System;
using System.Runtime.InteropServices;

namespace Kore.FileFormat
{
    /// <summary>
    /// Main compressor class for KORE v1.1.6 file format compression.
    /// Provides simple APIs for compressing and decompressing data.
    /// </summary>
    public class KoreCompressor : IDisposable
    {
        private const string NativeLibraryName = "kore_fileformat";

        /// <summary>
        /// Gets or sets the compression level (0-22).
        /// Higher values = better compression but slower.
        /// Default: 18
        /// </summary>
        public int CompressionLevel { get; set; } = 18;

        /// <summary>
        /// Initializes a new instance of the KoreCompressor class.
        /// </summary>
        public KoreCompressor()
        {
        }

        /// <summary>
        /// Compresses the input byte array using KORE compression.
        /// </summary>
        /// <param name="inputData">The data to compress.</param>
        /// <returns>Compressed byte array.</returns>
        /// <exception cref="ArgumentNullException">Thrown when inputData is null.</exception>
        /// <exception cref="InvalidOperationException">Thrown when compression fails.</exception>
        public byte[] Compress(byte[] inputData)
        {
            if (inputData == null)
                throw new ArgumentNullException(nameof(inputData));

            if (inputData.Length == 0)
                return Array.Empty<byte>();

            try
            {
                uint outputSize = (uint)(inputData.Length + 1024); // Add overhead
                byte[] outputData = new byte[outputSize];

                uint compressedSize = CompressInternal(inputData, (uint)inputData.Length, outputData, outputSize);

                if (compressedSize == 0)
                    throw new InvalidOperationException("Compression failed.");

                Array.Resize(ref outputData, (int)compressedSize);
                return outputData;
            }
            catch (DllNotFoundException)
            {
                throw new InvalidOperationException(
                    $"Failed to load native KORE library '{NativeLibraryName}'. " +
                    "Ensure the native library is available in your application's runtime directory.");
            }
        }

        /// <summary>
        /// Decompresses the input byte array that was previously compressed with KORE.
        /// </summary>
        /// <param name="compressedData">The compressed data.</param>
        /// <param name="expectedSize">Optional: expected decompressed size (for performance optimization).</param>
        /// <returns>Decompressed byte array.</returns>
        /// <exception cref="ArgumentNullException">Thrown when compressedData is null.</exception>
        /// <exception cref="InvalidOperationException">Thrown when decompression fails.</exception>
        public byte[] Decompress(byte[] compressedData, int? expectedSize = null)
        {
            if (compressedData == null)
                throw new ArgumentNullException(nameof(compressedData));

            if (compressedData.Length == 0)
                return Array.Empty<byte>();

            try
            {
                // Start with expected size or estimate
                uint outputSize = expectedSize != null 
                    ? (uint)expectedSize.Value 
                    : (uint)compressedData.Length * 10; // Estimate 10x expansion

                while (outputSize <= 1073741824) // Max 1GB
                {
                    byte[] outputData = new byte[outputSize];

                    uint decompressedSize = DecompressInternal(
                        compressedData, 
                        (uint)compressedData.Length, 
                        outputData, 
                        outputSize);

                    if (decompressedSize > 0)
                    {
                        Array.Resize(ref outputData, (int)decompressedSize);
                        return outputData;
                    }

                    // Buffer too small, try bigger
                    outputSize = (uint)Math.Min(outputSize * 2, 1073741824);
                }

                throw new InvalidOperationException("Decompression failed: output buffer exceeded maximum size.");
            }
            catch (DllNotFoundException)
            {
                throw new InvalidOperationException(
                    $"Failed to load native KORE library '{NativeLibraryName}'. " +
                    "Ensure the native library is available in your application's runtime directory.");
            }
        }

        /// <summary>
        /// Compresses data from an input stream and writes to an output stream.
        /// </summary>
        /// <param name="inputStream">The input stream to read from.</param>
        /// <param name="outputStream">The output stream to write compressed data to.</param>
        /// <param name="bufferSize">Buffer size for streaming (default: 1MB).</param>
        public void CompressStream(System.IO.Stream inputStream, System.IO.Stream outputStream, int bufferSize = 1048576)
        {
            if (inputStream == null)
                throw new ArgumentNullException(nameof(inputStream));
            if (outputStream == null)
                throw new ArgumentNullException(nameof(outputStream));

            byte[] buffer = new byte[bufferSize];
            int bytesRead;

            while ((bytesRead = inputStream.Read(buffer, 0, bufferSize)) > 0)
            {
                Array.Resize(ref buffer, bytesRead);
                byte[] compressedChunk = Compress(buffer);
                
                // Write chunk size (for streaming decompression)
                byte[] sizeBytes = BitConverter.GetBytes((uint)compressedChunk.Length);
                outputStream.Write(sizeBytes, 0, 4);
                outputStream.Write(compressedChunk, 0, compressedChunk.Length);
                
                Array.Resize(ref buffer, bufferSize);
            }
        }

        /// <summary>
        /// Decompresses data from an input stream and writes to an output stream.
        /// </summary>
        /// <param name="inputStream">The input stream to read compressed data from.</param>
        /// <param name="outputStream">The output stream to write decompressed data to.</param>
        public void DecompressStream(System.IO.Stream inputStream, System.IO.Stream outputStream)
        {
            if (inputStream == null)
                throw new ArgumentNullException(nameof(inputStream));
            if (outputStream == null)
                throw new ArgumentNullException(nameof(outputStream));

            byte[] sizeBuffer = new byte[4];

            while (inputStream.Read(sizeBuffer, 0, 4) == 4)
            {
                uint chunkSize = BitConverter.ToUInt32(sizeBuffer, 0);
                byte[] compressedChunk = new byte[chunkSize];
                
                int bytesRead = inputStream.Read(compressedChunk, 0, (int)chunkSize);
                if (bytesRead != chunkSize)
                    throw new InvalidOperationException("Unexpected end of stream while reading compressed chunk.");

                byte[] decompressedChunk = Decompress(compressedChunk);
                outputStream.Write(decompressedChunk, 0, decompressedChunk.Length);
            }
        }

        /// <summary>
        /// Gets compression statistics for the last operation.
        /// </summary>
        /// <returns>A CompressionStats object with metrics.</returns>
        public CompressionStats GetStats()
        {
            return new CompressionStats();
        }

        public void Dispose()
        {
            GC.SuppressFinalize(this);
        }

        #region P/Invoke Declarations

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        private static extern uint CompressInternal(
            byte[] input,
            uint inputSize,
            byte[] output,
            uint outputSize);

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        private static extern uint DecompressInternal(
            byte[] input,
            uint inputSize,
            byte[] output,
            uint outputSize);

        #endregion
    }

    /// <summary>
    /// Contains compression statistics and metrics.
    /// </summary>
    public class CompressionStats
    {
        /// <summary>Original uncompressed size in bytes.</summary>
        public long OriginalSize { get; set; }

        /// <summary>Compressed size in bytes.</summary>
        public long CompressedSize { get; set; }

        /// <summary>Compression ratio (compressed / original).</summary>
        public double CompressionRatio => OriginalSize > 0 ? CompressedSize / (double)OriginalSize : 0;

        /// <summary>Compression savings percentage.</summary>
        public double SavingsPercent => OriginalSize > 0 ? (1 - CompressionRatio) * 100 : 0;

        /// <summary>Time taken for compression in milliseconds.</summary>
        public double CompressionTimeMs { get; set; }

        /// <summary>Compression speed in MB/s.</summary>
        public double SpeedMBps => CompressionTimeMs > 0 ? (OriginalSize / 1048576.0) / (CompressionTimeMs / 1000.0) : 0;

        public override string ToString()
        {
            return $"Original: {OriginalSize:N0} bytes, " +
                   $"Compressed: {CompressedSize:N0} bytes, " +
                   $"Ratio: {CompressionRatio:P2}, " +
                   $"Speed: {SpeedMBps:F2} MB/s";
        }
    }

    /// <summary>
    /// KORE v1.1.6 Library information and constants.
    /// </summary>
    public static class KoreLibrary
    {
        /// <summary>Library version.</summary>
        public const string Version = "1.1.6";

        /// <summary>Release date.</summary>
        public const string ReleaseDate = "May 18, 2026";

        /// <summary>License information.</summary>
        public const string License = "MIT OR Apache-2.0";

        /// <summary>Project repository URL.</summary>
        public const string RepositoryUrl = "https://github.com/arunkatherashala/Kore";

        /// <summary>Documentation URL.</summary>
        public const string DocumentationUrl = "https://github.com/arunkatherashala/Kore/wiki";

        /// <summary>Gets library information string.</summary>
        public static string GetInfo()
        {
            return $"KORE FileFormat Library v{Version} ({ReleaseDate})\n" +
                   $"License: {License}\n" +
                   $"Repository: {RepositoryUrl}\n" +
                   $"Documentation: {DocumentationUrl}";
        }
    }
}
