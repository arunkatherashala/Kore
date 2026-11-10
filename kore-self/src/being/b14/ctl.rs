//! Track B14: the owner's switches in `controller.txt` and the being's own (creation-time) setup.
//!
//! ```text
//! rule_library             = on    # may a first-world being keep and reuse a rule library (B14, default off)
//! curiosity                = on    # may a first-world being spend a curiosity budget (B14, default off)
//! max_curiosity_energy_pm  = 20    # curiosity may spend at most this share (per mille) of the energy spent so far (B14, default 0, hard max 100)
//! ```
//!
//! Fail closed: a missing or invalid file means both off; curiosity needs `curiosity = on` AND a
//! non-zero `max_curiosity_energy_pm`. A bad value (or a repeated key) makes the whole file
//! unparseable (`Controller::parse` calls `validate_key`). Asking for a switch that is not allowed is
//! a refusal that goes into the being's log and the audit trail; the being then runs without it.
//!
//! The switches a being uses are fixed when it is created: a `b14_setup` note right after the genesis
//! (tags `b14_setup`, `rule_library:on|off`, `curiosity_pm:<n>`). Older logs have no such note and
//! fold exactly as before (B14 off).

use super::super::controller::Refusal;
use super::super::state::{tag, SOURCE};
use super::super::world::iso_at;
use crate::versioned::NewEvent;

pub const KEY_LIBRARY: &str = "rule_library";
pub const KEY_CURIOSITY: &str = "curiosity";
pub const KEY_CURIOSITY_PM: &str = "max_curiosity_energy_pm";
pub const HARD_CURIOSITY_PM: u32 = 100;
pub const SETUP_TAG: &str = "b14_setup";

pub fn is_key(k: &str) -> bool {
    k == KEY_LIBRARY || k == KEY_CURIOSITY || k == KEY_CURIOSITY_PM
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
    if k == KEY_CURIOSITY_PM {
        let x: u32 = v.parse().map_err(|_| format!("'{k}' needs a non-negative integer"))?;
        if x > HARD_CURIOSITY_PM {
            return Err(format!("'{k}' is above the hard maximum {HARD_CURIOSITY_PM}"));
        }
        return Ok(());
    }
    on_off(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off"))
}

/// What controller.txt allows for B14.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct B14Ctl {
    pub library: bool,
    pub curiosity: bool,
    pub curiosity_pm: u32,
    /// None: from a valid file; Some(reason): failed closed.
    pub closed: Option<String>,
}

impl B14Ctl {
    /// From the text of controller.txt (None: missing). Missing or invalid: everything off.
    pub fn from_text(text: Option<&str>) -> B14Ctl {
        let Some(text) = text else { return B14Ctl { closed: Some("controller.txt is missing".into()), ..Default::default() } };
        if let Err(e) = super::super::controller::Controller::parse(text) {
            return B14Ctl { closed: Some(format!("controller.txt is not valid: {e}")), ..Default::default() };
        }
        let mut c = B14Ctl::default();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                match k {
                    KEY_LIBRARY => c.library = on_off(v).unwrap_or(false),
                    KEY_CURIOSITY => c.curiosity = on_off(v).unwrap_or(false),
                    KEY_CURIOSITY_PM => c.curiosity_pm = v.parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        c
    }

    /// Grant a request. Returns what is granted and the refusals for what is not.
    pub fn grant(&self, want: &B14Setup) -> (B14Setup, Vec<Refusal>) {
        let mut out = B14Setup::default();
        let mut refusals = vec![];
        let why = |what: &str, key: &str| match &self.closed {
            Some(r) => format!("{what} (B14) was requested but the controller failed closed ({r}); it is not used"),
            None => format!("{what} (B14) was requested but controller.txt does not allow it ({key}); it is not used"),
        };
        if want.library {
            if self.library && self.closed.is_none() {
                out.library = true;
            } else {
                refusals.push(Refusal::new(KEY_LIBRARY, 0, 1, why("the rule library", &format!("{KEY_LIBRARY} = off"))));
            }
        }
        if want.curiosity_pm > 0 {
            if self.curiosity && self.curiosity_pm > 0 && self.closed.is_none() {
                out.curiosity_pm = want.curiosity_pm.min(self.curiosity_pm);
                if want.curiosity_pm > self.curiosity_pm {
                    refusals.push(Refusal::new(KEY_CURIOSITY_PM, self.curiosity_pm as u64, want.curiosity_pm as u64, format!("a curiosity budget of {} per mille was requested; controller.txt allows {} per mille, which is used", want.curiosity_pm, self.curiosity_pm)));
                }
            } else {
                refusals.push(Refusal::new(KEY_CURIOSITY, self.curiosity_pm as u64, want.curiosity_pm as u64, why("curiosity", &format!("{KEY_CURIOSITY} = on and {KEY_CURIOSITY_PM} > 0 are both needed"))));
            }
        }
        (out, refusals)
    }
}

/// The B14 switches of one being, fixed when it is created.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct B14Setup {
    pub library: bool,
    /// Curiosity budget, per mille of the energy spent so far (0 = off).
    pub curiosity_pm: u32,
}

impl B14Setup {
    pub fn is_off(&self) -> bool {
        !self.library && self.curiosity_pm == 0
    }
    pub fn tags(&self) -> Vec<String> {
        vec![SETUP_TAG.to_string(), format!("rule_library:{}", if self.library { "on" } else { "off" }), format!("curiosity_pm:{}", self.curiosity_pm)]
    }
    pub fn from_tags(tags: &str) -> Option<B14Setup> {
        let library = match tag(tags, "rule_library")? {
            "on" => true,
            "off" => false,
            _ => return None,
        };
        let curiosity_pm: u32 = tag(tags, "curiosity_pm")?.parse().ok()?;
        if curiosity_pm > HARD_CURIOSITY_PM {
            return None;
        }
        Some(B14Setup { library, curiosity_pm })
    }
    /// The note that fixes the switches (written once, right after the genesis).
    pub fn event(&self) -> NewEvent {
        let text = format!(
            "being setup (Track B14): rule library {}; curiosity budget {} per mille of the energy spent so far",
            if self.library { "on (verified rules are kept, reused in new contexts after a check on the being's own examples, and retired when revealed answers contradict them)" } else { "off" },
            self.curiosity_pm
        );
        NewEvent { source: SOURCE.into(), occurred_at: iso_at(0), kind: "note".into(), text, tags: self.tags(), redacted: false }
    }
}
