//! KoreConnect — Layer 14: JSON/CSV connectors, merge, split, sample

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use crate::kore_v2::{KoreReader, KoreWriter, KColumn, KType, KVal};

fn kv_str(v: &KVal) -> String {
    match v {
        KVal::Str(s) => s.clone(), KVal::Int(i) => i.to_string(),
        KVal::Float(f) => format!("{:.6}",f), KVal::Bool(b) => b.to_string(),
        KVal::Null => String::new(), _ => String::new(),
    }
}

// cols[col_idx][row_idx]  →  rows[row_idx][col_idx]
fn cols_to_rows(cols: &[Vec<KVal>], nrows: usize) -> Vec<Vec<KVal>> {
    (0..nrows).map(|ri| cols.iter().map(|c| c[ri].clone()).collect()).collect()
}

pub struct KoreConnect;
impl KoreConnect {
    /// NDJSON (newline-delimited JSON) → .kore. Returns row count.
    pub fn from_json(src: &str, dst: &str) -> Result<usize, String> {
        let txt = fs::read_to_string(src).map_err(|e| e.to_string())?;
        let mut rows_data: Vec<HashMap<String,String>> = vec![];
        let mut ordered_keys: Vec<String> = vec![];
        for line in txt.lines() {
            let line = line.trim();
            if line.is_empty() || !line.starts_with('{') { continue; }
            let row = Self::parse_json_obj(line);
            if ordered_keys.is_empty() { ordered_keys = { let mut k: Vec<String>=row.keys().cloned().collect(); k.sort(); k }; }
            rows_data.push(row);
        }
        if rows_data.is_empty() { return Err("No valid JSON objects found".into()); }
        let cols: Vec<KColumn> = ordered_keys.iter().map(|k| KColumn::new(k, KType::Str)).collect();
        let rows: Vec<Vec<KVal>> = rows_data.iter().map(|r| {
            ordered_keys.iter().map(|k| KVal::Str(r.get(k).cloned().unwrap_or_default())).collect()
        }).collect();
        let w = KoreWriter::new(cols);
        w.write(dst, &rows).map_err(|e| e.to_string())?;
        Ok(rows.len())
    }

    /// .kore → NDJSON. Returns row count.
    pub fn to_json(path: &str, dst: &str) -> Result<usize, String> {
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let hdrs: Vec<String> = r.columns.iter().map(|c| c.name.clone()).collect();
        let cols = r.read_all_columns();
        let nrows = cols.first().map(|c| c.len()).unwrap_or(0);
        let mut f = fs::File::create(dst).map_err(|e| e.to_string())?;
        for ri in 0..nrows {
            let pairs: Vec<String> = hdrs.iter().zip(cols.iter())
                .map(|(h,col)| format!("\"{}\":\"{}\"", h, kv_str(&col[ri]).replace('"',"'"))).collect();
            writeln!(f, "{{{}}}", pairs.join(",")).map_err(|e| e.to_string())?;
        }
        Ok(nrows)
    }

    /// .kore → CSV. Returns row count.
    pub fn to_csv(path: &str, dst: &str) -> Result<usize, String> {
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let hdrs: Vec<String> = r.columns.iter().map(|c| c.name.clone()).collect();
        let cols = r.read_all_columns();
        let nrows = cols.first().map(|c| c.len()).unwrap_or(0);
        let mut f = fs::File::create(dst).map_err(|e| e.to_string())?;
        writeln!(f, "{}", hdrs.join(",")).map_err(|e| e.to_string())?;
        for ri in 0..nrows {
            let cells: Vec<String> = cols.iter().map(|col| {
                let v = kv_str(&col[ri]);
                if v.contains(',') || v.contains('"') { format!("\"{}\"", v.replace('"',"\"\"")) } else { v }
            }).collect();
            writeln!(f, "{}", cells.join(",")).map_err(|e| e.to_string())?;
        }
        Ok(nrows)
    }

    /// Merge multiple .kore files (same schema) → dst. Returns total rows.
    pub fn merge(paths: Vec<String>, dst: &str) -> Result<usize, String> {
        if paths.is_empty() { return Err("No files to merge".into()); }
        let r0 = KoreReader::open(&paths[0]).map_err(|e| e.to_string())?;
        let schema: Vec<KColumn> = r0.columns.iter().map(|c| KColumn::new(&c.name, c.ktype)).collect();
        let ncols = schema.len();
        drop(r0);
        let mut all_rows: Vec<Vec<KVal>> = vec![];
        for p in &paths {
            let r = KoreReader::open(p).map_err(|e| e.to_string())?;
            let cols = r.read_all_columns();
            let n = cols.first().map(|c| c.len()).unwrap_or(0);
            all_rows.extend(cols_to_rows(&cols, n));
        }
        // Pad any short rows
        for row in &mut all_rows { while row.len() < ncols { row.push(KVal::Null); } }
        let total = all_rows.len();
        let w = KoreWriter::new(schema);
        w.write(dst, &all_rows).map_err(|e| e.to_string())?;
        Ok(total)
    }

    /// Split .kore by distinct values of col → dst_dir/{val}.kore. Returns output paths.
    pub fn split_by(path: &str, col: &str, dst_dir: &str) -> Result<Vec<String>, String> {
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let hdrs: Vec<String> = r.columns.iter().map(|c| c.name.clone()).collect();
        let schema: Vec<KColumn> = r.columns.iter().map(|c| KColumn::new(&c.name, c.ktype)).collect();
        let ci = hdrs.iter().position(|h| h==col).ok_or(format!("Column '{}' not found",col))?;
        let cols = r.read_all_columns();
        let nrows = cols.first().map(|c| c.len()).unwrap_or(0);
        let rows = cols_to_rows(&cols, nrows);
        let mut groups: HashMap<String,Vec<Vec<KVal>>> = HashMap::new();
        for row in rows { let k = kv_str(&row[ci]); groups.entry(k).or_default().push(row); }
        fs::create_dir_all(dst_dir).map_err(|e| e.to_string())?;
        let mut out_paths = vec![];
        for (key, grp_rows) in &groups {
            let safe = key.chars().map(|c| if c.is_alphanumeric()||c=='-'||c=='_' { c } else { '_' }).collect::<String>();
            let out = format!("{}/{}.kore", dst_dir, safe);
            let w = KoreWriter::new(schema.clone());
            w.write(&out, grp_rows).map_err(|e| e.to_string())?;
            out_paths.push(out);
        }
        out_paths.sort();
        Ok(out_paths)
    }

    /// Random sample of n rows → dst. Returns rows sampled.
    pub fn sample(path: &str, n: usize, dst: &str) -> Result<usize, String> {
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let schema: Vec<KColumn> = r.columns.iter().map(|c| KColumn::new(&c.name, c.ktype)).collect();
        let cols = r.read_all_columns();
        let nrows = cols.first().map(|c| c.len()).unwrap_or(0);
        let mut idx: Vec<usize> = (0..nrows).collect();
        let mut rng = 1234567891u64;
        for i in (1..nrows).rev() {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (rng >> 33) as usize % (i+1); idx.swap(i,j);
        }
        idx.truncate(n.min(nrows)); idx.sort();
        let take = idx.len();
        let rows: Vec<Vec<KVal>> = idx.iter().map(|&ri| cols.iter().map(|c| c[ri].clone()).collect()).collect();
        let w = KoreWriter::new(schema);
        w.write(dst, &rows).map_err(|e| e.to_string())?;
        Ok(take)
    }

    /// First n rows → dst. Returns rows written.
    pub fn head(path: &str, n: usize, dst: &str) -> Result<usize, String> {
        let r = KoreReader::open(path).map_err(|e| e.to_string())?;
        let schema: Vec<KColumn> = r.columns.iter().map(|c| KColumn::new(&c.name, c.ktype)).collect();
        let cols = r.read_all_columns();
        let nrows = cols.first().map(|c| c.len()).unwrap_or(0);
        let take = n.min(nrows);
        let rows: Vec<Vec<KVal>> = (0..take).map(|ri| cols.iter().map(|c| c[ri].clone()).collect()).collect();
        let w = KoreWriter::new(schema);
        w.write(dst, &rows).map_err(|e| e.to_string())?;
        Ok(take)
    }

    /// Compare schemas of two .kore files. Returns diff string.
    pub fn schema_diff(path1: &str, path2: &str) -> Result<String, String> {
        let r1 = KoreReader::open(path1).map_err(|e| e.to_string())?;
        let r2 = KoreReader::open(path2).map_err(|e| e.to_string())?;
        let h1: Vec<String> = r1.columns.iter().map(|c| format!("{}:{:?}", c.name, c.ktype)).collect();
        let h2: Vec<String> = r2.columns.iter().map(|c| format!("{}:{:?}", c.name, c.ktype)).collect();
        let mut lines = vec![
            format!("File1: {} [{} rows, {} cols]", path1, r1.nrows, r1.columns.len()),
            format!("File2: {} [{} rows, {} cols]", path2, r2.nrows, r2.columns.len()),
        ];
        for (i, col) in h1.iter().enumerate() {
            match h2.get(i) {
                Some(c) if c == col => lines.push(format!("  = [{}] {}", i, col)),
                Some(c) => lines.push(format!("  ~ [{}] {} -> {}", i, col, c)),
                None => lines.push(format!("  - [{}] {} (missing in file2)", i, col)),
            }
        }
        for i in h1.len()..h2.len() { lines.push(format!("  + [{}] {} (new in file2)", i, h2[i])); }
        Ok(lines.join("\n"))
    }

    fn parse_json_obj(s: &str) -> HashMap<String,String> {
        let mut map = HashMap::new();
        let inner: Vec<char> = s.trim_start_matches('{').trim_end_matches('}').chars().collect();
        let (mut i, n) = (0, inner.len());
        while i < n {
            while i < n && inner[i].is_whitespace() { i+=1; }
            if i >= n || inner[i] != '"' { break; }
            i+=1; let ks=i; while i<n && inner[i]!='"' { i+=1; } let key: String=inner[ks..i].iter().collect(); i+=1;
            while i<n && inner[i]!=':' { i+=1; } i+=1;
            while i<n && inner[i].is_whitespace() { i+=1; }
            let val = if i<n && inner[i]=='"' {
                i+=1; let vs=i; while i<n && inner[i]!='"' { i+=1; } let v: String=inner[vs..i].iter().collect(); i+=1; v
            } else {
                let vs=i; while i<n && inner[i]!=',' && inner[i]!='}' { i+=1; }
                inner[vs..i].iter().collect::<String>().trim().to_string()
            };
            map.insert(key, val);
            while i<n && inner[i]!=',' { i+=1; }
            if i<n { i+=1; }
        }
        map
    }
}
