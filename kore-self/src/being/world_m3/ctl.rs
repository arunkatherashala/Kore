//! The World M3 key of `controller.txt` (pure: parsing only; the tool layer reads the file).
//!
//! `world_m3_run = on` allows World M3 runs; anything else, a missing key, a missing or unparseable file: no
//! run (fail closed). The run is in addition asked through `authorize()` as the internal tool `world_m3_run`
//! (tier L2), default deny. Query limits are World T's keys (`world_t_sql_timeout_ms`, `world_t_sql_max_rows`).

pub const KEY: &str = "world_m3_run";

pub fn is_key(k: &str) -> bool {
    k == KEY
}

pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" | "off" | "false" | "no" | "0" => Ok(()),
        _ => Err(format!("'{k}' needs on or off")),
    }
}

/// Is World M3 allowed by this controller text? (None: the file is missing.)
pub fn allowed(text: Option<&str>) -> Result<(), String> {
    let Some(text) = text else { return Err("controller.txt is missing".into()) };
    crate::being::controller::Controller::parse(text).map_err(|e| format!("controller.txt is not valid: {e}"))?;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == KEY {
                return match v.trim().to_ascii_lowercase().as_str() {
                    "on" | "true" | "yes" | "1" => Ok(()),
                    _ => Err(format!("{KEY} = off in controller.txt")),
                };
            }
        }
    }
    Err(format!("controller.txt has no line {KEY} = on (World M3 is off by default)"))
}
