# frozen_string_literal: true

module KoreFileFormat
  class Decompressor
    def decompress(data)
      raise ArgumentError, "data cannot be nil" if data.nil?

      data_bytes = data.is_a?(String) ? data.bytes : data
      input = FFI::MemoryPointer.new(:uchar, data_bytes.length)
      input.put_array_of_uchar(0, data_bytes)

      output_size = (data_bytes.length * 4).to_i
      output = FFI::MemoryPointer.new(:uchar, output_size)
      decompressed_size = FFI::MemoryPointer.new(:int)

      result = Native.decompress_data(
        input, data_bytes.length,
        output, output_size,
        decompressed_size
      )

      raise CompressionError, "Decompression failed with code: #{result}" if result != 0

      output.get_array_of_uchar(0, decompressed_size.read_int).pack("c*")
    end
  end
end
