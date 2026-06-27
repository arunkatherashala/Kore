# frozen_string_literal: true

require "ffi"

module KoreFileFormat
  module Native
    extend FFI::Library
    
    begin
      # Try multiple paths for the library
      lib_name = case RUBY_PLATFORM
                 when /win32|mingw/
                   "kore_fileformat.dll"
                 when /linux/
                   "libkore_fileformat.so"
                 when /darwin/
                   "libkore_fileformat.dylib"
                 else
                   "kore_fileformat"
                 end
      
      # Search in gem lib directory first, then system paths
      lib_path = File.expand_path(lib_name, File.dirname(__FILE__) + "/../..")
      if File.exist?(lib_path)
        ffi_lib lib_path
      else
        ffi_lib lib_name
      end
    rescue LoadError => e
      raise "Native KORE library not found. Error: #{e.message}"
    end

    # C function declarations
    attach_function :compress_data, :compress_data,
                    [:pointer, :int, :pointer, :int, :pointer, :int],
                    :int, blocking: true
    
    attach_function :decompress_data, :decompress_data,
                    [:pointer, :int, :pointer, :int, :pointer],
                    :int, blocking: true
    
    attach_function :get_version, :get_version,
                    [:pointer, :pointer, :pointer],
                    :int, blocking: true
  end
end
