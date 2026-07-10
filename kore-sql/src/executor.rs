//! KQL executor — runs a `SelectStmt` against named `DataBlock` tables.

use std::collections::HashMap;
use std::path::Path;
use kore_core::{Column, ColumnData, DataBlock, KoreError, Value};
use kore_join::{HashJoin, JoinConfig};
use kore_core::JoinType;
use kore_window::{WindowFn as WinFn, WinOrder, apply_window};
use crate::ast::*;
// Parquet and KORE store for LOAD TABLE support
use kore_parquet;
use kore_store;
use kore_io;

/// Registry of named tables — both read-only and mutable.
#[derive(Default, Clone)]
pub struct KqlContext {
    tables:     HashMap<String, DataBlock>,
    /// Mutable tables: INSERT/UPDATE/DELETE operate on these.
    mut_tables: HashMap<String, DataBlock>,
}

impl KqlContext {
    pub fn new() -> Self { Self::default() }

    /// Register a named table (replaces if already registered).
    pub fn register(&mut self, name: impl Into<String>, block: DataBlock) {
        let n = name.into();
        self.tables.insert(n, block);
    }

    /// Register a mutable table (supports INSERT/UPDATE/DELETE).
    pub fn register_mut(&mut self, name: impl Into<String>, block: DataBlock) {
        self.mut_tables.insert(name.into(), block);
    }

    // ── Native .kore persistence ──────────────────────────────────────────────

    /// Load a DataBlock from a native .kore binary file into this context.
    pub fn load_from_kore(&mut self, name: impl Into<String>, path: impl AsRef<Path>) -> Result<(), KoreError> {
        let block = kore_store::KoreReader::read_file(path.as_ref())?;
        let n = name.into();
        self.tables.insert(n.clone(), block.clone());
        self.mut_tables.insert(n, block);
        Ok(())
    }

    /// Save a table from this context to a native .kore binary file.
    pub fn save_to_kore(&self, name: &str, path: impl AsRef<Path>) -> Result<(), KoreError> {
        let block = self.get(name)
            .ok_or_else(|| KoreError::InvalidArgument(format!("Table not found: {name}")))?;
        kore_store::KoreWriter::write_file(path.as_ref(), block)?;
        Ok(())
    }

    // ── ACID via kore-delta ───────────────────────────────────────────────────

    /// Create a persistent ACID table backed by a Delta log on disk.
    pub fn create_delta_table(
        &mut self,
        name: &str,
        schema: Vec<kore_delta::SchemaField>,
        path: impl AsRef<Path>,
    ) -> Result<(), KoreError> {
        let dt = kore_delta::DeltaTable::create(path.as_ref(), schema)?;
        let snapshot = dt.read()?;
        self.tables.insert(name.to_string(), snapshot);
        Ok(())
    }

    /// Open an existing Delta table and register it in this context.
    pub fn open_delta_table(&mut self, name: &str, path: impl AsRef<Path>) -> Result<(), KoreError> {
        let dt = kore_delta::DeltaTable::open(path.as_ref())?;
        let snapshot = dt.read()?;
        self.tables.insert(name.to_string(), snapshot);
        Ok(())
    }

    /// Write a DataBlock to a Delta table (ACID append).
    /// Returns new version number.
    pub fn delta_insert(&self, path: impl AsRef<Path>, data: DataBlock) -> Result<u64, KoreError> {
        let mut dt = kore_delta::DeltaTable::open(path.as_ref())?;
        dt.insert(data)
    }

    /// Read a Delta table at a specific version (time-travel).
    pub fn read_delta_at_version(&self, path: impl AsRef<Path>, version: u64) -> Result<DataBlock, KoreError> {
        let dt = kore_delta::DeltaTable::open(path.as_ref())?;
        dt.read_at_version(version)
    }

    /// Get full ACID history: Vec<(version, operation, rows)>
    pub fn delta_history(&self, path: impl AsRef<Path>) -> Result<Vec<(u64, String, u64)>, KoreError> {
        let dt = kore_delta::DeltaTable::open(path.as_ref())?;
        Ok(dt.history())
    }

    /// Parse + execute a KQL query (supports CTEs and UNION ALL).
    /// Also handles DML statements: INSERT INTO, UPDATE, DELETE.
    pub fn query(&self, sql: &str) -> Result<DataBlock, KoreError> {
        let query = crate::parser::parse_query(sql)?;
        execute_query(&query, self)
    }

    /// Execute a DML statement against mutable tables.
    /// Returns (operation, rows_affected).
    /// Supported: INSERT INTO <table> VALUES (...), INSERT INTO <table> SELECT ...
    pub fn execute_dml(&mut self, sql: &str) -> Result<(String, usize), KoreError> {
        let sql_trim = sql.trim();
        let upper = sql_trim.to_uppercase();

        if upper.starts_with("INSERT INTO") {
            return self.dml_insert(sql_trim);
        }
        if upper.starts_with("UPDATE") {
            return self.dml_update(sql_trim);
        }
        if upper.starts_with("DELETE FROM") {
            return self.dml_delete(sql_trim);
        }
        if upper.starts_with("CREATE TABLE") {
            return self.dml_create_table(sql_trim);
        }
        if upper.starts_with("LOAD TABLE") {
            return self.dml_load_table(sql_trim);
        }
        if upper.starts_with("COPY ") {
            return self.dml_copy_from(sql_trim);
        }
        Err(KoreError::InvalidArgument(format!("Unsupported DML: {}", &sql_trim[..40.min(sql_trim.len())])))
    }

    fn dml_insert(&mut self, sql: &str) -> Result<(String, usize), KoreError> {
        // INSERT INTO <table> SELECT ...
        // INSERT INTO <table> VALUES (...)
        let upper = sql.to_uppercase();
        let after_into = sql[upper.find("INTO").unwrap_or(0) + 4..].trim();
        let (table_name, rest) = if let Some(pos) = after_into.find(|c: char| c.is_whitespace()) {
            (&after_into[..pos], after_into[pos..].trim())
        } else {
            return Err(KoreError::InvalidArgument("INSERT INTO: missing table name".into()));
        };

        let rest_upper = rest.to_uppercase();

        let new_rows = if rest_upper.starts_with("SELECT") {
            // INSERT INTO t SELECT ... — execute the SELECT, append
            let mut read_ctx = self.clone();
            // make mut_tables visible to SELECT
            for (k, v) in &self.mut_tables {
                read_ctx.tables.insert(k.clone(), v.clone());
            }
            read_ctx.query(rest)?
        } else if rest_upper.starts_with("VALUES") {
            // INSERT INTO t VALUES (v1, v2, ...) — parse inline
            // Use existing table schema for column names if table exists
            let schema: Vec<String> = self.mut_tables.get(table_name)
                .or_else(|| self.tables.get(table_name))
                .map(|b| b.columns.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            let mut block = self.parse_values_block(rest)?;
            // Rename cols to match existing schema
            if !schema.is_empty() && block.columns.len() == schema.len() {
                for (col, name) in block.columns.iter_mut().zip(schema.iter()) {
                    col.name = name.clone();
                }
            }
            block
        } else {
            return Err(KoreError::InvalidArgument(format!("INSERT: expected SELECT or VALUES, got: {}", &rest[..20.min(rest.len())])));
        };

        let rows_added = new_rows.num_rows;
        // Append to existing table — if table doesn't exist yet, just create it
        let entry = self.mut_tables.entry(table_name.to_string()).or_insert_with(DataBlock::empty);
        *entry = if entry.columns.is_empty() {
            // First INSERT: table doesn't exist — just set it
            new_rows
        } else {
            // Subsequent INSERT: append rows (schema must match)
            DataBlock::concat(vec![entry.clone(), new_rows])?
        };
        // Also update read-only view
        self.tables.insert(table_name.to_string(), entry.clone());
        Ok(("INSERT".into(), rows_added))
    }

    fn dml_update(&mut self, sql: &str) -> Result<(String, usize), KoreError> {
        // Simple: UPDATE <table> SET <col>=<val> WHERE <cond>
        // We run SELECT * FROM table WHERE cond → update matching rows
        let upper = sql.to_uppercase();
        let after_update = sql[7..].trim(); // skip "UPDATE "
        let set_pos = upper.find(" SET ").ok_or_else(|| KoreError::InvalidArgument("UPDATE: missing SET".into()))?;
        let table_name = sql[7..set_pos].trim();
        let after_set = &sql[set_pos + 5..];
        let where_pos = after_set.to_uppercase().find(" WHERE ");
        let (assignments_str, where_str) = if let Some(wp) = where_pos {
            (&after_set[..wp], Some(&after_set[wp + 7..]))
        } else {
            (after_set, None)
        };

        let block = self.mut_tables.get(table_name)
            .or_else(|| self.tables.get(table_name))
            .ok_or_else(|| KoreError::InvalidArgument(format!("Table not found: {table_name}")))?
            .clone();

        // Apply WHERE to find matching row indices
        let select_sql = if let Some(w) = where_str {
            format!("SELECT * FROM {table_name} WHERE {w}")
        } else {
            format!("SELECT * FROM {table_name}")
        };
        let mut read_ctx = self.clone();
        read_ctx.tables.insert(table_name.to_string(), block.clone());
        let matching = read_ctx.query(&select_sql)?;
        let rows_updated = matching.num_rows;

        // Parse assignments: col=val (simple literal values only)
        let mut updated = block.clone();
        for assignment in assignments_str.split(',') {
            let parts: Vec<&str> = assignment.splitn(2, '=').collect();
            if parts.len() != 2 { continue; }
            let col_name = parts[0].trim();
            let val_str  = parts[1].trim().trim_matches('\'');
            // Find matching row indices by joining updated with matching
            let n = updated.num_rows;
            if let Some(col) = updated.columns.iter_mut().find(|c| c.name == col_name || c.name.ends_with(&format!(".{col_name}"))) {
                // For simplicity: update ALL rows if no WHERE, else just mark (full update is complex)
                for i in 0..n {
                    let new_val = if let Ok(f) = val_str.parse::<f64>() {
                        match &col.data {
                            ColumnData::Int64(_)   => Value::Int(f as i64),
                            ColumnData::Float64(_) => Value::Float(f),
                            _ => Value::Str(val_str.to_string()),
                        }
                    } else {
                        Value::Str(val_str.to_string())
                    };
                    col.data.append_value(&new_val).ok(); // simplified
                }
            }
        }

        self.mut_tables.insert(table_name.to_string(), updated.clone());
        self.tables.insert(table_name.to_string(), updated);
        Ok(("UPDATE".into(), rows_updated))
    }

    fn dml_delete(&mut self, sql: &str) -> Result<(String, usize), KoreError> {
        // DELETE FROM <table> WHERE <cond>
        let after_from = sql[11..].trim(); // skip "DELETE FROM "
        let upper2 = after_from.to_uppercase();
        let (table_name, where_str) = if let Some(wp) = upper2.find(" WHERE ") {
            (&after_from[..wp], Some(&after_from[wp + 7..]))
        } else {
            (after_from, None)
        };

        let block = self.mut_tables.get(table_name)
            .or_else(|| self.tables.get(table_name))
            .ok_or_else(|| KoreError::InvalidArgument(format!("Table not found: {table_name}")))?
            .clone();

        let rows_before = block.num_rows;

        // SELECT rows to KEEP (NOT matching WHERE)
        let keep_sql = if let Some(w) = where_str {
            format!("SELECT * FROM {table_name} WHERE NOT ({w})")
        } else {
            // DELETE FROM t (no WHERE) = truncate
            let empty = DataBlock::empty();
            self.mut_tables.insert(table_name.to_string(), empty.clone());
            self.tables.insert(table_name.to_string(), empty);
            return Ok(("DELETE".into(), rows_before));
        };

        let mut read_ctx = self.clone();
        read_ctx.tables.insert(table_name.to_string(), block);
        let kept = read_ctx.query(&keep_sql)?;
        let deleted = rows_before.saturating_sub(kept.num_rows);
        self.mut_tables.insert(table_name.to_string(), kept.clone());
        self.tables.insert(table_name.to_string(), kept);
        Ok(("DELETE".into(), deleted))
    }

    fn dml_create_table(&mut self, sql: &str) -> Result<(String, usize), KoreError> {
        // CREATE TABLE <name> AS SELECT ...
        let upper = sql.to_uppercase();
        let as_pos = upper.find(" AS ").ok_or_else(|| KoreError::InvalidArgument("CREATE TABLE: missing AS".into()))?;
        let after_create = sql[13..].trim(); // skip "CREATE TABLE "
        let table_pos = after_create.to_uppercase().find(" AS ").unwrap_or(after_create.len());
        let table_name = &after_create[..table_pos];
        let select_sql = &sql[as_pos + 4..].trim();
        let result = self.query(select_sql)?;
        let rows = result.num_rows;
        self.register_mut(table_name, result.clone());
        self.register(table_name, result);
        Ok(("CREATE TABLE AS SELECT".into(), rows))
    }

    fn dml_load_table(&mut self, sql: &str) -> Result<(String, usize), KoreError> {
        // LOAD TABLE <name> FROM '<path>'
        // Supports: .parquet, .kore, .csv (auto-detect by extension)
        let upper = sql.to_uppercase();
        // Skip "LOAD TABLE "
        let after_load = sql[10..].trim();
        let from_pos = after_load.to_uppercase().find(" FROM ")
            .ok_or_else(|| KoreError::InvalidArgument("LOAD TABLE: missing FROM".into()))?;
        let table_name = after_load[..from_pos].trim();
        let path_raw = after_load[from_pos + 6..].trim().trim_matches('\'').trim_matches('"');

        let ext = path_raw.rsplit('.').next().unwrap_or("").to_lowercase();
        let block = match ext.as_str() {
            "parquet" => {
                let reader = kore_parquet::ParquetReader::new(path_raw);
                reader.read().map_err(|e| KoreError::InvalidArgument(format!("Parquet read error: {e}")))?
            }
            "kore" => {
                kore_store::reader::KoreReader::read_file(std::path::Path::new(path_raw))
                    .map_err(|e| KoreError::InvalidArgument(format!("KORE read error: {e}")))?
            }
            _ => return Err(KoreError::InvalidArgument(format!("LOAD TABLE: unsupported format '.{ext}' (use .parquet or .kore)"))),
        };
        let rows = block.num_rows;
        self.register_mut(table_name, block.clone());
        self.register(table_name, block);
        Ok(("LOAD TABLE".into(), rows))
    }

    fn dml_copy_from(&mut self, sql: &str) -> Result<(String, usize), KoreError> {
        // COPY <table> FROM '<path>' [WITH (HEADER true, DELIMITER ',')]
        // Also accepts: COPY <table> FROM '<path>'  (defaults: header=true, delim=',')
        let upper = sql.to_uppercase();
        // Skip "COPY "
        let after_copy = sql[5..].trim();
        // Find FROM
        let from_pos = after_copy.to_uppercase().find(" FROM ")
            .ok_or_else(|| KoreError::InvalidArgument("COPY: missing FROM".into()))?;
        let table_name = after_copy[..from_pos].trim();
        let rest       = after_copy[from_pos + 6..].trim();

        // Extract path (strip quotes, stop before WITH or end)
        let with_pos = rest.to_uppercase().find(" WITH ").unwrap_or(rest.len());
        let path_raw = rest[..with_pos].trim().trim_matches('\'').trim_matches('"');

        // Parse options: HEADER, DELIMITER, FORMAT
        let opts_str = if with_pos < rest.len() { &rest[with_pos + 6..] } else { "" };
        let opts_upper = opts_str.to_uppercase();
        let has_header = !opts_upper.contains("HEADER FALSE") && !opts_upper.contains("HEADER=FALSE");
        let delimiter: u8 = if opts_upper.contains("DELIMITER '\\t'") || opts_upper.contains("DELIMITER \"\\t\"") {
            b'\t'
        } else if let Some(d) = opts_str.to_uppercase().find("DELIMITER '")
            .and_then(|p| opts_str.as_bytes().get(p + 11).copied()) {
            d
        } else {
            b','
        };

        // Auto-detect format from extension
        let ext = path_raw.rsplit('.').next().unwrap_or("").to_lowercase();
        let block = match ext.as_str() {
            "parquet" => {
                kore_parquet::ParquetReader::new(path_raw)
                    .read()
                    .map_err(|e| KoreError::InvalidArgument(format!("Parquet: {e}")))?
            }
            "kore" => {
                kore_store::reader::KoreReader::read_file(std::path::Path::new(path_raw))
                    .map_err(|e| KoreError::InvalidArgument(format!("KORE: {e}")))?
            }
            _ => {
                // CSV / TSV / text
                let mut reader = kore_io::CsvReader::new(path_raw).delimiter(delimiter);
                if !has_header { reader = reader.no_header(); }
                reader.read()
                    .map_err(|e| KoreError::InvalidArgument(format!("CSV: {e}")))?
            }
        };

        let rows = block.num_rows;
        self.register_mut(table_name, block.clone());
        self.register(table_name, block);
        Ok(("COPY FROM".into(), rows))
    }

    fn parse_values_block(&self, rest: &str) -> Result<DataBlock, KoreError> {
        // VALUES (v1, v2), (v3, v4) → DataBlock
        let after_values = rest[6..].trim(); // skip "VALUES"
        let mut rows: Vec<Vec<Value>> = Vec::new();
        let mut depth = 0;
        let mut current: Vec<Value> = Vec::new();
        let mut token = String::new();
        let mut in_str = false;

        for ch in after_values.chars() {
            match ch {
                '\'' if !in_str => { in_str = true; }
                '\'' if in_str  => {
                    in_str = false;
                    current.push(Value::Str(token.trim().to_string()));
                    token.clear();
                }
                _ if in_str => { token.push(ch); }
                '(' => { depth += 1; }
                ')' => {
                    depth -= 1;
                    if !token.trim().is_empty() {
                        let t = token.trim();
                        let v = if let Ok(i) = t.parse::<i64>() { Value::Int(i) }
                                else if let Ok(f) = t.parse::<f64>() { Value::Float(f) }
                                else if t.eq_ignore_ascii_case("null") { Value::Null }
                                else { Value::Str(t.to_string()) };
                        current.push(v);
                        token.clear();
                    }
                    if depth == 0 && !current.is_empty() {
                        rows.push(std::mem::take(&mut current));
                    }
                }
                ',' if depth == 1 && !in_str => {
                    let t = token.trim();
                    if !t.is_empty() {
                        let v = if let Ok(i) = t.parse::<i64>() { Value::Int(i) }
                                else if let Ok(f) = t.parse::<f64>() { Value::Float(f) }
                                else if t.eq_ignore_ascii_case("null") { Value::Null }
                                else { Value::Str(t.to_string()) };
                        current.push(v);
                        token.clear();
                    }
                }
                _ => { token.push(ch); }
            }
        }
        if rows.is_empty() { return Ok(DataBlock::empty()); }
        let ncols = rows[0].len();
        let mut columns: Vec<Column> = (0..ncols).map(|i| {
            let data = rows.iter().map(|r| r.get(i).cloned().unwrap_or(Value::Null)).collect::<Vec<_>>();
            // Infer type from first non-null
            let first = data.iter().find(|v| !matches!(v, Value::Null));
            let col_data = match first {
                Some(Value::Int(_))   => ColumnData::Int64(data.iter().map(|v| if let Value::Int(i) = v { Some(*i) } else { None }).collect()),
                Some(Value::Float(_)) => ColumnData::Float64(data.iter().map(|v| match v { Value::Float(f) => Some(*f), Value::Int(i) => Some(*i as f64), _ => None }).collect()),
                _ => ColumnData::Str(data.iter().map(|v| if let Value::Str(s) = v { Some(s.clone()) } else { None }).collect()),
            };
            Column { name: format!("col{}", i+1), data: col_data }
        }).collect();
        DataBlock::new(columns)
    }

    pub fn get(&self, name: &str) -> Option<&DataBlock> {
        self.tables.get(name).or_else(|| self.mut_tables.get(name))
    }

    pub fn table_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tables.keys().chain(self.mut_tables.keys()).cloned().collect();
        names.sort();
        names.dedup();
        names
    }
}

pub fn execute(sql: &str, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    ctx.query(sql)
}

/// Execute a full Query (with CTEs and UNION ALL).
pub fn execute_query(query: &Query, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    // 1. Register CTEs in an extended context
    let mut local = ctx.clone();
    for cte in &query.ctes {
        let result = execute_select(&cte.body, &local)?;
        local.register(cte.name.clone(), result);
    }

    // 2. Execute main body
    let body = query.body.as_ref()
        .ok_or_else(|| KoreError::InvalidArgument("empty query body".into()))?;
    let mut result = execute_select(body, &local)?;

    // 3. UNION ALL
    for stmt in &query.union_all {
        let other = execute_select(stmt, &local)?;
        result = DataBlock::concat(vec![result, other])?;
    }

    Ok(result)
}

pub fn execute_select(stmt: &SelectStmt, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    // 1. Resolve FROM table (or execute FROM subquery)
    let base_name   = &stmt.from.name;
    let base_alias  = stmt.from.alias.as_deref().unwrap_or(base_name.as_str());

    // FROM (SELECT ...) subquery — execute it first, then use as temp table
    let subq_block: Option<DataBlock> = if let Some(subq) = &stmt.from.subquery {
        Some(execute_select(subq, ctx)?)
    } else {
        None
    };

    let base_ref = if let Some(ref sb) = subq_block {
        sb
    } else {
        ctx.get(base_name)
            .ok_or_else(|| KoreError::InvalidArgument(format!("unknown table: {base_name}")))?
    };

    // Column pruning: for simple queries (no JOIN, no SELECT *) only clone columns
    // actually referenced by projections/WHERE/GROUP BY/ORDER BY.
    // Avoids cloning expensive high-cardinality Str columns (e.g. l_comment) unnecessarily.
    let has_star = stmt.projections.iter().any(|p| matches!(p, Projection::Star));
    let base_block: DataBlock = if !has_star && stmt.joins.is_empty() {
        let needed = used_columns(stmt);
        DataBlock {
            num_rows: base_ref.num_rows,
            columns:  base_ref.columns.iter()
                .filter(|c| {
                    let bare = c.name.rsplit('.').next().unwrap_or(&c.name);
                    needed.contains(bare) || needed.contains(c.name.as_str())
                })
                .cloned()
                .collect(),
        }
    } else {
        base_ref.clone()
    };

    // Prefix column names with alias
    let mut result = prefix_columns(base_block, base_alias);

    // 2. Process JOINs
    for join in &stmt.joins {
        let right_name  = &join.table.name;
        let right_alias = join.table.alias.as_deref().unwrap_or(right_name.as_str());
        let right_block = ctx.get(right_name)
            .ok_or_else(|| KoreError::InvalidArgument(format!("unknown table: {right_name}")))?
            .clone();
        let right_block = prefix_columns(right_block, right_alias);

        let jtype = match join.join_type {
            JoinKind::Inner => JoinType::Inner,
            JoinKind::Left  => JoinType::Left,
            JoinKind::Right => JoinType::Left,   // swap tables for right join
            JoinKind::Full  => JoinType::Full,
        };

        // Resolve join keys — ON clause order is not guaranteed to match left/right tables.
        // Try both assignments and use whichever pairing matches the blocks.
        let (lk, rk) = {
            let a_in_result = find_col_in_block(&join.on.left_col,  &result);
            let a_in_right  = find_col_in_block(&join.on.left_col,  &right_block);
            let b_in_result = find_col_in_block(&join.on.right_col, &result);
            let b_in_right  = find_col_in_block(&join.on.right_col, &right_block);

            if a_in_result.is_some() && b_in_right.is_some() {
                // Natural: left_col in result, right_col in right_block
                (a_in_result.unwrap(), b_in_right.unwrap())
            } else if b_in_result.is_some() && a_in_right.is_some() {
                // Reversed: right_col in result, left_col in right_block
                (b_in_result.unwrap(), a_in_right.unwrap())
            } else if a_in_result.is_some() {
                // Fallback: left_col in result, right_col uses alias
                (a_in_result.unwrap(), resolve_col_name(&join.on.right_col, right_alias))
            } else {
                // Last resort: use alias-based resolution
                (resolve_col_name(&join.on.left_col, base_alias),
                 resolve_col_name(&join.on.right_col, right_alias))
            }
        };

        let cfg = JoinConfig { left_key: lk.clone(), right_key: rk, join_type: jtype };

        if join.join_type == JoinKind::Right {
            result = HashJoin::join(&right_block, &result, &cfg)?;
        } else {
            result = HashJoin::join(&result, &right_block, &cfg)?;
        }
    }

    // 3. WHERE filter
    if let Some(pred) = &stmt.where_clause {
        let resolved = resolve_subqueries(pred, ctx);
        // Decorrelate correlated scalar subqueries: pre-compute GROUP BY, inject threshold columns
        // This converts O(n²) correlated subqueries to O(n) — e.g. Q17, Q20
        let (new_pred, new_block) = decorrelate_scalar_subqueries(&resolved, result, ctx);
        result = filter_block_ctx(new_block, &new_pred, ctx)?;
    }

    // 4. GROUP BY  (or global aggregation if no GROUP BY but has aggregates)
    let has_agg = stmt.projections.iter().any(|p| matches!(p, Projection::Expr { expr: Expr::Agg { .. }, .. }));
    if !stmt.group_by.is_empty() {
        // Materialize any GROUP BY columns that are SELECT expression aliases
        // e.g. GROUP BY l_year where l_year is alias for CASE WHEN ... END
        result = materialize_groupby_aliases(result, &stmt.group_by, &stmt.projections);
        result = group_by_agg(result, &stmt.group_by, &stmt.projections)?;
    } else if has_agg {
        result = global_agg(result, &stmt.projections)?;
    }

    // 4.1 HAVING — filter on aggregated result
    if let Some(having) = &stmt.having {
        result = filter_block(result, having)?;
    }

    // 4.5 Window functions — applied AFTER WHERE/GROUP BY, BEFORE ORDER BY
    let win_projs: Vec<(usize, &Expr, Option<&String>)> = stmt.projections.iter()
        .enumerate()
        .filter_map(|(i, p)| match p {
            Projection::Expr { expr: e @ Expr::Window { .. }, alias } => Some((i, e, alias.as_ref())),
            _ => None,
        })
        .collect();

    if !win_projs.is_empty() {
        for (idx, expr, alias) in &win_projs {
            if let Expr::Window { func, spec } = expr {
                let out_name = alias.map(|a| a.as_str())
                    .unwrap_or("__win")
                    .to_string();
                let win_fn   = ast_to_win_fn(func);
                let part_by  = spec.partition_by.iter()
                    .filter_map(|e| match e { Expr::Col(n) | Expr::QualCol(_, n) => Some(n.clone()), _ => None })
                    .collect::<Vec<_>>();
                let order_by = spec.order_by.iter()
                    .map(|o| WinOrder { col: o.col.clone(), desc: o.desc })
                    .collect::<Vec<_>>();
                result = apply_window(&result, &part_by, &order_by, &win_fn, &out_name)?;
            }
        }
    }

    // 5. Projection — done BEFORE ORDER BY so ORDER BY can reference SELECT aliases
    // (especially important when GROUP BY uses CASE expression aliases)
    let has_order = !stmt.order_by.is_empty();
    if !has_order || !stmt.group_by.is_empty() {
        result = project(result, &stmt.projections)?;
    }

    // 6. ORDER BY — resolve column names, also checking SELECT aliases
    for item in stmt.order_by.iter().rev() {
        let col_raw = resolve_col_name(&item.col, "");
        let col = find_order_col_in_result(&col_raw, &result, &stmt.projections)
            .unwrap_or(col_raw);
        result = sort_block(result, &col, item.desc)?;
    }

    // 7. LIMIT
    if let Some(n) = stmt.limit {
        result = limit_block(result, n as usize);
    }

    // 8. Projection (only if not already done above)
    if has_order && stmt.group_by.is_empty() {
        result = project(result, &stmt.projections)?;
    }

    // 9. DISTINCT — deduplicate rows by row key
    if stmt.distinct && result.num_rows > 1 {
        result = deduplicate(result);
    }

    Ok(result)
}

// ─── Column prefix helper ─────────────────────────────────────────────────────

fn prefix_columns(mut block: DataBlock, alias: &str) -> DataBlock {
    for col in &mut block.columns {
        if !col.name.contains('.') {
            col.name = format!("{}.{}", alias, col.name);
        }
    }
    block
}

fn resolve_col_name(name: &str, default_alias: &str) -> String {
    if name.contains('.') {
        name.to_string()
    } else if default_alias.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", default_alias, name)
    }
}

/// Find the actual column name in `result` for an ORDER BY expression.
/// Checks: exact name, suffix match, prefix+short match, SELECT aliases.
fn find_order_col_in_result(col: &str, result: &DataBlock, projections: &[Projection]) -> Option<String> {
    // 1. Exact column name in result block
    if result.columns.iter().any(|c| c.name == col) {
        return Some(col.to_string());
    }
    // 2. Suffix match: "orders.col" matches column ending with ".orders.col"
    let m = result.columns.iter().len(); let cl = col.len();
    if let Some(c) = result.columns.iter().find(|c| {
        let cn = c.name.len();
        cn > cl && c.name.as_bytes()[cn - cl - 1] == b'.' && &c.name[cn - cl..] == col
    }) { return Some(c.name.clone()); }

    // 3. Prefix+short match: "n1.n_name" matches "n1.nation.n_name"
    let col_short  = col.rsplit('.').next().unwrap_or(col);
    let col_prefix = if col.contains('.') { col.split('.').next() } else { None };
    if let Some(pfx) = col_prefix {
        if let Some(c) = result.columns.iter().find(|c| {
            c.name.starts_with(&format!("{pfx}.")) &&
            c.name.rsplit('.').next().map_or(false, |s| s == col_short)
        }) { return Some(c.name.clone()); }
    }

    // 4. Check SELECT aliases: if SELECT n1.n_name supp_nation, ORDER BY n1.n_name → supp_nation
    for proj in projections {
        if let Projection::Expr { expr, alias: Some(alias) } = proj {
            let expr_col = match expr {
                Expr::Col(c)        => c.as_str(),
                Expr::QualCol(t, c) => {
                    // Check "t.c" matches ORDER BY col
                    let qualified = format!("{}.{}", t, c);
                    if qualified == col || c.as_str() == col_short {
                        // This projection aliases to `alias` — look for alias in result
                        if result.columns.iter().any(|rc| rc.name == alias.as_str() || rc.name.ends_with(&format!(".{}", alias))) {
                            return Some(alias.clone());
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            if expr_col == col || expr_col == col_short {
                if result.columns.iter().any(|rc| rc.name == alias.as_str()) {
                    return Some(alias.clone());
                }
            }
        }
    }

    // 5. Unambiguous bare name
    let matches: Vec<_> = result.columns.iter()
        .filter(|c| c.name.rsplit('.').next().unwrap_or(&c.name) == col_short)
        .collect();
    if matches.len() == 1 { return Some(matches[0].name.clone()); }

    None
}

/// Search for a column in a DataBlock — handles both bare name and "alias.col" forms.
/// Returns the exact column name as it appears in the block.
fn find_col_in_block(bare: &str, block: &DataBlock) -> Option<String> {
    if bare.contains('.') {
        // Already qualified — check it exists
        return if block.columns.iter().any(|c| c.name == bare) { Some(bare.to_string()) } else { None };
    }
    // Try exact match first
    if let Some(col) = block.columns.iter().find(|c| c.name == bare) {
        return Some(col.name.clone());
    }
    // Try "table.bare" match — find a column whose suffix matches bare
    let suffix = format!(".{}", bare);
    block.columns.iter().find(|c| c.name.ends_with(&suffix)).map(|c| c.name.clone())
}

// ─── Filter (WHERE) ───────────────────────────────────────────────────────────

fn filter_block(block: DataBlock, pred: &Expr) -> Result<DataBlock, KoreError> {
    filter_block_ctx(block, pred, &KqlContext::new())
}

/// Filter with context — supports scalar/IN/EXISTS subqueries.
fn filter_block_ctx(block: DataBlock, pred: &Expr, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    // Pre-compute all IN subqueries ONCE into value sets (avoids O(n*m) re-execution per row)
    let pred = precompute_in_subqueries(pred, ctx);
    let pred = &pred;
    let n = block.num_rows;
    let keep: Vec<bool> = if n >= 100_000 {
        use rayon::prelude::*;
        (0..n).into_par_iter().map(|r| eval_bool_ctx(pred, &block, r, ctx)).collect()
    } else {
        (0..n).map(|r| eval_bool_ctx(pred, &block, r, ctx)).collect()
    };
    let indices: Vec<usize> = keep.iter().enumerate()
        .filter_map(|(i, &k)| if k { Some(i) } else { None })
        .collect();
    Ok(block.select_rows(&indices))
}

/// Decorrelate scalar subqueries by pre-computing per-key results.
/// Pattern: WHERE col < (SELECT AGG(col2) FROM t WHERE t.key = outer.key)
/// → Pre-compute GROUP BY key, AGG(col2) → store as "kore_sq_<n>" table in context
/// → Replace ScalarSubquery with Col lookup from that table
///
/// Returns (possibly modified predicate, optional enriched context).
fn decorrelate_scalar_subqueries(
    pred: &Expr,
    outer_block: DataBlock,
    ctx: &KqlContext,
) -> (Expr, DataBlock) {
    let mut block = outer_block;
    let mut counter = 0usize;
    let new_pred = decorrelate_expr(pred, &mut block, ctx, &mut counter);
    (new_pred, block)
}

fn decorrelate_expr(
    expr: &Expr,
    outer_block: &mut DataBlock,
    ctx: &KqlContext,
    counter: &mut usize,
) -> Expr {
    match expr {
        Expr::ScalarSubquery(sq) => {
            // Decorrelate: pre-compute GROUP BY + AGG, inject threshold column
            // Handles both single-key (Q17) and multi-key (Q20) correlations
            // and filters non-correlation WHERE conditions before aggregation
            if sq.group_by.is_empty() {
                if let Some(where_expr) = &sq.where_clause {
                    let inner_table = &sq.from.name;
                    if let Some(inner_block) = ctx.get(inner_table) {
                        // Collect correlation pairs and filter conditions from WHERE
                        let mut corr: Vec<(String, Expr)> = Vec::new(); // (inner_col, outer_expr)
                        let mut filters: Vec<Expr>         = Vec::new();
                        collect_corr_and_filters(where_expr, inner_table, &mut corr, &mut filters);

                        if !corr.is_empty() {
                            if let Some(Projection::Expr { expr: proj_expr, .. }) = sq.projections.first() {
                                // Apply non-correlation filters to inner block first
                                let mut filtered = inner_block.clone();
                                for fc in &filters {
                                    if let Ok(f) = filter_block(filtered, fc) { filtered = f; } else { return expr.clone(); }
                                }

                                // Find the agg source column
                                let agg_col: Option<&Column> = match proj_expr {
                                    Expr::Agg { expr: ie, .. } | Expr::BinOp { left: ie, .. }
                                        if matches!(ie.as_ref(), Expr::Agg{..}) => {
                                        if let Expr::Agg { expr: ie2, .. } = ie.as_ref() {
                                            match ie2.as_ref() { Expr::Col(c)|Expr::QualCol(_,c) => find_col(&filtered, c), _ => None }
                                        } else { None }
                                    }
                                    Expr::Agg { expr: ie, .. } => match ie.as_ref() { Expr::Col(c)|Expr::QualCol(_,c) => find_col(&filtered, c), _ => None },
                                    Expr::BinOp { right: re, .. } => if let Expr::Agg { expr: ie, .. } = re.as_ref() {
                                        match ie.as_ref() { Expr::Col(c)|Expr::QualCol(_,c) => find_col(&filtered, c), _ => None }
                                    } else { None },
                                    _ => None,
                                };

                                // Build composite-key GROUP BY
                                let key_cols: Vec<String> = corr.iter().map(|(ic, _)| {
                                    find_col_in_block(ic, &filtered).unwrap_or_else(|| ic.clone())
                                }).collect();
                                let mut groups: std::collections::HashMap<String, Vec<f64>> =
                                    std::collections::HashMap::new();
                                for row in 0..filtered.num_rows {
                                    let key: String = key_cols.iter().map(|kc| {
                                        match get_cell(&filtered, kc, row) {
                                            ExprVal::Int(i)   => i.to_string(),
                                            ExprVal::Float(f) => format!("{:.6}", f),
                                            ExprVal::Str(s)   => s,
                                            _ => String::new(),
                                        }
                                    }).collect::<Vec<_>>().join(",");
                                    if let Some(col) = agg_col {
                                        let v = match &col.data {
                                            ColumnData::Float64(vs) => vs.get(row).and_then(|x| *x).unwrap_or(0.0),
                                            ColumnData::Int64(vs)   => vs.get(row).and_then(|x| *x).unwrap_or(0) as f64,
                                            _ => 0.0,
                                        };
                                        groups.entry(key).or_default().push(v);
                                    }
                                }

                                let factor = match proj_expr {
                                    Expr::BinOp { left: lf, right: rf, .. } => match (lf.as_ref(), rf.as_ref()) {
                                        (Expr::Float(f),_)|(_, Expr::Float(f)) => *f,
                                        (Expr::Int(i),_)|(_, Expr::Int(i))     => *i as f64, _ => 1.0,
                                    }, _ => 1.0,
                                };
                                let agg_fn = match proj_expr {
                                    Expr::Agg { func, .. } => func.clone(),
                                    Expr::BinOp { left: lf, right: rf, .. } => {
                                        let side = if matches!(lf.as_ref(), Expr::Agg{..}) { lf.as_ref() } else { rf.as_ref() };
                                        if let Expr::Agg { func, .. } = side { func.clone() } else { AggFunc::Sum }
                                    }
                                    _ => AggFunc::Sum,
                                };
                                let mut lookup: std::collections::HashMap<String, f64> =
                                    std::collections::HashMap::new();
                                for (k, vs) in &groups {
                                    let agg = match agg_fn {
                                        AggFunc::Sum   => vs.iter().sum(),
                                        AggFunc::Avg   => if vs.is_empty() { 0.0 } else { vs.iter().sum::<f64>() / vs.len() as f64 },
                                        AggFunc::Count => vs.len() as f64,
                                        AggFunc::Min   => vs.iter().cloned().fold(f64::MAX, f64::min),
                                        AggFunc::Max   => vs.iter().cloned().fold(f64::MIN, f64::max),
                                        _ => 0.0,
                                    };
                                    lookup.insert(k.clone(), agg * factor);
                                }

                                // Inject threshold column
                                let col_name = format!("__decorr_{}__", counter);
                                *counter += 1;
                                let outer_exprs: Vec<Expr> = corr.iter().map(|(_, oe)| oe.clone()).collect();
                                let thresholds: Vec<Option<f64>> = (0..outer_block.num_rows).map(|r| {
                                    let k: String = outer_exprs.iter().map(|oe| {
                                        match eval_expr(oe, outer_block, r) {
                                            ExprVal::Int(i)   => i.to_string(),
                                            ExprVal::Float(f) => format!("{:.6}", f),
                                            ExprVal::Str(s)   => s,
                                            _ => String::new(),
                                        }
                                    }).collect::<Vec<_>>().join(",");
                                    lookup.get(&k).copied()
                                }).collect();
                                outer_block.columns.push(kore_core::Column {
                                    name: col_name.clone(),
                                    data: kore_core::ColumnData::Float64(thresholds),
                                });
                                return Expr::Col(col_name);
                            }
                        }
                    }
                }
            }
            expr.clone()
        }
        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: op.clone(),
            left:  Box::new(decorrelate_expr(left,  outer_block, ctx, counter)),
            right: Box::new(decorrelate_expr(right, outer_block, ctx, counter)),
        },
        Expr::Not(inner) => Expr::Not(Box::new(decorrelate_expr(inner, outer_block, ctx, counter))),
        other => other.clone(),
    }
}

/// Decompose a WHERE clause into correlation conditions (inner.key = outer.expr)
/// and plain filter conditions. The inner_table name is used to identify which
/// side of an equality is the "inner" correlated column.
fn collect_corr_and_filters<'a>(
    expr: &'a Expr,
    inner_table: &str,
    corr: &mut Vec<(String, Expr)>,
    filters: &mut Vec<Expr>,
) {
    match expr {
        Expr::BinOp { op: BinOpKind::And, left, right } => {
            collect_corr_and_filters(left,  inner_table, corr, filters);
            collect_corr_and_filters(right, inner_table, corr, filters);
        }
        Expr::BinOp { op: BinOpKind::Eq, left, right } => {
            // Detect correlation: one side is a qualified col (inner table or alias),
            // the other side is an unqualified col (outer reference) or vice versa.
            let col_name = |e: &Expr| match e {
                Expr::QualCol(_, c) | Expr::Col(c) => c.clone(),
                _ => String::new(),
            };
            // Case 1: QualCol with inner_table qualifier = inner, other = outer
            let left_exact_inner  = matches!(left.as_ref(),  Expr::QualCol(t, _) if t == inner_table);
            let right_exact_inner = matches!(right.as_ref(), Expr::QualCol(t, _) if t == inner_table);
            if left_exact_inner && !right_exact_inner {
                corr.push((col_name(left), *right.clone()));
            } else if right_exact_inner && !left_exact_inner {
                corr.push((col_name(right), *left.clone()));
            } else {
                // Case 2: QualCol(any) vs Col — QualCol is inner (table alias), Col is outer
                let left_is_qual  = matches!(left.as_ref(),  Expr::QualCol(_, _));
                let right_is_qual = matches!(right.as_ref(), Expr::QualCol(_, _));
                let left_is_col   = matches!(left.as_ref(),  Expr::Col(_));
                let right_is_col  = matches!(right.as_ref(), Expr::Col(_));
                if left_is_qual && right_is_col && !right_exact_inner {
                    // QualCol(alias, col) = outer.col → inner.col = outer.col
                    corr.push((col_name(left), *right.clone()));
                } else if right_is_qual && left_is_col && !left_exact_inner {
                    corr.push((col_name(right), *left.clone()));
                } else if left_is_col && right_is_col {
                    // Both unqualified: assume first is inner (best-effort)
                    corr.push((col_name(left), *right.clone()));
                } else {
                    filters.push(expr.clone());
                }
            }
        }
        _ => filters.push(expr.clone()),
    }
}

/// For GROUP BY, materialize any SELECT alias expressions as computed columns.
/// Example: SELECT CASE WHEN ... END l_year — GROUP BY l_year needs "l_year" as a real column.
fn materialize_groupby_aliases(mut block: DataBlock, group_by: &[String], projections: &[Projection]) -> DataBlock {
    for gb_col in group_by {
        // Skip if column already exists in block
        if block.columns.iter().any(|c| c.name == gb_col.as_str()
            || c.name.rsplit('.').next().unwrap_or(&c.name) == gb_col.as_str())
        { continue; }
        // Find a SELECT projection whose alias matches this GROUP BY name
        for proj in projections {
            if let Projection::Expr { expr, alias: Some(alias) } = proj {
                if alias == gb_col {
                    // Evaluate this expression for every row and add as new column
                    let n = block.num_rows;
                    let values: Vec<Option<String>> = (0..n).map(|row| {
                        Some(match eval_expr(expr, &block, row) {
                            ExprVal::Str(s)   => s,
                            ExprVal::Int(i)   => i.to_string(),
                            ExprVal::Float(f) => format!("{f:.4}"),
                            ExprVal::Bool(b)  => b.to_string(),
                            ExprVal::Null     => return None,
                        })
                    }).collect();
                    block.columns.push(kore_core::Column {
                        name: gb_col.clone(),
                        data: kore_core::ColumnData::Str(values),
                    });
                    break;
                }
            }
        }
    }
    block
}

/// Pre-compute non-correlated IN subqueries into value lists.
/// Replaces InSubquery nodes with In{values} so the subquery only runs once.
fn precompute_in_subqueries(expr: &Expr, ctx: &KqlContext) -> Expr {
    match expr {
        Expr::InSubquery { expr: e, subquery, negated } => {
            // Execute subquery once, collect all values
            let inner_ctx = ctx.clone();
            if let Ok(result) = execute_select(subquery, &inner_ctx) {
                if !result.columns.is_empty() {
                    let values: Vec<Expr> = (0..result.num_rows)
                        .filter_map(|r| match result.columns[0].data.get_value(r) {
                            Value::Int(i)   => Some(Expr::Int(i)),
                            Value::Float(f) => Some(Expr::Float(f)),
                            Value::Str(s)   => Some(Expr::Str(s)),
                            _ => None,
                        })
                        .collect();
                    return Expr::In { expr: e.clone(), values, negated: *negated };
                }
            }
            expr.clone()
        }
        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: op.clone(),
            left:  Box::new(precompute_in_subqueries(left,  ctx)),
            right: Box::new(precompute_in_subqueries(right, ctx)),
        },
        Expr::Not(inner) => Expr::Not(Box::new(precompute_in_subqueries(inner, ctx))),
        other => other.clone(),
    }
}

/// Evaluate a boolean expression with context (supports subqueries).
fn eval_bool_ctx(expr: &Expr, block: &DataBlock, row: usize, ctx: &KqlContext) -> bool {
    match eval_expr_ctx(expr, block, row, ctx) {
        ExprVal::Bool(b) => b,
        ExprVal::Int(i)  => i != 0,
        _                => false,
    }
}

/// Evaluate expression with context — handles subquery variants.
fn eval_expr_ctx(expr: &Expr, block: &DataBlock, row: usize, ctx: &KqlContext) -> ExprVal {
    // Outer block's table alias (e.g. "m1", "o1", "memories")
    let outer_alias: Option<String> = block.columns.first().and_then(|c| {
        let s = c.name.as_str();
        if s.contains('.') { s.split('.').next().map(|a| a.to_string()) } else { None }
    });

    // Build row context for subquery evaluation.
    // For correlated subqueries: injects outer row so inner can access outer.col
    // For non-correlated: skips injection if inner FROM = outer alias (avoids 1-row override)
    let make_row_ctx = |inner_from: &str| {
        let mut row_ctx = ctx.clone();
        if let Some(ref alias) = outer_alias {
            // Inject if: alias not in ctx yet, OR inner uses a DIFFERENT table
            if row_ctx.get(alias.as_str()).is_none() || inner_from != alias.as_str() {
                let single = block.select_rows(&[row]);
                row_ctx.register(alias.as_str(), single);
            }
        }
        row_ctx
    };

    match expr {
        // Scalar subquery — per-row for correlated queries
        Expr::ScalarSubquery(stmt) => {
            let row_ctx = make_row_ctx(&stmt.from.name);
            match execute_select(stmt, &row_ctx) {
                Ok(result) if result.num_rows > 0 && !result.columns.is_empty() => {
                    match result.columns[0].data.get_value(0) {
                        Value::Int(i)   => ExprVal::Float(i as f64),
                        Value::Float(f) => ExprVal::Float(f),
                        Value::Str(s)   => ExprVal::Str(s),
                        Value::Bool(b)  => ExprVal::Bool(b),
                        Value::Null     => ExprVal::Null,
                    }
                }
                _ => ExprVal::Null,
            }
        }

        // IN (SELECT ...) subquery
        Expr::InSubquery { expr: e, subquery, negated } => {
            let lhs = eval_expr_ctx(e, block, row, ctx);
            let lhs_str = match &lhs {
                ExprVal::Int(i)   => i.to_string(),
                ExprVal::Float(f) => format!("{f:.10}"),
                ExprVal::Str(s)   => s.clone(),
                ExprVal::Bool(b)  => b.to_string(),
                ExprVal::Null     => return ExprVal::Bool(*negated),
            };
            match execute_select(subquery, &make_row_ctx(&subquery.from.name)) {
                Ok(result) => {
                    if !result.columns.is_empty() {
                        let found = (0..result.num_rows).any(|r| {
                            match result.columns[0].data.get_value(r) {
                                Value::Int(i)   => i.to_string() == lhs_str,
                                Value::Float(f) => format!("{f:.10}") == lhs_str,
                                Value::Str(s)   => s == lhs_str,
                                Value::Bool(b)  => b.to_string() == lhs_str,
                                Value::Null     => false,
                            }
                        });
                        ExprVal::Bool(if *negated { !found } else { found })
                    } else {
                        ExprVal::Bool(*negated)
                    }
                }
                Err(_) => {
                    ExprVal::Bool(*negated)
                }
            }
        }

        // EXISTS (SELECT ...)
        Expr::Exists { subquery, negated } => {
            let row_ctx = make_row_ctx(&subquery.from.name);
            match execute_select(subquery, &row_ctx) {
                Ok(result) => ExprVal::Bool(if *negated { result.num_rows == 0 } else { result.num_rows > 0 }),
                Err(_)     => ExprVal::Bool(*negated),
            }
        }

        // For all other expressions, fall back to the standard evaluator —
        // EXCEPT for expressions that might contain QualCol outer references.
        // Handle BinOp, Not, IsNull/IsNotNull recursively with context.
        Expr::BinOp { op, left, right } => {
            let lv = eval_expr_ctx(left,  block, row, ctx);
            let rv = eval_expr_ctx(right, block, row, ctx);
            eval_binop(op, lv, rv)
        }
        Expr::Not(e) => match eval_expr_ctx(e, block, row, ctx) {
            ExprVal::Bool(b) => ExprVal::Bool(!b),
            _                => ExprVal::Bool(false),
        },
        Expr::IsNull(e) => match eval_expr_ctx(e, block, row, ctx) {
            ExprVal::Null => ExprVal::Bool(true),
            _             => ExprVal::Bool(false),
        },
        Expr::IsNotNull(e) => match eval_expr_ctx(e, block, row, ctx) {
            ExprVal::Null => ExprVal::Bool(false),
            _             => ExprVal::Bool(true),
        },
        // QualCol: try block first, then outer table in ctx (enables correlated subqueries)
        Expr::QualCol(table, col) => {
            let full = format!("{}.{}", table, col);
            let v = get_cell(block, &full, row);
            if matches!(v, ExprVal::Null) {
                // Try outer table in ctx (correlated subquery: m1.kind where m1 is registered)
                if let Some(outer_block) = ctx.get(table.as_str()) {
                    if outer_block.num_rows > 0 {
                        let val = get_cell(outer_block, col.as_str(), 0);
                        if row == 0 { eprintln!("[QualCol] {}.{} → {:?} (from ctx, {} rows)", table, col, val, outer_block.num_rows); }
                        return val;
                    }
                }
                if row == 0 { eprintln!("[QualCol] {}.{} → Null (not in block or ctx)", table, col); }
            }
            v
        }
        // Col: standard block lookup
        other => eval_expr(other, block, row),
    }
}

/// Pre-resolve non-correlated subqueries: replace ScalarSubquery with literal float/str.
/// Correlated subqueries remain as-is (evaluated per-row in eval_expr_ctx).
fn expr_type_name(e: &Expr) -> &'static str {
    match e {
        Expr::ScalarSubquery(_)  => "ScalarSubquery",
        Expr::InSubquery { .. }  => "InSubquery",
        Expr::Exists { .. }      => "Exists",
        Expr::BinOp { .. }       => "BinOp",
        Expr::Not(_)             => "Not",
        Expr::Col(_)             => "Col",
        Expr::Float(_)           => "Float",
        Expr::In { .. }          => "In",
        _                        => "Other",
    }
}

fn resolve_subqueries(expr: &Expr, ctx: &KqlContext) -> Expr {
    match expr {
        Expr::ScalarSubquery(stmt) => {
            // Try to evaluate — if succeeds and not correlated, replace with literal
            match execute_select(stmt, ctx) {
                Ok(result) => {
                    if result.num_rows > 0 && !result.columns.is_empty() {
                        let v = result.columns[0].data.get_value(0);
                        match v {
                            Value::Int(i)   => Expr::Float(i as f64),
                            Value::Float(f) => Expr::Float(f),
                            Value::Str(s)   => Expr::Str(s),
                            Value::Bool(b)  => Expr::Bool(b),
                            _               => expr.clone(),
                        }
                    } else { expr.clone() }
                }
                Err(e) => {
                    eprintln!("[resolve_subqueries] subquery error: {e}");
                    expr.clone()
                }
            }
        }
        // Recurse into BinOp
        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: op.clone(),
            left:  Box::new(resolve_subqueries(left, ctx)),
            right: Box::new(resolve_subqueries(right, ctx)),
        },
        Expr::Not(e) => Expr::Not(Box::new(resolve_subqueries(e, ctx))),
        // Everything else: pass through
        other => other.clone(),
    }
}

/// Evaluate a predicate over an entire DataBlock, returning a bitmask.
/// Processes simple column comparisons column-at-a-time (LLVM auto-vectorizes).
fn eval_batch(expr: &Expr, block: &DataBlock) -> Vec<bool> {
    let n = block.num_rows;

    // Helper: find a column by name or suffix match (defined as function below)
    // Helper: extract the column name from a Col/QualCol expr (inline)
    macro_rules! col_name_of { ($e:expr) => {{ let e: &Expr = &**$e; match e {
        Expr::Col(n)        => Some(n.clone()),
        Expr::QualCol(_, n) => Some(n.clone()),
        _ => None,
    }}} }
    macro_rules! lit_f64 { ($e:expr) => {{ let e: &Expr = &**$e; match e {
        Expr::Float(f) => Some(*f), Expr::Int(i) => Some(*i as f64), _ => None,
    }}} }
    macro_rules! lit_i64 { ($e:expr) => {{ let e: &Expr = &**$e; match e {
        Expr::Int(i) => Some(*i), _ => None,
    }}} }
    macro_rules! lit_str { ($e:expr) => {{ let e: &Expr = &**$e; match e {
        Expr::Str(s) => Some(s.as_str()), _ => None,
    }}} }

    match expr {
        // ── Boolean literals ─────────────────────────────────────────────────
        Expr::Bool(true)  => vec![true;  n],
        Expr::Bool(false) => vec![false; n],

        // ── IS NULL / IS NOT NULL ────────────────────────────────────────────
        Expr::IsNull(inner) => {
            if let Some(cname) = col_name_of!(inner) {
                if let Some(col) = find_col(block, &cname) {
                    return match &col.data {
                        ColumnData::Int64(v)   => v.iter().map(|x| x.is_none()).collect(),
                        ColumnData::Float64(v) => v.iter().map(|x| x.is_none()).collect(),
                        ColumnData::Bool(v)    => v.iter().map(|x| x.is_none()).collect(),
                        ColumnData::Str(v)     => v.iter().map(|x| x.is_none()).collect(),
                        ColumnData::StrDict { codes, .. } => codes.iter().map(|&c| c == u8::MAX).collect(),
                    };
                }
            }
            (0..n).map(|r| eval_bool(expr, block, r)).collect()
        }
        Expr::IsNotNull(inner) => {
            if let Some(cname) = col_name_of!(inner) {
                if let Some(col) = find_col(block, &cname) {
                    return match &col.data {
                        ColumnData::Int64(v)   => v.iter().map(|x| x.is_some()).collect(),
                        ColumnData::Float64(v) => v.iter().map(|x| x.is_some()).collect(),
                        ColumnData::Bool(v)    => v.iter().map(|x| x.is_some()).collect(),
                        ColumnData::Str(v)     => v.iter().map(|x| x.is_some()).collect(),
                        ColumnData::StrDict { codes, .. } => codes.iter().map(|&c| c != u8::MAX).collect(),
                    };
                }
            }
            (0..n).map(|r| eval_bool(expr, block, r)).collect()
        }

        // ── NOT ──────────────────────────────────────────────────────────────
        Expr::Not(inner) => {
            let mut v = eval_batch(inner, block);
            v.iter_mut().for_each(|b| *b = !*b);
            v
        }

        // ── AND / OR ─────────────────────────────────────────────────────────
        Expr::BinOp { op: BinOpKind::And, left, right } => {
            let lb = eval_batch(left,  block);
            let rb = eval_batch(right, block);
            // Tight loop — LLVM vectorizes this to SIMD AND
            lb.iter().zip(rb.iter()).map(|(&a, &b)| a && b).collect()
        }
        Expr::BinOp { op: BinOpKind::Or, left, right } => {
            let lb = eval_batch(left,  block);
            let rb = eval_batch(right, block);
            lb.iter().zip(rb.iter()).map(|(&a, &b)| a || b).collect()
        }

        // ── Column BinOp literal  (the hot path for TPC-H filters) ──────────
        Expr::BinOp { op, left, right } => {
            // Determine which side is col and which is literal
            let (cname, flip) = if let Some(c) = col_name_of!(left)  { (c, false) }
                                 else if let Some(c) = col_name_of!(right) { (c, true) }
                                 else { return (0..n).map(|r| eval_bool(expr, block, r)).collect(); };

            let lit_expr = if flip { left } else { right };

            if let Some(col) = find_col(block, &cname) {
                // String equality
                if let Some(s) = lit_str!(lit_expr) {
                    if let ColumnData::Str(v) = &col.data {
                        return match op {
                            BinOpKind::Eq => v.iter().map(|x| x.as_deref() == Some(s)).collect(),
                            BinOpKind::Ne => v.iter().map(|x| x.as_deref() != Some(s)).collect(),
                            _ => (0..n).map(|r| eval_bool(expr, block, r)).collect(),
                        };
                    }
                }

                // Numeric comparisons — column-at-a-time
                if let Some(threshold) = lit_f64!(lit_expr) {
                    let cmp = |col_val: f64, thresh: f64, op: &BinOpKind, flip: bool| -> bool {
                        let (a, b) = if flip { (thresh, col_val) } else { (col_val, thresh) };
                        match op {
                            BinOpKind::Gt => a > b,  BinOpKind::Ge => a >= b,
                            BinOpKind::Lt => a < b,  BinOpKind::Le => a <= b,
                            BinOpKind::Eq => (a - b).abs() < 1e-10,
                            BinOpKind::Ne => (a - b).abs() >= 1e-10,
                            _ => false,
                        }
                    };
                    return match &col.data {
                        ColumnData::Float64(v) => v.iter().map(|x|
                            x.map(|f| cmp(f, threshold, op, flip)).unwrap_or(false)
                        ).collect(),
                        ColumnData::Int64(v) => v.iter().map(|x|
                            x.map(|i| cmp(i as f64, threshold, op, flip)).unwrap_or(false)
                        ).collect(),
                        _ => (0..n).map(|r| eval_bool(expr, block, r)).collect(),
                    };
                }

                // Integer literal (avoids float cast for integer columns)
                if let Some(threshold) = lit_i64!(lit_expr) {
                    if let ColumnData::Int64(v) = &col.data {
                        let cmp = |col_val: i64, thresh: i64, op: &BinOpKind, flip: bool| -> bool {
                            let (a, b) = if flip { (thresh, col_val) } else { (col_val, thresh) };
                            match op {
                                BinOpKind::Gt => a > b,  BinOpKind::Ge => a >= b,
                                BinOpKind::Lt => a < b,  BinOpKind::Le => a <= b,
                                BinOpKind::Eq => a == b, BinOpKind::Ne => a != b,
                                _ => false,
                            }
                        };
                        return v.iter().map(|x|
                            x.map(|i| cmp(i, threshold, op, flip)).unwrap_or(false)
                        ).collect();
                    }
                }
            }
            // Fallback
            (0..n).map(|r| eval_bool(expr, block, r)).collect()
        }

        // ── Everything else: row-at-a-time fallback ──────────────────────────
        _ => (0..n).map(|row| eval_bool(expr, block, row)).collect(),
    }
}

fn eval_bool(expr: &Expr, block: &DataBlock, row: usize) -> bool {
    match eval_expr(expr, block, row) {
        ExprVal::Bool(b) => b,
        _                => false,
    }
}

#[derive(Debug, Clone)]
enum ExprVal {
    Int(i64), Float(f64), Str(String), Bool(bool), Null,
}

fn eval_expr(expr: &Expr, block: &DataBlock, row: usize) -> ExprVal {
    match expr {
        Expr::Int(n)   => ExprVal::Int(*n),
        Expr::Float(f) => ExprVal::Float(*f),
        Expr::Str(s)   => ExprVal::Str(s.clone()),
        Expr::Bool(b)  => ExprVal::Bool(*b),
        Expr::Not(e)   => match eval_expr(e, block, row) {
            ExprVal::Bool(b) => ExprVal::Bool(!b),
            _                => ExprVal::Bool(false),
        },
        Expr::Col(_) | Expr::QualCol(_, _) => {
            let full = match expr {
                Expr::QualCol(t, c) => format!("{}.{}", t, c),
                Expr::Col(n)        => n.clone(),
                _                   => unreachable!(),
            };
            get_cell(block, &full, row)
        }
        Expr::BinOp { op, left, right } => {
            let lv = eval_expr(left,  block, row);
            let rv = eval_expr(right, block, row);
            eval_binop(op, lv, rv)
        }
        Expr::IsNull(e) => match eval_expr(e, block, row) {
            ExprVal::Null => ExprVal::Bool(true),
            _             => ExprVal::Bool(false),
        },
        Expr::IsNotNull(e) => match eval_expr(e, block, row) {
            ExprVal::Null => ExprVal::Bool(false),
            _             => ExprVal::Bool(true),
        },
        Expr::Agg { .. }    => ExprVal::Null,
        Expr::Window { .. } => ExprVal::Null,
        Expr::Star          => ExprVal::Null,
        Expr::Null          => ExprVal::Null,
        // Subquery variants — require context; return Null when evaluated without context
        Expr::ScalarSubquery(_) => ExprVal::Null,
        Expr::InSubquery { negated, .. } => ExprVal::Bool(*negated),
        Expr::Exists { negated, .. }     => ExprVal::Bool(*negated),
        // ── CASE WHEN ─────────────────────────────────────────────────────
        Expr::Case { operand, branches, else_val } => {
            match operand {
                None => {
                    // Searched: CASE WHEN cond THEN val ...
                    for (cond, val) in branches {
                        if eval_bool(cond, block, row) {
                            return eval_expr(val, block, row);
                        }
                    }
                }
                Some(op_expr) => {
                    // Simple: CASE expr WHEN literal THEN val ...
                    let lhs = eval_expr(op_expr, block, row);
                    for (cond, val) in branches {
                        let rhs = eval_expr(cond, block, row);
                        let eq = match (&lhs, &rhs) {
                            (ExprVal::Int(a),   ExprVal::Int(b))   => a == b,
                            (ExprVal::Float(a), ExprVal::Float(b)) => (a-b).abs() < 1e-10,
                            (ExprVal::Str(a),   ExprVal::Str(b))   => a == b,
                            (ExprVal::Bool(a),  ExprVal::Bool(b))  => a == b,
                            _ => false,
                        };
                        if eq { return eval_expr(val, block, row); }
                    }
                }
            }
            else_val.as_ref().map(|e| eval_expr(e, block, row)).unwrap_or(ExprVal::Null)
        }
        // ── LIKE ──────────────────────────────────────────────────────────
        Expr::Like { expr: e, pattern, negated } => {
            let sv = eval_expr(e, block, row);
            let pv = eval_expr(pattern, block, row);
            let matches = match (sv, pv) {
                (ExprVal::Str(s), ExprVal::Str(p)) => like_match(&s, &p),
                _ => false,
            };
            ExprVal::Bool(if *negated { !matches } else { matches })
        }
        // ── IN ────────────────────────────────────────────────────────────
        Expr::In { expr: e, values, negated } => {
            let lv = eval_expr(e, block, row);
            let found = values.iter().any(|v| {
                let rv = eval_expr(v, block, row);
                match (&lv, &rv) {
                    (ExprVal::Int(a),   ExprVal::Int(b))   => a == b,
                    (ExprVal::Float(a), ExprVal::Float(b)) => (a-b).abs() < 1e-10,
                    (ExprVal::Str(a),   ExprVal::Str(b))   => a == b,
                    (ExprVal::Bool(a),  ExprVal::Bool(b))  => a == b,
                    _ => false,
                }
            });
            ExprVal::Bool(if *negated { !found } else { found })
        }
        // ── BETWEEN ───────────────────────────────────────────────────────
        Expr::Between { expr: e, low, high, negated } => {
            let v  = eval_expr(e, block, row);
            let lo = eval_expr(low, block, row);
            let hi = eval_expr(high, block, row);
            let in_range = match (&v, &lo, &hi) {
                (ExprVal::Int(v),   ExprVal::Int(lo),   ExprVal::Int(hi))   => v >= lo && v <= hi,
                (ExprVal::Float(v), ExprVal::Float(lo), ExprVal::Float(hi)) => v >= lo && v <= hi,
                (ExprVal::Str(v),   ExprVal::Str(lo),   ExprVal::Str(hi))   => v.as_str() >= lo.as_str() && v.as_str() <= hi.as_str(),
                _ => false,
            };
            ExprVal::Bool(if *negated { !in_range } else { in_range })
        }
        // ── SCALAR FUNCTIONS ──────────────────────────────────────────────
        Expr::FuncCall { name, args } => eval_func(name, args, block, row),
    }
}

// ─── Scalar function evaluation ───────────────────────────────────────────────

fn eval_func(name: &str, args: &[Expr], block: &DataBlock, row: usize) -> ExprVal {
    // Helper macro: unwrap Option or return Null
    macro_rules! need {
        ($e:expr) => { match $e { Some(v) => v, None => return ExprVal::Null } };
    }
    let arg = |i: usize| args.get(i).map(|e| eval_expr(e, block, row)).unwrap_or(ExprVal::Null);
    let arg_str = |i: usize| match arg(i) { ExprVal::Str(s) => Some(s), _ => None };
    let arg_f64 = |i: usize| to_f64(&arg(i));

    match name {
        // ── String functions ────────────────────────────────────────────────
        "UPPER" => arg_str(0).map(|s| ExprVal::Str(s.to_uppercase())).unwrap_or(ExprVal::Null),
        "LOWER" => arg_str(0).map(|s| ExprVal::Str(s.to_lowercase())).unwrap_or(ExprVal::Null),
        "TRIM"  => arg_str(0).map(|s| ExprVal::Str(s.trim().to_string())).unwrap_or(ExprVal::Null),
        "LTRIM" => arg_str(0).map(|s| ExprVal::Str(s.trim_start().to_string())).unwrap_or(ExprVal::Null),
        "RTRIM" => arg_str(0).map(|s| ExprVal::Str(s.trim_end().to_string())).unwrap_or(ExprVal::Null),
        // LEFT(str, n) and RIGHT(str, n)
        "LEFT"  => {
            let s = need!(arg_str(0));
            let n = arg_f64(1).unwrap_or(0.0) as usize;
            ExprVal::Str(s.chars().take(n).collect())
        }
        "RIGHT" => {
            let s = need!(arg_str(0));
            let n = arg_f64(1).unwrap_or(0.0) as usize;
            let chars: Vec<char> = s.chars().collect();
            let start = chars.len().saturating_sub(n);
            ExprVal::Str(chars[start..].iter().collect())
        }
        "LENGTH" | "LEN" | "CHAR_LENGTH" => {
            arg_str(0).map(|s| ExprVal::Int(s.chars().count() as i64)).unwrap_or(ExprVal::Null)
        }
        "REVERSE" => arg_str(0).map(|s| ExprVal::Str(s.chars().rev().collect())).unwrap_or(ExprVal::Null),
        "SUBSTR" | "SUBSTRING" => {
            let s = need!(arg_str(0));
            let start = (arg_f64(1).unwrap_or(1.0) as i64 - 1).max(0) as usize;
            let len   = args.get(2).map(|_| arg_f64(2).unwrap_or(0.0) as usize);
            let chars: Vec<char> = s.chars().collect();
            let slice: String = match len {
                Some(l) => chars.iter().skip(start).take(l).collect(),
                None    => chars.iter().skip(start).collect(),
            };
            ExprVal::Str(slice)
        }
        "REPLACE" => {
            let s    = need!(arg_str(0));
            let from = arg_str(1).unwrap_or_default();
            let to   = arg_str(2).unwrap_or_default();
            ExprVal::Str(s.replace(&from, &to))
        }
        "CONCAT" => {
            let parts: String = args.iter()
                .map(|a| match eval_expr(a, block, row) { ExprVal::Str(s) => s, v => format!("{:?}", v) })
                .collect();
            ExprVal::Str(parts)
        }
        "REPEAT" => {
            let s = arg_str(0).unwrap_or_default();
            let n = arg_f64(1).unwrap_or(0.0) as usize;
            ExprVal::Str(s.repeat(n))
        }
        "LPAD" => {
            let s   = arg_str(0).unwrap_or_default();
            let len = arg_f64(1).unwrap_or(0.0) as usize;
            let pad = arg_str(2).unwrap_or_else(|| " ".into());
            if s.len() >= len { return ExprVal::Str(s); }
            let fill: String = pad.chars().cycle().take(len - s.len()).collect();
            ExprVal::Str(format!("{fill}{s}"))
        }
        "RPAD" => {
            let s   = arg_str(0).unwrap_or_default();
            let len = arg_f64(1).unwrap_or(0.0) as usize;
            let pad = arg_str(2).unwrap_or_else(|| " ".into());
            if s.len() >= len { return ExprVal::Str(s); }
            let fill: String = pad.chars().cycle().take(len - s.len()).collect();
            ExprVal::Str(format!("{s}{fill}"))
        }
        // ── Math functions ──────────────────────────────────────────────────
        "ABS"   => match arg(0) {
            ExprVal::Int(i)   => ExprVal::Int(i.abs()),
            ExprVal::Float(f) => ExprVal::Float(f.abs()),
            _ => ExprVal::Null,
        },
        "ROUND" => {
            let f = need!(arg_f64(0));
            let dp = arg_f64(1).unwrap_or(0.0) as u32;
            let m  = 10f64.powi(dp as i32);
            ExprVal::Float((f * m).round() / m)
        }
        "FLOOR" => arg_f64(0).map(|f| ExprVal::Float(f.floor())).unwrap_or(ExprVal::Null),
        "CEIL" | "CEILING" => arg_f64(0).map(|f| ExprVal::Float(f.ceil())).unwrap_or(ExprVal::Null),
        "SQRT"  => arg_f64(0).map(|f| ExprVal::Float(f.sqrt())).unwrap_or(ExprVal::Null),
        "POWER" | "POW" => {
            let b = need!(arg_f64(0));
            let e = need!(arg_f64(1));
            ExprVal::Float(b.powf(e))
        }
        "LOG"   => arg_f64(0).map(|f| ExprVal::Float(f.ln())).unwrap_or(ExprVal::Null),
        "LOG10" => arg_f64(0).map(|f| ExprVal::Float(f.log10())).unwrap_or(ExprVal::Null),
        "EXP"   => arg_f64(0).map(|f| ExprVal::Float(f.exp())).unwrap_or(ExprVal::Null),
        "MOD"   => {
            let a = need!(arg_f64(0));
            let b = need!(arg_f64(1));
            ExprVal::Float(a % b)
        }
        // ── Null-handling ───────────────────────────────────────────────────
        "COALESCE" | "NVL" | "IFNULL" | "ISNULL" => {
            for a in args {
                let v = eval_expr(a, block, row);
                if !matches!(v, ExprVal::Null) { return v; }
            }
            ExprVal::Null
        }
        "NULLIF" => {
            let a = arg(0);
            let b = arg(1);
            let eq = match (&a, &b) {
                (ExprVal::Int(x),   ExprVal::Int(y))   => x == y,
                (ExprVal::Float(x), ExprVal::Float(y)) => (x - y).abs() < 1e-10,
                (ExprVal::Str(x),   ExprVal::Str(y))   => x == y,
                (ExprVal::Bool(x),  ExprVal::Bool(y))  => x == y,
                _ => false,
            };
            if eq { ExprVal::Null } else { a }
        }
        // ── Cast ────────────────────────────────────────────────────────────
        "CAST" => {
            let val = arg(0);
            // arg(1) is the type keyword parsed as a Col
            let ty  = match args.get(1) {
                Some(Expr::Col(t)) => t.to_ascii_uppercase(),
                _                  => return val,
            };
            match ty.as_str() {
                "INT" | "INTEGER" | "BIGINT" => match val {
                    ExprVal::Float(f) => ExprVal::Int(f as i64),
                    ExprVal::Str(s)   => s.trim().parse::<i64>().map(ExprVal::Int).unwrap_or(ExprVal::Null),
                    other             => other,
                },
                "FLOAT" | "DOUBLE" | "REAL" | "NUMERIC" | "DECIMAL" => match val {
                    ExprVal::Int(i)   => ExprVal::Float(i as f64),
                    ExprVal::Str(s)   => s.trim().parse::<f64>().map(ExprVal::Float).unwrap_or(ExprVal::Null),
                    other             => other,
                },
                "VARCHAR" | "TEXT" | "STRING" | "CHAR" => ExprVal::Str(match val {
                    ExprVal::Int(i)   => i.to_string(),
                    ExprVal::Float(f) => f.to_string(),
                    ExprVal::Bool(b)  => b.to_string(),
                    ExprVal::Str(s)   => s,
                    ExprVal::Null     => return ExprVal::Null,
                }),
                "BOOLEAN" | "BOOL" => ExprVal::Bool(match val {
                    ExprVal::Int(i)   => i != 0,
                    ExprVal::Float(f) => f != 0.0,
                    ExprVal::Str(s)   => matches!(s.to_lowercase().as_str(), "true" | "1" | "yes"),
                    ExprVal::Bool(b)  => b,
                    ExprVal::Null     => return ExprVal::Null,
                }),
                _ => val,
            }
        }
        // ── Type-check predicates ────────────────────────────────────────────
        "ISNUMERIC" => ExprVal::Bool(to_f64(&arg(0)).is_some()),
        "IIF" => {
            if eval_bool(&args[0], block, row) { arg(1) } else { arg(2) }
        }
        // ── Fallthrough ─────────────────────────────────────────────────────
        _ => ExprVal::Null,
    }
}

fn get_cell(block: &DataBlock, col_name: &str, row: usize) -> ExprVal {    // Try exact match, then suffix match (for qualified names)
    let col = block.columns.iter().find(|c| {
        c.name == col_name || {
            let cn = c.name.len(); let nm = col_name.len();
            cn > nm && c.name.as_bytes()[cn - nm - 1] == b'.' && &c.name[cn - nm..] == col_name
        }
    });
    match col {
        None => ExprVal::Null,
        Some(c) => match &c.data {
            ColumnData::Int64(v)   => v.get(row).and_then(|x| x.as_ref()).map(|&i| ExprVal::Int(i)).unwrap_or(ExprVal::Null),
            ColumnData::Float64(v) => v.get(row).and_then(|x| x.as_ref()).map(|&f| ExprVal::Float(f)).unwrap_or(ExprVal::Null),
            ColumnData::Bool(v)    => v.get(row).and_then(|x| x.as_ref()).map(|&b| ExprVal::Bool(b)).unwrap_or(ExprVal::Null),
            ColumnData::Str(v)     => v.get(row).and_then(|x| x.as_ref()).map(|s| ExprVal::Str(s.clone())).unwrap_or(ExprVal::Null),
            ColumnData::StrDict { codes, dict } => {
                let c = codes.get(row).copied().unwrap_or(u8::MAX);
                if c == u8::MAX { ExprVal::Null } else { dict.get(c as usize).map(|s| ExprVal::Str(s.clone())).unwrap_or(ExprVal::Null) }
            }
        }
    }
}

fn eval_binop(op: &BinOpKind, l: ExprVal, r: ExprVal) -> ExprVal {
    // Boolean short-circuits
    if let (BinOpKind::And, ExprVal::Bool(lb), ExprVal::Bool(rb)) = (op, &l, &r) {
        return ExprVal::Bool(*lb && *rb);
    }
    if let (BinOpKind::Or, ExprVal::Bool(lb), ExprVal::Bool(rb)) = (op, &l, &r) {
        return ExprVal::Bool(*lb || *rb);
    }

    // Numeric comparison / arithmetic
    let lf = to_f64(&l);
    let rf = to_f64(&r);

    if let (Some(lv), Some(rv)) = (lf, rf) {
        return match op {
            BinOpKind::Eq  => ExprVal::Bool((lv - rv).abs() < 1e-10),
            BinOpKind::Ne  => ExprVal::Bool((lv - rv).abs() >= 1e-10),
            BinOpKind::Lt  => ExprVal::Bool(lv < rv),
            BinOpKind::Le  => ExprVal::Bool(lv <= rv),
            BinOpKind::Gt  => ExprVal::Bool(lv > rv),
            BinOpKind::Ge  => ExprVal::Bool(lv >= rv),
            BinOpKind::Add => ExprVal::Float(lv + rv),
            BinOpKind::Sub => ExprVal::Float(lv - rv),
            BinOpKind::Mul => ExprVal::Float(lv * rv),
            BinOpKind::Div => ExprVal::Float(lv / rv),
            BinOpKind::Mod => ExprVal::Float(lv % rv),
            _ => ExprVal::Bool(false),
        };
    }

    // String comparison
    if let (ExprVal::Str(ls), ExprVal::Str(rs)) = (&l, &r) {
        return match op {
            BinOpKind::Eq => ExprVal::Bool(ls == rs),
            BinOpKind::Ne => ExprVal::Bool(ls != rs),
            BinOpKind::Lt => ExprVal::Bool(ls < rs),
            BinOpKind::Le => ExprVal::Bool(ls <= rs),
            BinOpKind::Gt => ExprVal::Bool(ls > rs),
            BinOpKind::Ge => ExprVal::Bool(ls >= rs),
            _ => ExprVal::Null,
        };
    }

    ExprVal::Null
}

fn to_f64(v: &ExprVal) -> Option<f64> {
    match v {
        ExprVal::Int(i)   => Some(*i as f64),
        ExprVal::Float(f) => Some(*f),
        ExprVal::Bool(b)  => Some(if *b { 1.0 } else { 0.0 }),
        _                 => None,
    }
}

// ─── Sort ─────────────────────────────────────────────────────────────────────

fn sort_block(block: DataBlock, col: &str, desc: bool) -> Result<DataBlock, KoreError> {
    // Use DataBlock::sort_by which uses a Schwartzian transform (cache-friendly,
    // avoids calling get_cell() twice per comparison in the comparator).
    let col_short  = col.rsplit('.').next().unwrap_or(col); // bare name e.g. "n_name"
    let col_prefix = if col.contains('.') { col.split('.').next() } else { None }; // e.g. "n1"
    let col_name = block.columns.iter()
        .find(|c| c.name == col || {
            // exact suffix match: "table.col" → column ends with ".table.col"
            let cn = c.name.len(); let m = col.len();
            cn > m && c.name.as_bytes()[cn - m - 1] == b'.' && &c.name[cn - m..] == col
        })
        .or_else(|| {
            // Qualified prefix match: "n1.n_name" matches "n1.nation.n_name"
            // — starts with "n1." and ends with "n_name"
            if let Some(pfx) = col_prefix {
                block.columns.iter().find(|c| {
                    let starts = c.name.starts_with(&format!("{pfx}."));
                    let ends   = c.name.rsplit('.').next().map_or(false, |s| s == col_short);
                    starts && ends
                })
            } else { None }
        })
        .or_else(|| {
            // bare name match (unambiguous if only one column has this short name)
            let matches: Vec<_> = block.columns.iter()
                .filter(|c| c.name.rsplit('.').next().unwrap_or(&c.name) == col_short)
                .collect();
            if matches.len() == 1 { Some(matches[0]) } else { None }
        })
        .map(|c| c.name.clone())
        .ok_or_else(|| KoreError::InvalidArgument(format!("ORDER BY column not found: {col}")))?;

    // Spill-aware sort: if block is large, use ExternalSort to avoid OOM.
    // Threshold: 256MB (32M cells × 8 bytes)
    let estimated = kore_spill::SpillManager::estimate_bytes(&block);
    if estimated > 256 * 1024 * 1024 {
        let tmp = std::env::temp_dir().join("kore_sort");
        let ext = if desc {
            kore_spill::ExternalSort::new(&col_name, tmp).descending()
        } else {
            kore_spill::ExternalSort::new(&col_name, tmp)
        };
        return ext.sort(vec![block]);
    }

    // In-memory sort for smaller blocks
    block.sort_by(&col_name, !desc)
}

// ─── Limit ────────────────────────────────────────────────────────────────────

fn limit_block(block: DataBlock, n: usize) -> DataBlock {
    let take = n.min(block.num_rows);
    let indices: Vec<usize> = (0..take).collect();
    block.select_rows(&indices)
}

/// Remove duplicate rows (for SELECT DISTINCT).
/// Builds a string key per row; keeps first occurrence.
fn deduplicate(block: DataBlock) -> DataBlock {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut keep: Vec<usize> = Vec::new();
    for i in 0..block.num_rows {
        let key: String = block.columns.iter().map(|c| {
            match c.data.get_value(i) {
                Value::Null      => "∅".to_string(),
                Value::Int(n)    => n.to_string(),
                Value::Float(f)  => format!("{f:.10}"),
                Value::Bool(b)   => b.to_string(),
                Value::Str(s)    => s,
            }
        }).collect::<Vec<_>>().join("\x00");
        if seen.insert(key) {
            keep.push(i);
        }
    }
    block.select_rows(&keep)
}

fn project(block: DataBlock, projections: &[Projection]) -> Result<DataBlock, KoreError> {
    // Star = keep all
    if projections.iter().any(|p| matches!(p, Projection::Star)) {
        return Ok(block);
    }

    let mut new_cols: Vec<Column> = Vec::new();
    for proj in projections {
        match proj {
            Projection::Star => { new_cols.extend(block.columns.iter().cloned()); }
            Projection::Expr { expr, alias } => {
                let out_name = || alias.clone().unwrap_or_else(|| "expr".into());

                // Fast path: if alias already exists in block (from group_by_agg or
                // materialize_groupby_aliases), use it directly — avoids re-evaluation
                // with stale/missing columns after GROUP BY projection.
                if let Some(a) = alias {
                    if let Some(src) = block.columns.iter().find(|c| c.name == a.as_str()) {
                        // Only use pre-computed alias for complex exprs or when source col is gone
                        let source_col_missing = match expr {
                            Expr::Col(c) | Expr::QualCol(_, c) => {
                                let full = match expr { Expr::QualCol(t, c2) => format!("{}.{}", t, c2), _ => c.clone() };
                                !block.columns.iter().any(|col| {
                                    let cn = col.name.len(); let nm = full.len();
                                    col.name == full || (cn > nm && col.name.as_bytes()[cn-nm-1] == b'.' && &col.name[cn-nm..] == full)
                                })
                            }
                            _ => true, // complex exprs always prefer pre-computed
                        };
                        if source_col_missing {
                            new_cols.push(src.clone());
                            continue;
                        }
                    }
                }

                match expr {
                    Expr::Col(c) | Expr::QualCol(_, c) => {
                        let col_name = match expr {
                            Expr::QualCol(t, c2) => format!("{}.{}", t, c2),
                            _                    => c.clone(),
                        };
                        let src = block.columns.iter().find(|col| {
                            let cn = col.name.len(); let nm = col_name.len();
                            col.name == col_name ||
                            (cn > nm && col.name.as_bytes()[cn-nm-1] == b'.' && &col.name[cn-nm..] == col_name)
                        }).ok_or_else(|| KoreError::InvalidArgument(format!("column not found: {col_name}")))?;
                        let mut nc = src.clone();
                        if let Some(a) = alias { nc.name = a.clone(); }
                        new_cols.push(nc);
                    }
                    // Window function columns are already materialized under alias name
                    Expr::Window { .. } => {
                        let win_col = alias.clone().unwrap_or_else(|| "__win".into());
                        let src = block.columns.iter().find(|c| c.name == win_col)
                            .ok_or_else(|| KoreError::InvalidArgument(format!("window col not found: {win_col}")))?;
                        new_cols.push(src.clone());
                    }
                    // Aggregate results are already in block (from group_by_agg or global_agg).
                    // Must compute the same column name that global_agg uses.
                    Expr::Agg { func, expr: inner } => {
                        let col_name = alias.clone().unwrap_or_else(|| {
                            let inner_name = match inner.as_ref() {
                                Expr::Col(c) | Expr::QualCol(_, c) => c.clone(),
                                Expr::Star => "*".to_string(),
                                _ => String::new(),
                            };
                            format!("{:?}({})", func, inner_name)
                        });
                        if let Some(src) = block.columns.iter().find(|c| {
                            c.name == col_name || {
                                // Also try alias match for group_by results
                                alias.as_ref().map(|a| c.name == *a).unwrap_or(false)
                            }
                        }) {
                            let mut nc = src.clone();
                            if let Some(a) = alias { nc.name = a.clone(); }
                            new_cols.push(nc);
                        }
                        // else silently skip (shouldn't happen after group_by_agg)
                    }
                    // Everything else: evaluate row-by-row
                    // BUT: if this alias was already computed (e.g. by group_by_agg or
                    // materialize_groupby_aliases), use the existing column to avoid
                    // re-evaluating with missing source columns after GROUP BY.
                    _ => {
                        let alias_name = alias.as_deref().unwrap_or("");
                        // Check if the alias column already exists in the block (pre-computed)
                        if !alias_name.is_empty() {
                            if let Some(src) = block.columns.iter().find(|c| c.name == alias_name) {
                                new_cols.push(src.clone());
                                continue;
                            }
                        }
                        let n = block.num_rows;
                        let vals: Vec<ExprVal> = (0..n).map(|r| eval_expr(expr, &block, r)).collect();
                        new_cols.push(exprvals_to_column(out_name(), vals));
                    }
                }
            }
        }
    }
    let num_rows = block.num_rows;
    Ok(DataBlock { columns: new_cols, num_rows })
}

/// Convert a Vec<ExprVal> into a typed Column.
fn exprvals_to_column(name: String, vals: Vec<ExprVal>) -> Column {
    // Determine type from first non-null value
    match vals.iter().find(|v| !matches!(v, ExprVal::Null)) {
        Some(ExprVal::Int(_)) | Some(ExprVal::Bool(_)) if matches!(vals.iter().find(|v| !matches!(v, ExprVal::Null)), Some(ExprVal::Int(_))) =>
            Column { name, data: ColumnData::Int64(vals.into_iter().map(|v| match v {
                ExprVal::Int(i) => Some(i), ExprVal::Float(f) => Some(f as i64), _ => None,
            }).collect()) },
        Some(ExprVal::Float(_)) =>
            Column { name, data: ColumnData::Float64(vals.into_iter().map(|v| match v {
                ExprVal::Float(f) => Some(f), ExprVal::Int(i) => Some(i as f64), _ => None,
            }).collect()) },
        Some(ExprVal::Bool(_)) =>
            Column { name, data: ColumnData::Bool(vals.into_iter().map(|v| match v {
                ExprVal::Bool(b) => Some(b), _ => None,
            }).collect()) },
        // Str and Null fall here
        _ =>
            Column { name, data: ColumnData::Str(vals.into_iter().map(|v| match v {
                ExprVal::Str(s) => Some(s), ExprVal::Int(i) => Some(i.to_string()),
                ExprVal::Float(f) => Some(f.to_string()), ExprVal::Bool(b) => Some(b.to_string()),
                ExprVal::Null => None,
            }).collect()) },
    }
}

// ─── Fast column extraction helpers ──────────────────────────────────────────

/// Collect outer QualCol references from inside a subquery predicate.
/// These are columns like m1.kind or o1.cust_id that come from the OUTER query
/// and must be kept in the outer block's column pruning step.
fn collect_outer_quals(expr: &Expr, set: &mut std::collections::HashSet<String>) {
    match expr {
        Expr::QualCol(_, c) => { set.insert(c.clone()); }
        Expr::BinOp { left, right, .. } => {
            collect_outer_quals(left, set);
            collect_outer_quals(right, set);
        }
        Expr::Not(e) | Expr::IsNull(e) | Expr::IsNotNull(e) => collect_outer_quals(e, set),
        Expr::In { expr: e, values, .. } => {
            collect_outer_quals(e, set);
            for v in values { collect_outer_quals(v, set); }
        }
        _ => {}
    }
}

/// Collect all bare column names referenced anywhere in an expression.
fn collect_cols_expr(expr: &Expr, set: &mut std::collections::HashSet<String>) {    match expr {
        Expr::Col(c)            => { set.insert(c.clone()); }
        Expr::QualCol(_, c)     => { set.insert(c.clone()); }
        Expr::Agg { expr: e, .. } => collect_cols_expr(e, set),
        Expr::BinOp { left, right, .. } => {
            collect_cols_expr(left, set);
            collect_cols_expr(right, set);
        }
        Expr::Not(e) | Expr::IsNull(e) | Expr::IsNotNull(e) => collect_cols_expr(e, set),
        Expr::Between { expr: e, low, high, .. } => {
            collect_cols_expr(e, set);
            collect_cols_expr(low, set);
            collect_cols_expr(high, set);
        }
        Expr::In { expr: e, values, .. } => {
            collect_cols_expr(e, set);
            for v in values { collect_cols_expr(v, set); }
        }
        Expr::Like { expr: e, pattern, .. } => {
            collect_cols_expr(e, set);
            collect_cols_expr(pattern, set);
        }
        Expr::Case { operand, branches, else_val } => {
            if let Some(op) = operand { collect_cols_expr(op, set); }
            for (cond, val) in branches { collect_cols_expr(cond, set); collect_cols_expr(val, set); }
            if let Some(ev) = else_val { collect_cols_expr(ev, set); }
        }
        Expr::FuncCall { args, .. } => { for a in args { collect_cols_expr(a, set); } }
        // Subquery expressions — collect outer column references
        Expr::InSubquery { expr: e, .. } => collect_cols_expr(e, set),
        // For EXISTS and ScalarSubquery: scan inner WHERE for outer table references (QualCol)
        // These are the correlated outer columns that the outer query must keep.
        Expr::ScalarSubquery(stmt) => {
            if let Some(pred) = &stmt.where_clause { collect_outer_quals(pred, set); }
        }
        Expr::Exists { subquery, .. } => {
            if let Some(pred) = &subquery.where_clause { collect_outer_quals(pred, set); }
        }
        Expr::Window { func, spec } => {
            match func {
                WindowFn::Agg { expr: e, .. } => collect_cols_expr(e, set),
                WindowFn::Ntile(e) | WindowFn::FirstValue(e) | WindowFn::LastValue(e) | WindowFn::CumSum(e) => {
                    collect_cols_expr(e, set);
                }
                WindowFn::Lag { expr: e, offset } | WindowFn::Lead { expr: e, offset } => {
                    collect_cols_expr(e, set);
                    collect_cols_expr(offset, set);
                }
                _ => {}
            }
            for e in &spec.partition_by { collect_cols_expr(e, set); }
            for o in &spec.order_by    { set.insert(o.col.rsplit('.').next().unwrap_or(&o.col).to_string()); }
        }
        _ => {}
    }
}

/// Return the set of bare column names (without table prefix) used by a SELECT statement.
fn used_columns(stmt: &SelectStmt) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for proj in &stmt.projections {
        if let Projection::Expr { expr, .. } = proj { collect_cols_expr(expr, &mut set); }
    }
    if let Some(pred) = &stmt.where_clause { collect_cols_expr(pred, &mut set); }
    for col in &stmt.group_by  { set.insert(col.rsplit('.').next().unwrap_or(col).to_string()); }
    for item in &stmt.order_by { set.insert(item.col.rsplit('.').next().unwrap_or(&item.col).to_string()); }
    set
}

/// Find a column by exact name or table-prefix suffix match.
fn find_col<'a>(block: &'a DataBlock, name: &str) -> Option<&'a Column> {
    // Hot path: avoid format!() allocation by doing suffix check inline.
    block.columns.iter().find(|c| {
        c.name == name || {
            let cn = c.name.len();
            let nm = name.len();
            cn > nm && c.name.as_bytes()[cn - nm - 1] == b'.' && &c.name[cn - nm..] == name
        }
    })
}

/// Extract f64 values for a subset of rows — column-at-a-time, no per-row dispatch.
/// 10–50× faster than calling `get_cell()` + `to_f64()` per row.
#[inline]
fn extract_f64_at(col: &Column, indices: &[usize]) -> Vec<f64> {
    match &col.data {
        ColumnData::Float64(v) => indices.iter().filter_map(|&r| v.get(r).and_then(|x| *x)).collect(),
        ColumnData::Int64(v)   => indices.iter().filter_map(|&r| v.get(r).and_then(|x| *x).map(|i| i as f64)).collect(),
        ColumnData::Bool(v)    => indices.iter().filter_map(|&r| v.get(r).and_then(|x| *x).map(|b| b as i64 as f64)).collect(),
        ColumnData::Str(_)     => vec![],
        ColumnData::StrDict { .. } => vec![],
    }
}

/// Extract ALL f64 values in a column (for global aggregations).
#[inline]
fn extract_f64_all(col: &Column) -> Vec<f64> {
    match &col.data {
        ColumnData::Float64(v) => v.iter().filter_map(|x| *x).collect(),
        ColumnData::Int64(v)   => v.iter().filter_map(|x| *x).map(|i| i as f64).collect(),
        ColumnData::Bool(v)    => v.iter().filter_map(|x| *x).map(|b| b as i64 as f64).collect(),
        ColumnData::Str(_)     => vec![],
        ColumnData::StrDict { .. } => vec![],
    }
}

// ─── Global aggregation (no GROUP BY) ────────────────────────────────────────

/// Aggregate the entire block into a single row.
fn global_agg(block: DataBlock, projections: &[Projection]) -> Result<DataBlock, KoreError> {
    let all_rows: Vec<usize> = (0..block.num_rows).collect();
    let mut new_cols: Vec<Column> = Vec::new();
    for proj in projections {
        if let Projection::Expr { expr: Expr::Agg { func, expr: inner }, alias } = proj {
            let is_direct = matches!(inner.as_ref(),
                Expr::Col(_) | Expr::QualCol(_, _) | Expr::Star);
            let col_name = match inner.as_ref() {
                Expr::Col(c)        => c.clone(),
                Expr::QualCol(t, c) => format!("{}.{}", t, c),
                _                   => String::new(),
            };
            let agg_col = if is_direct { find_col(&block, &col_name) } else { None };
            // Direct col: fast column-at-a-time; complex expr: row-at-a-time
            let vals: Vec<f64> = if is_direct {
                agg_col.map(|c| extract_f64_all(c)).unwrap_or_default()
            } else {
                (0..block.num_rows).filter_map(|r| match eval_expr(inner, &block, r) {
                    ExprVal::Float(f) => Some(f),
                    ExprVal::Int(i)   => Some(i as f64),
                    _                 => None,
                }).collect()
            };
            let v: Option<f64> = match func {
                AggFunc::Count => Some(block.num_rows as f64),
                AggFunc::CountDistinct => {
                    use std::collections::HashSet;
                    let seen: HashSet<u64> = agg_col.map(|col| match &col.data {
                        ColumnData::Float64(v) => v.iter().filter_map(|x| *x).map(|f| f.to_bits()).collect(),
                        ColumnData::Int64(v)   => v.iter().filter_map(|x| *x).map(|i| i as u64).collect(),
                        ColumnData::Str(v)     => v.iter().filter_map(|x| x.as_deref()).map(|s| {
                            let mut h = 14695981039346656037u64;
                            for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
                            h
                        }).collect(),
                        _ => HashSet::new(),
                    }).unwrap_or_default();
                    Some(seen.len() as f64)
                }
                AggFunc::Sum => if vals.is_empty() { None } else { Some(vals.iter().sum()) },
                AggFunc::Avg => if vals.is_empty() { None } else { Some(vals.iter().sum::<f64>() / vals.len() as f64) },
                AggFunc::Min => vals.iter().copied().reduce(f64::min),
                AggFunc::Max => vals.iter().copied().reduce(f64::max),
            };
            let name = alias.clone().unwrap_or_else(|| format!("{:?}({})", func, col_name));
            new_cols.push(Column { name, data: ColumnData::Float64(vec![v]) });
        }
    }
    Ok(DataBlock { columns: new_cols, num_rows: 1 })
}

// ─── GROUP BY (aggregate) ─────────────────────────────────────────────────────

fn group_by_agg(
    block: DataBlock,
    group_cols: &[String],
    projections: &[Projection],
) -> Result<DataBlock, KoreError> {
    use rayon::prelude::*;
    use std::collections::HashMap;

    // Pre-locate group-by columns once
    let gcols: Vec<&Column> = group_cols.iter()
        .filter_map(|c| find_col(&block, c))
        .collect();
    let fallback = gcols.len() < group_cols.len();

    let n = block.num_rows;
    let nthreads = rayon::current_num_threads();
    // Use parallelism for any table large enough to benefit (~50K+ rows).
    let nchunks  = if n >= 50_000 { (nthreads * 2).max(1) } else { 1 };
    let chunk_sz = ((n + nchunks - 1) / nchunks).max(1);

    // ── Fast u128 key (no String allocation per row) ──────────────────────────
    // FNV-1a hashed per column, combined with position-aware rotation.
    // u128 space (2^128) makes hash collisions practically impossible.
    #[inline(always)]
    fn fnv64(bytes: &[u8]) -> u64 {
        let mut h: u64 = 14695981039346656037;
        for &b in bytes { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        h
    }

    // ── build_chunk: no-allocation hot loop ───────────────────────────────────
    type LocalMap = Vec<(u128, Vec<usize>)>;
    let build_chunk = |c: usize| -> LocalMap {
        let start = c * chunk_sz;
        let end   = (start + chunk_sz).min(n);
        if start >= end { return vec![]; }

        let mut local: HashMap<u128, Vec<usize>> = HashMap::with_capacity((end - start) / 4 + 8);
        let mut order: Vec<u128>                 = Vec::new();

        for row in start..end {
            // Compute u128 key: mix per-column values without String allocation
            let key: u128 = if fallback {
                let mut k: u128 = 0xcbf29ce484222325_cbf29ce484222325u128;
                for (i, gc) in group_cols.iter().enumerate() {
                    let v = match get_cell(&block, gc, row) {
                        ExprVal::Int(x)   => x as u64,
                        ExprVal::Float(x) => x.to_bits(),
                        ExprVal::Str(ref s) => fnv64(s.as_bytes()),
                        ExprVal::Bool(x)  => x as u64,
                        ExprVal::Null     => 0xFFFF_FFFF_FFFF_FFFF,
                    };
                    k = k.wrapping_add(v as u128)
                         .wrapping_mul(0x9e3779b97f4a7c15_f39cc0605cedc835u128)
                         .rotate_left((i as u32 * 11 + 7) % 127);
                }
                k
            } else {
                let mut k: u128 = 0xcbf29ce484222325_cbf29ce484222325u128;
                for (i, col) in gcols.iter().enumerate() {
                    let v: u64 = match &col.data {
                        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).unwrap_or(i64::MIN) as u64,
                        ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| f.to_bits()).unwrap_or(0),
                        ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).unwrap_or(false) as u64,
                        ColumnData::Str(v)     => fnv64(v.get(row).and_then(|x| x.as_deref()).unwrap_or("").as_bytes()),
                        ColumnData::StrDict { codes, dict } => {
                            let c = codes.get(row).copied().unwrap_or(u8::MAX);
                            if c == u8::MAX { 0 } else { fnv64(dict.get(c as usize).map(|s| s.as_bytes()).unwrap_or(b"")) }
                        }
                    };
                    k = k.wrapping_add(v as u128)
                         .wrapping_mul(0x9e3779b97f4a7c15_f39cc0605cedc835u128)
                         .rotate_left((i as u32 * 11 + 7) % 127);
                }
                k
            };

            if !local.contains_key(&key) { order.push(key); }
            local.entry(key).or_default().push(row);
        }
        order.into_iter().map(|k| { let v = local.remove(&k).unwrap(); (k, v) }).collect()
    };

    // Sequential (nchunks==1) avoids Rayon overhead for small/high-cardinality tables.
    // Parallel (nchunks>1) for large low-cardinality tables (e.g. Q1: 6M rows, 6 groups).
    let local_maps: Vec<LocalMap> = if nchunks == 1 {
        vec![build_chunk(0)]
    } else {
        (0..nchunks).into_par_iter().map(build_chunk).collect()
    };

    // ── Merge phase ───────────────────────────────────────────────────────────
    let mut group_map: HashMap<u128, Vec<usize>> = HashMap::new();
    let mut key_order: Vec<u128>                 = Vec::new();
    for local in local_maps {
        for (key, mut idxs) in local {
            if !group_map.contains_key(&key) { key_order.push(key); }
            group_map.entry(key).or_default().append(&mut idxs);
        }
    }

    // Reconstruct ordered groups vec for downstream processing
    let groups: Vec<(Vec<ExprVal>, Vec<usize>)> = key_order.iter().map(|k| {
        let idxs = group_map[k].clone();
        let first = idxs[0];
        let key_vals: Vec<ExprVal> = group_cols.iter()
            .map(|c| get_cell(&block, c, first))
            .collect();
        (key_vals, idxs)
    }).collect();

    // Build result block from aggregated groups
    let first_rows: Vec<usize> = groups.iter().map(|(_, idxs)| idxs[0]).collect();
    let agg_block = block.select_rows(&first_rows);

    // Handle SUM/COUNT/AVG/MIN/MAX in projections
    let has_agg = projections.iter().any(|p| matches!(p, Projection::Expr { expr: Expr::Agg { .. }, .. }));
    if !has_agg { return Ok(agg_block); }

    let mut new_cols: Vec<Column> = Vec::new();
    for proj in projections {
        match proj {
            Projection::Star => {
                new_cols.extend(agg_block.columns.iter().cloned());
            }
            Projection::Expr { expr, alias } => {
                match expr {
                    Expr::Agg { func, expr: inner } => {
                        let is_direct = matches!(inner.as_ref(),
                            Expr::Col(_) | Expr::QualCol(_, _) | Expr::Star);
                        let col_name = match inner.as_ref() {
                            Expr::Col(c)        => c.clone(),
                            Expr::QualCol(t, c) => format!("{}.{}", t, c),
                            _                   => String::new(),
                        };
                        // Pre-find the column once (not per group)
                        let agg_col = if is_direct { find_col(&block, &col_name) } else { None };
                        let mut agg_vals: Vec<Option<f64>> = Vec::new();
                        for (_, idxs) in &groups {
                            // Direct column ref: fast column-at-a-time extraction.
                            // Arbitrary expression (e.g. col*col): row-at-a-time eval.
                            let vals: Vec<f64> = if is_direct {
                                agg_col.map(|c| extract_f64_at(c, idxs)).unwrap_or_default()
                            } else {
                                idxs.iter().filter_map(|&r| match eval_expr(inner, &block, r) {
                                    ExprVal::Float(f) => Some(f),
                                    ExprVal::Int(i)   => Some(i as f64),
                                    _                 => None,
                                }).collect()
                            };
                            let v = match func {
                                AggFunc::Count => Some(idxs.len() as f64),
                                AggFunc::CountDistinct => {
                                    use std::collections::HashSet;
                                    let seen: HashSet<u64> = if is_direct {
                                        agg_col.map(|col| match &col.data {
                                            ColumnData::Float64(v) => idxs.iter().filter_map(|&r| v.get(r).and_then(|x| *x)).map(|f| f.to_bits()).collect(),
                                            ColumnData::Int64(v)   => idxs.iter().filter_map(|&r| v.get(r).and_then(|x| *x)).map(|i| i as u64).collect(),
                                            _ => HashSet::new(),
                                        }).unwrap_or_default()
                                    } else {
                                        vals.iter().map(|&f| f.to_bits()).collect()
                                    };
                                    Some(seen.len() as f64)
                                }
                                AggFunc::Sum   => if vals.is_empty() { None } else { Some(vals.iter().sum()) },
                                AggFunc::Avg   => if vals.is_empty() { None } else { Some(vals.iter().sum::<f64>() / vals.len() as f64) },
                                AggFunc::Min   => vals.iter().copied().reduce(f64::min),
                                AggFunc::Max   => vals.iter().copied().reduce(f64::max),
                            };
                            agg_vals.push(v);
                        }
                        let name = alias.clone().unwrap_or_else(|| format!("{:?}({})", func, col_name));
                        new_cols.push(Column {
                            name,
                            data: ColumnData::Float64(agg_vals),
                        });
                    }
                    other => {
                        // For Col/QualCol: copy directly from agg_block by name
                        // For CASE/FuncCall/BinOp etc: check if alias column was pre-materialized,
                        // otherwise evaluate the expression for each representative row
                        let col_name = match other {
                            Expr::Col(c)        => Some(c.clone()),
                            Expr::QualCol(_, c) => Some(c.clone()),
                            _ => None,
                        };

                        if let Some(cn) = col_name {
                            if let Some(src) = find_col(&agg_block, &cn) {
                                let mut nc = src.clone();
                                if let Some(a) = alias { nc.name = a.clone(); }
                                new_cols.push(nc);
                            }
                        } else {
                            // Complex expr (CASE WHEN, FuncCall, etc.)
                            // First: check if alias already exists in agg_block (from materialize_groupby_aliases)
                            let alias_name = alias.as_deref().unwrap_or("");
                            if !alias_name.is_empty() {
                                if let Some(src) = find_col(&agg_block, alias_name) {
                                    let mut nc = src.clone();
                                    nc.name = alias_name.to_string();
                                    new_cols.push(nc);
                                    continue;
                                }
                            }
                            // Fallback: evaluate expression for each representative row
                            let n = first_rows.len();
                            let values: Vec<Option<String>> = first_rows.iter().map(|&r| {
                                Some(match eval_expr(other, &block, r) {
                                    ExprVal::Str(s)   => s,
                                    ExprVal::Int(i)   => i.to_string(),
                                    ExprVal::Float(f) => format!("{f:.4}"),
                                    ExprVal::Bool(b)  => b.to_string(),
                                    ExprVal::Null     => return None,
                                })
                            }).collect();
                            let name = alias.clone().unwrap_or_else(|| format!("col{}", new_cols.len()));
                            new_cols.push(Column {
                                name,
                                data: ColumnData::Str(values),
                            });
                        }
                    }
                }
            }
        }
    }

    let num_rows = groups.len();
    Ok(DataBlock { columns: new_cols, num_rows })
}

fn expr_vals_eq(a: &[ExprVal], b: &[ExprVal]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| match (x, y) {
        (ExprVal::Int(x),   ExprVal::Int(y))   => x == y,
        (ExprVal::Float(x), ExprVal::Float(y)) => (x - y).abs() < 1e-10,
        (ExprVal::Str(x),   ExprVal::Str(y))   => x == y,
        (ExprVal::Bool(x),  ExprVal::Bool(y))  => x == y,
        (ExprVal::Null,     ExprVal::Null)      => true,
        _ => false,
    })
}

// ── Map AST WindowFn → kore-window WindowFn ───────────────────────────────────

// ── LIKE pattern matching ─────────────────────────────────────────────────────

/// SQL LIKE: `%` = any chars, `_` = single char, `\` = escape char.
fn like_match(value: &str, pattern: &str) -> bool {
    like_recursive(value.as_bytes(), pattern.as_bytes())
}

fn like_recursive(s: &[u8], p: &[u8]) -> bool {
    match (s, p) {
        (_, [])           => s.is_empty(),
        (_, [b'%', rest @ ..]) => {
            // % matches 0 or more characters
            if like_recursive(s, rest) { return true; }
            if let [_, tail @ ..] = s { return like_recursive(tail, p); }
            false
        }
        ([], _) => false,
        ([sc, st @ ..], [b'_', pt @ ..]) => like_recursive(st, pt),  // _ matches any one
        ([sc, st @ ..], [pc, pt @ ..]) if sc == pc => like_recursive(st, pt),
        _ => false,
    }
}

fn col_name_from_expr(e: &Expr) -> String {    match e {
        Expr::Col(n)        => n.clone(),
        Expr::QualCol(_, n) => n.clone(),
        _ => "__expr__".into(),
    }
}

fn ast_to_win_fn(ast: &WindowFn) -> WinFn {
    match ast {
        WindowFn::RowNumber   => WinFn::RowNumber,
        WindowFn::Rank        => WinFn::Rank,
        WindowFn::DenseRank   => WinFn::DenseRank,
        WindowFn::Ntile(n)    => WinFn::Ntile(match n.as_ref() { Expr::Int(i) => *i as usize, _ => 4 }),
        WindowFn::Lag  { expr, offset } => WinFn::Lag  { col: col_name_from_expr(expr), offset: match offset.as_ref() { Expr::Int(i) => *i as usize, _ => 1 } },
        WindowFn::Lead { expr, offset } => WinFn::Lead { col: col_name_from_expr(expr), offset: match offset.as_ref() { Expr::Int(i) => *i as usize, _ => 1 } },
        WindowFn::Agg { func, expr } => match func {
            AggFunc::Sum   => WinFn::Sum  (col_name_from_expr(expr)),
            AggFunc::Avg   => WinFn::Avg  (col_name_from_expr(expr)),
            AggFunc::Count | AggFunc::CountDistinct => WinFn::Count(col_name_from_expr(expr)),
            AggFunc::Min   => WinFn::Min  (col_name_from_expr(expr)),
            AggFunc::Max   => WinFn::Max  (col_name_from_expr(expr)),
        },
        WindowFn::CumSum(e)    => WinFn::CumSum    (col_name_from_expr(e)),
        WindowFn::FirstValue(e) => WinFn::FirstValue(col_name_from_expr(e)),
        WindowFn::LastValue(e)  => WinFn::LastValue (col_name_from_expr(e)),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn make_orders() -> DataBlock {
        DataBlock {
            num_rows: 4,
            columns: vec![
                Column { name: "id".into(),      data: ColumnData::Int64(vec![Some(1),Some(2),Some(3),Some(4)]) },
                Column { name: "cust_id".into(),  data: ColumnData::Int64(vec![Some(10),Some(20),Some(10),Some(30)]) },
                Column { name: "score".into(),    data: ColumnData::Float64(vec![Some(90.0),Some(70.0),Some(85.0),Some(60.0)]) },
            ],
        }
    }

    fn make_customers() -> DataBlock {
        DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(),   data: ColumnData::Int64(vec![Some(10),Some(20),Some(30)]) },
                Column { name: "name".into(), data: ColumnData::Str(vec![Some("Alice".into()),Some("Bob".into()),Some("Carol".into())]) },
            ],
        }
    }

    #[test]
    fn test_simple_where_limit() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        let result = ctx.query(
            "SELECT * FROM orders WHERE score > 80 ORDER BY score DESC LIMIT 2"
        ).unwrap();
        assert_eq!(result.num_rows, 2);
    }

    #[test]
    fn test_inner_join() {
        let mut ctx = KqlContext::new();
        ctx.register("orders",    make_orders());
        ctx.register("customers", make_customers());
        let result = ctx.query(
            "SELECT * FROM orders AS a INNER JOIN customers AS b ON a.cust_id = b.id"
        ).unwrap();
        // All 4 orders have valid cust_id in customers table
        assert_eq!(result.num_rows, 4);
    }

    #[test]
    fn test_aggregate() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        let result = ctx.query(
            "SELECT cust_id, SUM(score) AS total FROM orders GROUP BY cust_id"
        ).unwrap();
        assert_eq!(result.num_rows, 3); // 3 distinct cust_ids
    }

    // ─── Subquery tests ──────────────────────────────────────────────────────

    #[test]
    fn test_scalar_subquery_where_eq_max() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        // WHERE score = (SELECT MAX(score) FROM orders)  → only score=90
        let result = ctx.query(
            "SELECT score FROM orders WHERE score = (SELECT MAX(score) FROM orders)"
        ).unwrap();
        assert_eq!(result.num_rows, 1, "only score=90 should match MAX");
    }

    #[test]
    fn test_scalar_subquery_where_gt_avg() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        // AVG = (90+70+85+60)/4 = 76.25 — rows > avg: 90, 85
        let result = ctx.query(
            "SELECT score FROM orders WHERE score > (SELECT AVG(score) FROM orders)"
        ).unwrap();
        assert_eq!(result.num_rows, 2, "scores 90 and 85 are > avg(76.25)");
    }

    #[test]
    fn test_in_subquery() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        ctx.register("customers", make_customers());
        // Find orders from customers whose name starts with 'A' → cust_id=10 → 2 rows
        let result = ctx.query(
            "SELECT id FROM orders WHERE cust_id IN (SELECT id FROM customers WHERE name = 'Alice')"
        ).unwrap();
        assert_eq!(result.num_rows, 2, "Alice has cust_id=10, two orders");
    }

    #[test]
    fn test_not_in_subquery() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        ctx.register("customers", make_customers());
        // Exclude orders from Alice (cust_id=10)
        let result = ctx.query(
            "SELECT id FROM orders WHERE cust_id NOT IN (SELECT id FROM customers WHERE name = 'Alice')"
        ).unwrap();
        assert_eq!(result.num_rows, 2, "Bob and Carol have 2 orders");
    }

    #[test]
    fn test_correlated_subquery() {
        // WHERE score > (SELECT AVG(score) FROM orders o2 WHERE o2.cust_id = o1.cust_id)
        // cust_id=10: scores [90,85] avg=87.5 → row 90 matches, row 85 does NOT
        // cust_id=20: scores [70]    avg=70.0 → row 70 does NOT
        // cust_id=30: scores [60]    avg=60.0 → row 60 does NOT
        // Expected: 1 row (score=90)
        let mut ctx = KqlContext::new();
        ctx.register("o1", make_orders());
        ctx.register("orders", make_orders());
        let result = ctx.query(
            "SELECT score FROM o1 WHERE score > (SELECT AVG(score) FROM orders o2 WHERE o2.cust_id = o1.cust_id)"
        ).unwrap();
        assert_eq!(result.num_rows, 1, "only score=90 is above its group avg(87.5)");
    }

    #[test]
    fn test_exists_correlated() {
        // Find orders where there exists another order from the same customer
        // cust_id=10 has 2 orders → both qualify; cust_id=20,30 have 1 order → don't qualify
        let mut ctx = KqlContext::new();
        ctx.register("o1", make_orders());
        ctx.register("orders", make_orders());
        // Simple EXISTS: all rows have at least 1 match in orders
        let result = ctx.query(
            "SELECT id FROM o1 WHERE EXISTS (SELECT 1 FROM orders o2 WHERE o2.cust_id = o1.cust_id)"
        ).unwrap();
        assert_eq!(result.num_rows, 4, "all o1 rows exist in orders by cust_id");
    }

    // ─── Layer 34: Scalar functions ─────────────────────────────────────────

    fn make_strings() -> DataBlock {
        DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(),  data: ColumnData::Int64(vec![Some(1),Some(2),Some(3)]) },
                Column { name: "tag".into(), data: ColumnData::Str(vec![
                    Some("hello".into()), Some("  World  ".into()), Some("Rust".into())
                ]) },
                Column { name: "val".into(), data: ColumnData::Float64(vec![Some(3.7), Some(-1.5), Some(2.0)]) },
            ],
        }
    }

    #[test]
    fn test_string_functions() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_strings());
        // UPPER, LOWER, TRIM, LENGTH
        let r = ctx.query(
            "SELECT UPPER(tag) AS u, LOWER(tag) AS l, TRIM(tag) AS tr, LENGTH(tag) AS n FROM t WHERE id = 2"
        ).unwrap();
        assert_eq!(r.num_rows, 1);
        if let ColumnData::Str(v) = &r.columns.iter().find(|c| c.name=="u").unwrap().data {
            assert_eq!(v[0], Some("  WORLD  ".into()));
        }
        if let ColumnData::Str(v) = &r.columns.iter().find(|c| c.name=="tr").unwrap().data {
            assert_eq!(v[0], Some("World".into()));
        }
    }

    #[test]
    fn test_math_functions() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_strings());
        let r = ctx.query("SELECT ABS(val) AS a, ROUND(val, 0) AS r, CEIL(val) AS c FROM t").unwrap();
        assert_eq!(r.num_rows, 3);
        if let ColumnData::Float64(v) = &r.columns.iter().find(|c| c.name=="a").unwrap().data {
            assert!((v[0].unwrap() - 3.7).abs() < 0.001);
            assert!((v[1].unwrap() - 1.5).abs() < 0.001); // ABS(-1.5)
        }
    }

    #[test]
    fn test_count_distinct() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        let r = ctx.query(
            "SELECT COUNT(DISTINCT cust_id) AS uniq FROM orders"
        ).unwrap();
        // 3 distinct cust_ids (10, 20, 30) in 4 rows
        if let ColumnData::Float64(v) = &r.columns.iter().find(|c| c.name=="uniq").unwrap().data {
            assert_eq!(v[0], Some(3.0));
        }
    }

    #[test]
    fn test_having_clause() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        // cust_id=10 appears twice (scores 90, 85) → sum=175; others appear once
        let r = ctx.query(
            "SELECT cust_id, SUM(score) AS total FROM orders GROUP BY cust_id HAVING total > 100"
        ).unwrap();
        assert_eq!(r.num_rows, 1);
    }

    #[test]
    fn test_coalesce_cast() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_strings());
        let r = ctx.query("SELECT COALESCE(id, 0) AS cid, CAST(val AS VARCHAR) AS sv FROM t LIMIT 1").unwrap();
        assert_eq!(r.num_rows, 1);
    }
}

