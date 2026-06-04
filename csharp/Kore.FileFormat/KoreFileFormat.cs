using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace Kore.FileFormat
{
    /// <summary>
    /// Provides methods for reading and writing Kore columnar files.
    /// </summary>
    public class KoreFileReader : IDisposable
    {
        private const string KORE_MAGIC = "KORE";
        private const byte KORE_VERSION = 2;
        private const int CHUNK_ROWS = 65536;

        private FileStream? _fileStream;
        private BinaryReader? _binaryReader;
        private KoreFileHeader? _header;
        private KoreFileColumn[]? _columns;

        /// <summary>
        /// Opens a Kore file for reading.
        /// </summary>
        /// <param name="path">Path to the Kore file</param>
        /// <exception cref="FileNotFoundException">Thrown when file does not exist</exception>
        /// <exception cref="InvalidOperationException">Thrown when file is not a valid Kore file</exception>
        public KoreFileReader(string path)
        {
            if (!File.Exists(path))
                throw new FileNotFoundException($"File not found: {path}");

            _fileStream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
            _binaryReader = new BinaryReader(_fileStream);

            ReadHeader();
        }

        /// <summary>
        /// Reads the file header and validates the Kore format.
        /// </summary>
        private void ReadHeader()
        {
            if (_binaryReader == null)
                throw new InvalidOperationException("Reader not initialized");

            byte[] magicBytes = _binaryReader.ReadBytes(4);
            string magic = Encoding.ASCII.GetString(magicBytes);

            if (magic != KORE_MAGIC)
                throw new InvalidOperationException("Invalid Kore file: bad magic bytes");

            byte version = _binaryReader.ReadByte();
            if (version != KORE_VERSION)
                throw new InvalidOperationException($"Unsupported Kore version: {version}");

            byte reserved = _binaryReader.ReadByte();
            ushort numCols = _binaryReader.ReadUInt16();
            ulong numRows = _binaryReader.ReadUInt64();

            _header = new KoreFileHeader
            {
                Magic = magic,
                Version = version,
                NumColumns = numCols,
                NumRows = numRows,
                NumChunks = (uint)((numRows + CHUNK_ROWS - 1) / CHUNK_ROWS)
            };

            // Read column metadata
            _columns = new KoreFileColumn[numCols];
            for (int i = 0; i < numCols; i++)
            {
                byte[] colBuf = _binaryReader.ReadBytes(256);
                
                ushort nameLen = BitConverter.ToUInt16(colBuf, 0);
                string name = Encoding.UTF8.GetString(colBuf, 2, Math.Min(nameLen, (ushort)64));
                string type = Encoding.UTF8.GetString(colBuf, 66, 64).TrimEnd('\0');
                ulong offset = BitConverter.ToUInt64(colBuf, 130);
                uint length = BitConverter.ToUInt32(colBuf, 138);
                bool encoded = colBuf[142] != 0;

                _columns[i] = new KoreFileColumn
                {
                    Name = name,
                    Type = type,
                    Offset = offset,
                    Length = length,
                    Encoded = encoded
                };
            }
        }

        /// <summary>
        /// Reads all data from the file as a list of columns (each column is a list of strings).
        /// </summary>
        /// <returns>List of columns, where each column contains string values</returns>
        public List<List<string>> ReadAllData()
        {
            if (_header == null || _columns == null)
                throw new InvalidOperationException("Header not loaded");

            var result = new List<List<string>>(_columns.Length);
            for (int i = 0; i < _columns.Length; i++)
            {
                result.Add(new List<string>((int)_header.NumRows));
            }

            // Read data chunk by chunk
            for (int chunk = 0; chunk < _header.NumChunks; chunk++)
            {
                uint chunkSize = CHUNK_ROWS;
                if ((ulong)chunk * CHUNK_ROWS + CHUNK_ROWS > _header.NumRows)
                {
                    chunkSize = (uint)(_header.NumRows - (ulong)chunk * CHUNK_ROWS);
                }

                for (int col = 0; col < _columns.Length; col++)
                {
                    for (int row = 0; row < chunkSize; row++)
                    {
                        uint strLen = _binaryReader!.ReadUInt32();
                        string value = "";
                        if (strLen > 0)
                        {
                            byte[] strBuf = _binaryReader.ReadBytes((int)strLen);
                            value = Encoding.UTF8.GetString(strBuf);
                        }
                        result[col].Add(value);
                    }
                }
            }

            return result;
        }

        /// <summary>
        /// Reads a specific column from the file.
        /// </summary>
        /// <param name="columnIndex">The zero-based index of the column to read</param>
        /// <returns>List of string values for the specified column</returns>
        public List<string> ReadColumn(int columnIndex)
        {
            if (_header == null || _columns == null)
                throw new InvalidOperationException("Header not loaded");

            if (columnIndex < 0 || columnIndex >= _columns.Length)
                throw new ArgumentOutOfRangeException(nameof(columnIndex));

            var column = new List<string>((int)_header.NumRows);

            for (int chunk = 0; chunk < _header.NumChunks; chunk++)
            {
                uint chunkSize = CHUNK_ROWS;
                if ((ulong)chunk * CHUNK_ROWS + CHUNK_ROWS > _header.NumRows)
                {
                    chunkSize = (uint)(_header.NumRows - (ulong)chunk * CHUNK_ROWS);
                }

                for (int row = 0; row < chunkSize; row++)
                {
                    uint strLen = _binaryReader!.ReadUInt32();
                    string value = "";
                    if (strLen > 0)
                    {
                        byte[] strBuf = _binaryReader.ReadBytes((int)strLen);
                        value = Encoding.UTF8.GetString(strBuf);
                    }
                    column.Add(value);
                }
            }

            return column;
        }

        /// <summary>
        /// Gets the file header information.
        /// </summary>
        public KoreFileHeader Header => _header ?? throw new InvalidOperationException("Header not loaded");

        /// <summary>
        /// Gets the column metadata.
        /// </summary>
        public KoreFileColumn[] Columns => _columns ?? throw new InvalidOperationException("Columns not loaded");

        /// <summary>
        /// Disposes resources used by the reader.
        /// </summary>
        public void Dispose()
        {
            _binaryReader?.Dispose();
            _fileStream?.Dispose();
        }
    }

    /// <summary>
    /// Provides methods for writing Kore columnar files.
    /// </summary>
    public class KoreFileWriter : IDisposable
    {
        private const string KORE_MAGIC = "KORE";
        private const byte KORE_VERSION = 2;
        private const int CHUNK_ROWS = 65536;

        private FileStream? _fileStream;
        private BinaryWriter? _binaryWriter;
        private bool _disposed = false;

        /// <summary>
        /// Creates a new Kore file for writing.
        /// </summary>
        /// <param name="path">Path where the Kore file will be written</param>
        public KoreFileWriter(string path)
        {
            _fileStream = new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.None);
            _binaryWriter = new BinaryWriter(_fileStream);
        }

        /// <summary>
        /// Writes column-oriented data to the Kore file.
        /// </summary>
        /// <param name="columns">List of columns, where each column is a list of string values</param>
        public void WriteData(List<List<string>> columns)
        {
            if (_binaryWriter == null)
                throw new InvalidOperationException("Writer not initialized");

            if (columns == null || columns.Count == 0)
                throw new ArgumentException("Columns cannot be empty");

            int numRows = columns[0].Count;
            for (int i = 1; i < columns.Count; i++)
            {
                if (columns[i].Count != numRows)
                    throw new ArgumentException("All columns must have the same number of rows");
            }

            // Write header
            _binaryWriter.Write(Encoding.ASCII.GetBytes(KORE_MAGIC));
            _binaryWriter.Write(KORE_VERSION);
            _binaryWriter.Write((byte)0); // Reserved
            _binaryWriter.Write((ushort)columns.Count);
            _binaryWriter.Write((ulong)numRows);

            // Write column metadata
            for (int i = 0; i < columns.Count; i++)
            {
                byte[] colBuf = new byte[256];
                
                string colName = $"col_{i}";
                byte[] nameBytes = Encoding.UTF8.GetBytes(colName);
                BitConverter.GetBytes((ushort)nameBytes.Length).CopyTo(colBuf, 0);
                nameBytes.CopyTo(colBuf, 2);

                string colType = "string";
                byte[] typeBytes = Encoding.UTF8.GetBytes(colType);
                typeBytes.CopyTo(colBuf, 66);

                _binaryWriter.Write(colBuf);
            }

            // Write data
            for (int col = 0; col < columns.Count; col++)
            {
                for (int row = 0; row < numRows; row++)
                {
                    string value = columns[col][row];
                    byte[] valueBytes = Encoding.UTF8.GetBytes(value ?? "");
                    
                    _binaryWriter.Write((uint)valueBytes.Length);
                    if (valueBytes.Length > 0)
                    {
                        _binaryWriter.Write(valueBytes);
                    }
                }
            }

            _binaryWriter.Flush();
        }

        /// <summary>
        /// Disposes resources used by the writer.
        /// </summary>
        public void Dispose()
        {
            if (!_disposed)
            {
                _binaryWriter?.Dispose();
                _fileStream?.Dispose();
                _disposed = true;
            }
        }
    }

    /// <summary>
    /// Represents the header of a Kore file.
    /// </summary>
    public class KoreFileHeader
    {
        public string Magic { get; set; } = "";
        public byte Version { get; set; }
        public ushort NumColumns { get; set; }
        public ulong NumRows { get; set; }
        public uint NumChunks { get; set; }
    }

    /// <summary>
    /// Represents metadata for a column in a Kore file.
    /// </summary>
    public class KoreFileColumn
    {
        public string Name { get; set; } = "";
        public string Type { get; set; } = "";
        public ulong Offset { get; set; }
        public uint Length { get; set; }
        public bool Encoded { get; set; }
    }

    /// <summary>
    /// Convenience class for reading and writing Kore files.
    /// </summary>
    public static class KoreFile
    {
        /// <summary>
        /// Reads an entire Kore file.
        /// </summary>
        public static List<List<string>> Read(string path)
        {
            using (var reader = new KoreFileReader(path))
            {
                return reader.ReadAllData();
            }
        }

        /// <summary>
        /// Writes an entire Kore file.
        /// </summary>
        public static void Write(string path, List<List<string>> data)
        {
            using (var writer = new KoreFileWriter(path))
            {
                writer.WriteData(data);
            }
        }

        /// <summary>
        /// Gets the version of the Kore FileFormat library.
        /// </summary>
        public static string Version => "1.2.2";
    }
}
