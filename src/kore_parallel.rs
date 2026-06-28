//! KoreParallel — Layer 11: std::thread parallel query execution

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use crate::kore_v2::{KoreReader, KVal};

pub struct BenchResult { pub single_ms: u64, pub parallel_ms: u64, pub speedup: f64 }

fn kv_str(v: &KVal) -> String {
    match v {
        KVal::Str(s) => s.clone(), KVal::Int(i) => i.to_string(),
        KVal::Float(f) => format!("{:.6}", f), KVal::Bool(b) => b.to_string(),
        KVal::Null => String::new(), _ => String::new(),
    }
}

fn load_str_table(path: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let r = KoreReader::open(path).map_err(|e| e.to_string())?;
    let headers: Vec<String> = r.columns.iter().map(|c| c.name.clone()).collect();
    let cols = r.read_all_columns();
    let nrows = cols.first().map(|c| c.len()).unwrap_or(0);
    let ncols = headers.len();
    let mut rows = vec![vec![String::new(); ncols]; nrows];
    for (ci, col) in cols.iter().enumerate() {
        for (ri, val) in col.iter().enumerate() { rows[ri][ci] = kv_str(val); }
    }
    Ok((headers, rows))
}

fn pred_match(row: &[String], hdrs: &[String], pred: &str) -> bool {
    if pred.trim().is_empty() { return true; }
    for op in &[">=", "<=", "!=", ">", "<", "="] {
        if let Some(pos) = pred.find(op) {
            let col = pred[..pos].trim();
            let val = pred[pos+op.len()..].trim().trim_matches('"').trim_matches('\'');
            if let Some(ci) = hdrs.iter().position(|h| h == col) {
                let cell = &row[ci];
                let cn = cell.parse::<f64>().ok();
                let vn = val.parse::<f64>().ok();
                return match *op {
                    "=" => cell.as_str() == val || (cn.is_some() && cn == vn),
                    "!=" => cell.as_str() != val,
                    ">" => matches!((cn,vn),(Some(a),Some(b)) if a>b),
                    ">=" => matches!((cn,vn),(Some(a),Some(b)) if a>=b),
                    "<" => matches!((cn,vn),(Some(a),Some(b)) if a<b),
                    "<=" => matches!((cn,vn),(Some(a),Some(b)) if a<=b),
                    _ => false,
                };
            }
        }
    }
    false
}

pub struct KoreParallel;
impl KoreParallel {
    /// Parallel filtered rows. Returns (headers, rows_as_strings).
    pub fn pfilter(path: &str, predicate: &str, threads: usize)
        -> Result<(Vec<String>, Vec<Vec<String>>), String>
    {
        let (hdrs, rows) = load_str_table(path)?;
        let hdrs = Arc::new(hdrs); let rows = Arc::new(rows);
        let pred = Arc::new(predicate.to_string());
        let n = rows.len(); let t = threads.max(1); let chunk = (n + t - 1) / t;
        let mut handles = vec![];
        for ti in 0..t {
            let (h, r, p) = (Arc::clone(&hdrs), Arc::clone(&rows), Arc::clone(&pred));
            let (s, e) = (ti*chunk, ((ti+1)*chunk).min(n));
            handles.push(thread::spawn(move || {
                (s..e).filter(|&i| pred_match(&r[i], &h, &p)).map(|i| r[i].clone()).collect::<Vec<_>>()
            }));
        }
        let result: Vec<Vec<String>> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        Ok(((*hdrs).clone(), result))
    }

    /// Parallel row count matching predicate.
    pub fn pcount(path: &str, predicate: &str, threads: usize) -> Result<usize, String> {
        Ok(Self::pfilter(path, predicate, threads)?.1.len())
    }

    /// Parallel sum of numeric column matching predicate.
    pub fn psum(path: &str, col: &str, predicate: &str, threads: usize) -> Result<f64, String> {
        let (hdrs, rows) = load_str_table(path)?;
        let ci = hdrs.iter().position(|h| h==col).ok_or(format!("Column '{}' not found",col))?;
        let hdrs = Arc::new(hdrs); let rows = Arc::new(rows);
        let pred = Arc::new(predicate.to_string());
        let n = rows.len(); let t = threads.max(1); let chunk = (n+t-1)/t;
        let mut handles = vec![];
        for ti in 0..t {
            let (h, r, p) = (Arc::clone(&hdrs), Arc::clone(&rows), Arc::clone(&pred));
            let (s, e) = (ti*chunk, ((ti+1)*chunk).min(n));
            handles.push(thread::spawn(move || {
                (s..e).filter(|&i| pred_match(&r[i],&h,&p)).map(|i| r[i][ci].parse::<f64>().unwrap_or(0.0)).sum::<f64>()
            }));
        }
        Ok(handles.into_iter().map(|h| h.join().unwrap()).sum())
    }

    /// Parallel average of numeric column.
    pub fn pavg(path: &str, col: &str, threads: usize) -> Result<f64, String> {
        let (hdrs, rows) = load_str_table(path)?;
        let ci = hdrs.iter().position(|h| h==col).ok_or(format!("Column '{}' not found",col))?;
        let rows = Arc::new(rows);
        let n = rows.len(); let t = threads.max(1); let chunk = (n+t-1)/t;
        let mut handles = vec![];
        for ti in 0..t {
            let r = Arc::clone(&rows);
            let (s, e) = (ti*chunk, ((ti+1)*chunk).min(n));
            handles.push(thread::spawn(move || {
                let (mut sm, mut cnt) = (0.0f64, 0usize);
                for i in s..e { if let Ok(v) = r[i][ci].parse::<f64>() { sm+=v; cnt+=1; } }
                (sm, cnt)
            }));
        }
        let (total_s, total_c) = handles.into_iter().map(|h| h.join().unwrap())
            .fold((0.0,0usize),|(s,c),(s2,c2)|(s+s2,c+c2));
        Ok(if total_c>0 { total_s/total_c as f64 } else { 0.0 })
    }

    /// Parallel group-by aggregation. agg_fn: "sum"|"avg"|"count"|"max"|"min"
    pub fn pgroup(path: &str, group_col: &str, agg_col: &str, agg_fn: &str, threads: usize)
        -> Result<Vec<(String, f64)>, String>
    {
        let (hdrs, rows) = load_str_table(path)?;
        let gi = hdrs.iter().position(|h| h==group_col).ok_or(format!("Column '{}' not found",group_col))?;
        let ai = hdrs.iter().position(|h| h==agg_col).ok_or(format!("Column '{}' not found",agg_col))?;
        let rows = Arc::new(rows);
        let n = rows.len(); let t = threads.max(1); let chunk = (n+t-1)/t;
        let mut handles = vec![];
        for ti in 0..t {
            let r = Arc::clone(&rows); let f = agg_fn.to_string();
            let (s, e) = (ti*chunk, ((ti+1)*chunk).min(n));
            handles.push(thread::spawn(move || {
                let mut m: HashMap<String,(f64,usize,bool)> = HashMap::new();
                for i in s..e {
                    let g = r[i][gi].clone();
                    let v = r[i][ai].parse::<f64>().unwrap_or(0.0);
                    let e = m.entry(g).or_insert((0.0,0,false));
                    match f.as_str() {
                        "sum"|"avg" => { e.0+=v; e.1+=1; }
                        "count" => { e.1+=1; }
                        "max" => { if !e.2||v>e.0 { e.0=v; e.2=true; } e.1+=1; }
                        "min" => { if !e.2||v<e.0 { e.0=v; e.2=true; } e.1+=1; }
                        _ => { e.0+=v; e.1+=1; }
                    }
                }
                m
            }));
        }
        let mut merged: HashMap<String,(f64,usize,bool)> = HashMap::new();
        for h in handles {
            for (k,(s,c,init)) in h.join().unwrap() {
                let e = merged.entry(k).or_insert((0.0,0,false));
                match agg_fn {
                    "max" => { if init&&(!e.2||s>e.0){ e.0=s; e.2=true; } e.1+=c; }
                    "min" => { if init&&(!e.2||s<e.0){ e.0=s; e.2=true; } e.1+=c; }
                    _ => { e.0+=s; e.1+=c; }
                }
            }
        }
        let mut res: Vec<(String,f64)> = merged.into_iter().map(|(k,(s,c,_))| {
            let v = match agg_fn { "avg"=>if c>0{s/c as f64}else{0.0}, "count"=>c as f64, _=>s };
            (k,v)
        }).collect();
        res.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(res)
    }

    /// Benchmark single vs parallel scan. Returns (single_ms, parallel_ms, speedup).
    pub fn benchmark(path: &str, threads: usize) -> Result<BenchResult, String> {
        let t0 = Instant::now();
        let _ = Self::pcount(path, "", 1)?;
        let single_ms = t0.elapsed().as_millis() as u64;
        let t1 = Instant::now();
        let _ = Self::pcount(path, "", threads)?;
        let parallel_ms = t1.elapsed().as_millis() as u64;
        let speedup = if parallel_ms>0 { single_ms as f64/parallel_ms as f64 } else { 1.0 };
        Ok(BenchResult { single_ms, parallel_ms, speedup })
    }
}
