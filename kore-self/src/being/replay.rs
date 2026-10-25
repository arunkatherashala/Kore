//! Replay: re-derive every decision of the being from its log alone.
//!
//! For each logged decision event, the state is folded from the events before
//! it (genesis, refills, outcomes; this includes the learned world model), the
//! offers are rebuilt from the world seed in the genesis event, and the
//! chooser is asked again. The result must equal what the loop recorded. This
//! is the check that preferences live in the log and nowhere else.

use super::chooser::{DomainPicker, Pick, Ucb1};
use super::discover::{rule_from_event, rule_id, search};
use super::planner::{MenuChooser, ModelChooser};
use super::run_loop::offers_of;
use super::state::{tag, BeingState, Chooser, SOURCE};
use super::world::{Domain, World};
use crate::versioned::EventRow;

#[derive(Debug, Clone)]
pub struct Replayed {
    pub step: u64,
    pub event_id: String,
    /// Single-task mode: the domain; menu mode: "slot:strategy" or "skip".
    pub recorded: String,
    pub rederived: String,
}

impl Replayed {
    pub fn matches(&self) -> bool {
        self.recorded == self.rederived
    }
}

/// Re-derive the pick that would be made next from the events in `rows` (single-task mode).
pub fn next_pick(rows: &[EventRow]) -> Option<Pick> {
    let st = BeingState::from_events(rows);
    pick_for(&st)
}

/// Re-derive the next decision as text, in either mode.
pub fn next_action(rows: &[EventRow]) -> Option<String> {
    action_for(&BeingState::from_events(rows))
}

fn pick_for(st: &BeingState) -> Option<Pick> {
    let g = st.genesis.as_ref()?;
    let world = World::with_extended(g.world_seed, g.extended);
    let menu = world.menu(st.step);
    let afford: Vec<Domain> = menu.iter().filter(|t| t.view.cost <= st.budget_left).map(|t| t.view.domain).collect();
    if afford.is_empty() {
        return None;
    }
    Some(Ucb1::new(g.loop_seed, g.explore_c).pick(st, &afford))
}

fn action_for(st: &BeingState) -> Option<String> {
    let g = st.genesis.as_ref()?;
    match g.chooser {
        Chooser::Ucb => pick_for(st).map(|p| p.domain.name().to_string()),
        Chooser::Model => {
            let offers = offers_of(&World::with_extended(g.world_seed, g.extended).offers(st.step));
            ModelChooser::new(g.explore_c).choose(st, &offers).map(|c| c.action.label())
        }
    }
}

/// For every rule note in the log: can the search, run over only the examples logged before that
/// note, produce exactly this rule (same id and support)? The rule set is a function of the log.
pub fn rederive_rules(rows: &[EventRow]) -> Vec<(String, bool)> {
    let mut st = BeingState::default();
    let mut out = vec![];
    for e in rows.iter().filter(|e| e.source == SOURCE) {
        if let Some(r) = rule_from_event(e) {
            let ok = search(&st.examples).iter().any(|f| rule_id(f.sig, &f.form) == r.id && f.train_ok == r.train_ok && f.val_ok == r.val_ok);
            out.push((r.id.clone(), ok));
        }
        st.fold_event(e);
    }
    out
}

pub fn replay_decisions(rows: &[EventRow]) -> Vec<Replayed> {
    let mut st = BeingState::default();
    let mut out = vec![];
    for e in rows.iter().filter(|e| e.source == SOURCE) {
        if e.kind == "decision" {
            let menu = st.genesis.as_ref().map_or(false, |g| g.chooser == Chooser::Model);
            let recorded = if menu { tag(&e.tags, "action").map(|s| s.to_string()) } else { tag(&e.tags, "domain").map(|s| s.to_string()) };
            let step = tag(&e.tags, "step").and_then(|s| s.parse().ok());
            if let (Some(recorded), Some(step), Some(a)) = (recorded, step, action_for(&st)) {
                out.push(Replayed { step, event_id: e.event_id.clone(), recorded, rederived: a });
            }
        } else {
            st.fold_event(e);
        }
    }
    out
}
