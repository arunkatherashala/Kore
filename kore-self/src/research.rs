//! Structured, source-grounded research records.
//! External retrieval is intentionally separate; this module only validates
//! evidence that KORE has been given or approved to ingest.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceSource {
    pub source_id: String,
    pub title: String,
    pub locator: String,
    pub publisher: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchClaim {
    pub claim_id: String,
    pub question: String,
    pub claim: String,
    pub evidence: Vec<String>,
    pub sources: Vec<EvidenceSource>,
    pub confidence: f64,
    pub status: String,
}

impl ResearchClaim {
    pub fn validate(&self) -> Result<(), String> {
        if self.claim_id.trim().is_empty() { return Err("claim_id is required".to_string()); }
        if self.question.trim().is_empty() { return Err("question is required".to_string()); }
        if self.claim.trim().is_empty() { return Err("claim is required".to_string()); }
        if self.evidence.is_empty() { return Err("at least one evidence item is required".to_string()); }
        if self.sources.is_empty() { return Err("at least one source is required".to_string()); }
        if !(0.0..=1.0).contains(&self.confidence) { return Err("confidence must be between 0 and 1".to_string()); }
        if !matches!(self.status.as_str(), "supported" | "mixed" | "unverified" | "refuted") {
            return Err("status must be supported, mixed, unverified, or refuted".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim() -> ResearchClaim {
        ResearchClaim {
            claim_id: "claim-1".to_string(),
            question: "Does the intervention help?".to_string(),
            claim: "The evidence suggests a possible benefit.".to_string(),
            evidence: vec!["Trial result".to_string()],
            sources: vec![EvidenceSource {
                source_id: "source-1".to_string(),
                title: "Example study".to_string(),
                locator: "https://example.org/study".to_string(),
                publisher: "Example Journal".to_string(),
                published_at: None,
            }],
            confidence: 0.6,
            status: "mixed".to_string(),
        }
    }

    #[test]
    fn research_claim_requires_evidence_and_sources() {
        assert!(claim().validate().is_ok());
        let mut missing_evidence = claim();
        missing_evidence.evidence.clear();
        assert!(missing_evidence.validate().is_err());
        let mut missing_source = claim();
        missing_source.sources.clear();
        assert!(missing_source.validate().is_err());
    }
}
