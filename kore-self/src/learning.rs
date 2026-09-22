//! Learning engine for Aru — update beliefs based on action outcomes and evidence.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BeliefUpdate {
    pub belief_id: String,
    pub topic: String,
    pub old_confidence: f64,
    pub new_confidence: f64,
    pub evidence_source: String,
    pub update_reason: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutcomeObservation {
    pub action_id: u64,
    pub action_description: String,
    pub intended_outcome: String,
    pub actual_outcome: String,
    pub success_score: f64, // 0.0 to 1.0
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningHistory {
    pub belief_updates: Vec<BeliefUpdate>,
    pub outcome_observations: Vec<OutcomeObservation>,
    pub prediction_accuracy: f64, // Average accuracy over time
    pub learning_cycles: u64,
}

impl LearningHistory {
    pub fn new() -> Self {
        Self {
            belief_updates: Vec::new(),
            outcome_observations: Vec::new(),
            prediction_accuracy: 0.5,
            learning_cycles: 0,
        }
    }

    /// Record an outcome observation and compute prediction error
    pub fn observe_outcome(&mut self, action_id: u64, action_desc: &str, intended: &str, actual: &str, success: f64, now: &str) {
        let obs = OutcomeObservation {
            action_id, action_description: action_desc.to_string(),
            intended_outcome: intended.to_string(), actual_outcome: actual.to_string(),
            success_score: success, timestamp: now.to_string(),
        };
        self.outcome_observations.push(obs);
    }

    /// Update a belief based on new evidence
    pub fn update_belief(&mut self, belief_id: &str, topic: &str, old_conf: f64, new_conf: f64, evidence: &str, reason: &str, now: &str) {
        let update = BeliefUpdate {
            belief_id: belief_id.to_string(), topic: topic.to_string(),
            old_confidence: old_conf, new_confidence: new_conf,
            evidence_source: evidence.to_string(), update_reason: reason.to_string(),
            timestamp: now.to_string(),
        };
        self.belief_updates.push(update);
        self.learning_cycles += 1;
    }

    /// Compute average prediction accuracy from observations
    pub fn compute_prediction_accuracy(&mut self) {
        if self.outcome_observations.is_empty() {
            self.prediction_accuracy = 0.5;
        } else {
            let total: f64 = self.outcome_observations.iter().map(|o| o.success_score).sum();
            self.prediction_accuracy = total / self.outcome_observations.len() as f64;
        }
    }

    /// Get most recent belief updates (limit)
    pub fn recent_updates(&self, limit: usize) -> Vec<&BeliefUpdate> {
        self.belief_updates.iter().rev().take(limit).collect()
    }

    /// Find observations where prediction failed (success < 0.5)
    pub fn failed_predictions(&self) -> Vec<&OutcomeObservation> {
        self.outcome_observations.iter().filter(|o| o.success_score < 0.5).collect()
    }

    /// Estimate confidence drift: how much have beliefs changed?
    pub fn confidence_drift(&self) -> f64 {
        if self.belief_updates.is_empty() {
            return 0.0;
        }
        let total_change: f64 = self.belief_updates.iter()
            .map(|u| (u.new_confidence - u.old_confidence).abs())
            .sum();
        total_change / self.belief_updates.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_history_records_outcomes() {
        let mut history = LearningHistory::new();
        history.observe_outcome(1, "tried_technique", "mastery", "partial_success", 0.7, "2026-09-21T10:00Z");
        assert_eq!(history.outcome_observations.len(), 1);
        assert_eq!(history.outcome_observations[0].success_score, 0.7);
    }

    #[test]
    fn belief_updates_accumulate_with_cycles() {
        let mut history = LearningHistory::new();
        history.update_belief("b1", "reasoning", 0.6, 0.8, "evidence_A", "positive_test", "t1");
        history.update_belief("b2", "planning", 0.7, 0.65, "counterexample", "negative_test", "t2");
        assert_eq!(history.belief_updates.len(), 2);
        assert_eq!(history.learning_cycles, 2);
    }

    #[test]
    fn prediction_accuracy_computed_from_outcomes() {
        let mut history = LearningHistory::new();
        history.observe_outcome(1, "action1", "goal1", "result1", 0.8, "t1");
        history.observe_outcome(2, "action2", "goal2", "result2", 0.6, "t2");
        history.observe_outcome(3, "action3", "goal3", "result3", 0.9, "t3");
        history.compute_prediction_accuracy();
        assert!((history.prediction_accuracy - 0.7666).abs() < 0.01);
    }

    #[test]
    fn failed_predictions_identified() {
        let mut history = LearningHistory::new();
        history.observe_outcome(1, "action1", "goal1", "result1", 0.8, "t1");
        history.observe_outcome(2, "action2", "goal2", "result2", 0.3, "t2");
        history.observe_outcome(3, "action3", "goal3", "result3", 0.1, "t3");
        let failures = history.failed_predictions();
        assert_eq!(failures.len(), 2);
    }

    #[test]
    fn confidence_drift_measures_belief_change() {
        let mut history = LearningHistory::new();
        history.update_belief("b1", "reasoning", 0.5, 0.8, "evidence", "reason", "t1");
        history.update_belief("b2", "planning", 0.7, 0.6, "counterexample", "reason", "t2");
        let drift = history.confidence_drift();
        assert!((drift - 0.2).abs() < 0.01);
    }
}
