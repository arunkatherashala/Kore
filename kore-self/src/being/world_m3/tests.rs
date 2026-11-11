//! World M3 tests. Most run on an author-made base table (`Base::from_rows`, generated below in code: no
//! taxi value). The tests at the bottom need the real source file and run only when `KORE_M3_SOURCE` points
//! at a checked copy (they are skipped otherwise, never failed).

use super::engine::*;
use super::learner::LearnerTCfg;
use super::measure::{run_life, run_life_cached, Pol};
use super::source::*;
use super::tools::{self, sniff_load, us_to_iso, ToolCtx};
use super::world::*;
use crate::being::policy::Policy;
use crate::being::world::Rng;
use crate::versioned::{EventRow, VersionedLog};
use std::sync::{Arc, OnceLock};

/// An author-made table in the source's shape: 31 days x 1500 rows plus a few out-of-month and backwards rows,
/// with NULLs on 5% of the rows (as payment_type 0 rows), negative fares, zero distances and totals that do not
/// add up on some rows.
fn synth() -> Arc<Base> {
    static B: OnceLock<Arc<Base>> = OnceLock::new();
    B.get_or_init(|| {
        let mut r = Rng::new(42);
        let mut rows: Vec<[Option<i64>; 19]> = vec![];
        let row = |r: &mut Rng, t: i64| {
            let null = r.below(20) == 0;
            let fare = if r.below(30) == 0 { -r.range(100, 5000) } else { r.range(300, 9000) };
            let comp = [fare, r.range(0, 3) * 50, 50, r.range(0, 900), if r.below(10) == 0 { 694 } else { 0 }, 100, if null { 0 } else { 250 }, if null { 0 } else { r.range(0, 1) * 175 }];
            let mut total: i64 = comp.iter().sum();
            if r.below(4) == 0 {
                total += r.range(1, 300);
            }
            let pu = [132, 138, 161, 237, 236, 186, 48, 1, 7, 79, 230, 162][r.below(12) as usize];
            let pu = if r.below(3) == 0 { r.range(1, 263) } else { pu };
            let dur = if r.below(500) == 0 { -r.range(60, 3000) } else { r.range(60, 4000) };
            let o = |x: i64| if null { None } else { Some(x) };
            [
                Some(1 + r.below(2)),
                Some(t),
                Some(t + dur),
                o(r.range(0, 4)),
                Some(if r.below(40) == 0 { 0 } else { r.range(10, 2500) }),
                o(if r.below(50) == 0 { 99 } else { 1 + r.below(2) }),
                o(if r.below(30) == 0 { 1 } else { 0 }),
                Some(pu),
                Some(r.range(1, 265)),
                Some(if null { 0 } else { 1 + r.below(4) }),
                Some(comp[0]),
                Some(comp[1]),
                Some(comp[2]),
                Some(comp[3]),
                Some(comp[4]),
                Some(comp[5]),
                Some(total),
                o(comp[6]),
                o(comp[7]),
            ]
        };
        for d in 0..31 {
            for _ in 0..1500 {
                let t = JAN1 + d * DAY + r.range(0, DAY - 1);
                rows.push(row(&mut r, t));
            }
        }
        for t in [JAN1 - 400 * DAY, JAN1 - 5, JAN1 + 31 * DAY + 30] {
            rows.push(row(&mut r, t));
        }
        Arc::new(Base::from_rows(&rows))
    })
    .clone()
}

fn world(seed: u64, budget: i64) -> Arc<WorldT> {
    Arc::new(WorldT::new(M3Cfg::new(seed, budget), synth()))
}

fn cx() -> ToolCtx<'static> {
    ToolCtx { rules: &[], sql: Default::default() }
}

#[test]
fn kore_sql_order_by_two_keys() {
    let b = b"id,v
1,5
2,7
3,7
4,5
5,7
6,5
7,1
";
    let l = sniff_load(b);
    let blk = crate::being::world_t::tools::to_block(&l).unwrap();
    let q = "SELECT v, COUNT(*) AS n FROM t GROUP BY v ORDER BY n DESC, v ASC LIMIT 3";
    let r = crate::being::world_t::tools::run_sql(blk.clone(), q).unwrap();
    let rows = crate::versioned::sql::block_strings(&r);
    eprintln!("{q} -> {rows:?}");
    assert_eq!(rows[..2], [vec!["5".to_string(), "3".into()], vec!["7".to_string(), "3".into()]]);
    let r2 = crate::being::world_t::tools::run_sql(blk, "SELECT MEDIAN(v) AS m FROM t").unwrap();
    assert_eq!(crate::versioned::sql::block_strings(&r2), vec![vec!["5".to_string()]]);
}

#[test]
fn us_timestamps_are_rewritten_by_the_sniffer_only() {
    assert_eq!(us_to_iso("01/05/2024 01:04:09 PM").as_deref(), Some("2024-01-05 13:04:09"));
    assert_eq!(us_to_iso("12/31/2002 12:00:00 AM").as_deref(), Some("2002-12-31 00:00:00"));
    assert_eq!(us_to_iso("2024-01-05 13:04:09"), None);
    let b = b"trip_id;tpep_pickup_datetime;fare_amount\n7;01/05/2024 01:04:09 PM;-12,50\n8;01/06/2024 11:00:00 AM;NA\n";
    let l = sniff_load(b);
    assert_eq!(l.rows[0][1].as_deref(), Some("2024-01-05 13:04:09"));
    assert_eq!(l.rows[1][2], None);
    let n = tools::naive_load(b);
    assert_eq!(n.header.len(), 1);
}

#[test]
fn the_world_is_a_pure_function_of_its_config() {
    let a = world(5, 3000);
    let b = world(5, 3000);
    assert_eq!(a.pool_digest(), b.pool_digest());
    assert_ne!(a.pool_digest(), world(6, 3000).pool_digest());
    assert!(a.files.len() >= 180, "{} files", a.files.len());
    for s in 0..200 {
        assert_eq!(a.task(s, 0), b.task(s, 0));
    }
    let c = M3Cfg::new(5, 3000);
    assert_eq!(M3Cfg::from_tags(&c.tags().join(",")), Some(c));
}

/// Every task's truth agrees with a tool that reads every quirk correctly: the sniffing loader with the native
/// executor or native-filter, the SQL plan for groups, the type check for malformed lines, the sniffer for the
/// format. A disagreement is a bug in the truth, the rendering or the tool.
#[test]
fn the_truth_of_every_task_agrees_with_the_quirk_aware_tools() {
    let w = world(11, 6000);
    let mut checked = 0;
    let mut bad: Vec<String> = vec![];
    let mut by: std::collections::BTreeMap<&str, (usize, String)> = Default::default();
    for step in 0..400u64 {
        for k in KINDS {
            let t = w.task_of(step, k);
            let plan = match t.view.tmpl.kind() {
                Kind::ColStat | Kind::Quantile | Kind::Lookup => Plan::SniffNative,
                Kind::Filter | Kind::Anomaly => Plan::NativeFilter,
                // KORE SQL sorts each ORDER BY key with an unstable sort, so the second key of the top-3 query is not
                // honoured on ties (an engine limit, kept): its truth is checked by the DuckDB cross-check instead
                Kind::Group if t.view.tmpl == Tmpl::GrpTop3 => continue,
                Kind::Group => Plan::SniffSql,
                Kind::FixRow => Plan::FixTypes,
                Kind::Format => Plan::FmtSniff,
                Kind::Ts => continue,
            };
            let f = &w.files[t.file_idx];
            let o = tools::run_plan(plan, &t.view, &f.bytes, &cx());
            checked += 1;
            if !o.answer.as_deref().map_or(false, |a| t.check(a)) {
                bad.push(format!("{} {} -> {:?} ({}) truth {:?}", t.view.prompt, plan.name(), o.answer, o.detail, t.truth));
                let e = by.entry(t.view.tmpl.name()).or_insert((0, bad.last().unwrap().clone()));
                e.0 += 1;
            }
        }
    }
    assert!(bad.is_empty(), "{} of {checked} disagree, by template: {by:#?}", bad.len());
}

/// The SQL plan agrees with the truth on every table task it has a query for.
#[test]
fn sniff_sql_agrees_with_the_truth() {
    let w = world(12, 6000);
    let mut bad: Vec<String> = vec![];
    let mut n = 0;
    for step in 0..250u64 {
        for k in [Kind::ColStat, Kind::Filter, Kind::Quantile, Kind::Lookup, Kind::Anomaly] {
            let t = w.task_of(step, k);
            let f = &w.files[t.file_idx];
            let o = tools::run_plan(Plan::SniffSql, &t.view, &f.bytes, &cx());
            n += 1;
            if !o.answer.as_deref().map_or(false, |a| t.check(a)) {
                bad.push(format!("{} -> {:?} ({}) truth {:?}", t.view.prompt, o.answer, o.detail, t.truth));
            }
        }
    }
    assert!(bad.is_empty(), "{} of {n} disagree: {:?}", bad.len(), &bad[..bad.len().min(5)]);
}

/// Plans that misread quirks are sometimes wrong (the learner has something to learn), never always.
#[test]
fn naive_plans_are_right_on_some_files_and_wrong_on_others() {
    let w = world(13, 6000);
    let (mut right, mut miss) = (0, 0);
    for step in 0..300u64 {
        let t = w.task_of(step, Kind::ColStat);
        let o = tools::run_plan(Plan::NaiveNative, &t.view, &w.files[t.file_idx].bytes, &cx());
        if o.answer.as_deref().map_or(false, |a| t.check(a)) {
            right += 1
        } else {
            miss += 1
        }
    }
    assert!(right > 30 && miss > 30, "right {right} miss {miss}");
}

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

/// The logged run takes exactly the measurement's steps, and every decision replays from the log.
#[test]
fn the_logged_run_equals_the_measurement_and_replays() {
    let w = world(21, 1500);
    let life = run_life(&w, Pol::Being(LearnerTCfg::default()));
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let cfg = RunTCfg { world: Some(w.cfg.clone()), learner: Some(LearnerTCfg::default()), discover_every: DEFAULT_DISCOVER_EVERY, ctl: None, sql: None };
    let s = run_t(&mut log, &cfg, &Policy::new(100_000), None, &MemSandbox(&w), synth()).unwrap();
    assert_eq!(s.truths.len(), life.truths.len());
    for (a, b) in s.truths.iter().zip(&life.truths) {
        assert_eq!((a.step, a.tmpl, a.plan, a.correct, a.reward, a.cost), (b.step, b.tmpl, b.plan, b.correct, b.reward, b.cost));
    }
    let rows: Vec<EventRow> = log.open_latest().rows();
    let (n, bad) = replay_t(&rows, &synth());
    assert_eq!((n, bad), (s.steps_run, 0));
    assert!(rows.iter().all(|e| e.source == SOURCE_T));
    assert_eq!(StateT::from_events(&rows).total_reward, s.reward);
    // a restart in the middle continues the same being
    let d2 = tempfile::tempdir().unwrap();
    let mut log2 = open_log(d2.path());
    run_t(&mut log2, &cfg, &Policy::new(200), None, &MemSandbox(&w), synth()).unwrap();
    let s2 = run_t(&mut log2, &cfg, &Policy::new(100_000), None, &MemSandbox(&w), synth()).unwrap();
    assert_eq!(StateT::from_events(&log2.open_latest().rows::<EventRow>()).total_reward, s.reward);
    assert_eq!(s2.steps_run + 200, s.steps_run);
}

#[test]
fn every_reference_policy_runs_a_life_and_no_wrong_answer_is_paid() {
    let w = world(22, 1200);
    let mut cache = ToolCache::new();
    let pols = [
        Pol::Random,
        Pol::AlwaysSql,
        Pol::Cheapest,
        Pol::Ucb(super::baselines::UcbCfg { c: 0.03, per_ctx: true, skip: true }),
        Pol::Lin(super::baselines::LinCfg { alpha: 1.0, skip: true }),
        Pol::LinFile(super::baselines::LinCfg { alpha: 1.0, skip: true }),
        Pol::RewardBest,
        Pol::Being(LearnerTCfg { file_key: true, ..Default::default() }),
    ];
    for p in pols {
        let l = run_life_cached(&w, p, None, &mut cache);
        assert!(l.cost >= 1100, "{} spent {}", p.label(), l.cost);
        assert!(l.truths.iter().all(|t| !t.wrong_accepted));
    }
}

/// The tools and the learner never name the generator's hidden structures.
#[test]
fn the_tools_and_the_learner_do_not_see_the_generator() {
    for (f, src) in [("tools.rs", include_str!("tools.rs")), ("learner.rs", include_str!("learner.rs"))] {
        for hidden in ["TableSpec", "Logical", "Truth", "slice_rows", "Base", "source::"] {
            assert!(!src.contains(hidden), "{f} mentions {hidden}");
        }
    }
}

// ---- with the real source (KORE_M3_SOURCE) ----

fn real() -> Option<Arc<Base>> {
    static B: OnceLock<Option<Arc<Base>>> = OnceLock::new();
    B.get_or_init(|| {
        let p = std::env::var("KORE_M3_SOURCE").ok()?;
        Some(Arc::new(load(std::path::Path::new(&p)).expect("the source at KORE_M3_SOURCE must pass every check")))
    })
    .clone()
}

/// The pool digest of seed 101 (sha256 over file names and bytes; a hash, not data), pinned after the first build.
const POOL_DIGEST_101: &str = "e4e799aa6717ef5b4f492eb671d3421db8e94761f9c4fc47a7c14b299da882f7";

#[test]
fn the_real_source_has_the_facts_of_the_design_and_seed_101_is_pinned() {
    let Some(b) = real() else {
        eprintln!("KORE_M3_SOURCE not set: skipped");
        return;
    };
    assert_eq!(b.rows, SOURCE_ROWS);
    assert_eq!(b.out_month.len(), 18);
    assert_eq!(b.backwards.len(), 56);
    let n = |p: &dyn Fn(u32) -> bool| (0..b.rows as u32).filter(|r| p(*r)).count();
    assert_eq!(n(&|r| b.get(C_FARE, r).map_or(false, |v| v < 0)), 37_448);
    assert_eq!(n(&|r| b.get(C_TOTAL, r).map_or(false, |v| v < 0)), 35_504);
    assert_eq!(n(&|r| b.get(C_DIST, r) == Some(0)), 60_371);
    assert_eq!(n(&|r| b.get(3, r).is_none()), 140_162);
    let days: Vec<usize> = (1..=31).map(|d| b.by_day[d].len()).collect();
    assert_eq!((*days.iter().min().unwrap(), *days.iter().max().unwrap()), (67_543, 110_515));
    let w1 = WorldT::new(M3Cfg::new(101, DEFAULT_BUDGET), b.clone());
    let w2 = WorldT::new(M3Cfg::new(101, DEFAULT_BUDGET), b.clone());
    assert_eq!(w1.pool_digest(), w2.pool_digest());
    assert_eq!(w1.pool_digest(), POOL_DIGEST_101, "pin this digest");
}
