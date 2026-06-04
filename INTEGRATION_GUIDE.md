# Language Integration Guide for KORE v1.3.3

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Overview](#overview)
2. [Integration Architecture](#integration-architecture)
3. [Language Interoperability](#language-interoperability)
4. [Use Case Patterns](#use-case-patterns)
5. [Data Flow](#data-flow)
6. [Best Practices](#best-practices)

---

## Overview

KORE v1.3.3 is built in **Rust** but designed to integrate with multiple programming languages for different use cases:

- **Rust** — KORE engine core (primary)
- **Python** — Data analysis and processing
- **Java/Kotlin** — Enterprise applications
- **Go** — Microservices and APIs
- **JavaScript/TypeScript** — Web interfaces and tools
- **C#** — Windows integration
- **SQL** — Database operations
- **Maven** — Build orchestration

---

## Integration Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    KORE v1.3.3 Core                         │
│                   (Rust - KORE Engine)                      │
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐    │
│  │ Schema   │  │ ACID     │  │ Query    │  │   AI     │    │
│  │Evolution │  │Transactions│Optimization│ Features │    │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘    │
└────────────────────────┬────────────────────────────────────┘
                         │
        ┌────────────────┼────────────────┐
        │                │                │
        ▼                ▼                ▼
   ┌─────────┐      ┌─────────┐      ┌─────────┐
   │  APIs   │      │ Files   │      │Database │
   │ (REST)  │      │(.kore)  │      │(SQL)    │
   └────┬────┘      └────┬────┘      └────┬────┘
        │                │                │
   ┌────┴────────────────┴────────────────┴─────┐
   │                                             │
   │     Language Integration Layer              │
   │                                             │
   ├─────────────┬─────────────┬────────────────┤
   │   Go        │  Python     │ JavaScript/TS  │
   │ Microsvcs   │ Data Tools  │  Web APIs      │
   │             │             │                │
   │   Java      │  Kotlin     │  C#            │
   │ Enterprise  │  Modern JVM │  .NET Windows  │
   └─────────────┴─────────────┴────────────────┘
```

---

## Language Interoperability

### 1. Rust ↔ Python (Data Processing)

**Use Case:** KORE engine processes data, Python analyzes it

**Pattern:**
```
Rust (KORE Core)
    ↓
[Serialize KORE data]
    ↓
Python Script
    ↓
[Analyze with numpy, pandas]
    ↓
[Return results]
    ↓
Rust (Store results)
```

**Implementation:**
```rust
// In Rust (KORE)
use std::process::Command;

pub fn run_python_analysis(data_file: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("python")
        .arg("analyze.py")
        .arg(data_file)
        .output()?;
    
    Ok(String::from_utf8(output.stdout)?)
}
```

```python
# In Python (analyze.py)
import sys
import numpy as np
import pandas as pd

def analyze_kore_data(filename):
    # Load KORE data
    data = pd.read_csv(filename)
    
    # Analyze
    stats = {
        'mean': float(data.mean()),
        'std': float(data.std()),
        'min': float(data.min()),
        'max': float(data.max())
    }
    
    return stats

if __name__ == "__main__":
    filename = sys.argv[1]
    stats = analyze_kore_data(filename)
    print(json.dumps(stats))
```

---

### 2. Rust ↔ Go (Microservices)

**Use Case:** Go services expose REST APIs to KORE data

**Pattern:**
```
KORE (Rust Core)
    ↓
[Expose via REST API]
    ↓
Go Microservice
    ↓
[Handles HTTP requests]
    ↓
[Calls KORE via HTTP]
    ↓
REST Response
```

**Implementation:**
```go
// In Go (server.go)
package main

import (
    "encoding/json"
    "fmt"
    "net/http"
    "net/http/httputil"
    "net/url"
)

func main() {
    // Proxy requests to KORE API
    koreURL, _ := url.Parse("http://localhost:8000")
    proxy := httputil.NewSingleHostReverseProxy(koreURL)
    
    http.HandleFunc("/api/kore/", func(w http.ResponseWriter, r *http.Request) {
        proxy.ServeHTTP(w, r)
    })
    
    http.HandleFunc("/api/data", handleDataRequest)
    
    fmt.Println("Go service on :9000")
    http.ListenAndServe(":9000", nil)
}

func handleDataRequest(w http.ResponseWriter, r *http.Request) {
    // Get data from KORE
    resp, _ := http.Get("http://localhost:8000/api/kore/metadata")
    defer resp.Body.Close()
    
    w.Header().Set("Content-Type", "application/json")
    json.NewEncoder(w).Encode(map[string]string{
        "status": "success",
        "message": "Connected to KORE",
    })
}
```

---

### 3. Rust ↔ Java/Kotlin (Enterprise)

**Use Case:** Java applications integrate with KORE engine

**Pattern:**
```
Java/Kotlin App
    ↓
[Load native KORE library]
    ↓
Rust (KORE Core)
    ↓
[FFI - Foreign Function Interface]
    ↓
Java/Kotlin
    ↓
[Process results]
```

**Implementation:**
```kotlin
// In Kotlin (KoreClient.kt)
class KoreClient {
    external fun processKoreFile(path: String): String
    external fun queryKoreData(query: String): ByteArray
    
    companion object {
        init {
            System.loadLibrary("kore_ffi")
        }
    }
}

fun main() {
    val client = KoreClient()
    val result = client.processKoreFile("data.kore")
    println("KORE Result: $result")
}
```

```rust
// In Rust (lib.rs) - FFI bridge
#[no_mangle]
pub extern "C" fn process_kore_file(path: *const c_char) -> *const c_char {
    let c_str: &CStr = unsafe { CStr::from_ptr(path) };
    let filename = c_str.to_string_lossy().into_owned();
    
    let result = KoreProcessor::new(&filename).process();
    
    CString::new(result).unwrap().into_raw()
}
```

---

### 4. Rust ↔ JavaScript/TypeScript (Web)

**Use Case:** Web dashboard for KORE data

**Pattern:**
```
KORE (Rust) [WebAssembly]
    ↓
Node.js / Browser
    ↓
React/Vue Component
    ↓
Display KORE Data
```

**Implementation:**
```typescript
// In TypeScript (web.ts)
import init, { KoreProcessor } from 'kore-wasm';

async function initKore() {
    await init();
    
    const processor = new KoreProcessor("data.kore");
    const metadata = processor.getMetadata();
    
    console.log("KORE File:", metadata.filename);
    console.log("Version:", metadata.version);
    
    return metadata;
}

// React Component
function KoreViewer() {
    const [metadata, setMetadata] = React.useState(null);
    
    React.useEffect(() => {
        initKore().then(setMetadata);
    }, []);
    
    return metadata ? (
        <div>
            <h1>KORE File: {metadata.filename}</h1>
            <p>Version: {metadata.version}</p>
        </div>
    ) : <p>Loading...</p>;
}
```

---

### 5. SQL ↔ All Languages

**Use Case:** Database stores KORE metadata, all languages query it

**Pattern:**
```
All Languages
    ↓
SQL Database (PostgreSQL/MySQL)
    ↓
KORE Metadata Tables
    ↓
Shared data source
```

**SQL Schema for Multi-Language Access:**
```sql
-- All languages can read/write this
CREATE TABLE kore_files (
    id SERIAL PRIMARY KEY,
    filename VARCHAR(255) NOT NULL,
    version VARCHAR(10),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE kore_operations (
    id SERIAL PRIMARY KEY,
    file_id INTEGER REFERENCES kore_files(id),
    operation VARCHAR(50),  -- 'read', 'write', 'analyze'
    language VARCHAR(50),   -- 'rust', 'python', 'go', etc.
    status VARCHAR(50),
    result_json JSONB,
    executed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

**Access from Different Languages:**

Python:
```python
import psycopg2
conn = psycopg2.connect("dbname=kore user=postgres")
cur = conn.cursor()
cur.execute("SELECT * FROM kore_files")
```

Go:
```go
import "database/sql"
_ "github.com/lib/pq"
db, _ := sql.Open("postgres", "postgres://...")
rows, _ := db.Query("SELECT * FROM kore_files")
```

Java:
```java
import java.sql.*;
Connection conn = DriverManager.getConnection("jdbc:postgresql://...");
Statement stmt = conn.createStatement();
ResultSet rs = stmt.executeQuery("SELECT * FROM kore_files");
```

---

## Use Case Patterns

### Pattern 1: ETL Pipeline (Extract-Transform-Load)

```
Python (Extract)
    ↓
[Read from various sources]
    ↓
Rust/KORE (Transform)
    ↓
[Compress, optimize, validate]
    ↓
SQL Database (Load)
    ↓
[Store metadata]
    ↓
Go API (Expose)
    ↓
[Serve to clients]
```

**Implementation Example:**
```python
# Python: Extract
def extract_data():
    return pd.read_csv("raw_data.csv")

# (Call Rust)
# rust_kore.process_data(df)

# (Query SQL)
# SELECT * FROM kore_files WHERE status='processed'

# Go: Expose via API
# GET /api/data returns processed results
```

### Pattern 2: Real-Time Analytics

```
JavaScript/TypeScript (Collect)
    ↓
[Browser/Node.js collects events]
    ↓
Go Microservice (Aggregate)
    ↓
[HTTP server receives data]
    ↓
Rust/KORE (Process)
    ↓
[Compress and store]
    ↓
Python (Analyze)
    ↓
[ML models, stats]
    ↓
SQL (Store Results)
    ↓
JavaScript (Display)
    ↓
[Dashboard visualization]
```

### Pattern 3: Enterprise Integration

```
C# / .NET (Windows System)
    ↓
[Enterprise app on Windows]
    ↓
Java/Kotlin (Middleware)
    ↓
[Process business logic]
    ↓
Rust/KORE (Data Engine)
    ↓
[High-performance storage]
    ↓
SQL (Audit logs, metadata)
    ↓
Go (API Layer)
    ↓
[Expose to other systems]
```

---

## Data Flow

### Complete ETL Example

```
SOURCE DATA
    ↓
┌───────────────────────────────────────────┐
│ Python (SETUP_PYTHON.md)                  │
│ - Load CSV, validate, clean              │
│ - Export to KORE format                  │
└───────────────────┬───────────────────────┘
                    ↓
         data.kore (KORE v2 format)
                    ↓
┌───────────────────────────────────────────┐
│ Rust (SETUP_RUST.md)                      │
│ - KORE Engine                            │
│ - Schema Evolution, ACID, Query Opt      │
│ - Compression, Encryption                │
└───────────────────┬───────────────────────┘
                    ↓
        ┌───────────┴───────────┐
        ↓                       ↓
    metadata              compressed_data
        ↓                       ↓
┌──────────────────────────────────────────┐
│ SQL (SETUP_SQL.md)                       │
│ - Store KORE metadata                    │
│ - Track operations                       │
│ - Audit logs                             │
└──────────────────┬───────────────────────┘
                   ↓
    ┌──────────────┴────────────────┐
    ↓                               ↓
┌─────────────┐             ┌─────────────┐
│ Python      │             │ Go API      │
│ Analytics   │             │ Microservice│
│ (SETUP_PYTHON)            (SETUP_GO)   │
└─────────────┘             └─────────────┘
    ↓                               ↓
Results/Insights              REST Endpoint
                                    ↓
                         ┌─────────────────┐
                         │ JavaScript/TS   │
                         │ Web Dashboard   │
                         │(SETUP_JAVASCRIPT)
                         └─────────────────┘
                                 ↓
                         User Visualization
```

---

## Best Practices

### DO:

✅ **Separate Concerns**
- Rust for performance-critical code
- Python for data analysis
- Go for APIs and services
- JavaScript for UI
- Java for enterprise logic

✅ **Use Appropriate Tools**
- SQL for structured metadata
- KORE for column-oriented storage
- Go for networking
- Python for quick prototyping

✅ **Define Clear Interfaces**
- REST APIs between services
- Message queues for async
- File formats for data exchange
- Schemas for validation

✅ **Monitor Integration Points**
- Log all service calls
- Track data transformations
- Monitor performance bottlenecks
- Alert on failures

### DON'T:

❌ **Tight Coupling**
- Don't hardcode service URLs
- Don't assume data formats
- Don't skip validation
- Don't ignore error handling

❌ **Performance Pitfalls**
- Don't serialize unnecessarily
- Don't make blocking calls
- Don't duplicate data
- Don't ignore caching

❌ **Operational Issues**
- Don't skip logging
- Don't ignore versioning
- Don't skip testing integration
- Don't assume network availability

---

## Configuration Management

Create a central config file for multi-language integration:

**config.yaml:**
```yaml
kore:
  version: "1.3.3"
  data_dir: "/data/kore"
  
services:
  rust_engine:
    host: "localhost"
    port: 8000
    timeout: 30s
    
  go_api:
    host: "localhost"
    port: 9000
    timeout: 10s
    
  python_analyzer:
    host: "localhost"
    port: 5000
    timeout: 60s
    
database:
  postgres:
    host: "localhost"
    port: 5432
    database: "kore_metadata"
    user: "kore_user"
    
logging:
  level: "INFO"
  format: "json"
  output: "stdout"
```

**Usage in Different Languages:**

Python:
```python
import yaml
with open('config.yaml') as f:
    config = yaml.safe_load(f)
```

Go:
```go
import "gopkg.in/yaml.v2"
var config interface{}
yaml.Unmarshal(data, &config)
```

JavaScript:
```javascript
import yaml from 'js-yaml';
const config = yaml.load(fs.readFileSync('config.yaml'));
```

---

## Testing Multi-Language Integration

### Integration Test Example

```python
# test_integration.py
import requests
import json

def test_full_pipeline():
    # 1. Submit data via Python
    data = prepare_test_data()
    
    # 2. Process with Rust/KORE
    response = requests.post("http://localhost:8000/api/process", json=data)
    assert response.status_code == 200
    
    # 3. Verify in database
    db_result = query_database("SELECT * FROM kore_operations")
    assert len(db_result) > 0
    
    # 4. Get results from Go API
    api_response = requests.get("http://localhost:9000/api/results")
    assert api_response.json()['status'] == 'success'
    
    # 5. Verify with JavaScript client
    js_test = requests.get("http://localhost:3000/test")
    assert js_test.json()['passed'] == True
```

---

## Deployment Topology

```
PRODUCTION KORE v1.3.3
│
├── Rust Services (Primary)
│   ├── KORE Engine (Port 8000)
│   └── File Manager (Port 8001)
│
├── Go Services (APIs)
│   ├── REST API Gateway (Port 9000)
│   └── gRPC Service (Port 9001)
│
├── Python Services (Analytics)
│   ├── Data Analyzer (Port 5000)
│   └── ML Pipeline (Port 5001)
│
├── Java/Kotlin Services (Enterprise)
│   ├── Business Logic (Port 8080)
│   └── Report Generator (Port 8081)
│
├── JavaScript/TypeScript (Web)
│   ├── Frontend (Port 3000)
│   └── Admin Console (Port 3001)
│
├── Databases
│   ├── PostgreSQL (Port 5432) - Metadata
│   └── Redis (Port 6379) - Cache
│
└── Monitoring
    ├── Prometheus (Port 9090)
    └── Grafana (Port 3100)
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Language integration guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Related Documentation:**
- [SETUP_PYTHON.md](SETUP_PYTHON.md)
- [SETUP_RUST.md](SETUP_RUST.md)
- [SETUP_GO.md](SETUP_GO.md)
- [SETUP_JAVA.md](SETUP_JAVA.md)
- [SETUP_JAVASCRIPT_TYPESCRIPT.md](SETUP_JAVASCRIPT_TYPESCRIPT.md)
- [SETUP_SQL.md](SETUP_SQL.md)
