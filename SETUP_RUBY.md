# Ruby Setup & Integration Guide for KORE v1.3.3

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Verification](#verification)
4. [KORE Integration](#kore-integration)
5. [Common Tasks](#common-tasks)
6. [Troubleshooting](#troubleshooting)

---

## Prerequisites

| Requirement | Minimum | Recommended | Notes |
|-------------|---------|-------------|-------|
| Ruby Version | 2.7+ | 3.2+ | Latest stable |
| RubyGems | 3.0+ | 3.4+ | Package manager |
| Bundler | 2.0+ | 2.4+ | Dependency management |
| Rails | 6.0+ | 7.0+ | Optional framework |
| OS Support | Windows 10+ | Ubuntu 20.04+ | WSL2 for Windows |
| RAM | 1 GB | 2 GB | For development |
| Disk Space | 500 MB | 1 GB | Ruby + gems |

---

## Installation

### Step 1: Install Ruby

**Windows (RubyInstaller):**
```powershell
# Using Chocolatey
choco install ruby

# Or download from https://rubyinstaller.org/

# Verify installation
ruby --version
gem --version
```

**Linux (Ubuntu/Debian):**
```bash
# Update package list
sudo apt-get update

# Install Ruby
sudo apt-get install -y ruby-full build-essential

# Verify
ruby --version
gem --version
```

**macOS:**
```bash
# Using Homebrew
brew install ruby

# Add to PATH (if needed)
echo 'export PATH="/usr/local/opt/ruby/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Verify
ruby --version
```

### Step 2: Install Bundler

```powershell
# Install bundler (global)
gem install bundler

# Verify
bundle --version
```

### Step 3: Setup KORE Ruby Project

```powershell
# Create directory
mkdir kore-ruby-tools
cd kore-ruby-tools

# Initialize git repository
git init

# Create Gemfile
@"
source 'https://rubygems.org'

gem 'bundler', '~> 2.4'
gem 'json', '~> 2.6'
gem 'rake', '~> 13.0'

# For web APIs
gem 'sinatra', '~> 3.0'
gem 'puma', '~> 6.0'

# For data processing
gem 'csv', '~> 3.2'

# Testing
gem 'rspec', '~> 3.12', :group => :test
gem 'rack-test', '~> 2.1', :group => :test

# Development tools
gem 'pry', '~> 0.14', :group => :development
"@ | Out-File -Encoding UTF8 Gemfile

# Install dependencies
bundle install
```

---

## Verification

### Quick Check

```powershell
# Check Ruby version
ruby --version

# Check gems
gem list

# Create test file
@"
puts "Hello from KORE Ruby!"
puts "Ruby version: #{RUBY_VERSION}"
"@ | Out-File -Encoding UTF8 test.rb

# Run
ruby test.rb

# Clean up
Remove-Item test.rb
```

### Complete Environment

```powershell
# Show Ruby info
ruby --version
bundler --version

# List installed gems
bundle list

# Check gem locations
gem list --local
```

---

## KORE Integration

### Ruby with KORE

Ruby is excellent for:
- Quick prototyping and scripting
- Web APIs and microservices
- Data processing and ETL
- DevOps and automation
- Testing and CI/CD integration

### Create KORE Ruby Library

**Gemfile:**
```ruby
source 'https://rubygems.org'

gem 'json'
gem 'fileutils'
gem 'bundler'
```

**lib/kore_processor.rb:**
```ruby
require 'json'
require 'fileutils'

class KoreProcessor
  VERSION = '1.3.3'
  
  attr_reader :file_path
  
  def initialize(file_path)
    @file_path = file_path
  end
  
  def process
    {
      filename: File.basename(file_path),
      version: VERSION,
      timestamp: Time.now.to_i,
      status: 'processed'
    }
  end
  
  def get_metadata_json
    JSON.pretty_generate(process)
  end
  
  def self.version
    VERSION
  end
end
```

**bin/kore.rb:**
```ruby
#!/usr/bin/env ruby

require_relative '../lib/kore_processor'

def main
  puts "KORE Ruby Integration v#{KoreProcessor.version}"
  
  processor = KoreProcessor.new('data.kore')
  puts processor.get_metadata_json
end

main if __FILE__ == $0
```

---

## Common Tasks

### Building Ruby Projects

```powershell
# Install dependencies
bundle install

# Update gems
bundle update

# Create gemspec
gem build

# Package as gem
gem build kore.gemspec
```

### Running Ruby Programs

```powershell
# Direct execution
ruby script.rb

# With bundler
bundle exec ruby script.rb

# Run test suite
bundle exec rspec

# Run Sinatra app
bundle exec ruby app.rb
```

### Ruby KORE Integration Examples

**Process KORE Files:**
```ruby
class KoreReader
  def initialize(file_path)
    @file_path = file_path
  end
  
  def read_metadata
    File.read(@file_path) # Simplified
  end
  
  def validate
    File.exist?(@file_path) && File.readable?(@file_path)
  end
end

# Usage
reader = KoreReader.new('data.kore')
puts "Valid: #{reader.validate}"
```

**Web API with Sinatra:**
```ruby
require 'sinatra'
require_relative 'lib/kore_processor'

get '/api/kore/metadata' do
  processor = KoreProcessor.new('data.kore')
  content_type :json
  processor.get_metadata_json
end

get '/api/kore/version' do
  content_type :json
  { version: KoreProcessor.version }.to_json
end

post '/api/kore/process' do
  data = JSON.parse(request.body.read)
  processor = KoreProcessor.new(data['filename'])
  content_type :json
  processor.get_metadata_json
end
```

---

## Troubleshooting

### Issue 1: "ruby is not recognized"

**Solution:**
```powershell
# Check PATH
$env:Path -split ';' | findstr ruby

# Add Ruby to PATH
$rubyPath = "C:\Ruby32\bin"
[Environment]::SetEnvironmentVariable("PATH", "$env:Path;$rubyPath", "User")

# Restart PowerShell
```

### Issue 2: "Bundler command not found"

**Solution:**
```powershell
# Install bundler globally
gem install bundler

# Or use gem directly
ruby -S bundler install
```

### Issue 3: "Gem installation fails"

**Solution:**
```powershell
# Clear bundler cache
bundle clean

# Reinstall dependencies
bundle install --full-index

# Or use specific gem source
bundle config set github.https true
```

---

## Best Practices

✅ **DO:**
- Use Bundler for dependency management
- Write tests with RSpec
- Use Ruby idioms and conventions
- Follow Ruby style guide (snake_case for methods)
- Use symbols for string keys
- Chain methods efficiently
- Write clear, readable code
- Use Ruby's built-in methods

❌ **DON'T:**
- Mix versions of gems
- Commit Gemfile.lock in gemspecs
- Use global variables
- Ignore warnings
- Use mutable default arguments
- Create unnecessary dependencies
- Hardcode configuration
- Ignore error handling

---

## Project Structure

```
kore-ruby-tools/
├── Gemfile
├── Gemfile.lock
├── README.md
├── lib/
│   └── kore_processor.rb
├── bin/
│   └── kore.rb
├── spec/
│   └── kore_processor_spec.rb
├── app.rb              (if using Sinatra)
└── Rakefile
```

---

## Quick Reference

```powershell
# Ruby commands
ruby script.rb                 # Run script
ruby -v                       # Check version
ruby -r json -e 'puts JSON'   # Require and eval

# Gem commands
gem install name              # Install gem
gem list                      # List installed gems
gem uninstall name            # Remove gem
gem search keyword            # Search gems

# Bundle commands
bundle install               # Install dependencies
bundle update               # Update gems
bundle exec command         # Run command with bundle
bundle list                 # List installed gems
bundle show gem_name        # Show gem location

# Testing
bundle exec rspec           # Run all tests
bundle exec rspec spec/     # Run tests in directory
bundle exec rspec -f d      # Documentation format
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial Ruby setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** PHP Setup & Integration Guide
