// ============================================================================
// KORE ∞ — Layer 2: kore_mind
// ============================================================================
//
// Intent engine: natural language patterns → structured query → executes on .kore
//
// No ML, no internet, no dependencies — pure Rust pattern matching.
// Handles the 20 most common data engineering query patterns.
//
// Python API:
//   from kore_fileformat import KoreMind
//   m = KoreMind("data.kore")
//   m.ask("top 5 by amount")
//   m.ask("count by category")
//   m.ask("average amount by category")
//   m.ask("where score > 500")
//   m.ask("null columns")
//   m.ask("describe")
// ============================================================================

use crate::kore_v2::{KoreReader, KVal};
use std::collections::HashMap;

// ── Query types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum KoreQuery {
    Describe,
    TopN     { col: String, n: usize, asc: bool },
    GroupBy  { group_col: String, agg: Agg, agg_col: Option<String> },
    Filter   { col: String, op: CmpOp, val: KVal },
    Select   { cols: Vec<String>, limit: Option<usize> },
    NullCols,
    Unique   { col: String },
    Sample   { n: usize },
    Unknown  { raw: String },
}

#[derive(Debug, Clone)]
pub enum Agg { Count, Sum, Avg, Min, Max }

#[derive(Debug, Clone)]
pub enum CmpOp { Gt, Gte, Lt, Lte, Eq, Neq, Contains }

// ── Query result ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub query_text: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
    pub execution_note: String,
}

impl QueryResult {
    pub fn to_table(&self) -> String {
        if self.rows.is_empty() {
            return format!("  (no results)\n  Query: {}\n", self.query_text);
        }
        // Compute column widths
        let ncols = self.headers.len();
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.len()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < ncols { widths[i] = widths[i].max(cell.len()); }
            }
        }
        let mut out = String::new();
        out.push_str(&format!("  Query: {}\n", self.query_text));
        if !self.execution_note.is_empty() {
            out.push_str(&format!("  Note : {}\n", self.execution_note));
        }
        out.push_str("  ");
        for (i, h) in self.headers.iter().enumerate() {
            out.push_str(&format!("{:<width$}  ", h, width = widths[i]));
        }
        out.push('\n');
        out.push_str("  ");
        for w in &widths { out.push_str(&"-".repeat(w + 2)); }
        out.push('\n');
        for row in &self.rows {
            out.push_str("  ");
            for (i, cell) in row.iter().enumerate() {
                let w = widths.get(i).copied().unwrap_or(10);
                out.push_str(&format!("{:<width$}  ", cell, width = w));
            }
            out.push('\n');
        }
        out.push_str(&format!("  ({} row{})\n", self.row_count,
            if self.row_count == 1 { "" } else { "s" }));
        out
    }

    pub fn to_json(&self) -> String {
        let mut j = String::from("[\n");
        for (ri, row) in self.rows.iter().enumerate() {
            j.push_str("  {");
            for (ci, cell) in row.iter().enumerate() {
                let key = self.headers.get(ci).map(|s| s.as_str()).unwrap_or("col");
                // Detect numeric vs string
                let val = if cell.parse::<f64>().is_ok() {
                    cell.clone()
                } else {
                    format!("\"{}\"", cell.replace('"', "\\\""))
                };
                j.push_str(&format!("\"{}\":{}", key, val));
                if ci + 1 < row.len() { j.push(','); }
            }
            j.push('}');
            if ri + 1 < self.rows.len() { j.push(','); }
            j.push('\n');
        }
        j.push(']');
        j
    }
}

// ── Intent parser ─────────────────────────────────────────────────────────────

fn tokenize(input: &str) -> Vec<String> {
    input.to_lowercase()
        .split_whitespace()
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '.' && c != '-').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn find_col<'a>(tokens: &[String], cols: &'a [String]) -> Option<&'a String> {
    for tok in tokens {
        if let Some(c) = cols.iter().find(|c| c.to_lowercase() == *tok) {
            return Some(c);
        }
    }
    None
}

fn parse_intent(input: &str, col_names: &[String]) -> KoreQuery {
    let toks = tokenize(input);
    let lower = input.to_lowercase();

    // describe / schema
    if toks.iter().any(|t| matches!(t.as_str(), "describe" | "schema" | "columns" | "info")) {
        return KoreQuery::Describe;
    }

    // null columns
    if lower.contains("null") && (lower.contains("col") || lower.contains("miss") || lower.contains("empty")) {
        return KoreQuery::NullCols;
    }

    // sample N
    if lower.contains("sample") || lower.contains("preview") || lower.contains("head") {
        let n = toks.iter().filter_map(|t| t.parse::<usize>().ok()).next().unwrap_or(5);
        return KoreQuery::Sample { n };
    }

    // unique / distinct col
    if lower.contains("unique") || lower.contains("distinct") {
        if let Some(col) = find_col(&toks, col_names) {
            return KoreQuery::Unique { col: col.clone() };
        }
    }

    // top N by col / bottom N by col
    let is_top    = lower.contains("top") || lower.contains("highest") || lower.contains("largest") || lower.contains("most");
    let is_bottom = lower.contains("bottom") || lower.contains("lowest") || lower.contains("smallest") || lower.contains("least");
    if is_top || is_bottom {
        let n = toks.iter().filter_map(|t| t.parse::<usize>().ok()).next().unwrap_or(10);
        if let Some(col) = find_col(&toks, col_names) {
            return KoreQuery::TopN { col: col.clone(), n, asc: is_bottom };
        }
    }

    // aggregation: count/sum/avg/min/max by col
    let agg = if lower.contains("count") || lower.contains("how many") {
        Some(Agg::Count)
    } else if lower.contains("sum") || lower.contains("total") {
        Some(Agg::Sum)
    } else if lower.contains("avg") || lower.contains("average") || lower.contains("mean") {
        Some(Agg::Avg)
    } else if lower.contains("min") || lower.contains("minimum") || lower.contains("smallest") {
        Some(Agg::Min)
    } else if lower.contains("max") || lower.contains("maximum") || lower.contains("largest") {
        Some(Agg::Max)
    } else {
        None
    };

    if let Some(agg) = agg {
        // "avg amount by category" → group_col=category, agg_col=amount
        let by_idx = toks.iter().position(|t| t == "by");
        let group_col = if let Some(bi) = by_idx {
            find_col(&toks[bi..], col_names)
        } else {
            find_col(&toks, col_names)
        };
        let agg_col_name = match &agg {
            Agg::Count => None,
            _ => {
                // find numeric col that is NOT the group col
                let exclude = group_col.map(|c| c.as_str()).unwrap_or("");
                col_names.iter().find(|c| c.as_str() != exclude)
                    .or_else(|| find_col(&toks, col_names))
                    .cloned()
            }
        };
        if let Some(gc) = group_col {
            return KoreQuery::GroupBy {
                group_col: gc.clone(),
                agg,
                agg_col: agg_col_name,
            };
        }
    }

    // filter: where col > value
    let ops: &[(&str, CmpOp)] = &[
        (">=", CmpOp::Gte), (">", CmpOp::Gt),
        ("<=", CmpOp::Lte), ("<", CmpOp::Lt),
        ("!=", CmpOp::Neq), ("=", CmpOp::Eq),
        ("contains", CmpOp::Contains), ("like", CmpOp::Contains),
    ];
    for (op_str, op) in ops {
        if lower.contains(op_str) {
            // find col and value around the operator
            let parts: Vec<&str> = input.splitn(2, op_str).collect();
            if parts.len() == 2 {
                let left_toks = tokenize(parts[0]);
                let right = parts[1].trim();
                if let Some(col) = find_col(&left_toks, col_names) {
                    let val = if let Ok(i) = right.parse::<i64>() {
                        KVal::Int(i)
                    } else if let Ok(f) = right.parse::<f64>() {
                        KVal::Float(f)
                    } else {
                        KVal::Str(right.trim_matches('"').trim_matches('\'').to_string())
                    };
                    return KoreQuery::Filter { col: col.clone(), op: op.clone(), val };
                }
            }
        }
    }

    // select cols limit N
    let mentioned: Vec<String> = col_names.iter()
        .filter(|c| lower.contains(c.to_lowercase().as_str()))
        .cloned().collect();
    if !mentioned.is_empty() {
        let limit = toks.iter().filter_map(|t| t.parse::<usize>().ok()).next();
        return KoreQuery::Select { cols: mentioned, limit };
    }

    KoreQuery::Unknown { raw: input.to_string() }
}

// ── Query executor ────────────────────────────────────────────────────────────

pub struct KoreMind {
    path: String,
}

impl KoreMind {
    pub fn new(path: &str) -> Self {
        KoreMind { path: path.to_string() }
    }

    pub fn ask(&self, question: &str) -> Result<QueryResult, String> {
        let reader = KoreReader::open(&self.path)?;
        let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();
        let query = parse_intent(question, &col_names);
        self.execute(question, query, &reader)
    }

    fn execute(&self, raw: &str, query: KoreQuery, reader: &KoreReader) -> Result<QueryResult, String> {
        let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();
        let col_data = reader.read_all_columns();

        match query {
            KoreQuery::Describe => {
                let headers = vec!["column".to_string(), "type".to_string(),
                    "rows".to_string(), "nulls".to_string()];
                let rows: Vec<Vec<String>> = reader.columns.iter().enumerate().map(|(i, col)| {
                    let nulls = col_data.get(i).map(|v| {
                        v.iter().filter(|x| matches!(x, KVal::Null)).count()
                    }).unwrap_or(0);
                    vec![col.name.clone(), format!("{:?}", col.ktype),
                        reader.nrows.to_string(), nulls.to_string()]
                }).collect();
                let n = rows.len();
                Ok(QueryResult { query_text: raw.to_string(), headers, rows,
                    row_count: n, execution_note: String::new() })
            }

            KoreQuery::NullCols => {
                let headers = vec!["column".to_string(), "null_count".to_string(), "null_pct".to_string()];
                let rows: Vec<Vec<String>> = reader.columns.iter().enumerate().filter_map(|(i, col)| {
                    let nulls = col_data.get(i).map(|v|
                        v.iter().filter(|x| matches!(x, KVal::Null)).count()).unwrap_or(0);
                    if nulls > 0 {
                        let pct = nulls as f64 / reader.nrows as f64 * 100.0;
                        Some(vec![col.name.clone(), nulls.to_string(), format!("{:.1}%", pct)])
                    } else { None }
                }).collect();
                let n = rows.len();
                Ok(QueryResult { query_text: raw.to_string(), headers, rows,
                    row_count: n,
                    execution_note: if n == 0 { "No null columns found".to_string() } else { String::new() } })
            }

            KoreQuery::Sample { n } => {
                let headers = col_names.clone();
                let rows: Vec<Vec<String>> = (0..n.min(reader.nrows)).map(|ri| {
                    col_data.iter().map(|col_vals| {
                        col_vals.get(ri).map(|v| v.display()).unwrap_or_default()
                    }).collect()
                }).collect();
                let actual = rows.len();
                Ok(QueryResult { query_text: raw.to_string(), headers, rows,
                    row_count: actual, execution_note: format!("showing first {} rows", actual) })
            }

            KoreQuery::TopN { col, n, asc } => {
                let ci = col_names.iter().position(|c| *c == col)
                    .ok_or(format!("Column '{}' not found", col))?;
                let vals = col_data.get(ci).ok_or("Column data missing")?;

                // Build sortable vec
                let mut indexed: Vec<(usize, f64)> = vals.iter().enumerate()
                    .filter_map(|(i, v)| match v {
                        KVal::Int(x)   => Some((i, *x as f64)),
                        KVal::Float(x) => Some((i, *x)),
                        _ => None,
                    }).collect();
                if asc { indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap()); }
                else   { indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap()); }
                indexed.truncate(n);

                let headers = col_names.clone();
                let rows: Vec<Vec<String>> = indexed.iter().map(|(ri, _)| {
                    col_data.iter().map(|cd|
                        cd.get(*ri).map(|v| v.display()).unwrap_or_default()
                    ).collect()
                }).collect();
                let actual = rows.len();
                Ok(QueryResult { query_text: raw.to_string(), headers, rows,
                    row_count: actual,
                    execution_note: format!("{} {} by {}", if asc {"bottom"} else {"top"}, n, col) })
            }

            KoreQuery::GroupBy { group_col, agg, agg_col } => {
                let gi = col_names.iter().position(|c| *c == group_col)
                    .ok_or(format!("Column '{}' not found", group_col))?;
                let group_vals = col_data.get(gi).ok_or("Group column data missing")?;

                // Group row indices
                let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
                for (ri, v) in group_vals.iter().enumerate() {
                    groups.entry(v.display()).or_default().push(ri);
                }

                let agg_label = match &agg {
                    Agg::Count => "count".to_string(),
                    Agg::Sum   => format!("sum({})", agg_col.as_deref().unwrap_or("?")),
                    Agg::Avg   => format!("avg({})", agg_col.as_deref().unwrap_or("?")),
                    Agg::Min   => format!("min({})", agg_col.as_deref().unwrap_or("?")),
                    Agg::Max   => format!("max({})", agg_col.as_deref().unwrap_or("?")),
                };
                let headers = vec![group_col.clone(), agg_label];

                let agg_ci = agg_col.as_ref().and_then(|ac|
                    col_names.iter().position(|c| c == ac));

                let mut result_rows: Vec<(String, f64)> = groups.iter().map(|(key, idxs)| {
                    let agg_val = match &agg {
                        Agg::Count => idxs.len() as f64,
                        _ => {
                            if let Some(aci) = agg_ci {
                                let nums: Vec<f64> = idxs.iter().filter_map(|&ri| {
                                    col_data.get(aci)?.get(ri).and_then(|v| match v {
                                        KVal::Int(x) => Some(*x as f64),
                                        KVal::Float(x) => Some(*x),
                                        _ => None,
                                    })
                                }).collect();
                                if nums.is_empty() { return (key.clone(), 0.0); }
                                match &agg {
                                    Agg::Sum => nums.iter().sum(),
                                    Agg::Avg => nums.iter().sum::<f64>() / nums.len() as f64,
                                    Agg::Min => nums.iter().cloned().fold(f64::INFINITY, f64::min),
                                    Agg::Max => nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                                    _ => 0.0,
                                }
                            } else { idxs.len() as f64 }
                        }
                    };
                    (key.clone(), agg_val)
                }).collect();
                result_rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

                let rows: Vec<Vec<String>> = result_rows.iter().map(|(k, v)| {
                    let v_str = match &agg {
                        Agg::Count => format!("{}", *v as u64),
                        _          => format!("{:.2}", v),
                    };
                    vec![k.clone(), v_str]
                }).collect();
                let n = rows.len();
                Ok(QueryResult { query_text: raw.to_string(), headers, rows,
                    row_count: n, execution_note: String::new() })
            }

            KoreQuery::Filter { col, op, val } => {
                let ci = col_names.iter().position(|c| *c == col)
                    .ok_or(format!("Column '{}' not found", col))?;
                let col_vals = col_data.get(ci).ok_or("Column data missing")?;

                let matching: Vec<usize> = col_vals.iter().enumerate().filter_map(|(ri, v)| {
                    let matches = match (&op, v, &val) {
                        (CmpOp::Gt,  KVal::Int(a),   KVal::Int(b))   => a > b,
                        (CmpOp::Gt,  KVal::Float(a), KVal::Float(b)) => a > b,
                        (CmpOp::Gt,  KVal::Int(a),   KVal::Float(b)) => (*a as f64) > *b,
                        (CmpOp::Gte, KVal::Int(a),   KVal::Int(b))   => a >= b,
                        (CmpOp::Gte, KVal::Int(a),   KVal::Float(b)) => (*a as f64) >= *b,
                        (CmpOp::Lt,  KVal::Int(a),   KVal::Int(b))   => a < b,
                        (CmpOp::Lt,  KVal::Float(a), KVal::Float(b)) => a < b,
                        (CmpOp::Lt,  KVal::Int(a),   KVal::Float(b)) => (*a as f64) < *b,
                        (CmpOp::Lte, KVal::Int(a),   KVal::Int(b))   => a <= b,
                        (CmpOp::Lte, KVal::Int(a),   KVal::Float(b)) => (*a as f64) <= *b,
                        (CmpOp::Eq,  a,              b)               => a.display() == b.display(),
                        (CmpOp::Neq, a,              b)               => a.display() != b.display(),
                        (CmpOp::Contains, KVal::Str(a), KVal::Str(b)) => a.to_lowercase().contains(b.to_lowercase().as_str()),
                        _ => false,
                    };
                    if matches { Some(ri) } else { None }
                }).take(1000).collect();

                let headers = col_names.clone();
                let rows: Vec<Vec<String>> = matching.iter().map(|&ri| {
                    col_data.iter().map(|cd|
                        cd.get(ri).map(|v| v.display()).unwrap_or_default()
                    ).collect()
                }).collect();
                let n = rows.len();
                Ok(QueryResult { query_text: raw.to_string(), headers, rows,
                    row_count: n,
                    execution_note: if n == 1000 { "capped at 1000 rows".to_string() } else { String::new() } })
            }

            KoreQuery::Unique { col } => {
                let ci = col_names.iter().position(|c| *c == col)
                    .ok_or(format!("Column '{}' not found", col))?;
                let mut seen = std::collections::HashSet::new();
                let vals: Vec<Vec<String>> = col_data.get(ci).map(|cd|
                    cd.iter().filter_map(|v| {
                        let s = v.display();
                        if seen.insert(s.clone()) { Some(vec![s]) } else { None }
                    }).collect()
                ).unwrap_or_default();
                let n = vals.len();
                Ok(QueryResult { query_text: raw.to_string(),
                    headers: vec![col],
                    rows: vals, row_count: n, execution_note: String::new() })
            }

            KoreQuery::Select { cols, limit } => {
                let indices: Vec<usize> = cols.iter().filter_map(|c|
                    col_names.iter().position(|cn| cn == c)
                ).collect();
                let max_rows = limit.unwrap_or(20).min(reader.nrows);
                let rows: Vec<Vec<String>> = (0..max_rows).map(|ri| {
                    indices.iter().map(|&ci|
                        col_data.get(ci).and_then(|cd| cd.get(ri))
                            .map(|v| v.display()).unwrap_or_default()
                    ).collect()
                }).collect();
                let n = rows.len();
                Ok(QueryResult { query_text: raw.to_string(), headers: cols,
                    rows, row_count: n, execution_note: String::new() })
            }

            KoreQuery::Unknown { raw: r } => {
                Ok(QueryResult {
                    query_text: r.clone(),
                    headers: vec!["message".to_string()],
                    rows: vec![vec![
                        format!("Unknown query: '{}'. Try: 'top 5 by amount', 'count by category', 'where score > 500', 'describe', 'sample 10'", r)
                    ]],
                    row_count: 0,
                    execution_note: "query not understood".to_string(),
                })
            }
        }
    }

    /// List supported query patterns
    pub fn help() -> String {
        r#"
 KORE MIND — Query Patterns:
  describe                       → column names, types, null counts
  sample 10                      → first N rows
  top 5 by amount                → highest N rows by numeric column
  bottom 5 by score              → lowest N rows by numeric column
  count by category              → GROUP BY category COUNT(*)
  total amount by category       → GROUP BY category SUM(amount)
  average amount by category     → GROUP BY category AVG(amount)
  min score by category          → GROUP BY category MIN(score)
  max amount by category         → GROUP BY category MAX(amount)
  where score > 500              → filter rows
  where category = sales         → filter by string value
  where name contains user_1     → string contains filter
  null columns                   → show columns with missing data
  unique category                → distinct values in a column
"#.to_string()
    }
}
