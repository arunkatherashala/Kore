//! Goal-directed planning for Aru's continuing life loop.
//! Plans describe work; they do not grant permission to perform external actions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Active,
    Complete,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanStep {
    pub id: String,
    pub description: String,
    pub verification: String,
    pub status: StepStatus,
    #[serde(default)]
    pub verification_evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LifePlan {
    pub plan_id: String,
    pub problem: String,
    pub purpose: String,
    pub steps: Vec<PlanStep>,
    pub current_step: usize,
    pub status: String,
}

impl LifePlan {
    pub fn for_problem(plan_id: &str, problem: &str, purpose: &str) -> Self {
        let problem = problem.trim().to_string();
        Self {
            plan_id: plan_id.to_string(),
            problem: problem.clone(),
            purpose: purpose.to_string(),
            steps: vec![
                PlanStep {
                    id: "understand".to_string(),
                    description: format!("Define the problem and its constraints: {problem}"),
                    verification: "A precise problem statement and explicit constraints exist.".to_string(),
                    status: StepStatus::Active,
                    verification_evidence: Vec::new(),
                },
                PlanStep {
                    id: "research".to_string(),
                    description: "Collect approved evidence and record source provenance.".to_string(),
                    verification: "Every important claim has evidence and a source.".to_string(),
                    status: StepStatus::Pending,
                    verification_evidence: Vec::new(),
                },
                PlanStep {
                    id: "hypothesize".to_string(),
                    description: "Generate competing hypotheses and state uncertainty.".to_string(),
                    verification: "At least one falsifiable hypothesis and alternative are recorded.".to_string(),
                    status: StepStatus::Pending,
                    verification_evidence: Vec::new(),
                },
                PlanStep {
                    id: "test".to_string(),
                    description: "Run a safe, bounded analysis or experiment.".to_string(),
                    verification: "The test is reproducible and produces an observable result.".to_string(),
                    status: StepStatus::Pending,
                    verification_evidence: Vec::new(),
                },
                PlanStep {
                    id: "conclude".to_string(),
                    description: "Compare results with hypotheses and write the conclusion.".to_string(),
                    verification: "Conclusion includes evidence, confidence, limitations, and next action.".to_string(),
                    status: StepStatus::Pending,
                    verification_evidence: Vec::new(),
                },
            ],
            current_step: 0,
            status: "active".to_string(),
        }
    }

    pub fn advance(&mut self) -> Result<&PlanStep, String> {
        self.advance_verified("legacy test transition")
    }

    pub fn advance_verified(&mut self, evidence: &str) -> Result<&PlanStep, String> {
        if self.current_step >= self.steps.len() {
            self.status = "complete".to_string();
            return Err("plan is already complete".to_string());
        }
        if evidence.trim().is_empty() {
            return Err("verification evidence is required".to_string());
        }
        self.steps[self.current_step].verification_evidence.push(evidence.trim().to_string());
        self.steps[self.current_step].status = StepStatus::Complete;
        self.current_step += 1;
        if self.current_step >= self.steps.len() {
            self.status = "complete".to_string();
            return Err("plan completed".to_string());
        }
        self.steps[self.current_step].status = StepStatus::Active;
        Ok(&self.steps[self.current_step])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_plan_has_verification_gates() {
        let plan = LifePlan::for_problem("plan-1", "Understand a DNA research question", "preserve human curiosity");
        assert_eq!(plan.steps.len(), 5);
        assert!(plan.steps.iter().all(|step| !step.verification.is_empty()));
        assert_eq!(plan.steps[0].status, StepStatus::Active);
    }

    #[test]
    fn plan_advances_in_order_and_completes() {
        let mut plan = LifePlan::for_problem("plan-2", "Solve a bounded problem", "learn");
        for _ in 0..4 { assert!(plan.advance().is_ok()); }
        assert_eq!(plan.status, "active");
        assert_eq!(plan.advance(), Err("plan completed".to_string()));
        assert_eq!(plan.status, "complete");
    }

    #[test]
    fn plan_rejects_unverified_progress() {
        let mut plan = LifePlan::for_problem("plan-3", "Verify progress", "learn");
        assert_eq!(plan.advance_verified(""), Err("verification evidence is required".to_string()));
        assert!(plan.advance_verified("Problem constraints recorded").is_ok());
        assert_eq!(plan.steps[0].verification_evidence.len(), 1);
    }
}
