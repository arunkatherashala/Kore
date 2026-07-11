# KORE Engine — Complete User Guide

> **The fastest embeddable columnar engine in Rust.**  
> Beats Spark 17/17. Beats DuckDB on most queries. Single binary. Zero JVM.

---

## Table of Contents

1. [Quick Start — First Query in 30 Seconds](#1-quick-start)
2. [Loading Data](#2-loading-data)
3. [SELECT Queries](#3-select-queries)
4. [Aggregation Functions](#4-aggregation-functions)
5. [Window Functions](#5-window-functions)
6. [Date & Time Functions](#6-date--time-functions)
7. [String Functions](#7-string-functions)
8. [Math Functions](#8-math-functions)
9. [JOINs](#9-joins)
10. [Subqueries & CTEs](#10-subqueries--ctes)
11. [DML — INSERT / UPDATE / DELETE / MERGE](#11-dml)
12. [Meta Queries — SHOW / DESCRIBE / EXPLAIN](#12-meta-queries)
13. [kore-self — AI Twin & MCP Tools](#13-kore-self--mcp-tools)
14. [Distributed Queries](#14-distributed-queries)
15. [Python Integration](#15-python-integration)
16. [Rust Embedding](#16-rust-embedding)
17. [Benchmarking](#17-benchmarking)
18. [FAQ & Troubleshooting](#18-faq)

---

## 1. Quick Start

### Build
```bash
git clone https://github.com/arunkatherashala/Kore
cd Kore
cargo build --release
```

### Your First Query
```bash
# Run TPC-H benchmark (generates 7.8M rows, runs 17 queries)
cargo run --release -p kore-tpch
```

### Run kore-self (AI Twin + MCP server)
```bash
cargo run --release -p kore-self -- arun
```

### SQL in 30 seconds (via Python)
```python
import subprocess, json

KORE = "./target/release/kore-self.exe"  # or kore-self on Linux/Mac

def kore_query(sql):
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize",
                       "params":{"protocolVersion":"2024-11-05","capabilities":{},
                                 "clientInfo":{"name":"demo","version":"1"}}})
    msg  = json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call",
                       "params":{"name":"self_query","arguments":{"sql":sql}}})
    p = subprocess.run([KORE,"arun"], input=(init+"\n"+msg+"\n").encode(),
                       capture_output=True, timeout=15)
    for line in p.stdout.decode().split("\n"):
        try:
            r = json.loads(line)
            if r.get("id") == 2:
                return r["result"]["content"][0]["text"]
        except: pass

# No need to load any table — works out of the box
print(kore_query("SELECT 1 + 1 AS result"))
print(kore_query("SELECT NOW() AS today"))
```

---

## 2. Loading Data

### COPY FROM (recommended — fastest path)
```sql
-- CSV (auto-detects header, comma delimiter)
COPY lineitem FROM '/path/to/tpch_lineitem.csv';

-- TSV
COPY logs FROM '/path/to/logs.tsv' WITH (DELIMITER '\t');

-- CSV without header
COPY raw FROM '/path/to/data.csv' WITH (HEADER FALSE);

-- Parquet
COPY orders FROM '/path/to/orders.parquet';

-- Native .kore binary (fastest re-load)
COPY snapshot FROM '/path/to/data.kore';
```

### LOAD TABLE (alias of COPY)
```sql
LOAD TABLE sales FROM 'sales.parquet';
LOAD TABLE customers FROM 'customers.csv';
```

### CREATE TABLE AS SELECT
```sql
CREATE TABLE summary AS
  SELECT l_returnflag, SUM(l_quantity) AS total_qty
  FROM lineitem
  GROUP BY l_returnflag;
```

### INSERT INTO
```sql
-- From SELECT
INSERT INTO archive SELECT * FROM orders WHERE order_year < 2020;

-- From VALUES
INSERT INTO lookup VALUES (1, 'A', 'Active'), (2, 'B', 'Inactive');
```

### Inline VALUES table
```sql
-- Use VALUES directly in FROM
SELECT *
FROM (VALUES (1, 'Alice', 95.0), (2, 'Bob', 87.5), (3, 'Carol', 92.0))
         AS students(id, name, score)
ORDER BY score DESC;
```

---

## 3. SELECT Queries

### Basic SELECT
```sql
-- All rows
SELECT * FROM customers;

-- Specific columns
SELECT customer_id, name, email FROM customers;

-- No table (expressions, constants, functions)
SELECT 1 + 1 AS two;
SELECT NOW() AS today;
SELECT UPPER('hello kore') AS greeting;
SELECT PI() AS pi, SQRT(2) AS sqrt2;
```

### WHERE Clause
```sql
SELECT * FROM orders WHERE total > 1000;
SELECT * FROM orders WHERE status = 'shipped' AND total > 500;
SELECT * FROM orders WHERE status IN ('shipped', 'delivered');
SELECT * FROM orders WHERE name LIKE 'A%';
SELECT * FROM orders WHERE amount BETWEEN 100 AND 500;
SELECT * FROM orders WHERE notes IS NULL;
SELECT * FROM orders WHERE notes IS NOT NULL;
```

### ORDER BY
```sql
-- Standard sort
SELECT * FROM orders ORDER BY total DESC;

-- Multi-column sort
SELECT * FROM orders ORDER BY status ASC, total DESC;

-- NULLS FIRST / NULLS LAST (standard SQL)
SELECT * FROM orders ORDER BY delivered_at DESC NULLS LAST;
SELECT * FROM orders ORDER BY priority ASC NULLS FIRST;
```

### LIMIT + OFFSET (pagination)
```sql
-- First 10 rows
SELECT * FROM orders ORDER BY id LIMIT 10;

-- Page 3 (rows 21–30)
SELECT * FROM orders ORDER BY id LIMIT 10 OFFSET 20;

-- Standard SQL syntax
SELECT * FROM orders ORDER BY id FETCH FIRST 10 ROWS ONLY;
SELECT * FROM orders ORDER BY id FETCH FIRST 10 ROWS ONLY OFFSET 20;
```

### SELECT DISTINCT
```sql
SELECT DISTINCT status FROM orders;
SELECT DISTINCT country, region FROM customers;
```

### CASE WHEN
```sql
SELECT order_id,
  CASE
    WHEN total > 10000 THEN 'premium'
    WHEN total > 1000  THEN 'standard'
    ELSE 'small'
  END AS tier
FROM orders;
```

### QUALIFY (filter on window function results)
```sql
-- Keep only the most recent order per customer
SELECT customer_id, order_id, order_date
FROM orders
QUALIFY ROW_NUMBER() OVER (PARTITION BY customer_id ORDER BY order_date DESC) = 1;

-- Keep top-3 products by revenue per category
SELECT category, product, revenue
FROM sales
QUALIFY RANK() OVER (PARTITION BY category ORDER BY revenue DESC) <= 3;
```

---

## 4. Aggregation Functions

### Basic Aggregates
```sql
SELECT
  COUNT(*)                AS total_rows,
  COUNT(DISTINCT user_id) AS unique_users,
  SUM(amount)             AS total_amount,
  AVG(amount)             AS avg_amount,
  MIN(amount)             AS min_amount,
  MAX(amount)             AS max_amount
FROM transactions;
```

### GROUP BY + HAVING
```sql
SELECT
  category,
  COUNT(*) AS cnt,
  SUM(revenue) AS total
FROM sales
GROUP BY category
HAVING SUM(revenue) > 10000
ORDER BY total DESC;
```

### GROUP BY ROLLUP (subtotals + grand total)
```sql
SELECT
  country,
  city,
  SUM(revenue) AS revenue
FROM sales
GROUP BY ROLLUP(country, city);
-- Produces: (country, city), (country, NULL), (NULL, NULL)
```

### GROUP BY CUBE (all combinations)
```sql
SELECT region, product, SUM(sales)
FROM data
GROUP BY CUBE(region, product);
```

### Statistical Aggregates
```sql
SELECT
  department,
  AVG(salary)               AS avg_salary,
  STDDEV(salary)            AS salary_stddev,
  VARIANCE(salary)          AS salary_variance,
  MEDIAN(salary)            AS salary_median,
  PERCENTILE_CONT(0.25) WITHIN GROUP (ORDER BY salary) AS p25,
  PERCENTILE_CONT(0.75) WITHIN GROUP (ORDER BY salary) AS p75,
  PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY salary) AS p95
FROM employees
GROUP BY department;
```

### String Aggregation
```sql
-- Concatenate values in a group
SELECT
  department,
  STRING_AGG(name, ', ') AS employee_names
FROM employees
GROUP BY department;

-- GROUP_CONCAT (MySQL-style)
SELECT category, GROUP_CONCAT(product) AS products
FROM inventory
GROUP BY category;
```

### ARRAY_AGG
```sql
SELECT department, ARRAY_AGG(salary) AS all_salaries
FROM employees
GROUP BY department;
```

---

## 5. Window Functions

### ROW_NUMBER, RANK, DENSE_RANK
```sql
SELECT
  name,
  department,
  salary,
  ROW_NUMBER() OVER (PARTITION BY department ORDER BY salary DESC) AS rn,
  RANK()        OVER (PARTITION BY department ORDER BY salary DESC) AS rank,
  DENSE_RANK()  OVER (PARTITION BY department ORDER BY salary DESC) AS dense_rank
FROM employees;
```

### PERCENT_RANK, CUME_DIST
```sql
SELECT
  name,
  salary,
  PERCENT_RANK() OVER (ORDER BY salary) AS pct_rank,  -- 0.0 to 1.0
  CUME_DIST()    OVER (ORDER BY salary) AS cume_dist  -- fraction of rows ≤ current
FROM employees;
```

### LAG, LEAD (access adjacent rows)
```sql
SELECT
  order_date,
  revenue,
  LAG(revenue,  1) OVER (ORDER BY order_date) AS prev_day_revenue,
  LEAD(revenue, 1) OVER (ORDER BY order_date) AS next_day_revenue,
  revenue - LAG(revenue, 1) OVER (ORDER BY order_date) AS day_over_day_change
FROM daily_sales;
```

### FIRST_VALUE, LAST_VALUE
```sql
SELECT
  name,
  salary,
  FIRST_VALUE(salary) OVER (PARTITION BY department ORDER BY salary DESC) AS max_in_dept,
  LAST_VALUE(salary)  OVER (PARTITION BY department ORDER BY salary)       AS min_in_dept
FROM employees;
```

### Running Aggregates (ROWS/RANGE frames)
```sql
SELECT
  order_date,
  amount,
  SUM(amount) OVER (
    ORDER BY order_date
    ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
  ) AS running_total,
  AVG(amount) OVER (
    ORDER BY order_date
    ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
  ) AS rolling_7day_avg
FROM daily_orders;
```

### NTILE (bucketing)
```sql
SELECT name, salary,
  NTILE(4) OVER (ORDER BY salary) AS quartile
FROM employees;
-- 1=bottom 25%, 4=top 25%
```

---

## 6. Date & Time Functions

### Extraction
```sql
SELECT
  order_date,
  YEAR(order_date)             AS yr,
  MONTH(order_date)            AS mo,
  DAY(order_date)              AS dy,
  QUARTER(order_date)          AS qtr,
  EXTRACT(year  FROM order_date) AS year_std,
  EXTRACT(month FROM order_date) AS month_std
FROM orders;
```

### Truncation
```sql
SELECT
  DATE_TRUNC('year',    order_date) AS year_start,
  DATE_TRUNC('quarter', order_date) AS quarter_start,
  DATE_TRUNC('month',   order_date) AS month_start
FROM orders;
```

### Arithmetic
```sql
SELECT
  ship_date,
  DATEADD('day',   7,  ship_date) AS due_date,
  DATEADD('month', 1,  ship_date) AS next_month,
  DATEDIFF('day',  order_date, ship_date) AS days_to_ship,
  NOW()                           AS current_timestamp
FROM shipments;
```

### Formatting
```sql
SELECT STRFTIME('%Y-%m', order_date) AS year_month FROM orders;
SELECT STRFTIME('%d/%m/%Y', order_date) AS formatted FROM orders;
```

---

## 7. String Functions

```sql
SELECT
  UPPER(name)                           AS upper_name,
  LOWER(email)                          AS lower_email,
  TRIM(notes)                           AS clean_notes,
  LTRIM(code)                           AS ltrim_code,
  RTRIM(code)                           AS rtrim_code,
  LENGTH(description)                   AS desc_len,
  LEFT(name, 3)                         AS name_prefix,
  RIGHT(postal_code, 3)                 AS pc_suffix,
  SUBSTRING(phone, 1, 3)                AS area_code,
  REPLACE(email, '@old.com', '@new.com') AS new_email,
  CONCAT(first_name, ' ', last_name)    AS full_name,
  REVERSE(code)                         AS rev_code,
  REPEAT('*', 5)                        AS stars,
  LPAD(id, 6, '0')                      AS padded_id,
  RPAD(name, 20, '.')                   AS padded_name,
  INITCAP(city)                         AS proper_city,
  CHARINDEX('@', email)                 AS at_position,
  SPLIT_PART(email, '@', 2)             AS email_domain,
  ASCII(LEFT(name,1))                   AS first_char_code,
  CHR(65)                               AS letter_A,
  SPACE(3)                              AS three_spaces
FROM contacts;
```

---

## 8. Math Functions

```sql
SELECT
  ABS(-42)            AS absolute,
  ROUND(3.14159, 2)   AS rounded,
  FLOOR(3.7)          AS floored,
  CEIL(3.2)           AS ceiling,
  TRUNCATE(3.987, 1)  AS truncated,   -- 3.9
  SQRT(16)            AS sq_root,
  POWER(2, 10)        AS two_to_ten,
  MOD(17, 5)          AS remainder,
  SIGN(-3.14)         AS sign_val,    -- -1
  LOG(2.718)          AS natural_log,
  LOG10(100)          AS log_base10,
  LOG2(1024)          AS log_base2,
  EXP(1)              AS e,
  PI()                AS pi_val,
  SIN(PI()/2)         AS sine_90,
  COS(0)              AS cosine_0,
  DEGREES(PI())       AS pi_in_deg,   -- 180
  RADIANS(180)        AS deg_in_rad,
  CBRT(27)            AS cube_root,
  GREATEST(1, 5, 3)   AS greatest,
  LEAST(1, 5, 3)      AS least,
  RAND()              AS random_val
FROM dual;
```

---

## 9. JOINs

### INNER JOIN
```sql
SELECT o.order_id, c.name, o.total
FROM orders o
INNER JOIN customers c ON o.customer_id = c.id
WHERE o.total > 1000;
```

### LEFT JOIN
```sql
-- All orders, with customer name (NULL if no match)
SELECT o.order_id, c.name, o.total
FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id;
```

### Multiple JOINs (up to 6 tables)
```sql
SELECT
  o.order_id,
  c.name     AS customer,
  p.name     AS product,
  s.name     AS supplier,
  n.n_name   AS nation,
  r.r_name   AS region
FROM orders o
JOIN customers  c ON o.custkey    = c.custkey
JOIN lineitem   l ON o.orderkey   = l.orderkey
JOIN part       p ON l.partkey    = p.partkey
JOIN supplier   s ON l.suppkey    = s.suppkey
JOIN nation     n ON s.nationkey  = n.nationkey
JOIN region     r ON n.regionkey  = r.regionkey
LIMIT 10;
```

### FULL OUTER JOIN
```sql
SELECT COALESCE(a.id, b.id) AS id, a.val AS left_val, b.val AS right_val
FROM table_a a
FULL OUTER JOIN table_b b ON a.id = b.id;
```

---

## 10. Subqueries & CTEs

### Scalar Subquery
```sql
SELECT name, salary,
  salary - (SELECT AVG(salary) FROM employees) AS vs_avg
FROM employees;
```

### IN Subquery
```sql
SELECT * FROM orders
WHERE customer_id IN (
  SELECT id FROM customers WHERE country = 'USA'
);
```

### EXISTS
```sql
SELECT * FROM customers c
WHERE EXISTS (
  SELECT 1 FROM orders o
  WHERE o.customer_id = c.id AND o.total > 5000
);
```

### Correlated Subquery (auto-decorrelated — O(n) not O(n²))
```sql
-- Orders above average for their category
SELECT product, category, price
FROM products p
WHERE price > (
  SELECT AVG(price) FROM products p2 WHERE p2.category = p.category
);
```

### FROM Subquery
```sql
SELECT dept, avg_sal
FROM (
  SELECT department AS dept, AVG(salary) AS avg_sal
  FROM employees
  GROUP BY department
) dept_avgs
WHERE avg_sal > 80000;
```

### CTE (WITH clause)
```sql
WITH high_value AS (
  SELECT customer_id, SUM(total) AS lifetime_value
  FROM orders
  GROUP BY customer_id
  HAVING SUM(total) > 10000
),
ranked AS (
  SELECT *, RANK() OVER (ORDER BY lifetime_value DESC) AS rank
  FROM high_value
)
SELECT * FROM ranked WHERE rank <= 10;
```

### UNION / INTERSECT / EXCEPT
```sql
-- All customers who ordered this year OR last year
SELECT customer_id FROM orders WHERE YEAR(order_date) = 2026
UNION
SELECT customer_id FROM orders WHERE YEAR(order_date) = 2025;

-- Customers who ordered BOTH this year AND last year
SELECT customer_id FROM orders WHERE YEAR(order_date) = 2026
INTERSECT
SELECT customer_id FROM orders WHERE YEAR(order_date) = 2025;

-- Customers who ordered this year but NOT last year
SELECT customer_id FROM orders WHERE YEAR(order_date) = 2026
EXCEPT
SELECT customer_id FROM orders WHERE YEAR(order_date) = 2025;
```

---

## 11. DML

### UPDATE
```sql
UPDATE orders SET status = 'archived' WHERE order_date < '2020-01-01';
UPDATE products SET price = price * 1.1 WHERE category = 'electronics';
```

### DELETE
```sql
DELETE FROM orders WHERE status = 'cancelled';
DELETE FROM sessions WHERE created_at < '2025-01-01';
```

### MERGE (UPSERT)
```sql
-- Update existing rows, insert new ones
MERGE INTO customers
USING customer_updates src ON customers.id = src.id
WHEN MATCHED     THEN UPDATE SET email = src.email, name = src.name
WHEN NOT MATCHED THEN INSERT VALUES (src.id, src.name, src.email);
```

---

## 12. Meta Queries

```sql
-- List all loaded tables
SHOW TABLES;

-- Show columns and types for a table
DESCRIBE lineitem;
SHOW COLUMNS FROM orders;

-- Explain a query plan
EXPLAIN SELECT COUNT(*) FROM lineitem GROUP BY l_returnflag;
```

**Sample DESCRIBE output:**
```
column_name         data_type   rows
------------------  ----------  -------
l_orderkey          BIGINT      6000000
l_partkey           BIGINT      6000000
l_suppkey           BIGINT      6000000
l_linenumber        BIGINT      6000000
l_quantity          DOUBLE      6000000
l_extendedprice     DOUBLE      6000000
l_shipdate          VARCHAR     6000000
```

---

## 13. kore-self — MCP Tools

Connect KORE to any AI (Claude, GPT, VS Code Copilot) via MCP:

**`mcp.json` / `claude_desktop_config.json`:**
```json
{
  "mcpServers": {
    "kore": {
      "command": "/path/to/kore-self",
      "args": ["arun"]
    }
  }
}
```

### Query Tools
| Tool | Description | Example |
|------|-------------|---------|
| `self_query(sql)` | Run SELECT — tables persist across calls | `SELECT COUNT(*) FROM orders` |
| `self_dml(sql)` | COPY / INSERT / UPDATE / DELETE / MERGE | `COPY orders FROM 'orders.csv'` |

### Introspection Tools
| Tool | Description |
|------|-------------|
| `self_tables()` | List all loaded tables |
| `self_brief()` | Current engine state summary |
| `self_goals()` | KORE's active goals |

### Digital Life Tools
| Tool | Description |
|------|-------------|
| `self_needs()` | KORE's 7 internal needs (learn/create/evolve...) |
| `self_becoming()` | What KORE is becoming |
| `self_temporal()` | Past/present/future self snapshot |
| `self_story()` | KORE's autobiographical narrative |
| `self_heartbeat()` | Trigger lifecycle tick |
| `self_evolve(insight)` | Feed new insight to BecomingEngine |
| `self_species()` | KORE species definition |

### Persistence Tools
| Tool | Description |
|------|-------------|
| `self_save(name, data)` | Persist memory to .kore store |
| `self_load(name)` | Load memories |
| `self_delta_save(path, data)` | ACID-append to Delta table |
| `self_delta_history(path)` | Read Delta changelog |
| `self_push()` | Push state to GitHub |

### AI Conversation Tools
| Tool | Description |
|------|-------------|
| `self_chat(message)` | Conversational interface |
| `self_remind(note)` | Add reminder |

### Distributed Tools
| Tool | Description |
|------|-------------|
| `self_distributed_query(sql, workers)` | Run SQL across worker nodes |
| `self_broadcast(message)` | Broadcast to cluster |
| `self_context_sync()` | Sync context with cluster |

---

## 14. Distributed Queries

KORE has a built-in distributed compute engine that mirrors Apache Spark's architecture — without the JVM.

### Start Workers
```bash
# Terminal 1 — Worker node 1
kore-worker-node --port 9001 --coord 127.0.0.1:7878

# Terminal 2 — Worker node 2
kore-worker-node --port 9002 --coord 127.0.0.1:7878

# Terminal 3 — Worker node 3
kore-worker-node --port 9003 --coord 127.0.0.1:7878
```

### Start Coordinator and Run Query
```bash
kore-coordinator \
  --port 7878 \
  --workers 127.0.0.1:9001,127.0.0.1:9002,127.0.0.1:9003 \
  --sql "SELECT l_returnflag, COUNT(*), SUM(l_extendedprice) FROM lineitem GROUP BY l_returnflag"
```

### How It Works
```
Your SQL
   │
   ▼
Coordinator (port 7878)
   │  partitions DataBlock into N shards (HashPartitioner / RangePartitioner)
   │  sends QueryTask to each worker over TCP
   │
   ├──► Worker :9001  executes SQL on shard  ──► TaskResult
   ├──► Worker :9002  executes SQL on shard  ──► TaskResult
   └──► Worker :9003  executes SQL on shard  ──► TaskResult
            │
            └── two-phase aggregation (local agg → merge) → final DataBlock
```

### Fault Tolerance
- Workers send heartbeats every 5s
- Coordinator retries failed tasks (exponential backoff)
- Lineage DAG tracks computation — lost partitions can be recomputed
- Checkpoint: periodic disk snapshots to truncate lineage chains

---

## 15. Python Integration

### Via kore-self subprocess (works today)
```python
import subprocess, json, time

KORE = "./target/release/kore-self"

class KoreSession:
    """Persistent KORE session — tables survive across queries."""

    def __init__(self):
        self._init = json.dumps({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2024-11-05","capabilities":{},
                      "clientInfo":{"name":"py","version":"1"}}
        })

    def _call(self, tool, args, msg_id=2):
        msg = json.dumps({"jsonrpc":"2.0","id":msg_id,"method":"tools/call",
                          "params":{"name":tool,"arguments":args}})
        p = subprocess.run([KORE,"arun"],
                           input=(self._init+"\n"+msg+"\n").encode(),
                           capture_output=True, timeout=30)
        for line in p.stdout.decode().split("\n"):
            try:
                r = json.loads(line)
                if r.get("id") == msg_id:
                    return r["result"]["content"][0]["text"]
            except: pass
        return ""

    def load(self, table, path):
        return self._call("self_dml", {"sql": f"COPY {table} FROM '{path}'"})

    def query(self, sql):
        return self._call("self_query", {"sql": sql})

    def dml(self, sql):
        return self._call("self_dml", {"sql": sql})


# Example usage
kore = KoreSession()

# Load a CSV
kore.load("sales", "/data/sales.csv")

# Query
print(kore.query("SELECT category, SUM(revenue) total FROM sales GROUP BY category ORDER BY total DESC LIMIT 5"))

# Window function
print(kore.query("""
  SELECT name, revenue,
    RANK() OVER (PARTITION BY category ORDER BY revenue DESC) AS rank
  FROM sales
  QUALIFY rank <= 3
"""))
```

### Via kore-python FFI (native — coming soon)
```python
import kore  # native Python extension via kore-python crate

ctx = kore.Context()
ctx.copy_from("lineitem", "tpch_lineitem.csv")
result = ctx.query("SELECT COUNT(*) FROM lineitem")
print(result.to_pandas())
```

---

## 16. Rust Embedding

Add KORE to your Rust project:

**`Cargo.toml`:**
```toml
[dependencies]
kore-sql  = { path = "path/to/Kore/kore-sql"  }
kore-core = { path = "path/to/Kore/kore-core" }
kore-io   = { path = "path/to/Kore/kore-io"   }
```

**Usage:**
```rust
use kore_sql::executor::KqlContext;
use kore_io::CsvReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = KqlContext::new();

    // Load CSV
    let block = CsvReader::new("orders.csv").read()?;
    ctx.register("orders", block);

    // Run SQL
    let result = ctx.query("
        SELECT status, COUNT(*) AS cnt, SUM(total) AS revenue
        FROM orders
        GROUP BY status
        ORDER BY revenue DESC
    ")?;

    for row in 0..result.num_rows {
        println!("{:?}", result.get_row(row));
    }

    // DML
    ctx.execute_dml("UPDATE orders SET status = 'archived' WHERE total < 10")?;

    // Persist
    ctx.save_to_kore("orders", "orders_snapshot.kore")?;

    Ok(())
}
```

### ACID Delta Tables
```rust
use kore_sql::executor::KqlContext;
use kore_core::DataBlock;

let ctx = KqlContext::new();

// Append (ACID)
let new_data: DataBlock = /* ... */;
let version = ctx.delta_insert("./orders_delta", new_data)?;
println!("Written at version {version}");

// Time travel
let v1 = ctx.read_delta_at_version("./orders_delta", 1)?;
let v5 = ctx.read_delta_at_version("./orders_delta", 5)?;
```

---

## 17. Benchmarking

### Run Full TPC-H Benchmark
```bash
# Build + run (generates synthetic 7.8M rows, 17 queries)
cargo run --release -p kore-tpch

# Output includes query times and speedup vs Spark
```

### Compare vs DuckDB
```bash
python kore_vs_spark.py
# Requires DuckDB installed: pip install duckdb
```

### Validate All Features
```bash
python -X utf8 validate_all.py
# Should output: ALL 40+ VALIDATIONS PASSED
```

### Run Python Benchmark
```bash
python benchmark_kore.py
# Tests: load CSV, GROUP BY, JOIN, window, sort, correlated subquery
```

### Benchmark Results (TPC-H SF-1, 6M rows)

| Query | KORE | Spark | KORE wins |
|-------|------|-------|-----------|
| Q1 GROUP BY | 13 ms | 4,200 ms | **318x** |
| Q7 Multi-join | 10 ms | 14,200 ms | **1,413x** |
| Q8 6-table join | 18 ms | 18,500 ms | **1,046x** |
| Q6 Filter+SUM | 28 ms | 2,800 ms | **100x** |
| S1 Sort 6M rows | 79 ms | 5,100 ms | **65x** |

---

## 18. FAQ

**Q: Can I use KORE without Rust?**  
A: Yes. The `kore-self` binary is a standalone MCP server. Use it from Python, JavaScript, any language via subprocess. Native Python FFI is in `kore-python`.

**Q: Does KORE support multiple simultaneous queries?**  
A: Yes — the distributed cluster (kore-cluster) runs queries in parallel across workers. Single-node KORE uses Rayon for multi-threaded GROUP BY and parallel execution.

**Q: How do I save query results?**  
```sql
-- Save as native .kore (fastest)
COPY result_table TO 'output.kore';

-- Or use CREATE TABLE AS SELECT and then save
CREATE TABLE result AS SELECT ...;
-- Then from Rust: ctx.save_to_kore("result", "result.kore")
```

**Q: What file formats does KORE read?**  
CSV, TSV, Parquet, Native `.kore` binary, Apache ORC (via kore-orc), Apache Iceberg (via kore-iceberg).

**Q: How does KORE compare to DuckDB?**  
Both are embeddable single-node columnar engines. KORE is faster on most queries (63x on Q1), adds a distributed cluster, ACID Delta, and the Living AI Twin (kore-self MCP server). DuckDB has broader SQL dialect coverage and mature client libraries.

**Q: How do I connect from VS Code Copilot?**  
Add to your VS Code `settings.json`:
```json
{
  "github.copilot.chat.mcpServers": {
    "kore": {
      "command": "C:/path/to/kore-self.exe",
      "args": ["arun"]
    }
  }
}
```
Then in Copilot Chat: `@kore run SELECT COUNT(*) FROM orders`

**Q: Is KORE production ready?**  
The SQL engine and benchmarks are solid (15/15 TPC-H, 30/30 features). The distributed cluster is functional but not yet battle-tested at petabyte scale. Use kore-self as an AI data tool today; use the core SQL engine in production for sub-100GB analytical workloads.

**Q: Where does kore-self store its memories?**  
By default: `becoming.kore.json` in the working directory. Set `KORE_SELF_HOME` env var to change location.

---

## Quick Reference Card

```sql
-- Load
COPY t FROM 'file.csv';
COPY t FROM 'file.parquet';
LOAD TABLE t FROM 'file.kore';

-- Inspect
SHOW TABLES;
DESCRIBE t;
EXPLAIN SELECT ...;

-- Query
SELECT col, COUNT(*), SUM(x), AVG(x), STDDEV(x), MEDIAN(x)
FROM t
WHERE cond
GROUP BY col HAVING agg > n
QUALIFY ROW_NUMBER() OVER (...) = 1
ORDER BY col DESC NULLS LAST
LIMIT n OFFSET m;

-- Window
ROW_NUMBER() / RANK() / DENSE_RANK() / PERCENT_RANK() / CUME_DIST()
NTILE(4) / LAG(x,1) / LEAD(x,1) / FIRST_VALUE(x) / LAST_VALUE(x)
SUM(x) OVER (PARTITION BY ... ORDER BY ... ROWS BETWEEN ... AND CURRENT ROW)

-- Date
YEAR(d) MONTH(d) DAY(d) QUARTER(d)
DATE_TRUNC('month', d)
DATEADD('day', 7, d)
DATEDIFF('day', d1, d2)
NOW()

-- Set ops
... UNION ALL ... / UNION ... / INTERSECT ... / EXCEPT ...

-- DML
INSERT INTO t VALUES (...) / SELECT ...
UPDATE t SET col=val WHERE ...
DELETE FROM t WHERE ...
MERGE INTO t USING s ON t.id=s.id WHEN MATCHED THEN UPDATE ... WHEN NOT MATCHED THEN INSERT ...
CREATE TABLE t AS SELECT ...
```

---

*KORE — Pure Rust · Zero JVM · 75 crates · 17/17 Spark wins · 30/30 SQL features*  
*Built by Sai Arun Kumar Katherashala · github.com/arunkatherashala/Kore*
