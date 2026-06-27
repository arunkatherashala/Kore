#!/usr/bin/env ruby
# frozen_string_literal: true

# Example usage of Kore FileFormat library in Ruby

require 'kore_fileformat'

puts "=== Kore FileFormat Ruby Examples ==="
puts

# Example 1: Basic compression and decompression
puts "Example 1: Basic Compression"
puts "-" * 40

original_data = "The quick brown fox jumps over the lazy dog. " * 10
puts "Original size: #{original_data.bytesize} bytes"

compressed = KoreFileFormat.compress(original_data)
puts "Compressed size: #{compressed.bytesize} bytes"
ratio = ((1 - (compressed.bytesize.to_f / original_data.bytesize)) * 100).round(2)
puts "Compression ratio: #{ratio}%"

decompressed = KoreFileFormat.decompress(compressed)
puts "Decompressed matches: #{decompressed == original_data}"
puts

# Example 2: Compression levels
puts "Example 2: Compression Levels"
puts "-" * 40

data = "Hello, World! " * 1000

fast = KoreFileFormat.compress_with_level(data, :fast)
balanced = KoreFileFormat.compress_with_level(data, :balanced)
maximum = KoreFileFormat.compress_with_level(data, :maximum)

puts "Original: #{data.bytesize} bytes"
puts "Fast:     #{fast.bytesize} bytes"
puts "Balanced: #{balanced.bytesize} bytes"
puts "Maximum:  #{maximum.bytesize} bytes"
puts

# Example 3: Error handling
puts "Example 3: Error Handling"
puts "-" * 40

begin
  KoreFileFormat.decompress("invalid data")
rescue KoreFileFormat::Error => e
  puts "Caught expected error: #{e.class}"
end
puts

# Example 4: File operations (example only)
puts "Example 4: File Operations"
puts "-" * 40

test_file = "test_data.kore"
original_content = "File content " * 100

begin
  # Write compressed file
  File.write(test_file, KoreFileFormat.compress(original_content))
  puts "Wrote #{File.size(test_file)} bytes to #{test_file}"

  # Read and decompress
  compressed_content = File.read(test_file)
  decompressed_content = KoreFileFormat.decompress(compressed_content)
  puts "Decompressed: #{decompressed_content.bytesize} bytes"
  puts "Content matches: #{decompressed_content == original_content}"

  # Cleanup
  File.delete(test_file)
rescue => e
  puts "Error: #{e.message}"
ensure
  File.delete(test_file) if File.exist?(test_file)
end
puts

# Example 5: Performance benchmark (optional)
puts "Example 5: Performance"
puts "-" * 40

require 'benchmark'

data = "x" * (1024 * 1024)  # 1MB of data

time = Benchmark.measure do
  10.times { KoreFileFormat.compress(data) }
end

puts "10x compression of 1MB: #{time.real.round(3)}s"
puts "Throughput: #{(10 * 1024.0 / time.real).round(2)} MB/s"
