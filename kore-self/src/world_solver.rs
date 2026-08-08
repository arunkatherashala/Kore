//! World Solver — route any stated problem to the best KORE engine (SQL math,
//! memory analytics, units, percentages, physics, chemistry, space science,
//! languages, and all major subject areas).

use std::collections::HashMap;

use kore_core::DataBlock;
use kore_sql::executor::KqlContext;

use crate::kore_query;
use crate::world_knowledge;
use crate::world_science::{self, ScienceAnswer};
use crate::world_types::WorldAnswer;
use crate::world_languages;
use crate::Memory;

#[derive(Debug, Clone, Default)]
pub struct WorldSolverEngine {
    pub attempts: u64,
    pub successes: u64,
    /// Problems that fell through to decompose (no closed-form answer).
    pub decompose_count: u64,
    pub recent_unsolved: Vec<String>,
}

impl WorldSolverEngine {
    pub fn unsolved_count(&self) -> u64 {
        self.decompose_count
    }

    fn record_unsolved(&mut self, problem: &str) {
        self.decompose_count += 1;
        let q = truncate(problem, 120);
        if self.recent_unsolved.iter().any(|s| s == &q) {
            return;
        }
        self.recent_unsolved.push(q);
        const MAX: usize = 12;
        if self.recent_unsolved.len() > MAX {
            self.recent_unsolved.remove(0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct SolveResult {
    pub method: String,
    pub answer: String,
    pub steps: Vec<String>,
    pub confidence: f64,
    pub sql_used: Option<String>,
}

impl WorldSolverEngine {
    pub fn solve(
        &mut self,
        problem: &str,
        memories: &[Memory],
        dml_tables: &HashMap<String, DataBlock>,
    ) -> SolveResult {
        self.attempts += 1;
        let problem = problem.trim();
        if problem.is_empty() {
            return SolveResult {
                method: "none".into(),
                answer: "Pass a non-empty problem string.".into(),
                steps: vec![],
                confidence: 0.0,
                sql_used: None,
            };
        }

        let mut steps = vec![format!("Problem: {}", problem)];
        let script = world_languages::detect_script(problem);
        if script != "Latin script" && script != "Latin (default) / undetermined" {
            steps.push(format!("Input script: {}", script));
        }

        if let Some(r) = try_sql_expression(problem, memories, dml_tables, &mut steps) {
            self.successes += 1;
            return r;
        }
        if let Some(sci) = world_science::try_science(problem, &mut steps) {
            self.successes += 1;
            return science_to_result(sci, steps);
        }
        if let Some(w) = world_knowledge::try_world(problem, &mut steps) {
            self.successes += 1;
            return world_to_result(w, steps);
        }
        if let Some(r) = try_percent(problem, memories, dml_tables, &mut steps) {
            self.successes += 1;
            return r;
        }
        if let Some(r) = try_unit_conversion(problem, &mut steps) {
            self.successes += 1;
            return r;
        }
        if let Some(r) = try_memory_question(problem, memories, dml_tables, &mut steps) {
            self.successes += 1;
            return r;
        }
        if let Some(r) = try_memory_recall(problem, memories, &mut steps) {
            self.successes += 1;
            return r;
        }

        steps.push("No closed-form solution — decomposed into research steps.".into());
        steps.push("KORE does not know this yet — see self_world_unknown for gaps; use self_fill_gaps or self_fetch.".into());
        self.record_unsolved(problem);
        SolveResult {
            method: "decompose".into(),
            answer: decompose_plan(problem, memories.len()),
            steps,
            confidence: 0.35,
            sql_used: None,
        }
    }
}

fn science_to_result(sci: ScienceAnswer, steps: Vec<String>) -> SolveResult {
    SolveResult {
        method: sci.method,
        answer: sci.answer,
        steps,
        confidence: sci.confidence,
        sql_used: None,
    }
}

fn world_to_result(w: WorldAnswer, steps: Vec<String>) -> SolveResult {
    SolveResult {
        method: w.method,
        answer: w.answer,
        steps,
        confidence: w.confidence,
        sql_used: None,
    }
}

fn run_sql(
    sql: &str,
    memories: &[Memory],
    dml_tables: &HashMap<String, DataBlock>,
) -> Result<String, String> {
    let mut ctx = KqlContext::new();
    ctx.register("memories", kore_query::memories_to_block(memories));
    for (name, block) in dml_tables {
        ctx.register(name, block.clone());
    }
    let block = ctx.query(sql).map_err(|e| e.to_string())?;
    Ok(kore_query::block_to_display(&block))
}

fn try_sql_expression(
    problem: &str,
    memories: &[Memory],
    dml_tables: &HashMap<String, DataBlock>,
    steps: &mut Vec<String>,
) -> Option<SolveResult> {
    let lower = problem.to_lowercase();
    let expr = extract_expression(problem)?;
    if !looks_computable(&expr) {
        return None;
    }
    let sql = format!("SELECT {} AS result", expr);
    steps.push(format!("Model as KORE SQL: {}", sql));
    let display = run_sql(&sql, memories, dml_tables).ok()?;
    steps.push(format!("Compute: {}", display.trim()));
    Some(SolveResult {
        method: "kore_sql_math".into(),
        answer: format!("{}\n\n{}", display.trim(), if lower.contains("prove") {
            "Numeric result computed by KORE SQL engine (same core as analytics)."
        } else {
            "Calculated via KORE SQL."
        }),
        steps: steps.clone(),
        confidence: 0.92,
        sql_used: Some(sql),
    })
}

fn extract_expression(problem: &str) -> Option<String> {
    let lower = problem.to_lowercase();
    for prefix in [
        "calculate ",
        "compute ",
        "what is ",
        "what's ",
        "solve ",
        "evaluate ",
        "find ",
        "how much is ",
    ] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return Some(sanitize_sql_expr(rest.trim()));
        }
        if let Some(pos) = lower.find(prefix) {
            let rest = &problem[pos + prefix.len()..];
            return Some(sanitize_sql_expr(rest.trim().trim_end_matches('?')));
        }
    }
    if looks_computable(problem) {
        return Some(sanitize_sql_expr(problem.trim_end_matches('?')));
    }
    None
}

fn sanitize_sql_expr(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '0'..='9' | '+' | '-' | '*' | '/' | '.' | '(' | ')' | ' ' | ',' => out.push(ch),
            '×' => out.push('*'),
            '÷' => out.push('/'),
            _ if ch.is_ascii_alphabetic() => out.push(ch),
            _ => {}
        }
    }
    out.replace("sqrt", "SQRT")
        .replace("Sqrt", "SQRT")
        .replace("pow", "POWER")
        .replace("Pow", "POWER")
}

fn looks_computable(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
        && s.chars().all(|c| {
            "0123456789+-*/()., abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_"
                .contains(c)
        })
}

fn try_percent(
    problem: &str,
    memories: &[Memory],
    dml_tables: &HashMap<String, DataBlock>,
    steps: &mut Vec<String>,
) -> Option<SolveResult> {
    let lower = problem.to_lowercase();
    if !lower.contains('%') && !lower.contains("percent") {
        return None;
    }
    let nums: Vec<f64> = problem
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() < 2 {
        return None;
    }
    let (pct, base) = (nums[0], nums[1]);
    let sql = format!("SELECT {} * {} / 100.0 AS result", base, pct);
    steps.push(format!("Percent model: {}% of {}", pct, base));
    let display = run_sql(&sql, memories, dml_tables).ok()?;
    Some(SolveResult {
        method: "percent".into(),
        answer: display,
        steps: steps.clone(),
        confidence: 0.9,
        sql_used: Some(sql),
    })
}

fn try_unit_conversion(problem: &str, steps: &mut Vec<String>) -> Option<SolveResult> {
    let lower = problem.to_lowercase();
    let nums: Vec<f64> = problem
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    let n = *nums.first()?;

    let (label, value) = if lower.contains("km") && (lower.contains("mile") || lower.contains("mi")) {
        steps.push("Convert km → miles (× 0.621371)".into());
        ("miles", n * 0.621_371)
    } else if lower.contains("mile") && lower.contains("km") {
        steps.push("Convert miles → km (× 1.60934)".into());
        ("km", n * 1.609_34)
    } else if lower.contains("celsius") || lower.contains("°c") {
        steps.push("Convert °C → °F".into());
        ("°F", n * 9.0 / 5.0 + 32.0)
    } else if lower.contains("fahrenheit") || lower.contains("°f") {
        steps.push("Convert °F → °C".into());
        ("°C", (n - 32.0) * 5.0 / 9.0)
    } else if lower.contains("kg") && lower.contains("lb") {
        steps.push("Convert kg → lb".into());
        ("lb", n * 2.204_62)
    } else {
        return None;
    };

    Some(SolveResult {
        method: "unit_conversion".into(),
        answer: format!("{:.6} {}", value, label),
        steps: steps.clone(),
        confidence: 0.88,
        sql_used: None,
    })
}

fn try_memory_question(
    problem: &str,
    memories: &[Memory],
    dml_tables: &HashMap<String, DataBlock>,
    steps: &mut Vec<String>,
) -> Option<SolveResult> {
    let lower = problem.to_lowercase();
    let about_memories = lower.contains("memory") || lower.contains("memories") || lower.contains("how many");
    if !about_memories {
        return None;
    }
    let sql = if lower.contains("kind") || lower.contains("type") {
        "SELECT kind, COUNT(*) AS cnt FROM memories GROUP BY kind ORDER BY cnt DESC".to_string()
    } else if lower.contains("important") || lower.contains("importance") {
        "SELECT COUNT(*) AS high_importance FROM memories WHERE importance >= 0.9".to_string()
    } else {
        "SELECT COUNT(*) AS total_memories FROM memories".to_string()
    };
    steps.push(format!("Analytics SQL on your life data: {}", sql));
    let display = run_sql(&sql, memories, dml_tables).ok()?;
    Some(SolveResult {
        method: "memory_analytics".into(),
        answer: display,
        steps: steps.clone(),
        confidence: 0.85,
        sql_used: Some(sql),
    })
}

fn try_memory_recall(problem: &str, memories: &[Memory], steps: &mut Vec<String>) -> Option<SolveResult> {
    let tokens: Vec<String> = problem
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3)
        .map(|s| s.to_string())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut scored: Vec<(f64, &Memory)> = memories
        .iter()
        .map(|m| {
            let c = m.content.to_lowercase();
            let hit = tokens.iter().filter(|t| c.contains(t.as_str())).count() as f64;
            (hit * m.importance, m)
        })
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<_> = scored.into_iter().take(3).collect();
    if top.is_empty() {
        return None;
    }
    steps.push("Recalled evidence from KORE memories matching problem terms.".into());
    let mut answer = String::from("From KORE memory (evidence-based):\n");
    for (score, m) in top {
        answer.push_str(&format!(
            "\n• [{} | {:.2}] {}\n",
            m.kind,
            score,
            truncate(&m.content, 400)
        ));
    }
    Some(SolveResult {
        method: "memory_recall".into(),
        answer,
        steps: steps.clone(),
        confidence: 0.7,
        sql_used: None,
    })
}

fn decompose_plan(problem: &str, memory_count: usize) -> String {
    format!(
        "KORE World Solver — open problem plan\n\
         ═══════════════════════════════════\n\
         1. MODEL — Restate unknowns and knowns for: \"{}\"\n\
         2. COMPUTE — Use self_solve with a numeric sub-question, or self_query for data you loaded\n\
         3. EXTERNAL — Use heartbeat knowledge burst / federation if peers are online\n\
         4. VERIFY — Cross-check with self_predict or a second calculation\n\
         5. BECOME — Ingest result with self_ingest so KORE remembers the solution\n\n\
         Session context: {} memories in graph. \
         Tip: math \"calculate …\", science \"molar mass of water\", geography \"capital of France\", \
         languages \"how many languages in the world\", subjects via self_world_catalog.",

        truncate(problem, 200),
        memory_count
    )
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}…", &s[..max])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn solves_simple_arithmetic() {
        let mut engine = WorldSolverEngine::default();
        let memories: Vec<Memory> = vec![];
        let tables = HashMap::new();
        let r = engine.solve("calculate 2 + 2", &memories, &tables);
        assert!(r.confidence >= 0.7);
        assert!(r.answer.contains('4') || r.steps.iter().any(|s| s.contains('4')));
    }

    #[test]
    fn empty_problem_low_confidence() {
        let mut engine = WorldSolverEngine::default();
        let r = engine.solve("", &[], &HashMap::new());
        assert_eq!(r.confidence, 0.0);
    }
}
