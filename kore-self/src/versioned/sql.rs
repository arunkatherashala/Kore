//! Read-only SQL over a snapshot: table/view registration and a query guard.

use super::model::*;
use super::{LogError, Snapshot};
use kore_core::{Column, DataBlock, Value};
use kore_sql::KqlContext;
use std::collections::{HashMap, HashSet};

const FORBIDDEN: [&str; 22] = [
    "insert", "update", "delete", "drop", "create", "alter", "truncate", "merge", "load", "copy", "attach", "detach",
    "replace", "grant", "revoke", "pragma", "vacuum", "call", "exec", "execute", "into", "set",
];

/// Accept exactly one SELECT / WITH query. Comments are stripped, string
/// literals are respected, a trailing `;` is allowed, anything after it is not.
/// Returns the comment-free statement.
pub fn check_readonly(sql: &str) -> Result<String, LogError> {
    let bad = |m: &str| Err(LogError::Invalid(format!("read-only SQL guard: {m}")));
    let cs: Vec<char> = sql.chars().collect();
    let (mut clean, mut masked) = (String::new(), String::new());
    let mut i = 0;
    let mut stmts = 0usize;
    let mut cur_nonblank = false;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '-' if cs.get(i + 1) == Some(&'-') => {
                while i < cs.len() && cs[i] != '\n' {
                    i += 1;
                }
                clean.push(' ');
                masked.push(' ');
            }
            '/' if cs.get(i + 1) == Some(&'*') => {
                i += 2;
                loop {
                    if i + 1 >= cs.len() {
                        return bad("unterminated block comment");
                    }
                    if cs[i] == '*' && cs[i + 1] == '/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                clean.push(' ');
                masked.push(' ');
            }
            '\'' | '"' => {
                clean.push(c);
                masked.push(' ');
                i += 1;
                cur_nonblank = true;
                loop {
                    if i >= cs.len() {
                        return bad("unterminated quoted string");
                    }
                    clean.push(cs[i]);
                    masked.push(' ');
                    if cs[i] == c {
                        if cs.get(i + 1) == Some(&c) {
                            clean.push(c);
                            masked.push(' ');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            ';' => {
                if cur_nonblank {
                    stmts += 1;
                    cur_nonblank = false;
                }
                // Only whitespace/comments may follow a terminating ';'.
                let mut j = i + 1;
                while j < cs.len() {
                    if cs[j].is_whitespace() {
                        j += 1;
                    } else if cs[j] == '-' && cs.get(j + 1) == Some(&'-') {
                        while j < cs.len() && cs[j] != '\n' {
                            j += 1;
                        }
                    } else if cs[j] == '/' && cs.get(j + 1) == Some(&'*') {
                        j += 2;
                        while j + 1 < cs.len() && !(cs[j] == '*' && cs[j + 1] == '/') {
                            j += 1;
                        }
                        j += 2;
                    } else {
                        return bad("multiple statements are not allowed");
                    }
                }
                i = cs.len();
            }
            _ => {
                if !c.is_whitespace() {
                    cur_nonblank = true;
                }
                clean.push(c);
                masked.push(c);
                i += 1;
            }
        }
    }
    if cur_nonblank {
        stmts += 1;
    }
    if stmts != 1 {
        return bad("exactly one statement is required");
    }
    let words: Vec<String> = masked
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    match words.first().map(String::as_str) {
        Some("select") | Some("with") => {}
        _ => return bad("only SELECT or WITH queries are allowed"),
    }
    if let Some(w) = words.iter().find(|w| FORBIDDEN.contains(&w.as_str())) {
        return bad(&format!("keyword '{w}' is not allowed"));
    }
    Ok(clean.trim().trim_end_matches(';').trim().to_string())
}

fn revisions_block(rows: &[&RevisionRow], supported: &HashSet<String>) -> DataBlock {
    let s = |f: fn(&RevisionRow) -> String| -> Vec<Option<String>> { rows.iter().map(|r| Some(f(r))).collect() };
    DataBlock::new(vec![
        Column::str_col("rev_id", s(|r| r.rev_id.clone())),
        Column::str_col("entity_type", s(|r| r.entity_type.clone())),
        Column::str_col("key", s(|r| r.key.clone())),
        Column::str_col("stance", s(|r| r.stance.clone())),
        Column::float64("confidence", rows.iter().map(|r| Some(r.confidence)).collect()),
        Column::str_col("rationale", s(|r| r.rationale.clone())),
        Column::str_col("supersedes", rows.iter().map(|r| r.supersedes.clone()).collect()),
        Column::str_col("valid_from", s(|r| r.valid_from.clone())),
        Column::str_col("author", s(|r| r.author.clone())),
        Column::str_col("recorded_at", s(|r| r.recorded_at.clone())),
        // 1 = no cited event (user revision without evidence).
        Column::int64("unsupported", rows.iter().map(|r| Some(if supported.contains(&r.rev_id) { 0 } else { 1 })).collect()),
    ])
    .expect("equal-length columns")
}

/// beliefs_current: per (entity_type, key), the latest revision that no other
/// revision supersedes. beliefs_asof: per (entity_type, key), the latest
/// revision with `valid_from <= snapshot.valid_at` that is not superseded by
/// another such revision.
pub fn current_heads<'a>(revs: &'a [RevisionRow], valid_at: Option<&str>) -> Vec<&'a RevisionRow> {
    let cand: Vec<(usize, &RevisionRow)> = revs.iter().enumerate().filter(|(_, r)| valid_at.map_or(true, |v| r.valid_from.as_str() <= v)).collect();
    let superseded: HashSet<&str> = cand.iter().filter_map(|(_, r)| r.supersedes.as_deref()).collect();
    let mut best: HashMap<(&str, &str), (usize, &RevisionRow)> = HashMap::new();
    for (i, r) in cand.into_iter().filter(|(_, r)| !superseded.contains(r.rev_id.as_str())) {
        let k = (r.entity_type.as_str(), r.key.as_str());
        let better = best.get(&k).map_or(true, |(bi, b)| (r.valid_from.as_str(), i) > (b.valid_from.as_str(), *bi));
        if better {
            best.insert(k, (i, r));
        }
    }
    let mut v: Vec<(usize, &RevisionRow)> = best.into_values().collect();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, r)| r).collect()
}

/// Register `events`, `revisions` (+ computed `unsupported`), `revision_causes`,
/// `predictions`, `outcomes`, `schema_meta`, and the views `beliefs_current`
/// and `beliefs_asof` for this snapshot.
pub fn register_into(ctx: &mut KqlContext, snap: &Snapshot) {
    for t in [Table::Events, Table::RevisionCauses, Table::Predictions, Table::Outcomes, Table::SchemaMeta] {
        ctx.register(t.name(), snap.block(t));
    }
    let revs: Vec<RevisionRow> = snap.rows();
    let causes: Vec<CauseRow> = snap.rows();
    let supported: HashSet<String> = causes.into_iter().map(|c| c.rev_id).collect();
    let all: Vec<&RevisionRow> = revs.iter().collect();
    ctx.register("revisions", revisions_block(&all, &supported));
    ctx.register("beliefs_current", revisions_block(&current_heads(&revs, None), &supported));
    ctx.register("beliefs_asof", revisions_block(&current_heads(&revs, Some(&snap.valid_at)), &supported));
}

/// Run one SELECT/WITH query on an in-memory copy of the snapshot.
pub fn run_readonly_sql(snap: &Snapshot, sql: &str) -> Result<DataBlock, LogError> {
    let clean = check_readonly(sql)?;
    let mut ctx = KqlContext::new();
    register_into(&mut ctx, snap);
    ctx.query(&clean).map_err(|e| LogError::Invalid(format!("query error: {e}")))
}

/// Rows as display strings (NULL for nulls).
pub fn block_strings(b: &DataBlock) -> Vec<Vec<String>> {
    (0..b.num_rows)
        .map(|i| {
            b.columns
                .iter()
                .map(|c| match c.data.get_value(i) {
                    Value::Null => "NULL".into(),
                    Value::Int(n) => n.to_string(),
                    Value::Float(f) => format!("{f}"),
                    Value::Bool(x) => x.to_string(),
                    Value::Str(s) => s,
                })
                .collect()
        })
        .collect()
}

/// Plain-text table, cells cut at `max_w` characters (char-boundary safe).
pub fn render(b: &DataBlock, max_w: usize) -> String {
    if b.num_rows == 0 {
        return "(no rows)".into();
    }
    let cut = |s: &str| -> String {
        if s.chars().count() > max_w {
            format!("{}~", s.chars().take(max_w.saturating_sub(1)).collect::<String>())
        } else {
            s.to_string()
        }
    };
    let head: Vec<String> = b.columns.iter().map(|c| cut(&c.name)).collect();
    let rows: Vec<Vec<String>> = block_strings(b).into_iter().map(|r| r.iter().map(|c| cut(&c.replace('\n', " "))).collect()).collect();
    let w: Vec<usize> = (0..head.len()).map(|i| rows.iter().map(|r| r[i].chars().count()).chain([head[i].chars().count()]).max().unwrap_or(0)).collect();
    let line = |cells: &[String]| cells.iter().zip(&w).map(|(c, w)| format!("{c:<w$}")).collect::<Vec<_>>().join(" | ");
    let mut out = vec![line(&head), w.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("-+-")];
    out.extend(rows.iter().map(|r| line(r)));
    out.push(format!("({} rows)", rows.len()));
    out.join("\n")
}
