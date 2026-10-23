//! Deterministic, rule-based learning over the versioned log (milestone M2b/M2c).
//!
//! No language model, no randomness, no network. Every derived row is written
//! through the versioned library as `author = rule:<id>` and cites the events
//! it was computed from. The pure detectors (`habits`, `changes`, `stances`)
//! take plain event views; `derive` turns their output into revisions and
//! writes them idempotently; `evolve` tunes the learner's own parameters
//! (rules only, never source code); `ledger` writes frequency-baseline
//! predictions before any outcome exists.
//!
//! This module only uses `crate::versioned` and std, so examples and
//! integration tests can include it with `#[path]` next to `versioned`.
//! See `kore-self/docs/versioned_log.md` ("Learning") and `docs/measured.md`.
#![allow(dead_code)]

pub mod changes;
pub mod dates;
pub mod derive;
pub mod evolve;
pub mod habits;
pub mod ledger;
pub mod params;
pub mod stances;

pub use params::Params;

use crate::versioned::EventRow;

/// Tag prefixes the learners understand.
pub const CTX_PREFIX: &str = "ctx:";
pub const ACT_PREFIX: &str = "act:";
pub const TOPIC_PREFIX: &str = "topic:";
/// Events carrying this tag are the evolution loop's own records; learners ignore them.
pub const EVOLVE_TAG: &str = "evolve";
pub const NO_ACTION: &str = "(none)";

/// A read-only view of one event, with the day number pre-computed.
#[derive(Debug, Clone, PartialEq)]
pub struct Ev {
    pub id: String,
    /// Normalised `YYYY-MM-DDTHH:MM:SSZ`.
    pub ts: String,
    /// Days since 1970-01-01 of `ts` (UTC).
    pub day: i64,
    pub kind: String,
    pub source: String,
    pub text: String,
    pub tags: Vec<String>,
}

impl Ev {
    pub fn from_row(r: &EventRow) -> Option<Ev> {
        let day = dates::day_of(&r.occurred_at)?;
        Some(Ev {
            id: r.event_id.clone(),
            ts: r.occurred_at.clone(),
            day,
            kind: r.kind.clone(),
            source: r.source.clone(),
            text: r.text.clone(),
            tags: r.tags.split(',').filter(|t| !t.is_empty()).map(|t| t.to_string()).collect(),
        })
    }
    /// First tag value with this prefix (`ctx:` -> the part after it).
    pub fn tag_val(&self, prefix: &str) -> Option<&str> {
        self.tags.iter().find_map(|t| t.strip_prefix(prefix))
    }
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

/// Events the learners may read: everything except the evolution loop's own
/// records, sorted by (time, id) so results do not depend on storage order.
pub fn learnable(rows: &[EventRow]) -> Vec<Ev> {
    let mut v: Vec<Ev> = rows.iter().filter_map(Ev::from_row).filter(|e| !e.has_tag(EVOLVE_TAG)).collect();
    v.sort_by(|a, b| (&a.ts, &a.id).cmp(&(&b.ts, &b.id)));
    v
}

/// Only the events that happened strictly before `day`. This is the single
/// door through which backtests and ledger snapshots look at the past, so a
/// later event cannot reach a decision made at `day`.
pub fn visible_before(events: &[Ev], day: i64) -> Vec<Ev> {
    events.iter().filter(|e| e.day < day).cloned().collect()
}
