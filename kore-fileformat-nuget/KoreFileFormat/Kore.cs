using System;

namespace KoreFileFormat
{
    /// <summary>
    /// KORE high-performance compression library main API
    /// </summary>
    public static class Kore
    {
        /// <summary>Library major version</summary>
        public const int MajorVersion = 1;

        /// <summary>Library minor version</summary>
        public const int MinorVersion = 2;

        /// <summary>Library patch version</summary>
        public const int PatchVersion = 1;

        /// <summary>
        /// Compress data with default settings (Balanced)
        /// </summary>
        /// <param name="data">Input data to compress</param>
        /// <returns>Compressed data</returns>
        /// <exception cref="ArgumentNullException">Thrown if data is null</exception>
        /// <exception cref="CompressionException">Thrown if compression fails</exception>
        public static byte[] Compress(byte[] data)
        {
            return Compress(data, CompressionLevel.Balanced);
        }

        /// <summary>
        /// Compress data with specified level
        /// </summary>
        /// <param name="data">Input data to compress</param>
        /// <param name="level">Compression level (Fast, Balanced, Maximum)</param>
        /// <returns>Compressed data</returns>
        /// <exception cref="ArgumentNullException">Thrown if data is null</exception>
        /// <exception cref="CompressionException">Thrown if compression fails</exception>
        public static byte[] Compress(byte[] data, CompressionLevel level)
        {
            if (data == null)
                throw new ArgumentNullException(nameof(data));

            // Allocate output buffer (1.5x input size for safety)
            var output = new byte[(int)(data.Length * 1.5) + 1024];
            int outputSize = 0;

            try
            {
                int result = Native.CompressData(
                    data, data.Length,
                    output, output.Length,
                    out outputSize,
                    (int)level
                );

                if (result != 0)
                    throw new CompressionException($"Native compression failed with code: {result}");

                // Resize to actual compressed size
                Array.Resize(ref output, outputSize);
                return output;
            }
            catch (DllNotFoundException)
            {
                throw new CompressionException(
                    "Native KORE library not found. Ensure proper installation and that the correct architecture binaries are available.",
                    new DllNotFoundException()
                );
            }
        }

        /// <summary>
        /// Decompress data
        /// </summary>
        /// <param name="data">Compressed data to decompress</param>
        /// <returns>Decompressed data</returns>
        /// <exception cref="ArgumentNullException">Thrown if data is null</exception>
        /// <exception cref="CompressionException">Thrown if decompression fails</exception>
        public static byte[] Decompress(byte[] data)
        {
            if (data == null)
                throw new ArgumentNullException(nameof(data));

            // Allocate larger output buffer for decompression
            var output = new byte[data.Length * 4];
            int outputSize = 0;

            try
            {
                int result = Native.DecompressData(
                    data, data.Length,
                    output, output.Length,
                    out outputSize
                );

                if (result != 0)
                    throw new CompressionException($"Native decompression failed with code: {result}");

                // Resize to actual decompressed size
                Array.Resize(ref output, outputSize);
                return output;
            }
            catch (DllNotFoundException)
            {
                throw new CompressionException(
                    "Native KORE library not found. Ensure proper installation and that the correct architecture binaries are available.",
                    new DllNotFoundException()
                );
            }
        }

        /// <summary>
        /// Get library version
        /// </summary>
        public static Version GetLibraryVersion()
        {
            try
            {
                int result = Native.GetVersion(out int major, out int minor, out int patch);
                if (result != 0)
                    throw new CompressionException($"Failed to get version: {result}");
                
                return new Version(major, minor, patch);
            }
            catch (DllNotFoundException)
            {
                return new Version(MajorVersion, MinorVersion, PatchVersion);
            }
        }
    }
}
