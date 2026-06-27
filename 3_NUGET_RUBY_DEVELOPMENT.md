# KORE v1.2.1 NuGet & Ruby Development - Starter Files & Setup

**Purpose**: Initialize NuGet (C#/.NET) and Ruby gem development for v1.2.1  
**Timeline**: Weeks 1-6 of v1.2.1 (June 1 - July 15, 2026)  
**Deliverables**: Production-ready packages on NuGet and RubyGems  
**Owner**: Engineering Team

---

## 🔧 NuGet Package Setup (C# / .NET)

### 1. Dependency Check

**Verify Rust has C ABI export**:
```bash
# Check Cargo.toml [lib] section
[lib]
crate-type = ["cdylib"]  # Required for C interface

# Check for C header generation in build.rs
cargo build --release
# Should produce: target/release/kore_fileformat.dll (Windows)
#                 target/release/libkore_fileformat.so (Linux)
#                 target/release/libkore_fileformat.dylib (macOS)
```

### 2. Project Structure

```
kore-fileformat-nuget/
├── KoreFileFormat/
│   ├── KoreFileFormat.csproj          [Package config]
│   ├── Kore.cs                        [Main API]
│   ├── Compressor.cs                  [Compress class]
│   ├── Decompressor.cs                [Decompress class]
│   ├── CompressionLevel.cs            [Enum: Fast/Balanced/Maximum]
│   ├── CompressionException.cs        [Error handling]
│   ├── Native.cs                      [P/Invoke declarations]
│   ├── bin/
│   │   └── Release/net6.0/
│   │       ├── runtimes/win-x64/native/kore_fileformat.dll
│   │       ├── runtimes/linux-x64/native/libkore_fileformat.so
│   │       └── runtimes/osx-arm64/native/libkore_fileformat.dylib
│   └── obj/
├── Tests/
│   ├── KoreFileFormat.Tests.csproj
│   ├── CompressorTests.cs
│   ├── DecompressorTests.cs
│   └── IntegrationTests.cs
├── README.md
├── LICENSE
└── kore-fileformat.nuspec              [NuGet manifest]
```

### 3. KoreFileFormat.csproj Template

```xml
<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <TargetFrameworks>net6.0;net7.0;net8.0</TargetFrameworks>
    <RuntimeIdentifiers>win-x64;linux-x64;osx-arm64;osx-x64</RuntimeIdentifiers>
    <LangVersion>latest</LangVersion>
    <Nullable>enable</Nullable>
    <GenerateDocumentationFile>true</GenerateDocumentationFile>
    
    <!-- NuGet Package Info -->
    <PackageId>kore-fileformat</PackageId>
    <Version>1.2.1</Version>
    <Title>KORE File Format - High-Performance Compression</Title>
    <Authors>Sai Arun Kumar Katherashala</Authors>
    <Description>
      KORE is a high-performance, multi-platform file compression format.
      
      Key Features:
      • 19.1 GB/s throughput (verified)
      • 42.1% compression ratio
      • &lt;1ms metadata extraction
      • Production-validated
      
      Supports .NET 6, 7, 8 on Windows, Linux, macOS
    </Description>
    <PackageProjectUrl>https://github.com/arunkatherashala/Kore</PackageProjectUrl>
    <PackageLicenseExpression>Apache-2.0</PackageLicenseExpression>
    <RepositoryUrl>https://github.com/arunkatherashala/Kore</RepositoryUrl>
    <RepositoryType>git</RepositoryType>
    <PackageTags>compression;data-format;performance;library</PackageTags>
    <PackageReleaseNotes>
      v1.2.1: Production release with .NET support
      - Full C# bindings
      - 3 compression levels (Fast, Balanced, Maximum)
      - Multi-platform support
      - Comprehensive error handling
    </PackageReleaseNotes>
  </PropertyGroup>

</Project>
```

### 4. Core API Template (Kore.cs)

```csharp
using System;

namespace KoreFileFormat
{
    /// <summary>
    /// KORE high-performance compression library
    /// </summary>
    public static class Kore
    {
        public const int MajorVersion = 1;
        public const int MinorVersion = 2;
        public const int PatchVersion = 1;

        /// <summary>
        /// Compress data with default settings (Balanced)
        /// </summary>
        public static byte[] Compress(byte[] data)
        {
            return Compress(data, CompressionLevel.Balanced);
        }

        /// <summary>
        /// Compress data with specified level
        /// </summary>
        public static byte[] Compress(byte[] data, CompressionLevel level)
        {
            if (data == null)
                throw new ArgumentNullException(nameof(data));

            var output = new byte[data.Length * 2];  // Allocate buffer
            int outputSize = 0;

            try
            {
                int result = Native.CompressData(
                    data, data.Length,
                    output, output.Length,
                    out outputSize,
                    (int)level
                );

                if (result != 0)
                    throw new CompressionException($"Compression failed: {result}");

                Array.Resize(ref output, outputSize);
                return output;
            }
            catch (DllNotFoundException)
            {
                throw new CompressionException(
                    "Native KORE library not found. Ensure proper installation."
                );
            }
        }

        /// <summary>
        /// Decompress data
        /// </summary>
        public static byte[] Decompress(byte[] data)
        {
            if (data == null)
                throw new ArgumentNullException(nameof(data));

            var output = new byte[data.Length * 4];  // Initial buffer
            int outputSize = 0;

            try
            {
                int result = Native.DecompressData(
                    data, data.Length,
                    output, output.Length,
                    out outputSize
                );

                if (result != 0)
                    throw new CompressionException($"Decompression failed: {result}");

                Array.Resize(ref output, outputSize);
                return output;
            }
            catch (DllNotFoundException)
            {
                throw new CompressionException(
                    "Native KORE library not found. Ensure proper installation."
                );
            }
        }
    }

    /// <summary>
    /// Compression level for quality vs speed tradeoff
    /// </summary>
    public enum CompressionLevel
    {
        Fast = 0,           // Maximum speed, lower compression ratio
        Balanced = 1,       // Default balance
        Maximum = 2,        // Maximum compression ratio, slower
    }

    /// <summary>
    /// Exception thrown during compression/decompression
    /// </summary>
    public class CompressionException : Exception
    {
        public CompressionException(string message) : base(message) { }
    }
}
```

### 5. P/Invoke Native Bindings (Native.cs)

```csharp
using System;
using System.Runtime.InteropServices;

namespace KoreFileFormat
{
    internal static class Native
    {
        private const string LibraryName = "kore_fileformat";

        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl)]
        public static extern int CompressData(
            byte[] input, int inputSize,
            byte[] output, int outputSize,
            out int compressedSize,
            int level
        );

        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl)]
        public static extern int DecompressData(
            byte[] input, int inputSize,
            byte[] output, int outputSize,
            out int decompressedSize
        );

        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl)]
        public static extern int GetVersion(
            out int major,
            out int minor,
            out int patch
        );
    }
}
```

### 6. GitHub Actions Workflow (.github/workflows/publish-nuget.yml)

```yaml
name: Publish to NuGet

on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Setup .NET
        uses: actions/setup-dotnet@v3
        with:
          dotnet-version: '7.0'
      
      - name: Download pre-built binaries
        uses: actions/download-artifact@v3
        with:
          name: native-binaries
          path: KoreFileFormat/bin/Release/net7.0/
      
      - name: Build NuGet package
        run: dotnet pack -c Release
      
      - name: Publish to NuGet
        run: dotnet nuget push **/*.nupkg -s https://api.nuget.org/v3/index.json -k ${{ secrets.NUGET_API_KEY }}
```

### 7. Testing (CompressorTests.cs)

```csharp
using Xunit;

namespace KoreFileFormat.Tests
{
    public class CompressorTests
    {
        [Fact]
        public void Compress_WithValidData_ReturnsCompressedData()
        {
            var data = new byte[] { 1, 2, 3, 4, 5 };
            var result = Kore.Compress(data);
            
            Assert.NotNull(result);
            Assert.NotEmpty(result);
        }

        [Fact]
        public void Compress_WithNullData_ThrowsException()
        {
            Assert.Throws<ArgumentNullException>(() => Kore.Compress(null));
        }

        [Fact]
        public void Compress_Decompress_RoundTrip()
        {
            var original = new byte[] { 72, 101, 108, 108, 111 };  // "Hello"
            var compressed = Kore.Compress(original);
            var decompressed = Kore.Decompress(compressed);
            
            Assert.Equal(original, decompressed);
        }

        [Fact]
        public void Compress_AllLevels_Succeeds()
        {
            var data = new byte[1024];
            
            foreach (CompressionLevel level in System.Enum.GetValues(typeof(CompressionLevel)))
            {
                var result = Kore.Compress(data, level);
                Assert.NotNull(result);
            }
        }
    }
}
```

---

## 💎 Ruby Gem Setup

### 1. Project Structure

```
kore-fileformat-ruby/
├── lib/
│   ├── kore_fileformat.rb             [Main entry point]
│   ├── kore_fileformat/version.rb
│   ├── kore_fileformat/native.rb      [FFI declarations]
│   ├── kore_fileformat/compressor.rb
│   └── kore_fileformat/decompressor.rb
├── ext/
│   ├── kore_fileformat/
│   │   ├── extconf.rb                 [Build configuration]
│   │   └── wrapper.c                  [C wrapper]
├── spec/
│   ├── kore_fileformat_spec.rb
│   ├── compressor_spec.rb
│   └── decompressor_spec.rb
├── Gemfile
├── Rakefile
├── kore-fileformat.gemspec
└── README.md
```

### 2. kore-fileformat.gemspec

```ruby
# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name          = "kore-fileformat"
  spec.version       = "1.2.1"
  spec.authors       = ["Sai Arun Kumar Katherashala"]
  spec.email         = ["arunkatherashala@gmail.com"]

  spec.summary       = "KORE: High-performance compression library"
  spec.description   = <<~TEXT
    KORE is a high-performance, multi-platform file compression format.
    
    Key Features:
    • 19.1 GB/s throughput
    • 42.1% compression ratio
    • <1ms metadata extraction
    • Production-validated
  TEXT

  spec.homepage      = "https://github.com/arunkatherashala/Kore"
  spec.license       = "Apache-2.0"
  spec.required_ruby_version = ">= 2.7"

  spec.files         = Dir.glob(["lib/**/*.rb", "ext/**/*.{rb,c}", "README.md", "LICENSE"])
  spec.require_paths = ["lib"]
  spec.extensions    = ["ext/kore_fileformat/extconf.rb"]

  spec.add_development_dependency "bundler", "~> 2.0"
  spec.add_development_dependency "rake", "~> 13.0"
  spec.add_development_dependency "rspec", "~> 3.0"
  spec.add_development_dependency "rake-compiler", "~> 1.2"

  spec.add_dependency "ffi", "~> 1.15"
end
```

### 3. Main Ruby Module (lib/kore_fileformat.rb)

```ruby
# frozen_string_literal: true

require 'ffi'
require_relative 'kore_fileformat/version'
require_relative 'kore_fileformat/native'
require_relative 'kore_fileformat/compressor'
require_relative 'kore_fileformat/decompressor'

module KoreFileFormat
  class Error < StandardError; end
  class CompressionError < Error; end

  # Compress data with default settings (Balanced)
  def self.compress(data)
    compress_with_level(data, :balanced)
  end

  # Compress data with specific level
  def self.compress_with_level(data, level = :balanced)
    Compressor.new(level).compress(data)
  end

  # Decompress data
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
```

### 4. FFI Native Bindings (lib/kore_fileformat/native.rb)

```ruby
# frozen_string_literal: true

require 'ffi'

module KoreFileFormat
  module Native
    extend FFI::Library
    
    begin
      ffi_lib 'kore_fileformat'
    rescue LoadError
      raise "Native KORE library not found. Please ensure proper installation."
    end

    # C function declarations
    attach_function :compress_data, :compress_data,
                    [:pointer, :int, :pointer, :int, :pointer, :int],
                    :int
    
    attach_function :decompress_data, :decompress_data,
                    [:pointer, :int, :pointer, :int, :pointer],
                    :int
    
    attach_function :get_version, :get_version,
                    [:pointer, :pointer, :pointer],
                    :int
  end
end
```

### 5. Compressor Class (lib/kore_fileformat/compressor.rb)

```ruby
# frozen_string_literal: true

module KoreFileFormat
  class Compressor
    def initialize(level = :balanced)
      @level = parse_level(level)
    end

    def compress(data)
      raise ArgumentError, "data cannot be nil" if data.nil?

      input = FFI::MemoryPointer.new(:uchar, data.bytesize)
      input.put_bytes(0, data)

      output_size = (data.bytesize * 1.5).to_i
      output = FFI::MemoryPointer.new(:uchar, output_size)
      compressed_size = FFI::MemoryPointer.new(:int)

      result = Native.compress_data(
        input, data.bytesize,
        output, output_size,
        compressed_size,
        @level
      )

      raise CompressionError, "Compression failed: #{result}" if result != 0

      output.get_bytes(0, compressed_size.read_int)
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
```

### 6. Decompressor Class (lib/kore_fileformat/decompressor.rb)

```ruby
# frozen_string_literal: true

module KoreFileFormat
  class Decompressor
    def decompress(data)
      raise ArgumentError, "data cannot be nil" if data.nil?

      input = FFI::MemoryPointer.new(:uchar, data.bytesize)
      input.put_bytes(0, data)

      output_size = (data.bytesize * 4).to_i
      output = FFI::MemoryPointer.new(:uchar, output_size)
      decompressed_size = FFI::MemoryPointer.new(:int)

      result = Native.decompress_data(
        input, data.bytesize,
        output, output_size,
        decompressed_size
      )

      raise CompressionError, "Decompression failed: #{result}" if result != 0

      output.get_bytes(0, decompressed_size.read_int)
    end
  end
end
```

### 7. RSpec Tests (spec/kore_fileformat_spec.rb)

```ruby
require 'spec_helper'

describe KoreFileFormat do
  describe '.compress' do
    it 'compresses data successfully' do
      data = "Hello World" * 100
      compressed = KoreFileFormat.compress(data)
      
      expect(compressed).not_to be_nil
      expect(compressed.bytesize).to be < data.bytesize
    end

    it 'raises error on nil data' do
      expect { KoreFileFormat.compress(nil) }.to raise_error(ArgumentError)
    end
  end

  describe '.decompress' do
    it 'decompresses data successfully' do
      original = "Hello World" * 100
      compressed = KoreFileFormat.compress(original)
      decompressed = KoreFileFormat.decompress(compressed)
      
      expect(decompressed).to eq(original)
    end
  end

  describe 'round-trip' do
    it 'compresses and decompresses without data loss' do
      data = (0..255).to_a.pack('C*') * 10
      compressed = KoreFileFormat.compress(data)
      decompressed = KoreFileFormat.decompress(compressed)
      
      expect(decompressed).to eq(data)
    end
  end
end
```

### 8. GitHub Actions Workflow (.github/workflows/publish-ruby.yml)

```yaml
name: Publish to RubyGems

on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Setup Ruby
        uses: ruby/setup-ruby@v1
        with:
          ruby-version: '3.2'
          bundler-cache: true
      
      - name: Download pre-built binaries
        uses: actions/download-artifact@v3
        with:
          name: native-binaries
          path: lib/
      
      - name: Build gem
        run: bundle exec rake build
      
      - name: Publish to RubyGems
        run: |
          mkdir ~/.gem
          echo ":rubygems_api_key: ${{ secrets.RUBYGEMS_API_KEY }}" > ~/.gem/credentials
          chmod 600 ~/.gem/credentials
          gem push pkg/*.gem
```

---

## 📅 Development Timeline

### Week 1-2: Setup & Scaffolding
- [ ] Create NuGet project structure
- [ ] Create Ruby gem project structure
- [ ] Set up FFI bindings
- [ ] Configure build systems

### Week 3-4: Core Implementation
- [ ] Implement C# API wrappers
- [ ] Implement Ruby API wrappers
- [ ] Create comprehensive tests
- [ ] Build native libraries for all platforms

### Week 5-6: Testing & Release Prep
- [ ] Cross-platform testing (Windows, Linux, macOS)
- [ ] Performance testing
- [ ] Documentation
- [ ] Prepare for package publishing

### Week 7-8: Publishing
- [ ] Publish to NuGet
- [ ] Publish to RubyGems
- [ ] Announce releases
- [ ] Monitor for issues

---

## ✅ Success Criteria

- ✅ NuGet package published and installable
- ✅ Ruby gem published and installable
- ✅ Both support all compression levels
- ✅ 100% test coverage
- ✅ Zero critical bugs in first month
- ✅ Performance within 5% of native Rust

---

**Last Updated**: May 21, 2026  
**Owner**: Engineering Team  
**Status**: Ready for Week 1 start (June 1)
