# 📦 Installation Guide - KORE v1.1.5

Complete step-by-step guide to install and setup KORE on your system.

---

## Table of Contents
1. [System Requirements](#system-requirements)
2. [Python Installation](#python-installation)
3. [Rust Installation](#rust-installation)
4. [Verify Installation](#verify-installation)
5. [Troubleshooting](#troubleshooting)

---

## System Requirements

### Python Installation
- **Python Version**: 3.8 - 3.12
- **OS Support**: Windows, macOS, Linux
- **Space Required**: ~100 MB

### Rust Installation
- **Rust Version**: 1.70+
- **Edition**: 2021
- **Build Tools**: Cargo package manager

---

## Python Installation

### Option 1: Using pip (Recommended for Users)

#### Step 1: Verify Python Installation
```bash
python --version
```
Expected output: `Python 3.8+` to `3.12`

#### Step 2: Create Virtual Environment (Optional but Recommended)
```bash
# Windows
python -m venv kore_env
kore_env\Scripts\activate

# macOS/Linux
python -m venv kore_env
source kore_env/bin/activate
```

#### Step 3: Install KORE
```bash
pip install kore-fileformat==1.1.5
```

#### Step 4: Verify Installation
```bash
python -c "import kore_fileformat; print(f'KORE v{kore_fileformat.__version__}')"
```
Expected output: `KORE v1.1.5`

---

### Option 2: From Source (For Developers)

#### Step 1: Clone Repository
```bash
git clone https://github.com/arunkatherashala/Kore.git
cd Kore
```

#### Step 2: Build with maturin
```bash
# Install maturin
pip install maturin

# Build wheels
maturin build --release
```

#### Step 3: Install Locally
```bash
pip install target/wheels/*.whl
```

---

## Rust Installation

### For Developers Only (Users: Skip This Section)

#### Step 1: Install Rust
```bash
# macOS/Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Windows (Use PowerShell as Administrator)
# Download from: https://www.rust-lang.org/tools/install
```

#### Step 2: Verify Rust Installation
```bash
rustc --version
cargo --version
```

#### Step 3: Install Nightly Rust (Required for KORE)
```bash
rustup default nightly
rustup update
```

#### Step 4: Clone and Build KORE
```bash
git clone https://github.com/arunkatherashala/Kore.git
cd Kore
cargo build --release
```

#### Step 5: Run Tests
```bash
cargo test --release
```

---

## Verify Installation

### Quick Test Script
```python
# test_kore_install.py
from kore_fileformat import KoreWriter, KoreReader, compress_csv, get_kore_info

print("✅ KORE modules imported successfully!")

# Test basic functionality
import tempfile
import os

# Create test data
with tempfile.TemporaryDirectory() as tmpdir:
    csv_file = os.path.join(tmpdir, "test.csv")
    kore_file = os.path.join(tmpdir, "test.kore")
    
    # Write test CSV
    with open(csv_file, 'w') as f:
        f.write("id,value,timestamp\n")
        f.write("1,100,2026-05-16 10:00:00\n")
        f.write("2,200,2026-05-16 10:05:00\n")
        f.write("3,300,2026-05-16 10:10:00\n")
    
    # Compress CSV to KORE
    original_size, compressed_size, compression_ratio = compress_csv(csv_file, kore_file)
    
    print(f"\n✅ Compression Test:")
    print(f"   Original Size:    {original_size:,} bytes")
    print(f"   Compressed Size:  {compressed_size:,} bytes")
    print(f"   Compression Ratio: {compression_ratio:.2%}")
    
    # Get file info
    info = get_kore_info(kore_file)
    print(f"\n✅ File Info:")
    print(f"   Total Records: {info['total_records']}")
    print(f"   Version: {info['version']}")

print("\n🎉 Installation verified successfully!")
```

### Run Verification
```bash
python test_kore_install.py
```

Expected output:
```
✅ KORE modules imported successfully!

✅ Compression Test:
   Original Size:    108 bytes
   Compressed Size:  156 bytes
   Compression Ratio: 143.53%

✅ File Info:
   Total Records: 3
   Version: kore_fileformat-1.1.5

🎉 Installation verified successfully!
```

---

## Docker Installation

### Option 3: Using Docker

#### Step 1: Create Dockerfile
```dockerfile
FROM python:3.12-slim

WORKDIR /app

# Install KORE
RUN pip install kore-fileformat==1.1.5

# Copy your code
COPY . .

CMD ["python", "your_script.py"]
```

#### Step 2: Build Docker Image
```bash
docker build -t kore-app:latest .
```

#### Step 3: Run Container
```bash
docker run -v $(pwd):/app kore-app:latest
```

---

## Platform-Specific Installation

### Windows
```powershell
# PowerShell (as Administrator)
python -m venv kore_env
.\kore_env\Scripts\Activate.ps1
pip install kore-fileformat==1.1.5
```

### macOS
```bash
# Using Homebrew (optional)
brew install python@3.12

# Using venv
python3 -m venv kore_env
source kore_env/bin/activate
pip install kore-fileformat==1.1.5
```

### Linux (Ubuntu/Debian)
```bash
# Install Python
sudo apt-get update
sudo apt-get install python3.12 python3.12-venv

# Create virtual environment
python3.12 -m venv kore_env
source kore_env/bin/activate

# Install KORE
pip install kore-fileformat==1.1.5
```

---

## Troubleshooting

### Issue 1: "ModuleNotFoundError: No module named 'kore_fileformat'"
```
✅ Solution:
1. Verify Python version: python --version (should be 3.8+)
2. Reinstall: pip uninstall kore-fileformat && pip install kore-fileformat==1.1.5
3. Check pip: pip list | grep kore
```

### Issue 2: "pip install" hangs or times out
```
✅ Solutions:
1. Use default pip index: pip install --default-timeout=1000 kore-fileformat
2. Use alternative index: pip install -i https://pypi.org/simple/ kore-fileformat
3. Check internet connection
```

### Issue 3: "Wheel not available for your platform"
```
✅ Solution:
Ensure you have the right Python version:
- Windows: cp312-win_amd64 (Python 3.12 on Windows)
- macOS: cp312-macosx_11_0_arm64 (Python 3.12 on Apple Silicon)
- Linux: cp312-manylinux_2_34_x86_64 (Python 3.12 on Linux)

Check your platform: python -c "import wheel.pep425tags; print(wheel.pep425tags.get_supported())"
```

### Issue 4: "Permission denied" on Linux/macOS
```bash
# Run with sudo
sudo pip install kore-fileformat==1.1.5

# OR use user installation
pip install --user kore-fileformat==1.1.5
```

### Issue 5: Building from source fails
```bash
# Ensure you have Rust installed
rustc --version
cargo --version

# Update Rust
rustup update

# Try building again
maturin build --release
```

---

## Getting Help

### Resources
- 📚 **User Guide**: See [USER_GUIDE.md](USER_GUIDE.md)
- 🔧 **API Reference**: See [API_REFERENCE.md](API_REFERENCE.md)
- 📖 **Examples**: See [EXAMPLES.md](EXAMPLES.md)
- 🆘 **Troubleshooting**: See [TROUBLESHOOTING.md](TROUBLESHOOTING.md)

### Contact
- 💬 GitHub Issues: https://github.com/arunkatherashala/Kore/issues
- 📧 Email: arunkatherashala@gmail.com

---

## Next Steps

After successful installation:
1. ✅ Read [USER_GUIDE.md](USER_GUIDE.md) for basic usage
2. ✅ Check [EXAMPLES.md](EXAMPLES.md) for code samples
3. ✅ Explore [API_REFERENCE.md](API_REFERENCE.md) for detailed API docs
4. ✅ Start compressing files! 🚀
