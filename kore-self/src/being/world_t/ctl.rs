//! The World T key of `controller.txt` (pure: parsing only; the tool layer reads the file).
//!
//! `world_t_run = on` allows World T runs; anything else, a missing key, a missing or unparseable file:
//! no run (fail closed). The run is in addition asked through `authorize()` as the internal tool
//! `world_t_run` (tier L2: it appends to the being's own log and reads the sandbox), default deny.

use super::tools::{SqlLimits, HARD_SQL_MAX_ROWS, HARD_SQL_TIMEOUT_MS};

pub const KEY: &str = "world_t_run";
/// Timeout of one query in milliseconds (default 2000, hard maximum 60000; 0 = no query may run).
pub const KEY_SQL_TIMEOUT: &str = "world_t_sql_timeout_ms";
/// Largest result of one query in rows (default 1000, hard maximum 1000000).
pub const KEY_SQL_ROWS: &str = "world_t_sql_max_rows";
/// Track W2: the file-keyed estimate for a NEW World T being (on / off; default off). Recorded at the
/// being's genesis; an existing being keeps its setting and a run asking for the other one is refused.
pub const KEY_FILE_KEY: &str = "wt_file_key";

pub fn is_key(k: &str) -> bool {
    k == KEY || k == KEY_SQL_TIMEOUT || k == KEY_SQL_ROWS || k == KEY_FILE_KEY
}

fn num(k: &str, v: &str, hard: u64) -> Result<u64, String> {
    let x: u64 = v.parse().map_err(|_| format!("'{k}' needs a non-negative integer"))?;
    if x > hard {
        return Err(format!("'{k}' is above the hard maximum {hard}"));
    }
    Ok(x)
}

pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    match k {
        KEY_SQL_TIMEOUT => num(k, v, HARD_SQL_TIMEOUT_MS).map(|_| ()),
        KEY_SQL_ROWS => num(k, v, HARD_SQL_MAX_ROWS as u64).map(|_| ()),
        _ => parse_bool(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off")),
    }
}

/// The query limits of a controller text. Fail closed: a missing or invalid file gives limits under which
/// no query runs (timeout 0, 0 rows); a valid file without the keys gives the defaults (2000 ms, 1000 rows).
pub fn sql_limits(text: Option<&str>) -> SqlLimits {
    let Some(text) = text else { return SqlLimits::closed() };
    if crate::being::controller::Controller::parse(text).is_err() {
        return SqlLimits::closed();
    }
    let mut l = SqlLimits::default();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            let (k, v) = (k.trim(), v.trim());
            if k == KEY_SQL_TIMEOUT {
                l.timeout_ms = num(k, v, HARD_SQL_TIMEOUT_MS).unwrap_or(0);
            } else if k == KEY_SQL_ROWS {
                l.max_rows = num(k, v, HARD_SQL_MAX_ROWS as u64).unwrap_or(0) as usize;
            }
        }
    }
    l
}


/// The `wt_file_key` request of a controller text: Some(on/off) when the key is set in a valid file; None
/// when the file is missing or invalid or the key is absent (fail closed: no request, so a new being is
/// created with it off; an invalid file also refuses the run itself, `allowed`).
pub fn file_key(text: Option<&str>) -> Option<bool> {
    let text = text?;
    crate::being::controller::Controller::parse(text).ok()?;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == KEY_FILE_KEY {
                return parse_bool(v.trim());
            }
        }
    }
    None
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Is World T allowed by this controller text? (None: the file is missing.) Returns Err(reason) when
/// not allowed; the whole file must also parse as a controller (`Controller::parse`).
pub fn allowed(text: Option<&str>) -> Result<(), String> {
    let Some(text) = text else { return Err("controller.txt is missing".into()) };
    crate::being::controller::Controller::parse(text).map_err(|e| format!("controller.txt is not valid: {e}"))?;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == KEY {
                return match parse_bool(v.trim()) {
                    Some(true) => Ok(()),
                    _ => Err(format!("{KEY} = off in controller.txt")),
                };
            }
        }
    }
    Err(format!("controller.txt has no line {KEY} = on (World T is off by default)"))
}
