//! The being's state, derived from its own log.
//!
//! Nothing here is stored anywhere except as events in the versioned log
//! (source `being:loop`). `BeingState::from_snapshot` folds those events, in
//! log order, into counts per domain, the remaining budget and the clock, and
//! the live loop updates the same structure with the same `apply` call. The
//! replay tests check that both routes give the same state and therefore the
//! same next decision.

use super::world::{iso_at, Domain, DOMAINS};
use crate::versioned::{event_id, EventRow, NewEvent, Snapshot};
use std::collections::VecDeque;

pub const SOURCE: &str = "being:loop";
/// A decision cites at most this many of the latest outcome events per domain
/// (plus the genesis event). The counts it used come from all outcomes.
pub const EVIDENCE_WINDOW: usize = 5;
/// Efficiency (reward points per energy unit) is divided by this to land
/// roughly in 0..1 before the exploration bonus is added.
pub const EFF_NORM: f64 = 10.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DomainStat {
    pub n: u64,
    pub correct: u64,
    pub reward: i64,
    pub cost: i64,
}

impl DomainStat {
    /// Reward points per energy unit spent (the quantity the being is paid in).
    pub fn efficiency(&self) -> f64 {
        if self.cost > 0 {
            self.reward as f64 / self.cost as f64
        } else {
            0.0
        }
    }
    pub fn value(&self) -> f64 {
        self.efficiency() / EFF_NORM
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Genesis {
    pub world_seed: u64,
    pub loop_seed: u64,
    pub budget: i64,
    pub explore_c: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub step: u64,
    pub domain: Domain,
    pub task_id: String,
    pub difficulty: u8,
    pub correct: bool,
    pub reward: i64,
    pub cost: i64,
    pub budget_left: i64,
    pub clock: i64,
    pub event_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct BeingState {
    pub genesis: Option<Genesis>,
    pub genesis_event: String,
    /// Completed steps (outcome events).
    pub step: u64,
    pub budget_left: i64,
    /// World seconds since the origin.
    pub clock: i64,
    pub stats: [DomainStat; 4],
    /// Latest outcome event ids per domain, oldest first.
    pub recent: [VecDeque<String>; 4],
    pub total_reward: i64,
    pub total_cost: i64,
    pub refills: u64,
}

/// Value of `key:` among comma-joined tags.
pub fn tag<'a>(tags: &'a str, key: &str) -> Option<&'a str> {
    tags.split(',').find_map(|t| t.strip_prefix(key).and_then(|r| r.strip_prefix(':')))
}

fn has_tag(tags: &str, key: &str) -> bool {
    tags.split(',').any(|t| t == key)
}

impl BeingState {
    pub fn total_pulls(&self) -> u64 {
        self.stats.iter().map(|s| s.n).sum()
    }

    pub fn start(&mut self, g: Genesis, event: String) {
        self.budget_left = g.budget;
        self.genesis = Some(g);
        self.genesis_event = event;
    }

    pub fn apply(&mut self, o: &Outcome) {
        let s = &mut self.stats[o.domain.idx()];
        s.n += 1;
        s.correct += o.correct as u64;
        s.reward += o.reward;
        s.cost += o.cost;
        self.total_reward += o.reward;
        self.total_cost += o.cost;
        self.budget_left = o.budget_left;
        self.clock = o.clock;
        self.step += 1;
        let q = &mut self.recent[o.domain.idx()];
        q.push_back(o.event_id.clone());
        while q.len() > EVIDENCE_WINDOW {
            q.pop_front();
        }
    }

    pub fn apply_refill(&mut self, budget_left: i64, clock: i64) {
        self.budget_left = budget_left;
        self.clock = clock;
        self.refills += 1;
    }

    /// The events a decision made now would cite.
    pub fn evidence_ids(&self) -> Vec<String> {
        let mut v = vec![self.genesis_event.clone()];
        for d in DOMAINS {
            v.extend(self.recent[d.idx()].iter().cloned());
        }
        v
    }

    /// Fold the being's events (log order) into a state.
    pub fn from_snapshot(snap: &Snapshot) -> BeingState {
        Self::from_events(&snap.rows::<EventRow>())
    }

    pub fn from_events(rows: &[EventRow]) -> BeingState {
        let mut st = BeingState::default();
        for e in rows.iter().filter(|e| e.source == SOURCE) {
            st.fold_event(e);
        }
        st
    }

    /// Apply one logged event (genesis, refill or outcome); others are ignored.
    pub fn fold_event(&mut self, e: &EventRow) {
        if e.source != SOURCE {
            return;
        }
        if e.kind == "note" && has_tag(&e.tags, "genesis") {
            if let Some(g) = genesis_from_tags(&e.tags) {
                if self.genesis.is_none() {
                    self.start(g, e.event_id.clone());
                }
            }
        } else if e.kind == "note" && has_tag(&e.tags, "refill") {
            let left = tag(&e.tags, "budget_left").and_then(|v| v.parse().ok()).unwrap_or(0);
            let clock = tag(&e.tags, "clock").and_then(|v| v.parse().ok()).unwrap_or(self.clock);
            self.apply_refill(left, clock);
        } else if e.kind == "outcome" {
            if let Some(o) = outcome_from_event(e) {
                self.apply(&o);
            }
        }
    }
}

fn genesis_from_tags(tags: &str) -> Option<Genesis> {
    Some(Genesis {
        world_seed: tag(tags, "world_seed")?.parse().ok()?,
        loop_seed: tag(tags, "loop_seed")?.parse().ok()?,
        budget: tag(tags, "budget")?.parse().ok()?,
        explore_c: tag(tags, "explore_c")?.parse().ok()?,
    })
}

pub fn outcome_from_event(e: &EventRow) -> Option<Outcome> {
    let t = &e.tags;
    Some(Outcome {
        step: tag(t, "step")?.parse().ok()?,
        domain: Domain::parse(tag(t, "domain")?)?,
        task_id: tag(t, "task")?.to_string(),
        difficulty: tag(t, "difficulty")?.parse().ok()?,
        correct: tag(t, "result")? == "correct",
        reward: tag(t, "reward")?.parse().ok()?,
        cost: tag(t, "cost")?.parse().ok()?,
        budget_left: tag(t, "budget_left")?.parse().ok()?,
        clock: tag(t, "clock")?.parse().ok()?,
        event_id: e.event_id.clone(),
    })
}

pub fn genesis_event(g: &Genesis) -> (NewEvent, String) {
    let text = format!(
        "being genesis: a deterministic simulation world and loop start; world_seed {} loop_seed {} budget {} energy units; choice policy ucb1 with exploration constant {}",
        g.world_seed, g.loop_seed, g.budget, g.explore_c
    );
    let at = iso_at(0);
    let id = event_id(SOURCE, &at, &text);
    let ev = NewEvent {
        source: SOURCE.into(),
        occurred_at: at,
        kind: "note".into(),
        text,
        tags: vec![
            "genesis".into(),
            format!("world_seed:{}", g.world_seed),
            format!("loop_seed:{}", g.loop_seed),
            format!("budget:{}", g.budget),
            format!("explore_c:{}", g.explore_c),
            "policy:ucb1".into(),
        ],
        redacted: false,
    };
    (ev, id)
}

/// Refill: only when the caller allows it (`stop_after_budget: false`). Logged.
pub fn refill_event(budget: i64, clock_before: i64, refills_so_far: u64) -> (NewEvent, i64) {
    let clock = ((clock_before / 86_400) + 1) * 86_400;
    let text = format!("budget refilled to {budget} energy units at the start of a new world day (refill {})", refills_so_far + 1);
    let ev = NewEvent {
        source: SOURCE.into(),
        occurred_at: iso_at(clock),
        kind: "note".into(),
        text,
        tags: vec!["refill".into(), format!("budget_left:{budget}"), format!("clock:{clock}")],
        redacted: false,
    };
    (ev, clock)
}
