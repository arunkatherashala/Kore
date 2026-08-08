//! What KORE-self does *not* know from the world — explicit gap analysis (epistemic humility).

use std::collections::HashSet;

use crate::world_languages;
use crate::world_technical;
use crate::world_solver::WorldSolverEngine;
use crate::Memory;

/// Wikipedia slug, display name — priority domains KORE tries to learn.
pub const PRIORITY_DOMAIN_TOPICS: &[(&str, &str)] = &[
    ("Mathematics", "Mathematics"),
    ("Physics", "Physics"),
    ("Chemistry", "Chemistry"),
    ("Biology", "Biology"),
    ("History", "History"),
    ("Psychology", "Psychology"),
    ("Ethics", "Ethics"),
    ("Consciousness", "Consciousness"),
    ("Ancient_Egypt", "Ancient Egypt"),
    ("Ancient_Greece", "Ancient Greece"),
    ("Economics", "Economics"),
    ("Music_theory", "Music theory"),
    ("Climate_change", "Climate change"),
    ("Genetics", "Genetics"),
    ("Neuroscience", "Neuroscience"),
    ("Quantum_mechanics", "Quantum mechanics"),
    ("Evolution", "Evolution"),
    ("Philosophy", "Philosophy"),
    ("Logic", "Logic"),
    ("Democracy", "Democracy"),
    ("Law", "Law"),
    ("Astronomy", "Astronomy"),
    ("Computing", "Computing"),
    ("Literature", "Literature"),
    ("Religion", "Religion"),
    ("Medicine", "Medicine"),
    ("Ecology", "Ecology"),
    ("Sociology", "Sociology"),
    ("Linguistics", "Linguistics"),
    ("Art", "Art"),
];

/// Extended topics tracked in gap reports (not all in heartbeat burst).
pub const EXTENDED_DOMAIN_TOPICS: &[&str] = &[
    "Ancient_Rome",
    "Mesopotamia",
    "Epistemology",
    "Artificial_intelligence",
    "Machine_learning",
    "Biodiversity",
    "Buddhism",
    "Islam",
    "Christianity",
    "Space_exploration",
    "Robotics",
    "Cryptography",
    "Information_theory",
    "Rust_(programming_language)",
    "Python_(programming_language)",
    "Bash_(Unix_shell)",
    "Linux",
    "Shell_script",
    "Git",
    "Docker_(software)",
    "Kubernetes",
    "Unix",
    "DevOps",
];

/// Subject areas where KORE only routes / gives pointers — no closed-form solver yet.
pub const WEAK_SOLVER_SUBJECTS: &[&str] = &[
    "Engineering",
    "Medicine & Health",
    "Law & Politics",
    "Sociology & Anthropology",
    "Literature & Writing",
    "Arts & Music",
    "Religion & Mythology",
    "Education",
    "Agriculture & Food Science",
];

pub fn domain_display_names_in_memory(memories: &[Memory]) -> HashSet<String> {
    memories
        .iter()
        .filter(|m| m.kind == "domain_knowledge")
        .filter_map(|m| {
            if let Some(cap) = m.content.split("[Domain Knowledge:").nth(1) {
                Some(cap.split('@').next().unwrap_or("").trim().to_string())
            } else if m.content.contains("Domain Knowledge:") {
                None
            } else {
                None
            }
        })
        .collect()
}

pub fn missing_priority_domains(memories: &[Memory]) -> Vec<&'static str> {
    let known = domain_display_names_in_memory(memories);
    PRIORITY_DOMAIN_TOPICS
        .iter()
        .filter(|(_, display)| !known.iter().any(|k| k.contains(display)))
        .map(|(_, display)| *display)
        .collect()
}

pub fn missing_extended_domains(memories: &[Memory]) -> Vec<&'static str> {
    let known = domain_display_names_in_memory(memories);
    EXTENDED_DOMAIN_TOPICS
        .iter()
        .filter(|&&t| {
            !memories.iter().any(|m| {
                m.kind == "domain_knowledge" && (m.content.contains(t) || m.content.contains(&t.replace('_', " ")))
            }) && !known.iter().any(|k| k.contains(t))
        })
        .copied()
        .collect()
}

pub fn wikipedia_languages_not_read(memories: &[Memory]) -> Vec<&'static str> {
    world_languages::wikipedia_rotation()
        .iter()
        .filter(|(name, _, _)| {
            !memories.iter().any(|m| {
                m.kind == "language_knowledge" && m.content.contains(name)
            })
        })
        .map(|(name, _, _)| *name)
        .collect()
}

pub fn structural_unknowns() -> &'static str {
    "STRUCTURAL LIMITS (always unknown offline until fetched):\n\
     • ~7,000 living languages — KORE indexes ISO 639-1 (184 codes), not every dialect or oral language.\n\
     • No live web in every solver path — deep facts need self_fetch / Wikipedia / heartbeat.\n\
     • No symbolic CAS — advanced algebra, proof, equation balancing beyond heuristics.\n\
     • No personal/private world data unless you ingest it (self_ingest, self_fetch).\n\
     • Medicine/law/engineering — pointers only; not licensed professional advice.\n\
     • Time-sensitive news — stale until self_fetch or heartbeat refreshes."
}

pub fn full_report(memories: &[Memory], solver: &WorldSolverEngine) -> String {
    let missing_pri = missing_priority_domains(memories);
    let missing_ext = missing_extended_domains(memories);
    let langs_unread = wikipedia_languages_not_read(memories);
    let domain_n = memories
        .iter()
        .filter(|m| m.kind == "domain_knowledge")
        .count();
    let lang_n = memories
        .iter()
        .filter(|m| m.kind == "language_knowledge")
        .count();
    let unsolved = solver.unsolved_count();
    let solve_rate = if solver.attempts > 0 {
        (solver.successes as f64 / solver.attempts as f64) * 100.0
    } else {
        0.0
    };

    let mut out = String::from(
        "WHAT KORE-SELF DOES NOT KNOW (WORLD)\n\
         ===================================\n\
         Principle: KORE lists gaps first — then fills them via heartbeat, self_fill_gaps, self_fetch.\n\n",
    );

    out.push_str(&format!(
        "SOLVER GAPS: {} self_solve attempts, {:.0}% closed-form success, {} open (decomposed).\n",
        solver.attempts, solve_rate, unsolved
    ));
    if !solver.recent_unsolved.is_empty() {
        out.push_str("Recent questions KORE could not solve directly:\n");
        for q in &solver.recent_unsolved {
            out.push_str(&format!("  • {}\n", q));
        }
    }
    out.push('\n');

    out.push_str(&format!(
        "DOMAIN MEMORY GAPS: {}/{} priority Wikipedia domains ingested.\n",
        PRIORITY_DOMAIN_TOPICS.len().saturating_sub(missing_pri.len()),
        PRIORITY_DOMAIN_TOPICS.len()
    ));
    if !missing_pri.is_empty() {
        out.push_str("Missing priority domains:\n  ");
        out.push_str(&missing_pri.join(", "));
        out.push_str("\n\n");
    }

    if !missing_ext.is_empty() {
        out.push_str("Missing extended domains (sample):\n  ");
        out.push_str(&missing_ext.join(", "));
        out.push_str("\n\n");
    }

    out.push_str(&format!(
        "LANGUAGE GAPS: {} Wikipedia editions read; {} still unread in rotation (of {}).\n",
        lang_n,
        langs_unread.len(),
        world_languages::wikipedia_rotation().len()
    ));
    if !langs_unread.is_empty() {
        let preview: Vec<_> = langs_unread.iter().take(20).copied().collect();
        out.push_str("Not yet read: ");
        out.push_str(&preview.join(", "));
        if langs_unread.len() > 20 {
            out.push_str(&format!(" … +{} more", langs_unread.len() - 20));
        }
        out.push_str("\n\n");
    }

    out.push_str(&format!(
        "ISO 639-1: {} codes in catalog; ~{} living languages on Earth not individually stored.\n\n",
        world_languages::ISO639_1.len(),
        7000_usize.saturating_sub(world_languages::ISO639_1.len())
    ));

    out.push_str(&format!(
        "TECHNICAL: {} programming languages, {} shells, {} Linux commands built-in.\n\
         Heartbeat fills: Rust, Python, Bash, Linux, Git, Docker, Kubernetes, …\n\
         Catalog: self_world_catalog action=programming | shells | linux | technical\n\n",
        world_technical::PROGRAMMING_LANGUAGES.len(),
        world_technical::SHELLS.len(),
        world_technical::LINUX_COMMANDS.len()
    ));

    out.push_str("WEAK SOLVER AREAS (classify only — fetch or ingest for depth):\n  ");
    out.push_str(&WEAK_SOLVER_SUBJECTS.join(", "));
    out.push_str("\n\n");

    out.push_str(structural_unknowns());
    out.push_str("\n\nACTIONS:\n\
     • self_world_unknown — this report\n\
     • self_fill_self — fill gaps automatically (domains + languages)\n\
     • self_fill_gaps {\"topic\": \"…\"} — ingest one domain\n\
     • self_fetch source=wikipedia — live article\n\
     • self_solve — retry after you add memories\n");

    out.push_str(&format!(
        "\n(Session: {} domain_knowledge, {} language_knowledge memories.)",
        domain_n, lang_n
    ));
    out
}

pub fn brief_for_belief(memories: &[Memory], solver: &WorldSolverEngine) -> String {
    let missing = missing_priority_domains(memories);
    format!(
        "I explicitly track world unknowns: {} priority domains missing, {} Wikipedia languages unread, \
         {} programming languages indexed, {} self_solve decompositions. I know bash, linux, and technical gaps.",
        missing.len(),
        wikipedia_languages_not_read(memories).len(),
        world_technical::PROGRAMMING_LANGUAGES.len(),
        solver.unsolved_count()
    )
}

/// Next English Wikipedia topic KORE should ingest (priority list, then extended).
pub fn next_wikipedia_topic_to_fill(memories: &[Memory]) -> Option<(&'static str, String)> {
    for (slug, display) in PRIORITY_DOMAIN_TOPICS {
        if !memories.iter().any(|m| {
            m.kind == "domain_knowledge" && m.content.contains(display)
        }) {
            return Some((*slug, display.to_string()));
        }
    }
    for (slug, display) in world_technical::PRIORITY_TECH_TOPICS {
        if !memories.iter().any(|m| {
            m.kind == "domain_knowledge" && m.content.contains(display)
        }) {
            return Some((*slug, display.to_string()));
        }
    }
    for &slug in EXTENDED_DOMAIN_TOPICS {
        let display = slug.replace('_', " ");
        if !memories.iter().any(|m| {
            m.kind == "domain_knowledge"
                && (m.content.contains(slug) || m.content.contains(&display))
        }) {
            return Some((slug, display));
        }
    }
    None
}

pub fn fill_gaps_enabled(continuous: bool) -> bool {
    match std::env::var("KORE_FILL_GAPS") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        _ => continuous,
    }
}

pub fn domain_fill_burst(continuous: bool) -> usize {
    std::env::var("KORE_DOMAIN_BURST")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(if continuous { 2 } else { 1 })
        .clamp(1, 5)
}

/// Guess a Wikipedia article slug from free text (for curiosity / unsolved questions).
pub fn wiki_slug_from_text(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    for (slug, display) in PRIORITY_DOMAIN_TOPICS {
        if lower.contains(&display.to_lowercase()) || lower.contains(&slug.to_lowercase()) {
            return Some(slug.to_string());
        }
    }
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3)
        .take(4)
        .collect();
    if words.is_empty() {
        return None;
    }
    Some(words.join("_"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_slug_from_physics_question() {
        let slug = wiki_slug_from_text("What is quantum physics about?").unwrap();
        assert_eq!(slug, "Physics");
    }

    #[test]
    fn fill_gaps_follows_continuous_default() {
        std::env::remove_var("KORE_FILL_GAPS");
        assert!(fill_gaps_enabled(true));
        assert!(!fill_gaps_enabled(false));
    }

    #[test]
    fn next_topic_skips_known_domains() {
        let memories = vec![Memory {
            id: 1,
            timestamp: "2026".into(),
            content: "[Domain Knowledge: Physics @tick 1] summary".into(),
            kind: "domain_knowledge".into(),
            importance: 0.8,
            tags: vec![],
        }];
        let next = next_wikipedia_topic_to_fill(&memories).unwrap();
        assert_ne!(next.1, "Physics");
    }
}
