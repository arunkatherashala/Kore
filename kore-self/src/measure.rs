//! Scores the learners against synthetic ground truth (see `synth.rs`).
//! Shared by the unit test (thresholds) and `examples/measure_learning.rs`
//! (prints the numbers recorded in `docs/measured.md`).
//!
//! Definitions
//! * Habits: truth = planted (context, action) with p >= 0.6; detected =
//!   habits returned by `habits::detect`. Precision = TP / detected,
//!   recall = TP / truth.
//! * Change points: a detection matches a planted change if the topic is the
//!   same and the detected day is within `CHANGE_TOL` days. Precision and
//!   recall as above; `abs_error` = mean |detected - planted| days.
//!   Delay = days after the planted change at which the detector, run on the
//!   data truncated at that day, first reports a matching change. False
//!   positive rate = share of unchanged topics with any detection on the full
//!   series.
//! * Stances: event-level; a detection is correct if the cited event is a
//!   planted statement. Key accuracy = share of true detections whose topic
//!   equals the planted topic.
//! * min-n refusal: of the planted rare contexts (fewer occasions than
//!   min n), the share reported as insufficient data AND not turned into a
//!   habit; with min n = 1 the same contexts would have produced habits.
#![allow(dead_code)]

use crate::learn::{changes, habits, stances, Params};
use crate::synth::{self, SynthConfig, Truth};
use std::collections::BTreeSet;

pub const CHANGE_TOL: i64 = 7;

#[derive(Debug, Clone, Default)]
pub struct RunMetrics {
    pub habit_precision: f64,
    pub habit_recall: f64,
    pub habit_detected: usize,
    pub change_precision: f64,
    pub change_recall: f64,
    pub change_abs_error: f64,
    /// Mean delay over detected changes (days); None if none detected.
    pub change_delay: Option<f64>,
    pub change_fp_rate: f64,
    pub stance_precision: f64,
    pub stance_recall: f64,
    pub stance_key_accuracy: f64,
    pub minn_refused: f64,
    pub minn1_would_derive: f64,
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 {
        1.0
    } else {
        a as f64 / b as f64
    }
}

pub fn run_one(cfg: &SynthConfig, p: &Params) -> RunMetrics {
    let (events, truth) = synth::generate(cfg);
    let evs = synth::to_evs(&events);
    score(&evs, &truth, p)
}

pub fn score(evs: &[crate::learn::Ev], truth: &Truth, p: &Params) -> RunMetrics {
    let mut m = RunMetrics::default();
    // habits
    let (hs, insufficient) = habits::detect(evs, p);
    let det: BTreeSet<(String, String)> = hs.iter().map(|h| (h.context.clone(), h.action.clone())).collect();
    let tru: BTreeSet<(String, String)> = truth.habits.iter().map(|h| (h.0.clone(), h.1.clone())).collect();
    let tp = det.intersection(&tru).count();
    m.habit_detected = det.len();
    m.habit_precision = ratio(tp, det.len());
    m.habit_recall = ratio(tp, tru.len());
    // min-n refusal
    let refused = truth.rare_contexts.iter().filter(|c| insufficient.iter().any(|i| &i.context == *c) && !det.iter().any(|d| &d.0 == *c)).count();
    m.minn_refused = ratio(refused, truth.rare_contexts.len());
    let p1 = Params { habit_min_n: 1, ..p.clone() };
    let (h1, _) = habits::detect(evs, &p1);
    let would = truth.rare_contexts.iter().filter(|c| h1.iter().any(|h| &h.context == *c)).count();
    m.minn1_would_derive = ratio(would, truth.rare_contexts.len());
    // change points
    let (first, series) = changes::daily_counts(evs);
    let mut found: Vec<(String, changes::Change)> = Vec::new();
    for (t, counts) in &series {
        if counts.iter().sum::<u32>() < p.change_min_total {
            continue;
        }
        for c in changes::detect_series(counts, p, first) {
            found.push((t.clone(), c));
        }
    }
    let mut matched_truth = 0usize;
    let mut matched_found = BTreeSet::new();
    let (mut err_sum, mut delays) = (0i64, Vec::new());
    for tc in &truth.changes {
        let hit = found.iter().enumerate().filter(|(_, (t, c))| *t == tc.topic && (c.day - tc.day).abs() <= CHANGE_TOL).min_by_key(|(_, (_, c))| (c.day - tc.day).abs());
        if let Some((i, (_, c))) = hit {
            matched_truth += 1;
            matched_found.insert(i);
            err_sum += (c.day - tc.day).abs();
            // delay: truncate the series at successive days
            let counts = &series[&tc.topic];
            let start = (tc.day - first).max(0) as usize;
            for end in start..counts.len() {
                let cut = &counts[..=end];
                if changes::detect_series(cut, p, first).iter().any(|c| (c.day - tc.day).abs() <= CHANGE_TOL) {
                    delays.push((end - start) as f64);
                    break;
                }
            }
        }
    }
    m.change_precision = ratio(matched_found.len(), found.len());
    m.change_recall = ratio(matched_truth, truth.changes.len());
    m.change_abs_error = if matched_truth == 0 { 0.0 } else { err_sum as f64 / matched_truth as f64 };
    m.change_delay = if delays.is_empty() { None } else { Some(delays.iter().sum::<f64>() / delays.len() as f64) };
    let fp_topics = truth.steady_topics.iter().filter(|t| found.iter().any(|(ft, _)| ft == *t)).count();
    m.change_fp_rate = ratio(fp_topics, truth.steady_topics.len());
    // stances
    let cues = stances::extract(evs);
    let truth_ids: BTreeSet<&str> = truth.stances.iter().map(|s| s.event_id.as_str()).collect();
    let det_ids: BTreeSet<&str> = cues.iter().map(|c| c.event_id.as_str()).collect();
    let tp_s = det_ids.intersection(&truth_ids).count();
    m.stance_precision = ratio(tp_s, det_ids.len());
    m.stance_recall = ratio(tp_s, truth_ids.len());
    let key_ok = cues.iter().filter(|c| truth.stances.iter().any(|s| s.event_id == c.event_id && s.topic == c.topic)).count();
    m.stance_key_accuracy = ratio(key_ok.min(tp_s), tp_s);
    m
}

#[derive(Debug, Clone, Copy)]
pub struct Agg {
    pub min: f64,
    pub median: f64,
    pub mean: f64,
    pub max: f64,
}

pub fn agg(mut v: Vec<f64>) -> Option<Agg> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    Some(Agg { min: v[0], median: v[v.len() / 2], mean, max: v[v.len() - 1] })
}

#[derive(Debug, Clone)]
pub struct Row {
    pub noise: f64,
    pub seeds: Vec<u64>,
    pub runs: Vec<RunMetrics>,
}

impl Row {
    pub fn col(&self, f: impl Fn(&RunMetrics) -> Option<f64>) -> Option<Agg> {
        agg(self.runs.iter().filter_map(f).collect())
    }
}

pub const SEEDS: [u64; 20] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
pub const NOISE: [f64; 3] = [0.0, 0.2, 0.5];

pub fn sweep(seeds: &[u64], noise: &[f64], p: &Params) -> Vec<Row> {
    noise
        .iter()
        .map(|&n| Row { noise: n, seeds: seeds.to_vec(), runs: seeds.iter().map(|&s| run_one(&synth::default_config(s, n), p)).collect() })
        .collect()
}
