//! Tests of Track B10 (all skills together, `all_skills.rs`, `skills.rs`, `observe.rs`): with every part off the
//! all-skills run is `run_v2` byte for byte; each part needs its own controller key (fail closed); with every
//! part on, runs are deterministic, the state folds back from the log, decisions and rules replay, every
//! retirement and model change is logged with its evidence, the caps and the kill switch hold; retirement
//! retires stale rules and none in a clean world; the B3b model in World v2 is the B1 model in mode Switch; the
//! observer reports a logged run from the log alone.

use super::all_skills::{grant, profile_use, run_v2_all, AllCfg, Want};
use super::engine::{run_v2, LearnerCfg, Run2Cfg, State2};
use super::experiment::{replay_experiments2, EXP_POLICY};
use super::measure::run_w2_with;
use super::model::FamModel;
use super::observe::observe;
use super::replay::{rederive_rules2, replay_decisions2};
use super::skills::SkillsCfg;
use super::tests_use::log_digest;
use super::world::*;
use crate::being::adapt_b3b::DEFAULT_SWITCH;
use crate::being::controller::Controller;
use crate::being::model::{AdaptMode, Model, Obs};
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::solver::{Strategy, STRATEGIES_EXT};
use crate::being::state::tag;
use crate::being::world::{Rng, ALL_DOMAINS_X};
use crate::versioned::{CauseRow, EventRow, RevisionRow, VersionedLog};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

const ALL_ON: &str = "max_live_rules = 16\nmax_steps_per_run = 2000\nrules_use = on\nadaptation = on\nrule_retirement = on\nexperiments = on\nmax_experiment_energy_pm = 50\nmax_experiments_per_run = 200\nlookahead = on\nmax_lookahead_nodes = 20000\nmax_lookahead_depth = 3\n";
const ALL_BUT_LOOK: &str = "max_live_rules = 16\nmax_steps_per_run = 2000\nrules_use = on\nadaptation = on\nrule_retirement = on\nexperiments = on\nmax_experiment_energy_pm = 50\nmax_experiments_per_run = 200\n";

fn drift_world(seed: u64) -> W2Cfg {
    W2Cfg { noise_pm: 100, drift_per_10k: 60, ..W2Cfg::new(seed, 12, 3000) }
}

fn run_all(dir: &std::path::Path, ctl_text: &str, w: W2Cfg, pol: &Policy) -> (VersionedLog, super::all_skills::AllOut) {
    let mut log = open(dir);
    let g = grant(Some(ctl_text), Want::all(), None);
    let limits = Controller::parse(ctl_text).unwrap().limits(false, false).0;
    let cfg = Run2Cfg { world: Some(w), learner: Some(g.learner), discover_every: 10, ctl: Some(limits) };
    let o = run_v2_all(&mut log, &cfg, pol, &g.all).unwrap();
    (log, o)
}

fn tagged(log: &VersionedLog, t: &str) -> Vec<EventRow> {
    log.open_latest().rows::<EventRow>().into_iter().filter(|e| e.tags.split(',').any(|x| x == t)).collect()
}

#[test]
fn b3b_model_in_world_v2_equals_the_b1_model_in_switch_mode_bit_for_bit() {
    let mut m = Model::new(AdaptMode::Switch, 1e300);
    let mut f = FamModel::with_switch(7, DEFAULT_SWITCH);
    let mut r = Rng::new(11);
    for i in 0..4000 {
        let d = ALL_DOMAINS_X[r.below(7) as usize];
        let k = 1 + r.below(5) as u8;
        let s = STRATEGIES_EXT[r.below(4) as usize];
        // a rate that changes half way, so the switch and the prior check both fire
        let ok = if i < 2000 { r.below(5) > 0 } else { r.below(5) == 0 };
        let listed = r.range(3, 11);
        let spent = s.energy(listed);
        m.observe(&Obs { domain: d, difficulty: k, strategy: s, correct: ok, listed_cost: listed, spent });
        f.observe(d.idx(), k, s, ok, listed, spent);
        if i % 13 == 0 {
            for dd in ALL_DOMAINS_X {
                for kk in 1..=5u8 {
                    for ss in STRATEGIES_EXT {
                        let (a, b) = (m.estimate(dd, kk, ss), f.estimate(dd.idx(), kk, ss));
                        assert_eq!((a.n, a.p.to_bits(), a.sd.to_bits(), a.cost_ratio.to_bits(), a.group_n.to_bits()), (b.n, b.p.to_bits(), b.sd.to_bits(), b.cost_ratio.to_bits(), b.group_n.to_bits()), "step {i}");
                    }
                }
            }
        }
    }
    assert!(f.changes.iter().any(|c| c.kind == "cell-switch") && f.changes.iter().any(|c| c.kind == "prior-conflict"), "{:?}", f.changes.len());
    // off: the plain model
    let plain = FamModel::new(7);
    assert!(plain.switch_cfg().is_none() && plain.changes.is_empty());
}

#[test]
fn with_every_part_off_the_all_skills_run_is_run_v2_byte_for_byte() {
    for (seed, fams, budget) in [(1u64, 6usize, 1500i64), (2, 12, 3000)] {
        let w = W2Cfg { noise_pm: 80, drift_per_10k: 25, curve_pct: 50, ..W2Cfg::new(seed, fams, budget) };
        let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (mut l1, mut l2) = (open(d1.path()), open(d2.path()));
        let cfg = Run2Cfg { world: Some(w), learner: None, discover_every: 10, ctl: Some(Controller::defaults().limits(false, false).0) };
        run_v2(&mut l1, &cfg, &Policy::new(400), None).unwrap();
        run_v2_all(&mut l2, &cfg, &Policy::new(400), &AllCfg::default()).unwrap();
        assert_eq!(log_digest(&l1), log_digest(&l2));
        // and the learner constants of a default being carry no B10 tag
        let g = l1.open_latest().rows::<EventRow>().into_iter().find(|e| e.tags.contains("w2_genesis")).unwrap();
        assert!(!g.tags.contains("w2_skills") && !g.tags.contains("w2_use"));
    }
}

#[test]
fn each_part_needs_its_own_controller_key_and_the_grant_fails_closed() {
    // no file: nothing is on, every part is refused
    let g = grant(None, Want::all(), None);
    assert!(!g.learner.use_.any() && !g.learner.skills.any() && g.all.exp.is_none() && g.all.look.is_none());
    assert_eq!(g.all.refusals.len(), 5, "{:?}", g.all.refusals);
    // an invalid file: same
    let g = grant(Some("rules_use = sometimes\n"), Want::all(), None);
    assert!(!g.learner.use_.any() && g.all.exp.is_none() && g.all.look.is_none() && g.all.refusals.len() == 5);
    // a valid file with defaults: all off (fail-closed defaults stay)
    let g = grant(Some("max_steps_per_run = 100\n"), Want::all(), None);
    assert!(!g.learner.use_.any() && !g.learner.skills.any() && g.all.exp.is_none() && g.all.look.is_none() && g.all.refusals.len() == 5);
    // experiments on without a budget: refused
    let g = grant(Some("experiments = on\n"), Want::all(), None);
    assert!(g.all.exp.is_none() && g.all.refusals.iter().any(|r| r.cap == "max_experiment_energy_pm"));
    // only some parts allowed: exactly those are on
    let g = grant(Some("rules_use = on\nrule_retirement = on\n"), Want::all(), None);
    assert_eq!(g.learner.use_, profile_use());
    assert!(!g.learner.skills.adapt && g.learner.skills.retire && g.all.exp.is_none() && g.all.look.is_none());
    assert_eq!(g.all.refusals.iter().map(|r| r.cap.as_str()).collect::<Vec<_>>(), vec!["adaptation", "experiments", "lookahead"]);
    // everything allowed
    let g = grant(Some(ALL_ON), Want::all(), None);
    assert!(g.learner.use_.any() && g.learner.skills.adapt && g.learner.skills.retire && g.all.refusals.is_empty());
    let e = g.all.exp.unwrap();
    assert_eq!((e.energy_pm, e.max_experiments), (50, 200));
    let l = g.all.look.unwrap();
    assert_eq!((l.depth, l.max_nodes), (2, 20000));
    assert!(g.all.profile.as_ref().unwrap().contains("0 refused"));
    // an existing being keeps its creation-time switches; asking for more is refused
    let g = grant(Some(ALL_ON), Want::all(), Some(LearnerCfg::default()));
    assert!(!g.learner.use_.any() && !g.learner.skills.any() && g.all.refusals.iter().any(|r| r.cap == "fixed_at_creation"));
    assert!(g.all.exp.is_some() && g.all.look.is_some());
}

#[test]
fn all_skills_run_is_deterministic_folds_back_and_replays_with_evidence() {
    // find a seed whose life retires a rule within 700 steps (strong drift)
    let mut found = false;
    for seed in 1..=8u64 {
        let pol = Policy::new(700);
        let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (l1, o1) = run_all(d1.path(), ALL_BUT_LOOK, drift_world(seed), &pol);
        let (l2, _) = run_all(d2.path(), ALL_BUT_LOOK, drift_world(seed), &pol);
        assert_eq!(log_digest(&l1), log_digest(&l2), "seed {seed}: two runs differ");
        let snap = l1.open_latest();
        let rows: Vec<EventRow> = snap.rows();
        // the state folded from the log equals the live state (model with the B3b mechanisms, rules, retirements, changes)
        let st = State2::from_events(&rows);
        assert_eq!(st, o1.summary.state, "seed {seed}: folded state differs");
        assert!(st.learner.skills.adapt && st.learner.skills.retire && st.learner.use_.any());
        // replay: every learner and experimenter decision, every rule
        let rd = replay_decisions2(&rows);
        assert!(!rd.is_empty() && rd.iter().all(|r| r.matches()), "seed {seed}");
        let rx = replay_experiments2(&rows);
        assert_eq!(rx.len() as u64, o1.summary.steps_run);
        assert!(rx.iter().all(|r| r.1 == r.2), "seed {seed}: {:?}", rx.iter().find(|r| r.1 != r.2));
        assert!(rx.iter().any(|r| r.1.starts_with(EXP_POLICY)), "seed {seed}: no experiment");
        let rr = rederive_rules2(&rows);
        assert!(rr.iter().all(|r| r.1), "seed {seed}: {:?}", rr.iter().filter(|r| !r.1).collect::<Vec<_>>());
        // evidence: every revision's causes exist; retirements cite the rule and failing outcomes; changes cite outcomes
        let ids: HashSet<String> = rows.iter().map(|e| e.event_id.clone()).collect();
        let causes: Vec<CauseRow> = snap.rows();
        assert!(causes.iter().all(|c| ids.contains(&c.event_id)));
        let revs: Vec<RevisionRow> = snap.rows();
        let ret = tagged(&l1, "rule_retired");
        assert_eq!(ret.len(), o1.retired);
        assert_eq!(st.retirements.len(), o1.retired);
        for e in &ret {
            let id = tag(&e.tags, "rule_id").unwrap();
            let rv = revs.iter().find(|r| r.author == "rule:being-retire" && r.key.ends_with(id)).expect("retirement revision");
            let cited: Vec<&CauseRow> = causes.iter().filter(|c| c.rev_id == rv.rev_id).collect();
            let outcomes = cited.iter().filter(|c| rows.iter().any(|x| x.event_id == c.event_id && x.kind == "outcome")).count();
            assert!(outcomes >= 1 && cited.iter().any(|c| rows.iter().any(|x| x.event_id == c.event_id && x.kind == "note" && (x.tags.contains(",rule,") || x.tags.starts_with("rule,") || x.tags.contains("const_rule")))), "seed {seed}: retirement of {id} lacks evidence");
            assert!(st.rules.iter().any(|r| r.id == id && r.retired) || st.retired_consts.iter().any(|c| c == id));
        }
        let ch = tagged(&l1, "w2_change");
        assert_eq!(ch.len(), st.changes.len());
        assert!(revs.iter().filter(|r| r.key.starts_with("being/world2/change/")).count() == ch.len());
        // the profile note says what was on
        let p = tagged(&l1, "skills_profile");
        assert_eq!(p.len(), 1);
        assert!(p[0].text.contains("lookahead (B8) off") && p[0].text.contains("1 refused"), "{}", p[0].text);
        if o1.retired > 0 && !ch.is_empty() {
            found = true;
            break;
        }
    }
    assert!(found, "no seed retired a rule and changed the model");
}

#[test]
fn caps_and_the_kill_switch_hold_with_all_skills_on() {
    // kill switch: checked before every step; turns on at the 31st check, so exactly 30 steps run
    let d = tempfile::tempdir().unwrap();
    let mut pol = Policy::new(500);
    let c = Arc::new(AtomicU64::new(0));
    let c2 = c.clone();
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 31)));
    let (log, o) = run_all(d.path(), ALL_ON, drift_world(2), &pol);
    assert_eq!(o.summary.stopped, StopReason::Killed);
    assert_eq!(o.summary.steps_run, 30);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert_eq!(rows.iter().filter(|e| e.source == SOURCE2 && e.kind == "outcome").count(), 30);
    assert!(rows.iter().any(|e| e.tags.contains("controller_stop") && e.tags.contains("stop:kill_switch")));
    // a lookahead decision was made (the profile's planner is on) and every one stays within the node cap
    assert!(o.look.decisions > 0 && o.look.max_nodes <= 20000);
    // caps: live rules, energy per run, experiment budget, lookahead nodes
    let ctl = "max_live_rules = 3\nmax_steps_per_run = 2000\nrules_use = on\nadaptation = on\nrule_retirement = on\nexperiments = on\nmax_experiment_energy_pm = 20\nmax_experiments_per_run = 5\nlookahead = on\nmax_lookahead_nodes = 40\nmax_lookahead_depth = 2\n";
    let d = tempfile::tempdir().unwrap();
    let mut pol = Policy::new(2000);
    pol.max_energy = Some(1200);
    let (log, o) = run_all(d.path(), ctl, W2Cfg { noise_pm: 50, ..W2Cfg::new(4, 12, 3000) }, &pol);
    let s = &o.summary;
    assert_eq!(s.stopped, StopReason::EnergyCap);
    assert!(s.cost <= 1200 + 2 * 12, "{}", s.cost);
    assert!(s.state.live_rules() <= 3);
    assert!(o.experiments.len() <= 5);
    assert!(o.look.max_nodes <= 40 && o.look.truncations > 0);
    let caps: Vec<String> = log.open_latest().rows::<EventRow>().into_iter().filter(|e| e.tags.contains("controller_refusal")).filter_map(|e| tag(&e.tags, "cap").map(|x| x.to_string())).collect();
    for want in ["max_live_rules", "max_energy_per_run", "max_lookahead_nodes"] {
        assert!(caps.iter().any(|c| c == want), "no {want} refusal in {caps:?}");
    }
    assert!(caps.iter().any(|c| c == "max_experiments_per_run" || c == "max_experiment_energy_pm"), "no experiment budget refusal in {caps:?}");
}

#[test]
fn retirement_retires_stale_rules_under_drift_and_none_in_a_clean_world() {
    let lc = LearnerCfg { use_: profile_use(), skills: SkillsCfg { retire: true, ..SkillsCfg::default() }, ..LearnerCfg::default() };
    let (mut retired, mut stale) = (0, 0);
    for seed in 1..=6u64 {
        let w = W2Cfg { drift_per_10k: 60, ..W2Cfg::new(seed, 12, 3000) };
        let r = run_w2_with(&w, &mut lc.learner(), 10, None, lc);
        retired += r.retired.len();
        stale += r.retired.iter().filter(|x| x.right_share_then.map_or(true, |s| s < 0.9)).count();
        for x in &r.retired {
            assert!(r.state.rules.iter().any(|q| q.id == x.rule_id && q.retired) || r.state.retired_consts.contains(&x.rule_id));
        }
        // clean world: answers never contradict a rule the guard accepted, so nothing is retired
        let c = W2Cfg::new(seed, 12, 3000);
        let r = run_w2_with(&c, &mut lc.learner(), 10, None, lc);
        assert!(r.retired.is_empty(), "seed {seed}: retired in a clean world");
    }
    assert!(retired > 0 && stale * 10 >= retired * 9, "retired {retired}, stale {stale}");
    // off: never retires
    let off = LearnerCfg { use_: profile_use(), ..LearnerCfg::default() };
    let r = run_w2_with(&W2Cfg { drift_per_10k: 60, ..W2Cfg::new(1, 12, 3000) }, &mut off.learner(), 10, None, off);
    assert!(r.retired.is_empty() && r.state.retirements.is_empty());
}

#[test]
fn the_observer_reports_a_logged_run_from_the_log_alone() {
    let d = tempfile::tempdir().unwrap();
    let mut pol = Policy::new(400);
    pol.max_energy = Some(1500);
    let (log, o) = run_all(d.path(), ALL_BUT_LOOK, drift_world(3), &pol);
    let v0 = log.latest_version();
    let snap = log.open_latest();
    let (rows, revs): (Vec<EventRow>, Vec<RevisionRow>) = (snap.rows(), snap.rows());
    let ob = observe(&rows, &revs, 50);
    let j = ob.to_json();
    let st = &o.summary.state;
    assert_eq!(j["steps"], st.step);
    assert_eq!(j["experiments"].as_u64().unwrap() as usize, o.experiments.len());
    let rules = j["rules"].as_array().unwrap();
    assert_eq!(rules.iter().filter(|r| r["accepted_at_step"].is_u64()).count(), st.rules.len() + st.const_rules.len() + st.retired_consts.len());
    assert_eq!(rules.iter().filter(|r| !r["retired"].is_null()).count(), st.retirements.len());
    let answers: u64 = rules.iter().map(|r| r["answers"].as_u64().unwrap()).sum();
    assert_eq!(answers as usize, o.summary.truths.iter().filter(|t| t.attempted && t.rule_used).count());
    assert_eq!(j["model_changes"].as_u64().unwrap() as usize, st.changes.len());
    let wins = j["windows"].as_array().unwrap();
    assert_eq!(wins.iter().map(|w| w["steps"].as_u64().unwrap()).sum::<u64>(), st.step);
    assert_eq!(wins.iter().map(|w| w["energy"].as_i64().unwrap()).sum::<i64>(), st.total_cost);
    assert!(j["stops"].as_array().unwrap().iter().any(|s| s.as_str().unwrap().contains("energy")));
    assert!(j["controller_refusals"].as_array().unwrap().iter().any(|s| s.as_str().unwrap().contains("[lookahead]")));
    let md = ob.to_md(5);
    for h in ["# World v2 being: behaviour report", "## Over time", "## Choices per task type and strategy", "## Rules", "## Experiments", "## Controller refusals", "## Stops"] {
        assert!(md.contains(h), "missing {h}");
    }
    // read-only: nothing was written
    assert_eq!(log.latest_version(), v0);
    // an empty log
    assert!(observe(&[], &[], 0).to_md(5).contains("No World v2 being"));
}
