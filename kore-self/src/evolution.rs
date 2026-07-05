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

    // ── Step 3: Generate code ─────────────────────────────────────────────────

    /// Generate Rust scaffold code for a proposal. Returns generated file content.
    /// If src_dir is Some, also writes to disk.
    pub fn generate_code(
        &mut self,
        proposal: &mut FeatureProposal,
        src_dir:   Option<&Path>,
    ) -> GeneratedFile {
        let code = match proposal.kind {
            FeatureKind::NewAnalyzer  => gen_analyzer(&proposal.module_name, &proposal.title, &proposal.evidence),
            FeatureKind::NewTracker   => gen_tracker(&proposal.module_name, &proposal.title, &proposal.evidence),
            FeatureKind::NewPredictor => gen_predictor(&proposal.module_name, &proposal.title, &proposal.evidence),
            FeatureKind::NewTool      => gen_tool(&proposal.module_name, &proposal.title, &proposal.evidence),
            FeatureKind::NewMemoryKind => gen_memory_kind(&proposal.module_name, &proposal.evidence),
        };

        let filename = format!("{}.rs", proposal.module_name);
        let mut written_to = String::new();

        // Write to disk if src_dir provided
        if let Some(dir) = src_dir {
            let path = dir.join(&filename);
            if let Err(e) = fs::write(&path, code.as_bytes()) {
                eprintln!("[kore-self:evolution] Write failed: {e}");
            } else {
                written_to = path.to_string_lossy().to_string();
                eprintln!("[kore-self:evolution] Generated: {}", written_to);
            }
        }

        let gf = GeneratedFile {
            filename:     filename.clone(),
            content:      code,
            proposal_id:  proposal.id,
            generated_at: crate::now(),
            written_to:   written_to.clone(),
        };

        proposal.status = if written_to.is_empty() { "generated".to_string() } else { "written".to_string() };
        self.generated.push(gf.clone());
        self.total_evolutions += 1;

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
        "{t}" => {{
            me.shadow.observe_tool("{t}");
            json!({{ "content": [{{ "type": "text", "text": me.{m}.to_json().to_string() }}] }})
        }}

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
    let t    = tool_name(module);
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

        let mut freq: HashMap<String, (u32, f64)> = HashMap::new();

        for m in memories {{
            // TODO: add your detection logic here
            // Currently: detect memories containing '{kw}'
            if m.content.to_lowercase().contains("{kw}") {{
                let key = extract_key(&m.content);
                let e = freq.entry(key).or_insert((0, 0.0));
                e.0 += 1;
                e.1 += m.importance;
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

fn extract_key(content: &str) -> String {{
    content.split_whitespace()
        .next()
        .unwrap_or("unknown")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
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
        // TODO: add '{kw}'-specific detection signals here
        let signals = ["{kw}"];
        let hits = signals.iter().filter(|&&s| content.contains(s)).count() as f64;
        if hits == 0.0 {{ 0.0 }} else {{ (hits / signals.len() as f64) * importance }}
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
}}

impl {en} {{
    pub fn new() -> Self {{
        Self {{ predictions: vec![], total_made: 0, training_size: 0 }}
    }}

    /// Build prediction model from memory history.
    pub fn train(&mut self, memories: &[Memory]) {{
        // TODO: build '{kw}'-specific prediction model from memories
        self.training_size = memories.iter()
            .filter(|m| m.content.to_lowercase().contains("{kw}"))
            .count();
    }}

    /// Predict '{kw}'-related outcome for a given context.
    pub fn predict(&mut self, context: &str) -> Option<{en}Prediction> {{
        if self.training_size < 5 {{ return None; }}
        // TODO: implement prediction logic
        let pred = {en}Prediction {{
            context:    context.to_string(),
            predicted:  format!("[{kw} prediction placeholder — implement logic]"),
            confidence: 0.5,
            made_at:    crate::now(),
        }};
        self.predictions.push(pred.clone());
        self.predictions.truncate(20);
        self.total_made += 1;
        Some(pred)
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
"#,
        title = title,
        en    = en,
        kw    = kw,
    )
}

fn gen_tool(_module: &str, title: &str, topics: &[String]) -> String {
    let kw = topics.first().cloned().unwrap_or_default();
    format!(
r#"// kore-self — Auto-generated tool scaffold: {title}
// This is a lightweight tool — no new module needed.
// Add the handler below directly into handle_tool() in main.rs.

/*
── Add to handle_tool() in main.rs ─────────────────────────────────────────────

        "self_{kw}" => {{
            me.shadow.observe_tool("self_{kw}");
            // TODO: implement '{kw}' tool logic using existing engines
            let result = me.memories.iter()
                .filter(|m| m.content.to_lowercase().contains("{kw}"))
                .count();
            json!({{ "content": [{{ "type": "text", "text":
                format!("Found {{}} memories related to '{kw}'", result)
            }}] }})
        }}

── Add to tool_list() in main.rs ────────────────────────────────────────────────

      {{ "name": "self_{kw}",
        "description": "Auto-generated: {title}",
        "inputSchema": {{ "type": "object", "properties": {{
          "query": {{ "type": "string" }}
        }} }}
      }},
*/

// No new .rs file needed for a simple tool — implement inline in main.rs.
"#,
        title = title,
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
                // TODO: update values/thinking from {kw} memories
                self.thinking.metrics_driven = blend(self.thinking.metrics_driven, 0.6, w);
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
