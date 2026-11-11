//! Unattended-run scheduler keys of `controller.txt` (pure: parsing only; the tool layer
//! `world_t_sched.rs` reads the file and runs the chunks through the World T gate).
//!
//! ```text
//! sched_run              = on       # may the scheduler run World T chunks unattended (default off)
//! sched_chunk_steps      = 1000     # steps asked per chunk (1..=5000; the gate still cuts to max_steps_per_run)
//! sched_max_chunks       = 100      # chunks per scheduler run (1..=1000000)
//! sched_max_steps_total  = 100000   # steps per scheduler run (1..=1000000000)
//! sched_max_energy_total = 10000000 # energy units per scheduler run (1..=1000000000000)
//! sched_min_free_disk_mb = 4096     # stop when free disk of the owner directory is below this (1024..=1048576)
//! sched_interval_ms      = 0        # pause between chunks (0..=3600000; KILL is checked during the pause)
//! ```
//!
//! Fail closed: a missing or invalid file, or `sched_run` absent or off, means no scheduler run. A
//! value outside its range makes the whole controller invalid (`Controller::parse` calls `validate_key`).

pub const KEY_RUN: &str = "sched_run";
pub const KEY_CHUNK: &str = "sched_chunk_steps";
pub const KEY_CHUNKS: &str = "sched_max_chunks";
pub const KEY_STEPS: &str = "sched_max_steps_total";
pub const KEY_ENERGY: &str = "sched_max_energy_total";
pub const KEY_DISK: &str = "sched_min_free_disk_mb";
pub const KEY_INTERVAL: &str = "sched_interval_ms";

/// (key, min, max, default) of the numeric keys.
pub const RANGES: [(&str, u64, u64, u64); 6] = [
    (KEY_CHUNK, 1, 5000, 1000),
    (KEY_CHUNKS, 1, 1_000_000, 100),
    (KEY_STEPS, 1, 1_000_000_000, 100_000),
    (KEY_ENERGY, 1, 1_000_000_000_000, 10_000_000),
    (KEY_DISK, 1024, 1_048_576, 4096),
    (KEY_INTERVAL, 0, 3_600_000, 0),
];

#[derive(Debug, Clone, PartialEq)]
pub struct SchedCfg {
    pub run: bool,
    pub chunk_steps: u64,
    pub max_chunks: u64,
    pub max_steps_total: u64,
    pub max_energy_total: u64,
    pub min_free_disk_mb: u64,
    pub interval_ms: u64,
}

impl Default for SchedCfg {
    fn default() -> SchedCfg {
        let d = |k: &str| RANGES.iter().find(|r| r.0 == k).map(|r| r.3).unwrap_or(0);
        SchedCfg {
            run: false,
            chunk_steps: d(KEY_CHUNK),
            max_chunks: d(KEY_CHUNKS),
            max_steps_total: d(KEY_STEPS),
            max_energy_total: d(KEY_ENERGY),
            min_free_disk_mb: d(KEY_DISK),
            interval_ms: d(KEY_INTERVAL),
        }
    }
}

pub fn is_key(k: &str) -> bool {
    k == KEY_RUN || RANGES.iter().any(|r| r.0 == k)
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn num(k: &str, v: &str) -> Result<u64, String> {
    let (_, lo, hi, _) = RANGES.iter().find(|r| r.0 == k).copied().ok_or_else(|| format!("unknown key '{k}'"))?;
    let x: u64 = v.parse().map_err(|_| format!("'{k}' needs a non-negative integer"))?;
    if !(lo..=hi).contains(&x) {
        return Err(format!("'{k}' must be {lo}..={hi}"));
    }
    Ok(x)
}

/// Check the value (called by `Controller::parse`; an error makes the whole file invalid).
pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    if k == KEY_RUN {
        return parse_bool(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off"));
    }
    num(k, v).map(|_| ())
}

/// The scheduler settings of a controller text. Err: the file is missing or invalid (fail closed).
/// A valid file without the keys gives `run = false` and the defaults.
pub fn from_text(text: Option<&str>) -> Result<SchedCfg, String> {
    let text = text.ok_or_else(|| "controller.txt is missing".to_string())?;
    super::controller::Controller::parse(text).map_err(|e| format!("controller.txt is not valid: {e}"))?;
    let mut c = SchedCfg::default();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            KEY_RUN => c.run = parse_bool(v).unwrap_or(false),
            KEY_CHUNK => c.chunk_steps = num(k, v)?,
            KEY_CHUNKS => c.max_chunks = num(k, v)?,
            KEY_STEPS => c.max_steps_total = num(k, v)?,
            KEY_ENERGY => c.max_energy_total = num(k, v)?,
            KEY_DISK => c.min_free_disk_mb = num(k, v)?,
            KEY_INTERVAL => c.interval_ms = num(k, v)?,
            _ => {}
        }
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::being::controller::Controller;

    #[test]
    fn sched_keys_default_off_and_out_of_range_values_make_the_controller_invalid() {
        assert!(from_text(None).is_err());
        let c = from_text(Some("max_steps_per_run = 40\n")).unwrap();
        assert!(!c.run);
        assert_eq!(c, SchedCfg::default());
        assert_eq!(c.min_free_disk_mb, 4096);
        let c = from_text(Some("sched_run = on\nsched_max_chunks = 3\nsched_interval_ms = 0\n")).unwrap();
        assert!(c.run && c.max_chunks == 3);
        for bad in ["sched_run = maybe", "sched_max_chunks = 0", "sched_chunk_steps = 5001", "sched_min_free_disk_mb = 10", "sched_interval_ms = -1", "sched_max_energy_total = lots", "sched_max_steps_total = 0"] {
            assert!(Controller::parse(bad).is_err(), "{bad}");
            assert!(Controller::parse_or_closed(bad).is_closed());
            assert!(from_text(Some(bad)).is_err(), "{bad}");
        }
        assert!(Controller::parse("sched_run = on\nsched_run = on\n").is_err());
    }
}
