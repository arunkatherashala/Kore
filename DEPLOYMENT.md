# KORE Engine — Deployment & Installation Guide

**Version:** 0.1.0 (Development)  
**Status:** Alpha — functional for evaluation, not yet production-hardened  

---

## Quick Start (Build from Source)

### Prerequisites

- Rust 1.75+ with cargo
- (Optional) Python 3.10+ for the Python API

### Option 1: Rust — Build from Source

```bash
git clone <repo-url> && cd kore

# Build all crates
cargo build --release

# Run the coordinator
./target/release/kore-coord --port 9090

# In another terminal, run a worker
./target/release/kore-worker --id worker-1 --master 127.0.0.1:9090

# Submit a query
./target/release/kore-submit --master 127.0.0.1:9090 \
  --data orders.parquet --table orders \
  --sql "SELECT region, SUM(amount) FROM orders GROUP BY region"
```

### Option 2: Python API (ctypes, build Rust first)

```bash
# Build the shared library
cargo build --release -p kore-python

# Copy to py-kore/ directory
cp target/release/libkore_python.so py-kore/   # Linux
cp target/release/kore_python.dll py-kore/      # Windows

# Use from Python
cd py-kore
python -c "
from kore import KoreSession
spark = KoreSession()
spark.load_csv('data.csv', 'orders')
df = spark.table('orders').filter('amount > 100').select('id', 'amount')
df.show()
"
```

### Option 3: Docker (Coming Soon)

```bash
docker pull kore-engine:1.8.0
docker run -it kore-engine:1.8.0 bash
```

---

## Installation Details

### Rust Installation

**System Requirements:**
- Rust 1.70+ (install via [rustup.rs](https://rustup.rs))
- Linux, macOS, Windows, or BSD
- 8GB RAM minimum (16GB recommended for TPC-H benchmarks)
- 1GB disk space for crates

**Supported Targets:**
- `x86_64-unknown-linux-gnu` (Linux, primary target)
- `x86_64-apple-darwin` (macOS Intel)
- `aarch64-apple-darwin` (macOS ARM/M1+)
- `x86_64-pc-windows-msvc` (Windows MSVC)
- `x86_64-pc-windows-gnu` (Windows GNU)

**Installation:**

```bash
# Method 1: Add to Cargo.toml (recommended)
[dependencies]
kore-core = "1.8.0"
kore-sql = "1.8.0"
kore-join = "1.8.0"
kore-vectorized = "1.8.0"

# Method 2: Install binary (if available)
cargo install kore-cli --version 1.8.0

# Method 3: Build from source
git clone https://github.com/yourusername/kore.git
cd kore
git checkout v1.8.0
cargo build --release
./target/release/kore-cli
```

### Python Installation

**System Requirements:**
- Python 3.8+ (tested on 3.10, 3.11, 3.12, 3.13, 3.14)
- pip 21.0+
- Linux, macOS, or Windows
- 2GB RAM minimum

**Native Libraries:**
- Automatic binary download on `pip install`
- Supports: Linux (x86_64, ARM64), macOS (Intel, ARM), Windows (x86_64)
- Pre-built binaries: `.so`, `.dylib`, `.dll`

**Installation:**

```bash
# Simple install
pip install kore-fileformat==1.8.0

# With virtual environment (recommended)
python3 -m venv kore_env
source kore_env/bin/activate  # On Windows: kore_env\Scripts\activate
pip install kore-fileformat==1.8.0

# Verify installation
python3 -c "from kore_fileformat import KoreSession; print('✅ KORE installed')"
```

### Verification

**Rust:**
```bash
cargo new my_kore_app
cd my_kore_app

# Add to Cargo.toml
[dependencies]
kore-core = "1.8.0"

# Verify it compiles
cargo build --release
```

**Python:**
```bash
python3 << 'EOF'
from kore_fileformat import KoreSession
session = KoreSession()
print("✅ KORE Python bindings working!")
print(f"Version: 1.8.0")
EOF
```

---

## Usage Examples

### Python - Basic Query

```python
from kore_fileformat import KoreSession

# Create session
session = KoreSession()

# Load CSV data
session.load_csv("orders.csv", "orders")

# Run SQL query
results = session.execute("""
    SELECT customer_id, SUM(amount) as total
    FROM orders
    GROUP BY customer_id
    HAVING SUM(amount) > 1000
    ORDER BY total DESC
    LIMIT 10
""")

# Get results as Pandas DataFrame
df = results.to_pandas()
print(df)

# Save results
results.to_csv("top_customers.csv")
results.to_parquet("top_customers.parquet")
results.to_kore("top_customers.kore")
```

### Python - Window Functions

```python
from kore_fileformat import KoreSession

session = KoreSession()
session.load_csv("sales.csv", "sales")

# Analyze sales with window functions
results = session.execute("""
    SELECT 
        date,
        product,
        revenue,
        SUM(revenue) OVER (
            PARTITION BY product 
            ORDER BY date 
            ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
        ) as revenue_7day_ma,
        ROW_NUMBER() OVER (
            PARTITION BY product 
            ORDER BY revenue DESC
        ) as rank_in_product
    FROM sales
    ORDER BY date, product
""")

print(results)
```

### Python - ACID Transactions

```python
from kore_fileformat import KoreSession

session = KoreSession()

# Begin transaction
session.begin()

try:
    # Execute multiple statements
    session.execute("UPDATE accounts SET balance = balance - 100 WHERE id = 1")
    session.execute("UPDATE accounts SET balance = balance + 100 WHERE id = 2")
    
    # Commit if all succeed
    session.commit()
    print("✅ Transaction committed")
except Exception as e:
    # Rollback on error
    session.rollback()
    print(f"❌ Transaction rolled back: {e}")
```

### Rust - Using KORE Engine

```rust
use kore_core::DataBlock;
use kore_sql::KoreSQL;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create SQL engine
    let mut engine = KoreSQL::new();
    
    // Create table
    engine.execute(
        "CREATE TABLE orders (
            id INT,
            customer_id INT,
            amount DECIMAL(10, 2),
            date DATE
        )"
    )?;
    
    // Load data
    engine.execute(
        "INSERT INTO orders VALUES
         (1, 100, 250.50, '2024-01-15'),
         (2, 101, 150.00, '2024-01-16')"
    )?;
    
    // Query
    let results = engine.execute(
        "SELECT * FROM orders WHERE amount > 200"
    )?;
    
    println!("{:?}", results);
    Ok(())
}
```

---

## Performance Tuning

### Memory Configuration

**Python:**
```python
from kore_fileformat import KoreSession, SessionConfig

config = SessionConfig(
    max_memory_mb=4096,  # Limit to 4GB
    buffer_size=1000000   # Row buffer size
)
session = KoreSession(config)
```

**Rust:**
```rust
let config = kore_core::SessionConfig {
    memory_limit: 4096 * 1024 * 1024,  // 4GB
    buffer_size: 1_000_000,
    ..Default::default()
};
```

### Query Optimization

1. **Use column projection** - SELECT only needed columns
   ```sql
   SELECT id, name, email FROM users  -- Good ✅
   SELECT * FROM users                 -- Avoid ❌
   ```

2. **Filter early** - Apply WHERE before JOIN
   ```sql
   SELECT * FROM a JOIN b ON a.id = b.id WHERE a.status = 'active'
   ```

3. **Use indexes** - For high-cardinality columns
   ```sql
   CREATE INDEX idx_customer_id ON orders(customer_id)
   ```

4. **Batch operations** - Process multiple rows together
   ```python
   # Good ✅
   session.execute("INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie')")
   
   # Avoid ❌
   for user in users:
       session.execute(f"INSERT INTO users VALUES ({user.id}, '{user.name}')")
   ```

### Parallel Query Execution

```python
from kore_fileformat import KoreSession, ParallelConfig

config = ParallelConfig(
    num_threads=8,      # Use 8 threads
    vectorization=True  # Enable SIMD vectorization
)
session = KoreSession(config)
```

---

## Troubleshooting

### Python: ImportError - Cannot find kore_fileformat

**Error:** `ModuleNotFoundError: No module named 'kore_fileformat'`

**Solutions:**
1. Verify installation: `pip list | grep kore-fileformat`
2. Reinstall: `pip install --upgrade --force-reinstall kore-fileformat==1.8.0`
3. Check Python version: `python --version` (3.8+ required)
4. Try with full path: `python3 -m pip install kore-fileformat==1.8.0`

### Out of Memory Errors

**Error:** `Memory allocation failed` or `Cannot allocate 4GB`

**Solutions:**
1. Increase available RAM or reduce dataset size
2. Enable spilling to disk:
   ```python
   config = SessionConfig(enable_spill=True)
   ```
3. Process data in chunks
4. Use sampling for initial analysis

### Query Timeout

**Error:** `Query exceeded max execution time`

**Solutions:**
1. Check query performance: Add LIMIT for initial tests
2. Ensure indexes exist on join columns
3. Check CPU usage - may need more resources
4. Enable query caching for repeated queries

### Binary Not Found (Linux/macOS)

**Error:** `kore_ffi.so: cannot open shared object file`

**Solutions:**
1. Check LD_LIBRARY_PATH: `echo $LD_LIBRARY_PATH`
2. Install missing dependencies: `sudo apt-get install libssl-dev libffi-dev`
3. Rebuild from source: `pip install --no-binary :all: kore-fileformat==1.8.0`

---

## Platform-Specific Notes

### Linux (Ubuntu/Debian)

```bash
# Install dependencies
sudo apt-get update
sudo apt-get install -y build-essential libssl-dev pkg-config

# Install Rust (if using native Rust)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Install Python package
pip install kore-fileformat==1.8.0
```

### macOS

```bash
# Using Homebrew
brew install rust  # Optional if using Rust

# Python installation
pip install kore-fileformat==1.8.0

# For M1/M2 (ARM): Should auto-detect and download ARM binary
# For Intel: Uses x86_64 binary
```

### Windows

```powershell
# Using pip
py -m pip install kore-fileformat==1.8.0

# Or with conda
conda install -c conda-forge kore-fileformat

# For Rust development
# Download from https://rustup.rs/ and run installer
```

---

## Support & Resources

**Documentation:** [KORE v1.8.0 Documentation](https://docs.kore-engine.dev)  
**GitHub Repository:** [kore-engine/kore](https://github.com/yourusername/kore)  
**Issue Tracker:** [GitHub Issues](https://github.com/yourusername/kore/issues)  
**Discussions:** [GitHub Discussions](https://github.com/yourusername/kore/discussions)  

**Performance Benchmarks:** [KORE vs DuckDB vs Spark](./KORE_BENCHMARK_REPORT.md)  

---

## License

KORE v1.8.0 is released under the **Apache License 2.0**. See LICENSE file for details.

---

## Release Notes

See [RELEASE_NOTES.md](./RELEASE_NOTES.md) for detailed changelog and migration guide.

---

**Installation Complete!** 🚀  

Next: [Run your first query](./GETTING_STARTED.md)
