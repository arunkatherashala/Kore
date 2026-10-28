//! Learned trust in an outside advisor (Track B, step B4).
//!
//! The being keeps one reliability estimate per (advisor, kind), where kind is a domain name
//! (menu advice about a task of that domain) or `rule` (proposed rules). Everything here is
//! folded from the log: the advice events (source `advisor`), the decision events and the outcome
//! events. Nothing is stored anywhere else, and nothing here reads the world: a test scans this
//! file for the hidden-parameter names.
//!
//! ## What is learned
//!
//! Menu advice names one (task, strategy) option. Only when the being FOLLOWS advice that
//! DISAGREES with what its own model alone would have chosen is there something to learn (when
//! they agree the decision is the same, whatever the trust). Such a "test" is judged as follows:
//! the model alone predicted, for its own choice, a reward per energy `ref`; the followed advice
//! earned `x = reward / energy actually spent`. The advice WINS the test if `x >= ref`, else it
//! loses. Trust is the Beta(1 + wins, 1 + losses) posterior of the win rate. A win rate of
//! `NULL_RATE` (one half) is what advice that is no better than the model would get, so advice
//! is useful only above it.
//!
//! Rule advice is factual: a proposed rule is verified by the same search that accepts discovered
//! rules. Accepted = win; contradicted by the being's own logged examples (applicable and wrong) =
//! loss; merely not supported by enough examples yet = no verdict. This is recorded under the kind `rule`; it never
//! changes a decision (the verified search is the only gate), it is reported.
//!
//! ## How trust is used (mixing weight)
//!
//! `w = clamp((mean + beta * sd - NULL_RATE) / (1 - NULL_RATE), 0, 1)` with the posterior mean and
//! standard deviation of the cell: an optimistic (UCB) weight, so a new or uncertain advisor is
//! tried (the exploration needed to learn trust) and the optimism, hence the cost of testing a
//! useless advisor, shrinks as 1/sqrt(tests). The planner (see `planner.rs`) lifts the advised
//! option's score towards `(1 + LIFT) x` the best score by the fraction `w`.
//!
//! ## Degradation
//!
//! A one-sided CUSUM (the B3 idea) watches the standardized residuals `(win - mean) / sd0` of
//! the tests; when it crosses `CUSUM_H` the advisor's evidence in that cell is cut to
//! `KEEP_AFTER_ALARM` of its weight, the trust falls back towards the prior and the optimism
//! sends the being to test the advisor again. The alarm is logged.

use std::collections::BTreeMap;

/// Win rate of advice that is no better than the being's own model.
pub const NULL_RATE: f64 = 0.5;
/// Optimism (in posterior standard deviations) of the mixing weight.
/// Chosen on tuning seeds 101..120 only (see docs/measured.md).
pub const DEFAULT_TRUST_BETA: f64 = 1.0;
/// The advised option is lifted towards `(1 + LIFT)` times the best score.
pub const LIFT: f64 = 0.1;
/// CUSUM reference value and threshold for the degradation detector, and the share of evidence kept after an alarm.
/// Chosen on tuning seeds 101..120 only (see docs/measured.md).
pub const CUSUM_K: f64 = 0.5;
pub const CUSUM_H: f64 = 6.0;
pub const KEEP_AFTER_ALARM: f64 = 0.2;
/// A cell needs at least this much evidence before it feeds the detector.
pub const MIN_EVIDENCE: f64 = 6.0;
const VAR_FLOOR: f64 = 0.09;

/// Reliability of one advisor for one kind of advice.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrustCell {
    /// Discounted (after an alarm) win and loss counts of the tests.
    pub wins: f64,
    pub losses: f64,
    /// Raw counts, never discounted.
    pub tests: u64,
    pub test_wins: u64,
    /// Advice that was given (an option was named), followed or not.
    pub given: u64,
    /// Advice the being followed although its model alone preferred something else.
    pub followed_against_model: u64,
    /// CUSUM statistic (drop side).
    pub down: f64,
    pub alarms: u32,
}

impl TrustCell {
    pub fn mean(&self) -> f64 {
        (1.0 + self.wins) / (2.0 + self.wins + self.losses)
    }
    pub fn sd(&self) -> f64 {
        let (a, b) = (1.0 + self.wins, 1.0 + self.losses);
        (a * b / ((a + b) * (a + b) * (a + b + 1.0))).sqrt()
    }
    /// Optimistic mixing weight in 0..=1 (see the module comment).
    pub fn weight(&self, beta: f64) -> f64 {
        ((self.mean() + beta * self.sd() - NULL_RATE) / (1.0 - NULL_RATE)).clamp(0.0, 1.0)
    }
}

/// A flagged degradation of one advisor in one kind of advice.
#[derive(Debug, Clone, PartialEq)]
pub struct TrustAlarm {
    pub advisor: String,
    pub kind: String,
    pub statistic: f64,
    /// Trust (posterior mean) just before the alarm.
    pub mean_before: f64,
    pub tests: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrustTable {
    cells: BTreeMap<(String, String), TrustCell>,
    /// Detector settings (defaults are the constants above; the measurement tunes them on the tuning seeds).
    pub cusum_k: f64,
    pub cusum_h: f64,
    pub keep: f64,
}

impl Default for TrustTable {
    fn default() -> Self {
        TrustTable { cells: BTreeMap::new(), cusum_k: CUSUM_K, cusum_h: CUSUM_H, keep: KEEP_AFTER_ALARM }
    }
}

impl TrustTable {
    pub fn cell(&self, advisor: &str, kind: &str) -> TrustCell {
        self.cells.get(&(advisor.to_string(), kind.to_string())).copied().unwrap_or_default()
    }

    fn at(&mut self, advisor: &str, kind: &str) -> &mut TrustCell {
        self.cells.entry((advisor.to_string(), kind.to_string())).or_default()
    }

    /// Advice naming an option was given.
    pub fn note_given(&mut self, advisor: &str, kind: &str) {
        self.at(advisor, kind).given += 1;
    }

    /// One test (advice followed against the model, or a verified rule): win or loss.
    /// Returns an alarm when the degradation detector fires.
    pub fn observe(&mut self, advisor: &str, kind: &str, win: bool, against_model: bool) -> Option<TrustAlarm> {
        let (ck, ch, keep) = (self.cusum_k, self.cusum_h, self.keep);
        let c = self.at(advisor, kind);
        let mean = c.mean();
        let mut alarm = None;
        if c.wins + c.losses >= MIN_EVIDENCE {
            let z = (win as u8 as f64 - mean) / (mean * (1.0 - mean)).max(VAR_FLOOR).sqrt();
            c.down = (c.down - z - ck).max(0.0);
            if c.down > ch {
                alarm = Some(TrustAlarm { advisor: advisor.to_string(), kind: kind.to_string(), statistic: c.down, mean_before: mean, tests: c.tests + 1 });
                c.wins *= keep;
                c.losses *= keep;
                c.down = 0.0;
                c.alarms += 1;
            }
        }
        c.tests += 1;
        c.followed_against_model += against_model as u64;
        if win {
            c.wins += 1.0;
            c.test_wins += 1;
        } else {
            c.losses += 1.0;
        }
        alarm
    }

    pub fn weight(&self, advisor: &str, kind: &str, beta: f64) -> (f64, f64, f64) {
        let c = self.cell(advisor, kind);
        (c.weight(beta), c.mean(), c.sd())
    }

    pub fn cells(&self) -> impl Iterator<Item = (&(String, String), &TrustCell)> {
        self.cells.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prior_is_neutral_and_optimistic_weight_shrinks_with_evidence() {
        let mut t = TrustTable::default();
        let (w0, m0, _) = t.weight("a", "algebra", 1.0);
        assert!((m0 - 0.5).abs() < 1e-12 && w0 > 0.5, "an unknown advisor is tried: w {w0}");
        // a useless advisor (win rate 0.4): the weight falls to zero as tests accumulate
        for i in 0..200 {
            t.observe("a", "algebra", i % 5 < 2, true);
        }
        let (w, m, _) = t.weight("a", "algebra", 1.0);
        assert!(m < 0.45 && w == 0.0, "w {w} mean {m}");
        // a good advisor keeps a high weight
        for i in 0..200 {
            t.observe("b", "algebra", i % 10 != 0, true);
        }
        assert!(t.weight("b", "algebra", 1.0).0 > 0.7);
    }

    #[test]
    fn degradation_is_flagged_and_evidence_cut() {
        let mut t = TrustTable::default();
        for _ in 0..60 {
            t.observe("a", "physics", true, true);
        }
        assert_eq!(t.cell("a", "physics").alarms, 0);
        let mut alarms = 0;
        for _ in 0..30 {
            alarms += t.observe("a", "physics", false, true).is_some() as u32;
        }
        assert!(alarms >= 1);
        assert!(t.cell("a", "physics").losses < 30.0 * 0.9 + 1.0);
    }
}
