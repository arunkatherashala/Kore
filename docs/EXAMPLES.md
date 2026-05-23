# 💡 Code Examples - KORE v1.1.4

Ready-to-use code examples for common KORE tasks.

---

## Table of Contents
1. [Quick Examples](#quick-examples)
2. [File Operations](#file-operations)
3. [Data Processing](#data-processing)
4. [Kafka Integration](#kafka-integration)
5. [S3 Integration](#s3-integration)
6. [Advanced Patterns](#advanced-patterns)

---

## Quick Examples

### Example 1: Compress a CSV File (One-liner)

```python
from kore_fileformat import compress_csv

original, compressed, ratio = compress_csv("data.csv", "data.kore")
print(f"✅ Compressed {ratio:.1%}")
```

### Example 2: Check File Info

```python
from kore_fileformat import get_kore_info

info = get_kore_info("data.kore")
print(f"Records: {info['total_records']:,}")
```

### Example 3: Read KORE File

```python
from kore_fileformat import KoreReader

with KoreReader("data.kore") as reader:
    records = reader.read(limit=10)
    for r in records:
        print(r)
```

### Example 4: Write KORE File

```python
from kore_fileformat import KoreWriter

with KoreWriter("output.kore") as writer:
    writer.write({"id": 1, "value": 100})
    writer.write({"id": 2, "value": 200})
```

---

## File Operations

### Example 5: Compress Multiple Files

```python
import os
from pathlib import Path
from kore_fileformat import compress_csv

csv_dir = "data/csv"
output_dir = "data/kore"
os.makedirs(output_dir, exist_ok=True)

for csv_file in Path(csv_dir).glob("*.csv"):
    output = Path(output_dir) / f"{csv_file.stem}.kore"
    original, compressed, ratio = compress_csv(str(csv_file), str(output))
    print(f"✅ {csv_file.name}: {ratio:.1%}")
```

### Example 6: Batch Processing with Progress

```python
from pathlib import Path
from kore_fileformat import compress_csv
from tqdm import tqdm

csv_files = list(Path("data/csv").glob("*.csv"))

total_original = 0
total_compressed = 0

for csv_file in tqdm(csv_files, desc="Compressing"):
    output = csv_file.parent.parent / "kore" / f"{csv_file.stem}.kore"
    original, compressed, _ = compress_csv(str(csv_file), str(output))
    total_original += original
    total_compressed += compressed

ratio = (total_compressed / total_original) * 100
print(f"🎉 Total compression: {ratio:.1f}%")
print(f"📊 Saved: {(total_original - total_compressed) / 1024 / 1024:.2f} MB")
```

### Example 7: Verify Compression

```python
import os
from kore_fileformat import compress_csv, get_kore_info

def verify_compression(csv_file, kore_file):
    """Verify KORE file integrity"""
    
    # Compress
    original, compressed, ratio = compress_csv(csv_file, kore_file)
    
    # Verify
    if not os.path.exists(kore_file):
        return False, "File not created"
    
    info = get_kore_info(kore_file)
    
    if info['total_records'] == 0:
        return False, "No records found"
    
    if os.path.getsize(kore_file) == 0:
        return False, "File is empty"
    
    return True, f"✅ {ratio:.1%} compression"

success, message = verify_compression("data.csv", "data.kore")
print(message)
```

### Example 8: Archive Old Files

```python
import os
import time
from datetime import datetime, timedelta
from pathlib import Path
from kore_fileformat import compress_csv

# Archive files older than 7 days
archive_days = 7
archive_time = time.time() - (archive_days * 24 * 60 * 60)

archive_dir = "archive"
os.makedirs(archive_dir, exist_ok=True)

for csv_file in Path("logs").glob("*.csv"):
    file_time = os.path.getmtime(csv_file)
    
    if file_time < archive_time:
        output = Path(archive_dir) / f"{csv_file.stem}.kore"
        original, compressed, ratio = compress_csv(str(csv_file), str(output))
        
        # Delete original
        os.remove(csv_file)
        print(f"📦 Archived {csv_file.name}")
```

---

## Data Processing

### Example 9: Filter and Compress

```python
import csv
from kore_fileformat import KoreWriter

# Read CSV, filter, write KORE
input_csv = "raw_data.csv"
output_kore = "filtered_data.kore"

with KoreWriter(output_kore) as writer:
    with open(input_csv) as f:
        reader = csv.DictReader(f)
        for row in reader:
            # Filter: only active users
            if row['status'] == 'active':
                writer.write(row)

print("✅ Filtered and compressed")
```

### Example 10: Transform Data

```python
from kore_fileformat import KoreReader, KoreWriter

# Read, transform, write
with KoreReader("input.kore") as reader:
    with KoreWriter("output.kore") as writer:
        for record in reader.read():
            # Transform
            record['salary'] = float(record['salary']) * 1.1
            record['processed'] = True
            
            writer.write(record)

print("✅ Transformed data")
```

### Example 11: Aggregation

```python
from kore_fileformat import KoreReader
from collections import defaultdict

# Aggregate by category
with KoreReader("sales.kore") as reader:
    sales_by_category = defaultdict(float)
    
    for record in reader.read():
        category = record['category']
        amount = float(record['amount'])
        sales_by_category[category] += amount

for category, total in sorted(sales_by_category.items()):
    print(f"{category}: ${total:,.2f}")
```

### Example 12: Statistical Analysis

```python
from kore_fileformat import KoreReader
import statistics

with KoreReader("data.kore") as reader:
    values = []
    
    for record in reader.read():
        values.append(float(record['value']))

stats = {
    'mean': statistics.mean(values),
    'median': statistics.median(values),
    'stdev': statistics.stdev(values),
    'min': min(values),
    'max': max(values),
}

print(f"📊 Statistics:")
for key, val in stats.items():
    print(f"  {key}: {val:.2f}")
```

---

## Kafka Integration

### Example 13: Kafka Consumer → KORE

```python
from kafka import KafkaConsumer
from kore_fileformat import KoreWriter
import json

# Consume from Kafka and write to KORE
consumer = KafkaConsumer(
    'events',
    bootstrap_servers=['localhost:9092'],
    value_deserializer=lambda m: json.loads(m.decode('utf-8')),
    consumer_timeout_ms=10000
)

with KoreWriter("kafka_events.kore") as writer:
    for message in consumer:
        writer.write(message.value)
        
        if message.offset % 1000 == 0:
            print(f"📥 Consumed {message.offset} messages")

print("✅ Kafka to KORE complete")
```

### Example 14: KORE → Kafka Producer

```python
from kafka import KafkaProducer
from kore_fileformat import KoreReader
import json

producer = KafkaProducer(
    bootstrap_servers=['localhost:9092'],
    value_serializer=lambda m: json.dumps(m).encode('utf-8')
)

with KoreReader("data.kore") as reader:
    count = 0
    for record in reader.read():
        producer.send('output-topic', record)
        count += 1
        
        if count % 1000 == 0:
            print(f"📤 Produced {count} messages")

producer.flush()
print(f"✅ KORE to Kafka: {count} messages")
```

### Example 15: Real-time Stream Processing

```python
from kafka import KafkaConsumer, KafkaProducer
from kore_fileformat import KoreWriter, KoreReader
import json

# Config
BATCH_SIZE = 10000
BATCH_DIR = "batch_data"

# Consumer
consumer = KafkaConsumer(
    'events',
    bootstrap_servers=['localhost:9092'],
    value_deserializer=lambda m: json.loads(m.decode('utf-8')),
)

# Batch processor
batch = []
for message in consumer:
    batch.append(message.value)
    
    if len(batch) >= BATCH_SIZE:
        # Compress batch
        with KoreWriter(f"{BATCH_DIR}/batch_{len(batch)}.kore") as w:
            for record in batch:
                w.write(record)
        
        print(f"✅ Batch {len(batch)} compressed")
        batch = []
```

---

## S3 Integration

### Example 16: Upload KORE to S3

```python
import boto3
from kore_fileformat import compress_csv

# Compress locally
compress_csv("large_file.csv", "large_file.kore")

# Upload to S3
s3 = boto3.client('s3')
s3.upload_file(
    'large_file.kore',
    'my-bucket',
    'data/large_file.kore'
)

print("✅ Uploaded to S3")
```

### Example 17: Download and Read from S3

```python
import boto3
import tempfile
from kore_fileformat import KoreReader

s3 = boto3.client('s3')

# Download from S3
with tempfile.NamedTemporaryFile() as tmp:
    s3.download_file('my-bucket', 'data/data.kore', tmp.name)
    
    # Read from temp file
    with KoreReader(tmp.name) as reader:
        records = reader.read(limit=100)
        for record in records:
            print(record)
```

---

## Advanced Patterns

### Example 18: Parallel Processing

```python
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from kore_fileformat import compress_csv

def compress_file(csv_file):
    output = csv_file.parent.parent / "kore" / f"{csv_file.stem}.kore"
    original, compressed, ratio = compress_csv(str(csv_file), str(output))
    return csv_file.name, ratio

csv_files = list(Path("data/csv").glob("*.csv"))

with ThreadPoolExecutor(max_workers=4) as executor:
    results = executor.map(compress_file, csv_files)
    
    for filename, ratio in results:
        print(f"✅ {filename}: {ratio:.1%}")
```

### Example 19: Error Handling Pipeline

```python
from pathlib import Path
from kore_fileformat import compress_csv
import logging

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

def safe_compress(csv_file, kore_file):
    try:
        original, compressed, ratio = compress_csv(csv_file, kore_file)
        logger.info(f"✅ {csv_file}: {ratio:.1%}")
        return True
    except FileNotFoundError:
        logger.error(f"❌ File not found: {csv_file}")
        return False
    except Exception as e:
        logger.error(f"❌ Error processing {csv_file}: {e}")
        return False

# Process with error handling
for csv_file in Path("data").glob("*.csv"):
    safe_compress(str(csv_file), str(csv_file.with_suffix(".kore")))
```

### Example 20: Monitoring Dashboard

```python
from kore_fileformat import get_kore_info, KoreReader
import os

def file_stats(kore_file):
    """Get comprehensive file statistics"""
    
    info = get_kore_info(kore_file)
    file_size = os.path.getsize(kore_file)
    
    with KoreReader(kore_file) as reader:
        first_record = reader.read(limit=1)[0] if reader.count() > 0 else {}
    
    return {
        'file': os.path.basename(kore_file),
        'size_mb': file_size / 1024 / 1024,
        'records': info['total_records'],
        'version': info['version'],
        'columns': len(first_record),
    }

# Dashboard
for kore_file in Path("data").glob("*.kore"):
    stats = file_stats(str(kore_file))
    print(f"{stats['file']:30s} | {stats['size_mb']:8.2f} MB | {stats['records']:10,d} records")
```

---

## Complete Application Example

### Example 21: ETL Pipeline

```python
"""
Complete ETL (Extract, Transform, Load) pipeline using KORE
"""
from pathlib import Path
from kore_fileformat import KoreWriter, KoreReader, compress_csv
import csv

class ETLPipeline:
    def __init__(self, source_dir, target_dir):
        self.source_dir = Path(source_dir)
        self.target_dir = Path(target_dir)
        self.target_dir.mkdir(exist_ok=True)
    
    def extract(self, csv_file):
        """Extract data from CSV"""
        with open(csv_file) as f:
            return list(csv.DictReader(f))
    
    def transform(self, records):
        """Transform data"""
        transformed = []
        for record in records:
            # Clean and transform
            record['amount'] = float(record.get('amount', 0))
            record['processed'] = True
            transformed.append(record)
        return transformed
    
    def load(self, records, output_file):
        """Load data to KORE"""
        with KoreWriter(output_file) as writer:
            for record in records:
                writer.write(record)
    
    def process_file(self, csv_file):
        """Process single file"""
        print(f"Processing {csv_file.name}...")
        
        # Extract
        records = self.extract(csv_file)
        
        # Transform
        transformed = self.transform(records)
        
        # Load
        output = self.target_dir / f"{csv_file.stem}.kore"
        self.load(transformed, str(output))
        
        print(f"✅ Saved to {output}")
    
    def run(self):
        """Process all CSV files"""
        for csv_file in self.source_dir.glob("*.csv"):
            self.process_file(csv_file)

# Usage
pipeline = ETLPipeline("source_data", "target_data")
pipeline.run()
```

---

## Getting Help

### 📚 Related Documents
- 📦 [Installation Guide](INSTALLATION.md)
- 📖 [User Guide](USER_GUIDE.md)
- 🔧 [API Reference](API_REFERENCE.md)

### 💬 Questions?
- GitHub Issues: https://github.com/arunkatherashala/Kore/issues
- Email: arunkatherashala@gmail.com

---

**Happy coding! 🚀**
