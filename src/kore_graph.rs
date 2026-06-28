//! KoreGraph — Layer 12: Query optimizer, column stats, index, explain plans

use std::collections::HashSet;
use std::fs;
use std::time::Instant;
use crate::kore_v2::{KoreReader, KVal};

pub struct ColStats { pub name: String, pub min_val: String, pub max_val: String, pub null_count: usize, pub distinct_count: usize, pub mean: f64 }
pub struct ProfileResult { pub plan: String, pub rows_scanned: usize, pub rows_out: usize, pub elapsed_ms: u64 }

fn kv_str(v: &KVal) -> String {
    match v {
        KVal::Str(s) => s.clone(), KVal::Int(i) => i.to_string(),
        KVal::Float(f) => format!("{:.6}",f), KVal::Bool(b) => b.to_string(),
        KVal::Null => "NULL".to_string(), _ => String::new(),
    }
}

pub struct KoreGraph;
impl KoreGraph {
    /// Per-column statistics: min, max, null count, distinct count, mean.
    pub fn stats(path: &str) -> Result<Vec<ColStats>, String> {
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let headers: Vec<String> = r.columns.iter().map(|c| c.name.clone()).collect();
        let cols = r.read_all_columns();
        let mut result = vec![];
        for (ci, col) in cols.iter().enumerate() {
            let name = headers[ci].clone();
            let (mut min_v, mut max_v) = (String::new(), String::new());
            let (mut nulls, mut sum, mut ncnt) = (0usize, 0.0f64, 0usize);
            let mut dist: HashSet<String> = HashSet::new();
            for val in col {
                if matches!(val, KVal::Null) { nulls += 1; continue; }
                let s = kv_str(val); dist.insert(s.clone());
                if min_v.is_empty() || s < min_v { min_v = s.clone(); }
                if s > max_v { max_v = s.clone(); }
                if let Ok(f) = s.parse::<f64>() { sum += f; ncnt += 1; }
            }
            let mean = if ncnt > 0 { sum / ncnt as f64 } else { 0.0 };
            result.push(ColStats { name, min_val: min_v, max_val: max_v, null_count: nulls, distinct_count: dist.len(), mean });
        }
        Ok(result)
    }

    /// Human-readable execution plan for a SQL query.
    pub fn explain(path: &str, query: &str) -> Result<String, String> {
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let q = query.to_uppercase();
        let mut plan = vec![format!("SCAN {} [{} rows, {} cols]", path, r.nrows, r.columns.len())];
        if q.contains("WHERE")    { plan.push("  -> FILTER [predicate eligible for pushdown]".into()); }
        if q.contains("JOIN")     { plan.push("  -> HASH JOIN".into()); }
        if q.contains("GROUP BY") { plan.push("  -> HASH AGGREGATE".into()); }
        if q.contains("HAVING")   { plan.push("  -> POST-AGG FILTER".into()); }
        if q.contains("ORDER BY") { plan.push("  -> SORT".into()); }
        if q.contains("LIMIT")    { plan.push("  -> LIMIT".into()); }
        if q.contains("SELECT *") { plan.push("  [WARN] SELECT * - prefer explicit columns".into()); }
        if !q.contains("WHERE") && r.nrows > 100_000 {
            plan.push(format!("  [WARN] Full scan: {} rows - add WHERE clause", r.nrows));
        }
        plan.push(format!("Estimated cost: {:.1} units", r.nrows as f64 * 0.001));
        Ok(plan.join("\n"))
    }

    /// Build a sorted text index for fast lookup. Saves to path.idx/col.idx
    pub fn build_index(path: &str, col: &str) -> Result<usize, String> {
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let ci = r.columns.iter().position(|c| c.name == col)
            .ok_or(format!("Column '{}' not found", col))?;
        let cols = r.read_all_columns();
        let mut entries: Vec<(String, usize)> = cols[ci].iter().enumerate()
            .map(|(i,v)| (kv_str(v), i)).collect();
        entries.sort_by(|a,b| a.0.cmp(&b.0));
        let idx_dir = format!("{}.idx", path);
        fs::create_dir_all(&idx_dir).map_err(|e| e.to_string())?;
        let content: String = entries.iter().map(|(v,i)| format!("{}\t{}", v, i)).collect::<Vec<_>>().join("\n");
        fs::write(format!("{}/{}.kdx", idx_dir, col), content).map_err(|e| e.to_string())?;
        Ok(entries.len())
    }

    /// Binary search in index. Returns matching row indices.
    pub fn index_lookup(path: &str, col: &str, value: &str) -> Result<Vec<usize>, String> {
        let idx_path = format!("{}.idx/{}.kdx", path, col);
        let content = fs::read_to_string(&idx_path)
            .map_err(|_| format!("No index for '{}'. Call build_index first.", col))?;
        let lines: Vec<&str> = content.lines().collect();
        let mut result = vec![];
        let (mut lo, mut hi) = (0usize, lines.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let tab = lines[mid].find('\t').unwrap_or(lines[mid].len());
            match lines[mid][..tab].cmp(value) {
                std::cmp::Ordering::Less    => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal   => {
                    let mut i = mid;
                    while i > 0 { let t = lines[i-1].find('\t').unwrap_or(0); if lines[i-1][..t]==*value { i-=1; } else { break; } }
                    while i < lines.len() {
                        let t = lines[i].find('\t').unwrap_or(lines[i].len());
                        if lines[i][..t] != *value { break; }
                        if let Ok(idx) = lines[i][t+1..].parse::<usize>() { result.push(idx); }
                        i += 1;
                    }
                    break;
                }
            }
        }
        Ok(result)
    }

    /// Profile a query: explain + time it + return rows_out.
    pub fn profile(path: &str, query: &str) -> Result<ProfileResult, String> {
        let plan = Self::explain(path, query)?;
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let rows_scanned = r.nrows;
        let t = Instant::now();
        let rows_out = crate::kore_flow::KoreFlow::sql(query).map(|(_,r)| r.len()).unwrap_or(0);
        let elapsed_ms = t.elapsed().as_millis() as u64;
        Ok(ProfileResult { plan, rows_scanned, rows_out, elapsed_ms })
    }

    /// Rule-based query optimizer hints.
    pub fn optimize(query: &str) -> Result<String, String> {
        let mut hints = vec![];
        let q = query.to_uppercase();
        if !q.contains("LIMIT") { hints.push("[HINT] Add LIMIT to reduce output size"); }
        if q.contains("SELECT *") { hints.push("[HINT] Replace SELECT * with specific columns"); }
        if q.contains("OR ") { hints.push("[HINT] OR predicates prevent index use - consider UNION"); }
        if q.contains("NOT IN") { hints.push("[HINT] NOT IN is slow - prefer NOT EXISTS or LEFT JOIN"); }
        let mut out = query.to_string();
        if !hints.is_empty() { out.push_str(&format!("\n-- Optimizer: {}", hints.join("; "))); }
        Ok(out)
    }
}
