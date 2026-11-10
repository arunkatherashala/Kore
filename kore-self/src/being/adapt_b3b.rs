//! Track B3b: per-cell change handling for the world model (second attempt at adaptation).
//!
//! Attempt 1 (B3, `AdaptMode::Full`) ran one CUSUM per (domain, strategy) group and, on an alarm, cut
//! the whole group's evidence to 10% of its weight. The diagnosis (docs/measured.md, Track B3b) showed
//! that cutting weight sends a cell back to a stale prior with a large uncertainty, which the
//! optimistic planner then re-explores, and that a cell whose own evidence is cleared can be skipped
//! for ever (the skip rule compares an uncertain score with the realized rate, and a skip produces no
//! evidence). This file holds the replacements, used by `model.rs` in mode `AdaptMode::Switch`:
//!
//!   * Windowed switch (M1). Every (domain, difficulty, strategy) cell keeps its last `window`
//!     outcomes. After each attempt a generalized likelihood ratio compares the window with the
//!     evidence before it (two Bernoulli rates against one). Above `nats`, and with at least
//!     `min_old` attempts before the window, the cell's evidence is REPLACED by the window: the cell
//!     keeps real, recent data (no reset to the prior, so no burst of optimism and no skip trap).
//!   * Prior-data conflict (M2). While a cell holds little own evidence, the log-likelihood of its
//!     outcomes under the model's prior-based prediction is compared with a flat prediction from the
//!     cell's own outcomes. When the prior loses by more than `prior_nats`, the cell's prior weight is
//!     cut to `prior_keep` of `PRIOR_K` (the prior was wrong for this cell, e.g. a new domain whose
//!     hard tasks do not behave like its easy ones).
//!   * Cross-domain prior (M4, measured, off by default): while a (domain, strategy) group is young,
//!     a cell's prior mean is the mean posterior of the same (difficulty, strategy) cell in the other
//!     domains that hold evidence there.
//!   * Sibling spread (M3, measured, off by default). When a cell switches, the same (difficulty, strategy) cells of the other
//!     domains test with the lower threshold `nats * spread` from then on (a change seen in one domain
//!     raises the prior odds of the same change elsewhere; it never changes their evidence by itself).
//!
//! All of it is a fixed order of float operations on the logged outcomes, so the fold is replay-exact.
//! No hidden world parameter is read (the scan test of `model.rs` covers this file too).

use std::collections::VecDeque;

/// Settings of the B3b mechanisms. Defaults chosen on tuning seeds 101..120 only (docs/measured.md).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct SwitchCfg {
    /// M1 on/off.
    pub detect: bool,
    /// Recent attempts per cell compared with the older evidence.
    pub window: usize,
    /// Generalized likelihood ratio (nats) above which a cell switches.
    pub nats: f64,
    /// Attempts before the window a cell needs before it can switch.
    pub min_old: f64,
    /// M2 on/off.
    pub prior_check: bool,
    /// Log-likelihood loss (nats) of the prior against a flat prediction that rejects the prior.
    pub prior_nats: f64,
    /// Share of PRIOR_K a cell keeps after its prior is rejected.
    pub prior_keep: f64,
    /// The prior check runs only while the cell holds fewer own attempts than this.
    pub prior_max_n: f64,
    /// M3: threshold factor for sibling cells after a switch (1.0 = off).
    pub spread: f64,
    /// M4: a cell's prior mean is the mean posterior of the same (difficulty, strategy) cell in the other
    /// domains that hold evidence there, instead of its own domain's pooled (domain, strategy) rate.
    pub cross: bool,
    /// M4 applies only while the (domain, strategy) group holds fewer effective attempts than this (a young domain).
    pub cross_below: f64,
}

pub const DEFAULT_SWITCH: SwitchCfg = SwitchCfg { detect: true, window: 6, nats: 8.0, min_old: 8.0, prior_check: true, prior_nats: 1.0, prior_keep: 0.1, prior_max_n: 16.0, spread: 1.0, cross: false, cross_below: f64::INFINITY };

/// Per-cell state.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CellSw {
    /// Last outcomes, oldest first: (correct, energy spent, listed cost).
    pub recent: VecDeque<(bool, i64, i64)>,
    /// Cumulative log-likelihood of the prior-based prediction minus the flat one (M2).
    pub prior_ll: f64,
    pub prior_off: bool,
    /// Threshold factor (M3; 1.0 until a sibling switched).
    pub factor: f64,
    pub switches: u32,
}

/// What a switch test found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwitchHit {
    pub llr: f64,
    pub n_window: u32,
    pub succ_window: u32,
    /// Rate of the evidence before the window.
    pub p_old: f64,
    pub drop: bool,
    /// Sums of the window, which replace the cell's evidence.
    pub spent: i64,
    pub listed: i64,
}

fn ll(p: f64, y: bool) -> f64 {
    if y { p.ln() } else { (1.0 - p).ln() }
}

/// Bernoulli log-likelihood of `s` successes in `n` at rate `p`.
fn bll(s: f64, n: f64, p: f64) -> f64 {
    s * p.ln() + (n - s) * (1.0 - p).ln()
}

impl CellSw {
    pub fn new() -> CellSw {
        CellSw { factor: 1.0, ..Default::default() }
    }

    /// M2: account for one outcome before it is added; `p_model` is the model's prediction for the
    /// cell, `own` the cell's own (attempts, successes). True when the prior is rejected now.
    pub fn prior_step(&mut self, cfg: &SwitchCfg, p_model: f64, own_n: f64, own_s: f64, y: bool) -> bool {
        if !cfg.prior_check || self.prior_off || own_n >= cfg.prior_max_n {
            return false;
        }
        let flat = (own_s + 1.0) / (own_n + 2.0);
        self.prior_ll += ll(p_model.clamp(1e-6, 1.0 - 1e-6), y) - ll(flat, y);
        if self.prior_ll < -cfg.prior_nats {
            self.prior_off = true;
            return true;
        }
        false
    }

    /// M1: record one outcome, then test the window against the evidence before it. `cell_n` and
    /// `cell_s` are the cell's evidence after the outcome was added.
    pub fn switch_step(&mut self, cfg: &SwitchCfg, y: bool, spent: i64, listed: i64, cell_n: f64, cell_s: f64) -> Option<SwitchHit> {
        self.recent.push_back((y, spent, listed));
        while self.recent.len() > cfg.window.max(1) {
            self.recent.pop_front();
        }
        if !cfg.detect || self.recent.len() < cfg.window.max(1) {
            return None;
        }
        let nw = self.recent.len() as f64;
        let sw = self.recent.iter().filter(|r| r.0).count() as f64;
        let n_old = cell_n - nw;
        if n_old < cfg.min_old {
            return None;
        }
        let s_old = (cell_s - sw).max(0.0);
        let p_old = ((s_old + 0.5) / (n_old + 1.0)).clamp(1e-6, 1.0 - 1e-6);
        let p_w = ((sw + 0.5) / (nw + 1.0)).clamp(1e-6, 1.0 - 1e-6);
        let p_all = ((cell_s + 0.5) / (cell_n + 1.0)).clamp(1e-6, 1.0 - 1e-6);
        let llr = bll(sw, nw, p_w) + bll(s_old, n_old, p_old) - bll(cell_s, cell_n, p_all);
        if llr > cfg.nats * self.factor {
            self.switches += 1;
            let (spent, listed) = self.recent.iter().fold((0i64, 0i64), |a, r| (a.0 + r.1, a.1 + r.2));
            return Some(SwitchHit { llr, n_window: nw as u32, succ_window: sw as u32, p_old, drop: p_w < p_old, spent, listed });
        }
        None
    }
}

/// The log records of one B3b change (a note event and a `belief` revision citing the outcomes it rests on).
pub fn cell_change_records(c: &super::state::Change) -> (crate::versioned::NewEvent, crate::versioned::NewRevision) {
    use super::model::Detector;
    let a = &c.alarm;
    let k = a.cell.unwrap_or(0);
    let dir = if a.drop { "lower" } else { "higher" };
    let at = super::world::iso_at(c.clock);
    let (what, action) = match a.kind {
        Detector::CellSwitch => (
            format!("the last {} attempts ({} right) disagree with the older evidence of this cell (older rate {:.2}; likelihood ratio {:.2} nats)", a.run_n, a.run_succ, if a.run_n > 0 { a.run_pred / a.run_n as f64 } else { 0.0 }, a.statistic),
            "the cell's evidence is replaced by its recent window".to_string(),
        ),
        _ => (
            format!("its {} outcomes ({} right, {:.1} predicted) are less likely under the model's prior than under a flat prediction by {:.2} nats", a.run_n, a.run_succ, a.run_pred, a.statistic),
            "the prior weight of this cell is cut".to_string(),
        ),
    };
    let text = format!(
        "change {} detected at step {} ({}): strategy {} in domain {} at difficulty {k} looks {dir} than the model predicted: {what}; {action}",
        c.index, c.step, a.kind.name(), a.strategy.name(), a.domain.name()
    );
    let key = format!("being/change/{:04}", c.index);
    let ev = crate::versioned::NewEvent {
        source: super::state::SOURCE.into(),
        occurred_at: at.clone(),
        kind: "note".into(),
        text,
        tags: vec![
            "change".into(),
            format!("change_index:{}", c.index),
            format!("change_kind:{}", a.kind.name()),
            format!("step:{}", c.step),
            format!("domain:{}", a.domain.name()),
            format!("difficulty:{k}"),
            format!("strategy:{}", a.strategy.name()),
            format!("direction:{}", if a.drop { "drop" } else { "rise" }),
            format!("rev_key:{key}"),
        ],
        redacted: false,
    };
    let ev_id = crate::versioned::event_id(&ev.source, &ev.occurred_at, &ev.text);
    let mut caused_by = c.cites.clone();
    caused_by.push(ev_id);
    let rationale = serde_json::json!({
        "detector": a.kind.name(), "domain": a.domain.name(), "difficulty": k, "strategy": a.strategy.name(),
        "direction": if a.drop { "drop" } else { "rise" }, "statistic_nats": a.statistic,
        "attempts": a.run_n, "right": a.run_succ, "predicted_right": a.run_pred, "cited_outcomes": c.cites.len(), "action": action,
    });
    let rev = crate::versioned::NewRevision {
        entity_type: "belief".into(),
        key,
        stance: format!("{} difficulty {k} with {}: success rate {dir} than predicted", a.domain.name(), a.strategy.name()),
        confidence: (a.statistic / (a.statistic + 4.0)).clamp(0.0, 1.0),
        rationale: rationale.to_string(),
        supersedes: None,
        valid_from: at,
        author: "rule:being-detector".into(),
        caused_by,
    };
    (ev, rev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_that_disagrees_with_long_evidence_switches_and_a_steady_one_does_not() {
        let cfg = SwitchCfg { window: 8, nats: 4.0, ..DEFAULT_SWITCH };
        let mut c = CellSw::new();
        let (mut n, mut s) = (0.0, 0.0);
        // 40 attempts at 90%: never switches
        for i in 0..40 {
            let y = i % 10 != 0;
            n += 1.0;
            s += y as u8 as f64;
            assert!(c.switch_step(&cfg, y, 2, 4, n, s).is_none(), "steady cell switched at {i}");
        }
        // then mostly wrong: switches within one window
        let mut hit = None;
        for i in 0..8 {
            let y = i == 3;
            n += 1.0;
            s += y as u8 as f64;
            if let Some(h) = c.switch_step(&cfg, y, 2, 4, n, s) {
                hit = Some((i, h));
                break;
            }
        }
        let (i, h) = hit.expect("the drop is detected within one window");
        assert!(i <= 7 && h.drop && h.n_window == 8 && h.llr > cfg.nats);
        assert!(h.succ_window <= 3 && h.spent == 16 && h.listed == 32);
        // switching off works
        let mut off = CellSw::new();
        let cfg_off = SwitchCfg { detect: false, ..cfg };
        for k in 0..60 {
            assert!(off.switch_step(&cfg_off, k < 40, 2, 4, (k + 1) as f64, (k + 1).min(40) as f64).is_none());
        }
    }

    #[test]
    fn the_prior_is_rejected_after_surprising_failures_and_kept_when_it_fits() {
        let cfg = DEFAULT_SWITCH;
        let mut bad = CellSw::new();
        let mut rejected_at = None;
        for i in 0..6 {
            // the model predicts 0.8, the cell fails every time
            if bad.prior_step(&cfg, 0.8, i as f64, 0.0, false) {
                rejected_at = Some(i);
                break;
            }
        }
        assert!(matches!(rejected_at, Some(1..=3)), "{rejected_at:?}");
        assert!(bad.prior_off);
        let mut good = CellSw::new();
        for i in 0..8 {
            let y = i != 4;
            assert!(!good.prior_step(&cfg, 0.85, i as f64, (i as f64 - (i > 4) as u8 as f64).max(0.0), y));
        }
        assert!(!good.prior_off);
        let off = SwitchCfg { prior_check: false, ..cfg };
        let mut c = CellSw::new();
        for i in 0..6 {
            assert!(!c.prior_step(&off, 0.99, i as f64, 0.0, false));
        }
    }
}
