//! Native KORE persistence for Aru artifacts.
//!
//! Domain records remain JSON-shaped for schema evolution, but the file itself
//! uses KORE's native columnar format rather than a JSON file on disk.

use kore_core::{Column, DataBlock, ColumnData};
use kore_store::{KoreReader, KoreWriter};
use crate::planning::{LifePlan, PlanStep, StepStatus};
use crate::learning::{BeliefUpdate, LearningHistory, OutcomeObservation};
use std::io;
use std::path::{Path, PathBuf};

pub fn artifact_path(owner: &str, artifact: &str) -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join(".kore-self").join(owner).join(format!("{artifact}.kore"))
}

pub fn write_payload(path: &Path, payload: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let block = DataBlock::new(vec![Column::str_col(
        "payload",
        vec![Some(payload.to_string())],
    )]).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let tmp = path.with_extension("kore.tmp");
    KoreWriter::write_file(&tmp, &block)?;
    std::fs::rename(tmp, path)
}

pub fn read_payload(path: &Path) -> io::Result<Option<String>> {
    let block = KoreReader::read_file(path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    Ok(block.columns.iter()
        .find(|column| column.name == "payload")
        .and_then(|column| column.data.get_str(0))
        .map(str::to_string))
}

pub fn write_plan(path: &Path, plan: &LifePlan) -> io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let rows = plan.steps.len().max(1);
    let repeated = |value: String| vec![Some(value); rows];
    let steps = if plan.steps.is_empty() {
        vec![PlanStep { id: String::new(), description: String::new(), verification: String::new(), status: StepStatus::Pending, verification_evidence: Vec::new() }]
    } else { plan.steps.clone() };
    let block = DataBlock::new(vec![
        Column::str_col("plan_id", repeated(plan.plan_id.clone())),
        Column::str_col("problem", repeated(plan.problem.clone())),
        Column::str_col("purpose", repeated(plan.purpose.clone())),
        Column::str_col("plan_status", repeated(plan.status.clone())),
        Column::int64("current_step", vec![Some(plan.current_step as i64); rows]),
        Column::str_col("step_id", steps.iter().map(|s| Some(s.id.clone())).collect()),
        Column::str_col("description", steps.iter().map(|s| Some(s.description.clone())).collect()),
        Column::str_col("verification", steps.iter().map(|s| Some(s.verification.clone())).collect()),
        Column::str_col("step_status", steps.iter().map(|s| Some(format!("{:?}", s.status))).collect()),
        Column::str_col("verification_evidence", steps.iter().map(|s| Some(s.verification_evidence.join("\n"))).collect()),
    ]).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let tmp = path.with_extension("kore.tmp");
    KoreWriter::write_file(&tmp, &block)?;
    std::fs::rename(tmp, path)
}

pub fn read_plan(path: &Path) -> io::Result<Option<LifePlan>> {
    let block = KoreReader::read_file(path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let text = |name: &str, row: usize| block.columns.iter().find(|c| c.name == name).and_then(|c| c.data.get_str(row)).unwrap_or("").to_string();
    let int = |name: &str| block.columns.iter().find(|c| c.name == name).and_then(|c| match &c.data { ColumnData::Int64(v) => v.first().and_then(|x| *x), _ => None }).unwrap_or(0);
    if block.num_rows == 0 { return Ok(None); }
    let mut steps = Vec::with_capacity(block.num_rows);
    for row in 0..block.num_rows {
        let status = match text("step_status", row).as_str() {
            "Active" => StepStatus::Active, "Complete" => StepStatus::Complete, "Blocked" => StepStatus::Blocked, _ => StepStatus::Pending,
        };
        let evidence = text("verification_evidence", row);
        steps.push(PlanStep { id: text("step_id", row), description: text("description", row), verification: text("verification", row), status, verification_evidence: if evidence.is_empty() { Vec::new() } else { evidence.lines().map(str::to_string).collect() } });
    }
    Ok(Some(LifePlan { plan_id: text("plan_id", 0), problem: text("problem", 0), purpose: text("purpose", 0), steps, current_step: int("current_step") as usize, status: text("plan_status", 0) }))
}

pub fn write_learning_history(path: &Path, history: &LearningHistory) -> io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let rows = history.belief_updates.len() + history.outcome_observations.len();
    let mut kind = Vec::with_capacity(rows);
    let mut action_id = Vec::with_capacity(rows);
    let mut belief_id = Vec::with_capacity(rows);
    let mut topic = Vec::with_capacity(rows);
    let mut old_confidence = Vec::with_capacity(rows);
    let mut new_confidence = Vec::with_capacity(rows);
    let mut action_description = Vec::with_capacity(rows);
    let mut intended_outcome = Vec::with_capacity(rows);
    let mut actual_outcome = Vec::with_capacity(rows);
    let mut success_score = Vec::with_capacity(rows);
    let mut evidence_source = Vec::with_capacity(rows);
    let mut update_reason = Vec::with_capacity(rows);
    let mut timestamp = Vec::with_capacity(rows);
    for update in &history.belief_updates {
        kind.push(Some("belief".to_string())); action_id.push(None);
        belief_id.push(Some(update.belief_id.clone())); topic.push(Some(update.topic.clone()));
        old_confidence.push(Some(update.old_confidence)); new_confidence.push(Some(update.new_confidence));
        action_description.push(None); intended_outcome.push(None); actual_outcome.push(None); success_score.push(None);
        evidence_source.push(Some(update.evidence_source.clone())); update_reason.push(Some(update.update_reason.clone()));
        timestamp.push(Some(update.timestamp.clone()));
    }
    for observation in &history.outcome_observations {
        kind.push(Some("outcome".to_string())); action_id.push(Some(observation.action_id as i64));
        belief_id.push(None); topic.push(None); old_confidence.push(None); new_confidence.push(None);
        action_description.push(Some(observation.action_description.clone())); intended_outcome.push(Some(observation.intended_outcome.clone()));
        actual_outcome.push(Some(observation.actual_outcome.clone())); success_score.push(Some(observation.success_score));
        evidence_source.push(None); update_reason.push(None); timestamp.push(Some(observation.timestamp.clone()));
    }
    let repeated_accuracy = vec![Some(history.prediction_accuracy); rows];
    let repeated_cycles = vec![Some(history.learning_cycles as i64); rows];
    let block = DataBlock::new(vec![
        Column::str_col("record_kind", kind), Column::int64("action_id", action_id),
        Column::str_col("belief_id", belief_id), Column::str_col("topic", topic),
        Column::float64("old_confidence", old_confidence), Column::float64("new_confidence", new_confidence),
        Column::str_col("action_description", action_description), Column::str_col("intended_outcome", intended_outcome),
        Column::str_col("actual_outcome", actual_outcome), Column::float64("success_score", success_score),
        Column::str_col("evidence_source", evidence_source), Column::str_col("update_reason", update_reason),
        Column::str_col("timestamp", timestamp), Column::float64("prediction_accuracy", repeated_accuracy),
        Column::int64("learning_cycles", repeated_cycles),
    ]).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let tmp = path.with_extension("kore.tmp");
    KoreWriter::write_file(&tmp, &block)?;
    std::fs::rename(tmp, path)
}

pub fn read_learning_history(path: &Path) -> io::Result<Option<LearningHistory>> {
    if !path.exists() { return Ok(None); }
    let block = KoreReader::read_file(path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    if block.columns.iter().any(|column| column.name == "payload") {
        return Ok(None);
    }
    let text = |name: &str, row: usize| block.columns.iter().find(|c| c.name == name).and_then(|c| c.data.get_str(row)).map(str::to_string);
    let int = |name: &str, row: usize| block.columns.iter().find(|c| c.name == name).and_then(|c| match &c.data { ColumnData::Int64(v) => v.get(row).and_then(|x| *x), _ => None });
    let float = |name: &str, row: usize| block.columns.iter().find(|c| c.name == name).and_then(|c| match &c.data { ColumnData::Float64(v) => v.get(row).and_then(|x| *x), _ => None });
    let mut history = LearningHistory::new();
    for row in 0..block.num_rows {
        match text("record_kind", row).as_deref() {
            Some("belief") => history.belief_updates.push(BeliefUpdate {
                belief_id: text("belief_id", row).unwrap_or_default(), topic: text("topic", row).unwrap_or_default(),
                old_confidence: float("old_confidence", row).unwrap_or(0.5), new_confidence: float("new_confidence", row).unwrap_or(0.5),
                evidence_source: text("evidence_source", row).unwrap_or_default(), update_reason: text("update_reason", row).unwrap_or_default(),
                timestamp: text("timestamp", row).unwrap_or_default(),
            }),
            Some("outcome") => history.outcome_observations.push(OutcomeObservation {
                action_id: int("action_id", row).unwrap_or(0) as u64, action_description: text("action_description", row).unwrap_or_default(),
                intended_outcome: text("intended_outcome", row).unwrap_or_default(), actual_outcome: text("actual_outcome", row).unwrap_or_default(),
                success_score: float("success_score", row).unwrap_or(0.5), timestamp: text("timestamp", row).unwrap_or_default(),
            }),
            _ => {}
        }
        history.prediction_accuracy = float("prediction_accuracy", row).unwrap_or(history.prediction_accuracy);
        history.learning_cycles = int("learning_cycles", row).unwrap_or(history.learning_cycles as i64) as u64;
    }
    Ok(Some(history))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_kore_payload_round_trips() {
        let path = std::env::temp_dir().join(format!("aru-store-{}.kore", std::process::id()));
        write_payload(&path, "{\"kind\":\"plan\",\"version\":1}").unwrap();
        assert_eq!(read_payload(&path).unwrap().as_deref(), Some("{\"kind\":\"plan\",\"version\":1}"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn typed_plan_round_trips_without_payload_json() {
        let path = std::env::temp_dir().join(format!("aru-plan-{}.kore", std::process::id()));
        let plan = LifePlan::for_problem("p1", "typed plan", "continue");
        write_plan(&path, &plan).unwrap();
        let restored = read_plan(&path).unwrap().unwrap();
        assert_eq!(restored.problem, "typed plan");
        assert_eq!(restored.steps.len(), plan.steps.len());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn typed_learning_history_round_trips_without_payload_json() {
        let path = std::env::temp_dir().join(format!("aru-learning-{}.kore", std::process::id()));
        let mut history = LearningHistory::new();
        history.observe_outcome(7, "test_action", "goal", "done", 0.9, "t1");
        history.update_belief("b1", "topic", 0.4, 0.8, "evidence", "reason", "t2");
        history.compute_prediction_accuracy();
        write_learning_history(&path, &history).unwrap();
        let restored = read_learning_history(&path).unwrap().unwrap();
        assert_eq!(restored.outcome_observations, history.outcome_observations);
        assert_eq!(restored.belief_updates, history.belief_updates);
        assert_eq!(restored.learning_cycles, 1);
        let _ = std::fs::remove_file(path);
    }
}
