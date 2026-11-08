// Golden digests of the being's choosers with every switch off (Track B13). This part uses only what
// existed at e9ed7d1d: the same functions were run there and the digests pinned below.

use crate::being::lookahead::beingl::{run_l, run_wl, GreedyL, RunLCfg};
use crate::being::lookahead::worldl::{WLCfg, SETTINGS};
use crate::being::measure::{run_menu_ext, DISCOVER_EVERY};
use crate::being::planner::{ModelChooser, DEFAULT_BETA};
use crate::being::policy::Policy;
use crate::being::world::World;
use crate::being::world2::engine::{run_v2, Run2Cfg};
use crate::being::world2::learner::Learner2;
use crate::being::world2::measure::run_w2;
use crate::being::world2::world::W2Cfg;
use crate::being::world_o::engine::{run_o, RunOCfg};
use crate::being::world_o::world::WOCfg;
use crate::being::{run, RunCfg, ScriptedThinker};
use crate::versioned::{sha256_hex, CauseRow, EventRow, RevisionRow, VersionedLog};

fn gopen(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

/// Everything a logged run wrote: events, revisions (with rationale) and their causes.
fn glog_digest(log: &VersionedLog) -> String {
    let snap = log.open_latest();
    let mut s = String::new();
    for e in snap.rows::<EventRow>() {
        s.push_str(&format!("E|{}|{}|{}|{}|{}|{}\n", e.event_id, e.source, e.occurred_at, e.kind, e.text, e.tags));
    }
    for r in snap.rows::<RevisionRow>() {
        s.push_str(&format!("R|{}|{}|{}|{}|{}|{}|{}|{}\n", r.rev_id, r.entity_type, r.key, r.stance, r.confidence, r.rationale, r.valid_from, r.author));
    }
    for c in snap.rows::<CauseRow>() {
        s.push_str(&format!("C|{}|{}\n", c.rev_id, c.event_id));
    }
    sha256_hex(&[s.as_bytes()])
}

/// [World v2 log, World O log, first world log, World L log, measured lives of all four choosers].
pub(super) fn golden_digests() -> Vec<String> {
    let mut out = vec![];
    // World v2: the logged loop, with noise, drift and a difficulty curve
    {
        let d = tempfile::tempdir().unwrap();
        let mut log = gopen(d.path());
        let c = W2Cfg { noise_pm: 80, drift_per_10k: 25, curve_pct: 50, ..W2Cfg::new(7, 12, 3000) };
        run_v2(&mut log, &Run2Cfg { world: Some(c), learner: None, discover_every: 10, ctl: None }, &Policy::new(400), None).unwrap();
        out.push(glog_digest(&log));
    }
    // World O: the logged loop
    {
        let d = tempfile::tempdir().unwrap();
        let mut log = gopen(d.path());
        let c = WOCfg { intro_every: 400, life_intros: 6, tier_every: 2, drift_pm: 500, ..WOCfg::new(2, 40_000) };
        run_o(&mut log, &RunOCfg { world: Some(c), learner: None, discover_every: 10, ctl: None, max_explore_pm: None }, &Policy::new(1200), None).unwrap();
        out.push(glog_digest(&log));
    }
    // the first world: the logged loop with the model planner in the extended pattern world, discovery every 10 steps
    {
        let d = tempfile::tempdir().unwrap();
        let mut log = gopen(d.path());
        let cfg = RunCfg { world_seed: Some(3), loop_seed: Some(1), budget: Some(3000), chooser: Some(crate::being::state::Chooser::Model), steps: 400, extended: Some(true), discover_every: Some(10), ..Default::default() };
        run(&mut log, &cfg, &ScriptedThinker, &Policy::new(400)).unwrap();
        out.push(glog_digest(&log));
    }
    // World L: the logged loop with the one-step planner (lookahead not asked for)
    {
        let d = tempfile::tempdir().unwrap();
        let mut log = gopen(d.path());
        for (i, st) in SETTINGS.iter().enumerate() {
            let _ = i;
            let d2 = tempfile::tempdir().unwrap();
            let mut l2 = gopen(d2.path());
            run_l(&mut l2, &RunLCfg { world: Some(WLCfg { seed: 5, setting: *st, budget: 400 }), lookahead: false, depth: 1, controller_text: Some("max_steps_per_run = 2000\n".into()) }, &Policy::new(150)).unwrap();
            out.push(glog_digest(&l2));
        }
        let _ = &mut log;
    }
    // measured lives (no log)
    {
        let mut s = String::new();
        for (seed, fams, budget, noise, drift) in [(1u64, 6usize, 1500i64, 0u32, 0u32), (2, 12, 3000, 100, 20), (3, 24, 3000, 0, 0)] {
            let c = W2Cfg { noise_pm: noise, drift_per_10k: drift, ..W2Cfg::new(seed, fams, budget) };
            let r = run_w2(&c, &mut Learner2::default(), 10, None);
            s.push_str(&format!("{} {} {} {}|{:?}|", r.true_reward, r.reported_reward, r.cost, r.steps, r.recs));
        }
        for (seed, budget) in [(1u64, 600i64), (2, 3000)] {
            let r = run_menu_ext(World::with_extended(seed, true), budget, &mut ModelChooser::new(DEFAULT_BETA), DISCOVER_EVERY, &[]);
            s.push_str(&format!("{} {} {} {:?}|{:?}|", r.run.reward, r.run.cost, r.run.skips, r.run.strategies, r.run.recs));
        }
        for st in SETTINGS {
            let r = run_wl(&WLCfg { seed: 4, setting: st, budget: 400 }, &mut GreedyL::default());
            s.push_str(&format!("{:?}|", r));
        }
        out.push(sha256_hex(&[s.as_bytes()]));
    }
    out
}
