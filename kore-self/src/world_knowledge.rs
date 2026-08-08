//! Cross-domain world knowledge: languages, humanities, geography, biology, and subject routing.

use crate::world_languages;
use crate::world_technical;
use crate::world_subjects;
use crate::world_types::WorldAnswer;

pub use crate::world_types::WorldAnswer as KnowledgeAnswer;

pub fn try_world(problem: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    let lower = problem.to_lowercase();
    if let Some(a) = world_technical::try_technical(problem, &lower, steps) {
        return Some(a);
    }
    if let Some(a) = world_languages::try_language_query(problem, &lower, steps) {
        return Some(a);
    }
    if let Some(a) = world_subjects::try_subjects(problem, &lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_subject_classify(&lower, steps) {
        return Some(a);
    }
    None
}

pub fn catalog_languages_summary() -> String {
    world_languages::catalog_summary()
}

pub fn catalog_subjects_summary() -> String {
    world_subjects::taxonomy_summary()
}

pub fn catalog_technical_summary() -> String {
    world_technical::catalog_summary()
}

fn try_subject_classify(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    let hits = world_subjects::classify(lower);
    if hits.is_empty() {
        return None;
    }
    if !lower.contains("subject")
        && !lower.contains("field")
        && !lower.contains("discipline")
        && !lower.contains("which area")
        && !lower.contains("what domain")
    {
        return None;
    }
    steps.push("Matched KORE world subject taxonomy.".into());
    Some(WorldAnswer {
        method: "subject_classify".into(),
        answer: format!(
            "Subject areas detected: {}\n\n{}",
            hits.join(", "),
            world_subjects::taxonomy_summary()
        ),
        confidence: 0.82,
    })
}
