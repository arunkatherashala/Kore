//! The owner's switches for planning ahead (Track B8), read from the same hand-written `controller.txt`
//! as the B5 caps. Three keys, all optional:
//!
//! ```text
//! lookahead            = on      # may the planner look several steps ahead (default off)
//! max_lookahead_nodes  = 20000   # simulated (state, action) evaluations per step (hard maximum 2000000)
//! max_lookahead_depth  = 3       # steps simulated ahead, the current one included (hard maximum 6)
//! ```
//!
//! * Missing keys take the safe defaults above (lookahead off). A value above the hard maximum or a
//!   value that does not parse makes the whole file unparseable (`controller::Controller::parse` calls
//!   `validate_key` for these keys), so the B5 rule applies: missing or unparseable file = fail closed.
//! * Fail closed means lookahead off, 0 nodes, depth 0: the being plans with the one-step B1 planner.
//! * Asking for lookahead when the file does not allow it is a refusal that is logged (`requested`).
//! * A search that reaches the node cap stops at once; the deepest depth that was completed inside the
//!   cap is used, and the cut is logged as a `max_lookahead_nodes` refusal (once per run).
//!
//! Pure: no file access here (the tool layer or the caller hands over the text).

use crate::being::controller::{Controller, Refusal};

pub const KEY_ON: &str = "lookahead";
pub const KEY_NODES: &str = "max_lookahead_nodes";
pub const KEY_DEPTH: &str = "max_lookahead_depth";
pub const DEFAULT_NODES: u64 = 20_000;
pub const DEFAULT_DEPTH: u32 = 3;
pub const HARD_NODES: u64 = 2_000_000;
pub const HARD_DEPTH: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookCtl {
    pub allowed: bool,
    pub max_nodes: u64,
    pub max_depth: u32,
    /// None: from a valid file; Some(reason): failed closed.
    pub closed: Option<String>,
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Is `k` one of the B8 keys?
pub fn is_key(k: &str) -> bool {
    k == KEY_ON || k == KEY_NODES || k == KEY_DEPTH
}

/// Check one B8 key's value (called by `Controller::parse`, so a bad value makes the whole file invalid).
pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    match k {
        KEY_ON => parse_bool(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off")),
        KEY_NODES => match v.parse::<u64>() {
            Ok(x) if x <= HARD_NODES => Ok(()),
            Ok(_) => Err(format!("'{k}' is above the hard maximum {HARD_NODES}")),
            Err(_) => Err(format!("'{k}' needs a non-negative integer")),
        },
        KEY_DEPTH => match v.parse::<u32>() {
            Ok(x) if x <= HARD_DEPTH => Ok(()),
            Ok(_) => Err(format!("'{k}' is above the hard maximum {HARD_DEPTH}")),
            Err(_) => Err(format!("'{k}' needs a non-negative integer")),
        },
        _ => Err(format!("unknown key '{k}'")),
    }
}

impl LookCtl {
    pub fn fail_closed(reason: impl Into<String>) -> LookCtl {
        LookCtl { allowed: false, max_nodes: 0, max_depth: 0, closed: Some(reason.into()) }
    }

    pub fn defaults() -> LookCtl {
        LookCtl { allowed: false, max_nodes: DEFAULT_NODES, max_depth: DEFAULT_DEPTH, closed: None }
    }

    /// From the text of `controller.txt` (None: the file is missing). The whole file must parse as a
    /// controller file (B5 rules); otherwise fail closed.
    pub fn from_text(text: Option<&str>) -> LookCtl {
        let Some(text) = text else { return LookCtl::fail_closed("controller.txt is missing") };
        if let Err(e) = Controller::parse(text) {
            return LookCtl::fail_closed(format!("controller.txt is not valid: {e}"));
        }
        let mut c = LookCtl::defaults();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                KEY_ON => c.allowed = parse_bool(v).unwrap_or(false),
                KEY_NODES => c.max_nodes = v.parse().unwrap_or(0),
                KEY_DEPTH => c.max_depth = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        c
    }

    /// What one run may use, given what the caller asked for. A request the file does not allow is
    /// a refusal (returned for the log) and the run plans one step at a time.
    pub fn grant(&self, want: bool) -> (Option<(u64, u32)>, Vec<Refusal>) {
        if !want {
            return (None, vec![]);
        }
        if !self.allowed || self.max_nodes == 0 || self.max_depth == 0 {
            let why = match &self.closed {
                Some(r) => format!("planning ahead was requested but the controller failed closed ({r}); the one-step planner is used"),
                None => "planning ahead was requested but controller.txt does not allow it (lookahead = off, or a zero node or depth cap); the one-step planner is used".into(),
            };
            return (None, vec![Refusal::new(KEY_ON, 0, 1, why)]);
        }
        (Some((self.max_nodes, self.max_depth)), vec![])
    }
}

/// The refusal logged when a search reached the node cap (same text every time, so it is logged once per run).
pub fn node_cap_refusal(cap: u64, wanted_depth: u32) -> Refusal {
    Refusal::new(KEY_NODES, cap, cap + 1, format!("a lookahead search reached the cap of {cap} simulated evaluations per step before finishing depth {wanted_depth}; the deepest completed depth was used"))
}
