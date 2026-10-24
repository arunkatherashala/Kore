//! Replay: re-derive every decision of the being from its log alone.
//!
//! For each logged decision event, the state is folded from the events before
//! it (genesis, refills, outcomes), the menu is rebuilt from the world seed in
//! the genesis event, and the chooser is asked again. The result must equal
//! what the loop recorded. This is the check that preferences live in the log
//! and nowhere else.

use super::chooser::{DomainPicker, Pick, Ucb1};
use super::state::{tag, BeingState, SOURCE};
use super::world::{Domain, World};
use crate::versioned::EventRow;

#[derive(Debug, Clone)]
pub struct Replayed {
    pub step: u64,
    pub event_id: String,
    pub recorded: Domain,
    pub rederived: Domain,
    pub pick: Pick,
}

impl Replayed {
    pub fn matches(&self) -> bool {
        self.recorded == self.rederived
    }
}

/// Re-derive the pick that would be made next from the events in `rows`.
pub fn next_pick(rows: &[EventRow]) -> Option<Pick> {
    let st = BeingState::from_events(rows);
    pick_for(&st)
}

fn pick_for(st: &BeingState) -> Option<Pick> {
    let g = st.genesis.as_ref()?;
    let world = World::new(g.world_seed);
    let menu = world.menu(st.step);
    let afford: Vec<Domain> = menu.iter().filter(|t| t.view.cost <= st.budget_left).map(|t| t.view.domain).collect();
    if afford.is_empty() {
        return None;
    }
    Some(Ucb1::new(g.loop_seed, g.explore_c).pick(st, &afford))
}

pub fn replay_decisions(rows: &[EventRow]) -> Vec<Replayed> {
    let mut st = BeingState::default();
    let mut out = vec![];
    for e in rows.iter().filter(|e| e.source == SOURCE) {
        if e.kind == "decision" {
            let recorded = tag(&e.tags, "domain").and_then(Domain::parse);
            let step = tag(&e.tags, "step").and_then(|s| s.parse().ok());
            if let (Some(recorded), Some(step), Some(p)) = (recorded, step, pick_for(&st)) {
                out.push(Replayed { step, event_id: e.event_id.clone(), recorded, rederived: p.domain, pick: p });
            }
        } else {
            st.fold_event(e);
        }
    }
    out
}
