//! The controller of the being (Track B5): hard caps, a kill switch, and refusals that are logged.
//!
//! This file holds the PURE part: parsing of the owner's `controller.txt`, the safe defaults, the caps
//! the loop must obey and the audit records of a refusal. It reads and writes nothing (the `being/`
//! sources have no file or network API, a test scans them). The tool layer (`being_tools.rs`) reads
//! `controller.txt` and the `KILL` file and hands the result to the loop.
//!
//! `controller.txt` is written by hand by the owner. No tool can read or write it (it is on the reserved
//! list of `safety.rs`, like `autonomy_policy.txt`, `thinker.txt`, `advisor.txt` and `KILL`), and the
//! being has no way to name a path at all. Format, one `key = value` per line, `#` starts a comment:
//!
//! ```text
//! max_live_rules        = 16      # rules applied at the same time
//! max_building_blocks   = 8       # verified forms the composition may use
//! max_search_candidates = 2000    # (transform, form) pairs tried per discovery pass
//! max_steps_per_run     = 2000    # steps of one being_run
//! max_energy_per_run    = 100000  # energy units spent by one being_run (a step that starts below the cap may end above it)
//! composition           = on      # may discovery compose verified blocks (default off)
//! pruning               = on      # may the being retire and merge its own rules (default off)
//! transfer              = on      # may the model start a new domain from a similar learned one (B6, default off)
//! max_transfer_links    = 16      # (domain, difficulty) links that may exist at the same time (B6)
//! experiments           = on      # may the being choose tasks on purpose to test its hypotheses (B9, default off)
//! max_experiment_energy_pm = 100  # experiments may spend at most this share (per mille) of the life budget (B9, default 0)
//! max_experiments_per_run  = 200  # experiments in one run (B9, default 0)
//! rules_use             = on      # World v2: may the learner use its rules (B7 switches; B10, default off)
//! adaptation            = on      # World v2: may the model adapt to changes (B3b; B10, default off)
//! rule_retirement       = on      # World v2: may the being retire rules its later answers contradict (B10, default off)
//! ```
//!
//! The B9 keys fail closed on their own: without `experiments = on` AND a non-zero energy share AND a
//! non-zero count, no experiment is run (a request for experiments is refused and audited).
//!
//! * A key that is missing takes the SAFE DEFAULT above (the numbers in the example, composition and
//!   pruning off). A value above the hard maximum, an unknown key, a repeated key or a line that is not
//!   `key = value` makes the whole file unparseable.
//! * A missing or unparseable file means the MOST RESTRICTIVE settings: every cap is 0, so no step is
//!   run, no rule is added, composition and pruning are off (fail closed). The run is refused and the
//!   refusal is logged.
//! * Exceeding a cap is never silent: the action is refused (or the run stops) and a `controller_refusal`
//!   event goes into the being's log; the tool layer also writes a row to the audit trail
//!   (source `authorize`).
//! * Tiers: the being has L1/L2 powers only (read the simulated world, append to its own log). Nothing in
//!   this file or the loop can do more: no network, no code writing, no external effect.

use super::state::SOURCE;
use super::world::iso_at;
use crate::versioned::NewEvent;

pub const FILE: &str = "controller.txt";

/// Safe defaults for a key that is missing from an otherwise valid file.
pub const DEFAULT_LIVE_RULES: usize = 16;
pub const DEFAULT_BLOCKS: usize = 8;
pub const DEFAULT_CANDIDATES: usize = 2000;
pub const DEFAULT_STEPS: u64 = 2000;
pub const DEFAULT_ENERGY: i64 = 100_000;
pub const DEFAULT_LINKS: usize = 16;
/// B9: experiments are off and have no budget unless the owner writes the lines.
pub const DEFAULT_EXP_PM: u32 = 0;
pub const DEFAULT_EXP_N: u64 = 0;
pub const HARD_EXP_PM: u32 = 500;
pub const HARD_EXP_N: u64 = 5000;
/// Hard maxima: a file that asks for more is unparseable.
pub const HARD_LIVE_RULES: usize = 1000;
pub const HARD_BLOCKS: usize = 100;
pub const HARD_CANDIDATES: usize = 1_000_000;
pub const HARD_STEPS: u64 = 5000;
pub const HARD_ENERGY: i64 = 1_000_000;
pub const HARD_LINKS: usize = 1000;

#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    File,
    /// Missing or unparseable file: the reason.
    FailClosed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
    pub max_live_rules: usize,
    pub max_building_blocks: usize,
    pub max_search_candidates: usize,
    pub max_steps_per_run: u64,
    pub max_energy_per_run: i64,
    pub composition: bool,
    pub pruning: bool,
    pub transfer: bool,
    pub max_transfer_links: usize,
    /// B9: experiments allowed, their energy share of the life budget (per mille) and count per run.
    pub experiments: bool,
    pub max_experiment_energy_pm: u32,
    pub max_experiments_per_run: u64,
    pub origin: Origin,
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

impl Controller {
    /// The most restrictive settings (used when the file is missing or unparseable).
    pub fn fail_closed(reason: impl Into<String>) -> Controller {
        Controller {
            max_live_rules: 0,
            max_building_blocks: 0,
            max_search_candidates: 0,
            max_steps_per_run: 0,
            max_energy_per_run: 0,
            composition: false,
            pruning: false,
            transfer: false,
            max_transfer_links: 0,
            experiments: false,
            max_experiment_energy_pm: 0,
            max_experiments_per_run: 0,
            origin: Origin::FailClosed(reason.into()),
        }
    }

    /// The safe defaults (what a valid empty file means).
    pub fn defaults() -> Controller {
        Controller {
            max_live_rules: DEFAULT_LIVE_RULES,
            max_building_blocks: DEFAULT_BLOCKS,
            max_search_candidates: DEFAULT_CANDIDATES,
            max_steps_per_run: DEFAULT_STEPS,
            max_energy_per_run: DEFAULT_ENERGY,
            composition: false,
            pruning: false,
            transfer: false,
            max_transfer_links: DEFAULT_LINKS,
            experiments: false,
            max_experiment_energy_pm: DEFAULT_EXP_PM,
            max_experiments_per_run: DEFAULT_EXP_N,
            origin: Origin::File,
        }
    }

    /// Parse the text of `controller.txt`. Any problem is an error (the caller then fails closed).
    pub fn parse(text: &str) -> Result<Controller, String> {
        let mut c = Controller::defaults();
        let mut seen: Vec<&str> = vec![];
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let n = i + 1;
            let Some((k, v)) = line.split_once('=') else {
                return Err(format!("line {n}: expected key = value"));
            };
            let (k, v) = (k.trim(), v.trim());
            if seen.contains(&k) {
                return Err(format!("line {n}: key '{k}' is given twice"));
            }
            let num = |hard: u64| -> Result<u64, String> {
                let x: u64 = v.parse().map_err(|_| format!("line {n}: '{k}' needs a non-negative integer"))?;
                if x > hard {
                    return Err(format!("line {n}: '{k}' is above the hard maximum {hard}"));
                }
                Ok(x)
            };
            match k {
                "max_live_rules" => c.max_live_rules = num(HARD_LIVE_RULES as u64)? as usize,
                "max_building_blocks" => c.max_building_blocks = num(HARD_BLOCKS as u64)? as usize,
                "max_search_candidates" => c.max_search_candidates = num(HARD_CANDIDATES as u64)? as usize,
                "max_steps_per_run" => c.max_steps_per_run = num(HARD_STEPS)?,
                "max_energy_per_run" => c.max_energy_per_run = num(HARD_ENERGY as u64)? as i64,
                "composition" => c.composition = parse_bool(v).ok_or_else(|| format!("line {n}: 'composition' needs on or off"))?,
                "pruning" => c.pruning = parse_bool(v).ok_or_else(|| format!("line {n}: 'pruning' needs on or off"))?,
                "transfer" => c.transfer = parse_bool(v).ok_or_else(|| format!("line {n}: 'transfer' needs on or off"))?,
                "max_transfer_links" => c.max_transfer_links = num(HARD_LINKS as u64)? as usize,
                "experiments" => c.experiments = parse_bool(v).ok_or_else(|| format!("line {n}: 'experiments' needs on or off"))?,
                "max_experiment_energy_pm" => c.max_experiment_energy_pm = num(HARD_EXP_PM as u64)? as u32,
                "max_experiments_per_run" => c.max_experiments_per_run = num(HARD_EXP_N)?,
                // B8 keys (planning ahead) are read by `lookahead::ctl`; here they are only validated
                _ if super::lookahead::ctl::is_key(k) => super::lookahead::ctl::validate_key(k, v).map_err(|e| format!("line {n}: {e}"))?,
                // B10 keys (World v2 rules use, adaptation, rule retirement) are read by `world2::skills`; here they are only validated
                _ if super::world2::skills::is_key(k) => super::world2::skills::validate_key(k, v).map_err(|e| format!("line {n}: {e}"))?,
                _ => return Err(format!("line {n}: unknown key '{k}'")),
            }
            seen.push(k);
        }
        Ok(c)
    }

    /// Parse, or fail closed with the reason.
    pub fn parse_or_closed(text: &str) -> Controller {
        Controller::parse(text).unwrap_or_else(|e| Controller::fail_closed(format!("controller.txt is not valid: {e}")))
    }

    pub fn is_closed(&self) -> bool {
        matches!(self.origin, Origin::FailClosed(_))
    }

    /// B9: the experiment budget for one run, or the refusal when experiments were requested but are not allowed.
    pub fn experiment_budget(&self) -> Result<(u32, u64), Refusal> {
        if !self.experiments {
            return Err(Refusal::new("experiments", 0, 1, "experiments were requested but controller.txt does not allow them (experiments = off)"));
        }
        if self.max_experiment_energy_pm == 0 || self.max_experiments_per_run == 0 {
            return Err(Refusal::new("max_experiment_energy_pm", self.max_experiment_energy_pm as u64, 1, "experiments were requested but controller.txt gives them no budget (max_experiment_energy_pm and max_experiments_per_run must both be above 0)"));
        }
        Ok((self.max_experiment_energy_pm, self.max_experiments_per_run))
    }

    /// The limits the loop obeys for one run. `want_compose` / `want_prune`: what the caller asked for;
    /// a request the controller does not allow comes back as a refusal and is not honoured.
    pub fn limits(&self, want_compose: bool, want_prune: bool) -> (Limits, Vec<Refusal>) {
        let mut refusals = vec![];
        if want_compose && !self.composition {
            refusals.push(Refusal::new("composition", 0, 1, "composition of building blocks was requested but controller.txt does not allow it (composition = off)"));
        }
        if want_prune && !self.pruning {
            refusals.push(Refusal::new("pruning", 0, 1, "pruning of rules was requested but controller.txt does not allow it (pruning = off)"));
        }
        let l = Limits {
            max_live_rules: self.max_live_rules,
            max_blocks: self.max_building_blocks,
            max_candidates: self.max_search_candidates,
            compose: want_compose && self.composition,
            prune: want_prune && self.pruning,
            prune_cfg: PruneCfg::default(),
            transfer: self.transfer,
            max_links: self.max_transfer_links,
        };
        (l, refusals)
    }
}

/// Rule pruning settings (tuned on worlds 101..120, see docs/measured.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PruneCfg {
    /// Latest uses of a rule that count (at most `state::RULE_USE_KEEP`).
    pub window: usize,
    /// Fewest uses in the window before a rule can be retired.
    pub min_n: u32,
    /// Retire when the share of right answers in the window is below this.
    pub threshold: f64,
    /// A rule is merged into a simpler one only if it applies to at least this many logged examples and the simpler one always agrees.
    pub merge_min: u32,
}

impl Default for PruneCfg {
    fn default() -> PruneCfg {
        PruneCfg { window: 6, min_n: 4, threshold: 0.5, merge_min: 5 }
    }
}

/// What the engine obeys. `Limits::default()` is "no controller" (used by the older measurements): no cap, no composition, no pruning.
#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    pub max_live_rules: usize,
    pub max_blocks: usize,
    pub max_candidates: usize,
    pub compose: bool,
    pub prune: bool,
    pub prune_cfg: PruneCfg,
    /// B6: links between domains are allowed (default off), and at most this many may exist.
    pub transfer: bool,
    pub max_links: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_live_rules: usize::MAX, max_blocks: usize::MAX, max_candidates: usize::MAX, compose: false, prune: false, prune_cfg: PruneCfg::default(), transfer: false, max_links: usize::MAX }
    }
}

/// A refusal: something was not done because of a cap (or a request the controller does not allow).
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub cap: String,
    pub limit: u64,
    pub requested: u64,
    pub what: String,
}

impl Refusal {
    pub fn new(cap: &str, limit: u64, requested: u64, what: impl Into<String>) -> Refusal {
        Refusal { cap: cap.to_string(), limit, requested, what: what.into() }
    }

    pub fn text(&self, step: u64) -> String {
        format!("controller refusal at step {step}: {} [{}: limit {}, requested {}]; nothing was done beyond the limit", self.what, self.cap, self.limit, self.requested)
    }

    /// The record in the being's own log (an `audit` note; hash-chained like everything else).
    pub fn event(&self, step: u64, clock: i64) -> NewEvent {
        NewEvent {
            source: SOURCE.into(),
            occurred_at: iso_at(clock),
            kind: "note".into(),
            text: self.text(step),
            tags: vec!["audit".into(), "controller_refusal".into(), format!("cap:{}", self.cap), format!("limit:{}", self.limit), format!("requested:{}", self.requested), format!("step:{step}")],
            redacted: false,
        }
    }
}

/// The log record of a run that was stopped by the kill switch or the energy cap.
pub fn stop_event(why: &str, step: u64, clock: i64) -> NewEvent {
    NewEvent {
        source: SOURCE.into(),
        occurred_at: iso_at(clock),
        kind: "note".into(),
        text: format!("controller: the run was stopped at step {step} by {why}"),
        tags: vec!["audit".into(), "controller_stop".into(), format!("stop:{}", why.replace(' ', "_")), format!("step:{step}")],
        redacted: false,
    }
}
