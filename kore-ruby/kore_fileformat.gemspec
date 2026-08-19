Gem::Specification.new do |spec|
  spec.name          = "kore-fileformat"
  spec.version       = "1.7.30"
  spec.authors       = ["Sai Arun Kumar Katherashala"]
  spec.email         = ["arunkatherashala@gmail.com"]

  spec.summary       = "KORE Binary Format — columnar file format with 11 ACID features"
  spec.description   = "KORE: Single-file columnar format with ACID transactions, CRC32, " \
                       "ZSTD compression, Bloom filters, AES-256-GCM encryption, schema evolution, " \
                       "MVCC time travel, partition evolution, row-level deletes. 8-language FFI."
  spec.homepage      = "https://github.com/arunkatherashala/Kore"
  spec.license       = "MIT"

  spec.required_ruby_version = ">= 2.7.0"

  spec.metadata["homepage_uri"]    = spec.homepage
  spec.metadata["source_code_uri"] = "https://github.com/arunkatherashala/Kore"
  spec.metadata["changelog_uri"]   = "https://github.com/arunkatherashala/Kore/blob/release/v0.1.0/CHANGELOG.md"

  spec.files = Dir["lib/**/*", "README.md", "LICENSE"]
  spec.require_paths = ["lib"]

  # koffi-equivalent for Ruby is built-in Fiddle — no extra gem needed
end
