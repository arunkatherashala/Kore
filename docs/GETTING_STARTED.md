# 🎯 Getting Started - KORE v1.2.0

Get started with KORE in just 5 minutes! Choose your language and follow along.

---

## 📋 Quick Navigation

- **[Python](#python-users)** - Most popular, easiest to learn
- **[JavaScript/Node.js](#javascriptnodejs-users)** - For web and Node.js
- **[Java](#java-users)** - For enterprise applications
- **[Go](#go-users)** - For high-performance systems
- **[C#/.NET](#cnet-users)** - For Windows and .NET applications
- **[Ruby](#ruby-users)** - For Ruby on Rails projects

---

## 🐍 Python Users

### Installation (1 minute)

```bash
# Using pip
pip install kore-fileformat

# Or with conda
conda install -c conda-forge kore-fileformat
```

**Verify Installation:**
```python
import kore_fileformat
print(kore_fileformat.get_kore_info.__doc__)
```

### Your First Program (2 minutes)

Create a file named `hello_kore.py`:

```python
import kore_fileformat

# Get information about a KORE file
size, version, flags = kore_fileformat.get_kore_info("data.kore")
print(f"✓ File size: {size} bytes")
print(f"✓ Version: {version}")
```

**Run it:**
```bash
python hello_kore.py
```

### Next Steps (2 minutes)

**Option 1: Analyze Compression**
```python
import kore_fileformat

reader = kore_fileformat.KoreReader("data.kore")
ratio, format_info = reader.get_compression_stats()
print(f"Compression: {ratio}%")
```

**Option 2: Compress a CSV**
```python
import kore_fileformat

kore_fileformat.compress_csv("data.csv", "data.kore")
print("✓ Compressed successfully")
```

**Option 3: Process Multiple Files**
```python
import kore_fileformat
from pathlib import Path

for file in Path("./data").glob("*.kore"):
    size, version, _ = kore_fileformat.get_kore_info(str(file))
    print(f"{file.name}: {size} bytes")
```

---

## 🟨 JavaScript/Node.js Users

### Installation (1 minute)

```bash
# Using npm
npm install kore-fileformat

# Or with yarn
yarn add kore-fileformat

# Or with pnpm
pnpm add kore-fileformat
```

**Verify Installation:**
```javascript
const kore = require('kore-fileformat');
console.log(typeof kore.getKoreInfo);
```

### Your First Program (2 minutes)

Create a file named `hello_kore.js`:

```javascript
const kore = require('kore-fileformat');

// Get information about a KORE file
const { size, version, flags } = kore.getKoreInfo('data.kore');
console.log(`✓ File size: ${size} bytes`);
console.log(`✓ Version: ${version}`);
```

**Run it:**
```bash
node hello_kore.js
```

### Next Steps (2 minutes)

**Option 1: Analyze Compression**
```javascript
const kore = require('kore-fileformat');

const reader = new kore.KoreReader('data.kore');
const { ratio, format } = reader.getCompressionStats();
console.log(`Compression: ${ratio}%`);
```

**Option 2: Batch Processing**
```javascript
const kore = require('kore-fileformat');
const fs = require('fs');
const path = require('path');

const dir = './data';
fs.readdirSync(dir)
  .filter(f => f.endsWith('.kore'))
  .forEach(file => {
    const info = kore.getKoreInfo(path.join(dir, file));
    console.log(`${file}: ${info.size} bytes`);
  });
```

---

## ☕ Java Users

### Installation (1 minute)

Add to your `pom.xml`:

```xml
<dependency>
    <groupId>com.arunkatherashala</groupId>
    <artifactId>kore-fileformat</artifactId>
    <version>1.2.0</version>
</dependency>
```

Or for Gradle:

```gradle
implementation 'com.arunkatherashala:kore-fileformat:1.2.0'
```

### Your First Program (2 minutes)

Create `HelloKore.java`:

```java
import com.kore.KoreFileFormat;

public class HelloKore {
    public static void main(String[] args) throws Exception {
        // Get information about a KORE file
        KoreFileFormat.FileInfo info = 
            KoreFileFormat.getKoreInfo("data.kore");
        
        System.out.println("✓ File size: " + info.getSize() + " bytes");
        System.out.println("✓ Version: " + info.getVersion());
    }
}
```

**Compile and run:**
```bash
javac HelloKore.java
java HelloKore
```

### Next Steps (2 minutes)

**Option 1: Analyze Compression**
```java
KoreFileFormat.KoreReader reader = 
    new KoreFileFormat.KoreReader("data.kore");
double ratio = reader.getCompressionStats();
System.out.println("Compression: " + ratio + "%");
```

**Option 2: Batch Processing**
```java
import java.nio.file.*;

Files.list(Paths.get("./data"))
    .filter(p -> p.toString().endsWith(".kore"))
    .forEach(p -> {
        try {
            var info = KoreFileFormat.getKoreInfo(p.toString());
            System.out.println(p.getFileName() + ": " + info.getSize());
        } catch (Exception e) {
            e.printStackTrace();
        }
    });
```

---

## 🐹 Go Users

### Installation (1 minute)

```bash
go get github.com/arunkatherashala/kore-go
```

### Your First Program (2 minutes)

Create `hello_kore.go`:

```go
package main

import (
    "fmt"
    "log"
    kore "github.com/arunkatherashala/kore-go"
)

func main() {
    // Get information about a KORE file
    info, err := kore.GetKoreInfo("data.kore")
    if err != nil {
        log.Fatal(err)
    }
    
    fmt.Printf("✓ File size: %d bytes\n", info.Size)
    fmt.Printf("✓ Version: %s\n", info.Version)
}
```

**Run it:**
```bash
go run hello_kore.go
```

### Next Steps (2 minutes)

**Option 1: Analyze Compression**
```go
reader := kore.NewKoreReader("data.kore")
ratio, _ := reader.GetCompressionStats()
fmt.Printf("Compression: %.1f%%\n", ratio)
```

**Option 2: Batch Processing**
```go
import "os"

files, _ := os.ReadDir("./data")
for _, f := range files {
    if strings.HasSuffix(f.Name(), ".kore") {
        info, _ := kore.GetKoreInfo("./data/" + f.Name())
        fmt.Printf("%s: %d bytes\n", f.Name(), info.Size)
    }
}
```

---

## 🎨 C#/.NET Users

### Installation (1 minute)

```bash
# Using NuGet
dotnet add package KoreFileFormat

# Or in Package Manager Console
Install-Package KoreFileFormat
```

### Your First Program (2 minutes)

Create `HelloKore.cs`:

```csharp
using KoreFileFormat;

class Program {
    static void Main() {
        // Get information about a KORE file
        var info = KoreFile.GetKoreInfo("data.kore");
        
        Console.WriteLine($"✓ File size: {info.Size} bytes");
        Console.WriteLine($"✓ Version: {info.Version}");
    }
}
```

**Run it:**
```bash
dotnet run
```

### Next Steps (2 minutes)

**Option 1: Analyze Compression**
```csharp
var reader = new KoreReader("data.kore");
double ratio = reader.GetCompressionStats();
Console.WriteLine($"Compression: {ratio:F1}%");
```

**Option 2: Batch Processing**
```csharp
using System.IO;

var files = Directory.GetFiles("./data", "*.kore");
foreach (var file in files) {
    var info = KoreFile.GetKoreInfo(file);
    Console.WriteLine($"{Path.GetFileName(file)}: {info.Size} bytes");
}
```

---

## 💎 Ruby Users

### Installation (1 minute)

Add to your `Gemfile`:

```ruby
gem 'kore_fileformat'
```

Then run:

```bash
bundle install
```

### Your First Program (2 minutes)

Create `hello_kore.rb`:

```ruby
require 'kore_fileformat'

# Get information about a KORE file
info = KoreFileFormat.get_kore_info('data.kore')

puts "✓ File size: #{info[:size]} bytes"
puts "✓ Version: #{info[:version]}"
```

**Run it:**
```bash
ruby hello_kore.rb
```

### Next Steps (2 minutes)

**Option 1: Analyze Compression**
```ruby
reader = KoreFileFormat::KoreReader.new('data.kore')
ratio = reader.compression_stats
puts "Compression: #{ratio}%"
```

**Option 2: Batch Processing**
```ruby
Dir.glob('data/*.kore').each do |file|
  info = KoreFileFormat.get_kore_info(file)
  puts "#{File.basename(file)}: #{info[:size]} bytes"
end
```

---

## 🎯 Common Tasks

### Task 1: Get File Metadata (All Languages)

| Language | Code |
|----------|------|
| Python | `kore_fileformat.get_kore_info("file.kore")` |
| JS/Node | `kore.getKoreInfo('file.kore')` |
| Java | `KoreFileFormat.getKoreInfo("file.kore")` |
| Go | `kore.GetKoreInfo("file.kore")` |
| C# | `KoreFile.GetKoreInfo("file.kore")` |
| Ruby | `KoreFileFormat.get_kore_info('file.kore')` |

### Task 2: Check Compression Ratio (All Languages)

| Language | Code |
|----------|------|
| Python | `reader = KoreReader("file.kore"); reader.get_compression_stats()` |
| JS/Node | `reader = new kore.KoreReader('file.kore'); reader.getCompressionStats()` |
| Java | `new KoreReader("file.kore").getCompressionStats()` |
| Go | `reader := kore.NewKoreReader("file.kore"); reader.GetCompressionStats()` |
| C# | `new KoreReader("file.kore").GetCompressionStats()` |
| Ruby | `KoreFileFormat::KoreReader.new('file.kore').compression_stats` |

### Task 3: Process Multiple Files (All Languages)

**Python:**
```python
from pathlib import Path
for f in Path("./data").glob("*.kore"):
    info = kore_fileformat.get_kore_info(str(f))
```

**JavaScript:**
```javascript
const files = fs.readdirSync('./data').filter(f => f.endsWith('.kore'));
files.forEach(f => kore.getKoreInfo(`./data/${f}`));
```

**Java:**
```java
Files.list(Paths.get("./data"))
    .filter(p -> p.toString().endsWith(".kore"))
    .forEach(p -> KoreFileFormat.getKoreInfo(p.toString()));
```

**Go:**
```go
files, _ := os.ReadDir("./data")
for _, f := range files {
    if strings.HasSuffix(f.Name(), ".kore") {
        kore.GetKoreInfo("./data/" + f.Name())
    }
}
```

**C#:**
```csharp
var files = Directory.GetFiles("./data", "*.kore");
foreach (var file in files) {
    KoreFile.GetKoreInfo(file);
}
```

**Ruby:**
```ruby
Dir.glob('data/*.kore').each { |f| KoreFileFormat.get_kore_info(f) }
```

---

## 🐛 Troubleshooting

### "Module not found" or "Package not found"

**Python:**
```bash
pip install --upgrade kore-fileformat
```

**JavaScript:**
```bash
npm install kore-fileformat
```

**Java:**
Make sure your `pom.xml` has the correct version.

**Go:**
```bash
go get -u github.com/arunkatherashala/kore-go
```

**C#:**
```bash
dotnet add package KoreFileFormat
```

**Ruby:**
```bash
bundle update kore_fileformat
```

---

### "File not found" Error

Make sure the file path is correct. Use absolute paths if relative paths don't work:

**Python:**
```python
from pathlib import Path
path = Path("data/file.kore").absolute()
info = kore_fileformat.get_kore_info(str(path))
```

**JavaScript:**
```javascript
const path = require('path').resolve('./data/file.kore');
const info = kore.getKoreInfo(path);
```

---

### Performance Issues

If operations are slow, check:

1. **File size** - Larger files take longer
2. **Disk speed** - SSD is faster than HDD
3. **System load** - Close other applications
4. **Use `get_kore_info()`** - It's faster than reading full file

---

## 📚 Next Steps

After mastering the basics:

1. **[Practical Tutorials](PRACTICAL_TUTORIALS.md)** - Real-world examples and patterns
2. **[API Quick Reference](API_QUICK_REFERENCE.md)** - Complete API documentation
3. **[User Guide](USER_GUIDE.md)** - Comprehensive guide with advanced features
4. **[Examples](EXAMPLES.md)** - Code examples for all features

---

## 💬 Need Help?

- **[Troubleshooting Guide](TROUBLESHOOTING.md)** - Solutions for common problems
- **GitHub Issues** - Report bugs or request features
- **Documentation** - Full API reference and examples

---

## 🎓 Learning Resources

| Resource | Time | Difficulty |
|----------|------|-----------|
| Getting Started | 5 min | Beginner |
| Practical Tutorials | 30 min | Beginner |
| API Reference | 20 min | Intermediate |
| User Guide | 1 hour | Intermediate |
| Advanced Patterns | 1-2 hours | Advanced |

---

## ✅ Success Checklist

- [ ] Installed KORE for your language
- [ ] Created first program
- [ ] Got file metadata successfully
- [ ] Checked compression ratio
- [ ] Processed multiple files
- [ ] Read error handling guide

**Congratulations!** 🎉 You're ready to use KORE!

---

**Last Updated:** May 20, 2026  
**KORE Version:** 1.2.0  
**Status:** ✅ Production Ready
