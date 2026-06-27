use std::collections::HashMap;
use crate::kore_v2::{KoreReader, KVal};

// ============================================================================
// Public types
// ============================================================================

/// One chunk in a scan/filter/window operation
#[derive(Debug, Clone)]
pub struct StreamBatch {
    pub batch_num: usize,
    pub headers:   Vec<String>,
    pub rows:      Vec<Vec<String>>,
}

/// Per-column descriptive statistics (Welford online algorithm for variance)
#[derive(Debug, Clone)]
pub struct ColumnStats {
    pub col:        String,
    pub count:      u64,
    pub sum:        f64,
    pub min:        f64,
    pub max:        f64,
    pub mean:       f64,
    pub variance:   f64,
    pub stddev:     f64,
    pub null_count: u64,
}

/// Entry returned by poll_dir
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub path:          String,
    pub modified_secs: u64,
    pub size_bytes:    u64,
}

// ============================================================================
// KoreStream public API
// ============================================================================

/// Layer 7: Zero-dependency pure-Rust streaming engine for .kore files.
///
/// Provides chunked scanning, streaming filters, tumbling-window aggregation,
/// multi-file merging, Welford online statistics, z-score anomaly detection,
/// per-partition top-N, and directory polling — no Spark, no Kafka required.
pub struct KoreStream;

impl KoreStream {
    // ── 1. Chunked scan ──────────────────────────────────────────────────────

    /// Read `path` in batches of `chunk_size` rows.
    /// Returns a Vec of (headers, rows) — one entry per batch.
    pub fn scan(path: &str, chunk_size: usize) -> Result<Vec<(Vec<String>, Vec<Vec<String>>)>, String> {
        let (cols, raw) = kload(path)?;
        let fmt: Vec<Vec<String>> = raw.iter().map(|r| r.iter().map(kfmt).collect()).collect();
        let cs = chunk_size.max(1);
        Ok(fmt.chunks(cs).map(|c| (cols.clone(), c.to_vec())).collect())
    }

    // ── 2. Streaming WHERE filter ────────────────────────────────────────────

    /// Apply a WHERE predicate to every row; return matching rows in chunks.
    /// `predicate` e.g. `"amount > 500 AND category = 'A'"` (WHERE keyword optional).
    pub fn filter(path: &str, predicate: &str, chunk_size: usize) -> Result<Vec<(Vec<String>, Vec<Vec<String>>)>, String> {
        let (cols, raw) = kload(path)?;
        let s = predicate.trim();
        let s = if s.to_uppercase().starts_with("WHERE") { s[5..].trim() } else { s };
        let cond = if s.is_empty() { Pred::True } else { parse_or(s)? };
        let matched: Vec<Vec<String>> = raw.iter()
            .filter(|r| eval(&cond, &cols, r))
            .map(|r| r.iter().map(kfmt).collect())
            .collect();
        let cs = chunk_size.max(1);
        Ok(matched.chunks(cs).map(|c| (cols.clone(), c.to_vec())).collect())
    }

    // ── 3. Tumbling-window aggregation ────────────────────────────────────────

    /// Divide `path` into non-overlapping windows of `window_size` rows.
    /// Within each window compute COUNT(*), SUM, AVG for every `agg_col`,
    /// grouped by `group_col`.
    /// Returns one (headers, rows) entry per window.
    pub fn window_agg(
        path:        &str,
        window_size: usize,
        group_col:   &str,
        agg_cols:    &[&str],
    ) -> Result<Vec<(Vec<String>, Vec<Vec<String>>)>, String> {
        let (cols, raw) = kload(path)?;
        let ws = window_size.max(1);
        let gi = cols.iter().position(|c| c.eq_ignore_ascii_case(group_col));
        let ais: Vec<Option<usize>> = agg_cols.iter()
            .map(|a| cols.iter().position(|c| c.eq_ignore_ascii_case(a)))
            .collect();

        // Headers: group_col, then for each agg_col: COUNT SUM AVG
        let mut hdrs = vec![group_col.to_string()];
        for ac in agg_cols {
            hdrs.push(format!("COUNT(*)"));
            hdrs.push(format!("SUM({})", ac));
            hdrs.push(format!("AVG({})", ac));
        }

        let mut batches = Vec::new();
        for window in raw.chunks(ws) {
            let mut groups: HashMap<String, (KVal, Vec<Vec<f64>>)> = HashMap::new();
            let mut order: Vec<String> = Vec::new();

            for row in window {
                let key_val  = gi.and_then(|i| row.get(i)).cloned().unwrap_or(KVal::Null);
                let key_str  = kfmt(&key_val).to_lowercase();
                let nums: Vec<f64> = ais.iter().map(|ai| {
                    ai.and_then(|i| row.get(i)).map(kf64).unwrap_or(0.0)
                }).collect();
                let e = groups.entry(key_str.clone()).or_insert_with(|| {
                    order.push(key_str.clone());
                    (key_val.clone(), Vec::new())
                });
                e.1.push(nums);
            }

            let result: Vec<Vec<String>> = order.iter().map(|k| {
                let (kv, row_nums) = &groups[k];
                let cnt = row_nums.len() as f64;
                let mut r = vec![kfmt(kv)];
                for (ai_idx, _) in agg_cols.iter().enumerate() {
                    let sum: f64 = row_nums.iter().map(|n| n[ai_idx]).sum();
                    let avg = if row_nums.is_empty() { 0.0 } else { sum / cnt };
                    r.push(fmt_f(cnt));
                    r.push(fmt_f(sum));
                    r.push(fmt_f(avg));
                }
                r
            }).collect();

            batches.push((hdrs.clone(), result));
        }
        Ok(batches)
    }

    // ── 4. Multi-file merge ──────────────────────────────────────────────────

    /// Union multiple .kore files (same schema) into a single (headers, rows) result.
    pub fn merge(paths: &[&str]) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        if paths.is_empty() { return Err("No paths provided".into()); }
        let (first_cols, first_raw) = kload(paths[0])?;
        let mut all: Vec<Vec<String>> = first_raw.iter().map(|r| r.iter().map(kfmt).collect()).collect();
        for p in &paths[1..] {
            let (_, raw) = kload(p)?;
            for row in raw { all.push(row.iter().map(kfmt).collect()); }
        }
        Ok((first_cols, all))
    }

    // ── 5. Welford online statistics ─────────────────────────────────────────

    /// Single-pass descriptive statistics for every column using Welford's
    /// online algorithm (numerically stable mean + variance).
    pub fn running_stats(path: &str) -> Result<Vec<ColumnStats>, String> {
        let (cols, raw) = kload(path)?;
        let n = cols.len();
        let mut count      = vec![0u64; n];
        let mut null_count = vec![0u64; n];
        let mut sum        = vec![0f64; n];
        let mut m2         = vec![0f64; n];
        let mut mean       = vec![0f64; n];
        let mut min        = vec![f64::MAX; n];
        let mut max        = vec![f64::MIN; n];

        for row in &raw {
            for (ci, v) in row.iter().enumerate().take(n) {
                match v {
                    KVal::Null => { null_count[ci] += 1; continue; }
                    KVal::Str(s) if s.is_empty() => { null_count[ci] += 1; continue; }
                    _ => {}
                }
                let f = kf64(v);
                count[ci] += 1;
                sum[ci]   += f;
                let delta   = f - mean[ci];
                mean[ci]   += delta / count[ci] as f64;
                m2[ci]     += delta * (f - mean[ci]);
                if f < min[ci] { min[ci] = f; }
                if f > max[ci] { max[ci] = f; }
            }
        }

        Ok(cols.iter().enumerate().map(|(ci, col)| {
            let variance = if count[ci] > 1 { m2[ci] / (count[ci]-1) as f64 } else { 0.0 };
            ColumnStats {
                col:        col.clone(),
                count:      count[ci],
                sum:        sum[ci],
                min:        if min[ci] < f64::MAX { min[ci] } else { 0.0 },
                max:        if max[ci] > f64::MIN { max[ci] } else { 0.0 },
                mean:       mean[ci],
                variance,
                stddev:     variance.sqrt(),
                null_count: null_count[ci],
            }
        }).collect())
    }

    // ── 6. Z-score anomaly detection ─────────────────────────────────────────

    /// Return rows where any numeric column has |z-score| > `threshold`.
    /// Uses a two-pass approach (stats first, then filter).
    pub fn anomalies(path: &str, threshold: f64) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        let stats = Self::running_stats(path)?;
        let (cols, raw) = kload(path)?;
        let result: Vec<Vec<String>> = raw.iter().filter(|row| {
            row.iter().enumerate().any(|(ci, v)| {
                let s = match stats.get(ci) { Some(s) => s, None => return false };
                if s.stddev < 1e-9 { return false; }
                let z = (kf64(v) - s.mean).abs() / s.stddev;
                z > threshold
            })
        }).map(|r| r.iter().map(kfmt).collect()).collect();
        Ok((cols, result))
    }

    // ── 7. Directory polling ──────────────────────────────────────────────────

    /// Return .kore files in `dir` whose mtime is within the last `since_secs` seconds.
    pub fn poll_dir(dir: &str, since_secs: u64) -> Result<Vec<DirEntry>, String> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now    = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let cutoff = now.saturating_sub(since_secs);
        let mut entries: Vec<DirEntry> = std::fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("kore") { return None; }
                let meta     = e.metadata().ok()?;
                let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                if modified < cutoff { return None; }
                Some(DirEntry { path: p.to_string_lossy().into_owned(), modified_secs: modified, size_bytes: meta.len() })
            })
            .collect();
        entries.sort_by(|a, b| b.modified_secs.cmp(&a.modified_secs));
        Ok(entries)
    }

    // ── 8. Per-partition streaming top-N ─────────────────────────────────────

    /// For each distinct value of `group_col`, return the top-`n` rows
    /// ranked by `value_col` descending.
    pub fn top_n(
        path:      &str,
        group_col: &str,
        value_col: &str,
        n:         usize,
    ) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        let (cols, raw) = kload(path)?;
        let gi = cols.iter().position(|c| c.eq_ignore_ascii_case(group_col))
            .ok_or_else(|| format!("Column '{}' not found", group_col))?;
        let vi = cols.iter().position(|c| c.eq_ignore_ascii_case(value_col))
            .ok_or_else(|| format!("Column '{}' not found", value_col))?;

        let mut groups: HashMap<String, Vec<Vec<KVal>>> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for row in raw {
            let k = kfmt(row.get(gi).unwrap_or(&KVal::Null)).to_lowercase();
            let e = groups.entry(k.clone()).or_insert_with(|| { order.push(k.clone()); Vec::new() });
            e.push(row);
        }

        let mut result: Vec<Vec<String>> = Vec::new();
        for k in &order {
            let mut gr = groups.remove(k).unwrap();
            gr.sort_by(|a, b| {
                let af = kf64(a.get(vi).unwrap_or(&KVal::Null));
                let bf = kf64(b.get(vi).unwrap_or(&KVal::Null));
                bf.partial_cmp(&af).unwrap_or(std::cmp::Ordering::Equal)
            });
            for row in gr.into_iter().take(n) { result.push(row.iter().map(kfmt).collect()); }
        }
        Ok((cols, result))
    }

    // ── 9. Render helper ─────────────────────────────────────────────────────

    /// Pretty-print a (headers, rows) tuple into an ASCII table string.
    pub fn table_str(headers: &[String], rows: &[Vec<String>]) -> String {
        render(headers, rows)
    }
}

// ============================================================================
// Internal: load + format helpers
// ============================================================================

fn kload(path: &str) -> Result<(Vec<String>, Vec<Vec<KVal>>), String> {
    let r    = KoreReader::open(path).map_err(|e| e.to_string())?;
    let cols: Vec<String> = r.columns.iter().map(|c| c.name.clone()).collect();
    let raw  = r.read_all_columns();
    let nrows = r.nrows;
    let ncols = raw.len();
    let mut rows: Vec<Vec<KVal>> = (0..nrows).map(|_| vec![KVal::Null; ncols]).collect();
    for (ci, col_data) in raw.iter().enumerate() {
        for (ri, val) in col_data.iter().enumerate() {
            if ri < nrows { rows[ri][ci] = val.clone(); }
        }
    }
    Ok((cols, rows))
}

fn kfmt(v: &KVal) -> String {
    match v {
        KVal::Int(x)   => x.to_string(),
        KVal::Float(x) => { let s = format!("{:.4}", x); s.trim_end_matches('0').trim_end_matches('.').to_string() }
        KVal::Str(s)   => s.clone(),
        KVal::Bool(b)  => b.to_string(),
        KVal::Null     => "NULL".into(),
        _              => format!("{:?}", v),
    }
}

fn fmt_f(f: f64) -> String {
    if f.fract() == 0.0 { format!("{:.0}", f) }
    else { format!("{:.4}", f).trim_end_matches('0').trim_end_matches('.').to_string() }
}

fn kf64(v: &KVal) -> f64 {
    match v {
        KVal::Int(x)   => *x as f64,
        KVal::Float(x) => *x,
        KVal::Str(s)   => s.parse().unwrap_or(0.0),
        _              => 0.0,
    }
}

fn render(hdrs: &[String], rows: &[Vec<String>]) -> String {
    if rows.is_empty() { return "  (no rows)\n  0 rows".into(); }
    let mut w: Vec<usize> = hdrs.iter().map(|h| h.len()).collect();
    for row in rows { for (i, c) in row.iter().enumerate() { if i < w.len() { w[i] = w[i].max(c.len()); } } }
    let sep: String = w.iter().map(|&ww| format!("+{}", "-".repeat(ww+2))).collect::<String>() + "+";
    let mut out = format!("{}\n", sep);
    out += &format!("| {} |\n", hdrs.iter().zip(&w).map(|(h,&ww)| format!("{:<ww$}", h, ww=ww)).collect::<Vec<_>>().join(" | "));
    out += &format!("{}\n", sep);
    for row in rows {
        out += &format!("| {} |\n", (0..hdrs.len()).map(|i| {
            let c = row.get(i).map(|s| s.as_str()).unwrap_or("");
            format!("{:<ww$}", c, ww=w[i])
        }).collect::<Vec<_>>().join(" | "));
    }
    out += &format!("{}\n  {} rows", sep, rows.len());
    out
}

// ============================================================================
// Simple predicate parser for filter()
// Grammar: expr := or_expr
//          or_expr  := and_expr ('OR' and_expr)*
//          and_expr := atom ('AND' atom)*
//          atom     := '(' or_expr ')' | 'NOT' atom | col IS [NOT] NULL | col op val
// ============================================================================

#[derive(Debug, Clone)]
enum Pred {
    Cmp(String, POp, PVal),
    IsNull(String, bool),
    And(Box<Pred>, Box<Pred>),
    Or(Box<Pred>, Box<Pred>),
    Not(Box<Pred>),
    True,
}

#[derive(Debug, Clone, PartialEq)]
enum POp { Eq, Neq, Lt, Lte, Gt, Gte }

#[derive(Debug, Clone)]
enum PVal { Str(String), Num(f64), Null }

fn parse_or(s: &str) -> Result<Pred, String> {
    let parts = split_kw(s, " OR ");
    if parts.len() > 1 {
        let mut it = parts.into_iter();
        let mut acc = parse_and(it.next().unwrap())?;
        for p in it { acc = Pred::Or(Box::new(acc), Box::new(parse_and(p)?)); }
        return Ok(acc);
    }
    parse_and(s)
}

fn parse_and(s: &str) -> Result<Pred, String> {
    let parts = split_kw(s, " AND ");
    if parts.len() > 1 {
        let mut it = parts.into_iter();
        let mut acc = parse_atom(it.next().unwrap())?;
        for p in it { acc = Pred::And(Box::new(acc), Box::new(parse_atom(p)?)); }
        return Ok(acc);
    }
    parse_atom(s)
}

fn parse_atom(s: &str) -> Result<Pred, String> {
    let s = s.trim();
    if s.starts_with('(') && s.ends_with(')') { return parse_or(&s[1..s.len()-1]); }
    let up = s.to_uppercase();
    if up.starts_with("NOT ") { return Ok(Pred::Not(Box::new(parse_atom(&s[4..])?))); }
    // IS NOT NULL / IS NULL
    if let Some(i) = up.find(" IS NOT NULL") {
        return Ok(Pred::IsNull(s[..i].trim().to_string(), false));
    }
    if let Some(i) = up.find(" IS NULL") {
        return Ok(Pred::IsNull(s[..i].trim().to_string(), true));
    }
    // Comparison operators (longest first to avoid ">=" matching ">")
    for (op_str, op) in &[("<=", POp::Lte), (">=", POp::Gte), ("<>", POp::Neq), ("!=", POp::Neq),
                           ("<", POp::Lt), (">", POp::Gt), ("=", POp::Eq)] {
        if let Some(i) = s.find(op_str) {
            let col = s[..i].trim().to_string();
            if col.is_empty() { continue; }
            let val_s = s[i + op_str.len()..].trim();
            let val = if val_s.to_uppercase() == "NULL" {
                PVal::Null
            } else if (val_s.starts_with('\'') && val_s.ends_with('\'')) ||
                      (val_s.starts_with('"')  && val_s.ends_with('"')) {
                PVal::Str(val_s[1..val_s.len()-1].to_string())
            } else if let Ok(f) = val_s.parse::<f64>() {
                PVal::Num(f)
            } else {
                PVal::Str(val_s.to_string())
            };
            return Ok(Pred::Cmp(col, op.clone(), val));
        }
    }
    Err(format!("Cannot parse predicate atom: '{}'", s))
}

/// Split `s` on case-insensitive `sep` but not inside parentheses.
fn split_kw<'a>(s: &'a str, sep: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut last  = 0usize;
    let sep_up    = sep.to_uppercase();
    let s_up      = s.to_uppercase();
    let bytes     = s.as_bytes();
    let mut i     = 0usize;
    while i < bytes.len() {
        match bytes[i] { b'(' => depth += 1, b')' => { depth = depth.saturating_sub(1); } _ => {} }
        if depth == 0 && s_up[i..].starts_with(&sep_up) {
            parts.push(&s[last..i]);
            last = i + sep.len();
            i    = last;
        } else {
            i += 1;
        }
    }
    parts.push(&s[last..]);
    parts
}

fn eval(pred: &Pred, cols: &[String], row: &[KVal]) -> bool {
    match pred {
        Pred::True      => true,
        Pred::And(l, r) => eval(l, cols, row) && eval(r, cols, row),
        Pred::Or(l, r)  => eval(l, cols, row) || eval(r, cols, row),
        Pred::Not(p)    => !eval(p, cols, row),
        Pred::IsNull(col, want_null) => {
            let is_null = col_idx(cols, col).map_or(true, |i|
                matches!(row.get(i), None | Some(KVal::Null))
            );
            is_null == *want_null
        }
        Pred::Cmp(col, op, val) => {
            let ci = match col_idx(cols, col) { Some(i) => i, None => return false };
            let v  = row.get(ci).unwrap_or(&KVal::Null);
            match val {
                PVal::Null  => matches!(op, POp::Eq) == matches!(v, KVal::Null),
                PVal::Num(n) => {
                    let f = kf64(v);
                    match op { POp::Eq=>f==*n, POp::Neq=>f!=*n, POp::Lt=>f<*n, POp::Lte=>f<=*n, POp::Gt=>f>*n, POp::Gte=>f>=*n }
                }
                PVal::Str(s) => {
                    let vs = match v { KVal::Str(x) => x.as_str(), _ => return false };
                    match op { POp::Eq=>vs==s, POp::Neq=>vs!=s, POp::Lt=>vs<s.as_str(), POp::Lte=>vs<=s.as_str(), POp::Gt=>vs>s.as_str(), POp::Gte=>vs>=s.as_str() }
                }
            }
        }
    }
}

fn col_idx(cols: &[String], name: &str) -> Option<usize> {
    let sn = name.rfind('.').map(|i| &name[i+1..]).unwrap_or(name);
    cols.iter().position(|c| c.eq_ignore_ascii_case(name) || c.eq_ignore_ascii_case(sn))
}
