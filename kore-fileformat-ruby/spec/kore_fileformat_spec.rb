require "spec_helper"

describe KoreFileFormat do
  describe ".compress" do
    it "compresses data successfully" do
      data = "Hello World" * 100
      compressed = KoreFileFormat.compress(data)
      
      expect(compressed).not_to be_nil
      expect(compressed.bytesize).to be < data.bytesize
    end

    it "raises error on nil data" do
      expect { KoreFileFormat.compress(nil) }.to raise_error(ArgumentError)
    end

    it "compresses with different levels" do
      data = "Test data to compress" * 50
      
      fast = KoreFileFormat.compress_with_level(data, :fast)
      balanced = KoreFileFormat.compress_with_level(data, :balanced)
      maximum = KoreFileFormat.compress_with_level(data, :maximum)
      
      expect(fast).not_to be_nil
      expect(balanced).not_to be_nil
      expect(maximum).not_to be_nil
    end
  end

  describe ".decompress" do
    it "decompresses data successfully" do
      original = "Hello World" * 100
      compressed = KoreFileFormat.compress(original)
      decompressed = KoreFileFormat.decompress(compressed)
      
      expect(decompressed).to eq(original)
    end

    it "raises error on nil data" do
      expect { KoreFileFormat.decompress(nil) }.to raise_error(ArgumentError)
    end
  end

  describe "round-trip compression" do
    it "maintains data integrity for text" do
      original = "The quick brown fox jumps over the lazy dog" * 25
      compressed = KoreFileFormat.compress(original)
      decompressed = KoreFileFormat.decompress(compressed)
      
      expect(decompressed).to eq(original)
    end

    it "maintains data integrity for binary" do
      original = (0..255).to_a.pack("C*") * 10
      compressed = KoreFileFormat.compress(original)
      decompressed = KoreFileFormat.decompress(compressed)
      
      expect(decompressed).to eq(original)
    end

    it "maintains data integrity for random data" do
      original = (0..10000).map { rand(256) }.pack("C*")
      compressed = KoreFileFormat.compress(original)
      decompressed = KoreFileFormat.decompress(compressed)
      
      expect(decompressed.length).to eq(original.length)
      expect(decompressed).to eq(original)
    end
  end

  describe "compression efficiency" do
    it "compresses repetitive data effectively" do
      repetitive = "ABCDE" * 1000
      compressed = KoreFileFormat.compress(repetitive)
      
      compression_ratio = compressed.bytesize.to_f / repetitive.bytesize
      expect(compression_ratio).to be < 0.6  # Expect at least 40% compression
    end
  end
end
