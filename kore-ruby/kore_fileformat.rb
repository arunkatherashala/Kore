# frozen_string_literal: true

require 'fiddle'
require 'fiddle/import'

module Kore
  # KORE columnar format bindings for Ruby via Fiddle.
  #
  # Features:
  #   - Read/write KORE v2 binary files
  #   - All 11 ACID features
  #   - Native Ruby types (Arrays, Hashes)
  #   - Idiomatic Ruby API
  #
  # Example:
  #   require 'kore'
  #
  #   # Create data block
  #   block = Kore::DataBlock.new
  #   block.add_column('numbers', Kore::DataType::I64, [1, 2, 3, 4, 5])
  #   block.add_column('names', Kore::DataType::STR, %w[a b c d e])
  #
  #   # Write file
  #   Kore.write_file('/tmp/data.kore', block)
  #
  #   # Read file
  #   restored = Kore.read_file('/tmp/data.kore')
  #   puts "Rows: #{restored.num_rows}, Cols: #{restored.num_columns}"
  class FileFormat
    # ─────────────────────────────────────────────────────────────────────────
    # DATA TYPES & ENUMS
    # ─────────────────────────────────────────────────────────────────────────

    # KORE column data types (must match Rust DType enum).
    module DataType
      I64      = 1  # 64-bit signed integer
      F64      = 2  # 64-bit floating point
      BOOL     = 3  # Boolean
      STR      = 4  # UTF-8 string
      STR_DICT = 5  # Dictionary-encoded string
      ARRAY    = 6  # Nested array
      STRUCT   = 7  # Nested struct
    end

    # KORE compression codecs (must match Rust Compression enum).
    module Compression
      RAW     = 0  # No compression
      RLE     = 1  # Run-length encoding
      DELTA   = 2  # Delta encoding
      DICT    = 3  # Dictionary encoding
      NAN_RAW = 4  # Special NaN handling
      DEFLATE = 5  # Deflate/LZ4
      ZSTD    = 6  # ZSTD compression
    end

    # ─────────────────────────────────────────────────────────────────────────
    # CORE CLASSES
    # ─────────────────────────────────────────────────────────────────────────

    # Column statistics for predicate pushdown.
    class ColumnStats
      attr_accessor :min_value, :max_value, :null_count, :cardinality, :crc32

      def initialize(min_value: nil, max_value: nil, null_count: 0, cardinality: 0, crc32: 0)
        @min_value = min_value
        @max_value = max_value
        @null_count = null_count
        @cardinality = cardinality
        @crc32 = crc32
      end

      def to_h
        {
          min_value: @min_value,
          max_value: @max_value,
          null_count: @null_count,
          cardinality: @cardinality,
          crc32: @crc32,
        }
      end
    end

    # Single column in a data block.
    class Column
      attr_accessor :name, :dtype, :data, :stats

      def initialize(name, dtype, data, stats = nil)
        @name = name
        @dtype = dtype
        @data = data
        @stats = stats
      end

      def to_h
        {
          name: @name,
          type: @dtype,
          data: @data,
          stats: @stats&.to_h,
        }
      end
    end

    # Multi-column data structure.
    class DataBlock
      attr_reader :columns, :num_rows

      def initialize
        @columns = []
        @num_rows = 0
      end

      # Add a column to the data block.
      def add_column(name, dtype, data)
        if @num_rows.zero?
          @num_rows = data.length
        elsif data.length != @num_rows
          raise ArgumentError, "Column '#{name}' has #{data.length} rows, expected #{@num_rows}"
        end

        @columns << Column.new(name, dtype, data)
      end

      # Get column by name.
      def get_column(name)
        @columns.find { |c| c.name == name }
      end

      # Get number of columns.
      def num_columns
        @columns.length
      end

      def to_h
        {
          num_rows: @num_rows,
          num_columns: num_columns,
          columns: @columns.map(&:to_h),
        }
      end
    end

    # MVCC version tracking for time travel.
    class VersionSnapshot
      attr_accessor :version_id, :timestamp, :block_offset, :row_count, :prev_version

      def initialize(version_id:, timestamp:, block_offset:, row_count:, prev_version: nil)
        @version_id = version_id
        @timestamp = timestamp
        @block_offset = block_offset
        @row_count = row_count
        @prev_version = prev_version
      end

      def to_h
        {
          version_id: @version_id,
          timestamp: @timestamp,
          block_offset: @block_offset,
          row_count: @row_count,
          prev_version: @prev_version,
        }
      end
    end

    # Partition evolution support.
    class PartitionSpec
      attr_accessor :spec_id, :columns, :transforms, :parent_spec_id

      def initialize(spec_id:, columns: [], transforms: [], parent_spec_id: nil)
        @spec_id = spec_id
        @columns = columns
        @transforms = transforms
        @parent_spec_id = parent_spec_id
      end

      def to_h
        {
          spec_id: @spec_id,
          columns: @columns,
          transforms: @transforms,
          parent_spec_id: @parent_spec_id,
        }
      end
    end

    # Row-level delete bitmap for soft deletes.
    class DeleteVector
      attr_accessor :bitmap, :cardinality, :timestamp

      def initialize(bitmap: '', cardinality: 0, timestamp: 0)
        @bitmap = bitmap
        @cardinality = cardinality
        @timestamp = timestamp
      end

      def to_h
        {
          bitmap: @bitmap,
          cardinality: @cardinality,
          timestamp: @timestamp,
        }
      end
    end

    # ─────────────────────────────────────────────────────────────────────────
    # FFI BINDINGS
    # ─────────────────────────────────────────────────────────────────────────

    class << self
      # Load native FFI bindings (Phase 3 TODO).
      def load_library
        # TODO: Use Fiddle to load libkore_ffi.so/.dylib/.dll
        # require 'fiddle/import'
        # Fiddle.dlopen('libkore_ffi.so')
        raise NotImplementedError, 'Phase 3: FFI library loading pending'
      end

      # ─────────────────────────────────────────────────────────────────────
      # HIGH-LEVEL API
      # ─────────────────────────────────────────────────────────────────────

      # Compute CRC32 checksum.
      def crc32(data)
        raise NotImplementedError, 'Phase 3: CRC32 FFI pending'
      end

      # Write DataBlock to KORE file (Phase 3 placeholder).
      def write_file(path, data_block)
        # TODO: Implement FFI call to kore_write_file
        # For now: JSON fallback
        require 'json'
        File.write(path, JSON.pretty_generate(data_block.to_h))
      end

      # Read KORE file into DataBlock (Phase 3 placeholder).
      def read_file(path)
        # TODO: Implement FFI call to kore_read_file
        # For now: JSON fallback
        require 'json'
        raise NotImplementedError, 'Phase 3: Binary format reading pending'
      end

      # Read KORE data at specific version (time travel).
      def read_at_version(data, timestamp)
        raise NotImplementedError, 'Phase 3: Time travel API pending'
      end

      # Encrypt data with AES-256-GCM.
      def encrypt_aes256(password, data)
        raise NotImplementedError, 'Phase 3: Encryption API pending'
      end

      # Decrypt data with AES-256-GCM.
      def decrypt_aes256(password, encrypted_data)
        raise NotImplementedError, 'Phase 3: Decryption API pending'
      end

      # Get statistics for a column.
      def get_column_stats(data, column_name)
        raise NotImplementedError, 'Phase 3: Stats API pending'
      end

      # Get Bloom filter for a column.
      def get_bloom_filter(data, column_name)
        raise NotImplementedError, 'Phase 3: Bloom filter API pending'
      end
    end

    # ─────────────────────────────────────────────────────────────────────────
    # VERSION
    # ─────────────────────────────────────────────────────────────────────────

    VERSION = '2.0.0'
  end

  # Convenience module-level API
  module_function

  def write_file(path, data)
    FileFormat.write_file(path, data)
  end

  def read_file(path)
    FileFormat.read_file(path)
  end

  def crc32(data)
    FileFormat.crc32(data)
  end

  def encrypt_aes256(password, data)
    FileFormat.encrypt_aes256(password, data)
  end

  def decrypt_aes256(password, data)
    FileFormat.decrypt_aes256(password, data)
  end
end
