//! Habits: how often an action is taken in a context.
//!
//! Convention: an event tagged `ctx:<C>` is one occasion of context C; the
//! action taken is its `act:<A>` tag (`(none)` when absent). For each
//! (C, A): k = occasions with action A, n = all occasions of C. A habit needs
//! n >= `habit_min_n` and a Wilson 95% lower bound of k/n >= `habit_lb`.
//! Contexts with fewer than `habit_min_n` occasions are refused and listed as
//! insufficient data; they are never turned into a habit.

use super::params::{wilson_lower, Params};
use super::tagrules::{builtin_context, TagRules};
use super::{Ev, ACT_PREFIX, CTX_PREFIX, NO_ACTION};
use std::collections::{BTreeMap, BTreeSet};

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
    /// True when day-level occasions from keyword rules contributed.
    pub from_rules: bool,
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
    detect_with(events, p, None)
}

/// Occasions of one context: `n` occasions, per action the number of
/// occasions with it and the (id, ts) of the events that show it.
#[derive(Debug, Clone, Default)]
struct Occ {
    n: u32,
    from_rules: bool,
    acts: BTreeMap<String, (u32, Vec<(String, String)>)>,
}

/// Occasions derived from keyword rules (free-text events).
///
/// * Built-in `weekday`/`weekend`: day-level. An occasion is a calendar day
///   (UTC date of `occurred_at`) with at least one event; the action A was
///   taken if an `action:A` rule matches (not negated) in any event of that
///   day; k counts such days. Days without events are not occasions (no entry
///   is not evidence of "did nothing").
/// * Keyword contexts `ctx:C`: sentence-level. Each event with a sentence
///   matching C is one occasion of C; A was taken if an action rule matches in
///   a sentence that also matches C. A day-level reading of keyword contexts
///   was tried first and measured badly on dense logs (every action appears
///   on most days, so every context "has" it), see docs/measured.md.
///
/// The cited events are the ones that matched the action.
fn day_tally(events: &[Ev], window_days: u32, rules: &TagRules) -> BTreeMap<String, Occ> {
    let mut out: BTreeMap<String, Occ> = BTreeMap::new();
    if !rules.has_actions() {
        return out;
    }
    let newest = events.iter().map(|e| e.day).max().unwrap_or(0);
    let mut by_day: BTreeMap<i64, Vec<&Ev>> = BTreeMap::new();
    for e in events {
        if window_days > 0 && e.day <= newest - window_days as i64 {
            continue;
        }
        by_day.entry(e.day).or_default().push(e);
    }
    for (day, evs) in by_day {
        let mut acts: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for e in &evs {
            let sents = rules.classify_sentences(&e.text);
            // keyword contexts: one occasion per event per context
            let mut per_ctx: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for s in &sents {
                for c in &s.ctxs {
                    per_ctx.entry(c.clone()).or_default().extend(s.actions.iter().cloned());
                }
            }
            for (c, a) in per_ctx {
                let o = out.entry(c).or_default();
                o.n += 1;
                o.from_rules = true;
                if a.is_empty() {
                    let slot = o.acts.entry(NO_ACTION.to_string()).or_default();
                    slot.0 += 1;
                    slot.1.push((e.id.clone(), e.ts.clone()));
                }
                for act in a {
                    let slot = o.acts.entry(act).or_default();
                    slot.0 += 1;
                    slot.1.push((e.id.clone(), e.ts.clone()));
                }
            }
            for s in &sents {
                for a in &s.actions {
                    let v = acts.entry(a.clone()).or_default();
                    if v.last().map_or(true, |l| l.0 != e.id) {
                        v.push((e.id.clone(), e.ts.clone()));
                    }
                }
            }
        }
        if rules.builtin {
            let o = out.entry(builtin_context(day).to_string()).or_default();
            o.n += 1;
            o.from_rules = true;
            if acts.is_empty() {
                let first = &evs[0];
                let slot = o.acts.entry(NO_ACTION.to_string()).or_default();
                slot.0 += 1;
                slot.1.push((first.id.clone(), first.ts.clone()));
            } else {
                for (a, ids) in &acts {
                    let slot = o.acts.entry(a.clone()).or_default();
                    slot.0 += 1;
                    slot.1.extend(ids.iter().cloned());
                }
            }
        }
    }
    out
}

/// Summary of the day-level occasions, for `self_tag_rules_check`:
/// context -> (days, action -> days with it).
pub fn day_summary(events: &[Ev], rules: &TagRules) -> BTreeMap<String, (u32, BTreeMap<String, u32>)> {
    day_tally(events, 0, rules)
        .into_iter()
        .map(|(c, o)| (c, (o.n, o.acts.into_iter().filter(|(a, _)| a != NO_ACTION).map(|(a, v)| (a, v.0)).collect())))
        .collect()
}

/// Like `detect`, additionally counting day-level occasions derived from
/// keyword `rules` (stored `ctx:`/`act:` tags are still read as before; a
/// context name present in both sources has its counts added). With `None`,
/// or rules without any `action:` rule, the result is exactly that of `detect`.
pub fn detect_with(events: &[Ev], p: &Params, rules: Option<&TagRules>) -> (Vec<Habit>, Vec<Insufficient>) {
    let mut all: BTreeMap<String, Occ> = BTreeMap::new();
    for (ctx, (n, acts)) in tally(events, p.habit_window_days) {
        let o = all.entry(ctx).or_default();
        o.n += n;
        for (a, occ) in acts {
            let slot = o.acts.entry(a).or_default();
            slot.0 += occ.len() as u32;
            slot.1.extend(occ);
        }
    }
    if let Some(r) = rules {
        for (ctx, d) in day_tally(events, p.habit_window_days, r) {
            let o = all.entry(ctx).or_default();
            o.n += d.n;
            o.from_rules = true;
            for (a, v) in d.acts {
                let slot = o.acts.entry(a).or_default();
                slot.0 += v.0;
                slot.1.extend(v.1);
            }
        }
    }
    let (mut habits, mut insufficient) = (Vec::new(), Vec::new());
    for (ctx, o) in &all {
        if o.n < p.habit_min_n {
            insufficient.push(Insufficient { context: ctx.clone(), n: o.n, min_n: p.habit_min_n });
            continue;
        }
        for (act, (k, occ)) in &o.acts {
            if act == NO_ACTION {
                continue;
            }
            let lower = wilson_lower(*k, o.n);
            if lower >= p.habit_lb {
                let mut occ = occ.clone();
                if o.from_rules {
                    occ.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
                    occ.dedup();
                }
                let last_ts = occ.iter().map(|x| x.1.clone()).max().unwrap_or_default();
                habits.push(Habit {
                    context: ctx.clone(),
                    action: act.clone(),
                    k: *k,
                    n: o.n,
                    rate: *k as f64 / o.n as f64,
                    lower,
                    cited: occ.iter().map(|x| x.0.clone()).collect(),
                    last_ts,
                    from_rules: o.from_rules,
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
