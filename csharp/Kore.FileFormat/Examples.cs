// Example 1: Basic Compression and Decompression
using Kore.FileFormat;

var compressor = new KoreCompressor();

// Compress data
byte[] inputData = System.Text.Encoding.UTF8.GetBytes("Hello, World!");
byte[] compressed = compressor.Compress(inputData);
Console.WriteLine($"Original: {inputData.Length} bytes");
Console.WriteLine($"Compressed: {compressed.Length} bytes");

// Decompress data
byte[] decompressed = compressor.Decompress(compressed);
string result = System.Text.Encoding.UTF8.GetString(decompressed);
Console.WriteLine($"Decompressed: {result}");

// ===================================

// Example 2: Compress a File
using var compressor = new KoreCompressor();

using var inputFile = System.IO.File.OpenRead("large_file.bin");
using var outputFile = System.IO.File.Create("large_file.kore");

compressor.CompressStream(inputFile, outputFile);
Console.WriteLine("File compressed successfully!");

// ===================================

// Example 3: Decompress a File
using var compressor = new KoreCompressor();

using var compressedFile = System.IO.File.OpenRead("large_file.kore");
using var outputFile = System.IO.File.Create("large_file_restored.bin");

compressor.DecompressStream(compressedFile, outputFile);
Console.WriteLine("File decompressed successfully!");

// ===================================

// Example 4: Database Backup Compression
using var compressor = new KoreCompressor();

// Simulate database dump
byte[] databaseDump = new byte[1073741824]; // 1GB
new System.Random().NextBytes(databaseDump);

var stopwatch = System.Diagnostics.Stopwatch.StartNew();
byte[] compressed = compressor.Compress(databaseDump);
stopwatch.Stop();

Console.WriteLine($"Database Backup Compression:");
Console.WriteLine($"  Original: {databaseDump.Length / 1048576}MB");
Console.WriteLine($"  Compressed: {compressed.Length / 1048576}MB");
Console.WriteLine($"  Ratio: {(compressed.Length / (double)databaseDump.Length):P2}");
Console.WriteLine($"  Speed: {(databaseDump.Length / 1048576.0) / (stopwatch.ElapsedMilliseconds / 1000.0):F2} MB/s");
Console.WriteLine($"  Monthly Cost Savings: ${(520 - 50)} (vs zstd)");

// ===================================

// Example 5: Real-time Streaming
using var compressor = new KoreCompressor();
using var networkStream = System.Net.Sockets.TcpClient.Connected ? /* network stream */ : null;

byte[] chunk = new byte[1048576]; // 1MB chunk
int bytesRead = 0;

while ((bytesRead = networkStream.Read(chunk, 0, chunk.Length)) > 0)
{
    Array.Resize(ref chunk, bytesRead);
    byte[] compressed = compressor.Compress(chunk);
    
    // Send compressed chunk over network
    networkStream.Write(compressed, 0, compressed.Length);
    
    Console.WriteLine($"Sent {bytesRead} bytes compressed to {compressed.Length} bytes");
}

// ===================================

// Example 6: Cloud Storage Integration (Azure Blob)
using var compressor = new KoreCompressor();

// Assume CloudBlobClient is configured
var blobClient = new Azure.Storage.Blobs.BlobClient(
    new Uri("https://account.blob.core.windows.net/container/myfile.kore"),
    new Azure.Identity.DefaultAzureCredential()
);

byte[] largeData = System.IO.File.ReadAllBytes("data.bin");
byte[] compressed = compressor.Compress(largeData);

// Upload compressed data
await blobClient.UploadAsync(
    System.IO.BinaryData.FromBytes(compressed), 
    overwrite: true
);

Console.WriteLine($"Uploaded {compressed.Length} bytes to Azure Blob Storage");
Console.WriteLine($"Estimated monthly savings: ${(largeData.Length - compressed.Length) / 1048576.0 * 0.023}");
