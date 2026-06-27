# frozen_string_literal: true

module KoreFileFormat
  class Compressor
    def initialize(level = :balanced)
      @level = parse_level(level)
    end

    def compress(data)
      raise ArgumentError, "data cannot be nil" if data.nil?

      data_bytes = data.is_a?(String) ? data.bytes : data
      input = FFI::MemoryPointer.new(:uchar, data_bytes.length)
      input.put_array_of_uchar(0, data_bytes)

      output_size = (data_bytes.length * 1.5).to_i + 1024
      output = FFI::MemoryPointer.new(:uchar, output_size)
      compressed_size = FFI::MemoryPointer.new(:int)

      result = Native.compress_data(
        input, data_bytes.length,
        output, output_size,
        compressed_size,
        @level
      )

      raise CompressionError, "Compression failed with code: #{result}" if result != 0

      output.get_array_of_uchar(0, compressed_size.read_int).pack("c*")
    end

    private

    def parse_level(level)
      case level
      when :fast
        CompressionLevel::FAST
      when :balanced
        CompressionLevel::BALANCED
      when :maximum
        CompressionLevel::MAXIMUM
      when Integer
        level
      else
        raise ArgumentError, "Invalid compression level: #{level}"
      end
    end
  end
end
