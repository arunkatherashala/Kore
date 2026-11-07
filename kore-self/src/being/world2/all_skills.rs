//! Track B10 (opt-in): the "all skills" profile of the World v2 being. One run with every skill the owner's
//! controller allows: the B7 rule-use switches (A, A2, B, C, G), B3b adaptation and rule retirement (both fixed
//! at creation, `skills.rs`), B9 experiments (per run, within an energy budget) and B8 lookahead (per run,
//! hybrid mode, within node and depth caps). Each part is asked for separately and granted only by its own
//! `controller.txt` key; a part that is not allowed stays off and the refusal is logged (fail closed). The
//! kill switch and the energy cap are checked before every step (`Policy::gate_cost`), as in `run_v2`.
//!
//! The run writes, beyond what `run_v2` writes: a `skills_profile` note at the start (what is on, what was
//! refused), the B9 `experiment_cfg` note and per-experiment records, B3b `w2_change` notes, `rule_retired`
//! notes, and the refusals of the experiment budget and the lookahead node cap. With every part off it is
//! `run_v2` (a test compares the logs).

use super::engine::{genesis_event2, refusal_event2, stop_event2, Engine2, LearnerCfg, Run2Cfg, Run2Summary, State2, FLUSH_EVERY};
use super::experiment::{credit_revision, exp_cfg_event, experiment_energy_from_log, experiment_records, Exp2, ExpRec2};
use super::learner::{Chooser2, Learner2};
use super::rules_use::UseCfg;
use super::skills::{SkillsCfg, SkillsCtl};
use super::world::{World2, SOURCE2};
use crate::being::controller::{Controller, Refusal};
use crate::being::experiment::ExpCfg;
use crate::being::lookahead::adapt::{Look2, LookStats};
use crate::being::lookahead::ctl::LookCtl;
use crate::being::lookahead::plan::{LookCfg, Mode};
use crate::being::policy::{Policy, StopReason};
use crate::being::world::iso_at;
use crate::versioned::{event_id, Batch, EventRow, LogError, NewEvent, VersionedLog};

/// B7 switches of the profile: A, A2, B (robust acceptance at the B7 threshold 0.4), C, G; rule prior weight cap 8.
pub fn profile_use() -> UseCfg {
    UseCfg { rule_est: true, rule_only: true, robust_milli: 400, fit_const: true, fit_grid: true, rule_k: super::rules_use::DEFAULT_RULE_K }
}

/// Experiment setting of the profile (chosen on tuning seeds 101..120; docs/measured.md, Track B10).
pub const PROFILE_KAPPA: f64 = 0.1;
pub const PROFILE_EXP_PM: u32 = 50;
/// Lookahead setting of the profile: hybrid mode, depth 2 (the B8 hybrid that stayed closest to B1 in World v2).
pub const PROFILE_LOOK_DEPTH: u32 = 2;

/// What one all-skills run uses.
#[derive(Debug, Clone, Default)]
pub struct AllCfg {
    pub exp: Option<ExpCfg>,
    pub look: Option<LookCfg>,
    /// Refusals of the profile request (logged at the start of the run).
    pub refusals: Vec<Refusal>,
    /// The `skills_profile` note text (None: no note; a plain run).
    pub profile: Option<String>,
}

/// What the profile asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Want {
    pub rules_use: bool,
    pub adaptation: bool,
    pub retirement: bool,
    pub experiments: bool,
    pub lookahead: bool,
}

impl Want {
    pub fn all() -> Want {
        Want { rules_use: true, adaptation: true, retirement: true, experiments: true, lookahead: true }
    }
}

/// The grant of a profile request: learner constants for a new being, the per-run parts, the refusals.
#[derive(Debug, Clone)]
pub struct Grant {
    pub learner: LearnerCfg,
    pub all: AllCfg,
}

/// Grant `want` under the owner's controller text (None: file missing). `existing`: the learner of a being that
/// already exists (its creation-time switches cannot change; asking for a different one is refused).
pub fn grant(text: Option<&str>, want: Want, existing: Option<LearnerCfg>) -> Grant {
    let ctl = match text {
        Some(t) => Controller::parse_or_closed(t),
        None => Controller::fail_closed("controller.txt is missing"),
    };
    let sk = SkillsCtl::from_text(text);
    let lk = LookCtl::from_text(text);
    let ((u, a, r), mut refusals) = sk.grant(want.rules_use, want.adaptation, want.retirement);
    let mut learner = LearnerCfg::default();
    if u {
        learner.use_ = profile_use();
    }
    learner.skills = SkillsCfg { adapt: a, retire: r, ..SkillsCfg::default() };
    if let Some(ex) = existing {
        if (u && !ex.use_.any()) || (a && !ex.skills.adapt) || (r && !ex.skills.retire) {
            refusals.push(Refusal::new("fixed_at_creation", 0, 1, "rules use, adaptation and rule retirement are fixed when a World v2 being is created; this being keeps the switches it was created with"));
        }
        learner = ex;
    }
    let exp = if want.experiments {
        match ctl.experiment_budget() {
            Ok((pm, n)) => Some(ExpCfg::eig(PROFILE_KAPPA, pm.min(PROFILE_EXP_PM), n)),
            Err(rf) => {
                refusals.push(rf);
                None
            }
        }
    } else {
        None
    };
    let (g, lrefs) = lk.grant(want.lookahead);
    refusals.extend(lrefs);
    let look = g.map(|(nodes, depth)| {
        let mut c = LookCfg::new(PROFILE_LOOK_DEPTH.min(depth), nodes);
        c.mode = Mode::Hybrid;
        c
    });
    let on = |b: bool| if b { "on" } else { "off" };
    let profile = format!(
        "all-skills profile for this run: rules use (B7 A, A2, B, C, G) {}, adaptation (B3b) {}, rule retirement {}, experiments (B9) {}, lookahead (B8) {}; {} refused by the controller{}",
        on(learner.use_.any()),
        on(learner.skills.adapt),
        on(learner.skills.retire),
        exp.map_or("off".to_string(), |e| format!("on (kappa {:.2}, at most {} per mille of the life budget, {} per run)", e.kappa, e.energy_pm, e.max_experiments)),
        look.map_or("off".to_string(), |c| format!("on (hybrid, depth {}, at most {} nodes per step)", c.depth, c.max_nodes)),
        refusals.len(),
        if refusals.is_empty() { String::new() } else { format!(": {}", refusals.iter().map(|r| r.cap.as_str()).collect::<Vec<_>>().join(", ")) }
    );
    Grant { learner, all: AllCfg { exp, look, refusals, profile: Some(profile) } }
}

enum Ch {
    Exp(Box<Exp2>),
    Look(Look2),
    Plain(Learner2),
}

impl Ch {
    fn chooser(&mut self) -> &mut dyn Chooser2 {
        match self {
            Ch::Exp(x) => x.as_mut(),
            Ch::Look(l) => l,
            Ch::Plain(p) => p,
        }
    }
    fn refusals(&mut self) -> Vec<Refusal> {
        match self {
            Ch::Exp(x) => {
                let mut v = std::mem::take(&mut x.refusals);
                if let Some(l) = x.look.as_mut() {
                    v.extend(l.take_refusals());
                }
                v
            }
            Ch::Look(l) => l.take_refusals(),
            Ch::Plain(_) => vec![],
        }
    }
    fn stats(&self) -> LookStats {
        match self {
            Ch::Exp(x) => x.look.as_ref().map(|l| l.stats).unwrap_or_default(),
            Ch::Look(l) => l.stats,
            Ch::Plain(_) => LookStats::default(),
        }
    }
}

/// The chooser of an all-skills life (used by the logged run and by the measurement).
pub fn all_chooser(st: &State2, exp: Option<ExpCfg>, look: Option<LookCfg>, compose: bool) -> Box<dyn Chooser2> {
    let w = st.cfg.clone().expect("a started state");
    match exp {
        Some(e) => {
            let mut x = Exp2::new(st.learner.learner(), e, World2::new(w.clone()), w.budget);
            x.compose_blocks = compose;
            x.look = look.map(|c| Look2::with_learner(Some(c), st.learner.learner()));
            Box::new(x)
        }
        None => match look {
            Some(c) => Box::new(Look2::with_learner(Some(c), st.learner.learner())),
            None => Box::new(st.learner.learner()),
        },
    }
}

pub fn profile_event(text: &str, st: &State2, all: &AllCfg) -> NewEvent {
    let mut tags = vec!["skills_profile".to_string(), format!("step:{}", st.step)];
    tags.push(format!("rules_use:{}", st.learner.use_.any() as u8));
    tags.push(format!("adaptation:{}", st.learner.skills.adapt as u8));
    tags.push(format!("retirement:{}", st.learner.skills.retire as u8));
    tags.push(format!("experiments:{}", all.exp.is_some() as u8));
    tags.push(format!("lookahead:{}", all.look.map_or(0, |c| c.depth)));
    tags.push(format!("refused:{}", all.refusals.len()));
    NewEvent { source: SOURCE2.into(), occurred_at: iso_at(st.clock), kind: "note".into(), text: text.to_string(), tags, redacted: false }
}

/// Output of an all-skills run.
pub struct AllOut {
    pub summary: Run2Summary,
    pub experiments: Vec<ExpRec2>,
    pub look: LookStats,
    pub retired: usize,
    pub changes: usize,
}

/// `run_v2` with the all-skills parts of `all` on the being's own log.
pub fn run_v2_all(log: &mut VersionedLog, cfg: &Run2Cfg, policy: &Policy, all: &AllCfg) -> Result<AllOut, LogError> {
    let version_before = log.latest_version();
    let rows: Vec<EventRow> = log.open_latest().rows();
    let mut state = State2::from_events(&rows);
    let mut created = false;
    match &state.cfg {
        Some(c) => {
            if let Some(w) = &cfg.world {
                if w != c {
                    return Err(LogError::Invalid(format!("this World v2 being already lives in world {:?}; the request says {:?}. A being keeps one world", c, w)));
                }
            }
        }
        None => {
            let w = cfg.world.clone().ok_or_else(|| LogError::Invalid("no World v2 being exists yet; a world config is needed to create one".into()))?;
            w.validate().map_err(LogError::Invalid)?;
            let l = cfg.learner.unwrap_or_default();
            if l.beta_milli > 10_000 || l.skip_milli > 10_000 {
                return Err(LogError::Invalid("learner constants must be within 0..=10".into()));
            }
            let (ev, id) = genesis_event2(&w, &l);
            log.append_events(&[ev])?;
            state.start(w, l, id);
            created = true;
        }
    }
    let compose = cfg.ctl.as_ref().map_or(false, |l| l.compose);
    let wcfg = state.cfg.clone().expect("started");
    let mut ch = match all.exp {
        Some(e) => {
            let mut x = Exp2::new(state.learner.learner(), e, World2::new(wcfg.clone()), wcfg.budget);
            x.compose_blocks = compose;
            x.book.energy = experiment_energy_from_log(&rows);
            x.look = all.look.map(|c| Look2::with_learner(Some(c), state.learner.learner()));
            Ch::Exp(Box::new(x))
        }
        None => match all.look {
            Some(c) => Ch::Look(Look2::with_learner(Some(c), state.learner.learner())),
            None => Ch::Plain(state.learner.learner()),
        },
    };
    let mut engine = Engine2::new(state, cfg.ctl.clone());
    let mut pending = Batch::default();
    if let Some(p) = &all.profile {
        pending.events.push(profile_event(p, &engine.state, all));
    }
    for r in &all.refusals {
        pending.events.push(refusal_event2(r, engine.state.step, engine.state.clock));
        engine.refusals_total.push(r.clone());
    }
    if let Some(e) = &all.exp {
        pending.events.push(exp_cfg_event(e, engine.state.step, engine.state.clock));
    }
    let (mut new_rules, mut truths) = (vec![], vec![]);
    let mut pending_steps = 0usize;
    let (mut done, mut rep, mut tru, mut cost) = (0u64, 0i64, 0i64, 0i64);
    let mut exp_outcomes: std::collections::HashMap<String, (String, u64)> = std::collections::HashMap::new();
    let (mut retired, changes_before) = (0usize, engine.state.changes.len());
    let stopped;
    loop {
        if let Some(r) = policy.gate_cost(done, cost) {
            stopped = r;
            if matches!(r, StopReason::Killed | StopReason::EnergyCap) {
                pending.events.push(stop_event2(r.as_str(), engine.state.step, engine.state.clock));
                if r == StopReason::EnergyCap {
                    engine.refuse(Refusal::new("max_energy_per_run", policy.max_energy.unwrap_or(0).max(0) as u64, cost.max(0) as u64, format!("the run spent {cost} energy units and may not start another step")));
                }
            }
            break;
        }
        let n_before = match &ch {
            Ch::Exp(x) => x.recs.len(),
            _ => 0,
        };
        match engine.step(ch.chooser()) {
            Ok(out) => {
                done += 1;
                rep += out.outcome.reward;
                tru += out.truth.true_reward;
                cost += out.outcome.cost;
                truths.push(out.truth.clone());
                let dev_id = event_id(&out.events[0].source, &out.events[0].occurred_at, &out.events[0].text);
                pending.events.extend(out.events);
                pending.revisions.push(out.revision);
                pending.predictions.extend(out.prediction);
                pending.outcomes.extend(out.resolved);
                if let Ch::Exp(x) = &ch {
                    if x.recs.len() > n_before {
                        let rec = x.recs.last().expect("experiment").clone();
                        let (ev, rv) = experiment_records(&rec, &out.outcome, &x.cache, &dev_id);
                        let nid = event_id(&ev.source, &ev.occurred_at, &ev.text);
                        exp_outcomes.insert(out.outcome.event_id.clone(), (nid, rec.step));
                        pending.events.push(ev);
                        pending.revisions.push(rv);
                    }
                }
                for (ev, rv) in engine.take_change_records() {
                    pending.events.push(ev);
                    pending.revisions.push(rv);
                }
                pending_steps += 1;
                if cfg.discover_every > 0 && engine.state.step % cfg.discover_every == 0 {
                    for nr in engine.retire_pass() {
                        retired += 1;
                        pending.events.push(nr.event);
                        pending.revisions.push(nr.revision);
                    }
                    for nr in engine.discover_pass() {
                        let credit: Vec<(String, u64)> = nr.revision.caused_by.iter().filter_map(|c| exp_outcomes.get(c).cloned()).collect();
                        let rule_ev = nr.rule.event_id.clone();
                        pending.events.push(nr.event);
                        pending.revisions.push(nr.revision);
                        if !credit.is_empty() {
                            pending.revisions.push(credit_revision(&nr.rule, &rule_ev, &credit, engine.state.clock));
                        }
                        new_rules.push(nr.rule);
                    }
                    for nc in engine.const_pass() {
                        pending.events.push(nc.event);
                        pending.revisions.push(nc.revision);
                    }
                }
                for r in engine.take_refusals().into_iter().chain(ch.refusals()) {
                    pending.events.push(refusal_event2(&r, engine.state.step, engine.state.clock));
                    engine.refusals_total.push(r);
                }
                if pending_steps >= FLUSH_EVERY {
                    log.commit_batch(std::mem::take(&mut pending))?;
                    pending_steps = 0;
                }
            }
            Err(r) => {
                stopped = r;
                break;
            }
        }
    }
    for r in engine.take_refusals().into_iter().chain(ch.refusals()) {
        pending.events.push(refusal_event2(&r, engine.state.step, engine.state.clock));
        engine.refusals_total.push(r);
    }
    if !pending.events.is_empty() {
        log.commit_batch(pending)?;
    }
    let experiments = match &ch {
        Ch::Exp(x) => x.recs.clone(),
        _ => vec![],
    };
    let look = ch.stats();
    let changes = engine.state.changes.len() - changes_before;
    let summary = Run2Summary { steps_run: done, stopped, reported_reward: rep, true_reward: tru, cost, state: engine.state, created, new_rules, refusals: engine.refusals_total, truths, version_before, version_after: log.latest_version() };
    Ok(AllOut { summary, experiments, look, retired, changes })
}

