// kore-self — kore_query.rs
//
// KORE SQL engine powering memory analysis.
//
// Converts Vec<Memory> → DataBlock → runs real KQL queries.
// This is what makes kore-self GENUINELY different:
// not HashMap word counting, but the same SQL engine that beats Apache Spark.
//
// Every analysis in dream.rs, consciousness.rs, predictive.rs runs through here.

use kore_core::{Column, DataBlock, Value};
use kore_sql::KqlContext;

use crate::Memory;

// ─── Core: memories → DataBlock ──────────────────────────────────────────────

/// Load all memories into a KORE DataBlock.
/// Schema: id(i64), kind(str), content(str), importance(f64), timestamp(str), tags(str)
pub fn memories_to_block(memories: &[Memory]) -> DataBlock {
    if memories.is_empty() {
        return DataBlock::empty();
    }
    let ids:        Vec<Option<i64>>    = memories.iter().map(|m| Some(m.id as i64)).collect();
    let kinds:      Vec<Option<String>> = memories.iter().map(|m| Some(m.kind.clone())).collect();
    let contents:   Vec<Option<String>> = memories.iter().map(|m| Some(m.content.clone())).collect();
    let importance: Vec<Option<f64>>    = memories.iter().map(|m| Some(m.importance)).collect();
    let timestamps: Vec<Option<String>> = memories.iter().map(|m| Some(m.timestamp.clone())).collect();
    let tags:       Vec<Option<String>> = memories.iter().map(|m| Some(m.tags.join(","))).collect();

    DataBlock::new(vec![
        Column::int64("id",         ids),
        Column::str_col("kind",     kinds),
        Column::str_col("content",  contents),
        Column::float64("importance", importance),
        Column::str_col("timestamp", timestamps),
        Column::str_col("tags",     tags),
    ]).expect("Memory DataBlock construction failed")
}

/// Run a KQL query on memories. Table name: "memories".
pub fn query(memories: &[Memory], sql: &str) -> Result<DataBlock, String> {
    let block = memories_to_block(memories);
    let mut ctx = KqlContext::new();
    ctx.register("memories", block);
    ctx.query(sql).map_err(|e| e.to_string())
}

/// Format a DataBlock as a human-readable table string. Used by self_query.
pub fn block_to_display(block: &DataBlock) -> String {
    if block.num_rows == 0 { return "(no rows)".to_string(); }
    let headers: Vec<String> = block.columns.iter().map(|c| c.name.clone()).collect();
    let rows = block_to_rows(block);
    let widths: Vec<usize> = headers.iter().enumerate()
        .map(|(i, h)| rows.iter().map(|r| r.get(i).map(|s| s.len()).unwrap_or(0)).max().unwrap_or(0).max(h.len()).min(60))
        .collect();
    let sep: String = widths.iter().map(|&w| "-".repeat(w+2)).collect::<Vec<_>>().join("+");
    let hdr: String = headers.iter().zip(&widths).map(|(h,&w)| format!(" {:w$} ", h)).collect::<Vec<_>>().join("|");
    let mut out = vec![sep.clone(), hdr, sep.clone()];
    for row in rows.iter().take(100) {
        let line: String = row.iter().zip(&widths).map(|(c,&w)| {
            let s = if c.len()>w { format!("{}…",&c[..w.saturating_sub(1)]) } else { c.clone() };
            format!(" {:w$} ", s)
        }).collect::<Vec<_>>().join("|");
        out.push(line);
    }
    out.push(sep);
    out.push(if block.num_rows>100 { format!("... ({} rows total)", block.num_rows) } else { format!("{} rows", block.num_rows) });
    out.join("\n")
}

/// Run a KQL query, return rows as Vec<Vec<String>> for easy display.
pub fn query_rows(memories: &[Memory], sql: &str) -> Result<Vec<Vec<String>>, String> {
    let block = query(memories, sql)?;
    Ok(block_to_rows(&block))
}

// ─── Pre-built analytical queries ─────────────────────────────────────────────

/// Distribution of memory kinds with counts and average importance.
pub fn kind_distribution(memories: &[Memory]) -> Vec<(String, i64, f64)> {
    let sql = "SELECT kind, COUNT(*) AS cnt, AVG(importance) AS avg_imp \
               FROM memories GROUP BY kind ORDER BY cnt DESC";
    let Ok(block) = query(memories, sql) else { return vec![]; };
    extract_kind_rows(&block)
}

/// Top-k memories by importance.
pub fn top_by_importance(memories: &[Memory], k: usize) -> Vec<(String, String, f64)> {
    let sql = format!(
        "SELECT kind, content, importance FROM memories \
         ORDER BY importance DESC LIMIT {k}"
    );
    let Ok(block) = query(memories, &sql) else { return vec![]; };
    (0..block.num_rows).map(|i| {
        let kind    = col_str(&block, 0, i);
        let content = col_str(&block, 1, i);
        let imp     = col_f64(&block, 2, i);
        (kind, content, imp)
    }).collect()
}

/// Memories of a specific kind, ordered by importance desc.
pub fn by_kind(memories: &[Memory], kind: &str) -> Vec<(u64, String, f64)> {
    let sql = format!(
        "SELECT id, content, importance FROM memories \
         WHERE kind = '{kind}' ORDER BY importance DESC"
    );
    let Ok(block) = query(memories, &sql) else { return vec![]; };
    (0..block.num_rows).map(|i| {
        let id  = col_i64(&block, 0, i) as u64;
        let con = col_str(&block, 1, i);
        let imp = col_f64(&block, 2, i);
        (id, con, imp)
    }).collect()
}

/// Average importance per kind — used by consciousness THINK phase.
pub fn importance_by_kind(memories: &[Memory]) -> Vec<(String, f64)> {
    let sql = "SELECT kind, AVG(importance) AS avg_imp \
               FROM memories GROUP BY kind ORDER BY avg_imp DESC";
    let Ok(block) = query(memories, &sql) else { return vec![]; };
    (0..block.num_rows).map(|i| (col_str(&block, 0, i), col_f64(&block, 1, i))).collect()
}

/// High-stakes memories (importance >= threshold) — used by shadow + dream.
pub fn high_importance(memories: &[Memory], threshold: f64) -> Vec<(String, String, f64)> {
    let sql = format!(
        "SELECT kind, content, importance FROM memories \
         WHERE importance >= {threshold:.2} ORDER BY importance DESC LIMIT 50"
    );
    let Ok(block) = query(memories, &sql) else { return vec![]; };
    (0..block.num_rows).map(|i| (col_str(&block,0,i), col_str(&block,1,i), col_f64(&block,2,i))).collect()
}

/// Recent memories (last N by id) — used by consciousness OBSERVE.
pub fn recent(memories: &[Memory], n: usize) -> Vec<(String, String, f64)> {
    let sql = format!(
        "SELECT kind, content, importance FROM memories \
         ORDER BY id DESC LIMIT {n}"
    );
    let Ok(block) = query(memories, &sql) else { return vec![]; };
    (0..block.num_rows).map(|i| (col_str(&block,0,i), col_str(&block,1,i), col_f64(&block,2,i))).collect()
}

/// Count memories by kind. Returns HashMap-equivalent as sorted Vec.
pub fn count_by_kind(memories: &[Memory]) -> Vec<(String, usize)> {
    kind_distribution(memories)
        .into_iter()
        .map(|(k, cnt, _)| (k, cnt as usize))
        .collect()
}

/// Total importance sum per kind — for identity model weighting.
pub fn importance_sum_by_kind(memories: &[Memory]) -> Vec<(String, f64)> {
    let sql = "SELECT kind, SUM(importance) AS total_imp \
               FROM memories GROUP BY kind ORDER BY total_imp DESC";
    let Ok(block) = query(memories, &sql) else { return vec![]; };
    (0..block.num_rows).map(|i| (col_str(&block, 0, i), col_f64(&block, 1, i))).collect()
}

/// Run an arbitrary KQL query and return a human-readable table string.
/// Used by the `self_query` MCP tool.
pub fn run_user_query(memories: &[Memory], sql: &str) -> String {
    match query(memories, sql) {
        Ok(block) => {
            if block.num_rows == 0 { return "(no rows)".to_string(); }
            let headers: Vec<String> = block.columns.iter().map(|c| c.name.clone()).collect();
            let rows = block_to_rows(&block);
            let widths: Vec<usize> = headers.iter().enumerate()
                .map(|(i, h)| {
                    let max_row = rows.iter()
                        .map(|r| r.get(i).map(|s| s.len()).unwrap_or(0))
                        .max().unwrap_or(0);
                    h.len().max(max_row).min(60)
                })
                .collect();
            let sep: String = widths.iter().map(|&w| "-".repeat(w + 2)).collect::<Vec<_>>().join("+");
            let header_line: String = headers.iter().zip(&widths)
                .map(|(h, &w)| format!(" {:w$} ", h))
                .collect::<Vec<_>>().join("|");
            let mut out = vec![sep.clone(), header_line, sep.clone()];
            for row in rows.iter().take(100) {
                let line: String = row.iter().zip(&widths)
                    .map(|(cell, &w)| {
                        let s = if cell.len() > w { format!("{}…", &cell[..w.saturating_sub(1)]) } else { cell.clone() };
                        format!(" {:w$} ", s)
                    })
                    .collect::<Vec<_>>().join("|");
                out.push(line);
            }
            out.push(sep);
            if block.num_rows > 100 {
                out.push(format!("... ({} rows total, showing 100)", block.num_rows));
            } else {
                out.push(format!("{} rows", block.num_rows));
            }
            out.join("\n")
        }
        Err(e) => format!("Query error: {e}"),
    }
}

// ─── DataBlock helpers ────────────────────────────────────────────────────────

/// Positional column access helpers — avoid table-prefix naming issues
fn col_str(block: &DataBlock, col_idx: usize, row: usize) -> String {
    block.columns.get(col_idx)
        .map(|c| match c.data.get_value(row) {
            Value::Str(s) => s,
            Value::Null   => String::new(),
            v             => format!("{v:?}"),
        })
        .unwrap_or_default()
}
fn col_f64(block: &DataBlock, col_idx: usize, row: usize) -> f64 {
    block.columns.get(col_idx)
        .map(|c| match c.data.get_value(row) {
            Value::Float(f) => f,
            Value::Int(i)   => i as f64,
            _               => 0.0,
        })
        .unwrap_or(0.0)
}
fn col_i64(block: &DataBlock, col_idx: usize, row: usize) -> i64 {
    block.columns.get(col_idx)
        .map(|c| match c.data.get_value(row) {
            Value::Int(i)   => i,
            Value::Float(f) => f as i64,
            _               => 0,
        })
        .unwrap_or(0)
}

pub fn block_to_rows(block: &DataBlock) -> Vec<Vec<String>> {
    (0..block.num_rows).map(|i| {
        block.columns.iter().map(|c| match c.data.get_value(i) {
            Value::Null      => "NULL".to_string(),
            Value::Int(n)    => n.to_string(),
            Value::Float(f)  => format!("{f:.4}"),
            Value::Bool(b)   => b.to_string(),
            Value::Str(s)    => s,
        }).collect()
    }).collect()
}

fn str_val(block: &DataBlock, col: &str, row: usize) -> String {
    // Try exact name first, then "memories.<col>" prefix (KORE SQL table-qualified names)
    let col_ref = block.column(col)
        .or_else(|| block.column(&format!("memories.{col}")))
        .or_else(|| block.columns.iter().find(|c| c.name.ends_with(&format!(".{col}")) || c.name == col));
    col_ref
        .map(|c| match c.data.get_value(row) {
            Value::Str(s) => s,
            v             => format!("{v:?}"),
        })
        .unwrap_or_default()
}

fn f64_val(block: &DataBlock, col: &str, row: usize) -> f64 {
    let col_ref = block.column(col)
        .or_else(|| block.column(&format!("memories.{col}")))
        .or_else(|| block.columns.iter().find(|c| c.name.ends_with(&format!(".{col}")) || c.name == col));
    col_ref
        .map(|c| match c.data.get_value(row) {
            Value::Float(f) => f,
            Value::Int(i)   => i as f64,
            _               => 0.0,
        })
        .unwrap_or(0.0)
}

fn i64_val(block: &DataBlock, col: &str, row: usize) -> i64 {
    let col_ref = block.column(col)
        .or_else(|| block.column(&format!("memories.{col}")))
        .or_else(|| block.columns.iter().find(|c| c.name.ends_with(&format!(".{col}")) || c.name == col));
    col_ref
        .map(|c| match c.data.get_value(row) {
            Value::Int(i)   => i,
            Value::Float(f) => f as i64,
            _               => 0,
        })
        .unwrap_or(0)
}

fn extract_kind_rows(block: &DataBlock) -> Vec<(String, i64, f64)> {
    (0..block.num_rows).map(|i| {
        // Use positional access — columns are always (kind, cnt, avg_imp) in order
        let kind = match block.columns.get(0).map(|c| c.data.get_value(i)) {
            Some(Value::Str(s)) => s,
            _ => String::new(),
        };
        let cnt = match block.columns.get(1).map(|c| c.data.get_value(i)) {
            Some(Value::Int(n))   => n,
            Some(Value::Float(f)) => f as i64,
            _ => 0,
        };
        let avg = match block.columns.get(2).map(|c| c.data.get_value(i)) {
            Some(Value::Float(f)) => f,
            Some(Value::Int(n))   => n as f64,
            _ => 0.0,
        };
        (kind, cnt, avg)
    }).collect()
}

// ─── Value Display ────────────────────────────────────────────────────────────

trait ValueDisplay {
    fn to_string(&self) -> String;
    fn type_name(&self) -> &str;
}

impl ValueDisplay for Value {
    fn to_string(&self) -> String {
        match self {
            Value::Null      => "NULL".to_string(),
            Value::Int(n)    => n.to_string(),
            Value::Float(f)  => format!("{f:.4}"),
            Value::Bool(b)   => b.to_string(),
            Value::Str(s)    => s.clone(),
        }
    }
    fn type_name(&self) -> &str {
        match self {
            Value::Null    => "null",
            Value::Int(_)  => "int",
            Value::Float(_)=> "float",
            Value::Bool(_) => "bool",
            Value::Str(_)  => "str",
        }
    }
}
