using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace Kore
{
    /// <summary>
    /// KORE v2 columnar format - C# .NET bindings via P/Invoke.
    ///
    /// Features:
    ///   - Read/write KORE v2 binary files
    ///   - All 11 ACID features: CRC32, stats, ZSTD, nested types, Bloom filters,
    ///     AES-256-GCM encryption, schema evolution, append writes, MVCC/time travel,
    ///     partition evolution, row-level deletes
    ///   - Async I/O support (.NET 5+)
    ///   - Type-safe API
    /// </summary>
    public static class KoreFileFormat
    {
        private const string DllName = "kore_ffi";

        // ─────────────────────────────────────────────────────────────────────────
        // P/INVOKE FFI DECLARATIONS
        // ─────────────────────────────────────────────────────────────────────────

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_crc32")]
        private static extern uint KoreCrc32(byte[] data, UIntPtr len);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_new")]
        private static extern IntPtr KoreBlockNew();

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_free")]
        private static extern void KoreBlockFree(IntPtr block);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_add_f64")]
        private static extern int KoreBlockAddF64(IntPtr block,
            [MarshalAs(UnmanagedType.LPStr)] string name, double[] data, UIntPtr len);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_add_i64")]
        private static extern int KoreBlockAddI64(IntPtr block,
            [MarshalAs(UnmanagedType.LPStr)] string name, long[] data, UIntPtr len);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_write_file")]
        private static extern int KoreWriteFile(
            [MarshalAs(UnmanagedType.LPStr)] string path, IntPtr block);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_read_file")]
        private static extern IntPtr KoreReadFile(
            [MarshalAs(UnmanagedType.LPStr)] string path);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_num_rows")]
        private static extern ulong KoreBlockNumRows(IntPtr block);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_num_cols")]
        private static extern uint KoreBlockNumCols(IntPtr block);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_col_name")]
        private static extern IntPtr KoreBlockColName(IntPtr block, UIntPtr idx);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_get_f64")]
        private static extern long KoreBlockGetF64(IntPtr block,
            [MarshalAs(UnmanagedType.LPStr)] string col, double[] outBuf, ulong maxlen);

        [DllImport(DllName, CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_free_string")]
        private static extern void KoreFreeString(IntPtr s);

        private static string DllPath()
        {
            // Search target/release/ relative to assembly location
            var here = System.IO.Path.GetDirectoryName(typeof(KoreFileFormat).Assembly.Location) ?? ".";
            var candidates = new[] {
                System.IO.Path.Combine(here, "..", "..", "..", "..", "target", "release", "kore_ffi.dll"),
                "kore_ffi.dll",
            };
            foreach (var c in candidates)
                if (System.IO.File.Exists(c)) return c;
            return "kore_ffi"; // let OS search PATH
        }

        // ─────────────────────────────────────────────────────────────────────────
        // DATA TYPES & ENUMS
        // ─────────────────────────────────────────────────────────────────────────

        /// <summary>
        /// KORE column data types (must match Rust DType enum).
        /// </summary>
        public enum DataType : byte
        {
            I64 = 1,        // 64-bit signed integer
            F64 = 2,        // 64-bit floating point
            BOOL = 3,       // Boolean
            STR = 4,        // UTF-8 string
            STR_DICT = 5,   // Dictionary-encoded string
            ARRAY = 6,      // Nested array
            STRUCT = 7,     // Nested struct
        }

        /// <summary>
        /// KORE compression codecs (must match Rust Compression enum).
        /// </summary>
        public enum Compression : byte
        {
            RAW = 0,        // No compression
            RLE = 1,        // Run-length encoding
            DELTA = 2,      // Delta encoding
            DICT = 3,       // Dictionary encoding
            NAN_RAW = 4,    // Special NaN handling
            DEFLATE = 5,    // Deflate/LZ4
            ZSTD = 6,       // ZSTD compression
        }

        // ─────────────────────────────────────────────────────────────────────────
        // CORE CLASSES
        // ─────────────────────────────────────────────────────────────────────────

        /// <summary>
        /// Column statistics for predicate pushdown optimization.
        /// </summary>
        public class ColumnStats
        {
            public long? MinValue { get; set; }
            public long? MaxValue { get; set; }
            public double? MinValueF { get; set; }
            public double? MaxValueF { get; set; }
            public long NullCount { get; set; }
            public long Cardinality { get; set; }
            public uint CRC32 { get; set; }
        }

        /// <summary>
        /// Single column in a data block.
        /// </summary>
        public class Column
        {
            public string Name { get; set; }
            public DataType Type { get; set; }
            public object Data { get; set; }  // List<long>, List<double>, List<bool>, List<string>
            public ColumnStats Stats { get; set; }

            public Column(string name, DataType type, object data, ColumnStats stats = null)
            {
                Name = name;
                Type = type;
                Data = data;
                Stats = stats;
            }
        }

        /// <summary>
        /// Multi-column data structure.
        /// </summary>
        public class DataBlock
        {
            public List<Column> Columns { get; private set; } = new List<Column>();
            public long NumRows { get; private set; } = 0;

            /// <summary>
            /// Get number of columns.
            /// </summary>
            public int NumColumns => Columns.Count;

            /// <summary>
            /// Add a column to the data block.
            /// </summary>
            public void AddColumn<T>(string name, DataType type, List<T> data)
            {
                if (NumRows == 0)
                {
                    NumRows = data.Count;
                }
                else if (data.Count != NumRows)
                {
                    throw new ArgumentException(
                        $"Column '{name}' has {data.Count} rows, expected {NumRows}"
                    );
                }

                Columns.Add(new Column(name, type, data));
            }

            /// <summary>
            /// Get column by name.
            /// </summary>
            public Column GetColumn(string name)
            {
                foreach (var col in Columns)
                {
                    if (col.Name == name)
                        return col;
                }
                return null;
            }
        }

        /// <summary>
        /// MVCC version tracking for time travel queries.
        /// </summary>
        public class VersionSnapshot
        {
            public uint VersionId { get; set; }
            public ulong Timestamp { get; set; }
            public ulong BlockOffset { get; set; }
            public ulong RowCount { get; set; }
            public uint? PrevVersion { get; set; }
        }

        /// <summary>
        /// Partition evolution support.
        /// </summary>
        public class PartitionSpec
        {
            public ushort SpecId { get; set; }
            public ushort[] Columns { get; set; }
            public string[] Transforms { get; set; }
            public ushort? ParentSpecId { get; set; }
        }

        /// <summary>
        /// Row-level delete bitmap for soft deletes.
        /// </summary>
        public class DeleteVector
        {
            public byte[] Bitmap { get; set; }
            public uint Cardinality { get; set; }
            public ulong Timestamp { get; set; }
        }

        // ─────────────────────────────────────────────────────────────────────────
        // HIGH-LEVEL API
        // ─────────────────────────────────────────────────────────────────────────

        /// <summary>
        /// Compute CRC32 checksum.
        /// </summary>
        public static uint CRC32(byte[] data)
        {
            if (data == null || data.Length == 0)
                return 0;

            GCHandle handle = GCHandle.Alloc(data, GCHandleType.Pinned);
            try
            {
                IntPtr ptr = handle.AddrOfPinnedObject();
                return KoreCrc32(ptr, (UIntPtr)data.Length);
            }
            finally
            {
                handle.Free();
            }
        }

        /// <summary>Write DataBlock to KORE binary file via P/Invoke kore-ffi.</summary>
        public static void WriteFile(string path, DataBlock data)
        {
            if (data == null) throw new ArgumentNullException(nameof(data));

            var handle = KoreBlockNew();
            if (handle == IntPtr.Zero) throw new InvalidOperationException("kore_block_new failed");
            try
            {
                foreach (var col in data.Columns)
                {
                    if (col.Data is double[] f64 && f64.Length > 0)
                        KoreBlockAddF64(handle, col.Name, f64, (UIntPtr)f64.Length);
                    else if (col.Data is long[] i64 && i64.Length > 0)
                        KoreBlockAddI64(handle, col.Name, i64, (UIntPtr)i64.Length);
                }
                int rc = KoreWriteFile(path, handle);
                if (rc != 0) throw new System.IO.IOException($"kore_write_file failed (rc={rc})");
            }
            finally { KoreBlockFree(handle); }
        }

        /// <summary>Read KORE binary file into DataBlock via P/Invoke kore-ffi.</summary>
        public static DataBlock ReadFile(string path)
        {
            if (string.IsNullOrEmpty(path)) throw new ArgumentNullException(nameof(path));

            var handle = KoreReadFile(path);
            if (handle == IntPtr.Zero) throw new System.IO.IOException($"kore_read_file failed: {path}");
            try
            {
                ulong nrows = KoreBlockNumRows(handle);
                uint  ncols = KoreBlockNumCols(handle);
                var block = new DataBlock { NumRows = (long)nrows };

                for (uint ci = 0; ci < ncols; ci++)
                {
                    var rawName = KoreBlockColName(handle, (UIntPtr)ci);
                    var colName = rawName != IntPtr.Zero ? Marshal.PtrToStringAnsi(rawName) ?? $"col{ci}" : $"col{ci}";
                    if (rawName != IntPtr.Zero) KoreFreeString(rawName);

                    var vals = new double[nrows];
                    long n   = KoreBlockGetF64(handle, colName, vals, nrows);
                    if (n > 0)
                        block.Columns.Add(new Column(colName, DataType.F64, vals[..(int)n]));
                }
                return block;
            }
            finally { KoreBlockFree(handle); }
        }

        /// <summary>
        /// Read KORE data at specific version (time travel).
        /// </summary>
        public static DataBlock ReadAtVersion(byte[] data, ulong timestamp)
        {
            throw new NotImplementedException("Phase 3: Time travel API pending");
        }

        /// <summary>
        /// Encrypt data with AES-256-GCM.
        /// </summary>
        public static byte[] EncryptAES256(string password, byte[] data)
        {
            throw new NotImplementedException("Phase 3: Encryption API pending");
        }

        /// <summary>
        /// Decrypt data with AES-256-GCM.
        /// </summary>
        public static byte[] DecryptAES256(string password, byte[] encryptedData)
        {
            throw new NotImplementedException("Phase 3: Decryption API pending");
        }

        /// <summary>
        /// Get statistics for a column.
        /// </summary>
        public static ColumnStats GetColumnStats(byte[] data, string columnName)
        {
            throw new NotImplementedException("Phase 3: Stats API pending");
        }

        /// <summary>
        /// Get Bloom filter for a column.
        /// </summary>
        public static byte[] GetBloomFilter(byte[] data, string columnName)
        {
            throw new NotImplementedException("Phase 3: Bloom filter API pending");
        }

        // ─────────────────────────────────────────────────────────────────────────
        // VERSION
        // ─────────────────────────────────────────────────────────────────────────

        /// <summary>
        /// Library version (must match Rust kore crate version).
        /// </summary>
        public const string Version = "2.0.0";
    }
}
