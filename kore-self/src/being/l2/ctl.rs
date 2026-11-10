//! The Track L2 key of `controller.txt` (pure: parsing only; the tool layer reads the file).
//!
//! `l2_meta_prior = on` allows a NEW World O being to use the Track L2 meta-prior (learning to learn across
//! families). Anything else, a missing key, a missing or unparseable file: off (fail closed). Asking for it
//! when it is not allowed is a refusal (`Refusal`, cap `l2_meta_prior`), logged before the first step.

use crate::being::controller::Refusal;

pub const KEY: &str = "l2_meta_prior";

pub fn is_key(k: &str) -> bool {
    k == KEY
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Called by `Controller::parse` for this key: a bad value makes the whole file unparseable.
pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    parse_bool(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off"))
}

/// Is the L2 meta-prior allowed by this controller text? (None: the file is missing.) Fail closed: a missing
/// file, an unparseable file (any line), a missing key or `off` all mean no.
pub fn allowed(text: Option<&str>) -> bool {
    let Some(text) = text else { return false };
    if crate::being::controller::Controller::parse(text).is_err() {
        return false;
    }
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == KEY {
                return parse_bool(v.trim()) == Some(true);
            }
        }
    }
    false
}

/// The request check: `want` the meta-prior under controller text `text`. Ok(true): granted; Ok(false): not
/// asked; Err: asked but not allowed (the refusal to log, the being runs without it).
pub fn grant(want: bool, text: Option<&str>) -> Result<bool, Refusal> {
    if !want {
        return Ok(false);
    }
    if allowed(text) {
        return Ok(true);
    }
    Err(Refusal::new(KEY, 0, 1, "the Track L2 meta-prior was requested but controller.txt does not allow it (l2_meta_prior = off, missing, or the file is missing or not valid)"))
}
