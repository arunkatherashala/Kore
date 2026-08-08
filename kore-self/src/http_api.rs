//! HTTP REST API — localhost by default, optional token on mutating routes.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use kore_sql::executor::KqlContext;

use crate::federation_net;
use crate::http_config;
use crate::kore_query;
use crate::mesh;
use crate::survival;
use crate::KoreSelf;

pub async fn run_http_api(owner: String, port: u16) {
    let me = KoreSelf::load_or_new(&owner);
    let bind = http_config::api_bind_host();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  KORE HTTP REST API — The World's Fastest SQL Engine");
    println!("  Owner: {}  |  Memories: {}  |  Bind: {}:{}",
        me.owner, me.memories.len(), bind, port);
    if http_config::api_token().is_some() {
        println!("  Auth: KORE_API_TOKEN required for POST /sql and POST /load");
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  http://{bind}:{port}/          → Web UI");
    println!("  POST  http://{bind}:{port}/sql  → Run SQL");
    println!("  GET   http://{bind}:{port}/tables → List tables");
    println!("  GET   http://{bind}:{port}/status → Engine status");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut base_ctx = KqlContext::new();
    base_ctx.register("memories", kore_query::memories_to_block(&me.memories));

    let shared_ctx = Arc::new(Mutex::new(base_ctx));
    let shared_me = Arc::new(Mutex::new(me));

    {
        let shutdown_save = Arc::clone(&shared_me);
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            if let Ok(me) = shutdown_save.lock() {
                me.save();
                eprintln!("[kore-api] Ctrl+C: saved {} memories.", me.memories.len());
            }
            std::process::exit(0);
        });
    }

    {
        let hb = Arc::clone(&shared_me);
        let interval_secs = shared_me.lock().map(|k| k.heartbeat_interval_secs).unwrap_or(30);
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                if let Ok(mut k) = hb.lock() {
                    k.heartbeat_tick();
                }
            }
        });
    }

    {
        let fed = Arc::clone(&shared_me);
        tokio::spawn(async move { federation_net::federation_server(fed).await });
        let fed_out = Arc::clone(&shared_me);
        tokio::spawn(async move { federation_net::federation_outbound(fed_out).await });
        let mesh_arc = Arc::clone(&shared_me);
        tokio::spawn(async move {
            if let Err(e) = mesh::start_mesh(mesh_arc).await {
                eprintln!("[kore-mesh] failed to start: {e}");
            }
        });
        let surv = Arc::clone(&shared_me);
        tokio::spawn(async move { survival::survival_monitor(surv).await });
    }

    let ctx = Arc::clone(&shared_ctx);
    let me_arc = Arc::clone(&shared_me);
    let bind_host = bind.clone();
    tokio::task::spawn_blocking(move || {
        let addr = format!("{bind_host}:{port}");
        let listener = TcpListener::bind(&addr).expect("cannot bind HTTP API");
        println!("[kore-api] Listening on http://{addr}");
        for stream in listener.incoming() {
            if let Ok(s) = stream {
                let ctx_c = Arc::clone(&ctx);
                let me_c = Arc::clone(&me_arc);
                std::thread::spawn(move || http_handle(s, ctx_c, me_c));
            }
        }
    })
    .await
    .unwrap();
}

pub fn http_handle(
    mut stream: TcpStream,
    ctx: Arc<Mutex<KqlContext>>,
    me: Arc<Mutex<KoreSelf>>,
) {
    let mut buf = vec![0u8; 16384];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let (method, path) = (parts[0], parts[1]);

    if let Err(msg) = http_config::authorize_request(&req, method, path) {
        let body = format!(r#"{{"error":"{}"}}"#, msg.replace('"', "'"));
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .as_bytes(),
        );
        return;
    }

    let body = if let Some(pos) = req.find("\r\n\r\n") {
        req[pos + 4..].trim().to_string()
    } else {
        String::new()
    };

    let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization, X-KORE-Token\r\n";

    if method == "OPTIONS" {
        let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\n{cors}\r\n").as_bytes());
        return;
    }

    let (status, content_type, response_body) = match (method, path) {
        ("GET", "/") | ("GET", "/ui") => {
            ("200 OK", "text/html; charset=utf-8", crate::WEB_UI.to_string())
        }
        ("GET", "/status") => {
            let info = if let Ok(k) = me.lock() {
                serde_json::json!({
                    "name": "KORE",
                    "version": "2026.07",
                    "owner": k.owner,
                    "memories": k.memories.len(),
                    "lifecycle": k.becoming.lifecycle_stage.name(),
                    "lifecycle_desc": k.becoming.lifecycle_stage.description(),
                    "evolutions": k.becoming.evolution_count,
                    "needs": { "learn": k.needs.learn, "create": k.needs.create, "evolve": k.needs.evolve },
                    "lightweight": k.lightweight_mode,
                    "principle": "Software executes. AI reasons. Agents act. KORE continues.",
                })
            } else {
                serde_json::json!({"error":"locked"})
            };
            ("200 OK", "application/json", info.to_string())
        }
        ("GET", "/tables") => {
            let tables = if let Ok(c) = ctx.lock() {
                c.table_names()
                    .iter()
                    .map(|n| {
                        let rows = c.get(n).map(|b| b.num_rows).unwrap_or(0);
                        let cols = c.get(n).map(|b| b.columns.len()).unwrap_or(0);
                        serde_json::json!({"name": n, "rows": rows, "columns": cols})
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![]
            };
            ("200 OK", "application/json", serde_json::json!(tables).to_string())
        }
        ("POST", "/sql") => {
            let sql = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v["sql"].as_str().map(|s| s.to_string()))
                .unwrap_or(body.clone());
            let t0 = std::time::Instant::now();
            let result = if let Ok(mut c) = ctx.lock() {
                let upper = sql.trim().to_ascii_uppercase();
                if upper.starts_with("COPY ")
                    || upper.starts_with("INSERT ")
                    || upper.starts_with("UPDATE ")
                    || upper.starts_with("DELETE ")
                    || upper.starts_with("CREATE TABLE")
                    || upper.starts_with("LOAD TABLE")
                    || upper.starts_with("MERGE ")
                {
                    match c.execute_dml(&sql) {
                        Ok((op, rows)) => serde_json::json!({
                            "operation": op, "rows_affected": rows,
                            "time_ms": t0.elapsed().as_secs_f64() * 1000.0
                        }),
                        Err(e) => serde_json::json!({"error": e.to_string()}),
                    }
                } else {
                    match c.query(&sql) {
                        Ok(block) => {
                            let ms = t0.elapsed().as_secs_f64() * 1000.0;
                            let columns: Vec<String> =
                                block.columns.iter().map(|c| c.name.clone()).collect();
                            let data: Vec<Vec<serde_json::Value>> = (0..block.num_rows)
                                .map(|row| {
                                    block
                                        .columns
                                        .iter()
                                        .map(|col| match &col.data {
                                            kore_core::ColumnData::Int64(v) => v
                                                .get(row)
                                                .and_then(|x| *x)
                                                .map(|i| serde_json::json!(i))
                                                .unwrap_or(serde_json::Value::Null),
                                            kore_core::ColumnData::Float64(v) => v
                                                .get(row)
                                                .and_then(|x| *x)
                                                .map(|f| serde_json::json!(f))
                                                .unwrap_or(serde_json::Value::Null),
                                            kore_core::ColumnData::Str(v) => v
                                                .get(row)
                                                .and_then(|x| x.as_deref())
                                                .map(|s| serde_json::json!(s))
                                                .unwrap_or(serde_json::Value::Null),
                                            kore_core::ColumnData::Bool(v) => v
                                                .get(row)
                                                .and_then(|x| *x)
                                                .map(|b| serde_json::json!(b))
                                                .unwrap_or(serde_json::Value::Null),
                                            kore_core::ColumnData::StrDict { codes, dict } => codes
                                                .get(row)
                                                .copied()
                                                .and_then(|c| dict.get(c as usize))
                                                .map(|s| serde_json::json!(s))
                                                .unwrap_or(serde_json::Value::Null),
                                        })
                                        .collect()
                                })
                                .collect();
                            serde_json::json!({
                                "rows": block.num_rows,
                                "columns": columns,
                                "data": data,
                                "time_ms": ms
                            })
                        }
                        Err(e) => serde_json::json!({"error": e.to_string()}),
                    }
                }
            } else {
                serde_json::json!({"error":"context locked"})
            };
            ("200 OK", "application/json", result.to_string())
        }
        ("POST", "/load") => {
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let path = v["path"]
                .as_str()
                .unwrap_or("")
                .trim_matches('\'')
                .trim_matches('"');
            let table = v["table"].as_str().unwrap_or_else(|| {
                path.rsplit('/')
                    .next()
                    .unwrap_or("t")
                    .split('.')
                    .next()
                    .unwrap_or("t")
            });
            let t0 = std::time::Instant::now();
            let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
            let load_result = match ext.as_str() {
                "parquet" => kore_parquet::ParquetReader::new(path)
                    .read()
                    .map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string())),
                "kore" => kore_store::KoreReader::read_file(std::path::Path::new(path))
                    .map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string())),
                _ => kore_io::CsvReader::new(path)
                    .read()
                    .map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string())),
            };
            let resp = match load_result {
                Ok(block) => {
                    let rows = block.num_rows;
                    let cols = block.columns.len();
                    if let Ok(mut c) = ctx.lock() {
                        c.register(table, block);
                    }
                    serde_json::json!({
                        "status": "loaded",
                        "table": table,
                        "rows": rows,
                        "columns": cols,
                        "time_ms": t0.elapsed().as_secs_f64() * 1000.0
                    })
                }
                Err(e) => serde_json::json!({"error": e.to_string()}),
            };
            ("200 OK", "application/json", resp.to_string())
        }
        _ => (
            "404 Not Found",
            "application/json",
            r#"{"error":"not found. Try POST /sql, GET /tables, GET /status, GET /"}"#.into(),
        ),
    };

    let body_bytes = response_body.as_bytes();
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body_bytes);
}
