# frozen_string_literal: true

require_relative "kore_fileformat/version"
require_relative "kore_fileformat/native"
require_relative "kore_fileformat/compressor"
require_relative "kore_fileformat/decompressor"

module KoreFileFormat
  class Error < StandardError; end
  
  class CompressionError < Error; end

  # Compress data with default settings (Balanced)
  # @param data [String, Bytes] Data to compress
  # @return [String] Compressed data
  def self.compress(data)
    compress_with_level(data, :balanced)
  end

  # Compress data with specific level
  # @param data [String, Bytes] Data to compress
  # @param level [Symbol] Compression level (:fast, :balanced, :maximum)
  # @return [String] Compressed data
  def self.compress_with_level(data, level = :balanced)
    Compressor.new(level).compress(data)
  end

  # Decompress data
  # @param data [String, Bytes] Compressed data
  # @return [String] Decompressed data
  def self.decompress(data)
    Decompressor.new.decompress(data)
  end

  # Compression levels
  module CompressionLevel
    FAST = 0
    BALANCED = 1
    MAXIMUM = 2
  end
end
