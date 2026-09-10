//! kore-submit — CLI for submitting jobs to a KORE cluster
//!
//! Similar to spark-submit: connects to a running coordinator and executes
//! distributed SQL queries, optionally uploading data first.

use std::process;
use kore_core::DataBlock;
use kore_net::{KoreFrame, KoreMsg, now_ms};

// ─── CLI argument parsing ─────────────────────────────────────────────────────

struct Args {
    master:  String,
    sql:     Option<String>,
    file:    Option<String>,
    table:   String,
    data:    Option<String>,
    format:  OutputFormat,
    token:   Option<String>,
    ping:    bool,
}

#[derive(Clone, Copy)]
enum OutputFormat {
    Table,
    Json,
    Csv,
}

impl Args {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();

        if args.len() < 2 || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
            print_usage();
            process::exit(0);
        }

        let mut master = String::from("127.0.0.1:9090");
        let mut sql = None;
        let mut file = None;
        let mut table = String::from("data");
        let mut data = None;
        let mut format = OutputFormat::Table;
        let mut token = std::env::var("KORE_CLIENT_TOKEN").ok();
        let mut ping = false;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--master" => { i += 1; master = arg_value(&args, i); }
                "--sql"    => { i += 1; sql = Some(arg_value(&args, i)); }
                "--file"   => { i += 1; file = Some(arg_value(&args, i)); }
                "--table"  => { i += 1; table = arg_value(&args, i); }
                "--data"   => { i += 1; data = Some(arg_value(&args, i)); }
                "--format" => {
                    i += 1;
                    format = match arg_value(&args, i).as_str() {
                        "json" => OutputFormat::Json,
                        "csv"  => OutputFormat::Csv,
                        "table" => OutputFormat::Table,
                        other => {
                            eprintln!("error: unknown format '{}' (use table, json, or csv)", other);
                            process::exit(1);
                        }
                    };
                }
                "--token"  => { i += 1; token = Some(arg_value(&args, i)); }
                "--ping"   => { ping = true; }
                other => {
                    eprintln!("error: unknown option '{}'", other);
                    eprintln!("Run with --help for usage.");
                    process::exit(1);
                }
            }
            i += 1;
        }

        Self { master, sql, file, table, data, format, token, ping }
    }
}

fn arg_value(args: &[String], idx: usize) -> String {
    if idx >= args.len() {
        eprintln!("error: expected value after '{}'", args[idx - 1]);
        process::exit(1);
    }
    args[idx].clone()
}

fn print_usage() {
    println!(
r#"kore-submit — Submit jobs to a KORE cluster

USAGE:
  kore-submit --master <host:port> --sql "SELECT ..." [options]
  kore-submit --master <host:port> --file query.sql [options]
  kore-submit --master <host:port> --table orders --data orders.parquet --sql "SELECT ..."
  kore-submit --master <host:port> --ping

OPTIONS:
  --master <addr>    Coordinator address (default: 127.0.0.1:9090)
  --sql <query>      SQL query to execute
  --file <path>      Read SQL from a file instead of --sql
  --table <name>     Table name for the uploaded data (default: "data")
  --data <path>      Data file to upload (.parquet or .csv)
  --format <fmt>     Output format: table (default), json, csv
  --token <secret>   Auth token (also reads KORE_CLIENT_TOKEN env var)
  --ping             Check if the coordinator is reachable
  --help, -h         Show this help message

EXAMPLES:
  # Ping the coordinator
  kore-submit --master 10.0.0.1:9090 --ping

  # Run a SQL query on previously loaded cluster data
  kore-submit --master 10.0.0.1:9090 --sql "SELECT region, SUM(sales) FROM sales GROUP BY region"

  # Upload a parquet file and query it
  kore-submit --master 10.0.0.1:9090 --table orders --data orders.parquet \
    --sql "SELECT status, COUNT(*) AS cnt FROM orders GROUP BY status"

  # Read SQL from a file and output as JSON
  kore-submit --master 10.0.0.1:9090 --file report.sql --format json

  # Upload CSV data with auth token
  kore-submit --master 10.0.0.1:9090 --table events --data events.csv \
    --sql "SELECT * FROM events WHERE level = 'ERROR'" --token my-secret

ENVIRONMENT:
  KORE_CLIENT_TOKEN  Auth token (overridden by --token flag)
"#);
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| {
        eprintln!("error: failed to start async runtime: {}", e);
        process::exit(1);
    });

    rt.block_on(async move {
        if args.ping {
            run_ping(&args.master).await;
        } else {
            run_query(&args).await;
        }
    });
}

// ─── Ping ─────────────────────────────────────────────────────────────────────

async fn run_ping(master: &str) {
    let start = std::time::Instant::now();
    match tokio::net::TcpStream::connect(master).await {
        Ok(mut stream) => {
            if let Err(e) = KoreFrame::write(&mut stream, &KoreMsg::Ping).await {
                eprintln!("FAIL — connected but write failed: {}", e);
                process::exit(1);
            }
            match KoreFrame::read(&mut stream).await {
                Ok(KoreMsg::Pong) => {
                    let ms = start.elapsed().as_millis();
                    println!("PONG — coordinator at {} is reachable ({}ms)", master, ms);
                }
                Ok(other) => {
                    eprintln!("WARN — unexpected response: {:?}", other);
                    process::exit(1);
                }
                Err(e) => {
                    eprintln!("FAIL — read error: {}", e);
                    process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("FAIL — cannot connect to {}: {}", master, e);
            process::exit(1);
        }
    }
}

// ─── Query execution ──────────────────────────────────────────────────────────

async fn run_query(args: &Args) {
    let sql = resolve_sql(args);

    let data = match &args.data {
        Some(path) => load_data_file(path),
        None => DataBlock::empty(),
    };

    let query_id = format!("submit-{}", now_ms());

    let mut stream = tokio::net::TcpStream::connect(&args.master).await.unwrap_or_else(|e| {
        eprintln!("error: cannot connect to coordinator at {}: {}", args.master, e);
        process::exit(1);
    });

    let msg = KoreMsg::SubmitQuery {
        query_id: query_id.clone(),
        sql: sql.clone(),
        table_name: args.table.clone(),
        data,
        reduce_sql: None,
        local_tables: true,
        auth_token: args.token.clone(),
    };

    if let Err(e) = KoreFrame::write(&mut stream, &msg).await {
        eprintln!("error: failed to send query: {}", e);
        process::exit(1);
    }

    match KoreFrame::read(&mut stream).await {
        Ok(KoreMsg::QueryResult { result, .. }) => {
            print_result(&result, args.format);
        }
        Ok(KoreMsg::QueryError { message, .. }) => {
            eprintln!("QUERY ERROR: {}", message);
            process::exit(1);
        }
        Ok(other) => {
            eprintln!("error: unexpected response from coordinator: {:?}", other);
            process::exit(1);
        }
        Err(e) => {
            eprintln!("error: failed to read response: {}", e);
            process::exit(1);
        }
    }
}

fn resolve_sql(args: &Args) -> String {
    if let Some(ref sql) = args.sql {
        return sql.clone();
    }
    if let Some(ref path) = args.file {
        return std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("error: cannot read SQL file '{}': {}", path, e);
            process::exit(1);
        });
    }
    eprintln!("error: either --sql or --file is required");
    eprintln!("Run with --help for usage.");
    process::exit(1);
}

fn load_data_file(path: &str) -> DataBlock {
    if path.ends_with(".parquet") || path.ends_with(".pq") {
        kore_parquet::ParquetReader::new(path)
            .read()
            .unwrap_or_else(|e| {
                eprintln!("error: failed to read parquet file '{}': {}", path, e);
                process::exit(1);
            })
    } else if path.ends_with(".csv") {
        kore_io::CsvReader::new(path)
            .read()
            .unwrap_or_else(|e| {
                eprintln!("error: failed to read CSV file '{}': {}", path, e);
                process::exit(1);
            })
    } else {
        eprintln!("error: unsupported data file format '{}' (use .parquet or .csv)", path);
        process::exit(1);
    }
}

// ─── Output formatting ───────────────────────────────────────────────────────

fn print_result(block: &DataBlock, format: OutputFormat) {
    if block.num_rows == 0 && block.columns.is_empty() {
        println!("(empty result)");
        return;
    }

    match format {
        OutputFormat::Table => print_table(block),
        OutputFormat::Json  => print_json(block),
        OutputFormat::Csv   => print_csv(block),
    }
}

fn print_table(block: &DataBlock) {
    use kore_core::ColumnData;

    let col_names: Vec<&str> = block.columns.iter().map(|c| c.name.as_str()).collect();
    let n = block.num_rows;

    let mut col_strs: Vec<Vec<String>> = Vec::with_capacity(block.columns.len());
    for col in &block.columns {
        let mut vals = Vec::with_capacity(n);
        for i in 0..n {
            vals.push(format_cell(&col.data, i));
        }
        col_strs.push(vals);
    }

    let mut widths: Vec<usize> = col_names.iter().map(|n| n.len()).collect();
    for (ci, col_vals) in col_strs.iter().enumerate() {
        for v in col_vals {
            widths[ci] = widths[ci].max(v.len());
        }
    }

    // Header
    let header: String = col_names.iter().enumerate()
        .map(|(i, name)| format!("{:width$}", name, width = widths[i]))
        .collect::<Vec<_>>().join(" | ");
    println!("{}", header);
    let sep: String = widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("-+-");
    println!("{}", sep);

    // Rows
    for row in 0..n {
        let line: String = col_strs.iter().enumerate()
            .map(|(ci, vals)| format!("{:width$}", vals[row], width = widths[ci]))
            .collect::<Vec<_>>().join(" | ");
        println!("{}", line);
    }

    println!("\n({} rows)", n);

    fn format_cell(data: &ColumnData, idx: usize) -> String {
        match data {
            ColumnData::Int64(v) => match v.get(idx) {
                Some(Some(x)) => x.to_string(),
                _ => "NULL".into(),
            },
            ColumnData::Float64(v) => match v.get(idx) {
                Some(Some(x)) => format!("{:.4}", x),
                _ => "NULL".into(),
            },
            ColumnData::Str(v) => match v.get(idx) {
                Some(Some(s)) => s.clone(),
                _ => "NULL".into(),
            },
            ColumnData::Bool(v) => match v.get(idx) {
                Some(Some(b)) => b.to_string(),
                _ => "NULL".into(),
            },
            ColumnData::StrDict { codes, dict } => match codes.get(idx) {
                Some(&c) if (c as usize) < dict.len() => dict[c as usize].clone(),
                _ => "NULL".into(),
            },
        }
    }
}

fn print_json(block: &DataBlock) {
    use kore_core::ColumnData;

    let mut rows: Vec<serde_json::Value> = Vec::with_capacity(block.num_rows);
    for i in 0..block.num_rows {
        let mut obj = serde_json::Map::new();
        for col in &block.columns {
            let val = match &col.data {
                ColumnData::Int64(v)   => match v.get(i) { Some(Some(x)) => serde_json::json!(x), _ => serde_json::Value::Null },
                ColumnData::Float64(v) => match v.get(i) { Some(Some(x)) => serde_json::json!(x), _ => serde_json::Value::Null },
                ColumnData::Str(v)     => match v.get(i) { Some(Some(s)) => serde_json::json!(s), _ => serde_json::Value::Null },
                ColumnData::Bool(v)    => match v.get(i) { Some(Some(b)) => serde_json::json!(b), _ => serde_json::Value::Null },
                ColumnData::StrDict { codes, dict } => match codes.get(i) {
                    Some(&c) if (c as usize) < dict.len() => serde_json::json!(dict[c as usize]),
                    _ => serde_json::Value::Null,
                },
            };
            obj.insert(col.name.clone(), val);
        }
        rows.push(serde_json::Value::Object(obj));
    }
    println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into()));
}

fn print_csv(block: &DataBlock) {
    use kore_core::ColumnData;

    let header: String = block.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(",");
    println!("{}", header);

    for i in 0..block.num_rows {
        let row: String = block.columns.iter().map(|col| {
            match &col.data {
                ColumnData::Int64(v)   => match v.get(i) { Some(Some(x)) => x.to_string(), _ => String::new() },
                ColumnData::Float64(v) => match v.get(i) { Some(Some(x)) => x.to_string(), _ => String::new() },
                ColumnData::Str(v)     => match v.get(i) { Some(Some(s)) => csv_escape(s), _ => String::new() },
                ColumnData::Bool(v)    => match v.get(i) { Some(Some(b)) => b.to_string(), _ => String::new() },
                ColumnData::StrDict { codes, dict } => match codes.get(i) {
                    Some(&c) if (c as usize) < dict.len() => csv_escape(&dict[c as usize]),
                    _ => String::new(),
                },
            }
        }).collect::<Vec<_>>().join(",");
        println!("{}", row);
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
