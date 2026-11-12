//! Track D1c controller key: `fast_open = on` in `controller.txt` lets the log tools open a format-5
//! log through its index runs (`versioned::index_runs`). Fail closed, as `checkpoint_every`: missing
//! file, unparseable file, missing key or any value but `on` = off = the full open as before D1c.

pub const KEY: &str = "fast_open";

pub fn is_key(k: &str) -> bool {
    k == KEY
}

/// Check the value (called by `Controller::parse`; an error makes the whole file invalid).
pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    match v {
        "on" | "off" => Ok(()),
        _ => Err(format!("'{k}' needs on or off")),
    }
}

/// True only if the text parses as a whole and sets `fast_open = on`.
pub fn on_from_text(text: Option<&str>) -> bool {
    let Some(text) = text else { return false };
    if super::controller::Controller::parse(text).is_err() {
        return false;
    }
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == KEY {
                return v.trim() == "on";
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rules_fail_closed() {
        assert!(!on_from_text(None), "missing file = off");
        assert!(!on_from_text(Some("")), "no key = off");
        assert!(!on_from_text(Some("fast_open = off\n")));
        assert!(!on_from_text(Some("fast_open = yes\n")), "other value = invalid file = off");
        assert!(!on_from_text(Some("fast_open = ON\n")));
        assert!(!on_from_text(Some("fast_open = on\nbogus_key = 1\n")), "unparseable file = off");
        assert!(!on_from_text(Some("fast_open = on\nfast_open = on\n")), "key twice = invalid = off");
        assert!(on_from_text(Some("fast_open = on\n")));
        assert!(on_from_text(Some("# c\nmax_steps_per_run = 10\nfast_open = on # yes\n")));
        assert!(validate_key(KEY, "on").is_ok() && validate_key(KEY, "off").is_ok() && validate_key(KEY, "1").is_err());
    }
}
