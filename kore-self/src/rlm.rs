//! Retrieval and reasoning layer for Aru.
use crate::Memory;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ReasoningResult {
    pub question: String,
    pub evidence: Vec<String>,
    pub kinds: Vec<String>,
    pub confidence: f64,
    pub uncertainty: String,
    pub next_action: String,
}

pub fn reason(memories: &[Memory], question: &str, limit: usize) -> ReasoningResult {
    let words: Vec<String> = question.to_lowercase().split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|word| word.len() >= 3).collect();
    let mut scored: Vec<(f64, &Memory)> = memories.iter().map(|memory| {
        let text = memory.content.to_lowercase();
        let hits = words.iter().filter(|word| text.contains(word.as_str())).count() as f64;
        (hits * (0.5 + memory.importance), memory)
    }).filter(|(score, _)| *score > 0.0).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit.max(1));
    let mut kind_counts = HashMap::new();
    for (_, memory) in &scored { *kind_counts.entry(memory.kind.clone()).or_insert(0usize) += 1; }
    let mut kinds: Vec<String> = kind_counts.into_iter().map(|(kind, count)| format!("{kind}:{count}")).collect();
    kinds.sort();
    let evidence = scored.iter().map(|(_, memory)| format!("[{} | {:.0}%] {}", memory.kind, memory.importance * 100.0, memory.content)).collect::<Vec<_>>();
    let confidence = if scored.is_empty() { 0.0 } else { (scored.iter().map(|(score, _)| *score).sum::<f64>() / (scored.len() as f64 * 2.0)).min(1.0) };
    ReasoningResult {
        question: question.to_string(), evidence, kinds,
        confidence,
        uncertainty: if scored.is_empty() { "No matching memory evidence; external research is required.".to_string() } else { "This is retrieval-grounded, not proof; check contradictory sources before concluding.".to_string() },
        next_action: if scored.is_empty() { "Create an approved research plan.".to_string() } else { "Compare evidence and test the strongest hypothesis.".to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rlm_returns_evidence_and_uncertainty() {
        let memories = vec![Memory { id: 1, timestamp: "t".into(), kind: "research".into(), content: "DNA repair evidence".into(), tags: vec![], importance: 0.9 }];
        let result = reason(&memories, "DNA repair", 5);
        assert_eq!(result.evidence.len(), 1);
        assert!(result.confidence > 0.0);
        assert!(!result.uncertainty.is_empty());
    }
}
