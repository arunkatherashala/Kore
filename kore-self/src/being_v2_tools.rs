//! Tools of the World v2 being: being_run_v2, being_status_v2 (opt-in; the original being tools are unchanged).
//!
//! A run goes through the same gates as `being_run`: the owner's hand-written `controller.txt`
//! (missing or unparseable: refused, fail closed), the `KILL` file (refused before the start, and
//! checked before every step), caps on steps, energy, live rules and search candidates, and an audit
//! row (source `authorize`) for every refusal. In addition every run is asked through
//! `authorize()` as the internal tool `world_v2_run` (tier L2: it appends to the being's own log),
//! so the owner has to list it in `autonomy_policy.txt`; default deny.

use crate::authorize::{self, Outcome, BEING_ACTOR};
use crate::being::controller::{self, Origin};
use crate::being::policy::KillCheck;
use crate::being::world2::engine::{run_v2, LearnerCfg, Run2Cfg, State2, DEFAULT_DISCOVER_EVERY};
use crate::being::world2::replay::{rederive_rules2, replay_decisions2};
use crate::being::world2::world::{W2Cfg, DEFAULT_MENU};
use crate::being::{self, Policy};
use crate::being_tools::load_controller;
use crate::log_tools::{err, log_err, open, text};
use crate::versioned::EventRow;
use serde_json::{json, Value};

pub const TOOLS: [&str; 3] = ["being_run_v2", "being_status_v2", "being_observe"];
/// The internal tool name the run is authorized as.
pub const AUTH_TOOL: &str = "world_v2_run";
const MAX_STEPS: u64 = 5000;

pub fn handle(name: &str, args: &Value, owner: &str) -> Result<Value, Value> {
    match name {
        "being_run_v2" => run(args, owner),
        "being_status_v2" => status(owner),
        "being_observe" => observe(args, owner),
        _ => Err(err("unknown tool")),
    }
}

fn opt_u64(args: &Value, k: &str) -> Result<Option<u64>, Value> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| err(format!("'{k}' must be a non-negative integer"))),
    }
}

fn audit(owner: &str, cap: &str, limit: u64, requested: u64, what: &str) {
    let _ = authorize::audit_controller_locked(owner, cap, limit, requested, what);
}

fn run(args: &Value, owner: &str) -> Result<Value, Value> {
    let steps = opt_u64(args, "steps")?.ok_or_else(|| err("'steps' is required (integer 1..5000)"))?;
    if !(1..=MAX_STEPS).contains(&steps) {
        return Err(err(format!("'steps' must be 1..{MAX_STEPS}")));
    }
    let discover = match args.get("discover") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(err("'discover' must be a boolean")),
    };
    let want_exp = match args.get("experiments") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(err("'experiments' must be a boolean")),
    };
    // B10: the all-skills profile (each part needs its own controller key; see world2/all_skills.rs)
    let profile = match args.get("profile") {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) if s == "all_skills" => true,
        Some(Value::String(s)) if s == "default" => false,
        Some(_) => return Err(err("'profile' must be \"default\" or \"all_skills\"")),
    };
    // B13: the context-aware chooser estimate (needs context_chooser = on in controller.txt; fixed when the being is created)
    let want_ctx = match args.get("chooser") {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) if s == "context" => true,
        Some(Value::String(s)) if s == "default" => false,
        Some(_) => return Err(err("'chooser' must be \"default\" or \"context\"")),
    };
    if profile && want_exp {
        return Err(err("'experiments' is part of the all_skills profile; do not pass both"));
    }
    let u32_of = |k: &str| -> Result<Option<u32>, Value> { Ok(opt_u64(args, k)?.map(|x| x.min(u32::MAX as u64) as u32)) };
    let world = W2Cfg {
        seed: opt_u64(args, "world_seed")?.unwrap_or(1),
        families: opt_u64(args, "families")?.unwrap_or(12) as usize,
        budget: opt_u64(args, "budget")?.unwrap_or(3000).min(i64::MAX as u64) as i64,
        noise_pm: u32_of("noise_pm")?.unwrap_or(0),
        drift_per_10k: u32_of("drift_per_10k")?.unwrap_or(0),
        curve_pct: u32_of("curve_pct")?.unwrap_or(0),
        menu: opt_u64(args, "menu")?.unwrap_or(DEFAULT_MENU as u64) as usize,
    };
    // controller first (fail closed), then the kill switch, then authorize()
    let ctl = load_controller(owner);
    if ctl.is_closed() {
        let reason = match &ctl.origin {
            Origin::FailClosed(r) => r.clone(),
            Origin::File => String::new(),
        };
        audit(owner, "controller_closed", 0, steps, &reason);
        return Err(err(format!("controller refused the run (fail closed): {reason}. Create {} in the owner directory by hand; no tool can write it", controller::FILE)));
    }
    if authorize::kill_active(owner) {
        audit(owner, "kill_switch", 0, steps, "the KILL file exists: the World v2 run was refused before it started");
        return Err(err("the KILL file exists in the owner directory: the being does not run. Delete the file by hand to allow runs"));
    }
    let d = authorize::authorize_locked(owner, BEING_ACTOR, AUTH_TOOL, &json!({ "steps": steps, "world_seed": world.seed, "families": world.families }));
    if d.outcome != Outcome::Allow {
        return Err(err(format!("{AUTH_TOOL} was not allowed ({}): {}. To allow World v2 runs, list {AUTH_TOOL} under [L2] in autonomy_policy.txt (edit that file by hand)", d.outcome.name(), d.reason)));
    }
    let (limits, _) = ctl.limits(false, false);
    let mut refused: Vec<(String, u64, u64, String)> = vec![];
    let steps_eff = if steps > ctl.max_steps_per_run {
        refused.push(("max_steps_per_run".into(), ctl.max_steps_per_run, steps, format!("{steps} steps were requested, the controller allows {}; the run is cut to the cap", ctl.max_steps_per_run)));
        ctl.max_steps_per_run
    } else {
        steps
    };
    let mut log = open(owner)?;
    let mut policy = Policy::new(steps_eff);
    policy.max_energy = Some(ctl.max_energy_per_run);
    let owner_k = owner.to_string();
    policy.kill = Some(KillCheck(std::sync::Arc::new(move || authorize::kill_active(&owner_k))));
    let mut cfg = Run2Cfg { world: Some(world), learner: Some(LearnerCfg::default()), discover_every: if discover { DEFAULT_DISCOVER_EVERY } else { 0 }, ctl: Some(limits) };
    let mut ctx_lin: Option<crate::being::linval::LinCfg> = None;
    if want_ctx {
        let rows: Vec<EventRow> = log.open_latest().rows();
        let st0 = State2::from_events(&rows);
        match crate::being::linval::LinCtl::from_text(load_controller_text(owner).as_deref()).grant(true) {
            Ok(_) if st0.cfg.is_none() => ctx_lin = Some(crate::being::linval::LIN_V2),
            Ok(_) if st0.learner.lin.is_some() => {}
            Ok(_) => refused.push(("fixed_at_creation".into(), 0, 1, "the chooser estimate is fixed when a World v2 being is created; this being keeps the B1 estimate".into())),
            Err(r) => refused.push((r.cap.clone(), r.limit, r.requested, r.what.clone())),
        }
        if let (Some(l), Some(lc)) = (cfg.learner.as_mut(), ctx_lin) {
            l.lin = Some(lc);
        }
    }
    if profile {
        let rows: Vec<EventRow> = log.open_latest().rows();
        let st0 = State2::from_events(&rows);
        let existing = st0.cfg.as_ref().map(|_| st0.learner);
        let ctl_text = load_controller_text(owner);
        let g = crate::being::world2::all_skills::grant(ctl_text.as_deref(), crate::being::world2::all_skills::Want::all(), existing);
        let mut learner = g.learner;
        if ctx_lin.is_some() && existing.is_none() {
            learner.lin = ctx_lin;
        }
        cfg.learner = Some(learner);
        let o = crate::being::world2::all_skills::run_v2_all(&mut log, &cfg, &policy, &g.all).map_err(log_err)?;
        drop(log);
        let s = o.summary;
        for r in &s.refusals {
            refused.push((r.cap.clone(), r.limit, r.requested, r.what.clone()));
        }
        if s.stopped == being::StopReason::Killed {
            refused.push(("kill_switch".into(), 0, s.steps_run, format!("the KILL file stopped the World v2 run after {} steps", s.steps_run)));
        }
        for r in &refused {
            audit(owner, &r.0, r.1, r.2, &r.3);
        }
        let mut out = s.to_json();
        out["profile"] = json!({
            "name": "all_skills", "rules_use": s.state.learner.use_.any(), "adaptation": s.state.learner.skills.adapt, "rule_retirement": s.state.learner.skills.retire,
            "experiments": g.all.exp.map(|e| json!({ "kappa": e.kappa, "energy_pm": e.energy_pm, "max_experiments": e.max_experiments })), "experiments_run": o.experiments.len(),
            "lookahead": g.all.look.map(|c| json!({ "mode": "hybrid", "depth": c.depth, "max_nodes": c.max_nodes })), "lookahead_decisions": o.look.decisions,
            "rules_retired_this_run": o.retired, "model_changes_this_run": o.changes,
        });
        out["controller"] = json!({ "refusals_audited": refused.iter().map(|r| json!({ "cap": r.0, "limit": r.1, "requested": r.2, "what": r.3 })).collect::<Vec<_>>() });
        return Ok(text(out));
    }
    // B9: experiments only if asked for AND allowed with a budget by controller.txt (else refused, audited, and the run is passive)
    let exp_budget = if want_exp {
        match ctl.experiment_budget() {
            Ok(b) => Some(b),
            Err(r) => {
                refused.push((r.cap.clone(), r.limit, r.requested, r.what.clone()));
                None
            }
        }
    } else {
        None
    };
    let (s, n_exp) = match exp_budget {
        Some((pm, n)) => {
            let (s, recs) = crate::being::world2::experiment::run_v2_exp(&mut log, &cfg, &policy, crate::being::experiment::ExpCfg::eig(crate::being::experiment::DEFAULT_KAPPA, pm, n)).map_err(log_err)?;
            (s, Some(recs.len()))
        }
        None => (run_v2(&mut log, &cfg, &policy, None).map_err(log_err)?, None),
    };
    drop(log);
    for r in &s.refusals {
        refused.push((r.cap.clone(), r.limit, r.requested, r.what.clone()));
    }
    if s.stopped == being::StopReason::Killed {
        refused.push(("kill_switch".into(), 0, s.steps_run, format!("the KILL file stopped the World v2 run after {} steps", s.steps_run)));
    }
    for r in &refused {
        audit(owner, &r.0, r.1, r.2, &r.3);
    }
    let mut out = s.to_json();
    out["controller"] = json!({
        "experiments_run": n_exp, "experiments_allowed": ctl.experiments, "max_experiment_energy_pm": ctl.max_experiment_energy_pm, "max_experiments_per_run": ctl.max_experiments_per_run,
        "max_live_rules": ctl.max_live_rules, "max_search_candidates": ctl.max_search_candidates, "max_steps_per_run": ctl.max_steps_per_run, "max_energy_per_run": ctl.max_energy_per_run,
        "refusals_audited": refused.iter().map(|r| json!({ "cap": r.0, "limit": r.1, "requested": r.2, "what": r.3 })).collect::<Vec<_>>(),
    });
    Ok(text(out))
}

/// The text of `controller.txt` (None: missing, too large, unsafe permissions or unreadable: the B10 parts then fail closed).
fn load_controller_text(owner: &str) -> Option<String> {
    let p = crate::safety::owner_dir(owner).join(controller::FILE);
    let meta = std::fs::metadata(&p).ok()?;
    if meta.len() > 16 * 1024 || authorize::perm_problem(&p).is_some() {
        return None;
    }
    std::fs::read(&p).ok().and_then(|b| String::from_utf8(b).ok())
}

/// B10: the behaviour report of the World v2 being, built only from the log (read-only).
fn observe(args: &Value, owner: &str) -> Result<Value, Value> {
    let window = opt_u64(args, "window")?.unwrap_or(0);
    let max_items = opt_u64(args, "max_items")?.unwrap_or(20).min(500) as usize;
    let format = match args.get("format") {
        None | Some(Value::Null) => "markdown",
        Some(Value::String(s)) if s == "markdown" || s == "json" => s.as_str(),
        Some(_) => return Err(err("'format' must be \"markdown\" or \"json\"")),
    };
    let log = open(owner)?;
    let snap = log.open_latest();
    let rows: Vec<EventRow> = snap.rows();
    let revs: Vec<crate::versioned::RevisionRow> = snap.rows();
    drop(log);
    let o = crate::being::world2::observe::observe(&rows, &revs, window);
    Ok(match format {
        "json" => text(o.to_json()),
        _ => crate::log_tools::plain(o.to_md(max_items)),
    })
}

fn status(owner: &str) -> Result<Value, Value> {
    let log = open(owner)?;
    let rows: Vec<EventRow> = log.open_latest().rows();
    let st = State2::from_events(&rows);
    let Some(c) = st.cfg.clone() else {
        return Ok(text(json!({ "exists": false, "note": "no World v2 being has run in this owner directory yet; call being_run_v2" })));
    };
    let rep = replay_decisions2(&rows);
    let rr = rederive_rules2(&rows);
    let rx = crate::being::world2::experiment::replay_experiments2(&rows);
    Ok(text(json!({
        "exists": true, "world": { "seed": c.seed, "families": c.families, "budget": c.budget, "noise_pm": c.noise_pm, "drift_per_10k": c.drift_per_10k, "curve_pct": c.curve_pct, "menu": c.menu },
        "steps": st.step, "budget_left": st.budget_left, "reported_reward": st.total_reward, "energy_spent": st.total_cost, "skips": st.skips,
        "model_attempts": st.model.attempts(), "rules": st.rules.len(), "revealed_examples": st.examples.len(),
        "replay": { "decisions": rep.len(), "rederived_identically": rep.iter().filter(|r| r.matches()).count(), "rules": rr.len(), "rules_rederived": rr.iter().filter(|r| r.1).count(),
                    "experimenter_decisions": rx.len(), "experimenter_rederived_identically": rx.iter().filter(|r| r.1 == r.2).count() },
        "note": "World v2 is a deterministic simulated world with author-defined task families; reported reward is what the being was told (feedback noise can make it wrong)",
    })))
}

pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({ "name": "being_run_v2",
          "description": "Opt-in World v2 run (many task families with hidden rules, feedback noise and drift; see docs/world_v2.md). Same gates as being_run: controller.txt caps (missing or unparseable = refused), the KILL file (checked before every step), audited refusals; additionally authorized as the internal tool world_v2_run (list it under [L2] in autonomy_policy.txt). The being picks offers and solver strategies with the existing learned model and planner; every decision, action and reported outcome is appended to its own log. A simulation with author-defined rewards, not a real-world agent",
          "inputSchema": { "type": "object", "properties": {
            "steps": { "type": "integer", "minimum": 1, "maximum": 5000 },
            "world_seed": { "type": "integer", "minimum": 0 }, "families": { "type": "integer", "minimum": 1, "maximum": 48 },
            "budget": { "type": "integer", "minimum": 1, "maximum": 1000000 }, "noise_pm": { "type": "integer", "minimum": 0, "maximum": 400 },
            "drift_per_10k": { "type": "integer", "minimum": 0, "maximum": 200 }, "curve_pct": { "type": "integer", "minimum": 0, "maximum": 100 },
            "menu": { "type": "integer", "minimum": 2, "maximum": 8 }, "discover": { "type": "boolean" },
            "experiments": { "type": "boolean", "description": "B9 (opt-in): choose some tasks on purpose to test the being's own rule hypotheses; needs experiments = on and a budget in controller.txt, otherwise refused and audited" },
            "chooser": { "type": "string", "enum": ["default", "context"], "description": "B13 (opt-in, fixed when the being is created): context uses the context-aware success estimate (each cell shrunk toward a shared linear estimate over family and difficulty, LinUCB-style optimism); needs context_chooser = on in controller.txt, otherwise refused and audited" },
            "profile": { "type": "string", "enum": ["default", "all_skills"], "description": "B10 (opt-in): all_skills asks for every skill (rules use, adaptation, rule retirement, experiments, lookahead); each part runs only if its own controller.txt key allows it (rules_use, adaptation, rule_retirement, experiments with a budget, lookahead), otherwise it stays off and the refusal is logged and audited. Rules use, adaptation and retirement are fixed when the being is created" }
          }, "required": ["steps"] } }),
        json!({ "name": "being_observe",
          "description": "Read-only behaviour report of the World v2 being, built only from its log: choices per task type and strategy over windows of steps, reported rewards, the being's own estimated regret, rules accepted/used/retired with their evidence, experiments and what they decided, lookahead plans, model changes, controller refusals and stops (kill switch). Writes nothing.",
          "inputSchema": { "type": "object", "properties": {
            "window": { "type": "integer", "minimum": 0, "description": "steps per window (0 or absent: about a tenth of the life)" },
            "max_items": { "type": "integer", "minimum": 0, "maximum": 500 },
            "format": { "type": "string", "enum": ["markdown", "json"] }
          } } }),
        json!({ "name": "being_status_v2",
          "description": "State of the World v2 being folded from its log, with a replay check of every decision and rule.",
          "inputSchema": { "type": "object", "properties": {} } }),
    ]
}

#[cfg(test)]
#[path = "being_v2_tools_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "being_v2_exp_tools_tests.rs"]
mod tests_exp;

#[cfg(test)]
#[path = "being_v2_all_tools_tests.rs"]
mod tests_all;
