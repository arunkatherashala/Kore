//! The learner's tunable parameters. These are what the evolution loop
//! versions as a rule set (entity_type `rule`, key `learner-params`).

use serde_json::{json, Value};

pub const RULE_KEY: &str = "learner-params";
pub const Z95: f64 = 1.959_963_984_540_054;

#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// A context with fewer occasions than this is reported as insufficient data.
    pub habit_min_n: u32,
    /// A habit needs a Wilson 95% lower bound at or above this.
    pub habit_lb: f64,
    /// Only the newest this-many days of events count for habits (0 = all).
    pub habit_window_days: u32,
    /// Change point: minimum 2*log-likelihood-ratio.
    pub change_llr: f64,
    /// Change point: each side must span at least this many days.
    pub change_min_seg: u32,
    /// Change point: the larger daily rate must be at least this multiple of the smaller (floor 0.1/day).
    pub change_min_ratio: f64,
    /// Topics with fewer events than this are reported as insufficient data.
    pub change_min_total: u32,
    /// Lift test: the in-context Wilson lower bound must exceed the Wilson upper bound of the action's
    /// rate in the other contexts of the same family by more than this margin. Negative switches it off.
    pub habit_lift: f64,
    /// A rule-derived context that holds on more than this share of its units (days for weekday/weekend,
    /// events for keyword contexts) is dropped as uninformative. 1.0 switches it off.
    pub habit_max_share: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params { habit_min_n: 8, habit_lb: 0.5, habit_window_days: 0, change_llr: 12.0, change_min_seg: 10, change_min_ratio: 1.8, change_min_total: 10, habit_lift: 0.0, habit_max_share: 0.9 }
    }
}

impl Params {
    pub fn to_json(&self) -> Value {
        json!({
            "habit_min_n": self.habit_min_n, "habit_lb": self.habit_lb, "habit_window_days": self.habit_window_days,
            "change_llr": self.change_llr, "change_min_seg": self.change_min_seg, "change_min_ratio": self.change_min_ratio,
            "change_min_total": self.change_min_total, "habit_lift": self.habit_lift, "habit_max_share": self.habit_max_share,
        })
    }
    /// Canonical single-line text (keys sorted), used as the revision stance.
    pub fn canonical(&self) -> String {
        self.to_json().to_string()
    }
    pub fn from_json(v: &Value) -> Option<Params> {
        let d = Params::default();
        let u = |k: &str, dv: u32| -> Option<u32> {
            match v.get(k) {
                None => Some(dv),
                Some(x) => x.as_u64().and_then(|n| u32::try_from(n).ok()),
            }
        };
        let f = |k: &str, dv: f64| -> Option<f64> {
            match v.get(k) {
                None => Some(dv),
                Some(x) => x.as_f64().filter(|n| n.is_finite()),
            }
        };
        let p = Params {
            habit_min_n: u("habit_min_n", d.habit_min_n)?,
            habit_lb: f("habit_lb", d.habit_lb)?,
            habit_window_days: u("habit_window_days", d.habit_window_days)?,
            change_llr: f("change_llr", d.change_llr)?,
            change_min_seg: u("change_min_seg", d.change_min_seg)?,
            change_min_ratio: f("change_min_ratio", d.change_min_ratio)?,
            change_min_total: u("change_min_total", d.change_min_total)?,
            habit_lift: f("habit_lift", d.habit_lift)?,
            habit_max_share: f("habit_max_share", d.habit_max_share)?,
        };
        p.validate().ok()?;
        Some(p)
    }
    pub fn from_canonical(s: &str) -> Option<Params> {
        serde_json::from_str::<Value>(s).ok().and_then(|v| Params::from_json(&v))
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.habit_min_n < 1 || self.habit_min_n > 100_000 {
            return Err("habit_min_n must be 1..=100000".into());
        }
        if !(0.0..=1.0).contains(&self.habit_lb) {
            return Err("habit_lb must be within 0..=1".into());
        }
        if self.habit_window_days > 36_500 {
            return Err("habit_window_days too large".into());
        }
        if !(-1.0..=1.0).contains(&self.habit_lift) {
            return Err("habit_lift must be within -1..=1 (negative = off)".into());
        }
        if !(0.0..=1.0).contains(&self.habit_max_share) {
            return Err("habit_max_share must be within 0..=1".into());
        }
        if self.change_llr < 0.0 || self.change_min_seg < 2 || self.change_min_ratio < 1.0 || self.change_min_total < 2 {
            return Err("change parameters out of range".into());
        }
        Ok(())
    }
    /// Short label for rationales.
    pub fn label(&self) -> String {
        format!("min_n={} lb>={:.2} window={}d", self.habit_min_n, self.habit_lb, self.habit_window_days)
    }
}

/// Wilson score interval lower bound for k successes in n trials (95%).
pub fn wilson_lower(k: u32, n: u32) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let (k, n) = (k as f64, n as f64);
    let p = k / n;
    let z2 = Z95 * Z95;
    let centre = p + z2 / (2.0 * n);
    let margin = Z95 * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    ((centre - margin) / (1.0 + z2 / n)).clamp(0.0, 1.0)
}

/// Wilson score interval upper bound for k successes in n trials (95%).
pub fn wilson_upper(k: u32, n: u32) -> f64 {
    if n == 0 {
        return 1.0;
    }
    (1.0 - wilson_lower(n - k.min(n), n)).clamp(0.0, 1.0)
}

/// Wilson-adjusted proportion `(k + z^2/2) / (n + z^2)`: a frequency pulled
/// toward 1/2 when n is small. Used as the baseline probability in the ledger.
pub fn wilson_smoothed(k: u32, n: u32) -> f64 {
    let z2 = Z95 * Z95;
    ((k as f64 + z2 / 2.0) / (n as f64 + z2)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wilson_known_values() {
        assert!((wilson_lower(8, 10) - 0.4902).abs() < 1e-3);
        assert!((wilson_lower(80, 100) - 0.7112).abs() < 1e-3);
        assert_eq!(wilson_lower(0, 0), 0.0);
        assert!(wilson_lower(5, 5) < 1.0);
        assert!(wilson_smoothed(0, 0) == 0.5);
    }
    #[test]
    fn params_round_trip() {
        let p = Params { habit_min_n: 12, habit_lb: 0.55, ..Params::default() };
        assert_eq!(Params::from_canonical(&p.canonical()), Some(p));
        assert!(Params::from_canonical("{\"habit_lb\": 2.0}").is_none());
    }
}
