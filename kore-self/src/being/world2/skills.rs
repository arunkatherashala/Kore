//! Track B10 (opt-in): the learner switches that the "all skills" profile adds to World v2 beyond B7,
//! B8 and B9, and the owner's switches for them in `controller.txt`.
//!
//! * **Adaptation (B3b in World v2)** (`adapt`). The family-keyed model (`model.rs`) runs the B3b
//!   mechanisms of `adapt_b3b.rs` with their tuned defaults (`DEFAULT_SWITCH`: M1 windowed switch,
//!   M2 prior-data conflict): a cell whose last 6 outcomes disagree with its older evidence by more
//!   than 8 nats keeps only the window; a young cell whose outcomes contradict the prior loses most of
//!   the prior weight. Every such change is a `w2_change` note and a `belief` revision in the log.
//! * **Rule retirement** (`retire`). After every discovery pass interval, each live rule (sequence rule
//!   or fitted constant) is checked against the revealed answers logged SINCE it was accepted, on the
//!   tasks of its signature where it applies: over the last `window` such answers, if there are at least
//!   `min_n` and the 95% Wilson UPPER bound of its share right is below `ub_milli / 1000`, the rule is
//!   retired with a `rule_retired` note and a `rule` revision that cites the rule's note and the outcome
//!   events it got wrong. Its `discovered` cells start over, and the examples of its signature (family
//!   for a constant) logged before the retirement are no longer used by the search, so a rule for the
//!   new regime can be accepted from the answers that follow ("the cell returns to learning").
//!
//! Both are fixed when the being is created (genesis tag `w2_skills`, only written when one is on), so
//! the state folds back from the log exactly. With both off nothing here runs and every log, state and
//! measurement is the one before B10 (tests pin this).
//!
//! Controller keys (validated by `controller::Controller::parse`, read here):
//!
//! ```text
//! rules_use        = on   # may the World v2 learner use its rules (B7 switches A, A2, B, C, G; default off)
//! adaptation       = on   # may the World v2 model adapt to changes (B3b M1+M2; default off)
//! rule_retirement  = on   # may the World v2 being retire rules that its later answers contradict (default off)
//! ```
//!
//! Missing keys are off; a bad value makes the whole file unparseable (fail closed). Only observable
//! things are used: the being's own revealed examples (as TOLD, with noise) and its accepted rules.

use super::engine::{Change2, Engine2, State2};
use super::rules_use::{param_answer, wilson_lb};
use super::world::SOURCE2;
use crate::being::controller::{Controller, Refusal};
use crate::being::discover::predict_x;
use crate::being::world::iso_at;
use crate::versioned::{event_id, NewEvent, NewRevision};
use serde_json::json;

pub const KEY_RULES_USE: &str = "rules_use";
pub const KEY_ADAPT: &str = "adaptation";
pub const KEY_RETIRE: &str = "rule_retirement";

/// Retirement defaults (chosen on tuning seeds 101..120; see docs/measured.md, Track B10).
pub const DEFAULT_RETIRE_WINDOW: u32 = 6;
pub const DEFAULT_RETIRE_MIN: u32 = 4;
pub const DEFAULT_RETIRE_UB_MILLI: u32 = 500;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillsCfg {
    pub adapt: bool,
    pub retire: bool,
    pub retire_window: u32,
    pub retire_min: u32,
    pub retire_ub_milli: u32,
}

impl Default for SkillsCfg {
    fn default() -> Self {
        SkillsCfg { adapt: false, retire: false, retire_window: DEFAULT_RETIRE_WINDOW, retire_min: DEFAULT_RETIRE_MIN, retire_ub_milli: DEFAULT_RETIRE_UB_MILLI }
    }
}

impl SkillsCfg {
    pub fn any(&self) -> bool {
        self.adapt || self.retire
    }
    pub fn tag(&self) -> String {
        format!("w2_skills:{}/{}/{}/{}/{}", self.adapt as u8, self.retire as u8, self.retire_window, self.retire_min, self.retire_ub_milli)
    }
    pub fn parse(v: &str) -> Option<SkillsCfg> {
        let p: Vec<&str> = v.split('/').collect();
        if p.len() != 5 {
            return None;
        }
        Some(SkillsCfg { adapt: p[0] == "1", retire: p[1] == "1", retire_window: p[2].parse().ok()?, retire_min: p[3].parse().ok()?, retire_ub_milli: p[4].parse().ok()? })
    }
    pub fn words(&self) -> String {
        format!(
            "; skills: adaptation (B3b windowed switch and prior conflict) {}, rule retirement {}",
            if self.adapt { "on" } else { "off" },
            if self.retire { format!("on (last {} answers where the rule applies, at least {}, retired when the 95% upper bound of the share right is below {:.3})", self.retire_window, self.retire_min, self.retire_ub_milli as f64 / 1000.0) } else { "off".into() }
        )
    }
}

/// 95% Wilson upper bound of `k` successes in `n`.
pub fn wilson_ub(k: u32, n: u32) -> f64 {
    if n == 0 {
        return 1.0;
    }
    1.0 - wilson_lb(n - k, n)
}

/// What the retirement check found for one rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Retire {
    pub rule_id: String,
    /// The rule's note event.
    pub rule_event: String,
    /// `seq` (a sequence rule of signature `sig`) or `const` (a fitted constant of family `fam`).
    pub kind: &'static str,
    pub sig: u8,
    pub fam: usize,
    pub n: u32,
    pub right: u32,
    pub ub: f64,
    /// Outcome events of the revealed answers the rule got wrong (newest first).
    pub failing: Vec<String>,
    /// All outcome events of the window (newest first).
    pub window: Vec<String>,
}

/// The rules the retirement rule would retire now (in rule order, constants after sequence rules).
pub fn retire_candidates(st: &State2, cfg: &SkillsCfg) -> Vec<Retire> {
    let w = cfg.retire_window.max(1) as usize;
    let bound = cfg.retire_ub_milli as f64 / 1000.0;
    let mut out = vec![];
    for r in st.rules.iter().filter(|r| !r.retired) {
        let since = st.rule_since.get(&r.id).copied().unwrap_or(0);
        let floor = st.sig_floor.get(&r.sig).copied().unwrap_or(0);
        let (mut n, mut right, mut failing, mut window) = (0u32, 0u32, vec![], vec![]);
        for e in st.examples[since.max(floor).min(st.examples.len())..].iter().rev() {
            if n as usize >= w {
                break;
            }
            if e.sig != r.sig {
                continue;
            }
            if let Some(v) = predict_x(r.xf, &r.form, &e.terms) {
                n += 1;
                window.push(e.event_id.clone());
                if v == e.next {
                    right += 1;
                } else {
                    failing.push(e.event_id.clone());
                }
            }
        }
        let ub = wilson_ub(right, n);
        if n >= cfg.retire_min && ub < bound {
            out.push(Retire { rule_id: r.id.clone(), rule_event: r.event_id.clone(), kind: "seq", sig: r.sig, fam: super::solve::sig2_split(r.sig).0, n, right, ub, failing, window });
        }
    }
    for c in &st.const_rules {
        let since = st.rule_since.get(&c.id).copied().unwrap_or(0);
        let floor = st.const_floor.get(&c.fam).copied().unwrap_or(0);
        let (mut n, mut right, mut failing, mut window) = (0u32, 0u32, vec![], vec![]);
        for e in st.cexamples[since.max(floor).min(st.cexamples.len())..].iter().rev() {
            if n as usize >= w {
                break;
            }
            if e.fam != c.fam {
                continue;
            }
            if let Some(v) = param_answer(&e.kind, c.g, &e.x) {
                n += 1;
                window.push(e.event_id.clone());
                if v == e.revealed {
                    right += 1;
                } else {
                    failing.push(e.event_id.clone());
                }
            }
        }
        let ub = wilson_ub(right, n);
        if n >= cfg.retire_min && ub < bound {
            out.push(Retire { rule_id: c.id.clone(), rule_event: c.event_id.clone(), kind: "const", sig: 0, fam: c.fam, n, right, ub, failing, window });
        }
    }
    out
}

// ---- the retirement pass and the log records (on the engine) ----

/// A retirement with its note and revision.
pub struct NewRetire2 {
    pub rec: Retire,
    pub step: u64,
    pub event: NewEvent,
    pub revision: NewRevision,
}

/// The note and revision of one retirement.
pub fn retire_records(r: &Retire, cfg: &SkillsCfg, step: u64, clock: i64) -> (NewEvent, NewRevision) {
    let at = iso_at(clock);
    let what = match r.kind {
        "seq" => {
            let (f, d) = super::solve::sig2_split(r.sig);
            format!("rule {} of family f{f:02} difficulty {d}", r.rule_id)
        }
        _ => format!("constant {} of family f{:02}", r.rule_id, r.fam),
    };
    let text = format!(
        "world v2 {what} retired at step {step}: of the last {} revealed answers since it was accepted on tasks where it applies, it gave {} (95% upper bound of the share right {:.3}, below {:.3}); it is not applied again, its discovered cells start over and the examples logged so far for it are no longer searched",
        r.n, r.right, r.ub, cfg.retire_ub_milli as f64 / 1000.0
    );
    let mut tags = vec!["rule_retired".to_string(), "world2".into(), format!("rule_id:{}", r.rule_id), format!("kind:{}", r.kind), format!("fam:{}", r.fam), format!("window_n:{}", r.n), format!("window_right:{}", r.right), format!("ub_milli:{}", (r.ub * 1000.0).round() as u32), format!("at_step:{step}")];
    if r.kind == "seq" {
        tags.push(format!("sig:{}", r.sig));
        tags.push(format!("difficulty:{}", super::solve::sig2_split(r.sig).1));
    }
    let event = NewEvent { source: SOURCE2.into(), occurred_at: at.clone(), kind: "note".into(), text, tags, redacted: false };
    let ev_id = event_id(&event.source, &event.occurred_at, &event.text);
    let mut caused_by = vec![];
    if !r.rule_event.is_empty() {
        caused_by.push(r.rule_event.clone());
    }
    caused_by.extend(r.failing.iter().cloned());
    caused_by.push(ev_id);
    let rationale = json!({
        "rule_id": r.rule_id, "kind": r.kind, "family": r.fam, "window_answers": r.n, "window_right": r.right, "upper_bound_95": r.ub,
        "threshold": cfg.retire_ub_milli as f64 / 1000.0, "min_answers": cfg.retire_min, "window": cfg.retire_window,
        "failing_outcomes": r.failing, "window_outcomes": r.window, "retired_at_step": step,
        "note": "the revealed answers are what the being was TOLD; with feedback noise some are wrong",
    });
    let revision = NewRevision {
        entity_type: "rule".into(),
        key: format!("being/world2/rule/{}", r.rule_id),
        stance: format!("retired: right on {} of the last {} revealed answers where it applies", r.right, r.n),
        confidence: (1.0 - r.ub).clamp(0.0, 1.0),
        rationale: rationale.to_string(),
        supersedes: None,
        valid_from: at,
        author: "rule:being-retire".into(),
        caused_by,
    };
    (event, revision)
}

/// The note and `belief` revision of one B3b change in World v2.
pub fn change_records2(c: &Change2) -> (NewEvent, NewRevision) {
    let ch = &c.change;
    let dir = if ch.drop { "lower" } else { "higher" };
    let at = iso_at(c.clock);
    let (what, action) = match ch.kind {
        "cell-switch" => (
            format!("the last {} attempts ({} right) disagree with the older evidence of this cell (older rate {:.2}; likelihood ratio {:.2} nats)", ch.n, ch.succ, if ch.n > 0 { ch.predicted / ch.n as f64 } else { 0.0 }, ch.statistic),
            "the evidence of the cell is replaced by its recent window",
        ),
        _ => (
            format!("its {} outcomes ({} right, {:.1} predicted) are less likely under the prior of the model than under a flat prediction by {:.2} nats", ch.n, ch.succ, ch.predicted, ch.statistic),
            "the prior weight of this cell is cut",
        ),
    };
    let text = format!("world v2 change {} detected at step {} ({}): strategy {} on family f{:02} at difficulty {} looks {dir} than the model predicted: {what}; {action}", c.index, c.step, ch.kind, ch.strategy.name(), ch.fam, ch.difficulty);
    let key = format!("being/world2/change/{:04}", c.index);
    let event = NewEvent {
        source: SOURCE2.into(),
        occurred_at: at.clone(),
        kind: "note".into(),
        text,
        tags: vec![
            "w2_change".into(),
            format!("change_index:{}", c.index),
            format!("change_kind:{}", ch.kind),
            format!("step:{}", c.step),
            format!("fam:{}", ch.fam),
            format!("difficulty:{}", ch.difficulty),
            format!("strategy:{}", ch.strategy.name()),
            format!("direction:{}", if ch.drop { "drop" } else { "rise" }),
            format!("rev_key:{key}"),
        ],
        redacted: false,
    };
    let ev_id = event_id(&event.source, &event.occurred_at, &event.text);
    let mut caused_by = c.cites.clone();
    caused_by.push(ev_id);
    let rationale = json!({
        "detector": ch.kind, "family": ch.fam, "difficulty": ch.difficulty, "strategy": ch.strategy.name(), "direction": if ch.drop { "drop" } else { "rise" },
        "statistic_nats": ch.statistic, "attempts": ch.n, "right": ch.succ, "predicted_right": ch.predicted, "cited_outcomes": c.cites.len(), "action": action,
    });
    let revision = NewRevision {
        entity_type: "belief".into(),
        key,
        stance: format!("f{:02} difficulty {} with {}: success rate {dir} than predicted", ch.fam, ch.difficulty, ch.strategy.name()),
        confidence: (ch.statistic / (ch.statistic + 4.0)).clamp(0.0, 1.0),
        rationale: rationale.to_string(),
        supersedes: None,
        valid_from: at,
        author: "rule:being-detector".into(),
        caused_by,
    };
    (event, revision)
}

impl Engine2 {
    /// B10: retire every live rule whose recent answers contradict it (nothing when retirement is off).
    pub fn retire_pass(&mut self) -> Vec<NewRetire2> {
        let cfg = self.state.learner.skills;
        if !cfg.retire {
            return vec![];
        }
        let mut out = vec![];
        for r in retire_candidates(&self.state, &cfg) {
            let step = self.state.step;
            if self.state.retire_rule(&r.rule_id, step) {
                let (event, revision) = retire_records(&r, &cfg, step, self.state.clock);
                out.push(NewRetire2 { rec: r, step, event, revision });
            }
        }
        out
    }

    /// B10: log records of the changes of the adaptive model not handed out yet (nothing when adaptation is off).
    pub fn take_change_records(&mut self) -> Vec<(NewEvent, NewRevision)> {
        let v: Vec<(NewEvent, NewRevision)> = self.state.changes[self.changes_logged.min(self.state.changes.len())..].iter().map(change_records2).collect();
        self.changes_logged = self.state.changes.len();
        v
    }
}

// ---- controller keys ----

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

pub fn is_key(k: &str) -> bool {
    k == KEY_RULES_USE || k == KEY_ADAPT || k == KEY_RETIRE
}

pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    parse_bool(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off"))
}

/// The three B10 switches of `controller.txt` (missing file or invalid file: all off, `closed` says why).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsCtl {
    pub rules_use: bool,
    pub adaptation: bool,
    pub retirement: bool,
    pub closed: Option<String>,
}

impl SkillsCtl {
    pub fn from_text(text: Option<&str>) -> SkillsCtl {
        let off = |r: String| SkillsCtl { rules_use: false, adaptation: false, retirement: false, closed: Some(r) };
        let Some(text) = text else { return off("controller.txt is missing".into()) };
        if let Err(e) = Controller::parse(text) {
            return off(format!("controller.txt is not valid: {e}"));
        }
        let mut c = SkillsCtl { rules_use: false, adaptation: false, retirement: false, closed: None };
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                KEY_RULES_USE => c.rules_use = parse_bool(v).unwrap_or(false),
                KEY_ADAPT => c.adaptation = parse_bool(v).unwrap_or(false),
                KEY_RETIRE => c.retirement = parse_bool(v).unwrap_or(false),
                _ => {}
            }
        }
        c
    }

    fn refusal(&self, key: &str, what: &str) -> Refusal {
        let why = match &self.closed {
            Some(r) => format!("{what} was requested but the controller failed closed ({r}); it stays off"),
            None => format!("{what} was requested but controller.txt does not allow it ({key} = off); it stays off"),
        };
        Refusal::new(key, 0, 1, why)
    }

    /// Grant the B10 learner switches that were asked for. Returns (rules use, adaptation, retirement) and the refusals.
    pub fn grant(&self, want_use: bool, want_adapt: bool, want_retire: bool) -> ((bool, bool, bool), Vec<Refusal>) {
        let mut refusals = vec![];
        let u = want_use && self.rules_use;
        if want_use && !u {
            refusals.push(self.refusal(KEY_RULES_USE, "using discovered rules (B7 switches)"));
        }
        let a = want_adapt && self.adaptation;
        if want_adapt && !a {
            refusals.push(self.refusal(KEY_ADAPT, "adaptation of the model (B3b)"));
        }
        let r = want_retire && self.retirement;
        if want_retire && !r {
            refusals.push(self.refusal(KEY_RETIRE, "rule retirement"));
        }
        ((u, a, r), refusals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_upper_bound_brackets_the_rate() {
        assert_eq!(wilson_ub(0, 0), 1.0);
        let u = wilson_ub(0, 8);
        assert!(u > 0.25 && u < 0.4, "{u}");
        assert!(wilson_ub(8, 8) > 0.99);
        assert!(wilson_ub(6, 10) > 0.6 && wilson_ub(6, 10) < 0.9);
        assert!(wilson_ub(1, 10) < 0.5);
    }

    #[test]
    fn skills_tag_round_trips_and_controller_keys_fail_closed() {
        let s = SkillsCfg { adapt: true, retire: true, retire_window: 12, retire_min: 5, retire_ub_milli: 450 };
        assert_eq!(SkillsCfg::parse(s.tag().strip_prefix("w2_skills:").unwrap()), Some(s));
        assert!(SkillsCfg::parse("1/0/3").is_none());
        assert!(!SkillsCfg::default().any());
        // missing file: closed, everything refused
        let c = SkillsCtl::from_text(None);
        let ((u, a, r), refs) = c.grant(true, true, true);
        assert!(!u && !a && !r && refs.len() == 3 && c.closed.is_some());
        // a bad value makes the whole file unparseable
        assert!(Controller::parse("rules_use = maybe\n").is_err());
        let c = SkillsCtl::from_text(Some("rules_use = maybe\n"));
        assert!(c.closed.is_some() && !c.rules_use);
        // valid file with defaults: all off, requests refused
        let c = SkillsCtl::from_text(Some("max_live_rules = 4\n"));
        assert!(c.closed.is_none());
        let ((u, a, r), refs) = c.grant(true, false, true);
        assert!(!u && !a && !r && refs.len() == 2);
        assert!(refs.iter().all(|x| x.what.contains("does not allow")));
        // allowed
        let c = SkillsCtl::from_text(Some("rules_use = on\nadaptation = on\nrule_retirement = on\n"));
        let ((u, a, r), refs) = c.grant(true, true, true);
        assert!(u && a && r && refs.is_empty());
        // not asked: nothing granted, nothing refused
        let ((u, a, r), refs) = c.grant(false, false, false);
        assert!(!u && !a && !r && refs.is_empty());
    }
}
