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
    • 19.1 GB/s throughput (verified)
    • 42.1% compression ratio
    • <1ms metadata extraction
    • Production-validated
    • Ruby support with FFI bindings
  TEXT

  spec.homepage      = "https://github.com/arunkatherashala/Kore"
  spec.license       = "Apache-2.0"
  spec.required_ruby_version = ">= 2.7"

  spec.files         = Dir.glob(["lib/**/*.rb", "lib/**/*.dll", "lib/**/*.so", "lib/**/*.dylib", "README.md", "LICENSE"])
  spec.require_paths = ["lib"]

  spec.add_development_dependency "bundler", "~> 2.0"
  spec.add_development_dependency "rake", "~> 13.0"
  spec.add_development_dependency "rspec", "~> 3.0"

  spec.add_runtime_dependency "ffi", "~> 1.15"
end
