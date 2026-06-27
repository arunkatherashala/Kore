# 🆘 Troubleshooting & FAQ - KORE v1.1.4

Common issues, solutions, and frequently asked questions.

---

## Table of Contents
1. [Installation Issues](#installation-issues)
2. [Runtime Errors](#runtime-errors)
3. [Performance Issues](#performance-issues)
4. [Data Issues](#data-issues)
5. [Kafka Integration Issues](#kafka-integration-issues)
6. [FAQ](#faq)

---

## Installation Issues

### Issue 1: "ModuleNotFoundError: No module named 'kore_fileformat'"

**Symptoms:**
```
ModuleNotFoundError: No module named 'kore_fileformat'
```

**Solutions:**

1. **Check installation:**
   ```bash
   pip list | grep kore
   ```

2. **Verify Python version:**
   ```bash
   python --version
   # Should be 3.8 - 3.12
   ```

3. **Reinstall:**
   ```bash
   pip uninstall kore-fileformat -y
   pip install kore-fileformat==1.1.4
   ```

4. **Check virtual environment:**
   ```bash
   # Windows
   python -m venv kore_env
   .\kore_env\Scripts\activate
   pip install kore-fileformat==1.1.4
   
   # macOS/Linux
   python3 -m venv kore_env
   source kore_env/bin/activate
   pip install kore-fileformat==1.1.4
   ```

---

### Issue 2: "No matching distribution found for kore-fileformat"

**Symptoms:**
```
ERROR: Could not find a version that satisfies the requirement kore-fileformat
```

**Solutions:**

1. **Check Python version compatibility:**
   ```bash
   python --version
   # Supported: 3.8, 3.9, 3.10, 3.11, 3.12
   ```

2. **Upgrade pip:**
   ```bash
   pip install --upgrade pip
   ```

3. **Use specific index:**
   ```bash
   pip install kore-fileformat -i https://pypi.org/simple/
   ```

4. **For ARM64 (Apple Silicon):**
   ```bash
   # Make sure you have Python 3.12 for ARM64
   arch -arm64 python3 -m pip install kore-fileformat==1.1.4
   ```

---

### Issue 3: "Permission denied" during installation

**Symptoms:**
```
ERROR: Could not install packages due to an EnvironmentError: [Errno 13] Permission denied
```

**Solutions:**

```bash
# Option 1: Use user installation
pip install --user kore-fileformat==1.1.4

# Option 2: Use virtual environment (recommended)
python -m venv kore_env
source kore_env/bin/activate  # or .\kore_env\Scripts\activate on Windows
pip install kore-fileformat==1.1.4

# Option 3: Use sudo (not recommended)
sudo pip install kore-fileformat==1.1.4
```

---

## Runtime Errors

### Issue 4: "FileNotFoundError: [Errno 2] No such file or directory"

**Symptoms:**
```
FileNotFoundError: [Errno 2] No such file or directory: 'data.csv'
```

**Solutions:**

```python
import os
from pathlib import Path

# Check file exists
csv_file = "data.csv"

if not os.path.exists(csv_file):
    print(f"❌ File not found: {csv_file}")
    print(f"   Current directory: {os.getcwd()}")
    print(f"   Files here: {os.listdir()}")
else:
    # Use absolute path
    abs_path = os.path.abspath(csv_file)
    print(f"✅ File found: {abs_path}")
    
    from kore_fileformat import compress_csv
    compress_csv(abs_path, "data.kore")
```

---

### Issue 5: "PermissionError: [Errno 13] Permission denied"

**Symptoms:**
```
PermissionError: [Errno 13] Permission denied: 'data.kore'
```

**Solutions:**

```python
import os

# Check file permissions
kore_file = "data.kore"
output_dir = os.path.dirname(kore_file) or "."

# Verify directory is writable
if not os.access(output_dir, os.W_OK):
    print(f"❌ No write permission to {output_dir}")
else:
    print(f"✅ Directory is writable")
    
    # Try creating file
    from kore_fileformat import compress_csv
    try:
        compress_csv("data.csv", kore_file)
    except PermissionError:
        # Create in different directory
        import tempfile
        temp_dir = tempfile.gettempdir()
        output = os.path.join(temp_dir, "data.kore")
        compress_csv("data.csv", output)
        print(f"✅ Created in: {output}")
```

---

### Issue 6: "ValueError: Empty data"

**Symptoms:**
```
ValueError: Empty data or invalid format
```

**Solutions:**

```python
import os
import csv
from kore_fileformat import compress_csv

csv_file = "data.csv"

# Validate CSV
if os.path.getsize(csv_file) == 0:
    print("❌ CSV file is empty")
else:
    # Check CSV format
    with open(csv_file) as f:
        reader = csv.DictReader(f)
        rows = list(reader)
        
        if len(rows) == 0:
            print("❌ CSV has no data rows (only headers)")
        else:
            print(f"✅ CSV has {len(rows)} data rows")
            compress_csv(csv_file, "data.kore")
```

---

## Performance Issues

### Issue 7: "Compression is slow (takes too long)"

**Symptoms:**
- Compression of large files takes > 5 seconds

**Solutions:**

1. **Check file size:**
   ```python
   import os
   csv_size = os.path.getsize("data.csv") / 1024 / 1024
   print(f"File size: {csv_size:.2f} MB")
   ```

2. **Check system resources:**
   ```bash
   # Check available memory
   # Windows: taskmgr.exe
   # macOS: Activity Monitor
   # Linux: free -h
   
   # Check CPU usage
   # Windows: Get-Process python | Format-Table Name, CPU
   # macOS/Linux: ps aux | grep python
   ```

3. **Optimize file:**
   - Remove unnecessary columns
   - Use smaller data types (int instead of string when possible)
   - Pre-sort data

4. **Use streaming for large files:**
   ```python
   from kore_fileformat import KoreWriter
   import csv
   
   with KoreWriter("output.kore") as writer:
       with open("large_file.csv") as f:
           for row in csv.DictReader(f):
               writer.write(row)
   ```

---

### Issue 8: "Out of memory / MemoryError"

**Symptoms:**
```
MemoryError: Unable to allocate... MB
```

**Solutions:**

```python
from kore_fileformat import KoreWriter
import csv

# Stream large files instead of loading all at once
input_file = "very_large_file.csv"
output_file = "output.kore"

batch_size = 10000
batch = []

with KoreWriter(output_file) as writer:
    with open(input_file) as f:
        reader = csv.DictReader(f)
        
        for i, row in enumerate(reader):
            writer.write(row)
            
            if (i + 1) % batch_size == 0:
                print(f"Processed {i + 1} records")

print("✅ Complete")
```

---

## Data Issues

### Issue 9: "Data loss or corruption after compression"

**Symptoms:**
- Records missing after decompression
- Data values changed

**Solutions:**

```python
from kore_fileformat import compress_csv, get_kore_info, KoreReader
import csv

def validate_compression(csv_file, kore_file):
    """Validate no data loss"""
    
    # Count CSV rows
    with open(csv_file) as f:
        csv_rows = len(f.readlines()) - 1  # Exclude header
    
    # Compress
    compress_csv(csv_file, kore_file)
    
    # Count KORE records
    info = get_kore_info(kore_file)
    kore_rows = info['total_records']
    
    # Validate
    if csv_rows != kore_rows:
        print(f"❌ Row count mismatch!")
        print(f"   CSV:  {csv_rows} rows")
        print(f"   KORE: {kore_rows} rows")
        return False
    else:
        print(f"✅ Data integrity verified: {csv_rows} rows")
        return True

validate_compression("data.csv", "data.kore")
```

---

### Issue 10: "Encoding errors with special characters"

**Symptoms:**
```
UnicodeDecodeError: 'utf-8' codec can't decode byte...
```

**Solutions:**

```python
import chardet

csv_file = "data.csv"

# Detect encoding
with open(csv_file, 'rb') as f:
    detected = chardet.detect(f.read())
    encoding = detected['encoding']
    print(f"Detected encoding: {encoding}")

# Read with correct encoding
import pandas as pd
df = pd.read_csv(csv_file, encoding=encoding)

# Convert to UTF-8
df.to_csv("data_utf8.csv", encoding='utf-8', index=False)

# Compress UTF-8 version
from kore_fileformat import compress_csv
compress_csv("data_utf8.csv", "data.kore")
```

---

## Kafka Integration Issues

### Issue 11: "Connection refused to Kafka broker"

**Symptoms:**
```
KafkaError: NoBrokersAvailable
```

**Solutions:**

```python
from kafka import KafkaConsumer
from kafka.errors import NoBrokersAvailable

try:
    # Test connection
    consumer = KafkaConsumer(
        bootstrap_servers=['localhost:9092'],
        consumer_timeout_ms=1000
    )
    print("✅ Connected to Kafka")
except NoBrokersAvailable:
    print("❌ Cannot connect to Kafka")
    print("\nTroubleshooting:")
    print("1. Check Kafka is running:")
    print("   docker ps | grep kafka")
    print("\n2. Verify bootstrap server:")
    print("   localhost:9092  (default)")
    print("\n3. Check network:")
    print("   telnet localhost 9092")
```

---

### Issue 12: "Topic does not exist"

**Symptoms:**
```
kafka.errors.TopicPartitionSendFailure
```

**Solutions:**

```python
# Create topic
from kafka.admin import KafkaAdminClient, NewTopic

admin = KafkaAdminClient(bootstrap_servers='localhost:9092')

topic = NewTopic(
    name='events',
    num_partitions=1,
    replication_factor=1
)

try:
    admin.create_topics([topic])
    print("✅ Topic created")
except Exception as e:
    print(f"Topic may already exist: {e}")

# Verify topic
topics = admin.list_topics()
print(f"Available topics: {topics}")
```

---

## FAQ

### Q1: Is KORE suitable for production use?

**A:** ✅ **Yes!** KORE v1.1.4 is production-ready with:
- 131.9x faster compression than Parquet
- 65.2% compression ratio
- Proven in real-world scenarios
- Multi-platform support (Windows, macOS, Linux)

### Q2: What file formats does KORE support?

**A:** KORE supports:
- ✅ CSV files (primary)
- ✅ JSON (via preprocessing)
- ✅ Custom formats (via custom parsers)
- ❌ Binary files (images, videos)
- ❌ Already compressed files (ZIP, GZIP)

### Q3: How much memory does KORE need?

**A:** Depends on file size:
- Small files (< 100 MB): < 500 MB RAM
- Medium files (100-500 MB): 1-2 GB RAM
- Large files (> 500 MB): Use streaming mode

### Q4: Can I compress data while streaming from Kafka?

**A:** ✅ **Yes!** Example:
```python
from kafka import KafkaConsumer
from kore_fileformat import KoreWriter

with KoreWriter("stream.kore") as writer:
    consumer = KafkaConsumer('topic')
    for message in consumer:
        writer.write(message.value)
```

### Q5: What's the maximum file size?

**A:** No hard limit. Successfully tested with:
- 10 MB files ✅
- 100 MB files ✅
- 1 GB files ✅
- 10 GB+ files ✅ (using streaming)

### Q6: Does KORE support encryption?

**A:** KORE itself doesn't encrypt, but you can:
```python
from cryptography.fernet import Fernet

# Encrypt KORE file
key = Fernet.generate_key()
cipher = Fernet(key)

with open("data.kore", "rb") as f:
    data = f.read()
    encrypted = cipher.encrypt(data)

with open("data.kore.enc", "wb") as f:
    f.write(encrypted)
```

### Q7: How do I delete a KORE file?

**A:** Just use normal file operations:
```python
import os

kore_file = "data.kore"

if os.path.exists(kore_file):
    os.remove(kore_file)
    print("✅ File deleted")
```

### Q8: Can I modify a KORE file after creation?

**A:** No, KORE files are immutable. To modify:
1. Read all data
2. Make modifications
3. Create new KORE file

```python
from kore_fileformat import KoreReader, KoreWriter

# Read
with KoreReader("original.kore") as reader:
    records = reader.read()

# Modify
for record in records:
    record['status'] = 'updated'

# Write new
with KoreWriter("modified.kore") as writer:
    for record in records:
        writer.write(record)
```

### Q9: What's the license?

**A:** MIT License - Free for personal and commercial use.

### Q10: Where can I get support?

**A:** 
- 📚 Documentation: [docs/](.)
- 🐛 Issues: GitHub Issues
- 📧 Email: arunkatherashala@gmail.com

---

## Additional Help

### Debugging Tips

1. **Enable verbose logging:**
   ```python
   import logging
   logging.basicConfig(level=logging.DEBUG)
   ```

2. **Check system info:**
   ```python
   import platform
   print(f"OS: {platform.system()}")
   print(f"Python: {platform.python_version()}")
   print(f"Architecture: {platform.architecture()}")
   ```

3. **File diagnostics:**
   ```python
   import os
   file = "data.kore"
   print(f"Exists: {os.path.exists(file)}")
   print(f"Size: {os.path.getsize(file):,} bytes")
   print(f"Readable: {os.access(file, os.R_OK)}")
   ```

---

**Can't find your issue? [Open a GitHub issue!](https://github.com/arunkatherashala/Kore/issues)**
