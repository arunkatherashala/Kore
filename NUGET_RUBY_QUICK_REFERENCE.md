# KORE v1.2.1 NuGet & Ruby Development - Quick Reference Guide

**Date**: May 21, 2026  
**Phase**: Week 1 - Development Kickoff  
**Status**: ✅ Complete

---

## 🗂️ Project Directory Structure

```
Kore/
├── kore-fileformat-nuget/              # NuGet (.NET) Package
│   ├── KoreFileFormat/                 # Main Library
│   │   ├── KoreFileFormat.csproj       # Multi-target config (.NET 6,7,8)
│   │   ├── Kore.cs                     # Public API (Compress/Decompress)
│   │   ├── Native.cs                   # P/Invoke FFI bindings
│   │   ├── CompressionLevel.cs         # Compression level enum
│   │   └── CompressionException.cs     # Custom exception
│   ├── Tests/                          # Unit Tests
│   │   ├── Tests.csproj                # xUnit configuration
│   │   ├── CompressorTests.cs          # Compression tests (6 cases)
│   │   └── DecompressorTests.cs        # Decompression tests (5 cases)
│   ├── kore-fileformat.nuspec          # NuGet manifest
│   └── README.md                       # NuGet documentation
│
├── kore-fileformat-ruby/               # Ruby Gem Package
│   ├── lib/                            # Main Library
│   │   └── kore_fileformat/
│   │       ├── kore_fileformat.rb      # Public API
│   │       ├── version.rb              # Version constant (1.2.1)
│   │       ├── native.rb               # FFI bindings
│   │       ├── compressor.rb           # Compression wrapper
│   │       └── decompressor.rb         # Decompression wrapper
│   ├── spec/                           # RSpec Tests
│   │   ├── spec_helper.rb              # RSpec setup
│   │   └── kore_fileformat_spec.rb     # Tests (10+ cases)
│   ├── ext/                            # C Extension (prepared)
│   │   └── kore_fileformat/
│   ├── kore-fileformat.gemspec         # Gem configuration
│   ├── Rakefile                        # Build tasks
│   └── README.md                       # Ruby documentation
│
├── .github/workflows/
│   ├── publish-nuget.yml               # NuGet CI/CD (auto-publish)
│   └── publish-ruby.yml                # Ruby gem CI/CD (auto-publish)
│
├── NUGET_RUBY_DEVELOPMENT_STATUS.md    # Comprehensive status report
├── NUGET_RUBY_EXECUTION_SUMMARY.md     # This week's summary
└── NUGET_RUBY_QUICK_REFERENCE.md       # Quick start guide (this file)
```

---

## 🚀 Quick Start Guide

### Building NuGet Locally

```bash
cd kore-fileformat-nuget

# Install dependencies + Build
dotnet restore
dotnet build -c Release

# Run tests
dotnet test -c Release --verbosity normal

# Create NuGet package
dotnet pack KoreFileFormat -c Release -o nupkg
```

**Output**: `nupkg/kore-fileformat.1.2.1.nupkg`

### Building Ruby Gem Locally

```bash
cd kore-fileformat-ruby

# Install dependencies
bundle install

# Run tests
bundle exec rspec spec/ --verbose

# Build gem
gem build kore-fileformat.gemspec
```

**Output**: `kore-fileformat-1.2.1.gem`

---

## 📝 API Quick Reference

### NuGet (.NET) API

```csharp
using KoreFileFormat;

// Compress (default: Balanced level)
byte[] compressed = Kore.Compress(data);

// Compress with specific level
byte[] compressed = Kore.Compress(data, CompressionLevel.Fast);
byte[] compressed = Kore.Compress(data, CompressionLevel.Maximum);

// Decompress
byte[] original = Kore.Decompress(compressed);

// Get library version
Version v = Kore.GetLibraryVersion();  // 1.2.1

// Compression levels
// CompressionLevel.Fast = 0 (speed priority)
// CompressionLevel.Balanced = 1 (default)
// CompressionLevel.Maximum = 2 (compression ratio priority)
```

### Ruby Gem API

```ruby
require 'kore_fileformat'

# Compress (default: Balanced level)
compressed = KoreFileFormat.compress(data)

# Compress with specific level
compressed = KoreFileFormat.compress_with_level(data, :fast)
compressed = KoreFileFormat.compress_with_level(data, :balanced)
compressed = KoreFileFormat.compress_with_level(data, :maximum)

# Decompress
original = KoreFileFormat.decompress(compressed)

# Compression levels
# :fast          (CompressionLevel::FAST = 0)
# :balanced      (CompressionLevel::BALANCED = 1, default)
# :maximum       (CompressionLevel::MAXIMUM = 2)
```

---

## 🧪 Running Tests

### NuGet Tests

```bash
cd kore-fileformat-nuget

# Run all tests
dotnet test -c Release

# Run specific test class
dotnet test -c Release --filter "FullyQualifiedName~CompressorTests"

# Run with output
dotnet test -c Release --verbosity detailed

# Run and generate coverage
dotnet test -c Release /p:CollectCoverage=true
```

### Ruby Tests

```bash
cd kore-fileformat-ruby

# Run all specs
bundle exec rspec spec/

# Run specific file
bundle exec rspec spec/kore_fileformat_spec.rb

# Run with verbose output
bundle exec rspec spec/ --verbose

# Run with documentation format
bundle exec rspec spec/ --format documentation
```

---

## 📦 Publishing

### Publish NuGet to NuGet.org

```bash
cd kore-fileformat-nuget

# Build package
dotnet pack KoreFileFormat -c Release -o nupkg

# Publish (requires NUGET_API_KEY secret)
dotnet nuget push nupkg/kore-fileformat.1.2.1.nupkg \
  -s https://api.nuget.org/v3/index.json \
  -k YOUR_NUGET_API_KEY
```

Or trigger GitHub Actions:
```bash
git tag v1.2.1
git push origin v1.2.1
# Workflow .github/workflows/publish-nuget.yml triggers automatically
```

### Publish Ruby Gem to RubyGems.org

```bash
cd kore-fileformat-ruby

# Build gem
gem build kore-fileformat.gemspec

# Setup credentials
echo ":rubygems_api_key: YOUR_RUBYGEMS_API_KEY" > ~/.gem/credentials
chmod 600 ~/.gem/credentials

# Publish
gem push kore-fileformat-1.2.1.gem
```

Or trigger GitHub Actions:
```bash
git tag v1.2.1
git push origin v1.2.1
# Workflow .github/workflows/publish-ruby.yml triggers automatically
```

---

## 📊 Project Status

| Component | Status | Progress | Owner |
|-----------|--------|----------|-------|
| NuGet Structure | ✅ Done | 100% | Dev 1 |
| NuGet Core API | ✅ Done | 100% | Dev 1 |
| NuGet Tests | ✅ Done | 100% | Dev 1 |
| Ruby Structure | ✅ Done | 100% | Dev 2 |
| Ruby Core API | ✅ Done | 100% | Dev 2 |
| Ruby Tests | ✅ Done | 100% | Dev 2 |
| CI/CD Workflows | ✅ Ready | 100% | DevOps |
| Local Build Test | 🔄 Pending | 0% | Dev 1,2 |
| Integration Test | ⏳ Pending | 0% | QA |
| Publishing | ⏳ Pending | 0% | DevOps |

---

## 📈 Performance Targets

All targets aligned with KORE v1.2.0 baseline:

| Metric | Target | Baseline |
|--------|--------|----------|
| Throughput | ≥19.1 GB/s | 19.1 GB/s |
| Compression Ratio | ≥42% | 42.1% |
| Latency | <1ms | 0.05-0.12ms |
| Data Integrity | 100% | 100% ✓ |

---

## 🔧 Common Tasks

### Update Package Version

All three files must be synchronized:

**NuGet**:
- `KoreFileFormat/KoreFileFormat.csproj` → `<Version>1.2.1</Version>`
- `kore-fileformat.nuspec` → `<version>1.2.1</version>`

**Ruby**:
- `kore-fileformat.gemspec` → `spec.version = "1.2.1"`
- `lib/kore_fileformat/version.rb` → `VERSION = "1.2.1"`

### Enable Pre-release Publishing

**NuGet**:
```xml
<!-- In .csproj -->
<PropertyGroup>
  <Version>1.2.1-beta1</Version>
</PropertyGroup>
```

**Ruby**:
```ruby
# In .gemspec
spec.version = "1.2.1.beta1"
```

### Support New Platform

**NuGet**:
- Add to `<RuntimeIdentifiers>` in `.csproj`
- Ensure native binary available in `runtimes/{platform}/native/`

**Ruby**:
- Native library support determined at runtime via FFI
- Test with target Ruby version

---

## 🐛 Troubleshooting

### "Native library not found" Error

**Cause**: Native KORE library not installed or not in system PATH

**Fix (Windows)**:
```bash
# Copy DLL to Windows System32
copy libkore_fileformat.dll C:\Windows\System32\
# Or add to PATH
set PATH=%PATH%;C:\path\to\native\libs
```

**Fix (Linux)**:
```bash
# Copy .so file to library path
sudo cp libkore_fileformat.so /usr/local/lib/
sudo ldconfig
```

**Fix (macOS)**:
```bash
# Copy .dylib file to library path
cp libkore_fileformat.dylib /usr/local/lib/
```

### Test Failures

**NuGet**:
```bash
# Run with verbose output to see errors
dotnet test -c Release --verbosity detailed

# Check if native library is accessible
dotnet repl  # Test P/Invoke manually
```

**Ruby**:
```bash
# Run with backtrace
bundle exec rspec spec/ --backtrace

# Check FFI library loading
ruby -e "require 'ffi'; puts FFI.library_paths"
```

### Build Failures

**NuGet**:
```bash
# Clean build
dotnet clean
dotnet restore
dotnet build -c Release

# Check for missing nuget packages
dotnet restore --force
```

**Ruby**:
```bash
# Clean and reinstall gems
rm Gemfile.lock
bundle install --redownload

# Check Ruby version compatibility
ruby --version  # Should be 2.7+
```

---

## 📚 Documentation Files

| File | Purpose | Status |
|------|---------|--------|
| `3_NUGET_RUBY_DEVELOPMENT.md` | Implementation guide with all starter code | ✅ Complete |
| `NUGET_RUBY_DEVELOPMENT_STATUS.md` | Detailed progress report | ✅ Complete |
| `NUGET_RUBY_EXECUTION_SUMMARY.md` | Week 1 execution summary | ✅ Complete |
| `NUGET_RUBY_QUICK_REFERENCE.md` | This file - Quick start | ✅ Complete |
| `kore-fileformat-nuget/README.md` | NuGet specific docs | ✅ Complete |
| `kore-fileformat-ruby/README.md` | Ruby specific docs | ✅ Complete |

---

## 🎓 Learning Resources

### NuGet/C# Development
- Microsoft Docs: https://learn.microsoft.com/en-us/nuget/
- xUnit Testing: https://xunit.net/docs/getting-started/netfx
- P/Invoke: https://learn.microsoft.com/en-us/dotnet/fundamentals/runtime-libraries/system-runtime-interopservices-dllimportattribute

### Ruby Development
- FFI Documentation: https://github.com/ffi/ffi/wiki
- RSpec Testing: https://rspec.info/
- RubyGems Publishing: https://guides.rubygems.org/publishing/

### KORE Documentation
- Repository: https://github.com/arunkatherashala/Kore
- Technical Paper: KORE_Technical_Paper_Fixed.pdf (in repository)
- Benchmarks: PRACTICAL_VALIDATION_RESULTS.md (in repository)

---

## 📋 Checklist for Week 1 Completion

- [ ] Obtain native binaries (DLL, SO, DYLIB)
- [ ] Run NuGet build locally
  - [ ] `dotnet restore` succeeds
  - [ ] `dotnet build -c Release` succeeds
  - [ ] `dotnet test -c Release` passes (11 tests)
- [ ] Run Ruby build locally
  - [ ] `bundle install` succeeds
  - [ ] `bundle exec rspec spec/` passes (10+ tests)
- [ ] Verify compression/decompression works
- [ ] Document build success
- [ ] Report metrics vs baseline

---

## 🔗 Related Projects (v1.2.1)

**Parallel Work Streams**:
- **Case Study Prospects** (Week 1-2): `2_CASE_STUDY_PROSPECTS.md`
- **Performance Profiling** (Week 1): `4_PERFORMANCE_PROFILING_SETUP.md`
- **Master Execution Plan** (Weeks 1-13): `5_MASTER_EXECUTION_PLAN.md`

**Coordinated Timeline**:
```
Week 1: NuGet/Ruby initialization + Case study outreach + Performance baseline
Week 2-3: Development + Case study pilots + Optimization starts
Week 4-5: Feature completion + Case study data collection
Week 6-7: Testing + Case study finalization + Performance optimization
Week 8-9: NuGet/Ruby publishing + Case studies published
Week 10-11: Issue resolution + community engagement
Week 12-13: v1.2.1 final release
```

---

## 📞 Getting Help

**Questions on NuGet Development**:
- Contact: Dev 1
- Docs: `kore-fileformat-nuget/README.md`
- Repository: `.github/workflows/publish-nuget.yml`

**Questions on Ruby Development**:
- Contact: Dev 2
- Docs: `kore-fileformat-ruby/README.md`
- Repository: `.github/workflows/publish-ruby.yml`

**Questions on CI/CD**:
- Contact: DevOps
- Workflows: `.github/workflows/publish-*.yml`

**Questions on Project Status**:
- Contact: Project Manager
- Status: `NUGET_RUBY_DEVELOPMENT_STATUS.md`
- Plan: `5_MASTER_EXECUTION_PLAN.md`

---

## ✅ Success Indicators

✅ **Week 1 Complete When**:
- [ ] All projects build successfully
- [ ] All tests pass
- [ ] No runtime errors
- [ ] Documentation verified
- [ ] Ready for Week 2 work

🎯 **v1.2.1 Complete When**:
- [ ] NuGet published to NuGet.org
- [ ] Ruby gem published to RubyGems.org
- [ ] 3-5 case studies published
- [ ] 80%+ GitHub issues resolved
- [ ] 20+ GB/s performance achieved
- [ ] Community satisfied

---

**Last Updated**: May 21, 2026  
**Next Update**: May 28, 2026  
**Status**: ✅ Ready for Development  
**Owner**: Dev Team (1, 2) + DevOps
