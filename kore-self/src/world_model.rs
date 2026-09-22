//! World model for Aru — entities, events, relationships, and causality.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entity {
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: u64,
    pub timestamp: String,
    pub actor: Entity,
    pub action: String,
    pub recipient: Option<Entity>,
    pub outcome: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Relationship {
    pub from: Entity,
    pub to: Entity,
    pub relation: String,
    pub strength: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CausalLink {
    pub cause_event_id: u64,
    pub effect_event_id: u64,
    pub causal_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldModel {
    pub entities: Vec<Entity>,
    pub events: Vec<Event>,
    pub relationships: Vec<Relationship>,
    pub causality: Vec<CausalLink>,
}

impl WorldModel {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            events: Vec::new(),
            relationships: Vec::new(),
            causality: Vec::new(),
        }
    }

    pub fn add_entity(&mut self, name: &str, kind: &str) {
        let entity = Entity { name: name.to_string(), kind: kind.to_string() };
        if !self.entities.contains(&entity) { self.entities.push(entity); }
    }

    pub fn add_event(&mut self, id: u64, timestamp: &str, actor: &str, action: &str, recipient: Option<&str>, outcome: &str, confidence: f64) {
        let actor_entity = Entity { name: actor.to_string(), kind: "agent".to_string() };
        self.add_entity(actor, "agent");
        if let Some(r) = recipient { self.add_entity(r, "entity"); }
        self.events.push(Event {
            id, timestamp: timestamp.to_string(), actor: actor_entity,
            action: action.to_string(), recipient: recipient.map(|r| Entity { name: r.to_string(), kind: "entity".to_string() }),
            outcome: outcome.to_string(), confidence,
        });
    }

    pub fn add_relationship(&mut self, from: &str, to: &str, relation: &str, strength: f64) {
        self.add_entity(from, "entity");
        self.add_entity(to, "entity");
        self.relationships.push(Relationship {
            from: Entity { name: from.to_string(), kind: "entity".to_string() },
            to: Entity { name: to.to_string(), kind: "entity".to_string() },
            relation: relation.to_string(), strength,
        });
    }

    pub fn link_causality(&mut self, cause_id: u64, effect_id: u64, confidence: f64) {
        self.causality.push(CausalLink { cause_event_id: cause_id, effect_event_id: effect_id, causal_confidence: confidence });
    }

    pub fn query_events_by_actor(&self, actor: &str) -> Vec<&Event> {
        self.events.iter().filter(|e| e.actor.name == actor).collect()
    }

    pub fn query_causal_chain(&self, start_event_id: u64) -> Vec<u64> {
        let mut chain = vec![start_event_id];
        let mut current = start_event_id;
        while let Some(link) = self.causality.iter().find(|l| l.cause_event_id == current) {
            chain.push(link.effect_event_id);
            current = link.effect_event_id;
        }
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_model_tracks_entities_and_events() {
        let mut world = WorldModel::new();
        world.add_entity("Alice", "person");
        world.add_event(1, "2026-09-21", "Alice", "learned", Some("Rust"), "success", 0.95);
        let alice_events = world.query_events_by_actor("Alice");
        assert_eq!(alice_events.len(), 1);
        assert_eq!(alice_events[0].action, "learned");
    }

    #[test]
    fn causality_chains_link_events() {
        let mut world = WorldModel::new();
        world.add_event(1, "t1", "Alice", "sent", Some("message"), "success", 0.9);
        world.add_event(2, "t2", "Bob", "received", Some("message"), "understood", 0.85);
        world.link_causality(1, 2, 0.8);
        let chain = world.query_causal_chain(1);
        assert_eq!(chain, vec![1, 2]);
    }
}
