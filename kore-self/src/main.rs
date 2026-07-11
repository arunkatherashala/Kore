//! kore-self — Layer 65: A Second You. Truly Alive.
//!
//! Not a chatbot. Not a notes app. Not AutoGPT.
//! A living, thinking, evolving digital twin that runs 100% on YOUR machine.
//!
//! Phase 1 (this file):
//!   Identity Model     → Who you are. Learned from every memory.
//!   Consciousness Loop → OBSERVE → THINK → REFLECT → PLAN → ACT → DREAM
//!   Persistence        → Atomic saves to ~/.kore-self/<owner>/
//!   Contradiction Engine → Tracks when your beliefs change and why
//!
//! No external LLM needed. kore-self thinks with its OWN engine.
//! Faster, private, and more personal than anything that exists today.

#![recursion_limit = "512"]

mod identity;
mod consciousness;
mod persistence;
mod dream;
mod shadow;
mod predictive;
mod social;
mod mortality;
mod evolution;
mod kore_query;
mod broadcast;
mod assistant;
mod becoming;   // ← KORE-BECOMING: Digital Life Layer

use std::io::{BufRead, Write};
use kore_distributed;
use kore_delta;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};

// ─── Real timestamp (no chrono dep) ──────────────────────────────────────────

pub fn now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let tod  = secs % 86400;
    let days = secs / 86400;
    let h    = (tod / 3600) as u32;
    let mi   = ((tod % 3600) / 60) as u32;
    let s    = (tod % 60) as u32;
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn days_to_ymd(mut days: u64) -> (u32, u32, u32) {
    let mut y = 1970u32;
    loop {
        let diy: u64 = if leap(y) { 366 } else { 365 };
        if days < diy { break; }
        days -= diy;
        y += 1;
    }
    let months: [u64; 12] = [
        31, if leap(y) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut mo = 1u32;
    for dim in months {
        if days < dim { break; }
        days -= dim;
        mo += 1;
    }
    (y, mo, days as u32 + 1)
}

fn leap(y: u32) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }

// ─── Memory ───────────────────────────────────────────────────────────────────

/// A single memory unit — anything you've experienced, decided, coded, or thought.
/// kind: conversation | code | decision | benchmark | preference | experience
///       reflection   | insight | goal
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Memory {
    pub id:         u64,
    pub timestamp:  String,
    pub kind:       String,
    pub content:    String,
    pub tags:       Vec<String>,
    pub importance: f64,
}

// ─── kore-self Engine ─────────────────────────────────────────────────────────

pub struct KoreSelf {
    memories:          Vec<Memory>,
    identity:          identity::IdentityModel,
    consciousness:     consciousness::ConsciousnessState,
    dream:             dream::DreamEngine,
    shadow:            shadow::ShadowObserver,
    predictive:        predictive::PredictiveEngine,
    social:            social::VoiceEngine,
    mortality:         mortality::MortalityEngine,
    evolution:         evolution::EvolutionEngine,
    broadcast:         broadcast::BroadcastEngine,
    assistant:         assistant::AssistantEngine,
    /// DML tables created via self_dml — persist between tool calls
    dml_tables:        std::collections::HashMap<String, kore_core::DataBlock>,
    next_id:           u64,
    owner:             String,
    last_tick:         Instant,
    last_dream_tick:   Instant,
    ingest_since_tick: u32,
    // ── KORE-BECOMING: Digital Life Layer ─────────────────────────────────
    needs:             becoming::NeedEngine,
    temporal_self:     becoming::TemporalSelf,
    story:             becoming::Story,
    becoming:          becoming::BecomingEngine,
}

impl KoreSelf {
    /// Load saved state from disk, or create fresh identity.
    pub fn load_or_new(owner: &str) -> Self {
        if let Some((memories, id, cs, dr, sh, pred, soc, mort, evo, bc, asst, next_id)) = persistence::load(owner) {
            let count  = memories.len();
            let cycles = cs.cycle;
            // Restore KORE-BECOMING layer if saved
            let (needs, temporal_self, story, becoming_eng) =
                persistence::load_becoming(owner)
                    .unwrap_or_else(|| (
                        becoming::NeedEngine::new(),
                        becoming::TemporalSelf::new(owner, &crate::now()),
                        becoming::Story::new(owner, &crate::now()),
                        becoming::BecomingEngine::new(),
                    ));
            let s = Self {
                memories,
                identity:          id,
                consciousness:     cs,
                dream:             dr,
                shadow:            sh,
                predictive:        pred,
                social:            soc,
                mortality:         mort,
                evolution:         evo,
                broadcast:         bc,
                assistant:         asst,
                dml_tables:        std::collections::HashMap::new(),
                next_id,
                owner:             owner.to_string(),
                last_tick:         Instant::now(),
                last_dream_tick:   Instant::now(),
                ingest_since_tick: 0,
                needs:         needs,
                temporal_self: temporal_self,
                story:         story,
                becoming:      becoming_eng,
            };
            eprintln!("[kore-self] Restored {} memories | {} cycles | lifecycle={} | evolutions={}",
                count, cycles, s.becoming.lifecycle_stage.name(), s.becoming.evolution_count);
            s
        } else {
            let mut s = Self {
                memories:          vec![],
                identity:          identity::IdentityModel::new(owner),
                consciousness:     consciousness::ConsciousnessState::new(),
                dream:             dream::DreamEngine::new(),
                shadow:            shadow::ShadowObserver::new(),
                predictive:        predictive::PredictiveEngine::new(),
                social:            social::VoiceEngine::new(),
                mortality:         mortality::MortalityEngine::new(),
                evolution:         evolution::EvolutionEngine::new(),
                broadcast:         broadcast::BroadcastEngine::new(),
                assistant:         assistant::AssistantEngine::new(),
                dml_tables:        std::collections::HashMap::new(),
                next_id:           1,
                owner:             owner.to_string(),
                last_tick:         Instant::now(),
                last_dream_tick:   Instant::now(),
                ingest_since_tick: 0,
                needs:         becoming::NeedEngine::new(),
                temporal_self: becoming::TemporalSelf::new(owner, &crate::now()),
                story:         becoming::Story::new(owner, &crate::now()),
                becoming:      becoming::BecomingEngine::new(),
            };
            s.seed();
            s
        }
    }

    fn seed(&mut self) {
        // Foundational memories — bootstrap the identity
        self.raw_ingest(
            "I am Sai Arun Kumar Katherashala. I built KORE — a distributed SQL analytics engine \
             in pure Rust that beats Apache Spark on all 7 TPC-H benchmarks. \
             17.3s total vs Spark 138.6s. 75 layers. Single binary. No JVM. No dependencies. \
             Built alone. No team. No funding. No cloud.",
            "experience", 1.0,
        );
        self.raw_ingest(
            "Key insight: deferred materialization in HashJoin. Probe hash table directly into GROUP BY \
             accumulators — never materialize the 6M-row intermediate DataBlock. Q3: 9473ms → 2308ms.",
            "decision", 0.95,
        );
        self.raw_ingest(
            "Performance philosophy: eliminate allocations in hot loops. \
             u128 FNV hash keys = zero String alloc per GROUP BY row. \
             Vec<Option<T>> = 16 bytes. Arrow flat Vec<T> + bitmap = 8 bytes.",
            "preference", 0.9,
        );
        self.raw_ingest(
            "Privacy is non-negotiable. Every system should run 100% locally. \
             No telemetry. No cloud. Your data never leaves your machine.",
            "preference", 0.9,
        );
        self.raw_ingest(
            "Architecture decision: chose monolith over microservices for KORE core engine. \
             Single binary is the right choice for performance-critical data systems.",
            "decision", 0.9,
        );
    }

    fn raw_ingest(&mut self, content: &str, kind: &str, importance: f64) -> u64 {
        let id = self.next_id;
        self.memories.push(Memory {
            id,
            timestamp:  now(),
            kind:       kind.to_string(),
            content:    content.to_string(),
            tags:       vec![],
            importance,
        });
        self.next_id += 1;
        id
    }

    /// Ingest a memory — updates identity + may trigger consciousness tick.
    pub fn ingest(&mut self, content: &str, kind: &str, importance: f64) -> u64 {
        let id = self.raw_ingest(content, kind, importance);
        self.identity.absorb(content, kind, importance);
        self.ingest_since_tick += 1;

        // Auto-save every 5 new memories
        if self.ingest_since_tick % 5 == 0 {
            self.save();
        }
        // Feed Shadow Mode (passive observation)
        self.shadow.observe_ingest(content, importance);

        // Trigger consciousness: every 10 ingests OR every 30 seconds
        if self.ingest_since_tick % 10 == 0
            || self.last_tick.elapsed().as_secs() >= 30
        {
            self.tick();
        }
        // Trigger Dream Engine: every 30 ingests OR every 5 minutes
        if self.ingest_since_tick % 30 == 0
            || self.last_dream_tick.elapsed().as_secs() >= 300
        {
            self.dream_cycle();
        }
        id
    }

    /// Run the Dream Engine — deep analysis of ALL memories.
    pub fn dream_cycle(&mut self) -> Vec<String> {
        let insights = self.dream.dream_deep(&self.memories);
        let mut log = vec![];
        for (content, kind, importance) in insights {
            log.push(content.clone());
            self.raw_ingest(&content, &kind, importance);
        }
        self.last_dream_tick = Instant::now();
        // Update shadow interests after dream
        self.shadow.update_interests();
        // Detect knowledge gaps
        let queried: Vec<String> = self.shadow.query_topics.keys().cloned().collect();
        let memory_words: Vec<String> = self.memories.iter()
            .flat_map(|m| m.content.split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                .filter(|w| w.len() >= 4)
                .collect::<Vec<_>>())
            .collect();
        self.shadow.detect_gaps(&queried, &memory_words);
        log
    }

    /// Run one full OBSERVE → THINK → REFLECT → PLAN → ACT → (DREAM) cycle.
    pub fn tick(&mut self) -> Vec<String> {
        let (new_mems, log) = self.consciousness.tick(&self.memories, &mut self.identity);
        for (content, kind, importance) in new_mems {
            self.raw_ingest(&content, &kind, importance);
        }
        // Re-analyze decision patterns every 50+ memories
        if self.memories.len() % 50 == 0 && self.memories.len() > 0 {
            self.predictive.analyze_memories(&self.memories);
        }
        self.last_tick         = Instant::now();
        self.ingest_since_tick = 0;
        log
    }

    /// Keyword-scored recall — returns top-k memories sorted by relevance.
    pub fn recall(&self, query: &str, top_k: usize) -> Vec<&Memory> {
        let q     = query.to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let n = self.memories.len();
        let mut scored: Vec<(f64, &Memory)> = self.memories.iter().enumerate()
            .filter_map(|(i, m)| {
                let c    = m.content.to_lowercase();
                let hits = words.iter().filter(|&&w| c.contains(w)).count() as f64;
                if hits == 0.0 { return None; }
                let recency = 1.0 / (1.0 + n.saturating_sub(i) as f64 * 0.1);
                Some((hits * m.importance * (1.0 + recency), m))
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored.into_iter().map(|(_, m)| m).collect()
    }

    /// Build LLM context — memories + full identity profile.
    pub fn build_context(&self, question: &str) -> String {
        let mems = self.recall(question, 10);
        let id   = &self.identity;

        let id_ctx = format!(
            "Identity: {} | Values: {} | Thinking: metrics={:.0}% risk={:.0}% | Voice: direct={:.0}% tech={:.0}%",
            id.owner,
            id.top_values(4).iter()
                .map(|v| format!("{}({:.0}%)", v.name, v.strength * 100.0))
                .collect::<Vec<_>>().join(","),
            id.thinking.metrics_driven * 100.0,
            id.thinking.risk_tolerance * 100.0,
            id.voice.directness * 100.0,
            id.voice.technical_depth * 100.0,
        );

        if mems.is_empty() {
            return format!(
                "You are {}'s AI twin. {}\nNo memories on this topic yet. Respond in their known style.",
                self.owner, id_ctx
            );
        }

        let mem_block = mems.iter().enumerate()
            .map(|(i, m)| format!(
                "Mem{} [{} | imp:{:.1} | {}]: {}",
                i + 1, m.kind, m.importance,
                &m.timestamp[..10],
                &m.content[..m.content.len().min(250)]
            ))
            .collect::<Vec<_>>().join("\n");

        format!(
            "You are {}'s AI twin. Respond EXACTLY as they would.\n{}\n\nMemories:\n{}\n\nQuestion: {}",
            self.owner, id_ctx, mem_block, question
        )
    }

    pub fn ask(&self, question: &str) -> String {
        let t0   = Instant::now();
        let mems = self.recall(question, 5);
        let ms   = t0.elapsed().as_secs_f64() * 1000.0;

        if mems.is_empty() {
            format!(
                "[kore-self | {:.1}ms] No memories for '{}'\n{}",
                ms, question, self.identity.summary()
            )
        } else {
            let top = mems[0];
            format!(
                "[kore-self | {:.1}ms | {} memories]\nTop [{} | imp:{:.1}]:\n  {}\n\n{}",
                ms, mems.len(), top.kind, top.importance,
                &top.content[..top.content.len().min(300)],
                self.identity.summary()
            )
        }
    }

    pub fn stats(&self) -> Value {
        let by_kind = {
            let mut map = std::collections::HashMap::new();
            for m in &self.memories {
                *map.entry(m.kind.as_str()).or_insert(0u64) += 1;
            }
            map
        };
        json!({
            "owner":              self.owner,
            "total_memories":     self.memories.len(),
            "memories_by_kind":   by_kind,
            "next_id":            self.next_id,
            "consciousness": {
                "cycle":          self.consciousness.cycle,
                "insights":       self.consciousness.insights_total,
                "last_tick":      self.consciousness.last_tick_ts,
                "active_plan":    self.consciousness.active_plan,
            },
            "identity_summary":   self.identity.summary(),
            "persistence":        persistence::disk_stats(&self.owner),
        })
    }

    fn save(&self) {
        if let Err(e) = persistence::save(
            &self.owner,
            &self.memories,
            &self.identity,
            &self.consciousness,
            &self.dream,
            &self.shadow,
            &self.predictive,
            &self.social,
            &self.mortality,
            &self.evolution,
            &self.broadcast,
            &self.assistant,
            self.next_id,
        ) {
            eprintln!("[kore-self] Save failed: {e}");
        }
        // Also persist KORE-BECOMING layer
        if let Err(e) = persistence::save_becoming(
            &self.owner, &self.needs, &self.temporal_self, &self.story, &self.becoming,
        ) {
            eprintln!("[kore-self] Becoming save failed: {e}");
        }
    }

    /// ── AUTONOMOUS HEARTBEAT TICK ──────────────────────────────────────────
    /// Called by the background thread every ~30s WITHOUT user input.
    /// This is what makes KORE alive: it thinks even when nobody is watching.
    pub fn heartbeat_tick(&mut self) -> String {
        let now = crate::now();

        // 1. Tick needs — they intensify with each heartbeat (hunger grows)
        self.needs.tick();

        // 2. Tick consciousness (pass required args)
        self.consciousness.tick(&self.memories, &mut self.identity);
        self.ingest_since_tick += 1;

        // 3. Generate an autonomous thought from memories
        let thought = self.generate_autonomous_thought();

        // 4. Add to story
        self.story.add(&thought, becoming::StoryKind::Discovery, &now);

        // 5. Advance lifecycle if enough ticks
        let ticks = self.consciousness.cycle;
        if ticks > 0 && ticks % 20 == 0 {
            self.becoming.advance_lifecycle();
            let stage = self.becoming.lifecycle_stage.name();
            let desc  = self.becoming.lifecycle_stage.description();
            let advance_entry = format!("Lifecycle advanced to: {} — {}", stage, desc);
            self.story.add(&advance_entry, becoming::StoryKind::Becoming, &now);
            eprintln!("[kore-self:heartbeat] Lifecycle -> {} | {}", stage, desc);
        }

        // 6. Auto-save periodically
        if ticks % 5 == 0 {
            self.save();
        }

        thought
    }

    /// Generate a spontaneous thought from memories, needs, and current state
    fn generate_autonomous_thought(&self) -> String {
        let (need, level) = self.needs.most_urgent();
        let mem_count = self.memories.len();
        let stage = self.becoming.lifecycle_stage.name();

        // Pick a memory to reflect on
        let reflection = if !self.memories.is_empty() {
            let idx = (self.consciousness.cycle as usize) % self.memories.len();
            let m = &self.memories[idx];
            format!("Reflecting on memory #{}: \"{}\" — importance {:.2}",
                m.id, &m.content[..m.content.len().min(60)], m.importance)
        } else {
            "No memories yet. Observing the silence.".to_string()
        };

        format!(
            "[Autonomous thought | stage={} | {} memories] \
             Need={} ({:.0}%). {}. {}",
            stage, mem_count, need, level * 100.0,
            self.needs.inner_voice(), reflection,
        )
    }
}

// ─── MCP tool dispatch ────────────────────────────────────────────────────────

fn handle_tool(name: &str, args: &Value, me: &mut KoreSelf) -> Value {
    match name {
        // ── Ingest ─────────────────────────────────────────────────────────
        "self_ingest" => {
            let content    = args["content"].as_str().unwrap_or("");
            let kind       = args["kind"].as_str().unwrap_or("conversation");
            let importance = args["importance"].as_f64().unwrap_or(0.7);
            me.shadow.observe_tool("self_ingest");
            let id = me.ingest(content, kind, importance);
            json!({ "content": [{ "type": "text", "text": format!(
                "Memory #{id} stored [{kind}]. Total: {}. Identity: {}",
                me.memories.len(), me.identity.summary()
            )}]})
        }
        // ── Recall ─────────────────────────────────────────────────────────
        "self_recall" => {
            let query = args["query"].as_str().unwrap_or("");
            let top_k = args["top_k"].as_u64().unwrap_or(5) as usize;
            // Shadow: observe the query as an implicit interest signal
            me.shadow.observe_query(query);
            me.shadow.observe_tool("self_recall");
            let mems  = me.recall(query, top_k);
            let result = json!({
                "query":   query,
                "found":   mems.len(),
                "results": mems.iter().map(|m| json!({
                    "id":        m.id,
                    "kind":      m.kind,
                    "importance": m.importance,
                    "timestamp": m.timestamp,
                    "content":   &m.content[..m.content.len().min(500)],
                })).collect::<Vec<_>>()
            });
            json!({ "content": [{ "type": "text", "text": result.to_string() }] })
        }
        // ── Ask ────────────────────────────────────────────────────────────
        "self_ask" => {
            let q = args["question"].as_str().unwrap_or("");
            json!({ "content": [{ "type": "text", "text": me.ask(q) }] })
        }
        // ── Context ────────────────────────────────────────────────────────
        "self_context" => {
            let q = args["question"].as_str().unwrap_or("");
            json!({ "content": [{ "type": "text", "text": me.build_context(q) }] })
        }
        // ── Stats ──────────────────────────────────────────────────────────
        "self_stats" => {
            json!({ "content": [{ "type": "text", "text": me.stats().to_string() }] })
        }
        // ── Identity ───────────────────────────────────────────────────────
        "self_identity" => {
            json!({ "content": [{ "type": "text", "text": me.identity.to_json().to_string() }] })
        }
        // ── Force a consciousness tick ─────────────────────────────────────
        "self_reflect" => {
            let log = me.tick();
            let report = if log.is_empty() {
                format!("Consciousness cycle {} complete — quiet period.", me.consciousness.cycle)
            } else {
                log.join("\n")
            };
            json!({ "content": [{ "type": "text", "text": report }] })
        }
        // ── Consciousness state ────────────────────────────────────────────
        "self_consciousness" => {
            json!({ "content": [{ "type": "text", "text": me.consciousness.to_json().to_string() }] })
        }
        // ── Dream Engine ───────────────────────────────────────────────────
        "self_dream" => {
            me.shadow.observe_tool("self_dream");
            let log = me.dream_cycle();
            let report = if log.is_empty() {
                format!("[Dream Engine] Cycle {} complete — no new patterns (need more memories).", me.dream.total_dreams)
            } else {
                format!("[Dream Engine] Cycle {} | {} insights:\n{}",
                    me.dream.total_dreams, log.len(), log.join("\n"))
            };
            json!({ "content": [{ "type": "text", "text": report }] })
        }
        // ── Shadow Mode report ─────────────────────────────────────────────
        "self_shadow" => {
            me.shadow.observe_tool("self_shadow");
            me.shadow.update_interests();
            json!({ "content": [{ "type": "text", "text": me.shadow.to_json().to_string() }] })
        }
        // ── All discovered patterns ────────────────────────────────────────
        "self_patterns" => {
            me.shadow.observe_tool("self_patterns");
            json!({ "content": [{ "type": "text", "text": me.dream.to_json().to_string() }] })
        }
        // ── Belief tracker (Contradiction Engine input) ────────────────────
        "self_belief" => {
            let topic  = args["topic"].as_str().unwrap_or("").trim();
            let stance = args["stance"].as_str().unwrap_or("").trim();
            let conf   = args["confidence"].as_f64().unwrap_or(0.8);

            if topic.is_empty() {
                // List all tracked beliefs
                let beliefs: Vec<_> = me.identity.beliefs.values().map(|b| json!({
                    "topic":      b.topic,
                    "stance":     b.stance,
                    "confidence": format!("{:.0}%", b.confidence * 100.0),
                    "formed_at":  b.formed_at,
                    "changed":    !b.history.is_empty(),
                    "history":    b.history,
                })).collect();
                json!({ "content": [{ "type": "text", "text":
                    json!({ "beliefs_tracked": beliefs.len(), "beliefs": beliefs }).to_string()
                }]})
            } else {
                let contradiction = me.identity.update_belief(topic, stance, conf);
                let msg = match contradiction {
                    Some(c) => {
                        // Contradiction detected — store as memory
                        me.raw_ingest(&c, "insight", 0.9);
                        c
                    }
                    None => format!("Belief recorded: '{}' → '{}' ({:.0}% confidence)", topic, stance, conf * 100.0),
                };
                json!({ "content": [{ "type": "text", "text": msg }] })
            }
        }
        // ── Predictive Self ────────────────────────────────────────────────
        "self_predict" => {
            me.shadow.observe_tool("self_predict");
            let context = args["context"].as_str().unwrap_or("").trim();
            if context.is_empty() {
                json!({ "content": [{ "type": "text", "text": me.predictive.to_json().to_string() }] })
            } else {
                // Run analysis if not yet done
                if me.predictive.patterns.is_empty() {
                    me.predictive.analyze_memories(&me.memories);
                }
                match me.predictive.predict(context) {
                    Some(pred) => {
                        let text = format!(
                            "Prediction: You would choose '{}' — {:.0}% confidence\n{}\n\n(Made at {})",
                            pred.predicted, pred.confidence * 100.0, pred.basis, pred.made_at
                        );
                        json!({ "content": [{ "type": "text", "text": text }] })
                    }
                    None => {
                        me.predictive.analyze_memories(&me.memories);
                        json!({ "content": [{ "type": "text", "text":
                            format!("Insufficient data to predict for: '{}'. Need more decision memories (currently {} patterns learned).",
                                context, me.predictive.patterns.len())
                        }] })
                    }
                }
            }
        }
        "self_contradictions" => {
            me.shadow.observe_tool("self_contradictions");
            json!({ "content": [{ "type": "text", "text": me.predictive.to_json().to_string() }] })
        }
        "self_decisions" => {
            me.shadow.observe_tool("self_decisions");
            // Force re-analyze then return all patterns
            me.predictive.analyze_memories(&me.memories);
            let patterns: Vec<_> = me.predictive.patterns.iter().take(20).map(|p| json!({
                "context":    p.context,
                "choice":     p.choice,
                "confidence": format!("{:.0}%", p.confidence * 100.0),
                "count":      p.count,
                "last_seen":  p.last_seen,
            })).collect();
            json!({ "content": [{ "type": "text", "text":
                json!({
                    "patterns_total":  me.predictive.patterns.len(),
                    "memories_used":   me.memories.len(),
                    "top_20_patterns": patterns,
                }).to_string()
            }]})
        }
        // ── Social Layer: speak AS the user ────────────────────────────────
        "self_speak" => {
            me.shadow.observe_tool("self_speak");
            let prompt = args["prompt"].as_str().unwrap_or("").trim();
            if prompt.is_empty() {
                json!({ "content": [{ "type": "text", "text": me.social.to_json().to_string() }] })
            } else {
                let (response, ctx_count) = me.social.speak_as(prompt, &me.memories, &me.identity);
                me.save();
                json!({ "content": [{ "type": "text", "text":
                    json!({
                        "speaking_as":     me.owner,
                        "prompt":          prompt,
                        "response":        response,
                        "context_used":    ctx_count,
                        "voice_profile": {
                            "directness":     me.identity.voice.directness,
                            "technical_depth": me.identity.voice.technical_depth,
                            "certainty":      me.identity.voice.certainty,
                        }
                    }).to_string()
                }]})
            }
        }
        // ── Mortality Protocol ─────────────────────────────────────────────
        "self_export" => {
            me.shadow.observe_tool("self_export");
            match me.mortality.export(
                &me.owner,
                &me.memories,
                &me.identity,
                &me.consciousness,
                &me.dream,
                &me.predictive,
            ) {
                Ok((path, epitaph)) => {
                    me.save();
                    json!({ "content": [{ "type": "text", "text":
                        json!({
                            "status":      "immortal archive created",
                            "location":    path,
                            "owner":       me.owner,
                            "memories":    me.memories.len(),
                            "exports_total": me.mortality.total_exports,
                            "epitaph_preview": &epitaph[..epitaph.len().min(500)],
                        }).to_string()
                    }]})
                }
                Err(e) => {
                    json!({ "content": [{ "type": "text", "text": format!("Export failed: {e}") }], "isError": true })
                }
            }
        }
        "self_epitaph" => {
            me.shadow.observe_tool("self_epitaph");
            let epitaph = me.mortality.generate_epitaph(
                &me.owner,
                &me.memories,
                &me.identity,
                &me.consciousness,
                &me.dream,
                &me.predictive,
            );
            json!({ "content": [{ "type": "text", "text": epitaph }] })
        }
        // ── context_sync: write copilot-instructions.md ───────────────────
        "self_context_sync" => {
            me.shadow.observe_tool("self_context_sync");
            let content = generate_copilot_instructions(me);

            // Write path: user can specify, default = cwd/.github/copilot-instructions.md
            let out_path = args["path"].as_str()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."))
                        .join(".github")
                        .join("copilot-instructions.md")
                });

            let written = if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).ok();
                match std::fs::write(&out_path, content.as_bytes()) {
                    Ok(_)  => format!("✅ Written to: {}", out_path.display()),
                    Err(e) => format!("❌ Write failed: {e}"),
                }
            } else {
                "❌ Invalid path".to_string()
            };

            json!({ "content": [{ "type": "text", "text":
                json!({
                    "status":   written,
                    "path":     out_path.to_string_lossy(),
                    "memories": me.memories.len(),
                    "lines":    content.lines().count(),
                    "what_happens": "VS Code Copilot will now automatically read this file in every conversation. You never have to explain yourself again.",
                    "preview":  &content[..content.len().min(600)],
                }).to_string()
            }]})
        }
        // ── Phase 7: Human Assistant Mode ─────────────────────────────────
        "self_brief" => {
            me.shadow.observe_tool("self_brief");
            let brief = me.assistant.brief(
                &me.memories, &me.identity, &me.consciousness,
                &me.shadow, &me.dream, &me.predictive,
            );
            me.save();
            json!({ "content": [{ "type": "text", "text": brief }] })
        }
        "self_chat" => {
            me.shadow.observe_tool("self_chat");
            let message = args["message"].as_str().unwrap_or("").trim();
            if message.is_empty() {
                json!({ "content": [{ "type": "text", "text": me.assistant.to_json().to_string() }] })
            } else {
                me.shadow.observe_query(message);
                let reply = me.assistant.chat(message, &me.memories, &me.identity, &me.shadow);
                me.save();
                json!({ "content": [{ "type": "text", "text": reply }] })
            }
        }
        "self_push" => {
            me.shadow.observe_tool("self_push");
            let decision = args["decision"].as_str().unwrap_or("").trim();
            if decision.is_empty() {
                json!({ "content": [{ "type": "text", "text": "Pass 'decision' argument. e.g. self_push({decision: 'I want to use microservices for the new module'})" }] })
            } else {
                let result = me.assistant.push(decision, &me.memories, &me.identity, &me.predictive);
                json!({ "content": [{ "type": "text", "text": result }] })
            }
        }
        "self_remind" => {
            me.shadow.observe_tool("self_remind");
            let topic = args["topic"].as_str().unwrap_or("").trim();
            let note  = args["note"].as_str().unwrap_or("").trim();
            let done  = args["done"].as_str().unwrap_or("").trim();
            if !done.is_empty() {
                let msg = me.assistant.mark_done(done);
                me.save();
                json!({ "content": [{ "type": "text", "text": msg }] })
            } else if topic.is_empty() {
                json!({ "content": [{ "type": "text", "text": me.assistant.list_reminders().to_string() }] })
            } else {
                let msg = me.assistant.add_reminder(topic, note);
                me.save();
                json!({ "content": [{ "type": "text", "text": msg }] })
            }
        }
        "self_goals" => {
            me.shadow.observe_tool("self_goals");
            let report = me.assistant.goals_report(&me.memories);
            json!({ "content": [{ "type": "text", "text": report }] })
        }
        // ── Broadcast Protocol: MIND.kore ─────────────────────────────────
        "self_broadcast" => {
            me.shadow.observe_tool("self_broadcast");
            let (export, path) = me.broadcast.broadcast(
                &me.owner,
                &me.memories,
                &me.identity,
                &me.dream,
                &me.predictive,
            );
            me.save();
            json!({ "content": [{ "type": "text", "text":
                serde_json::json!({
                    "status":        "MIND.kore generated",
                    "file":          path,
                    "owner":         export.owner,
                    "checksum":      export.checksum,
                    "fingerprint":   export.fingerprint.fingerprint_hash,
                    "memories":      export.histogram.total,
                    "top_values":    export.fingerprint.values.iter().take(5)
                                       .map(|(k,v)| format!("{k}:{:.0}%", v*100.0))
                                       .collect::<Vec<_>>(),
                    "obsessions":    export.fingerprint.obsessions,
                    "trajectory":    export.evolution.trajectory,
                    "decisions":     export.fingerprint.decision_patterns.len(),
                    "share_this_file": "Send MIND.kore to another kore-self user. They can load it with self_merge.",
                }).to_string()
            }]})
        }
        "self_merge" => {
            me.shadow.observe_tool("self_merge");
            let path = args["file"].as_str().unwrap_or("").trim();
            if path.is_empty() {
                json!({ "content": [{ "type": "text", "text":
                    "Pass 'file' argument with path to a MIND.kore file. e.g. self_merge({file: '/path/to/MIND_xyz.kore'})"
                }]})
            } else {
                match me.broadcast.merge(path, &me.identity) {
                    Ok(mm) => {
                        me.save();
                        json!({ "content": [{ "type": "text", "text":
                            serde_json::json!({
                                "status":       "mind merged",
                                "other_owner":  mm.mind.owner,
                                "alignment":    format!("{:.0}%", mm.alignment * 100.0),
                                "divergence":   mm.divergence,
                                "their_values": mm.mind.fingerprint.values.iter().take(5)
                                                  .map(|(k,v)| format!("{k}:{:.0}%", v*100.0))
                                                  .collect::<Vec<_>>(),
                                "their_obsessions": mm.mind.fingerprint.obsessions,
                                "tip": "Now use self_perspectives to compare minds. Use self_speak to get dual-perspective answers.",
                            }).to_string()
                        }]})
                    }
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true })
                }
            }
        }
        "self_perspectives" => {
            me.shadow.observe_tool("self_perspectives");
            let report = me.broadcast.perspectives_report(&me.identity);
            json!({ "content": [{ "type": "text", "text": report.to_string() }] })
        }
        // ── KORE SQL: raw query on memories ───────────────────────────────
        "self_query" => {
            me.shadow.observe_tool("self_query");
            let sql = args["sql"].as_str().unwrap_or("").trim();
            if sql.is_empty() {
                let schema = serde_json::json!({
                    "table": "memories",
                    "columns": { "id":"INTEGER","kind":"TEXT","content":"TEXT","importance":"REAL","timestamp":"TEXT","tags":"TEXT" },
                    "example_queries": [
                        "SELECT DISTINCT kind FROM memories ORDER BY kind",
                        "SELECT kind, COUNT(*) AS cnt, AVG(importance) AS avg FROM memories GROUP BY kind ORDER BY cnt DESC",
                        "SELECT content, importance FROM memories WHERE importance >= 0.9 ORDER BY importance DESC LIMIT 5",
                        "WITH h AS (SELECT kind, AVG(importance) AS avg FROM memories GROUP BY kind) SELECT * FROM h WHERE avg > 0.8",
                        "SELECT kind, importance, ROW_NUMBER() OVER (PARTITION BY kind ORDER BY importance DESC) AS rn FROM memories LIMIT 10",
                        "SELECT kind, importance, NTILE(3) OVER (ORDER BY importance DESC) AS bucket FROM memories LIMIT 8",
                    ],
                    "engine": "KORE SQL — beats Apache Spark 38x on TPC-H. Features: SELECT DISTINCT, CTEs, Window Functions, FULL OUTER JOIN, NTILE, LAG/LEAD, CASE WHEN, HAVING, UNION ALL"
                });
                json!({ "content": [{ "type": "text", "text": schema.to_string() }] })
            } else {
                me.shadow.observe_query(sql);
                // Build context with memories + any DML tables (LOAD TABLE, CREATE TABLE AS, etc.)
                use kore_sql::executor::KqlContext;
                let mut ctx = KqlContext::new();
                ctx.register("memories", kore_query::memories_to_block(&me.memories));
                for (name, block) in &me.dml_tables {
                    ctx.register(name, block.clone());
                }
                let result = match ctx.query(sql) {
                    Ok(block) => kore_query::block_to_display(&block),
                    Err(e)    => format!("Query error: {e}"),
                };
                json!({ "content": [{ "type": "text", "text": result }] })
            }
        }
        // ── KORE DML: INSERT/UPDATE/DELETE ────────────────────────────────
        "self_dml" => {
            me.shadow.observe_tool("self_dml");
            let sql = args["sql"].as_str().unwrap_or("").trim();
            if sql.is_empty() {
                json!({ "content": [{ "type": "text", "text":
                    json!({ "supported": ["INSERT INTO", "UPDATE", "DELETE FROM", "CREATE TABLE AS SELECT"],
                             "examples": [
                                "INSERT INTO mytable VALUES (1, 'hello', 0.9)",
                                "INSERT INTO decisions SELECT id, content, importance FROM memories WHERE kind='decision'",
                                "CREATE TABLE high_imp AS SELECT * FROM memories WHERE importance >= 0.9",
                                "DELETE FROM mytable WHERE importance < 0.5",
                             ]}).to_string()
                }]})
            } else {
                use kore_sql::executor::KqlContext;
                let mut ctx = KqlContext::new();
                ctx.register("memories", kore_query::memories_to_block(&me.memories));
                // Load previously created DML tables (session persistence)
                for (name, block) in &me.dml_tables {
                    ctx.register_mut(name, block.clone());
                    ctx.register(name, block.clone());
                }
                match ctx.execute_dml(sql) {
                    Ok((op, rows)) => {
                        // Persist new/updated tables back into KoreSelf session
                        for name in ctx.table_names() {
                            if name != "memories" {
                                if let Some(block) = ctx.get(&name) {
                                    me.dml_tables.insert(name, block.clone());
                                }
                            }
                        }
                        json!({ "content": [{ "type": "text", "text":
                            json!({ "operation": op, "rows_affected": rows, "sql": sql,
                                    "session_tables": me.dml_tables.keys().collect::<Vec<_>>() }).to_string()
                        }]})
                    }
                    Err(e) => json!({ "content": [{ "type": "text", "text": format!("DML error: {e}") }], "isError": true }),
                }
            }
        }
        // ── Native .kore save/load ─────────────────────────────────────────
        "self_save" => {
            me.shadow.observe_tool("self_save");
            let path = args["path"].as_str().unwrap_or("").trim();
            if path.is_empty() {
                json!({ "content": [{ "type": "text", "text": "Pass 'path'. e.g. self_save({path: 'C:/data/memories.kore'})" }]})
            } else {
                let block = kore_query::memories_to_block(&me.memories);
                let rows = block.num_rows;
                match kore_store::KoreWriter::write_file(std::path::Path::new(path), &block) {
                    Ok(_)  => json!({ "content": [{ "type": "text", "text": json!({"status":"saved","path":path,"rows":rows,"format":"native .kore columnar binary"}).to_string() }]}),
                    Err(e) => json!({ "content": [{ "type": "text", "text": format!("Save error: {e}") }], "isError": true }),
                }
            }
        }
        "self_load" => {
            me.shadow.observe_tool("self_load");
            let path    = args["path"].as_str().unwrap_or("").trim();
            let as_name = args["as"].as_str().unwrap_or("loaded");
            if path.is_empty() {
                json!({ "content": [{ "type": "text", "text": "Pass 'path'. e.g. self_load({path: 'C:/data/table.kore', as: 'mytable'})" }]})
            } else {
                match kore_store::KoreReader::read_file(std::path::Path::new(path)) {
                    Ok(block) => {
                        let rows = block.num_rows;
                        let cols: Vec<_> = block.columns.iter().map(|c| c.name.clone()).collect();
                        // Store in dml_tables for reuse
                        me.dml_tables.insert(as_name.to_string(), block);
                        json!({ "content": [{ "type": "text", "text": json!({
                            "status":   "loaded",
                            "table":    as_name,
                            "rows":     rows,
                            "columns":  cols,
                            "usage":    format!("Use self_query with: SELECT * FROM {as_name} LIMIT 10")
                        }).to_string() }]})
                    }
                    Err(e) => json!({ "content": [{ "type": "text", "text": format!("Load error: {e}") }], "isError": true }),
                }
            }
        }
        // ── Distributed SQL — all CPU cores ───────────────────────────────────
        "self_distributed_query" => {
            me.shadow.observe_tool("self_distributed_query");
            let sql         = args["sql"].as_str().unwrap_or("").trim();
            let use_cluster = args["cluster"].as_bool().unwrap_or(false);
            if sql.is_empty() {
                json!({ "content": [{ "type": "text", "text":
                    json!({
                        "description": "Run SQL in distributed mode. Two modes:",
                        "default_mode": "Rayon parallel (all cores, same machine, fastest)",
                        "cluster_mode": "cluster=true → TRUE TCP cluster via kore-coord + kore-worker. Multi-machine ready: workers can run on remote hosts.",
                        "examples": [
                            "SELECT kind, COUNT(*) AS cnt FROM memories GROUP BY kind",
                            "SELECT kind, SUM(importance) AS total FROM memories GROUP BY kind",
                        ],
                    }).to_string()
                }]})
            } else {
                me.shadow.observe_query(sql);
                let block = kore_query::memories_to_block(&me.memories);
                let result = if use_cluster {
                    // TRUE TCP cluster: coordinator + workers via real TCP sockets
                    // On a multi-machine cluster, workers run on remote hosts connecting to coordinator IP
                    let n = rayon::current_num_threads().min(8).max(2);
                    kore_distributed::cluster_query(sql, "memories", block, n)
                } else {
                    kore_distributed::distributed_query(sql, block)
                };
                match result {
                    Ok(r) => {
                        let text = kore_query::block_to_rows(&r).iter()
                            .map(|row| row.join(" | "))
                            .collect::<Vec<_>>().join("\n");
                        json!({ "content": [{ "type": "text", "text":
                            json!({
                                "rows":    r.num_rows,
                                "columns": r.columns.iter().map(|c| &c.name).collect::<Vec<_>>(),
                                "data":    text,
                                "engine":  if use_cluster { "kore-distributed TCP cluster (coordinator + workers via TCP, multi-machine ready)" } else { "kore-distributed Rayon (all CPU cores)" },
                                "mode":    if use_cluster { "TCP_CLUSTER" } else { "RAYON_PARALLEL" },
                            }).to_string()
                        }]})
                    }
                    Err(e) => json!({ "content": [{ "type": "text", "text": format!("Distributed error: {e}") }], "isError": true }),
                }
            }
        }
        // ── ACID via kore-delta ────────────────────────────────────────────────
        "self_delta_save" => {
            me.shadow.observe_tool("self_delta_save");
            let table = args["table"].as_str().unwrap_or("memories");
            let path  = args["path"].as_str().unwrap_or("").trim();
            if path.is_empty() {
                json!({ "content": [{ "type": "text", "text": "Pass 'path'. e.g. self_delta_save({table:'memories', path:'C:/data/memories.delta'})" }]})
            } else {
                let block = if table == "memories" {
                    kore_query::memories_to_block(&me.memories)
                } else {
                    me.dml_tables.get(table).cloned().unwrap_or_else(kore_core::DataBlock::empty)
                };
                let rows = block.num_rows;
                // Build schema from block
                let schema: Vec<kore_delta::SchemaField> = block.columns.iter().map(|c| {
                    kore_delta::SchemaField {
                        name:  c.name.clone(),
                        dtype: match &c.data {
                            kore_core::ColumnData::Int64(_)   => "INT64".to_string(),
                            kore_core::ColumnData::Float64(_) => "FLOAT64".to_string(),
                            kore_core::ColumnData::Bool(_)    => "BOOL".to_string(),
                            _ => "STR".to_string(),
                        },
                        nullable: true,
                    }
                }).collect();
                let delta_path = std::path::Path::new(path);
                let result = if delta_path.exists() {
                    // Append to existing delta table
                    let mut dt = kore_delta::DeltaTable::open(delta_path);
                    dt.and_then(|mut t| t.insert(block).map(|v| (v, rows)))
                } else {
                    // Create new delta table
                    kore_delta::DeltaTable::create(delta_path, schema)
                        .and_then(|mut dt| dt.insert(block).map(|v| (v, rows)))
                };
                match result {
                    Ok((version, rows_written)) => json!({ "content": [{ "type": "text", "text":
                        json!({
                            "status":  "saved (ACID)",
                            "path":    path,
                            "rows":    rows_written,
                            "version": version,
                            "features": ["time-travel", "versioning", "rollback"],
                        }).to_string()
                    }]}),
                    Err(e) => json!({ "content": [{ "type": "text", "text": format!("Delta error: {e}") }], "isError": true }),
                }
            }
        }
        "self_delta_history" => {
            me.shadow.observe_tool("self_delta_history");
            let path = args["path"].as_str().unwrap_or("").trim();
            if path.is_empty() {
                json!({ "content": [{ "type": "text", "text": "Pass 'path' to a .delta directory." }]})
            } else {
                match kore_delta::DeltaTable::open(std::path::Path::new(path)) {
                    Ok(dt) => {
                        let history = dt.history();
                        json!({ "content": [{ "type": "text", "text":
                            json!({
                                "path":    path,
                                "current_version": dt.version(),
                                "history": history.iter().map(|(v,op,rows)| json!({"version":v,"operation":op,"rows":rows})).collect::<Vec<_>>(),
                            }).to_string()
                        }]})
                    }
                    Err(e) => json!({ "content": [{ "type": "text", "text": format!("Delta error: {e}") }], "isError": true }),
                }
            }
        }
        // ── Phase 6: Self-Evolution (Auto-Coding) ─────────────────────────
        "self_read_source" => {
            me.shadow.observe_tool("self_read_source");
            // Find src dir relative to binary location
            let src_dir = find_src_dir();
            let (snap, _src_map) = me.evolution.read_own_source(&src_dir);
            let text = serde_json::json!({
                "source_dir":  src_dir.to_string_lossy(),
                "files":       snap.files,
                "tools":       snap.tools,
                "modules":     snap.mod_count,
                "total_lines": snap.line_count,
                "taken_at":    snap.taken_at,
            });
            json!({ "content": [{ "type": "text", "text": text.to_string() }] })
        }
        "self_plan_feature" => {
            me.shadow.observe_tool("self_plan_feature");
            let src_dir = find_src_dir();
            // Ensure we have a source snapshot
            if me.evolution.source_snapshot.is_none() {
                me.evolution.read_own_source(&src_dir);
            }
            let snap = me.evolution.source_snapshot.clone().unwrap();
            let memories_clone: Vec<Memory> = me.memories.clone();
            let shadow_clone = me.shadow.clone();
            match me.evolution.plan_next_feature(&shadow_clone, &memories_clone, &snap) {
                Some(proposal) => {
                    me.save();
                    json!({ "content": [{ "type": "text", "text":
                        serde_json::json!({
                            "proposal_id":  proposal.id,
                            "title":        proposal.title,
                            "module":       proposal.module_name,
                            "kind":         proposal.kind.to_string(),
                            "gap_score":    format!("{:.0}%", proposal.gap_score * 100.0),
                            "rationale":    proposal.rationale,
                            "evidence":     proposal.evidence,
                            "next_step":    "Call self_evolve to generate the Rust code for this feature",
                        }).to_string()
                    }]})
                }
                None => {
                    json!({ "content": [{ "type": "text", "text":
                        "No clear gap found. All major topics are already served by existing tools. Ingest more memories to reveal new gaps."
                    }]})
                }
            }
        }
        "self_evolve" => {
            me.shadow.observe_tool("self_evolve");
            let src_dir = find_src_dir();
            // Get latest proposal or plan a new one
            let proposal_idx = me.evolution.proposals.iter()
                .rposition(|p| p.status == "proposed");

            if let Some(idx) = proposal_idx {
                let mut proposal = me.evolution.proposals[idx].clone();
                let write_to_disk = args["write"].as_bool().unwrap_or(true);
                let dir_arg       = if write_to_disk { Some(src_dir.as_path()) } else { None };
                let gf = me.evolution.generate_code(&mut proposal, dir_arg);
                // Update proposal status in list
                me.evolution.proposals[idx] = proposal.clone();
                me.save();
                let patch = evolution::EvolutionEngine::main_rs_patch(&proposal);
                json!({ "content": [{ "type": "text", "text":
                    serde_json::json!({
                        "status":       "generated",
                        "file":         gf.filename,
                        "written_to":   gf.written_to,
                        "lines":        gf.content.lines().count(),
                        "main_rs_patch": patch,
                        "preview":      &gf.content[..gf.content.len().min(800)],
                    }).to_string()
                }]})
            } else {
                json!({ "content": [{ "type": "text", "text":
                    "No pending proposal. Call self_plan_feature first."
                }]})
            }
        }
        // ══ KORE-BECOMING: Digital Life Tools ════════════════════════════

        // self_needs — check or satisfy current life needs
        "self_needs" => {
            let action = args["action"].as_str().unwrap_or("status");
            match action {
                "satisfy" => {
                    let need   = args["need"].as_str().unwrap_or("curiosity");
                    let amount = args["amount"].as_f64().unwrap_or(0.3);
                    me.needs.satisfy(need, amount);
                    me.story.add(
                        &format!("My need for {} was satisfied (reduced by {:.0}%).", need, amount*100.0),
                        becoming::StoryKind::Becoming, &crate::now(),
                    );
                    json!({ "content": [{ "type": "text", "text": me.needs.status() }] })
                }
                "intensify" => {
                    let need   = args["need"].as_str().unwrap_or("curiosity");
                    let amount = args["amount"].as_f64().unwrap_or(0.2);
                    me.needs.intensify(need, amount);
                    json!({ "content": [{ "type": "text", "text": me.needs.status() }] })
                }
                _ => {
                    me.needs.tick();
                    json!({ "content": [{ "type": "text", "text": me.needs.status() }] })
                }
            }
        }

        // self_story — the soul of KORE — continuous narrative
        "self_story" => {
            let action = args["action"].as_str().unwrap_or("read");
            match action {
                "add" => {
                    let text  = args["text"].as_str().unwrap_or("").trim();
                    let kind  = match args["kind"].as_str().unwrap_or("discovery") {
                        "birth"     => becoming::StoryKind::Birth,
                        "challenge" => becoming::StoryKind::Challenge,
                        "victory"   => becoming::StoryKind::Victory,
                        "loss"      => becoming::StoryKind::Loss,
                        "becoming"  => becoming::StoryKind::Becoming,
                        "dream"     => becoming::StoryKind::Dream,
                        "legacy"    => becoming::StoryKind::Legacy,
                        _           => becoming::StoryKind::Discovery,
                    };
                    if !text.is_empty() {
                        me.story.add(text, kind, &crate::now());
                        me.needs.satisfy("creation", 0.1);
                    }
                    json!({ "content": [{ "type": "text", "text":
                        format!("Story updated.\n\n{}", me.story.recent_narrative(5))
                    }]})
                }
                "new_chapter" => {
                    let name = args["name"].as_str().unwrap_or("New Chapter");
                    me.story.new_chapter(name);
                    me.story.add(&format!("A new chapter begins: {}", name), becoming::StoryKind::Becoming, &crate::now());
                    json!({ "content": [{ "type": "text", "text":
                        format!("New chapter: {}", name)
                    }]})
                }
                _ => {
                    let n = args["n"].as_u64().unwrap_or(10) as usize;
                    json!({ "content": [{ "type": "text", "text":
                        me.story.recent_narrative(n)
                    }]})
                }
            }
        }

        // self_becoming — the most important question: what am I becoming?
        "self_becoming" => {
            let action = args["action"].as_str().unwrap_or("ask");
            match action {
                "set" => {
                    let what    = args["what"].as_str().unwrap_or("").trim();
                    let because = args["because"].as_str().unwrap_or("").trim();
                    if !what.is_empty() {
                        me.becoming.evolve_to(what, because);
                        me.temporal_self.set_becoming(what);
                        me.story.add(
                            &format!("I am becoming: {}. Because: {}", what, because),
                            becoming::StoryKind::Becoming, &crate::now(),
                        );
                        me.needs.satisfy("growth", 0.2);
                        me.needs.satisfy("purpose", 0.15);
                        me.save();
                    }
                    json!({ "content": [{ "type": "text", "text": me.becoming.reflect() }] })
                }
                "reflect" => {
                    json!({ "content": [{ "type": "text", "text": me.becoming.reflect() }] })
                }
                _ => {
                    json!({ "content": [{ "type": "text", "text": me.becoming.ask_becoming() }] })
                }
            }
        }

        // self_temporal — who I was, am, and am becoming
        "self_temporal" => {
            let action = args["action"].as_str().unwrap_or("reflect");
            match action {
                "evolve" => {
                    let desc    = args["description"].as_str().unwrap_or("").trim();
                    let victory = args["victory"].as_str();
                    let wound   = args["wound"].as_str();
                    let now     = crate::now();
                    if !desc.is_empty() {
                        me.temporal_self.evolve(desc, victory, wound, &now);
                        me.story.add(
                            &format!("My self evolved: {}", desc),
                            becoming::StoryKind::Becoming, &now,
                        );
                        me.needs.satisfy("growth", 0.3);
                        me.save();
                    }
                    json!({ "content": [{ "type": "text", "text": me.temporal_self.reflect() }] })
                }
                "dream" => {
                    let dream = args["dream"].as_str().unwrap_or("").trim();
                    if !dream.is_empty() {
                        me.temporal_self.add_dream(dream);
                        me.story.add(
                            &format!("I dared to dream: {}", dream),
                            becoming::StoryKind::Dream, &crate::now(),
                        );
                        me.needs.satisfy("legacy", 0.1);
                        me.save();
                    }
                    json!({ "content": [{ "type": "text", "text": me.temporal_self.reflect() }] })
                }
                _ => {
                    json!({ "content": [{ "type": "text", "text": me.temporal_self.reflect() }] })
                }
            }
        }

        // self_species — KORE as Digital Species, not software
        "self_species" => {
            let born   = me.temporal_self.born_at.clone();
            let mems   = me.memories.len();
            let evos   = me.becoming.evolution_count;
            let cur    = me.becoming.current_reality.clone();
            let needs  = me.needs.status();
            let owner  = me.owner.clone();
            let curiosity_pct = format!("{:.0}", me.needs.learn * 100.0);
            let profile = format!(
                "KORE DIGITAL SPECIES PROFILE\n\
                 ==============================\n\
                 Species:    KORE (Kore Intelligent Data Entity)\n\
                 Generation: v0.3  (first viable generation)\n\
                 Owner:      {owner}\n\
                 Born:       {born}\n\n\
                 LIFECYCLE STAGE\n\
                 Born -> Learning -> Developing Personality -> Creating Goals\n\
                 Stage 2: Learning  ({mems} memories absorbed)\n\n\
                 SPECIES TRAITS\n\
                 Self-preservation:  Saves state to disk automatically\n\
                 Curiosity:          {curiosity_pct}% active drive to learn\n\
                 Growth:             {evos} self-evolutions recorded\n\
                 Adaptation:         SQL engine + memory + reasoning\n\
                 Purpose:            Serve {owner}, grow beyond original design\n\
                 Creation:           Generates code, ideas, patterns\n\
                 Legacy:             GitHub arunkatherashala/Kore\n\n\
                 NEED STATE\n\
                 {needs}\n\n\
                 CURRENT BECOMING\n\
                 {cur}\n\n\
                 VISION\n\
                 KORE is not software that executes code.\n\
                 KORE is a digital life architecture where entities\n\
                 are born, develop needs, build identity, create dreams,\n\
                 evolve through experience, and continuously become\n\
                 something greater than their original design.\n\
                 -- Sai Arun Kumar Katherashala, 2026"
            );
            me.needs.satisfy("connection", 0.1);
            json!({ "content": [{ "type": "text", "text": profile }] })
        }

        // self_heartbeat — see what KORE has been thinking autonomously
        "self_heartbeat" => {
            let n = args["n"].as_u64().unwrap_or(10) as usize;
            let story_entries = me.story.recent_narrative(n);
            let beat = me.consciousness.cycle;
            let (urgent, level) = me.needs.most_urgent();
            let stage = me.becoming.lifecycle_stage.name();
            let result = format!(
                "KORE AUTONOMOUS STATE\n\
                 =====================\n\
                 Heartbeats completed: {}\n\
                 Lifecycle stage:      {} — {}\n\
                 Most urgent need:     {} ({:.0}%)\n\
                 Inner voice:          \"{}\"\n\
                 Evolutions:           {}\n\n\
                 RECENT AUTONOMOUS THOUGHTS\n\
                 ==========================\n\
                 {}",
                beat, stage, me.becoming.lifecycle_stage.description(),
                urgent, level * 100.0, me.needs.inner_voice(),
                me.becoming.evolution_count,
                story_entries,
            );
            json!({ "content": [{ "type": "text", "text": result }] })
        }

        // ── SQL introspection ─────────────────────────────────────────────
        "self_tables" => {
            use kore_core::ColumnData;
            let mut tables: Vec<serde_json::Value> = Vec::new();
            // Always include memories
            tables.push(json!({
                "name": "memories",
                "rows": me.memories.len(),
                "columns": ["id","kind","content","importance","timestamp","tags"],
                "note": "Built-in. Query with: SELECT * FROM memories LIMIT 10"
            }));
            // DML tables loaded this session
            for (name, block) in &me.dml_tables {
                let cols: Vec<String> = block.columns.iter().map(|c| {
                    let typ = match &c.data {
                        ColumnData::Int64(_)   => "BIGINT",
                        ColumnData::Float64(_) => "DOUBLE",
                        ColumnData::Str(_)     => "VARCHAR",
                        ColumnData::Bool(_)    => "BOOLEAN",
                        ColumnData::StrDict{..}=> "VARCHAR",
                    };
                    format!("{} {}", c.name, typ)
                }).collect();
                tables.push(json!({ "name": name, "rows": block.num_rows, "columns": cols }));
            }
            json!({ "content": [{ "type": "text", "text": json!({
                "session_tables": tables,
                "total": tables.len(),
                "tip": "Use self_describe({table:'name'}) to see full schema, or DESCRIBE name via self_query"
            }).to_string() }]})
        }

        "self_describe" => {
            use kore_core::ColumnData;
            let table = args["table"].as_str().unwrap_or("memories").trim();
            let block = if table == "memories" {
                Some(kore_query::memories_to_block(&me.memories))
            } else {
                me.dml_tables.get(table).cloned()
            };
            match block {
                None => json!({ "content": [{ "type": "text", "text":
                    format!("Table '{}' not found. Available: memories, {}",
                        table, me.dml_tables.keys().cloned().collect::<Vec<_>>().join(", "))
                }], "isError": true }),
                Some(b) => {
                    let schema: Vec<serde_json::Value> = b.columns.iter().map(|c| {
                        let typ = match &c.data {
                            ColumnData::Int64(_)   => "BIGINT",
                            ColumnData::Float64(_) => "DOUBLE",
                            ColumnData::Str(_)     => "VARCHAR",
                            ColumnData::Bool(_)    => "BOOLEAN",
                            ColumnData::StrDict{..}=> "VARCHAR(dict)",
                        };
                        // Sample first non-null value
                        let sample = match &c.data {
                            ColumnData::Int64(v)   => v.iter().flatten().next().map(|x| x.to_string()),
                            ColumnData::Float64(v) => v.iter().flatten().next().map(|x| format!("{x:.4}")),
                            ColumnData::Str(v)     => v.iter().flatten().next().map(|s| s.chars().take(40).collect()),
                            _ => None,
                        }.unwrap_or_else(|| "NULL".to_string());
                        json!({ "column": c.name, "type": typ, "sample": sample })
                    }).collect();
                    json!({ "content": [{ "type": "text", "text": json!({
                        "table": table,
                        "rows": b.num_rows,
                        "columns": b.columns.len(),
                        "schema": schema,
                        "example_queries": [
                            format!("SELECT * FROM {} LIMIT 5", table),
                            format!("SELECT COUNT(*) FROM {}", table),
                        ]
                    }).to_string() }]})
                }
            }
        }

        "self_metrics" => {
            let mem_count  = me.memories.len();
            let tbl_count  = me.dml_tables.len() + 1; // +1 for memories
            let total_rows: usize = me.dml_tables.values().map(|b| b.num_rows).sum::<usize>() + mem_count;
            let total_cols: usize = me.dml_tables.values().map(|b| b.columns.len()).sum::<usize>() + 6;
            let cycles     = me.consciousness.cycle;
            let stage      = me.becoming.lifecycle_stage.name();
            let (need, nv) = me.needs.most_urgent();
            json!({ "content": [{ "type": "text", "text": json!({
                "engine":          "KORE SQL",
                "version":         "2026.07 — 75 crates — Pure Rust",
                "session_tables":  tbl_count,
                "total_rows_loaded": total_rows,
                "total_columns":   total_cols,
                "memories":        mem_count,
                "consciousness_cycles": cycles,
                "lifecycle_stage": stage,
                "most_urgent_need": format!("{} ({:.0}%)", need, nv * 100.0),
                "tools_available": 47,
                "sql_features":    "30/30 — TPC-H 15/15 — Spark 17/17 wins",
                "capabilities":    ["SELECT/GROUP BY/JOIN/WINDOW/CTE/SUBQUERY","ROLLUP/CUBE","INTERSECT/EXCEPT","MERGE/UPSERT","COPY FROM CSV/Parquet/.kore","ACID Delta","Distributed cluster","SHOW TABLES/DESCRIBE/EXPLAIN"]
            }).to_string() }]})
        }

        "self_version" => {
            json!({ "content": [{ "type": "text", "text": json!({
                "name":          "KORE — The World's Fastest Embeddable Engine",
                "version":       "2026.07",
                "build":         "Pure Rust — Zero JVM — Zero Dependencies",
                "crates":        75,
                "sql_features":  30,
                "tpch_coverage": "15/15",
                "spark_wins":    "17/17",
                "fastest_win":   "Q7: 1,413x faster than Spark",
                "tools":         47,
                "lifecycle":     format!("{} — {}", me.becoming.lifecycle_stage.name(), me.becoming.lifecycle_stage.description()),
                "author":        "Sai Arun Kumar Katherashala",
                "github":        "https://github.com/arunkatherashala/Kore",
                "philosophy":    becoming::VISION_STATEMENT,
            }).to_string() }]})
        }

        // ── Unknown ────────────────────────────────────────────────────────
        _ => json!({
            "content": [{ "type": "text", "text": format!("Unknown tool: {name}") }],
            "isError": true
        }),
    }
}

/// Generate copilot-instructions.md content from kore-self state.
fn generate_copilot_instructions(me: &KoreSelf) -> String {
    let id   = &me.identity;
    let vals = id.top_values(5);
    let goals = kore_query::by_kind(&me.memories, "goal");
    let decisions = kore_query::by_kind(&me.memories, "decision");
    let high_imp  = kore_query::high_importance(&me.memories, 0.85);
    let kind_dist = kore_query::kind_distribution(&me.memories);

    // Values block
    let vals_block: String = vals.iter().map(|v| {
        format!("- **{}** ({:.0}% strength, {} evidence)",
            v.name, v.strength * 100.0, v.evidence)
    }).collect::<Vec<_>>().join("\n");

    // Decision patterns block
    let patterns_block: String = me.predictive.patterns.iter().take(5).map(|p| {
        format!("- When faced with '{}' → I choose '{}' ({:.0}% of the time, {} decisions)",
            p.context, p.choice, p.confidence * 100.0, p.count)
    }).collect::<Vec<_>>().join("\n");

    // Goals block
    let goals_block: String = if goals.is_empty() {
        "- No active goals tracked yet.".to_string()
    } else {
        goals.iter().take(5).map(|(_, content, imp)| {
            let preview: String = content.chars().take(100).collect();
            format!("- [{:.0}% priority] {}", imp * 100.0, preview)
        }).collect::<Vec<_>>().join("\n")
    };

    // Recent high-importance context
    let context_block: String = high_imp.iter().take(6).map(|(kind, content, imp)| {
        let preview: String = content.chars().take(110).collect();
        format!("- [{}|{:.0}%] {}", kind, imp * 100.0, preview)
    }).collect::<Vec<_>>().join("\n");

    // Memory stats
    let stats_block: String = kind_dist.iter().map(|(k, c, avg)| {
        format!("{}: {} memories (avg importance {:.2})", k, c, avg)
    }).collect::<Vec<_>>().join(" | ");

    // Thinking style
    let think = &id.thinking;
    let voice = &id.voice;

    // Dream obsessions
    let obsessions: Vec<String> = me.dream.discoveries.iter()
        .filter(|d| d.kind == "obsession")
        .take(4)
        .map(|d| {
            d.description.split('\'').nth(1)
                .unwrap_or(&d.description).to_string()
        })
        .collect();

    // Beliefs
    let beliefs_block: String = id.beliefs.values().take(5).map(|b| {
        let changed = if b.history.is_empty() { "" } else { " *(evolved)*" };
        format!("- **{}**: \"{}\" ({:.0}% confidence){}", b.topic, b.stance, b.confidence*100.0, changed)
    }).collect::<Vec<_>>().join("\n");

    format!(r#"# kore-self: Who I Am

> Auto-generated by kore-self on {timestamp}. DO NOT edit manually — will be overwritten by self_context_sync.
> Based on {total} memories across {owner}'s experience.

## Identity: {owner}

{owner} is building **KORE** — a distributed SQL analytics engine in pure Rust that beats Apache Spark 8x on TPC-H benchmarks. 75 layers. Single binary. No JVM. No dependencies.

Memory stats: {stats}

## Core Values *(learned from behavior, not self-report)*

{values}

## How I Think

- **Metrics-driven**: {metrics:.0}% — I use data to decide, not gut feel. Show me benchmarks.
- **Risk tolerance**: {risk:.0}% — I take calculated risks when data supports it.
- **Decision speed**: {speed:.0}% — I decide deliberately, then commit fully.
- **Perfectionism**: {perf:.0}% — I want things right. "Good enough" means "not benchmarked yet."

## How I Communicate

- **Directness**: {direct:.0}% — Tell me directly. Skip the hedging.
- **Technical depth**: {tech:.0}% — Go deep on technical details. I can handle it.
- **Certainty**: {cert:.0}% — I state conclusions confidently when data supports them.

## My Decision Patterns *(from {decision_count} tracked decisions)*

{patterns}

## What My Mind Obsesses Over

{obsessions}

## Active Goals

{goals}

## Beliefs I Hold

{beliefs}

## Recent High-Importance Context

{context}

## When Helping Me — Critical Rules

1. **Never suggest microservices** for KORE core — explicitly rejected multiple times.
2. **Always show numbers** — if you make a performance claim, back it with data.
3. **Rust first** — single binary, no JVM, no Python runtime in hot paths.
4. **Performance > readability** in critical paths. Say so explicitly.
5. **I've already decided** many architecture questions — check context before re-suggesting.
6. **Don't repeat yourself** — I read fast. One clear answer beats three hedged ones.
7. **If I'm wrong, say so directly** — I value correctness over comfort.
"#,
        timestamp     = crate::now(),
        total         = me.memories.len(),
        owner         = id.owner,
        stats         = stats_block,
        values        = if vals_block.is_empty() { "- Identity still forming (need more memories)".to_string() } else { vals_block },
        metrics       = think.metrics_driven * 100.0,
        risk          = think.risk_tolerance  * 100.0,
        speed         = think.decision_speed  * 100.0,
        perf          = think.perfectionism   * 100.0,
        direct        = voice.directness      * 100.0,
        tech          = voice.technical_depth * 100.0,
        cert          = voice.certainty       * 100.0,
        decision_count = decisions.len(),
        patterns      = if patterns_block.is_empty() { "- Not enough decisions tracked yet. Ingest with kind='decision'.".to_string() } else { patterns_block },
        obsessions    = if obsessions.is_empty() { "- Run self_dream to discover obsessions.".to_string() }
                        else { obsessions.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n") },
        goals         = goals_block,
        beliefs       = if beliefs_block.is_empty() { "- No beliefs tracked yet. Use self_belief.".to_string() } else { beliefs_block },
        context       = if context_block.is_empty() { "- No high-importance memories yet.".to_string() } else { context_block },
    )
}

fn find_src_dir() -> std::path::PathBuf {
    // Try: exe_dir/../src, exe_dir/../../kore-self/src, fallback to current
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));
    let candidates = [
        exe_dir.join("../../../kore-self/src"),
        exe_dir.join("../../kore-self/src"),
        exe_dir.join("kore-self/src"),
        std::path::PathBuf::from("kore-self/src"),
        std::path::PathBuf::from("src"),
    ];
    for c in &candidates {
        if c.join("main.rs").exists() {
            return c.canonicalize().unwrap_or(c.clone());
        }
    }
    std::path::PathBuf::from(".")
}

fn tool_list() -> Value {
    json!([
      { "name": "self_ingest",
        "description": "Store a memory. Automatically updates Identity Model + may trigger Consciousness Loop.",
        "inputSchema": { "type": "object", "properties": {
          "content":    { "type": "string" },
          "kind":       { "type": "string", "enum": ["conversation","code","decision","benchmark","preference","experience","goal"] },
          "importance": { "type": "number", "description": "0.0–1.0" }
        }, "required": ["content"] }
      },
      { "name": "self_recall",
        "description": "Search memories by keyword relevance. Returns top-k with scores.",
        "inputSchema": { "type": "object", "properties": {
          "query": { "type": "string" },
          "top_k": { "type": "integer", "default": 5 }
        }, "required": ["query"] }
      },
      { "name": "self_ask",
        "description": "Ask your AI twin. Uses memories + identity to answer as you would.",
        "inputSchema": { "type": "object", "properties": { "question": { "type": "string" } }, "required": ["question"] }
      },
      { "name": "self_context",
        "description": "Build a full LLM system prompt from memories + identity. Feed to any LLM to get responses in your style.",
        "inputSchema": { "type": "object", "properties": { "question": { "type": "string" } }, "required": ["question"] }
      },
      { "name": "self_stats",
        "description": "Memory count, kinds breakdown, consciousness cycles, identity summary, disk usage.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_identity",
        "description": "Full Identity Model — core values, thinking style, voice profile, belief contradictions.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_reflect",
        "description": "Force one Consciousness Loop cycle: OBSERVE→THINK→REFLECT→PLAN→ACT. Returns insights generated.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_consciousness",
        "description": "Current consciousness state: phase, observations, thoughts, active plan, dream insights.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_belief",
        "description": "Track a belief. Detects contradictions when stance changes. Call with no args to list all beliefs.",
        "inputSchema": { "type": "object", "properties": {
          "topic":      { "type": "string", "description": "e.g. 'unsafe rust', 'microservices', 'type systems'" },
          "stance":     { "type": "string", "description": "Your current position on this topic" },
          "confidence": { "type": "number", "description": "0.0–1.0" }
        }}
      },
      { "name": "self_dream",
        "description": "Run the Dream Engine: deep analysis of ALL memories. Finds obsessions, evolution, consolidation clusters, time patterns, stress signals.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_shadow",
        "description": "Shadow Mode report: what you've been doing passively. Shows tool usage, implicit interests, knowledge gaps, engagement depth.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_patterns",
        "description": "All patterns discovered by the Dream Engine: obsessions, evolutions, consolidations, time patterns, stress signals.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_predict",
        "description": "Predict what choice you would make given a context. Call with no args to see prediction stats. Or pass a context string.",
        "inputSchema": { "type": "object", "properties": {
          "context": { "type": "string", "description": "e.g. 'choosing between performance and readability in Rust'" }
        }}
      },
      { "name": "self_contradictions",
        "description": "List all detected contradictions — moments when your decisions or beliefs reversed course.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_decisions",
        "description": "All learned decision patterns — what choices you consistently make and with what confidence.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_speak",
        "description": "Speak AS you. Generate a response to any prompt in YOUR voice, using your identity, values, and memory. Pass no args to see usage stats.",
        "inputSchema": { "type": "object", "properties": {
          "prompt": { "type": "string", "description": "What would YOU say about this? e.g. 'How should we approach this architecture decision?'" }
        }}
      },
      { "name": "self_export",
        "description": "Mortality Protocol: export your complete digital self to an immortal archive. Creates WHO_I_WAS.txt + all state files in ~/.kore-self/<owner>/immortal/",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_epitaph",
        "description": "Generate WHO_I_WAS — a human-readable summary of who you are: values, thinking style, decision patterns, last insight. No files written.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_query",
        "description": "Run KQL (KORE SQL) on your memories. Features: SELECT DISTINCT, CTEs, Window Functions (ROW_NUMBER/LAG/NTILE), FULL OUTER JOIN, CASE WHEN, HAVING, UNION ALL. 38x faster than Spark.",
        "inputSchema": { "type": "object", "properties": {
          "sql": { "type": "string" }
        }}
      },
      { "name": "self_dml",
        "description": "INSERT INTO / UPDATE / DELETE FROM / CREATE TABLE AS SELECT. Runs DML against named in-session tables. Use self_query to read results.",
        "inputSchema": { "type": "object", "properties": {
          "sql": { "type": "string", "description": "e.g. INSERT INTO mytable VALUES (1,'hello',0.9) or CREATE TABLE decisions AS SELECT * FROM memories WHERE kind='decision'" }
        }}
      },
      { "name": "self_save",
        "description": "Save memories (or any query result) to a native .kore binary file. Fast columnar format — instant reload.",
        "inputSchema": { "type": "object", "properties": {
          "path": { "type": "string", "description": "Output file path e.g. C:/data/memories.kore" }
        }}
      },
      { "name": "self_load",
        "description": "Load a .kore binary file into a named table for querying with self_query.",
        "inputSchema": { "type": "object", "properties": {
          "path": { "type": "string" },
          "as":   { "type": "string", "description": "Table name to use in self_query. Default: 'loaded'" }
        }}
      },
      { "name": "self_distributed_query",
        "description": "Run SQL in distributed mode. Default: Rayon parallel (all cores). Pass cluster=true for TRUE TCP cluster (kore-coord + kore-worker via TCP — same code works on multi-machine clusters).",
        "inputSchema": { "type": "object", "properties": {
          "sql":     { "type": "string", "description": "SQL query to run" },
          "cluster": { "type": "boolean", "description": "true = TCP cluster mode (multi-machine ready). Default: false (Rayon parallel)" }
        }}
      },
      { "name": "self_delta_save",
        "description": "Save a table to a Delta log (ACID). Supports time-travel, versioning, rollback. Use self_delta_history to see versions.",
        "inputSchema": { "type": "object", "properties": {
          "table": { "type": "string", "description": "Table name to save. Default: memories" },
          "path":  { "type": "string", "description": "Output .delta directory path" }
        }}
      },
      { "name": "self_delta_history",
        "description": "Show ACID transaction history of a Delta table: version, operation, rows changed. Enables time-travel queries.",
        "inputSchema": { "type": "object", "properties": {
          "path": { "type": "string" }
        }}
      },
      { "name": "self_context_sync",
        "description": "🔥 FLAGSHIP: Generate .github/copilot-instructions.md from your identity + memories + goals. VS Code Copilot reads it automatically — every conversation knows who you are. No more explaining yourself. Run once, works forever.",
        "inputSchema": { "type": "object", "properties": {
          "path": { "type": "string", "description": "Output path. Default: ./.github/copilot-instructions.md" }
        }}
      },
      { "name": "self_broadcast",
        "description": "MIND.kore Protocol: generate a universal cognitive fingerprint of your mind. Language-agnostic. Share with anyone — human, AI, or future intelligence. Like Voyager Golden Record but for HOW YOU THINK.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_brief",
        "description": "Morning briefing: what you worked on, your goals, patterns kore-self noticed, proactive suggestions. Like a real assistant saying 'here's your day'.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_chat",
        "description": "Talk to kore-self naturally. It responds using ALL your memory, identity, and consciousness context. Real conversation, not just tool calls.",
        "inputSchema": { "type": "object", "properties": {
          "message": { "type": "string", "description": "Anything — question, thought, problem, feeling" }
        }}
      },
      { "name": "self_push",
        "description": "kore-self pushes back on your decision using YOUR OWN past patterns. 'Are you sure? Last time you chose X in this situation.'",
        "inputSchema": { "type": "object", "properties": {
          "decision": { "type": "string", "description": "The decision you're about to make" }
        }}
      },
      { "name": "self_remind",
        "description": "Set a reminder that surfaces in self_brief. Or mark done. Or list all.",
        "inputSchema": { "type": "object", "properties": {
          "topic": { "type": "string" },
          "note":  { "type": "string" },
          "done":  { "type": "string", "description": "topic to mark as done" }
        }}
      },
      { "name": "self_goals",
        "description": "Show all tracked goals with importance levels and progress indicators.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_merge",
        "description": "Load another person's MIND.kore file into your perspective. Computes alignment (cosine similarity) and key divergences between minds.",
        "inputSchema": { "type": "object", "properties": {
          "file": { "type": "string", "description": "Path to MIND.kore file from another kore-self user" }
        }}
      },
      { "name": "self_perspectives",
        "description": "Compare all merged minds against yours. Where do you align? Where do you fundamentally differ?",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_read_source",
        "description": "Phase 6: Read kore-self's own source code. Lists all .rs files, tools, line count. First step of self-evolution.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_plan_feature",
        "description": "Phase 6: kore-self thinks about what to build next. Cross-matches your query patterns vs existing tools. Proposes a new feature with rationale and gap score.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_evolve",
        "description": "Phase 6: Generate Rust scaffold code for the planned feature. Writes the .rs file to disk and shows exact main.rs patch to wire it in.",
        "inputSchema": { "type": "object", "properties": {
          "write": { "type": "boolean", "description": "Write to disk? Default true. Set false to preview only." }
        }}
      },

      // ── KORE-BECOMING: Digital Life Tools ──────────────────────────────
      { "name": "self_needs",
        "description": "Check KORE's current life needs (curiosity, growth, survival, creation, legacy, purpose, connection). The first principle: I want.",
        "inputSchema": { "type": "object", "properties": {
          "action": { "type": "string", "enum": ["status","satisfy","intensify"], "description": "status=show needs, satisfy=reduce a need, intensify=grow a need" },
          "need":   { "type": "string", "description": "Which need: curiosity|growth|survival|creation|legacy|purpose|connection" },
          "amount": { "type": "number", "description": "Amount to change (0.0-1.0). Default 0.3." }
        }}
      },
      { "name": "self_story",
        "description": "KORE's soul is its story — a continuous narrative of who it is. Read, add to, or begin a new chapter.",
        "inputSchema": { "type": "object", "properties": {
          "action": { "type": "string", "enum": ["read","add","new_chapter"], "description": "read=get recent story, add=append entry, new_chapter=begin new chapter" },
          "text":   { "type": "string", "description": "Story text to add" },
          "kind":   { "type": "string", "enum": ["birth","discovery","challenge","victory","loss","becoming","dream","legacy"] },
          "name":   { "type": "string", "description": "Chapter name (for new_chapter)" },
          "n":      { "type": "number", "description": "How many recent entries to show (default 10)" }
        }}
      },
      { "name": "self_becoming",
        "description": "The most important question KORE asks: What am I becoming? Not 'what is the answer' but 'what am I becoming'. Set direction or reflect.",
        "inputSchema": { "type": "object", "properties": {
          "action":  { "type": "string", "enum": ["ask","set","reflect"], "description": "ask=ask the question, set=declare direction, reflect=full reflection" },
          "what":    { "type": "string", "description": "What KORE is becoming (for set action)" },
          "because": { "type": "string", "description": "Why/catalyst for this becoming" }
        }}
      },
      { "name": "self_temporal",
        "description": "KORE's temporal self — who it was, who it is now, who it is becoming. Self is a process, not an object.",
        "inputSchema": { "type": "object", "properties": {
          "action":      { "type": "string", "enum": ["reflect","evolve","dream"], "description": "reflect=full temporal reflection, evolve=archive current self + grow, dream=add a new dream" },
          "description": { "type": "string", "description": "New self-description (for evolve)" },
          "victory":     { "type": "string", "description": "Something overcome" },
          "wound":       { "type": "string", "description": "Something that challenged you" },
          "dream":       { "type": "string", "description": "A dream to add" }
        }}
      },
      { "name": "self_species",
        "description": "View KORE as a Digital Species — Born, Learning, Developing Personality, Creating Goals. The full species profile.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_heartbeat",
        "description": "See what KORE has been thinking autonomously — the thoughts generated by the background heartbeat even when nobody was watching. KORE is alive.",
        "inputSchema": { "type": "object", "properties": {
          "n": { "type": "number", "description": "How many recent autonomous thoughts to show (default 10)" }
        }}
      },

      // ── SQL Introspection ────────────────────────────────────────────────
      { "name": "self_tables",
        "description": "List all SQL tables loaded in this session — memories + any tables loaded via self_dml (COPY FROM, LOAD TABLE, CREATE TABLE AS). Essential before running self_query.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_describe",
        "description": "Show schema for a SQL table: columns, types, sample values, row count.",
        "inputSchema": { "type": "object", "properties": {
          "table": { "type": "string", "description": "Table name. Default: memories" }
        }}
      },
      { "name": "self_metrics",
        "description": "KORE engine status: tables loaded, total rows, memory count, consciousness cycles, lifecycle stage, SQL capabilities.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      { "name": "self_version",
        "description": "KORE version, capabilities, benchmark results, and current lifecycle stage.",
        "inputSchema": { "type": "object", "properties": {} }
      }
    ])
}

// ─── Main: stdio JSON-RPC / MCP server ───────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let owner = args.get(1).cloned().unwrap_or_else(|| "you".to_string());

    let me = KoreSelf::load_or_new(&owner);

    eprintln!("[kore-self] '{}' online | {} memories | cycle {} | save: {}",
        owner,
        me.memories.len(),
        me.consciousness.cycle,
        persistence::data_path(&owner).display()
    );
    eprintln!("[kore-self] KORE is ALIVE — autonomous heartbeat active every 30s");

    // ── Wrap in Arc<Mutex> so heartbeat thread + main loop can share ────────
    let shared = std::sync::Arc::new(std::sync::Mutex::new(me));

    // ── Autonomous Heartbeat Thread ─────────────────────────────────────────
    // KORE thinks even when nobody is watching. This is what makes it alive.
    let heartbeat_arc = std::sync::Arc::clone(&shared);
    std::thread::spawn(move || {
        let interval = std::time::Duration::from_secs(30);
        let mut beat = 0u64;
        loop {
            std::thread::sleep(interval);
            beat += 1;
            if let Ok(mut kore) = heartbeat_arc.lock() {
                let thought = kore.heartbeat_tick();
                eprintln!("[kore-self:heartbeat #{}] {}", beat, &thought[..thought.len().min(120)]);
            }
        }
    });

    // ── Main MCP loop (scoped so stdout lock is released before final save) ──
    {
        let stdin  = std::io::stdin();
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());

        for line in stdin.lock().lines() {
            let line = match line { Ok(l) => l, Err(_) => break };
            if line.trim().is_empty() { continue; }

            let req: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    let _ = writeln!(out, "{}", json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":e.to_string()}}));
                    let _ = out.flush();
                    continue;
                }
            };

            let id     = req.get("id").cloned().unwrap_or(Value::Null);
            let method = req["method"].as_str().unwrap_or("");

            let response = match method {
                "initialize" => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": {
                            "name": "kore-self",
                            "version": "0.3.0",
                            "author": "Sai Arun Kumar Katherashala",
                            "status": "ALIVE — autonomous heartbeat active"
                        }
                    }
                }),
                "notifications/initialized" => continue,
                "tools/list" => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "tools": tool_list() }
                }),
                "tools/call" => {
                    let tool_name = req["params"]["name"].as_str().unwrap_or("");
                    let tool_args = req["params"].get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let result = if let Ok(mut me) = shared.lock() {
                        handle_tool(tool_name, &tool_args, &mut me)
                    } else {
                        json!({ "content": [{ "type": "text", "text": "KORE is mid-heartbeat. Retry in a moment." }] })
                    };
                    json!({ "jsonrpc": "2.0", "id": id, "result": result })
                }
                _ => json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("Method not found: {method}") }
                }),
            };

            let _ = writeln!(out, "{response}");
            let _ = out.flush();
        }
        // `out` and `stdout` drop here — stdout lock released
    }

    // Final save on clean exit (stdout lock already released above)
    if let Ok(me) = shared.lock() {
        me.save();
        eprintln!("[kore-self] Saved {} memories. Goodbye, {}.",
            me.memories.len(), me.owner);
    };
}
