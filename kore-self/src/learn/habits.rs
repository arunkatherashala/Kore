//! Habits: how often an action is taken in a context.
//!
//! Convention: an event tagged `ctx:<C>` is one occasion of context C; the
//! action taken is its `act:<A>` tag (`(none)` when absent). For each
//! (C, A): k = occasions with action A, n = all occasions of C. A habit needs
//! n >= `habit_min_n` and a Wilson 95% lower bound of k/n >= `habit_lb`.
//! Contexts with fewer than `habit_min_n` occasions are refused and listed as
//! insufficient data; they are never turned into a habit.

use super::params::{wilson_lower, Params};
use super::{Ev, ACT_PREFIX, CTX_PREFIX, NO_ACTION};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Habit {
    pub context: String,
    pub action: String,
    pub k: u32,
    pub n: u32,
    pub rate: f64,
    pub lower: f64,
    /// Ids of the occasions that took this action (the evidence), in time order.
    pub cited: Vec<String>,
    /// `occurred_at` of the newest cited event.
    pub last_ts: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Insufficient {
    pub context: String,
    pub n: u32,
    pub min_n: u32,
}

/// Per context: total occasions and, per action, the (id, ts) of each occasion.
pub type Tally = BTreeMap<String, (u32, BTreeMap<String, Vec<(String, String)>>)>;

/// Count occasions per (context, action), honouring the window: only events
/// within `window_days` days of the newest event are used (0 = all).
pub fn tally(events: &[Ev], window_days: u32) -> Tally {
    let newest = events.iter().map(|e| e.day).max().unwrap_or(0);
    let mut t: Tally = BTreeMap::new();
    for e in events {
        if window_days > 0 && e.day <= newest - window_days as i64 {
            continue;
        }
        let Some(c) = e.tag_val(CTX_PREFIX) else { continue };
        let a = e.tag_val(ACT_PREFIX).unwrap_or(NO_ACTION);
        let slot = t.entry(c.to_string()).or_insert_with(|| (0, BTreeMap::new()));
        slot.0 += 1;
        slot.1.entry(a.to_string()).or_default().push((e.id.clone(), e.ts.clone()));
    }
    t
}

pub fn detect(events: &[Ev], p: &Params) -> (Vec<Habit>, Vec<Insufficient>) {
    let t = tally(events, p.habit_window_days);
    let (mut habits, mut insufficient) = (Vec::new(), Vec::new());
    for (ctx, (n, acts)) in &t {
        if *n < p.habit_min_n {
            insufficient.push(Insufficient { context: ctx.clone(), n: *n, min_n: p.habit_min_n });
            continue;
        }
        for (act, occ) in acts {
            if act == NO_ACTION {
                continue;
            }
            let k = occ.len() as u32;
            let lower = wilson_lower(k, *n);
            if lower >= p.habit_lb {
                let last_ts = occ.iter().map(|o| o.1.clone()).max().unwrap_or_default();
                habits.push(Habit {
                    context: ctx.clone(),
                    action: act.clone(),
                    k,
                    n: *n,
                    rate: k as f64 / *n as f64,
                    lower,
                    cited: occ.iter().map(|o| o.0.clone()).collect(),
                    last_ts,
                });
            }
        }
    }
    (habits, insufficient)
}

/// Trivial baseline for backtests: in every context seen at least once, the
/// most frequent action so far (ties: alphabetical). No minimum n, no bound.
pub fn modal_baseline(events: &[Ev], window_days: u32) -> Vec<(String, String)> {
    let t = tally(events, window_days);
    let mut out = Vec::new();
    for (ctx, (_, acts)) in &t {
        let best = acts.iter().filter(|(a, _)| a.as_str() != NO_ACTION).max_by(|x, y| x.1.len().cmp(&y.1.len()).then_with(|| y.0.cmp(x.0)));
        if let Some((a, _)) = best {
            out.push((ctx.clone(), a.clone()));
        }
    }
    out
}
