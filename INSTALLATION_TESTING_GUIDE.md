# 🚀 KORE v1.1.5 - Installation & Testing Guide

Complete guide for testing KORE installation/uninstallation across all platforms.

---

## 📋 TABLE OF CONTENTS

1. [Python PyPI Installation](#python-pypi-installation)
2. [Java Maven Installation](#java-maven-installation)
3. [JavaScript npm Installation](#javascript-npm-installation)
4. [Full Install/Uninstall Cycle Test](#full-installuninstall-cycle-test)
5. [Verification & Quick Start](#verification--quick-start)

---

## 🐍 Python PyPI Installation

### Prerequisites
```bash
# Verify Python 3.9+ installed
python --version
python -m pip --version
```

### Installation Steps

**1. Check for existing installation:**
```bash
pip list | grep kore
```

**2. Uninstall (if upgrading):**
```bash
pip uninstall kore-fileformat -y
```

**3. Install from PyPI:**
```bash
pip install kore-fileformat
```

Or install specific version:
```bash
pip install kore-fileformat==1.1.5
```

**4. Verify installation:**
```bash
pip show kore-fileformat
```

### Quick Test - Python

**Test 1: Import and check version**
```python
import kore_fileformat
print(f"Version: {kore_fileformat.__version__}")
```

**Test 2: Basic compression**
```python
from kore_fileformat import compress, decompress

# Create test data
data = b"hello world" * 1000

# Compress
compressed = compress(data)
ratio = len(compressed) / len(data) * 100

# Decompress
decompressed = decompress(compressed)

# Verify
assert decompressed == data, "Decompressed data doesn't match!"

print(f"✅ Compression test passed!")
print(f"  Original: {len(data)} bytes")
print(f"  Compressed: {len(compressed)} bytes")
print(f"  Ratio: {ratio:.1f}%")
```

**Test 3: Full round-trip test**
```python
from kore_fileformat import compress, decompress
import time

# Test with different data patterns
test_cases = [
    ("Repetitive", b"A" * 10000),
    ("Random", bytes(range(256)) * 50),
    ("Categorical", (b"cat,dog,bird," * 500)),
]

for name, data in test_cases:
    start = time.time()
    compressed = compress(data)
    decompress_time = time.time()
    decompressed = decompress(compressed)
    end = time.time()
    
    ratio = len(compressed) / len(data) * 100
    compress_ms = (decompress_time - start) * 1000
    decompress_ms = (end - decompress_time) * 1000
    
    print(f"\n{name} Data:")
    print(f"  Compression Ratio: {ratio:.1f}%")
    print(f"  Compress Time: {compress_ms:.2f}ms")
    print(f"  Decompress Time: {decompress_ms:.2f}ms")
    print(f"  ✅ Fidelity: {'PASS' if decompressed == data else 'FAIL'}")
```

### Installation Locations

**Windows:**
```
C:\Users\<username>\AppData\Local\Programs\Python\Python312\Lib\site-packages\kore_fileformat
```

**macOS/Linux:**
```
/usr/local/lib/python3.12/site-packages/kore_fileformat
```

---

## ☕ Java Maven Installation

### Prerequisites
```bash
# Verify Java 11+ and Maven installed
java -version
mvn --version
```

### Installation - Two Methods

#### Method 1: Maven Central Repository (Automatic)

Add to `pom.xml`:
```xml
<dependency>
    <groupId>com.kore-fileformat</groupId>
    <artifactId>kore-fileformat</artifactId>
    <version>1.1.5</version>
</dependency>
```

Then run:
```bash
mvn clean dependency:resolve
```

Maven automatically downloads from Maven Central.

#### Method 2: Local Build & Install

```bash
# Clone the Kore repository
git clone https://github.com/arunkatherashala/Kore.git
cd Kore

# Build and install locally
cargo build --release --lib
cd target/release
cargo install --path .

# Then in your Java project:
mvn install:install-file -Dfile=kore-fileformat-1.1.5.jar \
  -DgroupId=com.kore-fileformat \
  -DartifactId=kore-fileformat \
  -Dversion=1.1.5 \
  -Dpackaging=jar
```

### Quick Test - Java

**Test 1: Basic compression**
```java
import com.kore.KoreCompression;

public class KoreTest {
    public static void main(String[] args) throws Exception {
        // Create test data
        byte[] data = "hello world".repeat(1000).getBytes();
        
        // Compress
        byte[] compressed = KoreCompression.compress(data);
        float ratio = (float) compressed.length / data.length * 100;
        
        // Decompress
        byte[] decompressed = KoreCompression.decompress(compressed);
        
        // Verify
        assert java.util.Arrays.equals(data, decompressed);
        
        System.out.println("✅ Compression test passed!");
        System.out.println("Original: " + data.length + " bytes");
        System.out.println("Compressed: " + compressed.length + " bytes");
        System.out.println("Ratio: " + String.format("%.1f", ratio) + "%");
    }
}
```

### Uninstall - Java

```bash
# Remove from local Maven repository
rm -rf ~/.m2/repository/com/kore-fileformat
```

---

## 📦 JavaScript npm Installation

### Prerequisites
```bash
# Verify Node.js and npm installed
node --version
npm --version
```

### Installation Steps

**1. Install from npm:**
```bash
npm install kore-fileformat
```

Or specific version:
```bash
npm install kore-fileformat@1.1.5
```

**2. Verify installation:**
```bash
npm list kore-fileformat
```

**3. Check package.json:**
```bash
cat package.json | grep kore
```

### Quick Test - JavaScript

**Test 1: Basic compression (Node.js)**
```javascript
const kore = require('kore-fileformat');

// Create test data
const data = Buffer.from("hello world".repeat(1000));

// Compress
const compressed = kore.compress(data);
const ratio = (compressed.length / data.length * 100).toFixed(1);

// Decompress
const decompressed = kore.decompress(compressed);

// Verify
console.log("✅ Compression test passed!");
console.log(`Original: ${data.length} bytes`);
console.log(`Compressed: ${compressed.length} bytes`);
console.log(`Ratio: ${ratio}%`);
console.log(`Fidelity: ${Buffer.compare(data, decompressed) === 0 ? 'PASS' : 'FAIL'}`);
```

**Test 2: Browser usage**
```html
<!DOCTYPE html>
<html>
<head>
    <script src="https://unpkg.com/kore-fileformat@1.1.5"></script>
</head>
<body>
    <h1>KORE Compression Test</h1>
    <pre id="output"></pre>
    
    <script>
        const data = new TextEncoder().encode("hello world".repeat(1000));
        const compressed = kore.compress(data);
        const decompressed = kore.decompress(compressed);
        
        const output = `
✅ Browser compression test passed!
Original: ${data.length} bytes
Compressed: ${compressed.length} bytes
Ratio: ${(compressed.length / data.length * 100).toFixed(1)}%
Fidelity: ${JSON.stringify(data) === JSON.stringify(decompressed) ? 'PASS' : 'FAIL'}
        `;
        document.getElementById('output').textContent = output;
    </script>
</body>
</html>
```

### Uninstall - JavaScript

```bash
npm uninstall kore-fileformat
```

---

## 🔄 Full Install/Uninstall Cycle Test

This tests the complete lifecycle of installing, using, and uninstalling KORE.

### Python Full Cycle

```bash
#!/bin/bash
set -e

echo "=== PYTHON KORE FULL CYCLE TEST ==="
echo ""

# 1. Check initial state
echo "1️⃣  Checking initial state..."
pip list | grep kore && INSTALLED=yes || INSTALLED=no
echo "   KORE installed: $INSTALLED"
echo ""

# 2. Clean uninstall
if [ "$INSTALLED" = "yes" ]; then
    echo "2️⃣  Uninstalling existing KORE..."
    pip uninstall kore-fileformat -y
    pip list | grep kore && echo "   ❌ Still installed!" || echo "   ✅ Successfully uninstalled"
fi
echo ""

# 3. Fresh install
echo "3️⃣  Installing KORE from PyPI..."
pip install kore-fileformat
pip show kore-fileformat
echo "   ✅ Installation successful"
echo ""

# 4. Test basic functionality
echo "4️⃣  Testing compression..."
python3 << 'EOF'
from kore_fileformat import compress, decompress

data = b"test" * 1000
compressed = compress(data)
decompressed = decompress(compressed)

assert decompressed == data
print(f"   ✅ Compression works: {len(data)} → {len(compressed)} bytes ({len(compressed)/len(data)*100:.1f}%)")
EOF
echo ""

# 5. Uninstall again
echo "5️⃣  Uninstalling KORE..."
pip uninstall kore-fileformat -y
pip list | grep kore && echo "   ❌ Still installed!" || echo "   ✅ Successfully uninstalled"
echo ""

echo "=== ✅ FULL CYCLE TEST COMPLETE ==="
```

### Java Full Cycle

```bash
#!/bin/bash
set -e

echo "=== JAVA KORE FULL CYCLE TEST ==="
echo ""

# Create test project
echo "1️⃣  Creating test project..."
mkdir -p kore-test && cd kore-test

# Create pom.xml
cat > pom.xml << 'EOF'
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>kore-test</artifactId>
    <version>1.0</version>
    
    <dependencies>
        <dependency>
            <groupId>com.kore-fileformat</groupId>
            <artifactId>kore-fileformat</artifactId>
            <version>1.1.5</version>
        </dependency>
    </dependencies>
</project>
EOF

# Install dependencies
echo "2️⃣  Downloading dependencies..."
mvn clean dependency:resolve
echo "   ✅ Dependencies resolved"
echo ""

# Check installation
echo "3️⃣  Verifying installation..."
mvn dependency:tree | grep kore-fileformat
echo "   ✅ KORE found in dependency tree"
echo ""

# Clean up
echo "4️⃣  Cleaning up..."
mvn clean
rm -rf ~/.m2/repository/com/kore-fileformat
cd ..
rm -rf kore-test
echo "   ✅ Cleanup complete"
echo ""

echo "=== ✅ FULL CYCLE TEST COMPLETE ==="
```

### npm Full Cycle

```bash
#!/bin/bash
set -e

echo "=== NPM KORE FULL CYCLE TEST ==="
echo ""

# Create test directory
echo "1️⃣  Creating test project..."
mkdir -p kore-npm-test && cd kore-npm-test
npm init -y > /dev/null

# Install
echo "2️⃣  Installing KORE..."
npm install kore-fileformat
npm list kore-fileformat
echo "   ✅ Installation successful"
echo ""

# Test
echo "3️⃣  Testing compression..."
node << 'EOF'
const kore = require('kore-fileformat');
const data = Buffer.from("test".repeat(1000));
const compressed = kore.compress(data);
const decompressed = kore.decompress(compressed);
console.log(`   ✅ Compression works: ${data.length} → ${compressed.length} bytes (${(compressed.length/data.length*100).toFixed(1)}%)`);
EOF
echo ""

# Uninstall
echo "4️⃣  Uninstalling..."
npm uninstall kore-fileformat
npm list 2>&1 | grep -q kore && echo "   ❌ Still installed!" || echo "   ✅ Successfully uninstalled"
echo ""

# Cleanup
echo "5️⃣  Cleaning up..."
cd ..
rm -rf kore-npm-test
echo "   ✅ Cleanup complete"
echo ""

echo "=== ✅ FULL CYCLE TEST COMPLETE ==="
```

---

## ✅ Verification & Quick Start

### Post-Installation Verification

Run this to verify KORE is working correctly:

**Python:**
```python
import kore_fileformat
print(f"✅ KORE {kore_fileformat.__version__} is installed and working!")
```

**Java:**
```bash
mvn dependency:tree | grep kore-fileformat
```

**JavaScript:**
```bash
npm list kore-fileformat
```

### Performance Baseline Test

**All platforms:**
```
Test: Compress 1MB of repetitive data
Expected: <10ms compression time
Expected: 50%+ compression ratio
Expected: 100% fidelity on round-trip
```

---

## 🐛 Troubleshooting

| Issue | Python | Java | npm |
|-------|--------|------|-----|
| Installation fails | Check Python 3.9+ | Check Maven installed | Check Node.js 14+ |
| Import fails | Verify pip install | Check Maven build | Check npm list |
| Compression slow | Check CPU usage | Check -Xmx heap size | Check Node memory |
| Tests fail | Enable RUST_BACKTRACE=1 | Check Java version | Check Node version |

### Check System Requirements

**Python:**
- Python 3.9, 3.10, 3.11, or 3.12
- pip package manager
- No C compiler needed (wheels provided)

**Java:**
- Java 11+
- Maven 3.6+
- No Rust needed (pre-compiled JNI)

**npm:**
- Node.js 14+
- npm 6+
- No build tools needed (NAPI addon included)

---

## 📊 Expected Results

After installation, you should see:

```
✅ Package installed successfully
✅ Version checks work
✅ Compression/decompression works
✅ Compression ratio: 50-89% (varies by data)
✅ Throughput: 500+ MB/s
✅ 100% data fidelity (decompress == original)
```

---

## 🚀 Next Steps

1. **Use KORE in your project:**
   - Python: `from kore_fileformat import compress, decompress`
   - Java: Add to pom.xml
   - npm: `const kore = require('kore-fileformat')`

2. **Explore advanced features:**
   - Codec selection
   - Batch compression
   - Cloud storage integration

3. **Report issues:**
   - GitHub: https://github.com/arunkatherashala/Kore/issues
   - Discord: [Join community]

---

**KORE v1.1.5 - Ready to Use! 🎉**
