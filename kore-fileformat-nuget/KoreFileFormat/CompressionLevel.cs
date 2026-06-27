namespace KoreFileFormat
{
    /// <summary>
    /// Compression level for quality vs speed tradeoff
    /// </summary>
    public enum CompressionLevel
    {
        /// <summary>Maximum speed, lower compression ratio</summary>
        Fast = 0,
        
        /// <summary>Default balance between speed and ratio</summary>
        Balanced = 1,
        
        /// <summary>Maximum compression ratio, slower speed</summary>
        Maximum = 2,
    }
}
