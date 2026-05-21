using System;

namespace KoreFileFormat
{
    /// <summary>
    /// Exception thrown during compression/decompression operations
    /// </summary>
    public class CompressionException : Exception
    {
        /// <summary>Create a new CompressionException with a message</summary>
        public CompressionException(string message) : base(message) { }

        /// <summary>Create a new CompressionException with a message and inner exception</summary>
        public CompressionException(string message, Exception innerException) 
            : base(message, innerException) { }
    }
}
