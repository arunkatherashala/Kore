//! Topic change points.
//!
//! Convention: an event tagged `topic:<T>` is one mention of topic T. Per
//! topic, mentions are counted per calendar day over the whole span of the
//! log (days without a mention count as zero). A change is found with a
//! Poisson likelihood-ratio scan: for every split, 2*(LL(two rates) -
//! LL(one rate)); the best split is accepted if the statistic reaches
//! `change_llr`, both sides span at least `change_min_seg` days and the rates
//! differ by `change_min_ratio`. Binary segmentation then looks inside both
//! sides (at most `MAX_CHANGES` per topic).
//!
//! The statistic assumes Poisson counts. Real writing habits are burstier, so
//! the false-positive rate measured on synthetic Poisson data is a floor, not
//! an estimate for real logs.

use super::params::Params;
use super::{dates, Ev, TOPIC_PREFIX};
use std::collections::BTreeMap;

pub const MAX_CHANGES: usize = 3;
const RATE_FLOOR: f64 = 0.1;

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// Index into the day series where the new regime starts.
    pub offset: usize,
    pub day: i64,
    pub date: String,
    pub rate_before: f64,
    pub rate_after: f64,
    /// 2 * log-likelihood ratio.
    pub stat: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopicChanges {
    pub topic: String,
    pub total: u32,
    pub changes: Vec<Change>,
    /// All mentions of the topic (the evidence the scan used), in time order.
    pub cited: Vec<String>,
    pub last_ts: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InsufficientTopic {
    pub topic: String,
    pub total: u32,
    pub min_total: u32,
}

fn xlogx(x: f64) -> f64 {
    if x <= 0.0 {
        0.0
    } else {
        x * x.ln()
    }
}

/// Log-likelihood (up to a constant) of a constant-rate Poisson segment.
fn seg_ll(sum: f64, len: f64) -> f64 {
    xlogx(sum) - sum * len.ln()
}

/// Core scan over a daily count series. Returns changes sorted by offset.
pub fn detect_series(counts: &[u32], p: &Params, start_day: i64) -> Vec<Change> {
    let mut pre = vec![0.0f64; counts.len() + 1];
    for (i, c) in counts.iter().enumerate() {
        pre[i + 1] = pre[i] + *c as f64;
    }
    let mut out = Vec::new();
    segment(&pre, 0, counts.len(), p, start_day, &mut out);
    out.sort_by_key(|c| c.offset);
    out
}

fn segment(pre: &[f64], lo: usize, hi: usize, p: &Params, start_day: i64, out: &mut Vec<Change>) {
    let min_seg = p.change_min_seg as usize;
    if out.len() >= MAX_CHANGES || hi < lo + 2 * min_seg {
        return;
    }
    let total = pre[hi] - pre[lo];
    let len = (hi - lo) as f64;
    if total <= 0.0 {
        return;
    }
    let null = seg_ll(total, len);
    let mut best: Option<(usize, f64)> = None;
    for s in (lo + min_seg)..=(hi - min_seg) {
        let (s1, s2) = (pre[s] - pre[lo], pre[hi] - pre[s]);
        let stat = 2.0 * (seg_ll(s1, (s - lo) as f64) + seg_ll(s2, (hi - s) as f64) - null);
        if best.map_or(true, |(_, b)| stat > b + 1e-12) {
            best = Some((s, stat));
        }
    }
    let Some((s, stat)) = best else { return };
    if stat < p.change_llr {
        return;
    }
    let r1 = (pre[s] - pre[lo]) / (s - lo) as f64;
    let r2 = (pre[hi] - pre[s]) / (hi - s) as f64;
    let (big, small) = (r1.max(r2), r1.min(r2));
    if big < p.change_min_ratio * small.max(RATE_FLOOR) {
        return;
    }
    out.push(Change { offset: s, day: start_day + s as i64, date: dates::date_of(start_day + s as i64), rate_before: r1, rate_after: r2, stat });
    segment(pre, lo, s, p, start_day, out);
    segment(pre, s, hi, p, start_day, out);
}

/// Daily counts per topic over [first day, last day] of `events`.
pub fn daily_counts(events: &[Ev]) -> (i64, BTreeMap<String, Vec<u32>>) {
    let first = events.iter().map(|e| e.day).min().unwrap_or(0);
    let last = events.iter().map(|e| e.day).max().unwrap_or(0);
    let len = (last - first + 1).max(0) as usize;
    let mut m: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for e in events {
        for t in e.tags.iter().filter_map(|t| t.strip_prefix(TOPIC_PREFIX)) {
            m.entry(t.to_string()).or_insert_with(|| vec![0; len])[(e.day - first) as usize] += 1;
        }
    }
    (first, m)
}

/// Run the scan for every topic. Only topics with at least one change are returned.
pub fn detect(events: &[Ev], p: &Params) -> (Vec<TopicChanges>, Vec<InsufficientTopic>) {
    let (first, series) = daily_counts(events);
    let (mut found, mut insufficient) = (Vec::new(), Vec::new());
    for (topic, counts) in &series {
        let total: u32 = counts.iter().sum();
        if total < p.change_min_total {
            insufficient.push(InsufficientTopic { topic: topic.clone(), total, min_total: p.change_min_total });
            continue;
        }
        let changes = detect_series(counts, p, first);
        if changes.is_empty() {
            continue;
        }
        let tag = format!("{TOPIC_PREFIX}{topic}");
        let mentions: Vec<&Ev> = events.iter().filter(|e| e.has_tag(&tag)).collect();
        let last_ts = mentions.iter().map(|e| e.ts.clone()).max().unwrap_or_default();
        found.push(TopicChanges { topic: topic.clone(), total, changes, cited: mentions.iter().map(|e| e.id.clone()).collect(), last_ts });
    }
    (found, insufficient)
}
