//! The observer (Track B10): a readable report of what the World v2 being did, built ONLY from its log.
//!
//! Input: the events and revisions of the owner's log (nothing else: no world, no hidden parameter, no
//! measurement record). Output: a markdown report over windows of steps with the choices per task type and
//! strategy, the reported rewards, the being's OWN estimated regret (the best mean reward per energy among
//! the alternatives its model listed minus the chosen one; the true regret needs the hidden world and is not
//! in the log), the rules accepted, used and retired with their evidence, the experiments and what they
//! decided, the plans of the lookahead planner, B3b changes, controller refusals and stops (kill switch,
//! energy cap). Pure: it reads rows and returns text (`observe_md`, `observe_html`, `observe_json`).

use super::world::SOURCE2;
use crate::being::state::tag;
use crate::versioned::{EventRow, RevisionRow};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn has(tags: &str, t: &str) -> bool {
    tags.split(',').any(|x| x == t)
}

fn num<T: std::str::FromStr>(tags: &str, k: &str) -> Option<T> {
    tag(tags, k).and_then(|v| v.parse().ok())
}

#[derive(Debug, Clone, Default)]
struct Win {
    steps: u64,
    attempts: u64,
    reported_right: u64,
    reward: i64,
    cost: i64,
    skips: u64,
    by_type: BTreeMap<String, u64>,
    by_strategy: BTreeMap<String, u64>,
    experiments: u64,
    lookahead: u64,
    rules_accepted: u64,
    rules_retired: u64,
    rule_answers: u64,
    rule_answers_right: u64,
    changes: u64,
    refusals: u64,
    stops: u64,
    regret_sum: f64,
    regret_n: u64,
}

#[derive(Debug, Clone, Default)]
struct RuleInfo {
    id: String,
    kind: String,
    what: String,
    accepted_step: Option<u64>,
    evidence: String,
    cited: usize,
    used: u64,
    used_right: u64,
    retired: Option<(u64, String, usize)>,
    experiment_credit: Vec<u64>,
}

#[derive(Debug, Clone)]
struct Item {
    step: u64,
    text: String,
}

/// Everything the observer extracted from the log.
#[derive(Debug, Clone, Default)]
pub struct Observed {
    pub genesis: Option<String>,
    profiles: Vec<Item>,
    exp_cfgs: Vec<Item>,
    windows: BTreeMap<u64, Win>,
    pub window: u64,
    pub steps: u64,
    cells: BTreeMap<(String, String), (u64, u64, i64, i64)>,
    rules: Vec<RuleInfo>,
    experiments: Vec<Item>,
    plans: Vec<Item>,
    changes: Vec<Item>,
    refusals: Vec<Item>,
    stops: Vec<Item>,
    audits: Vec<Item>,
    policies: BTreeMap<String, u64>,
    exp_kept: u64,
    exp_rejected: u64,
}

fn rule_mut<'a>(rules: &'a mut Vec<RuleInfo>, id: &str) -> &'a mut RuleInfo {
    if let Some(i) = rules.iter().position(|r| r.id == id) {
        return &mut rules[i];
    }
    rules.push(RuleInfo { id: id.to_string(), ..Default::default() });
    rules.last_mut().expect("pushed")
}

/// The being's own estimated regret of one decision (per energy): best mean reward per energy among the affordable
/// alternatives of its rationale minus the chosen one. None when the rationale does not list them (e.g. lookahead).
fn own_regret(rationale: &Value, action: &str) -> Option<f64> {
    let alts = rationale.get("alternatives")?.as_array()?;
    let mut best = f64::NEG_INFINITY;
    let mut chosen = None;
    for a in alts {
        let m = a.get("mean_reward_per_energy")?.as_f64()?;
        if a.get("affordable").and_then(|x| x.as_bool()) == Some(true) {
            best = best.max(m);
        }
        let label = format!("{}:{}", a.get("slot")?.as_u64()?, a.get("strategy")?.as_str()?);
        if label == action {
            chosen = Some(m);
        }
    }
    if action == "skip" || !best.is_finite() {
        return None;
    }
    Some((best - chosen?).max(0.0))
}

/// Read the log rows. `window`: steps per window (0: about a tenth of the life, at least 25).
pub fn observe(events: &[EventRow], revisions: &[RevisionRow], window: u64) -> Observed {
    let ev: Vec<&EventRow> = events.iter().filter(|e| e.source == SOURCE2).collect();
    let steps = ev.iter().filter(|e| e.kind == "outcome").filter_map(|e| num::<u64>(&e.tags, "step")).max().map_or(0, |s| s + 1);
    let w = if window > 0 { window } else { ((steps + 9) / 10).max(25) };
    let mut o = Observed { window: w, steps, ..Default::default() };
    let wi = |s: u64| s / w;
    // decisions: policy and action per step
    let mut action_of: BTreeMap<u64, String> = BTreeMap::new();
    for e in &ev {
        let t = &e.tags;
        match e.kind.as_str() {
            "note" if has(t, "w2_genesis") => o.genesis = Some(e.text.clone()),
            "note" if has(t, "skills_profile") => o.profiles.push(Item { step: num(t, "step").unwrap_or(0), text: e.text.clone() }),
            "note" if has(t, "experiment_cfg") => o.exp_cfgs.push(Item { step: num(t, "step").unwrap_or(0), text: e.text.clone() }),
            "decision" => {
                let step: u64 = num(t, "step").unwrap_or(0);
                let pol = tag(t, "policy").unwrap_or("?").to_string();
                *o.policies.entry(pol.clone()).or_default() += 1;
                let wd = o.windows.entry(wi(step)).or_default();
                if pol == "w2-experiment" {
                    wd.experiments += 1;
                } else if pol.starts_with("w2-lookahead") {
                    wd.lookahead += 1;
                    o.plans.push(Item { step, text: e.text.clone() });
                }
                if let Some(a) = tag(t, "action") {
                    action_of.insert(step, a.to_string());
                }
            }
            "outcome" => {
                let step: u64 = num(t, "step").unwrap_or(0);
                let res = tag(t, "result").unwrap_or("");
                let (reward, cost): (i64, i64) = (num(t, "reward").unwrap_or(0), num(t, "cost").unwrap_or(0));
                let wd = o.windows.entry(wi(step)).or_default();
                wd.steps += 1;
                wd.reward += reward;
                wd.cost += cost;
                if res == "skip" {
                    wd.skips += 1;
                }
                if res == "correct" || res == "wrong" {
                    let ty = tag(t, "type").unwrap_or("?").to_string();
                    let st = tag(t, "strategy").unwrap_or("?").to_string();
                    wd.attempts += 1;
                    wd.reported_right += (res == "correct") as u64;
                    *wd.by_type.entry(ty.clone()).or_default() += 1;
                    *wd.by_strategy.entry(st.clone()).or_default() += 1;
                    let c = o.cells.entry((ty, st)).or_default();
                    c.0 += 1;
                    c.1 += (res == "correct") as u64;
                    c.2 += reward;
                    c.3 += cost;
                    if let Some(r) = tag(t, "rule_used") {
                        wd.rule_answers += 1;
                        wd.rule_answers_right += (res == "correct") as u64;
                        let ri = rule_mut(&mut o.rules, r);
                        ri.used += 1;
                        ri.used_right += (res == "correct") as u64;
                    }
                }
            }
            "note" if has(t, "rule") && has(t, "world2") => {
                let id = tag(t, "rule_id").unwrap_or("?").to_string();
                let step = num(t, "at_step");
                let ri = rule_mut(&mut o.rules, &id);
                ri.kind = "sequence rule".into();
                ri.what = format!("f{:02} d{} form {}{}", num::<usize>(t, "fam").unwrap_or(0), tag(t, "difficulty").unwrap_or("?"), tag(t, "form").unwrap_or("?"), tag(t, "xf").map_or(String::new(), |x| format!(" after {x}")));
                ri.accepted_step = step;
                ri.evidence = format!("{} training + {} held-out examples reproduced{}", tag(t, "train").unwrap_or("?"), tag(t, "val").unwrap_or("?"), tag(t, "missed").map_or(String::new(), |m| format!(", {m} missed (robust acceptance)")));
                if let Some(s) = step {
                    o.windows.entry(wi(s)).or_default().rules_accepted += 1;
                }
            }
            "note" if has(t, "const_rule") && has(t, "world2") => {
                let id = tag(t, "rule_id").unwrap_or("?").to_string();
                let step = num(t, "at_step");
                let ri = rule_mut(&mut o.rules, &id);
                ri.kind = "fitted constant".into();
                ri.what = format!("f{:02} g/move rule = {}", num::<usize>(t, "fam").unwrap_or(0), tag(t, "g").unwrap_or("?"));
                ri.accepted_step = step;
                ri.evidence = format!("{} training + {} held-out examples reproduced, {} missed", tag(t, "train").unwrap_or("?"), tag(t, "val").unwrap_or("?"), tag(t, "missed").unwrap_or("0"));
                if let Some(s) = step {
                    o.windows.entry(wi(s)).or_default().rules_accepted += 1;
                }
            }
            "note" if has(t, "rule_retired") && has(t, "world2") => {
                let id = tag(t, "rule_id").unwrap_or("?").to_string();
                let step: u64 = num(t, "at_step").unwrap_or(0);
                let why = format!("right on {} of the last {} revealed answers where it applies (95% upper bound {:.3})", tag(t, "window_right").unwrap_or("?"), tag(t, "window_n").unwrap_or("?"), num::<f64>(t, "ub_milli").unwrap_or(0.0) / 1000.0);
                o.rule_retire(&id, step, why);
                o.windows.entry(wi(step)).or_default().rules_retired += 1;
            }
            "note" if has(t, "experiment") => {
                let step: u64 = num(t, "step").unwrap_or(0);
                o.exp_kept += num::<u64>(t, "kept").unwrap_or(0);
                o.exp_rejected += num::<u64>(t, "rejected").unwrap_or(0);
                o.experiments.push(Item { step, text: e.text.clone() });
            }
            "note" if has(t, "w2_change") => {
                let step: u64 = num(t, "step").unwrap_or(0);
                o.windows.entry(wi(step)).or_default().changes += 1;
                o.changes.push(Item { step, text: e.text.clone() });
            }
            "note" if has(t, "controller_refusal") => {
                let step: u64 = num(t, "step").unwrap_or(0);
                o.windows.entry(wi(step)).or_default().refusals += 1;
                o.refusals.push(Item { step, text: format!("[{}] {}", tag(t, "cap").unwrap_or("?"), e.text) });
            }
            "note" if has(t, "controller_stop") => {
                let step: u64 = num(t, "step").unwrap_or(0);
                o.windows.entry(wi(step)).or_default().stops += 1;
                o.stops.push(Item { step, text: format!("[{}] {}", tag(t, "stop").unwrap_or("?"), e.text) });
            }
            _ => {}
        }
    }
    // audit rows of the tool layer (refused runs, the kill switch before a run)
    for e in events.iter().filter(|e| e.source == "authorize") {
        if let Some(tool) = tag(&e.tags, "tool") {
            if tool.starts_with("controller_") || tool.contains("world_v2_run") {
                let reason = serde_json::from_str::<Value>(&e.text).ok().and_then(|v| v.get("reason").and_then(|r| r.as_str()).map(|s| s.to_string())).unwrap_or_else(|| e.text.clone());
                o.audits.push(Item { step: 0, text: format!("{} {}: {}", e.occurred_at, tool, reason) });
            }
        }
    }
    // decision revisions: own estimated regret; experiment credit
    for r in revisions {
        if let Some(s) = r.key.strip_prefix("being/world2/choice/") {
            let Ok(step) = s.parse::<u64>() else { continue };
            let Some(a) = action_of.get(&step) else { continue };
            if let Ok(v) = serde_json::from_str::<Value>(&r.rationale) {
                if let Some(g) = own_regret(&v, a) {
                    let wd = o.windows.entry(wi(step)).or_default();
                    wd.regret_sum += g;
                    wd.regret_n += 1;
                }
            }
        } else if let Some(id) = r.key.strip_prefix("being/world2/experiment_credit/") {
            if let Ok(v) = serde_json::from_str::<Value>(&r.rationale) {
                let steps: Vec<u64> = v.get("experiment_steps").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|y| y.as_u64()).collect()).unwrap_or_default();
                rule_mut(&mut o.rules, id).experiment_credit = steps;
            }
        } else if let Some(id) = r.key.strip_prefix("being/world2/rule/") {
            if r.author == "rule:being-retire" {
                if let Ok(v) = serde_json::from_str::<Value>(&r.rationale) {
                    let n = v.get("failing_outcomes").and_then(|x| x.as_array()).map_or(0, |a| a.len());
                    if let Some(ri) = o.rules.iter_mut().find(|x| x.id == id) {
                        if let Some(rt) = ri.retired.as_mut() {
                            rt.2 = n;
                        }
                    }
                }
            } else if let Some(ri) = o.rules.iter_mut().find(|x| x.id == id) {
                if let Ok(v) = serde_json::from_str::<Value>(&r.rationale) {
                    ri.cited = v.get("cited_examples").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                }
            }
        }
    }
    o
}

impl Observed {
    fn rule_retire(&mut self, id: &str, step: u64, why: String) {
        rule_mut(&mut self.rules, id).retired = Some((step, why, 0));
    }

    /// Machine-readable summary (for the tool).
    pub fn to_json(&self) -> Value {
        let wins: Vec<Value> = self
            .windows
            .iter()
            .map(|(k, w)| {
                json!({
                    "steps_from": k * self.window, "steps_to": (k + 1) * self.window - 1, "steps": w.steps, "attempts": w.attempts, "reported_right": w.reported_right, "skips": w.skips,
                    "reported_reward_per_energy": if w.cost > 0 { w.reward as f64 / w.cost as f64 } else { 0.0 }, "energy": w.cost,
                    "own_estimated_regret_per_energy": if w.regret_n > 0 { Some(w.regret_sum / w.regret_n as f64) } else { None },
                    "by_type": w.by_type, "by_strategy": w.by_strategy, "experiments": w.experiments, "lookahead_decisions": w.lookahead,
                    "rules_accepted": w.rules_accepted, "rules_retired": w.rules_retired, "rule_answers": w.rule_answers, "rule_answers_reported_right": w.rule_answers_right,
                    "model_changes": w.changes, "controller_refusals": w.refusals, "stops": w.stops,
                })
            })
            .collect();
        json!({
            "steps": self.steps, "window": self.window, "genesis": self.genesis, "profiles": self.profiles.iter().map(|p| &p.text).collect::<Vec<_>>(),
            "windows": wins, "policies": self.policies,
            "rules": self.rules.iter().map(|r| json!({ "rule_id": r.id, "kind": r.kind, "what": r.what, "accepted_at_step": r.accepted_step, "evidence": r.evidence, "cited_examples": r.cited, "answers": r.used, "reported_right": r.used_right, "retired": r.retired.as_ref().map(|x| json!({ "at_step": x.0, "why": x.1, "failing_outcomes_cited": x.2 })), "experiment_credit_steps": r.experiment_credit })).collect::<Vec<_>>(),
            "experiments": self.experiments.len(), "experiment_hypotheses_kept": self.exp_kept, "experiment_hypotheses_rejected": self.exp_rejected,
            "lookahead_decisions": self.plans.len(), "model_changes": self.changes.len(), "controller_refusals": self.refusals.iter().map(|x| &x.text).collect::<Vec<_>>(),
            "stops": self.stops.iter().map(|x| &x.text).collect::<Vec<_>>(), "audit_rows": self.audits.iter().map(|x| &x.text).collect::<Vec<_>>(),
            "note": "built only from the log; reward is what the being was told; regret is the being's own estimate from its listed alternatives (the true regret needs the hidden world)",
        })
    }

    /// The markdown report. `max_items`: how many experiments, plans, changes and refusals are listed in full.
    pub fn to_md(&self, max_items: usize) -> String {
        let mut s = String::new();
        let p = |s: &mut String, x: String| {
            s.push_str(&x);
            s.push('\n');
        };
        p(&mut s, "# World v2 being: behaviour report (from the log only)\n".into());
        match &self.genesis {
            Some(g) => p(&mut s, format!("Created: {g}\n")),
            None => {
                p(&mut s, "No World v2 being in this log.".into());
                return s;
            }
        }
        for x in &self.profiles {
            p(&mut s, format!("Step {}: {}\n", x.step, x.text));
        }
        for x in &self.exp_cfgs {
            p(&mut s, format!("Step {}: {}\n", x.step, x.text));
        }
        let (tr, tc): (i64, i64) = self.windows.values().fold((0, 0), |a, w| (a.0 + w.reward, a.1 + w.cost));
        p(&mut s, format!("Steps: {}. Reported reward {} for {} energy ({:.3} per energy; reported = what the being was told, feedback noise included). Decisions by policy: {}.\n", self.steps, tr, tc, if tc > 0 { tr as f64 / tc as f64 } else { 0.0 }, self.policies.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")));
        p(&mut s, format!("## Over time (windows of {} steps)\n", self.window));
        p(&mut s, "Own regret = mean over decisions of (best mean reward per energy the model listed - the chosen one), the being's own estimate; the true regret is not in the log.\n".into());
        p(&mut s, "| steps | attempts (reported right) | skips | reward / energy | own regret | types | strategies | experiments | lookahead | rules +/- | rule answers (right) | model changes | refusals | stops |".into());
        p(&mut s, "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|".into());
        for (k, w) in &self.windows {
            let kv = |m: &BTreeMap<String, u64>| m.iter().map(|(a, b)| format!("{a} {b}")).collect::<Vec<_>>().join(", ");
            p(
                &mut s,
                format!(
                    "| {}-{} | {} ({}) | {} | {:.3} | {} | {} | {} | {} | {} | +{} / -{} | {} ({}) | {} | {} | {} |",
                    k * self.window, (k + 1) * self.window - 1, w.attempts, w.reported_right, w.skips, if w.cost > 0 { w.reward as f64 / w.cost as f64 } else { 0.0 },
                    if w.regret_n > 0 { format!("{:.3}", w.regret_sum / w.regret_n as f64) } else { "-".into() }, kv(&w.by_type), kv(&w.by_strategy), w.experiments, w.lookahead, w.rules_accepted, w.rules_retired, w.rule_answers, w.rule_answers_right, w.changes, w.refusals, w.stops
                ),
            );
        }
        p(&mut s, "\n## Choices per task type and strategy (whole log)\n".into());
        p(&mut s, "| type | strategy | attempts | reported right | reported reward / energy |".into());
        p(&mut s, "|---|---|---|---|---|".into());
        for ((ty, st), (n, k, r, c)) in &self.cells {
            p(&mut s, format!("| {ty} | {st} | {n} | {k} ({:.0}%) | {:.3} |", 100.0 * *k as f64 / (*n).max(1) as f64, if *c > 0 { *r as f64 / *c as f64 } else { 0.0 }));
        }
        p(&mut s, format!("\n## Rules ({} accepted, {} retired)\n", self.rules.iter().filter(|r| r.accepted_step.is_some()).count(), self.rules.iter().filter(|r| r.retired.is_some()).count()));
        p(&mut s, "| rule | kind | what | accepted at step | evidence | answers (reported right) | experiment credit | retired |".into());
        p(&mut s, "|---|---|---|---|---|---|---|---|".into());
        for r in self.rules.iter().filter(|r| r.accepted_step.is_some()) {
            p(
                &mut s,
                format!(
                    "| {} | {} | {} | {} | {}{} | {} ({}) | {} | {} |",
                    r.id, r.kind, r.what, r.accepted_step.unwrap_or(0), r.evidence, if r.cited > 0 { format!(", {} examples cited", r.cited) } else { String::new() }, r.used, r.used_right,
                    if r.experiment_credit.is_empty() { "-".to_string() } else { format!("examples from experiments at steps {:?}", r.experiment_credit) },
                    r.retired.as_ref().map_or("-".to_string(), |x| format!("at step {}: {}; cites {} failing outcomes", x.0, x.1, x.2))
                ),
            );
        }
        let list = |s: &mut String, title: &str, items: &[Item]| {
            s.push_str(&format!("\n## {title} ({})\n\n", items.len()));
            for x in items.iter().take(max_items) {
                s.push_str(&format!("- step {}: {}\n", x.step, x.text));
            }
            if items.len() > max_items {
                s.push_str(&format!("- ... {} more\n", items.len() - max_items));
            }
        };
        list(&mut s, &format!("Experiments (hypotheses kept {}, rejected {})", self.exp_kept, self.exp_rejected), &self.experiments);
        list(&mut s, "Plans of the lookahead planner", &self.plans);
        list(&mut s, "Model changes (B3b adaptation)", &self.changes);
        list(&mut s, "Controller refusals", &self.refusals);
        list(&mut s, "Stops (kill switch, energy cap)", &self.stops);
        list(&mut s, "Audit rows of the tool layer (refused runs, kill switch)", &self.audits);
        s
    }
}

/// The markdown report of a log.
pub fn observe_md(events: &[EventRow], revisions: &[RevisionRow], window: u64, max_items: usize) -> String {
    observe(events, revisions, window).to_md(max_items)
}

/// The same report as a self-contained HTML page (the markdown, escaped, in a `pre` block).
pub fn observe_html(events: &[EventRow], revisions: &[RevisionRow], window: u64, max_items: usize) -> String {
    let md = observe_md(events, revisions, window, max_items);
    let esc = md.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!("<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>World v2 being: behaviour report</title></head>\n<body><pre style=\"font-family: monospace; white-space: pre-wrap\">\n{esc}</pre></body></html>\n")
}
