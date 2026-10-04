//! INSERT / UPDATE / DELETE / CREATE TABLE|VIEW / DROP / TRUNCATE for `KqlContext::execute_dml`.
//!
//! The statements are split with a small quote/paren aware scanner; every expression (VALUES rows, SET
//! right-hand sides, WHERE predicates, the SELECT of a CTAS) is evaluated by the regular query engine so the
//! semantics (NULL logic, string escapes, functions) are the same as in SELECT.

use std::sync::Arc;

use kore_core::{Column, ColumnData, DataBlock, KoreError};

use crate::executor::{ExprVal as V, KqlContext};
use crate::general::{column_values, vals_to_column};

fn err(m: impl Into<String>) -> KoreError { KoreError::InvalidArgument(m.into()) }

/// Split on `sep` at nesting depth 0, outside quotes.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0usize);
    let mut quote: Option<char> = None;
    for (i, ch) in s.char_indices() {
        match quote {
            Some(q) => if ch == q { quote = None },
            None => match ch {
                '\'' | '"' | '`' => quote = Some(ch),
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                c if c == sep && depth == 0 => { out.push(&s[start..i]); start = i + c.len_utf8(); }
                _ => {}
            },
        }
    }
    out.push(&s[start..]);
    out
}

/// Byte position of keyword `kw` (upper case, surrounded by whitespace or parens) at depth 0 outside quotes.
fn find_top_kw(s: &str, kw: &str) -> Option<usize> {
    let up = s.to_ascii_uppercase();
    let b = up.as_bytes();
    let (mut depth, mut quote) = (0i32, None::<u8>);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) => if c == q { quote = None },
            None => match c {
                b'\'' | b'"' | b'`' => quote = Some(c),
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ if depth == 0 && up[i..].starts_with(kw)
                    && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
                    && b.get(i + kw.len()).map_or(true, |n| !(n.is_ascii_alphanumeric() || *n == b'_')) => return Some(i),
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// Consume the leading keyword sequence `words` (each optional word is tried in order) from `s`.
fn strip_kw<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let t = s.trim_start();
    if t.len() >= kw.len() && t[..kw.len()].eq_ignore_ascii_case(kw)
        && t[kw.len()..].chars().next().map_or(true, |c| !(c.is_alphanumeric() || c == '_')) {
        Some(t[kw.len()..].trim_start())
    } else { None }
}

fn ident(s: &str) -> String { s.trim().trim_matches('`').trim_matches('"').to_string() }

/// Leading identifier (stops at whitespace or `(`).
fn take_ident(s: &str) -> (String, &str) {
    let s = s.trim_start();
    let end = s.find(|c: char| c.is_whitespace() || c == '(' || c == ';').unwrap_or(s.len());
    (ident(&s[..end]), s[end..].trim_start())
}

fn stored_name(ctx: &KqlContext, name: &str) -> String {
    ctx.table_names().into_iter().find(|n| n.eq_ignore_ascii_case(name)).unwrap_or_else(|| name.to_string())
}

fn set_table(ctx: &mut KqlContext, name: &str, block: DataBlock) {
    let name = stored_name(ctx, name);
    ctx.register_mut(name.clone(), block.clone());
    ctx.register(name, block);
}

fn table_block(ctx: &KqlContext, name: &str) -> Result<DataBlock, KoreError> {
    ctx.get(name).cloned().ok_or_else(|| err(format!("Table not found: {name}")))
}

fn sql_type(t: &str) -> Result<ColumnData, KoreError> {
    let base = t.trim().to_ascii_lowercase();
    let base = base.split('(').next().unwrap_or("").trim().to_string();
    Ok(match base.as_str() {
        "int" | "integer" | "bigint" | "long" | "smallint" | "short" | "tinyint" | "byte" => ColumnData::Int64(vec![]),
        "double" | "float" | "real" | "decimal" | "numeric" | "dec" => ColumnData::Float64(vec![]),
        "string" | "varchar" | "char" | "text" | "date" | "timestamp" | "binary" => ColumnData::Str(vec![]),
        "boolean" | "bool" => ColumnData::Bool(vec![]),
        other => return Err(err(format!("unsupported column type: {other}"))),
    })
}

/// Convert a value to the storage type of a column (Spark's store assignment, without silent truncation).
fn coerce(v: V, into: &ColumnData, col: &str) -> Result<V, KoreError> {
    if matches!(v, V::Null) { return Ok(V::Null); }
    let bad = |v: &V| err(format!("cannot store {v:?} in column {col}"));
    Ok(match into {
        ColumnData::Int64(_) => match v {
            V::Int(_) => v,
            V::Float(f) if f.fract() == 0.0 && f.abs() < 9.2e18 => V::Int(f as i64),
            V::Str(ref s) => V::Int(s.trim().parse::<i64>().map_err(|_| bad(&v))?),
            other => return Err(bad(&other)),
        },
        ColumnData::Float64(_) => match v {
            V::Float(_) => v,
            V::Int(i) => V::Float(i as f64),
            V::Str(ref s) => V::Float(s.trim().parse::<f64>().map_err(|_| bad(&v))?),
            other => return Err(bad(&other)),
        },
        ColumnData::Bool(_) => match v {
            V::Bool(_) => v,
            other => return Err(bad(&other)),
        },
        ColumnData::Str(_) | ColumnData::StrDict { .. } => match v {
            V::Str(_) => v,
            V::Int(i) => V::Str(i.to_string()),
            V::Float(f) => V::Str(crate::scalar::fmt_f64(f)),
            V::Bool(b) => V::Str(b.to_string()),
            V::Null => V::Null,
        },
    })
}

fn rebuild(block: &DataBlock, cols: Vec<Vec<V>>) -> DataBlock {
    let n = cols.first().map_or(0, |c| c.len());
    let columns = block.columns.iter().zip(cols)
        .map(|(c, vals)| vals_to_column(c.name.clone(), vals, Some(&c.data)))
        .collect();
    DataBlock { columns, num_rows: n }
}

/// Entry point: `None` when `sql` is not one of the statements handled here.
pub fn execute(ctx: &mut KqlContext, sql: &str) -> Option<Result<(String, usize), KoreError>> {
    let s = sql.trim().trim_end_matches(';').trim();
    if let Some(rest) = strip_kw(s, "INSERT") { return Some(insert(ctx, rest)); }
    if let Some(rest) = strip_kw(s, "UPDATE") { return Some(update(ctx, rest)); }
    if let Some(rest) = strip_kw(s, "DELETE") { return Some(delete(ctx, rest)); }
    if let Some(rest) = strip_kw(s, "TRUNCATE") { return Some(truncate(ctx, rest)); }
    if let Some(rest) = strip_kw(s, "CREATE") { return create(ctx, rest); }
    if let Some(rest) = strip_kw(s, "DROP") { return Some(drop_stmt(ctx, rest)); }
    None
}

fn insert(ctx: &mut KqlContext, rest: &str) -> Result<(String, usize), KoreError> {
    let (overwrite, rest) = if let Some(r) = strip_kw(rest, "OVERWRITE") { (true, r) }
        else if let Some(r) = strip_kw(rest, "INTO") { (false, r) }
        else { return Err(err("INSERT: expected INTO or OVERWRITE")); };
    let rest = strip_kw(rest, "TABLE").unwrap_or(rest);
    let (name, rest) = take_ident(rest);
    if name.is_empty() { return Err(err("INSERT INTO: missing table name")); }
    // optional column list
    let (cols, rest): (Option<Vec<String>>, &str) = if rest.starts_with('(') {
        let close = rest.find(')').ok_or_else(|| err("INSERT: unbalanced column list"))?;
        (Some(rest[1..close].split(',').map(ident).collect()), rest[close + 1..].trim_start())
    } else { (None, rest) };
    let existing = ctx.get(&name).cloned();
    let source = if let Some(v) = strip_kw(rest, "VALUES") {
        ctx.query(&format!("SELECT * FROM (VALUES {v}) AS q"))?
    } else if strip_kw(rest, "SELECT").is_some() || strip_kw(rest, "WITH").is_some() || rest.starts_with('(') {
        ctx.query(rest)?
    } else {
        return Err(err(format!("INSERT: expected SELECT or VALUES, got: {}", &rest[..rest.len().min(20)])));
    };
    let rows = source.num_rows;
    let new_block = match existing.filter(|b| !b.columns.is_empty()) {
        None => {
            // table does not exist yet (or has no schema): the inserted rows define it
            let mut b = source;
            if let Some(cl) = &cols {
                if cl.len() != b.columns.len() { return Err(err("INSERT: column list and value count differ")); }
                for (c, n) in b.columns.iter_mut().zip(cl) { c.name = n.clone(); }
            }
            b
        }
        Some(tbl) => {
            let ncols = tbl.columns.len();
            let targets: Vec<usize> = match &cols {
                Some(cl) => cl.iter().map(|n| tbl.columns.iter().position(|c| c.name.eq_ignore_ascii_case(n))
                    .ok_or_else(|| err(format!("INSERT: unknown column {n}")))).collect::<Result<_, _>>()?,
                None => (0..ncols).collect(),
            };
            if targets.len() != source.columns.len() {
                return Err(err(format!("INSERT: table {name} expects {} values per row, got {}", targets.len(), source.columns.len())));
            }
            let src_vals: Vec<Vec<V>> = source.columns.iter().map(column_values).collect();
            let mut merged: Vec<Vec<V>> = tbl.columns.iter().map(|c| if overwrite { Vec::new() } else { column_values(c) }).collect();
            for (k, c) in tbl.columns.iter().enumerate() {
                match targets.iter().position(|&t| t == k) {
                    Some(si) => for v in &src_vals[si] { merged[k].push(coerce(v.clone(), &c.data, &c.name)?); },
                    None => merged[k].extend(std::iter::repeat(V::Null).take(rows)),
                }
            }
            rebuild(&tbl, merged)
        }
    };
    set_table(ctx, &name, new_block);
    Ok(("INSERT".into(), rows))
}

/// The table with a trailing `__rid` row-number column, registered under `name` in a throw-away context.
fn with_rowids(ctx: &KqlContext, name: &str, block: &DataBlock) -> KqlContext {
    let mut c = ctx.clone();
    let mut b = block.clone();
    b.columns.push(Column::int64("__rid", (0..block.num_rows as i64).map(Some).collect()));
    c.register(stored_name(ctx, name), b);
    c
}

fn update(ctx: &mut KqlContext, rest: &str) -> Result<(String, usize), KoreError> {
    let (name, rest) = take_ident(rest);
    let rest = strip_kw(rest, "SET").ok_or_else(|| err("UPDATE: missing SET"))?;
    let (assign_str, where_str) = match find_top_kw(rest, "WHERE") {
        Some(p) => (&rest[..p], Some(rest[p + 5..].trim())),
        None => (rest, None),
    };
    let tbl = table_block(ctx, &name)?;
    let mut sets: Vec<(usize, String)> = Vec::new();
    for a in split_top(assign_str, ',') {
        let eq = split_top(a, '=');
        if eq.len() < 2 { return Err(err(format!("UPDATE: bad assignment '{}'", a.trim()))); }
        let col = ident(eq[0]);
        let col = col.rsplit('.').next().unwrap_or(&col).to_string();
        let idx = tbl.columns.iter().position(|c| c.name.eq_ignore_ascii_case(&col)).ok_or_else(|| err(format!("UPDATE: unknown column {col}")))?;
        let expr = a[a.find('=').unwrap() + 1..].trim().to_string();
        sets.push((idx, expr));
    }
    let tmp = with_rowids(ctx, &name, &tbl);
    let mut proj = vec!["__rid".to_string()];
    for (k, (_, e)) in sets.iter().enumerate() { proj.push(format!("({e}) AS __s{k}")); }
    let mut q = format!("SELECT {} FROM {}", proj.join(", "), stored_name(ctx, &name));
    if let Some(w) = where_str { q.push_str(&format!(" WHERE ({w})")); }
    let hit = tmp.query(&q)?;
    let rids: Vec<usize> = match &hit.columns[0].data {
        ColumnData::Int64(v) => v.iter().flatten().map(|&x| x as usize).collect(),
        _ => Vec::new(),
    };
    let mut cols: Vec<Vec<V>> = tbl.columns.iter().map(column_values).collect();
    for (k, (idx, _)) in sets.iter().enumerate() {
        let newv = column_values(&hit.columns[k + 1]);
        let c = &tbl.columns[*idx];
        for (j, &rid) in rids.iter().enumerate() { cols[*idx][rid] = coerce(newv[j].clone(), &c.data, &c.name)?; }
    }
    let out = rebuild(&tbl, cols);
    set_table(ctx, &name, out);
    Ok(("UPDATE".into(), rids.len()))
}

fn delete(ctx: &mut KqlContext, rest: &str) -> Result<(String, usize), KoreError> {
    let rest = strip_kw(rest, "FROM").ok_or_else(|| err("DELETE: expected FROM"))?;
    let (name, rest) = take_ident(rest);
    let tbl = table_block(ctx, &name)?;
    let where_str = match strip_kw(rest, "WHERE") {
        Some(w) => Some(w),
        None if rest.is_empty() => None,
        None => return Err(err(format!("DELETE: unexpected '{}'", &rest[..rest.len().min(20)]))),
    };
    let doomed: Vec<usize> = match where_str {
        None => (0..tbl.num_rows).collect(),
        Some(w) => {
            let tmp = with_rowids(ctx, &name, &tbl);
            let hit = tmp.query(&format!("SELECT __rid FROM {} WHERE ({w})", stored_name(ctx, &name)))?;
            match &hit.columns[0].data { ColumnData::Int64(v) => v.iter().flatten().map(|&x| x as usize).collect(), _ => Vec::new() }
        }
    };
    let dead: std::collections::HashSet<usize> = doomed.iter().copied().collect();
    let keep: Vec<usize> = (0..tbl.num_rows).filter(|r| !dead.contains(r)).collect();
    set_table(ctx, &name, tbl.select_rows(&keep));
    Ok(("DELETE".into(), dead.len()))
}

fn truncate(ctx: &mut KqlContext, rest: &str) -> Result<(String, usize), KoreError> {
    let rest = strip_kw(rest, "TABLE").unwrap_or(rest);
    let (name, _) = take_ident(rest);
    let tbl = table_block(ctx, &name)?;
    let n = tbl.num_rows;
    set_table(ctx, &name, tbl.select_rows(&[]));
    Ok(("TRUNCATE".into(), n))
}

fn create(ctx: &mut KqlContext, rest: &str) -> Option<Result<(String, usize), KoreError>> {
    let mut r = rest;
    let mut or_replace = false;
    if let Some(x) = strip_kw(r, "OR") { if let Some(y) = strip_kw(x, "REPLACE") { or_replace = true; r = y; } }
    let mut temp = false;
    for kw in ["GLOBAL", "TEMPORARY", "TEMP"] { if let Some(x) = strip_kw(r, kw) { temp = true; r = x; } }
    let _ = temp;
    let is_view = if let Some(x) = strip_kw(r, "VIEW") { r = x; true } else if let Some(x) = strip_kw(r, "TABLE") { r = x; false } else { return None };
    let mut if_not_exists = false;
    if let Some(x) = strip_kw(r, "IF") { if let Some(y) = strip_kw(x, "NOT") { if let Some(z) = strip_kw(y, "EXISTS") { if_not_exists = true; r = z; } } }
    let (name, mut rest) = take_ident(r);
    if name.is_empty() { return Some(Err(err("CREATE: missing name"))); }
    let exists = if is_view { ctx.view_exists(&name) } else { ctx.get(&name).is_some() };
    if exists && if_not_exists { return Some(Ok((if is_view { "CREATE VIEW" } else { "CREATE TABLE" }.into(), 0))); }
    // CREATE TABLE name (col type, ...)
    if !is_view && rest.starts_with('(') {
        let mut depth = 0;
        let mut close = None;
        for (i, ch) in rest.char_indices() { match ch { '(' => depth += 1, ')' => { depth -= 1; if depth == 0 { close = Some(i); break; } } _ => {} } }
        let Some(close) = close else { return Some(Err(err("CREATE TABLE: unbalanced column list"))) };
        let defs = &rest[1..close];
        let mut cols = Vec::new();
        for d in split_top(defs, ',') {
            let mut it = d.trim().splitn(2, char::is_whitespace);
            let (cn, ct) = (ident(it.next().unwrap_or("")), it.next().unwrap_or("").trim());
            let ct = ct.split_whitespace().next().unwrap_or("");
            let data = match sql_type(ct) { Ok(d) => d, Err(e) => return Some(Err(e)) };
            cols.push(Column { name: cn, data });
        }
        let after = rest[close + 1..].trim();
        if strip_kw(after, "AS").is_none() {
            let n = cols.len();
            let _ = n;
            set_table(ctx, &name, DataBlock { columns: cols, num_rows: 0 });
            return Some(Ok(("CREATE TABLE".into(), 0)));
        }
        rest = after;
    }
    // optional USING / STORED AS / PARTITIONED BY ... clauses before AS
    let as_pos = match find_top_kw(rest, "AS") { Some(p) => p, None => return Some(Err(err("CREATE: missing AS"))) };
    let select_sql = rest[as_pos + 2..].trim();
    let mut result = match ctx.query(select_sql) { Ok(b) => b, Err(e) => return Some(Err(e)) };
    if is_view {
        let _ = or_replace;
        // a view stores its text; validate it now so a broken definition fails at CREATE time
        ctx.create_view(name, select_sql.to_string());
        return Some(Ok(("CREATE VIEW".into(), 0)));
    }
    for c in &mut result.columns { c.name = crate::general::bare_name(&c.name); }
    let n = result.num_rows;
    set_table(ctx, &name, result);
    Some(Ok(("CREATE TABLE AS SELECT".into(), n)))
}

fn drop_stmt(ctx: &mut KqlContext, rest: &str) -> Result<(String, usize), KoreError> {
    let (is_view, rest) = if let Some(r) = strip_kw(rest, "VIEW") { (true, r) }
        else if let Some(r) = strip_kw(rest, "TABLE") { (false, r) }
        else { return Err(err("DROP: expected TABLE or VIEW")); };
    let (if_exists, rest) = match strip_kw(rest, "IF").and_then(|x| strip_kw(x, "EXISTS")) { Some(r) => (true, r), None => (false, rest) };
    let (name, _) = take_ident(rest);
    let gone = if is_view { ctx.drop_view(&name) } else { let n = stored_name(ctx, &name); ctx.drop_table(&n) };
    if !gone && !if_exists { return Err(err(format!("{} not found: {name}", if is_view { "View" } else { "Table" }))); }
    Ok((if is_view { "DROP VIEW" } else { "DROP TABLE" }.into(), 0))
}

#[allow(dead_code)]
fn _unused(_: Arc<DataBlock>) {}
