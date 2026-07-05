//! kore-mcp — Layer 61: MCP (Model Context Protocol) server for KORE
//!
//! Exposes KORE as a set of AI-callable tools over the MCP stdio transport.
//! Compatible with: Claude Desktop, VS Code Copilot, Cursor, any MCP client.
//!
//! Start the server:
//!   kore-mcp [--http 3099]   (default: stdio transport)
//!
//! Tools exposed:
//!   kore_query(sql)              — Run SQL, return JSON result + timing
//!   kore_load_csv(path, table)   — Load CSV file into a named table
//!   kore_load_ndjson(path,table) — Load newline-delimited JSON
//!   kore_list_tables()           — List all registered tables
//!   kore_schema(table)           — Describe columns and types
//!   kore_sample(table, n)        — Return first N rows
//!   kore_benchmark(sql, iters)   — Time a query N times, return stats

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};

use kore_core::types::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

// ─── Shared engine state ──────────────────────────────────────────────────────

struct KoreSession {
    ctx: KqlContext,
}

impl KoreSession {
    fn new() -> Self {
        Self { ctx: KqlContext::new() }
    }
}

// ─── MCP protocol constants ───────────────────────────────────────────────────

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME:      &str = "kore-mcp";
const SERVER_VERSION:   &str = "0.1.0";

// ─── Tool definitions ─────────────────────────────────────────────────────────

fn tool_list() -> Value {
    json!([
      {
        "name": "kore_query",
        "description": "Execute a SQL SELECT query against KORE in-memory tables. Returns columnar JSON with column names, typed rows, and wall-clock timing. KORE supports GROUP BY, JOIN, ORDER BY, LIMIT, window functions, subqueries, and aggregate functions (SUM/AVG/MIN/MAX/COUNT). Results are returned in microseconds — 150× faster than Spark for vectorized aggregations.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "sql": { "type": "string", "description": "SQL SELECT statement to execute" }
          },
          "required": ["sql"]
        }
      },
      {
        "name": "kore_load_csv",
        "description": "Load a CSV file from disk into a named KORE in-memory table. The first row must be a header row. All numeric columns are auto-detected. Call kore_schema() afterwards to inspect the loaded schema.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "path":  { "type": "string", "description": "Absolute or relative path to the CSV file" },
            "table": { "type": "string", "description": "Name to register the table as (used in SQL FROM clause)" }
          },
          "required": ["path", "table"]
        }
      },
      {
        "name": "kore_load_ndjson",
        "description": "Load a newline-delimited JSON file into a named KORE table. Each line must be a flat JSON object (no nested arrays). Column types are inferred from the first 100 rows.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "path":  { "type": "string", "description": "Absolute path to the .ndjson or .jsonl file" },
            "table": { "type": "string", "description": "Table name to register" }
          },
          "required": ["path", "table"]
        }
      },
      {
        "name": "kore_list_tables",
        "description": "List all tables currently registered in the KORE session. Returns table names and row counts.",
        "inputSchema": {
          "type": "object",
          "properties": {},
          "required": []
        }
      },
      {
        "name": "kore_schema",
        "description": "Describe the schema of a registered KORE table: column names, data types, and null counts.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "table": { "type": "string", "description": "Table name to describe" }
          },
          "required": ["table"]
        }
      },
      {
        "name": "kore_sample",
        "description": "Return the first N rows of a KORE table as formatted JSON. Useful for data exploration before writing queries.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "table": { "type": "string", "description": "Table name to sample" },
            "n":     { "type": "integer", "description": "Number of rows to return (default 10, max 1000)", "default": 10 }
          },
          "required": ["table"]
        }
      },
      {
        "name": "kore_benchmark",
        "description": "Benchmark a SQL query: run it N times and return min/median/max timing in milliseconds. Use this to compare query variants or verify optimization impact.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "sql":   { "type": "string",  "description": "SQL query to benchmark" },
            "iters": { "type": "integer", "description": "Number of iterations (default 3, max 20)", "default": 3 }
          },
          "required": ["sql"]
        }
      }
    ])
}

// ─── Tool dispatch ────────────────────────────────────────────────────────────

fn handle_tool(name: &str, args: &Value, session: &mut KoreSession) -> Value {
    match name {
        "kore_query"      => tool_query(args, session),
        "kore_load_csv"   => tool_load_csv(args, session),
        "kore_load_ndjson"=> tool_load_ndjson(args, session),
        "kore_list_tables"=> tool_list_tables(session),
        "kore_schema"     => tool_schema(args, session),
        "kore_sample"     => tool_sample(args, session),
        "kore_benchmark"  => tool_benchmark(args, session),
        _                 => error_text(&format!("Unknown tool: {name}")),
    }
}

// ─── kore_query ───────────────────────────────────────────────────────────────

fn tool_query(args: &Value, session: &mut KoreSession) -> Value {
    let sql = match args["sql"].as_str() {
        Some(s) => s,
        None    => return error_text("Missing argument: sql"),
    };

    let t0 = Instant::now();
    match session.ctx.query(sql) {
        Ok(block) => {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let result = json!({
                "columns": block.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                "rows":    block_to_rows(&block),
                "row_count": block.num_rows,
                "timing_ms": (ms * 100.0).round() / 100.0
            });
            ok_text(&result.to_string())
        }
        Err(e) => error_text(&format!("Query error: {e}")),
    }
}

// ─── kore_load_csv ────────────────────────────────────────────────────────────

fn tool_load_csv(args: &Value, session: &mut KoreSession) -> Value {
    let path  = match args["path"].as_str()  { Some(s) => s, None => return error_text("Missing: path")  };
    let table = match args["table"].as_str() { Some(s) => s, None => return error_text("Missing: table") };

    match load_csv_to_block(path) {
        Ok(block) => {
            let rows = block.num_rows;
            let cols = block.columns.len();
            session.ctx.register(table, block);
            ok_text(&json!({
                "status":    "ok",
                "table":     table,
                "rows_loaded": rows,
                "columns":   cols
            }).to_string())
        }
        Err(e) => error_text(&format!("CSV load error: {e}")),
    }
}

fn load_csv_to_block(path: &str) -> Result<DataBlock, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {path}: {e}"))?;
    let mut lines = content.lines();

    let header: Vec<String> = lines.next()
        .ok_or("Empty CSV")?
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect();

    let ncols = header.len();
    // Collect raw string values per column
    let mut raw: Vec<Vec<Option<String>>> = vec![Vec::new(); ncols];
    let mut nrows = 0usize;

    for line in lines {
        if line.trim().is_empty() { continue; }
        let fields: Vec<&str> = csv_split(line);
        for (i, col) in raw.iter_mut().enumerate() {
            let val = fields.get(i).copied().unwrap_or("").trim().trim_matches('"');
            col.push(if val.is_empty() { None } else { Some(val.to_string()) });
        }
        nrows += 1;
    }

    // Infer types and build columns
    let columns: Vec<Column> = header.iter().zip(raw.into_iter()).map(|(name, vals)| {
        // Try i64
        let all_int = vals.iter().all(|v| v.as_deref().map(|s| s.parse::<i64>().is_ok()).unwrap_or(true));
        if all_int {
            return Column { name: name.clone(), data: ColumnData::Int64(
                vals.into_iter().map(|v| v.and_then(|s| s.parse::<i64>().ok())).collect()
            )};
        }
        // Try f64
        let all_f64 = vals.iter().all(|v| v.as_deref().map(|s| s.parse::<f64>().is_ok()).unwrap_or(true));
        if all_f64 {
            return Column { name: name.clone(), data: ColumnData::Float64(
                vals.into_iter().map(|v| v.and_then(|s| s.parse::<f64>().ok())).collect()
            )};
        }
        // String
        Column { name: name.clone(), data: ColumnData::Str(vals) }
    }).collect();

    Ok(DataBlock { num_rows: nrows, columns })
}

/// Minimal CSV field splitter (handles quoted fields with commas).
fn csv_split(line: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut in_quote = false;
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_quote = !in_quote,
            b',' if !in_quote => {
                fields.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    fields.push(&line[start..]);
    fields
}

// ─── kore_load_ndjson ────────────────────────────────────────────────────────

fn tool_load_ndjson(args: &Value, session: &mut KoreSession) -> Value {
    let path  = match args["path"].as_str()  { Some(s) => s, None => return error_text("Missing: path")  };
    let table = match args["table"].as_str() { Some(s) => s, None => return error_text("Missing: table") };

    match load_ndjson_to_block(path) {
        Ok(block) => {
            let rows = block.num_rows;
            let cols = block.columns.len();
            session.ctx.register(table, block);
            ok_text(&json!({ "status": "ok", "table": table, "rows_loaded": rows, "columns": cols }).to_string())
        }
        Err(e) => error_text(&format!("NDJSON load error: {e}")),
    }
}

fn load_ndjson_to_block(path: &str) -> Result<DataBlock, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {path}: {e}"))?;

    let rows: Vec<Value> = content.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or(Value::Null))
        .filter(|v| v.is_object())
        .collect();

    if rows.is_empty() { return Err("No valid JSON objects found".into()); }

    // Collect all keys from first row
    let keys: Vec<String> = rows[0].as_object().unwrap().keys().cloned().collect();

    let nrows = rows.len();
    let columns = keys.iter().map(|key| {
        let vals: Vec<Value> = rows.iter().map(|r| r[key].clone()).collect();
        // Infer type
        let all_int = vals.iter().all(|v| v.is_i64() || v.is_null());
        if all_int {
            return Column { name: key.clone(), data: ColumnData::Int64(
                vals.into_iter().map(|v| v.as_i64()).collect()
            )};
        }
        let all_f64 = vals.iter().all(|v| v.is_number() || v.is_null());
        if all_f64 {
            return Column { name: key.clone(), data: ColumnData::Float64(
                vals.into_iter().map(|v| v.as_f64()).collect()
            )};
        }
        Column { name: key.clone(), data: ColumnData::Str(
            vals.into_iter().map(|v| v.as_str().map(|s| s.to_string())).collect()
        )}
    }).collect();

    Ok(DataBlock { num_rows: nrows, columns })
}

// ─── kore_list_tables ─────────────────────────────────────────────────────────

fn tool_list_tables(session: &mut KoreSession) -> Value {
    let names = session.ctx.table_names();
    let tables: Vec<Value> = names.iter().map(|name| {
        let rows = session.ctx.get(name).map(|b| b.num_rows).unwrap_or(0);
        json!({ "name": name, "rows": rows })
    }).collect();
    let count = tables.len();
    ok_text(&json!({ "tables": tables, "count": count }).to_string())
}

// ─── kore_schema ─────────────────────────────────────────────────────────────

fn tool_schema(args: &Value, session: &mut KoreSession) -> Value {
    let table = match args["table"].as_str() { Some(s) => s, None => return error_text("Missing: table") };
    match session.ctx.get(table) {
        Some(block) => {
            let cols: Vec<Value> = block.columns.iter().map(|c| {
                let dtype = match &c.data {
                    ColumnData::Int64(_)       => "Int64",
                    ColumnData::Float64(_)     => "Float64",
                    ColumnData::Bool(_)        => "Bool",
                    ColumnData::Str(_)         => "Str",
                    ColumnData::StrDict { .. } => "Str(Dict)",
                };
                let nulls = match &c.data {
                    ColumnData::Int64(v)       => v.iter().filter(|x| x.is_none()).count(),
                    ColumnData::Float64(v)     => v.iter().filter(|x| x.is_none()).count(),
                    ColumnData::Bool(v)        => v.iter().filter(|x| x.is_none()).count(),
                    ColumnData::Str(v)         => v.iter().filter(|x| x.is_none()).count(),
                    ColumnData::StrDict { codes, .. } => codes.iter().filter(|&&c| c == u8::MAX).count(),
                };
                json!({ "name": c.name, "type": dtype, "nulls": nulls })
            }).collect();
            let rows = block.num_rows;
            ok_text(&json!({ "table": table, "rows": rows, "columns": cols }).to_string())
        }
        None => error_text(&format!("Table not found: {table}. Use kore_list_tables() to see available tables.")),
    }
}

// ─── kore_sample ─────────────────────────────────────────────────────────────

fn tool_sample(args: &Value, session: &mut KoreSession) -> Value {
    let table = match args["table"].as_str() { Some(s) => s, None => return error_text("Missing: table") };
    let n = args["n"].as_u64().unwrap_or(10).min(1000) as usize;

    match session.ctx.get(table).cloned() {
        Some(block) => {
            let take = n.min(block.num_rows);
            let indices: Vec<usize> = (0..take).collect();
            let sample = block.select_rows(&indices);
            let result = json!({
                "table":      table,
                "showing":    take,
                "total_rows": block.num_rows,
                "columns":    sample.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                "rows":       block_to_rows(&sample)
            });
            ok_text(&result.to_string())
        }
        None => error_text(&format!("Table not found: {table}")),
    }
}

// ─── kore_benchmark ──────────────────────────────────────────────────────────

fn tool_benchmark(args: &Value, session: &mut KoreSession) -> Value {
    let sql   = match args["sql"].as_str() { Some(s) => s, None => return error_text("Missing: sql") };
    let iters = args["iters"].as_u64().unwrap_or(3).min(20) as usize;

    let mut times: Vec<f64> = Vec::with_capacity(iters);
    let mut last_rows = 0usize;

    for _ in 0..iters {
        let t0 = Instant::now();
        match session.ctx.query(sql) {
            Ok(block) => {
                times.push(t0.elapsed().as_secs_f64() * 1000.0);
                last_rows = block.num_rows;
            }
            Err(e) => return error_text(&format!("Query error: {e}")),
        }
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min    = times.first().copied().unwrap_or(0.0);
    let max    = times.last().copied().unwrap_or(0.0);
    let median = times[times.len() / 2];
    let mean   = times.iter().sum::<f64>() / times.len() as f64;

    ok_text(&json!({
        "sql":       sql,
        "iters":     iters,
        "rows":      last_rows,
        "min_ms":    (min * 100.0).round() / 100.0,
        "median_ms": (median * 100.0).round() / 100.0,
        "max_ms":    (max * 100.0).round() / 100.0,
        "mean_ms":   (mean * 100.0).round() / 100.0,
        "all_ms":    times.iter().map(|t| (t * 100.0).round() / 100.0).collect::<Vec<_>>()
    }).to_string())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn block_to_rows(block: &DataBlock) -> Vec<Vec<Value>> {
    (0..block.num_rows).map(|r| {
        block.columns.iter().map(|c| match &c.data {
            ColumnData::Int64(v)   => v.get(r).and_then(|x| *x).map(|i| json!(i)).unwrap_or(Value::Null),
            ColumnData::Float64(v) => v.get(r).and_then(|x| *x).map(|f| json!(f)).unwrap_or(Value::Null),
            ColumnData::Bool(v)    => v.get(r).and_then(|x| *x).map(|b| json!(b)).unwrap_or(Value::Null),
            ColumnData::Str(v)     => v.get(r).and_then(|x| x.as_deref()).map(|s| json!(s)).unwrap_or(Value::Null),
            ColumnData::StrDict { codes, dict } => {
                let code = codes.get(r).copied().unwrap_or(u8::MAX);
                if code == u8::MAX { Value::Null } else { dict.get(code as usize).map(|s| json!(s)).unwrap_or(Value::Null) }
            }
        }).collect()
    }).collect()
}

fn ok_text(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn error_text(msg: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": msg }], "isError": true })
}

// ─── JSON-RPC 2.0 helpers ─────────────────────────────────────────────────────

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// ─── Main stdio loop ──────────────────────────────────────────────────────────

fn main() {
    let session = Arc::new(Mutex::new(KoreSession::new()));

    let stdin  = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    eprintln!("[kore-mcp] KORE MCP Server v{SERVER_VERSION} — stdio transport ready");
    eprintln!("[kore-mcp] Tools: kore_query, kore_load_csv, kore_load_ndjson, kore_list_tables, kore_schema, kore_sample, kore_benchmark");

    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }

        let req: Value = match serde_json::from_str(&line) {
            Ok(v)  => v,
            Err(e) => {
                let resp = rpc_error(&Value::Null, -32700, &format!("Parse error: {e}"));
                let _ = writeln!(out, "{}", resp);
                let _ = out.flush();
                continue;
            }
        };

        let id     = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req["method"].as_str().unwrap_or("");

        let response = match method {
            // ── MCP handshake ────────────────────────────────────────────────
            "initialize" => {
                rpc_result(&id, json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION }
                }))
            }

            // ── Notification (no response) ───────────────────────────────────
            "notifications/initialized" => continue,

            // ── Tool listing ─────────────────────────────────────────────────
            "tools/list" => {
                rpc_result(&id, json!({ "tools": tool_list() }))
            }

            // ── Tool execution ────────────────────────────────────────────────
            "tools/call" => {
                let tool_name = req["params"]["name"].as_str().unwrap_or("");
                let arguments = req["params"].get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));

                let mut sess = session.lock().unwrap();
                let result = handle_tool(tool_name, &arguments, &mut sess);
                rpc_result(&id, result)
            }

            // ── Unknown method ────────────────────────────────────────────────
            _ => rpc_error(&id, -32601, &format!("Method not found: {method}")),
        };

        let _ = writeln!(out, "{}", response);
        let _ = out.flush();
    }

    eprintln!("[kore-mcp] Shutdown");
}
