//! Track E1: the owner's switches in `controller.txt` and the being's own (creation-time) setup.
//!
//! ```text
//! evolution            = on    # may a first-world being run evolutionary search over its rule DSL (E1, default off)
//! evolution_budget_pm  = 20    # evolution may spend at most this share (per mille) of the energy spent so far, in evaluations (E1, default 0, hard max 100)
//! ```
//!
//! Fail closed: a missing or invalid file means off; evolution needs `evolution = on` AND a non-zero
//! `evolution_budget_pm`. A bad value (or a repeated key) makes the whole file unparseable
//! (`Controller::parse` calls `validate_key`). Asking for evolution when it is not allowed is a
//! refusal that goes into the being's log and the audit trail; the being then runs without it.
//!
//! The budget is compute, not task energy: one energy unit buys `EVALS_PER_ENERGY` evaluations of
//! one candidate expression on one revealed example (an author-set exchange rate; the wall time is
//! measured separately). The switch is fixed when the being is created: an `e1_setup` note right
//! after the genesis (tags `e1_setup`, `evolution_pm:<n>`). Older logs have no such note and fold
//! exactly as before (E1 off).

use super::super::controller::Refusal;
use super::super::state::{tag, SOURCE};
use super::super::world::iso_at;
use crate::versioned::NewEvent;

pub const KEY_EVOLUTION: &str = "evolution";
pub const KEY_BUDGET_PM: &str = "evolution_budget_pm";
pub const HARD_BUDGET_PM: u32 = 100;
pub const SETUP_TAG: &str = "e1_setup";

pub fn is_key(k: &str) -> bool {
    k == KEY_EVOLUTION || k == KEY_BUDGET_PM
}

fn on_off(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Check one value (called by `Controller::parse`; an error makes the whole file invalid).
pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    if k == KEY_BUDGET_PM {
        let x: u32 = v.parse().map_err(|_| format!("'{k}' needs a non-negative integer"))?;
        if x > HARD_BUDGET_PM {
            return Err(format!("'{k}' is above the hard maximum {HARD_BUDGET_PM}"));
        }
        return Ok(());
    }
    on_off(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off"))
}

/// What controller.txt allows for E1.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct E1Ctl {
    pub evolution: bool,
    pub budget_pm: u32,
    /// None: from a valid file; Some(reason): failed closed.
    pub closed: Option<String>,
}

impl E1Ctl {
    /// From the text of controller.txt (None: missing). Missing or invalid: off.
    pub fn from_text(text: Option<&str>) -> E1Ctl {
        let Some(text) = text else { return E1Ctl { closed: Some("controller.txt is missing".into()), ..Default::default() } };
        if let Err(e) = super::super::controller::Controller::parse(text) {
            return E1Ctl { closed: Some(format!("controller.txt is not valid: {e}")), ..Default::default() };
        }
        let mut c = E1Ctl::default();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                match k {
                    KEY_EVOLUTION => c.evolution = on_off(v).unwrap_or(false),
                    KEY_BUDGET_PM => c.budget_pm = v.parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        c
    }

    /// Grant a request. Returns what is granted (0 = off) and the refusals for what is not.
    pub fn grant(&self, want: &E1Setup) -> (E1Setup, Vec<Refusal>) {
        let mut refusals = vec![];
        if want.pm == 0 {
            return (E1Setup::default(), refusals);
        }
        if !(self.evolution && self.budget_pm > 0 && self.closed.is_none()) {
            let what = match &self.closed {
                Some(r) => format!("evolution (E1) was requested but the controller failed closed ({r}); it is not used"),
                None => format!("evolution (E1) was requested but controller.txt does not allow it ({KEY_EVOLUTION} = on and {KEY_BUDGET_PM} > 0 are both needed); it is not used"),
            };
            refusals.push(Refusal::new(KEY_EVOLUTION, self.budget_pm as u64, want.pm as u64, what));
            return (E1Setup::default(), refusals);
        }
        if want.pm > self.budget_pm {
            refusals.push(Refusal::new(KEY_BUDGET_PM, self.budget_pm as u64, want.pm as u64, format!("an evolution budget of {} per mille was requested; controller.txt allows {} per mille, which is used", want.pm, self.budget_pm)));
        }
        (E1Setup { pm: want.pm.min(self.budget_pm) }, refusals)
    }
}

/// The E1 switch of one being, fixed when it is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct E1Setup {
    /// Evolution budget, per mille of the energy spent so far (0 = off).
    pub pm: u32,
}

impl E1Setup {
    pub fn is_off(&self) -> bool {
        self.pm == 0
    }
    pub fn tags(&self) -> Vec<String> {
        vec![SETUP_TAG.to_string(), format!("evolution_pm:{}", self.pm)]
    }
    pub fn from_tags(tags: &str) -> Option<E1Setup> {
        let pm: u32 = tag(tags, "evolution_pm")?.parse().ok()?;
        (pm <= HARD_BUDGET_PM).then_some(E1Setup { pm })
    }
    /// The note that fixes the switch (written once, right after the genesis).
    pub fn event(&self) -> NewEvent {
        let text = format!(
            "being setup (Track E1): evolutionary search over the rule expression language with a budget of {} per mille of the energy spent so far ({} evaluations of one candidate on one revealed example per energy unit); a candidate becomes a rule only after the B2 check on the being's own training and held-out examples",
            self.pm,
            super::evolve::EVALS_PER_ENERGY
        );
        NewEvent { source: SOURCE.into(), occurred_at: iso_at(0), kind: "note".into(), text, tags: self.tags(), redacted: false }
    }
}
