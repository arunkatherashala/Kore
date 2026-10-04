# KORE SQL: what is supported

This matrix is derived from the regression suites in `kore-sql/tests` (`sql_coverage.rs`, `sql_coverage2.rs`,
`sql_correctness.rs`, `differential.rs`, which compares against SQLite) and from hand-derived probes against Spark SQL
semantics. "Supported" means a test with a hand-derived expected value passes. Anything not listed is not tested and
should be assumed unsupported. The engine's rule is that unsupported syntax must be an **error**, never a silently
different answer; a query that is accepted but gives a different result than Spark is a bug, please report it.

Entry points: `KqlContext::query` (read-only statements) and `KqlContext::execute_dml` (statements that change the
catalog or tables). `query` also answers `SHOW TABLES`, `DESCRIBE t` and `EXPLAIN`.

## Data model limits (affect many rows below)

* Column types are BIGINT, DOUBLE, BOOLEAN and STRING only. DATE / TIMESTAMP values are ISO strings, DECIMAL is a
  double (`CAST(x AS DECIMAL(p,s))` rounds to the scale and returns NULL on overflow, but values print as doubles,
  `1.50` prints `1.5`, and `typeof` reports double). Arithmetic on decimal *literals* is exact (`0.1 + 0.2 = 0.3`).
* ARRAY and MAP values exist inside a query and print as Spark does (`[1, 2]`, `{a -> 1}`); they are carried as text, so
  they cannot be stored in a registered table column with a native array type.
* No STRUCT type. No INTERVAL type as a value (INTERVAL literals work as operands of `+` / `-` with dates).
* Whole tables live in memory.

## Query features

| Area | Status | Notes |
|---|---|---|
| SELECT, WHERE, DISTINCT, aliases, `*`, `t.*`, expressions | supported | |
| Identifiers case-insensitive, backtick quoting | supported | |
| Joins: inner, left, right, full, cross, natural, USING, semi, anti, implicit (comma) | supported | |
| Non-equi and multi-condition ON | supported | nested loop or residual filter after a hash join |
| Subqueries: scalar, IN, NOT IN, EXISTS, ANY/SOME/ALL, correlated | supported | scalar subquery with several rows is an error |
| UNION / UNION ALL / INTERSECT / EXCEPT (+ ALL), precedence, ORDER BY/LIMIT on the result | supported | |
| CTEs, column-aliased CTEs | supported | |
| WITH RECURSIVE | supported | anchor `UNION [ALL]` step; stops at 1000 rounds / 5M rows with an error |
| GROUP BY expressions, ordinals, aliases, HAVING | supported | |
| ROLLUP, CUBE, GROUPING SETS, `WITH ROLLUP/CUBE`, mixed `GROUP BY a, ROLLUP(b)`, `grouping()`, `grouping_id()` | supported | |
| GROUP BY ALL | supported | |
| ORDER BY column / ordinal / alias / expression, NULLS FIRST/LAST | supported | NULLs first ascending, last descending (Spark default) |
| LIMIT / OFFSET / FETCH FIRST | supported | LIMIT and OFFSET accept constant expressions (`LIMIT 1 + 1`, `LIMIT cast('3' as int)`); non-constant is an error |
| Window functions: row_number, rank, dense_rank, ntile, percent_rank, cume_dist, lag, lead, first/last/nth_value, aggregates | supported | ranking columns are BIGINT |
| Window frames ROWS / RANGE with numeric or INTERVAL offsets, named windows `WINDOW w AS (...)`, `OVER (w ...)` | supported | |
| `GROUPS` frames | unsupported | error |
| Window query without outer ORDER BY | partial | rows come out in the order of the last window (partition keys, then its ORDER BY). Spark's order is unspecified; do not depend on it |
| QUALIFY | supported | |
| PIVOT / UNPIVOT | supported | |
| LATERAL VIEW explode / posexplode / explode_outer / posexplode_outer, `explode(map)` | supported | `inline`, `stack`, `json_tuple` unsupported |
| TABLESAMPLE | unsupported | error (non-deterministic in Spark as well) |
| CLUSTER BY / DISTRIBUTE BY / SORT BY | unsupported | error |
| Query hints (`/*+ BROADCAST(t) */`) | partial | parsed; only some influence the plan |
| `EXPLAIN` | partial | prints a short summary, not Spark's plan format |
| `SHOW TABLES`, `DESCRIBE t` | partial | KORE's own output shape (table_name; column_name, data_type, rows) |
| `USE`, `SET`, `SHOW COLUMNS IN`, `ANALYZE TABLE` options | unsupported | `ANALYZE TABLE t` works |

## Statements (`execute_dml`)

| Statement | Status | Notes |
|---|---|---|
| `INSERT INTO [TABLE] t VALUES (...), (...)` / `SELECT ...` / `(col list)` | supported | values run through the normal expression engine; `''` escapes work; type mismatches (1.5 into BIGINT, text into BIGINT) are errors; unlisted columns get NULL |
| `INSERT OVERWRITE [TABLE] t ...` | supported | |
| `UPDATE t SET c = expr[, ...] [WHERE ...]` | supported | expressions may refer to the row; NULL predicate leaves the row alone |
| `DELETE FROM t [WHERE ...]` | supported | rows whose predicate is NULL are kept; schema is kept when all rows go |
| `TRUNCATE TABLE t` | supported | |
| `CREATE [OR REPLACE] [TEMP[ORARY]] TABLE [IF NOT EXISTS] t AS SELECT`, `CREATE TABLE t (cols)` | supported | column types map to BIGINT/DOUBLE/STRING/BOOLEAN |
| `CREATE [OR REPLACE] [TEMP[ORARY]] VIEW [IF NOT EXISTS] v AS SELECT` | supported | view text is stored and run on use |
| `DROP TABLE|VIEW [IF EXISTS]` | supported | dropping a missing object without IF EXISTS is an error |
| `MERGE INTO`, `COPY`, `LOAD DATA`, `CREATE TABLE ... STORED AS PARQUET`, `COPY ... TO` | supported (KORE-specific) | not Spark syntax |
| `ALTER TABLE`, `CREATE INDEX`, transactions | unsupported | error |

## Functions

| Group | Status | Notes |
|---|---|---|
| Aggregates: count, sum, avg, min, max, stddev/variance (samp/pop), median, percentile(_approx), mode, first/last, any_value, max_by/min_by, count_if, bool_and/or, every, corr, covar_*, skewness, kurtosis, approx_count_distinct, collect_list/set, string_agg/listagg, DISTINCT, FILTER (WHERE) | supported | integer SUM/MIN/MAX stay BIGINT and are exact beyond 2^53 |
| `bit_and`, `bit_or`, `bit_xor`, `regr_*`, `histogram_numeric` aggregates | unsupported | error |
| Strings: upper/lower, trim family, substr, concat(_ws), replace, lpad/rpad, repeat, initcap, translate, split, split_part, locate/instr, left/right, ascii/chr, levenshtein, base64/hex/md5/sha1/sha2/crc32, format_string/printf, format_number | supported | `format_number` rounds HALF_EVEN as Spark documents; `%e` prints Java style (`1.234568e+04`) |
| `soundex`, `hash`, `xxhash64`, `sentences`, `iff`, `gcd` | unsupported | error (`iff` and `gcd` are not Spark functions) |
| Regex: regexp_replace, regexp_extract, regexp_extract_all, regexp_like/count/instr/substr, RLIKE | supported | Rust regex syntax (no look-around or back-references) |
| Math, bitwise, conv, bin, width_bucket, round/bround, pmod, try_* | supported | integer division by zero gives NULL |
| Conditional: CASE, IF, coalesce, nvl, nvl2, ifnull, nullif, greatest, least, decode | supported | |
| CAST / TRY_CAST to int, bigint, double, string, boolean, date, timestamp, decimal(p,s) | supported | float to string follows Spark (`1.0E8`); out-of-range casts follow the non-ANSI rules |
| Dates and timestamps: date_add/sub, add_months, last_day, next_day, datediff, months_between, date_trunc, trunc, year..second, dayofweek, weekofyear, quarter, date_format, to_date, to_timestamp, make_date, make_timestamp, unix_timestamp, from_unixtime, unix_millis/micros/seconds, extract, date_part, timestampadd/diff, sequence over dates | supported | session time zone is UTC; `current_timezone()` returns UTC |
| `date +/- n` (days), `date + INTERVAL ..` | supported | |
| `date - date`, `timestamp - timestamp` | unsupported | Spark returns an INTERVAL; KORE raises an error (use `datediff`) |
| `from_utc_timestamp`, `to_utc_timestamp` with zone names, `window()` (time windows) | unsupported | error |
| Comparing a DATE with a TIMESTAMP value (`date '2024-03-15' = timestamp '2024-03-15 00:00:00'`) | partial | values are strings, so this is `false`; cast both sides to the same type |
| Arrays: array(), size, element_at, `arr[i]`, array_contains/position/join/sort/distinct/union/intersect/except/max/min/remove/compact/append/prepend/repeat, slice, flatten, reverse, sequence, split, sort_array, collect_list/set | supported | |
| Higher-order functions with lambdas `x -> ..`, `(x, i) -> ..`: transform, filter, exists, forall, aggregate/reduce, zip_with, array_sort(comparator), map_filter, transform_keys/values, map_zip_with | supported | lambda bodies may use any scalar expression and columns of the row |
| MAP: `map(k, v, ..)`, `m[k]`, element_at, size, map_keys/values/entries, map_contains_key, map_from_arrays/entries, map_concat, str_to_map | supported | duplicate keys: last wins; NULL key is an error; `CAST(.. AS MAP<..>)` unsupported |
| STRUCT: `struct()`, `named_struct()`, `s.field` | unsupported | error |
| JSON: get_json_object, json_array_length, to_json (arrays, maps, scalars), from_json with `ARRAY<..>` / `MAP<..>` schema | supported | `from_json` with a struct schema, `json_tuple`, `schema_of_json` are errors |
| UDFs registered from Rust (`register_udf`) | supported | |
| Unknown function names | error | never silently NULL |

## Known semantic differences (accepted, so be aware)

* DECIMAL is a double: results print without trailing zeros and exceed 15-17 significant digits imprecisely.
* Date and timestamp comparisons across types are string comparisons (see above).
* Window / unordered query output order is deterministic here and unspecified in Spark.
* `EXPLAIN` / `DESCRIBE` / `SHOW` output formats differ from Spark's.
* `INSERT` is stricter than Spark's non-ANSI mode: it will not narrow DOUBLE to BIGINT or parse text into numbers.

## How to extend the matrix

Add a case to `kore-sql/tests/sql_coverage2.rs` (`(sql, expected)` pairs, `ERR` for must-be-rejected) with an expected
value worked out by hand from Spark semantics, make it pass, and update the row here.
