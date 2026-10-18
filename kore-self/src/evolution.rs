// kore-self  —  gap suggestions (read-only)
//
// Reads kore-self's own source file names/tool names (read-only) and compares
// the topics in your memories with the existing tools, producing a plain TEXT
// suggestion for a feature. It NEVER writes source code or any file: the former
// code-generating `self_evolve` tool was removed for safety.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::shadow::ShadowObserver;
use crate::Memory;

// ─── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FeatureKind {
    NewAnalyzer,    // like dream.rs — analyzes memory patterns
    NewTracker,     // like shadow.rs — passively observes something
    NewPredictor,   // like predictive.rs — makes predictions
    NewTool,        // simple new MCP tool on existing data
    NewMemoryKind,  // new kind of memory the user should store
}

impl std::fmt::Display for FeatureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeatureKind::NewAnalyzer  => write!(f, "Analyzer"),
            FeatureKind::NewTracker   => write!(f, "Tracker"),
            FeatureKind::NewPredictor => write!(f, "Predictor"),
            FeatureKind::NewTool      => write!(f, "Tool"),
            FeatureKind::NewMemoryKind => write!(f, "MemoryKind"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureProposal {
    pub id:           u64,
    pub title:        String,
    pub module_name:  String,    // e.g. "emotion", "goal_tracker"
    pub kind:         FeatureKind,
    pub rationale:    String,    // WHY kore-self thinks this is needed
    pub evidence:     Vec<String>, // which queries / memory patterns
    pub gap_score:    f64,       // 0.0–1.0: how big is the gap this fills
    pub proposed_at:  String,
    pub status:       String,    // "proposed" | "generated" | "accepted" | "rejected"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedFile {
    pub filename:     String,
    pub content:      String,
    pub proposal_id:  u64,
    pub generated_at: String,
    pub written_to:   String,   // full path if written to disk, else ""
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub files:      Vec<String>,   // filenames
    pub tools:      Vec<String>,   // tool names parsed from source
    pub mod_count:  usize,
    pub line_count: usize,
    pub taken_at:   String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionEngine {
    pub proposals:        Vec<FeatureProposal>,
    pub generated:        Vec<GeneratedFile>,
    pub source_snapshot:  Option<SourceSnapshot>,
    pub total_evolutions: u32,
    pub next_id:          u64,
}

impl EvolutionEngine {
    pub fn new() -> Self {
        Self {
            proposals:        vec![],
            generated:        vec![],
            source_snapshot:  None,
            total_evolutions: 0,
            next_id:          1,
        }
    }

    // ── Step 1: Read own source ────────────────────────────────────────────────

    /// Read all .rs files in kore-self/src/ into a SourceSnapshot.
    /// Returns (snapshot, full_source_map)
    pub fn read_own_source(&mut self, src_dir: &Path) -> (SourceSnapshot, HashMap<String, String>) {
        let mut files     = vec![];
        let mut tools     = vec![];
        let mut line_count = 0;
        let mut source_map: HashMap<String, String> = HashMap::new();

        if let Ok(entries) = fs::read_dir(src_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "rs").unwrap_or(false) {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    if let Ok(content) = fs::read_to_string(&path) {
                        line_count += content.lines().count();
                        // Extract tool names: look for "tool_name" => { patterns
                        for line in content.lines() {
                            let l = line.trim();
                            if l.starts_with('"') && l.contains("\"self_") {
                                if let Some(start) = l.find("\"self_") {
                                    let rest = &l[start+1..];
                                    if let Some(end) = rest.find('"') {
                                        let tool = rest[..end].to_string();
                                        if !tools.contains(&tool) {
                                            tools.push(tool);
                                        }
                                    }
                                }
                            }
                        }
                        source_map.insert(name.clone(), content);
                        files.push(name);
                    }
                }
            }
        }
        files.sort();

        let snap = SourceSnapshot {
            mod_count:  files.len(),
            line_count,
            files:      files.clone(),
            tools:      tools.clone(),
            taken_at:   crate::now(),
        };
        self.source_snapshot = Some(snap.clone());
        (snap, source_map)
    }

    // ── Step 2: Plan next feature ─────────────────────────────────────────────

    /// Cross-match shadow query topics + memory content vs existing tools.
    /// Find the biggest gap → propose a new feature.
    pub fn plan_next_feature(
        &mut self,
        shadow:       &ShadowObserver,
        memories:     &[Memory],
        source_snap:  &SourceSnapshot,
    ) -> Option<FeatureProposal> {
        // Collect what the user queries about
        let mut topic_demand: HashMap<String, usize> = HashMap::new();
        for (topic, count) in &shadow.query_topics {
            *topic_demand.entry(topic.clone()).or_insert(0) += *count as usize;
        }
        // Also from memory content keywords
        for m in memories {
            for word in m.content.split_whitespace() {
                let w = clean(word);
                if w.len() >= 5 && !is_stop(&w) {
                    *topic_demand.entry(w).or_insert(0) += 1;
                }
            }
        }

        // Find topics NOT served by any existing tool
        let existing_tools_str = source_snap.tools.join(" ").to_lowercase();
        let existing_mods_str  = source_snap.files.join(" ").to_lowercase();

        let mut gaps: Vec<(String, usize)> = topic_demand.into_iter()
            .filter(|(topic, _)| {
                // not already in a tool name or module name
                !existing_tools_str.contains(topic.as_str()) &&
                !existing_mods_str.contains(topic.as_str())
            })
            .collect();
        gaps.sort_by(|a, b| b.1.cmp(&a.1));

        if gaps.is_empty() { return None; }

        // Top-3 gaps → build a coherent proposal
        let top_gaps: Vec<String> = gaps.iter().take(3).map(|(t, _)| t.clone()).collect();
        let total_demand: usize   = gaps.iter().take(3).map(|(_, c)| c).sum();
        let max_possible          = memories.len().max(1);
        let gap_score             = (total_demand as f64 / max_possible as f64).min(1.0);

        let (kind, module_name, title) = classify_gap(&top_gaps, memories);

        let rationale = format!(
            "kore-self detected '{}' as the highest-demand topic ({} occurrences) \
             with no existing tool or module to serve it. \
             Memory analysis shows this concept appears in {} memories. \
             Building a dedicated {} module would fill this gap.",
            top_gaps[0],
            gaps[0].1,
            memories.iter().filter(|m| m.content.to_lowercase().contains(&top_gaps[0])).count(),
            kind
        );

        let proposal = FeatureProposal {
            id:          self.next_id,
            title,
            module_name,
            kind,
            rationale,
            evidence:    top_gaps,
            gap_score,
            proposed_at: crate::now(),
            status:      "proposed".to_string(),
        };

        self.next_id += 1;
        self.proposals.push(proposal.clone());

        // Keep last 20
        if self.proposals.len() > 20 {
            self.proposals.remove(0);
        }

        Some(proposal)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total_evolutions":  self.total_evolutions,
            "proposals":         self.proposals.len(),
            "generated_files":   self.generated.len(),
            "source_snapshot":   self.source_snapshot.as_ref().map(|s| serde_json::json!({
                "files":      s.files,
                "tools":      s.tools,
                "line_count": s.line_count,
                "taken_at":   s.taken_at,
            })),
            "pending_proposals": self.proposals.iter()
                .filter(|p| p.status == "proposed")
                .map(|p| serde_json::json!({
                    "id":          p.id,
                    "title":       p.title,
                    "module":      p.module_name,
                    "kind":        p.kind.to_string(),
                    "gap_score":   format!("{:.0}%", p.gap_score * 100.0),
                    "evidence":    p.evidence,
                    "rationale":   p.rationale,
                }))
                .collect::<Vec<_>>(),
            "recent_generated": self.generated.iter().rev().take(3).map(|g| serde_json::json!({
                "file":     g.filename,
                "lines":    g.content.lines().count(),
                "written":  g.written_to,
                "when":     g.generated_at,
            })).collect::<Vec<_>>(),
        })
    }
}

impl Default for EvolutionEngine {
    fn default() -> Self { Self::new() }
}

// ─── Feature classification ───────────────────────────────────────────────────

fn classify_gap(topics: &[String], memories: &[Memory]) -> (FeatureKind, String, String) {
    let t = topics[0].to_lowercase();

    // Emotion/mood signals → tracker
    if t.contains("feel") || t.contains("mood") || t.contains("stress") || t.contains("emotion")
        || t.contains("energy") || t.contains("tired") || t.contains("happy")
    {
        return (FeatureKind::NewTracker,
            "emotion_tracker".to_string(),
            "Emotion & Mood Tracker — passively infers your emotional state from memory patterns".to_string());
    }

    // Goal/objective signals → analyzer
    if t.contains("goal") || t.contains("target") || t.contains("plan") || t.contains("milestone")
        || t.contains("objective") || t.contains("achieve")
    {
        return (FeatureKind::NewAnalyzer,
            "goal_engine".to_string(),
            "Goal Engine — tracks objectives, measures progress, detects blockers".to_string());
    }

    // Learning/skill signals → predictor
    if t.contains("learn") || t.contains("skill") || t.contains("study") || t.contains("read")
        || t.contains("course") || t.contains("knowl")
    {
        return (FeatureKind::NewPredictor,
            "learning_tracker".to_string(),
            "Learning Tracker — maps your knowledge growth over time".to_string());
    }

    // Collaboration/team signals → tracker
    if t.contains("team") || t.contains("collab") || t.contains("meet") || t.contains("people")
        || t.contains("person") || t.contains("social")
    {
        return (FeatureKind::NewTracker,
            "relationship_tracker".to_string(),
            "Relationship Tracker — maps people you interact with and context".to_string());
    }

    // Time/schedule signals → analyzer
    if t.contains("time") || t.contains("sched") || t.contains("deadline") || t.contains("week")
        || t.contains("daily") || t.contains("routine")
    {
        return (FeatureKind::NewAnalyzer,
            "time_engine".to_string(),
            "Time Engine — analyzes your temporal patterns and schedule adherence".to_string());
    }

    // Error/bug/problem signals → analyzer
    if t.contains("error") || t.contains("bug") || t.contains("problem") || t.contains("fail")
        || t.contains("issue") || t.contains("debug")
    {
        return (FeatureKind::NewAnalyzer,
            "problem_engine".to_string(),
            "Problem Engine — tracks recurring errors and resolution patterns".to_string());
    }

    // Memory kind signals
    let decision_count = memories.iter().filter(|m| m.kind == "decision").count();
    let insight_count  = memories.iter().filter(|m| m.kind == "insight").count();
    if insight_count < 3 && decision_count > 5 {
        return (FeatureKind::NewMemoryKind,
            "reflection_kind".to_string(),
            "Reflection Memory Kind — structured self-reflection after decisions".to_string());
    }

    // Default: generic analyzer for the top topic
    let module_name = format!("{}_engine", sanitize(&topics[0]));
    let title = format!("{} Engine — analyzes '{}' patterns in your memory", capitalize(&topics[0]), topics[0]);
    (FeatureKind::NewAnalyzer, module_name, title)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn sanitize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect::<String>().to_lowercase()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None    => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn clean(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

const EVO_STOPS: &[&str] = &[
    "would", "could", "should", "think", "about", "there", "their", "these",
    "those", "which", "while", "where", "other", "after", "before", "every",
    "often", "never", "always", "maybe", "being", "doing", "going", "thing",
    "things", "based", "makes", "might", "needs", "still", "great", "using",
    "built", "build", "layer", "kore", "self", "engine", "memory", "memories",
    "chose", "choose", "decided", "prefer", "single", "right", "always",
];

fn is_stop(w: &str) -> bool {
    EVO_STOPS.contains(&w)
}
