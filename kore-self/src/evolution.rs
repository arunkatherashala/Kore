// kore-self  —  Phase 6: Self-Evolution (Auto-Coding)
//
// "I read my own source. I find my own gaps. I write my own next feature."
//
// No external LLM. No scaffolding tool. kore-self reads its own .rs files,
// analyzes what it CANNOT do based on query patterns from ShadowObserver,
// proposes the next feature, generates valid Rust scaffold code,
// and writes it to disk — ready to compile.
//
// Loop:
//   1. self_read_source  → load all kore-self src/*.rs into memory
//   2. self_plan_feature → cross-match shadow.query_topics vs existing tools
//                          → identify gaps → propose next feature
//   3. self_evolve       → generate Rust scaffold → write to disk
//                          → suggest main.rs changes

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    #[serde(default)]
    pub last_evolved_at:  u64,
}

impl EvolutionEngine {
    pub fn new() -> Self {
        Self {
            proposals:        vec![],
            generated:        vec![],
            source_snapshot:  None,
            total_evolutions: 0,
            next_id:          1,
            last_evolved_at:  0,
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

    // ── Step 3: Generate code ─────────────────────────────────────────────────

    /// Generate Rust scaffold code for a proposal. Returns generated file content.
    /// If src_dir is Some, also writes to disk and runs a compile check before accepting.
    pub fn generate_code(
        &mut self,
        proposal: &mut FeatureProposal,
        src_dir:   Option<&Path>,
    ) -> GeneratedFile {
        // Safety gate: require a minimum gap score and a cooldown between auto-evolutions.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if !should_auto_trigger(proposal.gap_score, self.last_evolved_at, now, 0.30, 300) {
            eprintln!(
                "[kore-self:evolution] Auto-trigger blocked: gap_score={:.2}, cooldown active.",
                proposal.gap_score
            );
            proposal.status = "blocked".to_string();
            return GeneratedFile {
                filename:     format!("{}.rs", proposal.module_name),
                content:      String::new(),
                proposal_id:  proposal.id,
                generated_at: crate::now(),
                written_to:   String::new(),
            };
        }

        let code = match proposal.kind {
            FeatureKind::NewAnalyzer  => gen_analyzer(&proposal.module_name, &proposal.title, &proposal.evidence),
            FeatureKind::NewTracker   => gen_tracker(&proposal.module_name, &proposal.title, &proposal.evidence),
            FeatureKind::NewPredictor => gen_predictor(&proposal.module_name, &proposal.title, &proposal.evidence),
            FeatureKind::NewTool      => gen_tool(&proposal.module_name, &proposal.title, &proposal.evidence),
            FeatureKind::NewMemoryKind => gen_memory_kind(&proposal.module_name, &proposal.evidence),
        };

        let filename = format!("{}.rs", proposal.module_name);
        let mut written_to = String::new();
        let mut status = "generated".to_string();

        // Write to disk if src_dir provided
        if let Some(dir) = src_dir {
            let path = dir.join(&filename);
            if let Err(e) = fs::write(&path, code.as_bytes()) {
                eprintln!("[kore-self:evolution] Write failed: {e}");
            } else {
                written_to = path.to_string_lossy().to_string();
                if compile_check_generated(&path) {
                    status = "written".to_string();
                    self.last_evolved_at = now;
                    eprintln!("[kore-self:evolution] Generated and checked: {}", written_to);
                } else {
                    eprintln!(
                        "[kore-self:evolution] Generated file failed cargo check; removing and rejecting."
                    );
                    let _ = fs::remove_file(&path);
                    written_to.clear();
                    status = "rejected".to_string();
                }
            }
        }

        let gf = GeneratedFile {
            filename:     filename.clone(),
            content:      code,
            proposal_id:  proposal.id,
            generated_at: crate::now(),
            written_to:   written_to.clone(),
        };

        proposal.status = status.clone();
        if status != "blocked" {
            self.generated.push(gf.clone());
            self.total_evolutions += 1;
        }

        // Keep last 10 generated files
        if self.generated.len() > 10 {
            self.generated.remove(0);
        }

        gf
    }

    // ── main.rs patch suggestion ───────────────────────────────────────────────

    /// Returns the exact lines the user needs to add to main.rs to wire in the new module.
    pub fn main_rs_patch(proposal: &FeatureProposal) -> String {
        let m  = &proposal.module_name;
        let en = engine_name(m);
        let t  = tool_name(m);
        let handler = match proposal.kind {
            FeatureKind::NewTool => format!(
r#"        "{t}" => {{
            me.shadow.observe_tool("{t}");
            json!({{ "content": [{{ "type": "text", "text": me.{m}.run_tool(&me.memories) }}] }})
        }}"#
            ),
            _ => format!(
r#"        "{t}" => {{
            me.shadow.observe_tool("{t}");
            json!({{ "content": [{{ "type": "text", "text": me.{m}.to_json().to_string() }}] }})
        }}"#
            ),
        };
        format!(
r#"// ─── Add to main.rs ─────────────────────────────────────────────────────────

// 1. Top of file — add module:
mod {m};

// 2. In `pub struct KoreSelf` — add field:
    {m}: {m}::{en},

// 3. In `load_or_new` (both branches) — add:
    {m}: {m}::{en}::new(),

// 4. In `save()` — add to persistence::save() call:
    &self.{m},

// 5. In `handle_tool()` — add tool handler:
{handler}

// 6. In `tool_list()` — add tool entry:
      {{ "name": "{t}",
        "description": "Auto-generated {kind} for '{m}' analysis.",
        "inputSchema": {{ "type": "object", "properties": {{}} }}
      }},

// 7. In persistence.rs — import and add to SaveFile/save/load (same pattern as dream, shadow, etc.)
"#,
            m    = m,
            en   = en,
            t    = t,
            kind = proposal.kind,
            handler = handler,
        )
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

// ─── Code generators ──────────────────────────────────────────────────────────

fn gen_analyzer(module: &str, title: &str, topics: &[String]) -> String {
    let en   = engine_name(module);
    let _t   = tool_name(module);
    let kw   = topics.first().cloned().unwrap_or_else(|| module.to_string());
    format!(
r#"// kore-self — Auto-generated: {title}
// Generated by kore-self Phase 6 (Self-Evolution)
// Edit and expand as needed. Wire into main.rs using self_evolve patch output.

use serde::{{Deserialize, Serialize}};
use std::collections::HashMap;
use crate::Memory;

// ─── {en} ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {en}Pattern {{
    pub topic:      String,
    pub count:      u32,
    pub weight:     f64,
    pub last_seen:  String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {en} {{
    pub patterns:       Vec<{en}Pattern>,
    pub total_analyzed: u64,
    pub last_run:       String,
}}

impl {en} {{
    pub fn new() -> Self {{
        Self {{
            patterns:       vec![],
            total_analyzed: 0,
            last_run:       "never".to_string(),
        }}
    }}

    /// Analyze all memories for '{kw}'-related patterns.
    pub fn analyze(&mut self, memories: &[Memory]) -> Vec<String> {{
        if memories.is_empty() {{ return vec![]; }}

        let keywords = expand_keywords("{kw}");
        let mut freq: HashMap<String, (u32, f64)> = HashMap::new();

        for (i, m) in memories.iter().enumerate() {{
            let content_lc = m.content.to_lowercase();
            if let Some(matched) = keywords.iter().find(|k| content_lc.contains(*k)) {{
                let key = extract_phrase_around(&m.content, matched, 2);
                let recency = recency_weight(i, memories.len());
                let w = m.importance * (1.0 + recency);
                let e = freq.entry(key).or_insert((0, 0.0));
                e.0 += 1;
                e.1 += w;
            }}
        }}

        self.patterns.clear();
        for (topic, (count, weight)) in &freq {{
            self.patterns.push({en}Pattern {{
                topic:     topic.clone(),
                count:     *count,
                weight:    weight / *count as f64,
                last_seen: crate::now(),
            }});
        }}
        self.patterns.sort_by(|a, b| b.count.cmp(&a.count));
        self.patterns.truncate(50);

        self.total_analyzed += memories.len() as u64;
        self.last_run        = crate::now();

        // Return insights as memory content strings
        self.patterns.iter().take(3).map(|p| {{
            format!("[{en}] '{{}}' detected {{}} times (avg importance {{:.2}})", p.topic, p.count, p.weight)
        }}).collect()
    }}

    pub fn to_json(&self) -> serde_json::Value {{
        serde_json::json!({{
            "total_analyzed": self.total_analyzed,
            "last_run":       self.last_run,
            "patterns":       self.patterns.len(),
            "top_patterns":   self.patterns.iter().take(10).map(|p| serde_json::json!({{
                "topic":  p.topic,
                "count":  p.count,
                "weight": format!("{{:.2}}", p.weight),
            }})).collect::<Vec<_>>(),
        }})
    }}
}}

impl Default for {en} {{
    fn default() -> Self {{ Self::new() }}
}}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn extract_phrase_around(content: &str, keyword: &str, radius: usize) -> String {{
    let words: Vec<&str> = content.split_whitespace().collect();
    let kw = keyword.to_lowercase();
    for (i, w) in words.iter().enumerate() {{
        let w_clean = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if w_clean == kw || w_clean.contains(&kw) {{
            let start = i.saturating_sub(radius);
            let end = (i + radius + 1).min(words.len());
            return words[start..end]
                .iter()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
        }}
    }}
    kw
}}

fn extract_key(content: &str, keyword: &str) -> String {{
    extract_phrase_around(content, keyword, 1)
}}

fn expand_keywords(base: &str) -> Vec<String> {{
    let mut out = vec![base.to_lowercase()];
    if !base.ends_with('s') {{
        out.push(format!("{{}}s", base));
    }}
    if base.ends_with('s') && base.len() > 1 {{
        out.push(base[..base.len()-1].to_lowercase());
    }}
    match base.to_lowercase().as_str() {{
        "goal" | "goals" => {{ out.push("target".to_string()); out.push("objective".to_string()); }}
        "feel" | "feels" | "mood" | "moods" | "emotion" | "emotions" | "stress" => {{
            out.push("happy".to_string());
            out.push("sad".to_string());
            out.push("tired".to_string());
            out.push("angry".to_string());
        }}
        "learn" | "learning" | "skill" | "skills" | "study" => {{
            out.push("course".to_string());
            out.push("practice".to_string());
            out.push("read".to_string());
        }}
        "time" | "schedule" | "deadline" | "routine" | "daily" | "week" => {{
            out.push("plan".to_string());
            out.push("calendar".to_string());
            out.push("appointment".to_string());
        }}
        "problem" | "error" | "bug" | "issue" | "debug" | "fail" => {{
            out.push("fix".to_string());
            out.push("broken".to_string());
            out.push("crash".to_string());
        }}
        _ => {{}}
    }}
    out.sort();
    out.dedup();
    out
}}

fn recency_weight(index: usize, total: usize) -> f64 {{
    if total == 0 {{ return 1.0; }}
    (index as f64 + 1.0) / total as f64
}}
"#,
        title  = title,
        en     = en,
        kw     = kw,
    )
}

fn gen_tracker(module: &str, title: &str, topics: &[String]) -> String {
    let en  = engine_name(module);
    let kw  = topics.first().cloned().unwrap_or_else(|| module.to_string());
    format!(
r#"// kore-self — Auto-generated: {title}
// Generated by kore-self Phase 6 (Self-Evolution)

use serde::{{Deserialize, Serialize}};
use std::collections::VecDeque;
use crate::Memory;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {en}Event {{
    pub timestamp: String,
    pub signal:    String,
    pub intensity: f64,   // 0.0–1.0
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {en} {{
    pub events:          VecDeque<{en}Event>,  // ring buffer max 200
    pub current_state:   String,
    pub total_observed:  u64,
}}

impl {en} {{
    pub fn new() -> Self {{
        Self {{
            events:         VecDeque::new(),
            current_state:  "unknown".to_string(),
            total_observed: 0,
        }}
    }}

    /// Observe a new memory and extract '{kw}' signals.
    pub fn observe(&mut self, memory: &Memory) {{
        let content = memory.content.to_lowercase();
        let intensity = self.detect_intensity(&content, memory.importance);
        if intensity > 0.1 {{
            self.push_event(&memory.content, intensity);
        }}
    }}

    fn detect_intensity(&self, content: &str, importance: f64) -> f64 {{
        let signals = topic_signals("{kw}");
        let hits = signals.iter().filter(|&&s| content.contains(s)).count() as f64;
        if hits == 0.0 {{ 0.0 }} else {{ ((hits / signals.len() as f64) * importance).min(1.0) }}
    }}

    fn push_event(&mut self, signal: &str, intensity: f64) {{
        self.events.push_back({en}Event {{
            timestamp: crate::now(),
            signal:    signal.chars().take(100).collect(),
            intensity,
        }});
        if self.events.len() > 200 {{ self.events.pop_front(); }}
        self.total_observed += 1;
        // Update current state from recent trend
        self.current_state = self.infer_state();
    }}

    fn infer_state(&self) -> String {{
        if self.events.is_empty() {{ return "unknown".to_string(); }}
        let recent_avg = self.events.iter().rev().take(5)
            .map(|e| e.intensity).sum::<f64>() / 5.0_f64.min(self.events.len() as f64);
        if recent_avg > 0.7 {{ "high".to_string() }}
        else if recent_avg > 0.4 {{ "moderate".to_string() }}
        else {{ "low".to_string() }}
    }}

    fn topic_signals(kw: &str) -> Vec<&str> {{
        let base: &[&str] = match kw.to_lowercase().as_str() {{
            "feel" | "mood" | "emotion" | "stress" | "energy" | "tired" | "happy" | "sad" =>
                &["feel", "mood", "emotion", "stress", "energy", "tired", "happy", "sad", "angry", "anxious"],
            "goal" | "target" | "plan" | "milestone" | "objective" | "achieve" =>
                &["goal", "target", "plan", "milestone", "objective", "achieve", "complete", "finish"],
            "learn" | "skill" | "study" | "read" | "course" | "knowledge" =>
                &["learn", "skill", "study", "read", "course", "practice", "improve"],
            "team" | "collab" | "meeting" | "people" | "person" | "social" =>
                &["team", "collab", "meeting", "people", "person", "social", "conversation"],
            "time" | "schedule" | "deadline" | "routine" | "daily" | "week" =>
                &["time", "schedule", "deadline", "routine", "daily", "week", "appointment", "plan"],
            "error" | "bug" | "problem" | "fail" | "issue" | "debug" =>
                &["error", "bug", "problem", "fail", "issue", "debug", "broken", "crash"],
            _ => &[],
        }};
        let mut out = base.to_vec();
        let kl = kw.to_lowercase();
        if !out.iter().any(|s| s.to_lowercase() == kl) {{
            out.push(kw);
        }}
        out
    }}

    pub fn to_json(&self) -> serde_json::Value {{
        serde_json::json!({{
            "current_state":  self.current_state,
            "total_observed": self.total_observed,
            "event_count":    self.events.len(),
            "recent_events":  self.events.iter().rev().take(5).map(|e| serde_json::json!({{
                "when":      e.timestamp,
                "intensity": format!("{{:.2}}", e.intensity),
                "signal":    e.signal,
            }})).collect::<Vec<_>>(),
        }})
    }}
}}

impl Default for {en} {{
    fn default() -> Self {{ Self::new() }}
}}
"#,
        title = title,
        en    = en,
        kw    = kw,
    )
}

fn gen_predictor(module: &str, title: &str, topics: &[String]) -> String {
    let en = engine_name(module);
    let kw = topics.first().cloned().unwrap_or_else(|| module.to_string());
    format!(
r#"// kore-self — Auto-generated: {title}
// Generated by kore-self Phase 6 (Self-Evolution)

use serde::{{Deserialize, Serialize}};
use std::collections::HashMap;
use crate::Memory;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {en}Prediction {{
    pub context:    String,
    pub predicted:  String,
    pub confidence: f64,
    pub made_at:    String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {en} {{
    pub predictions:    Vec<{en}Prediction>,
    pub total_made:     u64,
    pub training_size:  usize,
    pub model:          HashMap<String, HashMap<String, usize>>,
}}

impl {en} {{
    pub fn new() -> Self {{
        Self {{
            predictions:   vec![],
            total_made:    0,
            training_size: 0,
            model:         HashMap::new(),
        }}
    }}

    /// Build a frequency table of contexts → outcomes from memory history.
    pub fn train(&mut self, memories: &[Memory]) {{
        let keywords = expand_keywords("{kw}");
        self.model.clear();
        for m in memories {{
            let content_lc = m.content.to_lowercase();
            if !keywords.iter().any(|k| content_lc.contains(k)) {{ continue; }}
            if let Some((ctx, outcome)) = find_context_and_outcome(&m.content, &keywords) {{
                *self.model.entry(ctx).or_default().entry(outcome).or_insert(0) += 1;
            }}
        }}
        self.training_size = memories.iter()
            .filter(|m| m.content.to_lowercase().contains("{kw}"))
            .count();
    }}

    /// Predict '{kw}'-related outcome for a given context.
    pub fn predict(&mut self, context: &str) -> Option<{en}Prediction> {{
        if self.training_size < 5 {{ return None; }}
        let ctx = context.to_lowercase();
        let (predicted, confidence) = self.most_likely_outcome(&ctx)?;
        let pred = {en}Prediction {{
            context:    context.to_string(),
            predicted,
            confidence,
            made_at:    crate::now(),
        }};
        self.predictions.push(pred.clone());
        self.predictions.truncate(20);
        self.total_made += 1;
        Some(pred)
    }}

    fn most_likely_outcome(&self, context: &str) -> Option<(String, f64)> {{
        if let Some(outcomes) = self.model.get(context) {{
            let total = outcomes.values().sum::<usize>() as f64;
            if total == 0.0 {{ return None; }}
            let best = outcomes.iter().max_by_key(|(_, c)| *c)?;
            return Some((best.0.clone(), (*best.1 as f64) / total));
        }}
        // Fallback: global most frequent outcome across all contexts.
        let mut global: HashMap<String, usize> = HashMap::new();
        let mut total = 0usize;
        for outcomes in self.model.values() {{
            for (out, c) in outcomes {{
                *global.entry(out.clone()).or_insert(0) += *c;
                total += *c;
            }}
        }}
        if total == 0 {{ return None; }}
        let best = global.iter().max_by_key(|(_, c)| *c)?;
        Some((best.0.clone(), (*best.1 as f64) / total as f64))
    }}

    pub fn to_json(&self) -> serde_json::Value {{
        serde_json::json!({{
            "total_made":    self.total_made,
            "training_size": self.training_size,
            "recent":        self.predictions.iter().rev().take(5).map(|p| serde_json::json!({{
                "context":    p.context,
                "predicted":  p.predicted,
                "confidence": format!("{{:.0}}%", p.confidence * 100.0),
            }})).collect::<Vec<_>>(),
        }})
    }}
}}

impl Default for {en} {{
    fn default() -> Self {{ Self::new() }}
}}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn expand_keywords(base: &str) -> Vec<String> {{
    let mut out = vec![base.to_lowercase()];
    if !base.ends_with('s') {{
        out.push(format!("{{}}s", base));
    }}
    if base.ends_with('s') && base.len() > 1 {{
        out.push(base[..base.len()-1].to_lowercase());
    }}
    out.sort();
    out.dedup();
    out
}}

fn find_context_and_outcome(content: &str, keywords: &[String]) -> Option<(String, String)> {{
    let words: Vec<&str> = content.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {{
        let w_clean = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if keywords.iter().any(|k| w_clean.contains(k)) {{
            let start = i.saturating_sub(2);
            let end = (i + 3).min(words.len());
            let context = words[start..end]
                .iter()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            let outcome = words.get(i + 1)
                .map(|next| next.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                .unwrap_or_else(|| "unknown".to_string());
            return Some((context, outcome));
        }}
    }}
    None
}}
"#,
        title = title,
        en    = en,
        kw    = kw,
    )
}

fn gen_tool(module: &str, title: &str, topics: &[String]) -> String {
    let en = engine_name(module);
    let kw = topics.first().cloned().unwrap_or_default();
    format!(
r#"// kore-self — Auto-generated tool module: {title}
// Generated by kore-self Phase 6 (Self-Evolution)
// Wire into main.rs using self_evolve patch output.

use serde::{{Deserialize, Serialize}};
use std::collections::HashMap;
use crate::Memory;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {en} {{
    pub total_queries: u64,
    pub last_run:      String,
}}

impl {en} {{
    pub fn new() -> Self {{
        Self {{
            total_queries: 0,
            last_run:      "never".to_string(),
        }}
    }}

    /// Analyze memories for '{kw}' and return a concise summary.
    pub fn analyze(&mut self, memories: &[Memory]) -> String {{
        let keyword = "{kw}".to_lowercase();
        let mut freq: HashMap<String, usize> = HashMap::new();
        let mut total = 0usize;
        for m in memories {{
            if m.content.to_lowercase().contains(&keyword) {{
                *freq.entry(m.kind.clone()).or_insert(0) += 1;
                total += 1;
            }}
        }}
        self.total_queries += 1;
        self.last_run = crate::now();
        if total == 0 {{
            return format!("No memories related to '{{}}' found.", "{kw}");
        }}
        let top: Vec<String> = freq.iter()
            .map(|(kind, count)| format!("{{}}: {{}}", kind, count))
            .collect();
        format!("'{{}}' tool results — {{}} matching, by kind: {{}}", "{kw}", total, top.join(", "))
    }}

    /// Generated tool logic entry point — calls analyze() and returns a text result.
    pub fn run_tool(&mut self, memories: &[Memory]) -> String {{
        self.analyze(memories)
    }}

    pub fn to_json(&self) -> serde_json::Value {{
        serde_json::json!({{
            "total_queries": self.total_queries,
            "last_run":      self.last_run,
        }})
    }}
}}

impl Default for {en} {{
    fn default() -> Self {{ Self::new() }}
}}
"#,
        title = title,
        en    = en,
        kw    = kw,
    )
}

fn gen_memory_kind(module: &str, topics: &[String]) -> String {
    let kw = topics.first().cloned().unwrap_or_else(|| module.to_string());
    format!(
r#"// kore-self — Auto-generated Memory Kind scaffold
// Suggested new memory kind: "{kw}"
//
// To use: call self_ingest with kind="{kw}"
// Then update identity.rs absorb() to recognize this kind.

/*
── Add to identity.rs  absorb() match ────────────────────────────────────────

            "{kw}" => {{
                // Kind-specific update for {kw} memories.
                self.bump_value("{kw}", importance * 0.06);
                self.thinking.perfectionism = lerp(self.thinking.perfectionism, 0.75, importance * 0.03);
                if importance >= 0.8 {{
                    self.thinking.risk_tolerance = lerp(self.thinking.risk_tolerance, 0.25, importance * 0.03);
                }}
                if content.chars().any(|c| c.is_ascii_digit()) {{
                    self.thinking.metrics_driven = lerp(self.thinking.metrics_driven, 0.8, importance * 0.03);
                }}
            }}

── Suggested template for users to call ──────────────────────────────────────

    self_ingest(
        content: "{{what happened, what you did, what you learned}}",
        kind: "{kw}",
        importance: 0.8
    )
*/
"#,
        kw = kw,
    )
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn engine_name(module: &str) -> String {
    // "goal_engine" → "GoalEngine"
    module.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None    => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

fn tool_name(module: &str) -> String {
    // "goal_engine" → "self_goal_engine"
    format!("self_{}", module)
}

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

fn should_auto_trigger(gap_score: f64, last_evolved_at: u64, now: u64, min_gap_score: f64, cooldown_secs: u64) -> bool {
    gap_score >= min_gap_score && (now.saturating_sub(last_evolved_at) >= cooldown_secs)
}

fn compile_check_generated(path: &Path) -> bool {
    let Some(src_dir) = path.parent() else { return false; };
    let Some(project_root) = src_dir.parent() else { return false; };
    match Command::new("cargo")
        .args(["check", "--message-format=short"])
        .current_dir(project_root)
        .output()
    {
        Ok(out) => {
            if !out.status.success() {
                eprintln!(
                    "[kore-self:evolution] cargo check stderr:\n{}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            out.status.success()
        }
        Err(e) => {
            eprintln!("[kore-self:evolution] Could not run cargo check: {e}");
            false
        }
    }
}

#[allow(dead_code)]
fn extract_phrase_around(content: &str, keyword: &str, radius: usize) -> String {
    let words: Vec<&str> = content.split_whitespace().collect();
    let kw = keyword.to_lowercase();
    for (i, w) in words.iter().enumerate() {
        let w_clean = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if w_clean == kw || w_clean.contains(&kw) {
            let start = i.saturating_sub(radius);
            let end = (i + radius + 1).min(words.len());
            return words[start..end]
                .iter()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    kw
}
