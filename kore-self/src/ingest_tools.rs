//! M2a tools: folder / CSV / .kore ingestion, export, and forget.
//!
//! Where input may come from: the owner directory (`<data root>/<owner>/`) and
//! any folder listed, one per line, in the file `ingest_roots` inside that
//! directory. That file is edited by hand; no tool can read or write it
//! (`safety::safe_path` refuses it). Paths are canonicalised before the check, so
//! `..` and symlinks cannot leave an allowed root. Exports are written only under
//! `<owner dir>/exports/`.
//!
//! Every event passes through `redact::Redactor` before it is stored, and every
//! event is dated by the source (front-matter, heading, file name, or a CSV
//! column). Nothing is ever stamped with the current time.

use crate::log_tools::{err, log_err, open, text};
use crate::redact::{Redactor, PATTERN_FILE};
use crate::safety;
use crate::versioned::{self as vl, Batch, EventRow, NewEvent, Row, Table, VersionedLog};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const ROOTS_FILE: &str = "ingest_roots";

const MAX_FILES: usize = 2000;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_DEPTH: usize = 32;
const MAX_CSV_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CSV_ROWS: usize = 50_000;
const MAX_KORE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_KORE_ROWS: usize = 200_000;
const MAX_LISTED: usize = 20;
const MAX_LISTED_PATHS: usize = 50;

/// Path for messages: drop the Windows verbatim prefix that canonicalisation adds.
fn disp(p: &Path) -> String {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

// ─── allow-list ──────────────────────────────────────────────────────────────

fn owner_canon(owner: &str) -> Result<PathBuf, Value> {
    safety::validate_owner(owner).map_err(|e| err(format!("invalid owner: {e}")))?;
    let dir = safety::owner_dir(owner);
    std::fs::create_dir_all(&dir).map_err(|e| err(format!("cannot create owner dir: {e}")))?;
    dir.canonicalize().map_err(|e| err(format!("cannot resolve owner dir: {e}")))
}

/// Canonical allowed roots: the owner directory plus every existing folder in
/// the `ingest_roots` file. Lines that do not resolve to a folder are reported
/// in the second element and otherwise ignored.
pub fn allowed_roots(owner_dir: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut roots = vec![owner_dir.to_path_buf()];
    let mut warnings = Vec::new();
    if let Ok(t) = std::fs::read_to_string(owner_dir.join(ROOTS_FILE)) {
        for line in t.lines() {
            let l = line.trim().trim_matches('"');
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            match Path::new(l).canonicalize() {
                Ok(p) if p.is_dir() => roots.push(p),
                _ => warnings.push(format!("ingest_roots entry ignored (not an existing folder): {l}")),
            }
        }
    }
    (roots, warnings)
}

/// Resolve a user path to a canonical existing path inside an allowed root.
fn resolve_input(owner: &str, arg: &str) -> Result<(PathBuf, PathBuf, Vec<String>), Value> {
    let p = arg.trim();
    if p.is_empty() {
        return Err(err("'path' must not be empty"));
    }
    if p.contains('\0') {
        return Err(err("path contains NUL"));
    }
    let oc = owner_canon(owner)?;
    let (roots, warnings) = allowed_roots(&oc);
    let raw = Path::new(p);
    let joined = if raw.is_absolute() { raw.to_path_buf() } else { oc.join(raw) };
    let canon = joined.canonicalize().map_err(|e| err(format!("cannot read '{}': {e}", safety::trunc(p, 120))))?;
    if !roots.iter().any(|r| canon.starts_with(r)) {
        let listed: Vec<String> = roots.iter().map(|r| disp(r)).collect();
        return Err(err(format!(
            "Rejected path '{}': it is not inside an allowed ingest root. Allowed: {}. To allow another folder, add its absolute path as a line in {} (edit that file by hand; no tool can change it).",
            safety::trunc(p, 120),
            listed.join(" | "),
            disp(&oc.join(ROOTS_FILE))
        )));
    }
    for reserved in [ROOTS_FILE, PATTERN_FILE] {
        if canon == oc.join(reserved) {
            return Err(err(format!("'{reserved}' is reserved and cannot be ingested")));
        }
    }
    Ok((canon, oc, warnings))
}

// ─── shared event pipeline ───────────────────────────────────────────────────

#[derive(Default)]
struct Tally {
    added: usize,
    would_add: usize,
    duplicate: usize,
    suppressed: usize,
    redacted: usize,
    errors: usize,
    error_details: Vec<String>,
}

impl Tally {
    fn error(&mut self, msg: String) {
        self.errors += 1;
        if self.error_details.len() < MAX_LISTED {
            self.error_details.push(msg);
        }
    }
}

/// Redact, normalise and validate one event; errors are strings for the tally.
fn prepare(red: &Redactor, tally: &mut Tally, source: &str, occurred: &str, kind: &str, raw_text: &str, tags: Vec<String>, already_redacted: bool) -> Result<NewEvent, String> {
    let occurred_at = vl::normalize_ts(occurred)?;
    let r = red.redact(raw_text);
    if r.changed() {
        tally.redacted += 1;
    }
    let changed = r.changed();
    let ev = NewEvent { source: source.to_string(), occurred_at, kind: kind.to_string(), text: r.text, tags, redacted: changed || already_redacted };
    VersionedLog::check_new_event(&ev).map_err(|e| e.to_string())?;
    Ok(ev)
}

/// Classify and (unless `dry_run`) commit events as one version. Returns the
/// version after the call.
fn commit_events(log: &mut VersionedLog, tally: &mut Tally, events: Vec<NewEvent>, dry_run: bool) -> Result<u64, Value> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut fresh: Vec<NewEvent> = Vec::new();
    for e in events {
        let id = vl::event_id(&e.source, &e.occurred_at, &e.text);
        if log.is_forgotten(&id) {
            tally.suppressed += 1;
        } else if log.has_event(&id) || !seen.insert(id) {
            tally.duplicate += 1;
        } else {
            fresh.push(e);
        }
    }
    let n = fresh.len();
    if dry_run {
        tally.would_add = n;
        return Ok(log.latest_version());
    }
    if n == 0 {
        return Ok(log.latest_version());
    }
    let info = log.commit_batch(Batch { events: fresh, ..Default::default() }).map_err(log_err)?;
    tally.added = n;
    Ok(info.version)
}

fn dry_run_flag(args: &Value) -> Result<bool, Value> {
    match args.get("dry_run") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(err("'dry_run' must be true or false")),
    }
}

// ─── dates ───────────────────────────────────────────────────────────────────

/// First valid ISO date (`YYYY-MM-DD`, optionally `THH:MM:SS[Z]`) in `s`, normalised.
pub fn find_iso(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() < 10 {
        return None;
    }
    for i in 0..=b.len() - 10 {
        let w = &b[i..i + 10];
        let shape = w[..4].iter().all(u8::is_ascii_digit) && w[4] == b'-' && w[5..7].iter().all(u8::is_ascii_digit) && w[7] == b'-' && w[8..].iter().all(u8::is_ascii_digit);
        if !shape || (i > 0 && b[i - 1].is_ascii_digit()) || b.get(i + 10).map_or(false, u8::is_ascii_digit) {
            continue;
        }
        if b.get(i + 10) == Some(&b'T') {
            let long = if b.get(i + 19) == Some(&b'Z') { s.get(i..i + 20) } else { s.get(i..i + 19) };
            if let Some(Ok(t)) = long.map(vl::normalize_ts) {
                return Some(t);
            }
        }
        if let Ok(t) = vl::normalize_ts(&s[i..i + 10]) {
            return Some(t);
        }
    }
    None
}

/// Date of a journal file: front-matter `date:`, else the first heading that
/// contains an ISO date, else a file name starting with `YYYY-MM-DD`.
pub fn journal_date(content: &str, file_name: &str) -> Option<String> {
    let body = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = body.lines().enumerate().peekable();
    let mut after_front = 0usize;
    if body.lines().next().map(|l| l.trim_end() == "---").unwrap_or(false) {
        lines.next();
        for (i, l) in lines.by_ref().take(100) {
            let t = l.trim();
            if t == "---" || t == "..." {
                after_front = i + 1;
                break;
            }
            if let Some((k, v)) = t.split_once(':') {
                if k.trim().eq_ignore_ascii_case("date") {
                    if let Some(d) = find_iso(v.trim().trim_matches(|c| c == '"' || c == '\'')) {
                        return Some(d);
                    }
                }
            }
        }
    }
    for l in body.lines().skip(after_front) {
        let t = l.trim_start();
        if t.starts_with('#') {
            if let Some(d) = find_iso(t) {
                return Some(d);
            }
        }
    }
    let prefix = file_name.get(..10)?;
    if file_name.as_bytes().get(10).map_or(false, u8::is_ascii_digit) {
        return None;
    }
    vl::normalize_ts(prefix).ok().filter(|_| prefix.as_bytes()[4] == b'-')
}

// ─── self_ingest_folder ──────────────────────────────────────────────────────

struct Walk {
    files: Vec<PathBuf>,
    skipped_symlinks: Vec<String>,
    truncated: bool,
}

fn rel_of(start: &Path, p: &Path) -> String {
    p.strip_prefix(start).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")).unwrap_or_default()
}

fn walk(start: &Path) -> Walk {
    let mut w = Walk { files: vec![], skipped_symlinks: vec![], truncated: false };
    let mut stack: Vec<(PathBuf, usize)> = vec![(start.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd.flatten().collect(),
            Err(_) => continue,
        };
        entries.sort_by_key(|e| e.file_name());
        let mut subdirs: Vec<(PathBuf, usize)> = Vec::new();
        for e in entries {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let ft = match e.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                // follow a file symlink only if its target stays inside the folder; never follow dir links
                match path.canonicalize() {
                    Ok(t) if t.is_file() && t.starts_with(start) => {}
                    _ => {
                        w.skipped_symlinks.push(rel_of(start, &path));
                        continue;
                    }
                }
            } else if ft.is_dir() {
                if name.starts_with('.') || depth + 1 > MAX_DEPTH {
                    continue;
                }
                match path.canonicalize() {
                    Ok(c) if c.starts_with(start) => subdirs.push((c, depth + 1)),
                    _ => w.skipped_symlinks.push(rel_of(start, &path)),
                }
                continue;
            }
            let ext = Path::new(&name).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext != "md" && ext != "txt" {
                continue;
            }
            if w.files.len() >= MAX_FILES {
                w.truncated = true;
                return w;
            }
            w.files.push(path);
        }
        // depth-first in name order
        for s in subdirs.into_iter().rev() {
            stack.push(s);
        }
    }
    w
}

pub fn ingest_folder(args: &Value, owner: &str) -> Result<Value, Value> {
    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| err("'path' is required (a folder)"))?;
    let dry_run = dry_run_flag(args)?;
    let (start, oc, warnings) = resolve_input(owner, path)?;
    if !start.is_dir() {
        return Err(err("'path' must be a folder (use self_ingest_csv or self_ingest_kore for files)"));
    }
    let red = Redactor::load(&oc);
    let w = walk(&start);
    let reserved = [oc.join(ROOTS_FILE), oc.join(PATTERN_FILE)];
    let mut tally = Tally::default();
    let mut events: Vec<NewEvent> = Vec::new();
    let mut undated: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for f in &w.files {
        let rel = rel_of(&start, f);
        if let Ok(c) = f.canonicalize() {
            if reserved.contains(&c) {
                continue;
            }
        }
        scanned += 1;
        match std::fs::metadata(f) {
            Ok(m) if m.len() > MAX_FILE_BYTES => {
                tally.error(format!("{rel}: larger than {} KiB, skipped", MAX_FILE_BYTES / 1024));
                continue;
            }
            Err(e) => {
                tally.error(format!("{rel}: {e}"));
                continue;
            }
            _ => {}
        }
        let bytes = match std::fs::read(f) {
            Ok(b) => b,
            Err(e) => {
                tally.error(format!("{rel}: {e}"));
                continue;
            }
        };
        let content = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                tally.error(format!("{rel}: not valid UTF-8, skipped"));
                continue;
            }
        };
        let file_name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let date = match journal_date(&content, &file_name) {
            Some(d) => d,
            None => {
                undated.push(rel);
                continue;
            }
        };
        if content.trim().is_empty() {
            tally.error(format!("{rel}: empty file, skipped"));
            continue;
        }
        let source = format!("journal:{rel}");
        match prepare(&red, &mut tally, &source, &date, "note", &content, vec!["journal".to_string()], false) {
            Ok(e) => events.push(e),
            Err(m) => tally.error(format!("{rel}: {m}")),
        }
    }
    undated.sort();
    let mut log = open(owner)?;
    let version = commit_events(&mut log, &mut tally, events, dry_run)?;
    let mut out = json!({
        "dry_run": dry_run,
        "folder": disp(&start),
        "files_scanned": scanned,
        "added": tally.added,
        "duplicate": tally.duplicate,
        "skipped_undated": undated.len(),
        "undated_files": undated.iter().take(MAX_LISTED_PATHS).collect::<Vec<_>>(),
        "redacted": tally.redacted,
        "errors": tally.errors,
        "error_details": tally.error_details,
        "suppressed_forgotten": tally.suppressed,
        "skipped_symlinks": w.skipped_symlinks.len(),
        "truncated_at_file_cap": w.truncated,
        "version": version,
    });
    if dry_run {
        out["would_add"] = json!(tally.would_add);
        out["note"] = json!("dry run: nothing was written; run again without dry_run to store the events");
    }
    if !undated.is_empty() {
        out["undated_note"] = json!("undated files were skipped, not stamped with today's date; add a date: line, a dated heading or a YYYY-MM-DD file name prefix");
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    Ok(text(out))
}

// ─── self_ingest_csv ─────────────────────────────────────────────────────────

fn col_index(header: Option<&[String]>, ncols: usize, key: &str, v: &Value) -> Result<usize, String> {
    match header {
        Some(h) => {
            let name = v.as_str().ok_or_else(|| format!("mapping.{key} must be a column name (string)"))?;
            if let Some(i) = h.iter().position(|c| c == name) {
                return Ok(i);
            }
            if let Some(i) = h.iter().position(|c| c.trim().eq_ignore_ascii_case(name.trim())) {
                return Ok(i);
            }
            Err(format!("mapping.{key}: no column named '{name}' (columns: {})", h.join(", ")))
        }
        None => {
            let i = match v {
                Value::Number(n) => n.as_u64(),
                Value::String(s) => s.trim().parse::<u64>().ok(),
                _ => None,
            }
            .ok_or_else(|| format!("mapping.{key} must be a 0-based column number when has_header is false"))? as usize;
            if i >= ncols {
                return Err(format!("mapping.{key}: column {i} does not exist (file has {ncols} columns)"));
            }
            Ok(i)
        }
    }
}

fn csv_ts(s: &str) -> Option<String> {
    let t = s.trim();
    if let Ok(n) = vl::normalize_ts(t) {
        return Some(n);
    }
    // "YYYY-MM-DD HH:MM:SS" is common in exports
    if t.len() == 19 && t.as_bytes()[10] == b' ' {
        return vl::normalize_ts(&format!("{}T{}", &t[..10], &t[11..])).ok();
    }
    None
}

fn split_tags(s: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for t in s.split(|c| c == ',' || c == ';') {
        let t = t.trim();
        if !t.is_empty() && !v.iter().any(|x| x == t) {
            v.push(t.to_string());
        }
    }
    v
}

pub fn ingest_csv(args: &Value, owner: &str) -> Result<Value, Value> {
    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| err("'path' is required (a .csv file)"))?;
    let mapping = args.get("mapping").and_then(|v| v.as_object()).ok_or_else(|| err("'mapping' is required, e.g. {\"occurred_at\": \"date\", \"text\": \"note\"}"))?;
    let has_header = match args.get("has_header") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(err("'has_header' must be true or false")),
    };
    let dry_run = dry_run_flag(args)?;
    for k in mapping.keys() {
        if !["occurred_at", "text", "source", "kind", "tags"].contains(&k.as_str()) {
            return Err(err(format!("unknown mapping key '{k}' (allowed: occurred_at, text, source, kind, tags)")));
        }
    }
    for k in ["occurred_at", "text"] {
        if !mapping.contains_key(k) {
            return Err(err(format!("mapping.{k} is required (a row without a date is rejected, never dated now)")));
        }
    }
    let (file, oc, warnings) = resolve_input(owner, path)?;
    if !file.is_file() {
        return Err(err("'path' must be a file"));
    }
    let size = std::fs::metadata(&file).map_err(|e| err(e.to_string()))?.len();
    if size > MAX_CSV_BYTES {
        return Err(err(format!("file is {size} bytes; the limit is {MAX_CSV_BYTES}")));
    }
    let content = String::from_utf8(std::fs::read(&file).map_err(|e| err(e.to_string()))?).map_err(|_| err("file is not valid UTF-8"))?;
    let mut records = crate::csvio::parse(&content).map_err(|e| err(format!("CSV parse error: {e}")))?;
    if records.is_empty() {
        return Err(err("the CSV file has no rows"));
    }
    let header: Option<Vec<String>> = if has_header { Some(records.remove(0)) } else { None };
    let ncols = header.as_ref().map(|h| h.len()).unwrap_or_else(|| records.first().map(|r| r.len()).unwrap_or(0));
    let idx = |k: &str| -> Result<Option<usize>, Value> {
        match mapping.get(k) {
            Some(v) => col_index(header.as_deref(), ncols, k, v).map(Some).map_err(err),
            None => Ok(None),
        }
    };
    let (c_at, c_text) = (idx("occurred_at")?.expect("required"), idx("text")?.expect("required"));
    let (c_src, c_kind, c_tags) = (idx("source")?, idx("kind")?, idx("tags")?);
    let default_source = format!("csv:{}", file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into()));
    let red = Redactor::load(&oc);
    let truncated = records.len() > MAX_CSV_ROWS;
    records.truncate(MAX_CSV_ROWS);
    let total_rows = records.len();
    let mut tally = Tally::default();
    let mut rejected_no_date = 0usize;
    let mut events = Vec::new();
    for (n, row) in records.iter().enumerate() {
        let rn = n + 1;
        if row.len() != ncols {
            tally.error(format!("row {rn}: expected {ncols} fields, found {}", row.len()));
            continue;
        }
        let at = match csv_ts(&row[c_at]) {
            Some(t) => t,
            None => {
                rejected_no_date += 1;
                if tally.error_details.len() < MAX_LISTED {
                    tally.error_details.push(format!("row {rn}: occurred_at '{}' is missing or not a date (YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ); row rejected", safety::trunc(row[c_at].trim(), 40)));
                }
                continue;
            }
        };
        let source = c_src.map(|i| row[i].trim()).filter(|s| !s.is_empty()).unwrap_or(&default_source).to_string();
        let kind = c_kind.map(|i| row[i].trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "note".into());
        let tags = c_tags.map(|i| split_tags(&row[i])).unwrap_or_default();
        match prepare(&red, &mut tally, &source, &at, &kind, &row[c_text], tags, false) {
            Ok(e) => events.push(e),
            Err(m) => tally.error(format!("row {rn}: {m}")),
        }
    }
    let mut log = open(owner)?;
    let version = commit_events(&mut log, &mut tally, events, dry_run)?;
    let mut out = json!({
        "dry_run": dry_run,
        "rows": total_rows,
        "added": tally.added,
        "duplicate": tally.duplicate,
        "rejected_no_date": rejected_no_date,
        "redacted": tally.redacted,
        "errors": tally.errors,
        "error_details": tally.error_details,
        "suppressed_forgotten": tally.suppressed,
        "truncated_at_row_cap": truncated,
        "version": version,
    });
    if dry_run {
        out["would_add"] = json!(tally.would_add);
        out["note"] = json!("dry run: nothing was written");
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    Ok(text(out))
}

// ─── self_ingest_kore ────────────────────────────────────────────────────────

pub fn ingest_kore(args: &Value, owner: &str) -> Result<Value, Value> {
    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| err("'path' is required (a .kore file written by self_export)"))?;
    let dry_run = dry_run_flag(args)?;
    let (file, oc, warnings) = resolve_input(owner, path)?;
    if !file.is_file() {
        return Err(err("'path' must be a file"));
    }
    let size = std::fs::metadata(&file).map_err(|e| err(e.to_string()))?.len();
    if size > MAX_KORE_BYTES {
        return Err(err(format!("file is {size} bytes; the limit is {MAX_KORE_BYTES}")));
    }
    let bytes = std::fs::read(&file).map_err(|e| err(e.to_string()))?;
    let block = kore_store::KoreReader::from_bytes(&bytes).map_err(|e| err(format!("not a readable .kore file: {e}")))?;
    let rows = vl::block_to_rows(Table::Events, &block).map_err(|e| err(format!("not an events export written by self_export: {e}")))?;
    let truncated = rows.len() > MAX_KORE_ROWS;
    let red = Redactor::load(&oc);
    let mut tally = Tally::default();
    let mut events = Vec::new();
    for (n, cells) in rows.iter().take(MAX_KORE_ROWS).enumerate() {
        let r = EventRow::from_cells(cells);
        let at = match vl::normalize_ts(&r.occurred_at) {
            Ok(t) => t,
            Err(m) => {
                tally.error(format!("row {}: {m}", n + 1));
                continue;
            }
        };
        if vl::event_id(&r.source, &at, &r.text) != r.event_id {
            tally.error(format!("row {}: event_id does not match its content (edited or corrupt); row rejected", n + 1));
            continue;
        }
        let tags = if r.tags.is_empty() { vec![] } else { r.tags.split(',').map(|t| t.to_string()).collect() };
        match prepare(&red, &mut tally, &r.source, &at, &r.kind, &r.text, tags, r.redacted) {
            Ok(e) => events.push(e),
            Err(m) => tally.error(format!("row {}: {m}", n + 1)),
        }
    }
    let total = rows.len().min(MAX_KORE_ROWS);
    let mut log = open(owner)?;
    let version = commit_events(&mut log, &mut tally, events, dry_run)?;
    let mut out = json!({
        "dry_run": dry_run,
        "rows": total,
        "added": tally.added,
        "duplicate": tally.duplicate,
        "redacted": tally.redacted,
        "errors": tally.errors,
        "error_details": tally.error_details,
        "suppressed_forgotten": tally.suppressed,
        "truncated_at_row_cap": truncated,
        "version": version,
    });
    if dry_run {
        out["would_add"] = json!(tally.would_add);
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    Ok(text(out))
}

// ─── self_export ─────────────────────────────────────────────────────────────

fn export_target(owner: &str, arg: &str) -> Result<(PathBuf, PathBuf), Value> {
    let p = arg.trim();
    if p.is_empty() {
        return Err(err("'path' must not be empty"));
    }
    if p.contains('\0') {
        return Err(err("path contains NUL"));
    }
    let oc = owner_canon(owner)?;
    let exports = oc.join("exports");
    safety::create_private_dir(&exports).map_err(|e| err(format!("cannot create exports dir: {e}")))?;
    let exports = exports.canonicalize().map_err(|e| err(format!("cannot resolve exports dir: {e}")))?;
    let raw = Path::new(p);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        let rel = p.strip_prefix("exports/").or_else(|| p.strip_prefix("exports\\")).unwrap_or(p);
        exports.join(rel)
    };
    let resolved = safety::safe_path(owner, &joined.to_string_lossy()).map_err(|e| err(format!("Rejected path '{}': {e}", safety::trunc(p, 120))))?;
    if !resolved.starts_with(&exports) || resolved == exports {
        return Err(err(format!("Rejected path '{}': exports are written only under {}", safety::trunc(p, 120), disp(&exports))));
    }
    Ok((resolved, exports))
}

pub const EXPORT_CSV_HEADER: [&str; 8] = ["event_id", "occurred_at", "source", "kind", "text", "tags", "redacted", "recorded_at"];

pub fn events_to_csv(rows: &[EventRow]) -> String {
    let mut s = String::new();
    crate::csvio::write_record(&mut s, &EXPORT_CSV_HEADER);
    for r in rows {
        crate::csvio::write_record(&mut s, &[r.event_id.as_str(), &r.occurred_at, &r.source, &r.kind, &r.text, &r.tags, if r.redacted { "true" } else { "false" }, &r.recorded_at]);
    }
    s
}

pub fn export(args: &Value, owner: &str) -> Result<Value, Value> {
    let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| err("'path' is required (a file name under exports/)"))?;
    let format = args.get("format").and_then(|v| v.as_str()).ok_or_else(|| err("'format' is required: csv or kore"))?;
    if format != "csv" && format != "kore" {
        return Err(err("'format' must be csv or kore"));
    }
    let (target, exports) = export_target(owner, path)?;
    let log = open(owner)?;
    let snap = log.open_latest();
    let rows: Vec<EventRow> = snap.rows();
    let bytes: Vec<u8> = if format == "csv" {
        events_to_csv(&rows).into_bytes()
    } else {
        kore_store::KoreWriter::to_bytes(&snap.block(Table::Events))
    };
    if let Some(parent) = target.parent() {
        safety::create_private_dir(parent).map_err(|e| err(format!("cannot create folder: {e}")))?;
    }
    // temp file + rename: a crash never leaves a half-written export under the final name
    let mut tmp_name = target.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp = target.with_file_name(tmp_name);
    safety::write_private(&tmp, &bytes).map_err(|e| err(format!("write failed: {e}")))?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        err(format!("write failed: {e}"))
    })?;
    Ok(text(json!({
        "path": disp(&target),
        "relative": rel_of(&exports, &target),
        "format": format,
        "events": rows.len(),
        "bytes": bytes.len(),
        "sha256": vl::sha256_hex(&[&bytes]),
        "version": snap.version,
        "note": "events only (beliefs, predictions and outcomes are not exported); the file is not encrypted",
    })))
}

// ─── self_forget ─────────────────────────────────────────────────────────────

pub fn forget(args: &Value, owner: &str) -> Result<Value, Value> {
    let id = args.get("event_id").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let prefix = args.get("source_prefix").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let confirm = args.get("confirm").and_then(|v| v.as_bool()) == Some(true);
    if args.get("event_id").is_some() && id.is_none() || args.get("source_prefix").is_some() && prefix.is_none() {
        return Err(err("'event_id' and 'source_prefix' must be non-empty strings when given"));
    }
    let mut log = open(owner)?;
    let ids: Vec<String> = match (id, prefix) {
        (Some(_), Some(_)) => return Err(err("pass exactly one of event_id or source_prefix")),
        (None, None) => return Err(err("pass event_id or source_prefix (and confirm: true)")),
        (Some(i), None) => vec![i.to_string()],
        (None, Some(p)) => {
            let v = log.event_ids_by_source_prefix(p);
            if v.is_empty() {
                return Err(err(format!("no stored event has a source starting with '{}'; nothing to forget", safety::trunc(p, 80))));
            }
            v
        }
    };
    if !confirm {
        return Err(err(format!(
            "self_forget permanently removes {} event(s) from every version of the log (the one exception to append-only) and cannot be undone. Call again with confirm: true to proceed.",
            ids.len()
        )));
    }
    let r = log.forget_events(&ids).map_err(log_err)?;
    let mut out = json!({
        "forgotten": r.forgotten,
        "already_forgotten": r.already_forgotten,
        "causes_dropped": r.causes_dropped,
        "revisions_now_unsupported": r.revisions_orphaned,
        "segments_rewritten": r.segments_rewritten,
        "segments_removed": r.segments_removed,
        "predictions_edited": r.predictions_edited,
        "outcomes_edited": r.outcomes_edited,
        "version": r.version,
        "note": "removed from all versions (older versions no longer contain it); manifest hashes of rewritten versions changed and a forget record (event-id hashes only) was added. Copies outside the log (backups, exports, other disks) are not touched.",
    });
    if !r.created {
        out["note"] = json!("nothing was removed: every id was already forgotten");
    }
    Ok(text(out))
}

// ─── tool definitions ────────────────────────────────────────────────────────

pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({ "name": "self_ingest_folder",
          "description": "Read .md and .txt files under an allow-listed folder and store one dated note event per file (idempotent, redacted before storing). Undated files are skipped and listed, never dated now",
          "inputSchema": { "type": "object", "properties": {
            "path": { "type": "string", "description": "Folder inside your owner directory or inside a folder listed in the hand-edited ingest_roots file. Each file is dated by front-matter date:, the first heading containing an ISO date, or a YYYY-MM-DD file name prefix. Limits: 2000 files, 512 KiB per file." },
            "dry_run": { "type": "boolean", "description": "Report counts without writing anything" }
          }, "required": ["path"] } }),
        json!({ "name": "self_ingest_csv",
          "description": "Import events from a CSV file inside an allow-listed folder; rows without a parseable occurred_at are rejected and counted, never dated now",
          "inputSchema": { "type": "object", "properties": {
            "path": { "type": "string", "description": "CSV file inside an allowed ingest root (see self_ingest_folder). Limits: 16 MiB, 50000 rows." },
            "mapping": { "type": "object", "description": "Columns to use: occurred_at and text are required; source, kind and tags are optional. Values are header names, or 0-based column numbers when has_header is false.",
              "properties": { "occurred_at": {}, "text": {}, "source": {}, "kind": {}, "tags": {} }, "required": ["occurred_at", "text"] },
            "has_header": { "type": "boolean", "description": "Default true" },
            "dry_run": { "type": "boolean" }
          }, "required": ["path", "mapping"] } }),
        json!({ "name": "self_ingest_kore",
          "description": "Import events from a .kore file written by self_export (idempotent, validated, redacted again before storing)",
          "inputSchema": { "type": "object", "properties": {
            "path": { "type": "string", "description": "A .kore events export inside an allowed ingest root (exports/ qualifies)." },
            "dry_run": { "type": "boolean" }
          }, "required": ["path"] } }),
        json!({ "name": "self_forget",
          "description": "Permanently remove events from every version of the log, the one documented exception to append-only; requires confirm true",
          "inputSchema": { "type": "object", "properties": {
            "event_id": { "type": "string", "description": "Exact event id to forget" },
            "source_prefix": { "type": "string", "description": "Forget every event whose source starts with this text, for example journal:2019/" },
            "confirm": { "type": "boolean", "description": "Must be true; without it nothing is removed" }
          }, "required": ["confirm"] } }),
        json!({ "name": "self_export",
          "description": "Write all events to a CSV or .kore file under the exports folder of your data directory",
          "inputSchema": { "type": "object", "properties": {
            "path": { "type": "string", "description": "File name or relative path inside exports/" },
            "format": { "type": "string", "enum": ["csv", "kore"] }
          }, "required": ["path", "format"] } }),
    ]
}

#[cfg(test)]
#[path = "ingest_tools_tests.rs"]
mod tests;
