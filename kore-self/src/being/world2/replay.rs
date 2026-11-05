//! Replay of a World v2 being from its log alone: every decision of the learner is re-derived from
//! the state folded from the events before it and the offers rebuilt from the logged world config;
//! every accepted rule is re-derived by the same search over the examples logged before it.

use super::engine::{offers_of2, State2};
use super::learner::{Chooser2, LEARNER_NAME};
use super::world::{World2, SOURCE2};
use crate::being::discover::{rule_from_event, rule_id_x, search, search_composed, Form};
use crate::being::state::tag;
use crate::versioned::EventRow;

#[derive(Debug, Clone, PartialEq)]
pub struct Replayed2 {
    pub step: u64,
    pub event_id: String,
    pub recorded: String,
    pub rederived: String,
}

impl Replayed2 {
    pub fn matches(&self) -> bool {
        self.recorded == self.rederived
    }
}

/// Re-derive every decision the learner made (decisions of other choosers are not the being's own and are skipped).
pub fn replay_decisions2(rows: &[EventRow]) -> Vec<Replayed2> {
    let mut st = State2::default();
    let mut world: Option<World2> = None;
    let mut out = vec![];
    for e in rows.iter().filter(|e| e.source == SOURCE2) {
        if e.kind == "decision" && tag(&e.tags, "policy") == Some(LEARNER_NAME) {
            if world.is_none() {
                world = st.cfg.clone().map(World2::new);
            }
            if let (Some(w), Some(rec), Some(step)) = (&world, tag(&e.tags, "action"), tag(&e.tags, "step").and_then(|s| s.parse().ok())) {
                let offers = offers_of2(&w.offers(st.step, st.clock));
                let again = st.learner.learner().choose(&st, &offers).map(|c| c.act.label()).unwrap_or_else(|| "none".into());
                out.push(Replayed2 { step, event_id: e.event_id.clone(), recorded: rec.to_string(), rederived: again });
            }
        }
        st.fold_event(e);
    }
    out
}

/// For every World v2 rule note: does the search over only the examples logged before it give exactly this rule?
pub fn rederive_rules2(rows: &[EventRow]) -> Vec<(String, bool)> {
    let mut st = State2::default();
    let mut out = vec![];
    for e in rows.iter().filter(|e| e.source == SOURCE2) {
        if e.kind == "note" && e.tags.split(',').any(|t| t == "world2") {
            if let Some(r) = rule_from_event(e) {
                let ok = if r.xf.is_id() {
                    search(&st.examples).iter().any(|f| rule_id_x(f.sig, f.xf, &f.form) == r.id && f.train_ok == r.train_ok && f.val_ok == r.val_ok)
                } else {
                    let blocks: Vec<Form> = st.blocks().into_iter().take(r.nblocks as usize).collect();
                    search_composed(&st.examples, &blocks, usize::MAX).found.iter().any(|f| rule_id_x(f.sig, f.xf, &f.form) == r.id && f.train_ok == r.train_ok && f.val_ok == r.val_ok)
                };
                out.push((r.id.clone(), ok));
            }
        }
        st.fold_event(e);
    }
    out
}
