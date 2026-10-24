//! Habits: how often an action is taken in a context.
//!
//! Convention: an event tagged `ctx:<C>` is one occasion of context C; the
//! action taken is its `act:<A>` tag (`(none)` when absent). For each
//! (C, A): k = occasions with action A, n = all occasions of C. A habit needs
//! n >= `habit_min_n` and a Wilson 95% lower bound of k/n >= `habit_lb`.
//! Contexts with fewer than `habit_min_n` occasions are refused and listed as
//! insufficient data; they are never turned into a habit.

use super::params::{wilson_lower, wilson_upper, Params};
use super::tagrules::{builtin_context, TagRules, WEEKDAY, WEEKEND};
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
    /// (k_out, n_out, Wilson upper bound out of context) when the lift test was made.
    pub lift: Option<(u32, u32, f64)>,
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
pub struct Occ {
    pub n: u32,
    pub from_rules: bool,
    /// Share of all units (days for built-in contexts, events for keyword
    /// contexts) on which the context holds; None for tag-derived contexts.
    pub share: Option<f64>,
    /// 0 stored tags, 1 built-in date contexts, 2 keyword contexts: the
    /// out-of-context comparison stays inside one family.
    pub family: u8,
    pub acts: BTreeMap<String, (u32, Vec<(String, String)>)>,
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
    let by_day_len = by_day.len();
    let n_events: usize = by_day.values().map(|v| v.len()).sum();
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
    let total_days = by_day_len as f64;
    let total_events = n_events as f64;
    for (c, o) in out.iter_mut() {
        let builtin = c == WEEKDAY || c == WEEKEND;
        o.family = if builtin { 1 } else { 2 };
        let total = if builtin { total_days } else { total_events };
        o.share = Some(if total > 0.0 { o.n as f64 / total } else { 0.0 });
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

/// All occasions per context: stored `ctx:`/`act:` tags (event-level) plus,
/// with rules, the day- and sentence-level occasions described above. A context
/// name present in both sources has its counts added.
pub fn occasions(events: &[Ev], window_days: u32, rules: Option<&TagRules>) -> BTreeMap<String, Occ> {
    let mut all: BTreeMap<String, Occ> = BTreeMap::new();
    for (ctx, (n, acts)) in tally(events, window_days) {
        let o = all.entry(ctx).or_default();
        o.n += n;
        for (a, occ) in acts {
            let slot = o.acts.entry(a).or_default();
            slot.0 += occ.len() as u32;
            slot.1.extend(occ);
        }
    }
    if let Some(r) = rules {
        for (ctx, d) in day_tally(events, window_days, r) {
            let o = all.entry(ctx).or_default();
            if o.n == 0 {
                o.family = d.family;
                o.share = d.share;
            }
            o.n += d.n;
            o.from_rules = true;
            for (a, v) in d.acts {
                let slot = o.acts.entry(a).or_default();
                slot.0 += v.0;
                slot.1.extend(v.1);
            }
        }
    }
    all
}

/// A context dropped because it holds on more than `habit_max_share` of its units.
#[derive(Debug, Clone, PartialEq)]
pub struct Uninformative {
    pub context: String,
    pub share: f64,
    pub max_share: f64,
}

/// A candidate habit that passed the Wilson bound but not the lift test.
#[derive(Debug, Clone, PartialEq)]
pub struct LiftRejected {
    pub context: String,
    pub action: String,
    pub k: u32,
    pub n: u32,
    pub lower: f64,
    pub k_out: u32,
    pub n_out: u32,
    pub upper_out: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Detection {
    pub habits: Vec<Habit>,
    pub insufficient: Vec<Insufficient>,
    pub uninformative: Vec<Uninformative>,
    pub lift_rejected: Vec<LiftRejected>,
}

/// Like `detect`, additionally counting occasions derived from keyword
/// `rules`. With `None`, or rules without any `action:` rule, the occasions
/// are exactly those of `detect`.
pub fn detect_with(events: &[Ev], p: &Params, rules: Option<&TagRules>) -> (Vec<Habit>, Vec<Insufficient>) {
    let d = detect_full(events, p, rules);
    (d.habits, d.insufficient)
}

/// The detector with everything it refused and why.
///
/// A habit (context C, action A) needs, in this order: n >= `habit_min_n`
/// occasions of C (else listed as insufficient); C not holding on more than
/// `habit_max_share` of its units (rule-derived contexts only: a context true
/// almost everywhere says nothing about when A happens); the Wilson 95% lower
/// bound of k/n >= `habit_lb`; and the lift test: that lower bound must exceed
/// the Wilson upper bound of A's rate in all OTHER contexts of the same family
/// (stored tags / built-in date contexts / keyword contexts) plus
/// `habit_lift` (a negative `habit_lift` switches the test off). A rate that
/// is no different outside the context is the action's base rate, not a habit
/// of the context. With fewer than `habit_min_n` out-of-context occasions the
/// test cannot be made and the habit is kept (nothing to compare with).
pub fn detect_full(events: &[Ev], p: &Params, rules: Option<&TagRules>) -> Detection {
    let all = occasions(events, p.habit_window_days, rules);
    let mut out = Detection::default();
    for (ctx, o) in &all {
        if o.n < p.habit_min_n {
            out.insufficient.push(Insufficient { context: ctx.clone(), n: o.n, min_n: p.habit_min_n });
            continue;
        }
        if let Some(sh) = o.share {
            if sh > p.habit_max_share {
                out.uninformative.push(Uninformative { context: ctx.clone(), share: sh, max_share: p.habit_max_share });
                continue;
            }
        }
        for (act, (k, occ)) in &o.acts {
            if act == NO_ACTION {
                continue;
            }
            let lower = wilson_lower(*k, o.n);
            if lower < p.habit_lb {
                continue;
            }
            let mut lift = None;
            if p.habit_lift >= 0.0 {
                let (mut k_out, mut n_out) = (0u32, 0u32);
                for (c2, o2) in &all {
                    if c2 != ctx && o2.family == o.family {
                        n_out += o2.n;
                        k_out += o2.acts.get(act).map_or(0, |v| v.0);
                    }
                }
                if n_out >= p.habit_min_n {
                    let upper = wilson_upper(k_out, n_out);
                    if lower <= upper + p.habit_lift {
                        out.lift_rejected.push(LiftRejected { context: ctx.clone(), action: act.clone(), k: *k, n: o.n, lower, k_out, n_out, upper_out: upper });
                        continue;
                    }
                    lift = Some((k_out, n_out, upper));
                }
            }
            let mut occ = occ.clone();
            if o.from_rules {
                occ.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
                occ.dedup();
            }
            let last_ts = occ.iter().map(|x| x.1.clone()).max().unwrap_or_default();
            out.habits.push(Habit {
                context: ctx.clone(),
                action: act.clone(),
                k: *k,
                n: o.n,
                rate: *k as f64 / o.n as f64,
                lower,
                cited: occ.iter().map(|x| x.0.clone()).collect(),
                last_ts,
                from_rules: o.from_rules,
                lift,
            });
        }
    }
    out
}

/// Trivial baseline for backtests: in every context seen at least once, the
/// most frequent action so far (ties: alphabetical). No minimum n, no bound.
pub fn modal_baseline(events: &[Ev], window_days: u32) -> Vec<(String, String)> {
    modal_baseline_with(events, window_days, None)
}

pub fn modal_baseline_with(events: &[Ev], window_days: u32, rules: Option<&TagRules>) -> Vec<(String, String)> {
    let all = occasions(events, window_days, rules);
    let mut out = Vec::new();
    for (ctx, o) in &all {
        let best = o.acts.iter().filter(|(a, _)| a.as_str() != NO_ACTION).max_by(|x, y| x.1 .0.cmp(&y.1 .0).then_with(|| y.0.cmp(x.0)));
        if let Some((a, _)) = best {
            out.push((ctx.clone(), a.clone()));
        }
    }
    out
}
