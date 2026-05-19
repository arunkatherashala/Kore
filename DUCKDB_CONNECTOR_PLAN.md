# 🦆 DuckDB Connector Implementation Plan
**Status:** IN PROGRESS  
**Start Date:** May 19, 2026  
**Target Completion:** June 15, 2026 (4 weeks)  
**Effort:** ~60-80 engineering hours (3 weeks active)

---

## 📋 Executive Summary

Building a **DuckDB Appender Protocol** connector to make Kore the default analytical format for the Python data ecosystem.

**Success Metric:** `SELECT * FROM 'file.kore'` just works in DuckDB.

---

## 🎯 Phase Overview

### Phase 1: Foundation (Week 1 - May 20-26)
- [ ] Arrow C Data Interface implementation
- [ ] ArrowRecordBatch serialization/deserialization
- [ ] Basic read protocol

### Phase 2: DuckDB Integration (Week 2-3 - May 27-Jun 9)
- [ ] DuckDB appender registration
- [ ] Full round-trip read/write
- [ ] Schema mapping (Kore → Arrow → DuckDB)

### Phase 3: Optimization (Week 4 - Jun 10-15)
- [ ] Column pruning (predicate pushdown)
- [ ] Vectorized scans
- [ ] Benchmark validation

---

## 📦 Architecture

```
User Code
  ↓
DuckDB SQL
  ↓
KoreDuckDBConnector (Rust)
  ├─ Schema Handler
  ├─ Arrow Conversion Layer
  ├─ Appender Protocol
  └─ Codec Integration
  ↓
Kore File Format (.kore)
```

---

## 🔧 Implementation Steps

### Step 1: Arrow Conversion Layer
**File:** `src/arrow_converter.rs` (300-400 lines)

```rust
pub struct ArrowConverter;

impl ArrowConverter {
    /// Convert Kore columns to Arrow RecordBatch
    pub fn to_arrow_batch(
        columns: Vec<KoreColumn>,
    ) -> Result<ArrowRecordBatch>;
    
    /// Convert Arrow RecordBatch back to Kore columns
    pub fn from_arrow_batch(
        batch: &ArrowRecordBatch,
    ) -> Result<Vec<KoreColumn>>;
    
    /// Map Kore data types to Arrow data types
    pub fn kore_type_to_arrow(kore_type: &KoreType) -> Result<ArrowDataType>;
}
```

### Step 2: DuckDB Appender
**File:** `src/duckdb_connector.rs` (400-500 lines)

```rust
pub struct KoreDuckDBConnector {
    file_path: PathBuf,
    schema: ArrowSchema,
}

impl KoreDuckDBConnector {
    /// Create a new connector pointing to a .kore file
    pub fn new(file_path: &str) -> Result<Self>;
    
    /// Read data as Arrow RecordBatch (for DuckDB to consume)
    pub fn read_as_arrow(&self) -> Result<Vec<ArrowRecordBatch>>;
    
    /// Write data from Arrow RecordBatch (for DuckDB to provide)
    pub fn append_from_arrow(
        &mut self,
        batch: ArrowRecordBatch,
    ) -> Result<()>;
    
    /// Get schema in Arrow format
    pub fn arrow_schema(&self) -> &ArrowSchema;
}
```

### Step 3: DuckDB Registration (FFI Layer)
**File:** `src/duckdb_ffi.rs` (200-300 lines)

```rust
#[no_mangle]
pub extern "C" fn kore_duckdb_init(db: *mut DuckDBDatabase) -> DuckDBError {
    // Register the appender protocol with DuckDB
    // Enable: SELECT * FROM 'file.kore'
}

#[no_mangle]
pub extern "C" fn kore_duckdb_scan(
    context: *mut DuckDBTableFunctionContext,
) -> DuckDBError {
    // Implement the scan callback
}
```

### Step 4: Integration Tests
**File:** `tests/duckdb_integration_test.rs` (300+ lines)

```rust
#[test]
fn test_duckdb_read_kore_file() {
    // 1. Create test Kore file
    // 2. Load in DuckDB
    // 3. SELECT * and verify results
}

#[test]
fn test_duckdb_write_to_kore() {
    // 1. Create data in DuckDB
    // 2. Write to Kore format
    // 3. Verify file is valid
}

#[test]
fn test_schema_mapping() {
    // Test type conversions: Kore ↔ Arrow ↔ DuckDB
}
```

---

## 📊 Weekly Breakdown

### Week 1 (May 20-26): Foundation
| Day | Task | Output |
|-----|------|--------|
| Mon-Tue | Arrow C Data Interface | arrow_converter.rs complete |
| Wed-Thu | Type mapping tests | 15+ tests passing |
| Fri | Documentation | Architecture doc, code comments |

**Deliverable:** Core conversion layer, 20 tests

---

### Week 2 (May 27-Jun 2): DuckDB Binding
| Day | Task | Output |
|-----|------|--------|
| Mon-Tue | DuckDB connector struct | kore_duckdb.rs skeleton |
| Wed-Thu | Read protocol | `SELECT * FROM kore` works |
| Fri | Write protocol | Append mode working |

**Deliverable:** Basic read/write, 15 tests, example: `duckdb_simple_test.rs`

---

### Week 3 (Jun 3-9): Full Integration
| Day | Task | Output |
|-----|------|--------|
| Mon-Tue | Schema inference | Auto-detect columns from Kore |
| Wed-Thu | Multi-batch handling | Large files (100MB+) |
| Fri | Round-trip testing | Write → Read cycle validation |

**Deliverable:** Production-ready connector, 25 tests

---

### Week 4 (Jun 10-15): Optimization & Benchmarks
| Day | Task | Output |
|-----|------|--------|
| Mon-Tue | Column pruning | Predicate pushdown (filter pushes to Kore) |
| Wed-Thu | Vectorized scans | Batch size optimization |
| Fri | Benchmarking | Official performance report |

**Deliverable:** Optimization docs, benchmark results, blog post draft

---

## 🧪 Testing Strategy

### Unit Tests (30+)
- Type conversion (Kore ↔ Arrow ↔ DuckDB)
- Schema inference
- Batch serialization

### Integration Tests (20+)
- DuckDB SELECT queries
- Write operations
- Multi-column scenarios
- Large file handling (500MB+)

### Benchmark Tests (5+)
- Read performance (MB/s)
- Write performance (MB/s)
- Predicate pushdown effectiveness
- Comparison: Kore vs Parquet vs CSV

---

## 📈 Success Criteria

| Criterion | Target | Status |
|-----------|--------|--------|
| **Feature Complete** | Read + Write | ⏳ |
| **Test Coverage** | 50+ tests, 100% pass | ⏳ |
| **Performance** | > 500 MB/s read | ⏳ |
| **Documentation** | API docs + examples | ⏳ |
| **Validation** | Works with real datasets | ⏳ |

---

## 🚀 Phase 2 Setup (After DuckDB)

Once DuckDB connector is complete:

1. **Arrow Interop Layer** (2 weeks)
   - Direct Arrow C Data Interface
   - Enables: Pandas, Polars, DuckDB, R integration

2. **Predicate Pushdown** (2-3 weeks)
   - Filter optimization
   - Column pruning
   - Expected 10-100x speedup for analytical queries

3. **Spark DataSource** (3 weeks)
   - Scala implementation
   - Native Spark integration
   - `spark.read.format("kore").load("file.kore")`

---

## 📚 Resources Needed

### Dependencies to Add
```toml
[dependencies]
arrow = "53.0"  # Arrow C Data Interface
duckdb = "0.10.0"  # DuckDB Rust bindings
arrow-array = "53.0"
arrow-schema = "53.0"
```

### External Tools
- DuckDB CLI (for manual testing)
- pyarrow (for validation)
- dbt (optional, for SQL testing)

---

## ⚠️ Risks & Mitigations

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Arrow/DuckDB version mismatch | High | Pin versions, use LTS releases |
| Type mapping gaps | Medium | Comprehensive test coverage |
| Performance regression | Medium | Benchmark before/after each change |
| Build complexity | Medium | Modular design, feature flags |

---

## 📝 Deliverables by Week

**Week 1:** `arrow_converter.rs` + 20 unit tests  
**Week 2:** `duckdb_connector.rs` + 15 integration tests  
**Week 3:** Full round-trip + 25 tests  
**Week 4:** Optimizations + benchmarks + blog post  

**Final Package:** Production-ready DuckDB connector ready for v1.2.0 release

---

## 🎯 Success Looks Like

```bash
# Install DuckDB + Kore
$ pip install duckdb kore-fileformat

# Python code
import duckdb
db = duckdb.sql("SELECT * FROM 'data.kore' WHERE year > 2020")

# Result: Works instantly, 50-100x faster than CSV
```

---

**Ready? Let's ship it! 🚀**
