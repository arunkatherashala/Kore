using System;
using System.Runtime.InteropServices;

namespace KoreFileFormat
{
    /// <summary>
    /// P/Invoke declarations for native KORE library
    /// </summary>
    internal static class Native
    {
        private const string LibraryName = "kore_fileformat";

        /// <summary>
        /// Compress data buffer
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, SetLastError = true)]
        public static extern int CompressData(
            byte[] input,
            int inputSize,
            byte[] output,
            int outputSize,
            out int compressedSize,
            int level
        );

        /// <summary>
        /// Decompress data buffer
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, SetLastError = true)]
        public static extern int DecompressData(
            byte[] input,
            int inputSize,
            byte[] output,
            int outputSize,
            out int decompressedSize
        );

        /// <summary>
        /// Get library version information
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, SetLastError = true)]
        public static extern int GetVersion(
            out int major,
            out int minor,
            out int patch
        );
    }
}
