//! KoreServe — Layer 13: Embedded HTTP/JSON REST API server (pure Rust)

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

static RUNNING: AtomicBool = AtomicBool::new(false);

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '"'  => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

pub struct KoreServe;
impl KoreServe {
    /// Start HTTP server in background thread. Non-blocking. port e.g. 7070.
    pub fn start(path: &str, port: u16) -> Result<(), String> {
        if RUNNING.swap(true, Ordering::SeqCst) { return Err("Server already running. Call stop() first.".into()); }
        let p = path.to_string();
        thread::spawn(move || {
            let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)) {
                Ok(l) => l, Err(_) => { RUNNING.store(false, Ordering::SeqCst); return; }
            };
            listener.set_nonblocking(true).ok();
            while RUNNING.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => { let pp = p.clone(); thread::spawn(move || { let _ = Self::handle(stream, &pp); }); }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { thread::sleep(Duration::from_millis(10)); }
                    Err(_) => break,
                }
            }
            RUNNING.store(false, Ordering::SeqCst);
        });
        Ok(())
    }

    /// Stop the background server.
    pub fn stop() -> Result<(), String> { RUNNING.store(false, Ordering::SeqCst); Ok(()) }

    /// Is the server running?
    pub fn is_running() -> bool { RUNNING.load(Ordering::SeqCst) }

    /// Process one request in-process (no socket). Useful for testing.
    /// method: "GET"|"POST", endpoint: "/sql"|"/schema"|"/health"|"/snapshots"|"/stats"
    pub fn handle_request(path: &str, method: &str, endpoint: &str, body: &str) -> Result<String, String> {
        Self::dispatch(path, method, endpoint, body)
    }

    fn handle(mut stream: std::net::TcpStream, path: &str) -> Result<(), String> {
        let mut buf = vec![0u8; 16384];
        let n = stream.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let mut lines = req.lines();
        let first = lines.next().unwrap_or("");
        let parts: Vec<&str> = first.split_whitespace().collect();
        if parts.len() < 2 { return Ok(()); }
        let (method, endpoint) = (parts[0], parts[1]);
        let body = req.find("\r\n\r\n").map(|p| req[p+4..].trim_end_matches('\0')).unwrap_or("").to_string();
        let (status, json) = match Self::dispatch(path, method, endpoint, &body) {
            Ok(r) => ("200 OK", r),
            Err(e) => ("500 Internal Server Error", format!("{{\"error\":\"{}\"}}", e.replace('"', "'"))),
        };
        let resp = format!("HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}", status, json.len(), json);
        stream.write_all(resp.as_bytes()).ok();
        Ok(())
    }

    fn dispatch(path: &str, method: &str, endpoint: &str, body: &str) -> Result<String, String> {
        let ep = endpoint.split('?').next().unwrap_or(endpoint);
        match (method, ep) {
            ("GET", "/health") => {
                let r = crate::kore_v2::KoreReader::open(path).map_err(|e| e.to_string())?;
                Ok(format!("{{\"status\":\"ok\",\"path\":\"{}\",\"rows\":{},\"cols\":{}}}", path, r.nrows, r.columns.len()))
            }
            ("GET", "/schema") => {
                let r = crate::kore_v2::KoreReader::open(path).map_err(|e| e.to_string())?;
                let cols: Vec<String> = r.columns.iter()
                    .map(|c| format!("{{\"name\":\"{}\",\"type\":\"{:?}\"}}", esc(&c.name), c.ktype)).collect();
                Ok(format!("[{}]", cols.join(",")))
            }
            ("POST", "/sql") => {
                let q = Self::jstr(body, "query").ok_or("Missing 'query' field")?;
                let (hdrs, rows) = crate::kore_flow::KoreFlow::sql(&q)?;
                let hj: Vec<String> = hdrs.iter().map(|h| format!("\"{}\"", esc(h))).collect();
                let rj: Vec<String> = rows.iter().map(|row| {
                    let cells: Vec<String> = hdrs.iter().zip(row.iter())
                        .map(|(h,v)| format!("\"{}\":\"{}\"", esc(h), esc(v))).collect();
                    format!("{{{}}}", cells.join(","))
                }).collect();
                Ok(format!("{{\"columns\":[{}],\"rows\":[{}],\"count\":{}}}", hj.join(","), rj.join(","), rows.len()))
            }
            ("GET", "/snapshots") => {
                let snaps = crate::kore_vault::KoreVault::list_snapshots(path)?;
                let items: Vec<String> = snaps.iter().map(|s|
                    format!("{{\"id\":\"{}\",\"tag\":\"{}\",\"ts\":{},\"size\":{}}}", esc(&s.id), esc(&s.tag), s.timestamp, s.size)
                ).collect();
                Ok(format!("[{}]", items.join(",")))
            }
            ("GET", "/stats") => {
                let stats = crate::kore_graph::KoreGraph::stats(path)?;
                let items: Vec<String> = stats.iter().map(|s|
                    format!("{{\"col\":\"{}\",\"min\":\"{}\",\"max\":\"{}\",\"nulls\":{},\"distinct\":{},\"mean\":{:.4}}}",
                        esc(&s.name), esc(&s.min_val), esc(&s.max_val), s.null_count, s.distinct_count, s.mean)
                ).collect();
                Ok(format!("[{}]", items.join(",")))
            }
            ("GET", "/explain") => {
                let q = Self::jstr(body, "query").unwrap_or_else(|| "SELECT * FROM t".to_string());
                crate::kore_graph::KoreGraph::explain(path, &q)
                    .map(|p| format!("{{\"plan\":\"{}\"}}", esc(&p).replace('\n', "\\n")))
            }
            _ => Err(format!("Unknown: {} {}", method, ep)),
        }
    }

    fn jstr(json: &str, key: &str) -> Option<String> {
        let pat = format!("\"{}\"", key);
        let pos = json.find(&pat)?;
        let after = json[pos+pat.len()..].trim_start();
        let after = after.strip_prefix(':')?.trim_start();
        if after.starts_with('"') {
            let inner = &after[1..];
            Some(inner[..inner.find('"')?].to_string())
        } else { None }
    }
}
