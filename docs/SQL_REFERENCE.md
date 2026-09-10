# KORE Engine — SQL Reference

Complete reference for the SQL dialect supported by the KORE query engine.

---

## 1. Query Statements

### SELECT

```sql
SELECT [DISTINCT] expr [AS alias], ...
FROM table_or_subquery [alias]
[JOIN ...]
[WHERE condition]
[GROUP BY col1, col2, ... | ROLLUP(col1, col2) | CUBE(col1, col2)]
[HAVING condition]
[QUALIFY condition]
[ORDER BY expr [ASC|DESC] [NULLS FIRST|LAST], ...]
[LIMIT n]
[OFFSET n [ROWS]]
```

**Examples:**

```sql
SELECT * FROM orders WHERE amount > 100 ORDER BY created_at DESC LIMIT 50;

SELECT department, AVG(salary) AS avg_salary
FROM employees
GROUP BY department
HAVING AVG(salary) > 60000
ORDER BY avg_salary DESC;

-- SELECT without FROM (dual/literal evaluation)
SELECT 1 + 1, NOW();
```

### SELECT DISTINCT

```sql
SELECT DISTINCT city FROM customers;
```

### FETCH FIRST (alternative to LIMIT)

```sql
SELECT * FROM orders FETCH FIRST 10 ROWS ONLY;
```

### WITH (Common Table Expressions)

```sql
WITH top_customers AS (
    SELECT customer_id, SUM(amount) AS total
    FROM orders
    GROUP BY customer_id
),
ranked AS (
    SELECT customer_id, total,
           ROW_NUMBER() OVER (ORDER BY total DESC) AS rn
    FROM top_customers
)
SELECT * FROM ranked WHERE rn <= 10;
```

Multiple CTEs are separated by commas. Each CTE is available to subsequent CTEs and the final SELECT.

### UNION ALL / UNION / INTERSECT / EXCEPT

```sql
SELECT name FROM employees
UNION ALL
SELECT name FROM contractors;

SELECT id FROM table_a
INTERSECT
SELECT id FROM table_b;

SELECT id FROM table_a
EXCEPT
SELECT id FROM table_b;
```

| Operator    | Behavior                              |
|-------------|---------------------------------------|
| UNION ALL   | Concatenate results (keeps duplicates)|
| UNION       | Concatenate and deduplicate           |
| INTERSECT   | Rows present in both queries          |
| EXCEPT      | Rows in first query but not second    |

### INSERT INTO ... VALUES

```sql
INSERT INTO users VALUES (1, 'Alice', 30);
INSERT INTO users VALUES (2, 'Bob', 25), (3, 'Carol', 28);
```

Column names are inferred from the target table schema if it already exists.

### INSERT INTO ... SELECT

```sql
INSERT INTO archive SELECT * FROM orders WHERE year = 2024;
```

### UPDATE ... SET ... WHERE

```sql
UPDATE employees SET salary = 75000 WHERE department = 'Engineering';
UPDATE products SET price = 9.99, stock = 0 WHERE discontinued = 1;
```

> **Note:** UPDATE evaluates SET values as simple literals only. Expression-based assignments (e.g., `SET price = price * 1.1`) are not supported.

### DELETE FROM ... WHERE

```sql
DELETE FROM sessions WHERE expires_at < '2024-01-01';
DELETE FROM temp_data;  -- deletes all rows (truncate)
```

### CREATE TABLE AS SELECT

```sql
CREATE TABLE monthly_report AS
SELECT month, SUM(revenue) AS total
FROM sales
GROUP BY month;
```

### CREATE VIEW

```sql
CREATE VIEW active_users AS SELECT * FROM users WHERE active = 1;
```

### CREATE TEMP TABLE

```sql
CREATE TEMP TABLE staging AS SELECT * FROM raw_data WHERE valid = 1;
CREATE TEMPORARY TABLE empty_table (id INT, name VARCHAR);
```

### DROP TABLE / DROP VIEW

```sql
DROP TABLE temp_data;
DROP VIEW active_users;
```

### LOAD TABLE / LOAD DATA

Load data from Parquet or native `.kore` files:

```sql
LOAD TABLE sales FROM 'data/sales.parquet';
LOAD DATA 'exports/archive.kore' INTO archive;
```

Supported formats: `.parquet`, `.kore`

### COPY FROM

Load from CSV/TSV/Parquet/KORE files with options:

```sql
COPY customers FROM 'data/customers.csv';
COPY logs FROM 'data/logs.tsv' WITH (DELIMITER '\t');
COPY events FROM 'data/events.csv' WITH (HEADER FALSE, DELIMITER ',');
COPY sales FROM 'data/sales.parquet';
```

### COPY TO (Export)

```sql
COPY orders TO 'output/orders.parquet';
```

### MERGE INTO

```sql
MERGE INTO target USING source ON target.id = source.id
WHEN MATCHED THEN UPDATE SET name = source.name, value = source.value
WHEN NOT MATCHED THEN INSERT;
```

### SHOW TABLES

```sql
SHOW TABLES;
```

Returns a single column `table_name` listing all registered tables.

### DESCRIBE / DESC / SHOW COLUMNS FROM

```sql
DESCRIBE employees;
DESC employees;
SHOW COLUMNS FROM employees;
```

Returns columns: `column_name`, `data_type`, `rows`.

### EXPLAIN

```sql
EXPLAIN SELECT * FROM orders WHERE amount > 100 ORDER BY id LIMIT 10;
```

Returns a text query plan showing FROM, GROUP BY, JOINS, aggregations, ORDER BY, and LIMIT.

### ANALYZE TABLE

```sql
ANALYZE TABLE orders;
```

Collects statistics (row counts, cardinality) for the query optimizer.

---

## 2. Expressions

### Arithmetic Operators

| Operator | Description    | Example            |
|----------|----------------|--------------------|
| `+`      | Addition       | `price + tax`      |
| `-`      | Subtraction    | `total - discount` |
| `*`      | Multiplication | `qty * price`      |
| `/`      | Division       | `total / count`    |
| `%`      | Modulo         | `id % 10`         |

Unary minus is supported: `SELECT -amount FROM transactions;`

### String Concatenation

The `||` operator concatenates values (auto-casts non-strings):

```sql
SELECT first_name || ' ' || last_name AS full_name FROM users;
```

### Comparison Operators

| Operator | Description      | Example          |
|----------|------------------|------------------|
| `=`      | Equal            | `status = 'A'`  |
| `<>`     | Not equal        | `type <> 'X'`   |
| `<`      | Less than        | `age < 18`      |
| `<=`     | Less or equal    | `score <= 100`  |
| `>`      | Greater than     | `amount > 0`    |
| `>=`     | Greater or equal | `rank >= 1`     |

### Logical Operators

```sql
SELECT * FROM orders WHERE amount > 100 AND status = 'active';
SELECT * FROM products WHERE category = 'A' OR category = 'B';
SELECT * FROM items WHERE NOT discontinued;
```

Precedence: `NOT` > `AND` > `OR`

### BETWEEN / NOT BETWEEN

```sql
SELECT * FROM orders WHERE amount BETWEEN 100 AND 500;
SELECT * FROM events WHERE date NOT BETWEEN '2024-01-01' AND '2024-06-30';
```

### IN / NOT IN

List form:

```sql
SELECT * FROM users WHERE country IN ('US', 'UK', 'CA');
SELECT * FROM orders WHERE status NOT IN ('cancelled', 'refunded');
```

Subquery form:

```sql
SELECT * FROM products WHERE id IN (SELECT product_id FROM featured);
SELECT * FROM users WHERE id NOT IN (SELECT user_id FROM banned);
```

### LIKE / NOT LIKE

```sql
SELECT * FROM users WHERE name LIKE 'J%';
SELECT * FROM files WHERE path NOT LIKE '%.tmp';
```

Pattern wildcards: `%` (any characters), `_` (single character).

### ILIKE / NOT ILIKE

Case-insensitive LIKE:

```sql
SELECT * FROM users WHERE email ILIKE '%@gmail.com';
```

### IS NULL / IS NOT NULL

```sql
SELECT * FROM contacts WHERE phone IS NULL;
SELECT * FROM orders WHERE shipped_date IS NOT NULL;
```

### CASE WHEN ... THEN ... ELSE ... END

Searched CASE:

```sql
SELECT name,
    CASE
        WHEN score >= 90 THEN 'A'
        WHEN score >= 80 THEN 'B'
        WHEN score >= 70 THEN 'C'
        ELSE 'F'
    END AS grade
FROM students;
```

Simple CASE:

```sql
SELECT CASE status
    WHEN 'A' THEN 'Active'
    WHEN 'I' THEN 'Inactive'
    ELSE 'Unknown'
END FROM accounts;
```

### EXISTS / NOT EXISTS

```sql
SELECT * FROM customers c
WHERE EXISTS (SELECT 1 FROM orders o WHERE o.customer_id = c.id);

SELECT * FROM products p
WHERE NOT EXISTS (SELECT 1 FROM order_items oi WHERE oi.product_id = p.id);
```

### Scalar Subqueries

```sql
SELECT name, (SELECT MAX(amount) FROM orders o WHERE o.user_id = u.id) AS max_order
FROM users u;
```

---

## 3. JOIN Types

### INNER JOIN

```sql
SELECT o.id, c.name
FROM orders o
INNER JOIN customers c ON o.customer_id = c.id;
```

`JOIN` without a qualifier defaults to INNER JOIN.

### LEFT [OUTER] JOIN

```sql
SELECT u.name, o.total
FROM users u
LEFT JOIN orders o ON u.id = o.user_id;
```

### RIGHT [OUTER] JOIN

```sql
SELECT o.id, c.name
FROM orders o
RIGHT JOIN customers c ON o.customer_id = c.id;
```

### FULL [OUTER] JOIN

```sql
SELECT a.id, b.id
FROM table_a a
FULL OUTER JOIN table_b b ON a.key = b.key;
```

### CROSS JOIN

```sql
SELECT a.color, b.size
FROM colors a
CROSS JOIN sizes b;
```

Cross joins have no ON clause and produce a Cartesian product.

### Multi-table JOINs

```sql
SELECT o.id, c.name, p.title
FROM orders o
JOIN customers c ON o.customer_id = c.id
JOIN products p ON o.product_id = p.id
WHERE o.amount > 50;
```

### Non-equi Joins

The ON clause supports arbitrary expressions, not just equality:

```sql
SELECT a.id, b.id
FROM ranges a
JOIN ranges b ON a.start <= b.end AND a.end >= b.start;
```

### Subquery as Table Source

```sql
SELECT sub.category, sub.total
FROM (SELECT category, SUM(amount) AS total FROM sales GROUP BY category) sub
WHERE sub.total > 10000;
```

### VALUES as Table Source

```sql
SELECT * FROM VALUES (1, 'a'), (2, 'b'), (3, 'c') AS t;
```

---

## 4. Aggregate Functions

| Function | Syntax | Description |
|----------|--------|-------------|
| COUNT    | `COUNT(*)` or `COUNT(expr)` | Number of rows / non-null values |
| COUNT DISTINCT | `COUNT(DISTINCT expr)` | Number of distinct non-null values |
| SUM      | `SUM(expr)` | Sum of values |
| AVG      | `AVG(expr)` | Arithmetic mean |
| MIN      | `MIN(expr)` | Minimum value |
| MAX      | `MAX(expr)` | Maximum value |
| STDDEV   | `STDDEV(expr)` | Standard deviation |
| VARIANCE | `VARIANCE(expr)` | Variance |
| MEDIAN   | `MEDIAN(expr)` | Median value |
| STRING_AGG | `STRING_AGG(expr, separator)` | Concatenate strings with delimiter |
| PERCENTILE_CONT | `PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY expr)` | Continuous percentile |
| PERCENTILE_DISC | `PERCENTILE_DISC(0.75) WITHIN GROUP (ORDER BY expr)` | Discrete percentile |

**Aliases:**

- `STDDEV`: also `STDEV`, `STDDEV_POP`, `STDDEV_SAMP`, `STD`
- `VARIANCE`: also `VAR_POP`, `VAR_SAMP`
- `STRING_AGG`: also `GROUP_CONCAT`, `LISTAGG`

**Examples:**

```sql
SELECT department,
       COUNT(*) AS headcount,
       AVG(salary) AS avg_salary,
       STDDEV(salary) AS salary_spread,
       MEDIAN(salary) AS median_salary
FROM employees
GROUP BY department;

SELECT STRING_AGG(name, ', ') AS all_names FROM team;

SELECT PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY response_time) AS p95
FROM requests;
```

---

## 5. Window Functions

All window functions use the `OVER` clause:

```sql
function(...) OVER (
    [PARTITION BY expr, ...]
    [ORDER BY expr [ASC|DESC], ...]
    [ROWS|RANGE BETWEEN frame_start AND frame_end]
)
```

### Ranking Functions

| Function | Description |
|----------|-------------|
| `ROW_NUMBER()` | Sequential number within partition |
| `RANK()` | Rank with gaps for ties |
| `DENSE_RANK()` | Rank without gaps for ties |
| `PERCENT_RANK()` | Relative rank as fraction (0 to 1) |
| `CUME_DIST()` | Cumulative distribution |
| `NTILE(n)` | Divide into n buckets |

```sql
SELECT name, department, salary,
    ROW_NUMBER() OVER (PARTITION BY department ORDER BY salary DESC) AS rn,
    RANK() OVER (ORDER BY salary DESC) AS salary_rank,
    NTILE(4) OVER (ORDER BY salary) AS quartile
FROM employees;
```

### Value Functions

| Function | Description |
|----------|-------------|
| `LAG(expr [, offset])` | Value from a previous row (default offset = 1) |
| `LEAD(expr [, offset])` | Value from a following row (default offset = 1) |
| `FIRST_VALUE(expr)` | First value in the window frame |
| `LAST_VALUE(expr)` | Last value in the window frame |

```sql
SELECT date, revenue,
    LAG(revenue, 1) OVER (ORDER BY date) AS prev_day,
    LEAD(revenue) OVER (ORDER BY date) AS next_day,
    FIRST_VALUE(revenue) OVER (ORDER BY date) AS first_revenue
FROM daily_sales;
```

### Cumulative Aggregates

| Function | Description |
|----------|-------------|
| `SUM(expr) OVER (...)` | Running/cumulative sum |
| `AVG(expr) OVER (...)` | Running/cumulative average |
| `COUNT(expr) OVER (...)` | Running/cumulative count |
| `MIN(expr) OVER (...)` | Running minimum |
| `MAX(expr) OVER (...)` | Running maximum |
| `CUMSUM(expr)` / `CUM_SUM(expr)` | Explicit cumulative sum |

```sql
SELECT date, amount,
    SUM(amount) OVER (ORDER BY date) AS running_total,
    AVG(amount) OVER (PARTITION BY category ORDER BY date
        ROWS BETWEEN 6 PRECEDING AND CURRENT ROW) AS week_avg
FROM transactions;
```

### Window Frame Specification

```sql
ROWS BETWEEN frame_start AND frame_end
RANGE BETWEEN frame_start AND frame_end
```

Frame bounds:
- `UNBOUNDED PRECEDING`
- `n PRECEDING`
- `CURRENT ROW`
- `n FOLLOWING`
- `UNBOUNDED FOLLOWING`

```sql
-- 7-day moving average
SELECT date, value,
    AVG(value) OVER (ORDER BY date ROWS BETWEEN 3 PRECEDING AND 3 FOLLOWING) AS smoothed
FROM metrics;
```

### QUALIFY

Filter on window function results (applied after window evaluation):

```sql
SELECT customer_id, order_date, amount,
    ROW_NUMBER() OVER (PARTITION BY customer_id ORDER BY order_date DESC) AS rn
FROM orders
QUALIFY rn = 1;
```

---

## 6. Scalar Functions

### String Functions

| Function | Syntax | Description |
|----------|--------|-------------|
| UPPER | `UPPER(str)` | Convert to uppercase |
| LOWER | `LOWER(str)` | Convert to lowercase |
| TRIM | `TRIM(str)` | Remove leading/trailing whitespace |
| LTRIM | `LTRIM(str)` | Remove leading whitespace |
| RTRIM | `RTRIM(str)` | Remove trailing whitespace |
| LENGTH | `LENGTH(str)` | Character count |
| SUBSTRING | `SUBSTRING(str, start [, length])` | Extract substring (1-based) |
| REPLACE | `REPLACE(str, from, to)` | Replace all occurrences |
| CONCAT | `CONCAT(a, b, ...)` | Concatenate strings |
| LEFT | `LEFT(str, n)` | First n characters |
| RIGHT | `RIGHT(str, n)` | Last n characters |
| REVERSE | `REVERSE(str)` | Reverse a string |
| REPEAT | `REPEAT(str, n)` | Repeat string n times |
| LPAD | `LPAD(str, length [, pad])` | Left-pad to length |
| RPAD | `RPAD(str, length [, pad])` | Right-pad to length |
| INITCAP | `INITCAP(str)` | Capitalize first letter of each word |
| SPLIT_PART | `SPLIT_PART(str, delimiter, n)` | Get nth part after splitting (1-based) |
| TRANSLATE | `TRANSLATE(str, from_chars, to_chars)` | Character-level substitution |
| ASCII | `ASCII(str)` | ASCII code of first character |
| CHR | `CHR(n)` | Character from ASCII code |
| CHARINDEX | `CHARINDEX(needle, haystack)` | Position of substring (1-based, 0 if not found) |
| SPACE | `SPACE(n)` | Generate n spaces |

**Aliases:** `SUBSTR` = `SUBSTRING`, `LEN` = `CHAR_LENGTH` = `LENGTH`, `INSTR` = `LOCATE` = `POSITION_OF` = `CHARINDEX`, `PROPERCASE` = `INITCAP`, `ORD` = `ASCII`, `CHAR` = `CHR`

**Examples:**

```sql
SELECT UPPER(name), LENGTH(name) FROM users;
SELECT SUBSTRING(phone, 1, 3) AS area_code FROM contacts;
SELECT LPAD(CAST(id AS VARCHAR), 8, '0') AS padded_id FROM orders;
SELECT SPLIT_PART(email, '@', 2) AS domain FROM users;
SELECT CHARINDEX('world', 'hello world');  -- returns 7
```

### Math Functions

| Function | Syntax | Description |
|----------|--------|-------------|
| ABS | `ABS(x)` | Absolute value |
| ROUND | `ROUND(x [, decimals])` | Round to n decimal places |
| FLOOR | `FLOOR(x)` | Round down to integer |
| CEIL | `CEIL(x)` | Round up to integer |
| SQRT | `SQRT(x)` | Square root |
| CBRT | `CBRT(x)` | Cube root |
| POWER | `POWER(base, exp)` | Exponentiation |
| LOG | `LOG(x)` | Natural logarithm (ln) |
| LOG2 | `LOG2(x)` | Base-2 logarithm |
| LOG10 | `LOG10(x)` | Base-10 logarithm |
| LN | `LN(x)` | Natural logarithm |
| EXP | `EXP(x)` | e raised to the power x |
| MOD | `MOD(a, b)` | Modulo (remainder) |
| SIGN | `SIGN(x)` | Sign: -1, 0, or 1 |
| TRUNC | `TRUNC(x [, decimals])` | Truncate toward zero |
| PI | `PI()` | Constant π |
| RAND | `RAND()` | Pseudo-random float [0, 1) |
| SIN | `SIN(x)` | Sine (radians) |
| COS | `COS(x)` | Cosine (radians) |
| TAN | `TAN(x)` | Tangent (radians) |
| DEGREES | `DEGREES(x)` | Radians to degrees |
| RADIANS | `RADIANS(x)` | Degrees to radians |

**Aliases:** `CEILING` = `CEIL`, `POW` = `POWER`, `TRUNCATE` = `TRUNC`, `RANDOM` = `RAND`

**Examples:**

```sql
SELECT ROUND(price * 1.08, 2) AS with_tax FROM products;
SELECT SQRT(POWER(x2 - x1, 2) + POWER(y2 - y1, 2)) AS distance FROM points;
SELECT FLOOR(age / 10) * 10 AS age_bucket, COUNT(*) FROM users GROUP BY 1;
```

### Date/Time Functions

| Function | Syntax | Description |
|----------|--------|-------------|
| EXTRACT | `EXTRACT(field FROM date)` | Extract year/month/day/quarter |
| YEAR | `YEAR(date)` | Extract year |
| MONTH | `MONTH(date)` | Extract month |
| DAY | `DAY(date)` | Extract day |
| DATE_TRUNC | `DATE_TRUNC('part', date)` | Truncate to year/month/quarter |
| DATEADD | `DATEADD('part', n, date)` | Add interval to date |
| DATEDIFF | `DATEDIFF('day', start, end)` | Difference between dates in days |
| NOW | `NOW()` | Current date/time as string |
| CURRENT_DATE | `CURRENT_DATE()` | Current date |
| TO_DATE | `TO_DATE(str)` | Parse string as date |
| STRFTIME | `STRFTIME(format, date)` | Format date as string |

**Date representations:** Dates can be stored as `YYYYMMDD` integers or `'YYYY-MM-DD'` strings. Both formats are accepted by all date functions.

**Aliases:** `CURRENT_TIMESTAMP` = `NOW`, `DATE_ADD` = `DATEADD`, `DATE_DIFF` = `DATEDIFF`, `EXTRACT_YEAR` = `YEAR`, `EXTRACT_MONTH` = `MONTH`, `EXTRACT_DAY` = `DAY`, `FORMAT_DATE` = `STRFTIME`, `DATE` = `TO_DATE`

**STRFTIME format codes:** `%Y` (year), `%m` (month), `%d` (day)

**Examples:**

```sql
SELECT EXTRACT(YEAR FROM order_date) AS yr, COUNT(*) FROM orders GROUP BY 1;
SELECT DATE_TRUNC('month', created_at) AS month, SUM(amount) FROM sales GROUP BY 1;
SELECT DATEADD('day', 30, '2024-01-15');  -- '2024-02-14'
SELECT DATEDIFF('day', '2024-01-01', '2024-03-01');  -- 60
```

> **Note:** DATEADD uses approximate months (30 days) and years (365 days).

### Null-Handling Functions

| Function | Syntax | Description |
|----------|--------|-------------|
| COALESCE | `COALESCE(a, b, ...)` | First non-null argument |
| NULLIF | `NULLIF(a, b)` | NULL if a equals b, else a |
| NVL | `NVL(a, b)` | First non-null (alias for COALESCE) |
| IFNULL | `IFNULL(a, b)` | First non-null (alias for COALESCE) |

**Examples:**

```sql
SELECT COALESCE(nickname, first_name, 'Unknown') AS display_name FROM users;
SELECT NULLIF(divisor, 0) FROM data;  -- returns NULL if divisor is 0
```

### Conditional Functions

| Function | Syntax | Description |
|----------|--------|-------------|
| IF | `IF(cond, then_val, else_val)` | Inline conditional |
| IIF | `IIF(cond, then_val, else_val)` | Alias for IF |
| GREATEST | `GREATEST(a, b, ...)` | Maximum of arguments |
| LEAST | `LEAST(a, b, ...)` | Minimum of arguments |

```sql
SELECT IF(score >= 60, 'Pass', 'Fail') FROM exams;
SELECT GREATEST(price_a, price_b, price_c) AS max_price FROM quotes;
```

### Type Conversion

| Function | Syntax | Description |
|----------|--------|-------------|
| CAST | `CAST(expr AS type)` | Convert to target type |
| CONVERT | `CONVERT(expr, type)` | Alias for CAST |
| ISNUMERIC | `ISNUMERIC(expr)` | Returns true if expr is numeric |

Supported target types: `INT`/`INTEGER`/`BIGINT`, `FLOAT`/`DOUBLE`/`REAL`/`NUMERIC`/`DECIMAL`, `VARCHAR`/`TEXT`/`STRING`/`CHAR`, `BOOLEAN`/`BOOL`

```sql
SELECT CAST('123' AS INT);
SELECT CAST(price AS VARCHAR) || ' USD' FROM products;
SELECT * FROM raw WHERE ISNUMERIC(value);
```

---

## 7. Data Types

| Type | Aliases | Storage | Description |
|------|---------|---------|-------------|
| BIGINT | `INT`, `INTEGER` | 64-bit signed integer | Whole numbers |
| DOUBLE | `FLOAT`, `REAL`, `NUMERIC`, `DECIMAL` | 64-bit IEEE 754 | Floating point |
| VARCHAR | `STRING`, `TEXT`, `CHAR` | UTF-8 string | Text data |
| BOOLEAN | `BOOL` | 1-bit | `true` / `false` |

**Internal storage variants:**
- `ColumnData::Int64` — nullable 64-bit integers
- `ColumnData::Float64` — nullable 64-bit floats
- `ColumnData::Str` — nullable UTF-8 strings
- `ColumnData::Bool` — nullable booleans
- `ColumnData::StrDict` — dictionary-encoded strings (used for low-cardinality columns)

**Null handling:** All types are nullable. NULL propagates through arithmetic and most functions.

**Literal syntax:**
```sql
42              -- integer
3.14            -- float
'hello world'  -- string
NULL            -- null
```

---

## 8. Known Limitations

### Partially Supported

| Feature | Limitation |
|---------|-----------|
| UPDATE SET | Only supports literal values in SET assignments. Expression-based updates (e.g., `SET x = x + 1`) are not supported. |
| DATEADD | Uses approximate intervals: month = 30 days, year = 365 days. No calendar-aware arithmetic. |
| GROUP BY ROLLUP/CUBE | Parsed but treated as plain GROUP BY (subtotals not generated). |
| MERGE INTO | Basic support for WHEN MATCHED/WHEN NOT MATCHED. Complex conditions within WHEN clauses may not be fully evaluated. |
| ORDER BY expressions | ORDER BY works best with column references. Complex expressions in ORDER BY may have limited support. |

### Not Supported

| Feature | Notes |
|---------|-------|
| CREATE INDEX | No index support; all scans are sequential or hash-based. |
| ALTER TABLE | Schema modifications after table creation are not supported. |
| Transactions (BEGIN/COMMIT/ROLLBACK) | Single-statement execution; no multi-statement transactions. |
| Foreign keys / constraints | No referential integrity enforcement. |
| Stored procedures / triggers | Not available. |
| Recursive CTEs | WITH RECURSIVE is not supported. |
| PIVOT / UNPIVOT | Not available. |
| JSON functions | No JSON path operations. |
| Regular expression matching (RLIKE/REGEXP) | Not supported; use LIKE/ILIKE for pattern matching. |
| Multi-database queries | Single-context execution only. |
| GRANT / REVOKE (security) | No access control. |
| WINDOW clause (named windows) | Define window inline with OVER; named window references are not supported. |
| UPDATE with expressions | `SET col = col + 1` or `SET col = other_col` not supported. |

### Engine Characteristics

- **In-memory execution**: All data must fit in memory.
- **Columnar storage**: Optimized for analytical (OLAP) workloads, not OLTP.
- **Hash joins**: Equi-joins use hash join; non-equi joins use nested-loop.
- **Predicate pushdown**: The optimizer pushes WHERE filters into table scans.
- **Limit pushdown**: LIMIT is propagated to the scan stage for early termination when possible.
- **File formats**: Native `.kore` binary format, Apache Parquet, and CSV/TSV for data loading.
- **UDFs**: User-defined functions can be registered programmatically via `register_udf()`.
