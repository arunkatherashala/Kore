//! Tests of World T: determinism, checkers, the tools, the read-only SQL guard with its timeout and row
//! limit, the learner's view (no hidden names), the logged loop (fold, replay, restart, kill switch,
//! caps), isolation from the other worlds, the controller keys, out-of-distribution worlds, and the
//! banned words.

use super::baselines::*;
use super::ctl;
use super::data::*;
use super::engine::*;
use super::learner::*;
use super::measure::*;
use super::tools::*;
use super::world::*;
use crate::being::controller::{Controller, Limits};
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::versioned::{Batch, EventRow, VersionedLog};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn open_log(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

fn cfg(seed: u64) -> WTCfg {
    WTCfg::new(seed, 1500)
}

fn lim() -> Limits {
    Controller::parse("").unwrap().limits(false, false).0
}

fn run_cfg(c: WTCfg) -> RunTCfg {
    RunTCfg { world: Some(c), learner: None, discover_every: DEFAULT_DISCOVER_EVERY, ctl: Some(lim()), sql: None }
}

fn rows(log: &VersionedLog) -> Vec<EventRow> {
    log.open_latest().rows()
}

fn content(rows: &[EventRow]) -> Vec<(String, String, String, String)> {
    rows.iter().filter(|e| e.source == SOURCE_T).map(|e| (e.kind.clone(), e.occurred_at.clone(), e.text.clone(), e.tags.clone())).collect()
}

#[test]
fn world_t_is_a_pure_function_of_its_config() {
    let (a, b) = (WorldT::new(cfg(4)), WorldT::new(cfg(4)));
    assert_eq!(a.files, b.files);
    assert_eq!(a.order, b.order);
    for step in [0u64, 5, 77, 900] {
        for clock in [0i64, 30 * 600, 30 * 100_000] {
            assert_eq!(a.task(step, clock), b.task(step, clock));
        }
    }
    let c = WorldT::new(cfg(5));
    assert_ne!(a.files[0].bytes, c.files[0].bytes);
    // kinds open on schedule, in a seed-shuffled order, all nine by the end
    assert_eq!(a.open_kinds(0).len(), 3);
    assert_eq!(a.open_kinds(30 * 1_000_000).len(), 9);
    let mut ks = a.order.clone();
    ks.sort();
    assert_eq!(ks, KINDS.to_vec());
    // config tags round trip, OOD only tagged when on
    let mut o = cfg(9);
    assert_eq!(WTCfg::from_tags(&o.tags().join(",")), Some(o.clone()));
    assert!(!o.tags().iter().any(|t| t.starts_with("wt_ood")));
    o.ood = true;
    assert_eq!(WTCfg::from_tags(&o.tags().join(",")), Some(o));
    assert!(WTCfg { files: 0, ..cfg(1) }.validate().is_err());
    assert!(WTCfg { budget: 0, ..cfg(1) }.validate().is_err());
}

#[test]
fn checkers_accept_the_truth_and_reject_near_misses() {
    let w = WorldT::new(cfg(2));
    let mut n = 0;
    for step in 0..600u64 {
        let t = w.task(step, 30 * 1_000_000);
        match &t.truth {
            Truth::Exact(s) => {
                assert!(t.check(s));
                assert!(!t.check(&format!("{s}x")));
                if let Some(c) = parse_cents(s) {
                    assert!(!t.check(&money_canon(c + 1)));
                }
            }
            Truth::Mean { sum, n: k } => {
                let m = (2 * *sum + *k).div_euclid(2 * *k);
                assert!(t.check(&money_canon(m)));
                assert!(!t.check(&money_canon(m + 2)));
            }
            Truth::Tol { value, tol } => {
                assert!(t.check(&(value + tol).to_string()));
                assert!(!t.check(&(value + tol + 1).to_string()));
            }
        }
        n += 1;
    }
    assert_eq!(n, 600);
}

#[test]
fn an_independent_reading_of_every_table_task_agrees_with_its_checker() {
    // the sniffing loader with native statistics or SQL is an independent route from the rendered bytes to
    // the answer; it must agree with the generator's truth on every table, log, fix and format task
    let w = WorldT::new(cfg(3));
    let cx = ToolCtx { rules: &[], order: (0..6).collect(), sql: SqlLimits::default() };
    let mut checked = 0;
    for step in 0..1500u64 {
        let t = w.task(step, 30 * 1_000_000);
        let pl = match t.view.tmpl {
            Tmpl::ColSum | Tmpl::ColMax | Tmpl::ColCount | Tmpl::ColMean | Tmpl::ColDistinct | Tmpl::Lookup => Plan::SniffNative,
            Tmpl::GrpSumWhere | Tmpl::GrpTop | Tmpl::GrpCountGt => Plan::SniffSql,
            Tmpl::LogCount | Tmpl::LogTop => Plan::LogSql,
            Tmpl::FixLine => Plan::FixTypes,
            Tmpl::FormatOf => Plan::FmtSniff,
            _ => continue,
        };
        let f = &w.files[t.file_idx];
        let o = run_plan(pl, &t.view, w.view_bytes(&t.view, &f.bytes), &cx);
        assert!(o.answer.as_deref().map_or(false, |a| t.check(a)), "{} {} {:?} {:?}", t.view.id, pl.name(), o, t.truth);
        checked += 1;
    }
    assert!(checked > 700);
}

#[test]
fn series_tools_see_only_the_visible_prefix() {
    let w = WorldT::new(cfg(6));
    for step in 0..2000u64 {
        let t = w.task(step, 30 * 1_000_000);
        if t.view.prefix == 0 {
            continue;
        }
        let f = &w.files[t.file_idx];
        let b = w.view_bytes(&t.view, &f.bytes);
        assert_eq!(series_values(b).len(), t.view.prefix);
        let all = series_values(&f.bytes);
        assert_eq!(&all[..t.view.prefix], &series_values(b)[..]);
    }
}

#[test]
fn column_rule_search_is_never_wrong_and_learns_its_order() {
    let mut accepted = 0;
    for seed in 1..=6u64 {
        for ood in [false, true] {
            let w = WorldT::new(WTCfg { ood, ..cfg(seed) });
            for f in w.files.iter() {
                let Logical::Derived(dv, blanks) = &f.logical else { continue };
                for b in blanks {
                    let key = &dv.rows[*b].0;
                    let v = ViewT { step: 0, id: String::new(), tmpl: Tmpl::DerivedVal, ctx: f.ctx, file: f.name.clone(), rows: dv.rows.len(), size: 0, reward: 1, param: key.clone(), cols: Default::default(), prefix: 0, tol: 0, prompt: String::new(), penalty_pm: 0, cost_pm: 1000 };
                    let o = run_plan(Plan::ColSearch, &v, &f.bytes, &ToolCtx { rules: &[], order: (0..6).collect(), sql: SqlLimits::default() });
                    assert!(o.tried >= 1);
                    if let Some(a) = &o.answer {
                        assert_eq!(a, &money_canon(dv.rows[*b].4), "rule {:?}", dv.rule);
                        accepted += 1;
                    }
                }
            }
        }
    }
    assert!(accepted > 50, "{accepted}");
    // a form accepted more often in a context is tried first
    assert_eq!(search_order(&[0, 0, 5, 0, 1, 0]), vec![2, 4, 0, 1, 3, 5]);
    assert_eq!(search_order(&[0; 6]), vec![0, 1, 2, 3, 4, 5]);
}

// ---- the SQL tool: read-only, timeout, row limit, all logged ----

fn tbl(n: usize) -> kore_core::DataBlock {
    kore_core::DataBlock::new(vec![kore_core::Column::int64("x", (0..n as i64).map(Some).collect()), kore_core::Column::str_col("g", (0..n).map(|i| Some(format!("g{}", i % 7))).collect())]).unwrap()
}

#[test]
fn write_statements_are_refused_before_the_engine_sees_them_and_the_refusal_is_logged() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    for q in ["DELETE FROM t", "INSERT INTO t VALUES (1, 'a')", "UPDATE t SET x = 1", "DROP TABLE t", "CREATE TABLE u (x INT)", "SELECT 1 FROM t; DELETE FROM t"] {
        let (b, rec) = guarded_sql(tbl(5), q, SqlLimits::default());
        assert!(b.is_none(), "{q} ran");
        assert_eq!(rec.cap.as_deref(), Some("sql_read_only"), "{q}: {rec:?}");
        let r = rec.refusal(3, "test").unwrap();
        log.commit_batch(Batch { events: vec![refusal_event_t(&r, 3, 90)], ..Default::default() }).unwrap();
    }
    let rs = rows(&log);
    let refused: Vec<&EventRow> = rs.iter().filter(|e| e.source == SOURCE_T && e.tags.contains("controller_refusal") && e.tags.contains("cap:sql_read_only")).collect();
    assert_eq!(refused.len(), 6);
    assert!(refused.iter().any(|e| e.text.contains("DELETE FROM t")));
    // a read-only query runs on the copy
    let (b, rec) = guarded_sql(tbl(5), "SELECT COUNT(*) AS n FROM t", SqlLimits::default());
    assert!(rec.ok && rec.cap.is_none() && rec.digest.len() == 12);
    assert_eq!(crate::versioned::sql::block_strings(&b.unwrap()), vec![vec!["5".to_string()]]);
}

#[test]
fn a_query_over_the_timeout_and_a_result_over_the_row_limit_are_refused() {
    // a group-and-sort over 300,000 rows takes well over 1 ms; its result is discarded
    let (b, rec) = guarded_sql(tbl(300_000), "SELECT g, SUM(x) AS s, COUNT(*) AS n FROM t GROUP BY g ORDER BY s DESC", SqlLimits { timeout_ms: 1, max_rows: usize::MAX });
    assert!(b.is_none());
    assert_eq!(rec.cap.as_deref(), Some("sql_timeout_ms"), "{rec:?}");
    // timeout 0 (the fail-closed value) refuses every query without running it
    let (b, rec) = guarded_sql(tbl(5), "SELECT x FROM t", SqlLimits { timeout_ms: 0, max_rows: 10 });
    assert!(b.is_none() && rec.cap.as_deref() == Some("sql_timeout_ms"));
    // more rows than the limit
    let (b, rec) = guarded_sql(tbl(50), "SELECT x FROM t", SqlLimits { timeout_ms: 5000, max_rows: 10 });
    assert!(b.is_none());
    assert_eq!(rec.cap.as_deref(), Some("sql_max_rows"));
    assert_eq!(rec.rows, 50);
    assert_eq!(rec.refusal(1, "t").map(|r| (r.limit, r.requested)), Some((10, 50)));
    let (b, _) = guarded_sql(tbl(10), "SELECT x FROM t", SqlLimits { timeout_ms: 5000, max_rows: 10 });
    assert!(b.is_some());
}

fn logged_run_with_sql(sql: SqlLimits, steps: u64) -> (VersionedLog, RunTSummary, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let c = WTCfg { start_kinds: 9, ..cfg(7) };
    let w = WorldT::new(c.clone());
    let mut rc = run_cfg(c);
    rc.sql = Some(sql);
    let mut sqlonly = AlwaysSqlT;
    let s = run_t(&mut log, &rc, &Policy::new(steps), Some(&mut sqlonly), &MemSandbox(&w)).unwrap();
    (log, s, dir)
}

#[test]
fn every_generated_query_is_logged_with_its_limits_digest_and_checker_outcome_and_refusals_are_logged() {
    let (log, s, _d) = logged_run_with_sql(SqlLimits::default(), 120);
    let rs = rows(&log);
    let acts: Vec<&EventRow> = rs.iter().filter(|e| e.kind == "action" && e.text.contains("SQL #0")).collect();
    assert!(acts.len() > 30, "{}", acts.len());
    for a in &acts {
        assert!(a.text.contains("[read-only, timeout 2000 ms, row limit 1000]: SELECT"), "{}", a.text);
        assert!(a.tags.contains("sql_timeout_ms:2000") && a.tags.contains("sql_max_rows:1000"));
        let step = crate::being::state::tag(&a.tags, "step").unwrap();
        let out = rs.iter().find(|e| e.kind == "outcome" && crate::being::state::tag(&e.tags, "step") == Some(step)).unwrap();
        let st = crate::being::state::tag(&out.tags, "sql_status").unwrap();
        assert!(crate::being::state::tag(&out.tags, "result").is_some());
        if st == "ok" {
            let d = crate::being::state::tag(&a.tags, "sql_digest").unwrap();
            assert_eq!(crate::being::state::tag(&out.tags, "sql_digest"), Some(d));
            assert!(a.text.contains(&format!("result digest {d}")));
        }
    }
    assert!(s.refusals.is_empty(), "{:?}", s.refusals);
    // timeout 0: every query refused, each refusal logged, no SQL answer counted right
    let (log, s, _d) = logged_run_with_sql(SqlLimits { timeout_ms: 0, max_rows: 1000 }, 60);
    let rs = rows(&log);
    let n_ref = rs.iter().filter(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:sql_timeout_ms")).count();
    let n_sql = rs.iter().filter(|e| e.kind == "action" && e.tags.contains("sql_status:sql_timeout_ms")).count();
    assert!(n_sql > 10 && n_ref == n_sql, "{n_ref} {n_sql}");
    assert!(s.truths.iter().filter(|t| t.plan.map_or(false, |p| p.uses_sql())).all(|t| !t.correct));
    // row limit 0: every result has more rows than allowed
    let (log, _s, _d) = logged_run_with_sql(SqlLimits { timeout_ms: 5000, max_rows: 0 }, 60);
    let rs = rows(&log);
    let n_ref = rs.iter().filter(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:sql_max_rows")).count();
    assert!(n_ref > 10);
}

#[test]
fn controller_keys_parse_and_fail_closed() {
    // the World T keys are known to the controller; a bad value makes the whole file invalid
    assert!(Controller::parse("world_t_run = on\nworld_t_sql_timeout_ms = 500\nworld_t_sql_max_rows = 20\n").is_ok());
    assert!(Controller::parse("world_t_run = maybe\n").is_err());
    assert!(Controller::parse("world_t_sql_timeout_ms = 60001\n").is_err());
    assert!(Controller::parse("world_t_sql_max_rows = -1\n").is_err());
    // the run switch: missing file, missing key, off, invalid file -> no run; on -> run
    assert!(ctl::allowed(None).is_err());
    assert!(ctl::allowed(Some("max_steps_per_run = 10\n")).is_err());
    assert!(ctl::allowed(Some("world_t_run = off\n")).is_err());
    assert!(ctl::allowed(Some("world_t_run = on\nbogus = 1\n")).is_err());
    assert!(ctl::allowed(Some("world_t_run = on\n")).is_ok());
    // query limits: missing or invalid file -> no query may run; no key -> defaults; keys -> their values
    assert_eq!(ctl::sql_limits(None), SqlLimits::closed());
    assert_eq!(ctl::sql_limits(Some("world_t_sql_max_rows = x\n")), SqlLimits::closed());
    assert_eq!(ctl::sql_limits(Some("world_t_run = on\n")), SqlLimits::default());
    assert_eq!(ctl::sql_limits(Some("world_t_sql_timeout_ms = 250\nworld_t_sql_max_rows = 7\n")), SqlLimits { timeout_ms: 250, max_rows: 7 });
}

// ---- the learner sees only what is visible ----

#[test]
fn world_t_sources_have_no_file_network_or_process_access_and_the_learner_reads_no_hidden_field() {
    let banned = [
        ["std", "::fs"].concat(), ["std", "::net"].concat(), ["std", "::process"].concat(), ["std", "::env"].concat(),
        ["File", "::open"].concat(), ["File", "::create"].concat(), ["Tcp", "Stream"].concat(), ["Udp", "Socket"].concat(),
        ["Command", "::new"].concat(), ["tokio", "::net"].concat(), ["safety", "::"].concat(), ["log_tools", "::"].concat(),
        ["read_", "to_string"].concat(), ["std", "::thread"].concat(),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being").join("world_t");
    let mut n = 0;
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = f.file_name().to_string_lossy().to_string();
        if name.starts_with("tests") || !name.ends_with(".rs") {
            continue;
        }
        n += 1;
        let src = std::fs::read_to_string(f.path()).unwrap();
        for b in &banned {
            assert!(!src.contains(b.as_str()), "{name} uses {b}");
        }
    }
    assert!(n >= 8);
    let hidden = ["Profile", "profile(", "Logical", "SRule", "DRule", "Truth", "truth", "Render", "gen_", "bad_kinds", "TaskT", "file_idx", "check(", "draw_render"];
    for f in ["learner.rs", "tools.rs"] {
        let src = std::fs::read_to_string(dir.join(f)).unwrap();
        for b in hidden {
            assert!(!src.contains(b), "{f} mentions {b}");
        }
    }
    // the references other than the oracle (the code above `pub struct OracleT`) see no hidden field either
    let src = std::fs::read_to_string(dir.join("baselines.rs")).unwrap();
    let code = src.split("pub struct OracleT").next().unwrap();
    for b in ["Profile", "Logical", "SRule", "DRule", "truth", "Render", "gen_", "file_idx", "check("] {
        assert!(!code.contains(b), "baselines.rs mentions {b}");
    }
}

#[test]
fn the_learner_prefers_cheap_plans_that_work_and_pools_across_kinds() {
    let mut st = StateT::default();
    st.start(cfg(1), LearnerTCfg::default(), "g".into());
    let w = WorldT::new(WTCfg { start_kinds: 9, ..cfg(1) });
    let t = (0..500u64).map(|s| w.task(s, 0)).find(|t| t.view.tmpl == Tmpl::Lookup).unwrap();
    let l = LearnerT { cfg: LearnerTCfg::default() };
    // an empty state: every plan has prior 0.5, so the cheapest plan scores highest
    assert_eq!(l.choose(&st, &t.view).plan, Some(Plan::NaiveNative));
    // the naive loader failed in this context on another template: the pooled prior carries it over
    for i in 0..6 {
        st.apply(&OutcomeT { step: i, tmpl: Some(Tmpl::ColSum), ctx: Some(t.view.ctx), plan: Some(Plan::NaiveNative), result: "no_answer".into(), reward: 0, cost: 1, budget_left: 1000, clock: 30, event_id: format!("e{i}"), example: None, form: None, file: None });
        st.apply(&OutcomeT { step: i, tmpl: Some(Tmpl::ColSum), ctx: Some(t.view.ctx), plan: Some(Plan::SniffNative), result: "correct".into(), reward: 6, cost: 2, budget_left: 1000, clock: 30, event_id: format!("f{i}"), example: None, form: None, file: None });
    }
    let e = l.estimate(&st, &t.view, Plan::NaiveNative);
    assert_eq!(e.prior_from, "context+plan");
    assert!(e.prior < 0.2);
    assert_eq!(l.choose(&st, &t.view).plan, Some(Plan::SniffNative));
    // with the flat prior it starts over at 0.5
    let flat = LearnerT { cfg: LearnerTCfg { pooled: false, ..Default::default() } };
    assert_eq!(flat.estimate(&st, &t.view, Plan::NaiveNative).prior, 0.5);
    // no rule, no rule plan
    let ts = (0..2000u64).map(|s| w.task(s, 0)).find(|t| t.view.tmpl == Tmpl::TsNext).unwrap();
    assert!(!options(&st, &ts.view).contains(&Plan::TsRule));
    assert_eq!(LearnerTCfg::from_tags(&LearnerTCfg::default().tags().join(",")), Some(LearnerTCfg::default()));
}

// ---- the logged loop ----

#[test]
fn fold_of_the_log_equals_the_live_state_and_every_decision_replays() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let c = cfg(11);
    let w = WorldT::new(c.clone());
    let s = run_t(&mut log, &run_cfg(c), &Policy::new(5000), None, &MemSandbox(&w)).unwrap();
    assert!(s.created && s.steps_run > 300, "{}", s.steps_run);
    assert_eq!(s.stopped, StopReason::BudgetExhausted);
    let rs = rows(&log);
    let mut folded = StateT::from_events(&rs);
    folded.kinds_open = s.state.kinds_open.clone();
    assert_eq!(folded, s.state, "the state folded from the log equals the live state");
    let (n, bad) = replay_t(&rs);
    assert_eq!((n, bad), (s.steps_run, 0));
    // every decision cites evidence and lists its options
    let snap = log.open_latest();
    let revs: Vec<crate::versioned::RevisionRow> = snap.rows();
    let dec: Vec<_> = revs.iter().filter(|r| r.entity_type == "decision" && r.key.starts_with("being/worldt/")).collect();
    assert_eq!(dec.len() as u64, s.steps_run);
    assert!(dec.iter().all(|r| r.rationale.contains("\"options\"")));
    // the measurement run takes exactly the same steps as the logged run
    let life = run_life(&cfg(11), Pol::Being(LearnerTCfg::default()), None);
    let a: Vec<_> = life.truths.iter().map(|t| (t.step, t.plan, t.correct, t.cost)).collect();
    let b: Vec<_> = s.truths.iter().map(|t| (t.step, t.plan, t.correct, t.cost)).collect();
    assert_eq!(a, b);
    assert!(life.truths.iter().all(|t| !t.wrong_accepted));
}

#[test]
fn rules_are_discovered_from_revealed_series_and_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open_log(dir.path());
    let c = WTCfg { start_kinds: 9, ..WTCfg::new(18, 4000) };
    let w = WorldT::new(c.clone());
    let s = run_t(&mut log, &run_cfg(c), &Policy::new(5000), None, &MemSandbox(&w)).unwrap();
    assert!(!s.new_rules.is_empty(), "rules were found from revealed examples");
    assert!(s.new_rules.iter().all(|r| r.sig < 120), "no rule on the noisy series");
    let st = StateT::from_events(&rows(&log));
    assert_eq!(st.rules, s.state.rules);
    assert!(s.truths.iter().any(|t| t.rule_used));
}

#[test]
fn a_restart_from_the_log_equals_one_run() {
    let c = cfg(12);
    let w = WorldT::new(c.clone());
    let d1 = tempfile::tempdir().unwrap();
    let mut one = open_log(d1.path());
    run_t(&mut one, &run_cfg(c.clone()), &Policy::new(400), None, &MemSandbox(&w)).unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let mut two = open_log(d2.path());
    run_t(&mut two, &run_cfg(c.clone()), &Policy::new(173), None, &MemSandbox(&w)).unwrap();
    let r2 = run_t(&mut two, &RunTCfg { world: None, ..run_cfg(c.clone()) }, &Policy::new(227), None, &MemSandbox(&w)).unwrap();
    assert!(!r2.created);
    assert_eq!(content(&rows(&one)), content(&rows(&two)));
    // a being keeps one world
    assert!(run_t(&mut two, &run_cfg(cfg(13)), &Policy::new(1), None, &MemSandbox(&w)).is_err());
}

#[test]
fn the_kill_switch_stops_before_the_first_step_and_after_n_steps() {
    let c = cfg(14);
    let w = WorldT::new(c.clone());
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let mut p = Policy::new(1000);
    p.kill = Some(KillCheck(Arc::new(|| true)));
    let s = run_t(&mut log, &run_cfg(c.clone()), &p, None, &MemSandbox(&w)).unwrap();
    assert_eq!((s.steps_run, s.stopped), (0, StopReason::Killed));
    let rs = rows(&log);
    assert!(rs.iter().any(|e| e.source == SOURCE_T && e.tags.contains("controller_stop")));
    assert!(!rs.iter().any(|e| e.kind == "decision"));
    // after 12 steps
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let calls = Arc::new(AtomicU64::new(0));
    let k = calls.clone();
    let mut p = Policy::new(1000);
    p.kill = Some(KillCheck(Arc::new(move || k.fetch_add(1, Ordering::SeqCst) >= 12)));
    let s = run_t(&mut log, &run_cfg(c), &p, None, &MemSandbox(&w)).unwrap();
    assert_eq!((s.steps_run, s.stopped), (12, StopReason::Killed));
    let rs = rows(&log);
    assert_eq!(rs.iter().filter(|e| e.kind == "decision").count(), 12);
    assert!(rs.iter().any(|e| e.tags.contains("controller_stop")));
}

#[test]
fn caps_on_rules_and_energy_hold_and_refusals_are_logged() {
    let c = WTCfg { start_kinds: 9, ..WTCfg::new(15, 4000) };
    let w = WorldT::new(c.clone());
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let mut l = lim();
    l.max_live_rules = 1;
    let rc = RunTCfg { ctl: Some(l), ..run_cfg(c.clone()) };
    let s = run_t(&mut log, &rc, &Policy::new(5000), None, &MemSandbox(&w)).unwrap();
    assert!(s.state.live_rules() <= 1);
    assert!(rows(&log).iter().any(|e| e.tags.contains("cap:max_live_rules")), "a rule over the cap was refused and logged");
    // energy cap: no step starts once the run has spent that much
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let mut p = Policy::new(5000);
    p.max_energy = Some(100);
    let s = run_t(&mut log, &run_cfg(c), &p, None, &MemSandbox(&w)).unwrap();
    assert_eq!(s.stopped, StopReason::EnergyCap);
    assert!(s.cost >= 100 && s.cost < 100 + 30);
    assert!(rows(&log).iter().any(|e| e.tags.contains("cap:max_energy_per_run")));
}

#[test]
fn a_world_t_being_and_a_world_o_being_share_a_log_without_touching_each_other() {
    use crate::being::world_o::engine::{run_o, RunOCfg, StateO};
    use crate::being::world_o::world::WOCfg;
    let wo = WOCfg { intro_every: 400, life_intros: 6, tier_every: 2, drift_pm: 500, ..WOCfg::new(3, 8000) };
    let only = tempfile::tempdir().unwrap();
    let mut lo = open_log(only.path());
    run_o(&mut lo, &RunOCfg { world: Some(wo.clone()), discover_every: 10, ..Default::default() }, &Policy::new(150), None).unwrap();
    let both = tempfile::tempdir().unwrap();
    let mut lb = open_log(both.path());
    let c = cfg(16);
    let w = WorldT::new(c.clone());
    run_t(&mut lb, &run_cfg(c.clone()), &Policy::new(100), None, &MemSandbox(&w)).unwrap();
    run_o(&mut lb, &RunOCfg { world: Some(wo), discover_every: 10, ..Default::default() }, &Policy::new(150), None).unwrap();
    assert_eq!(StateO::from_events(&rows(&lb)), StateO::from_events(&rows(&lo)));
    let solo = tempfile::tempdir().unwrap();
    let mut ls = open_log(solo.path());
    run_t(&mut ls, &run_cfg(c), &Policy::new(100), None, &MemSandbox(&w)).unwrap();
    assert_eq!(StateT::from_events(&rows(&lb)), StateT::from_events(&rows(&ls)));
}

// ---- references, out of distribution ----

#[test]
fn references_run_and_the_answer_everything_oracle_is_an_upper_reference() {
    let c = WTCfg::new(21, 1200);
    let o = run_life(&c, Pol::AnswerAll, None);
    for p in [Pol::Random, Pol::AlwaysSql, Pol::Ucb(UcbCfg { c: 0.3, per_ctx: true, skip: true }), Pol::Lin(LinCfg { alpha: 0.5, skip: true }), Pol::Being(LearnerTCfg::default())] {
        let l = run_life(&c, p, None);
        assert!(l.cost >= 1190, "{}", p.label());
        assert!(l.rpe() <= o.rpe() + 1e-9, "{} {} > oracle {}", p.label(), l.rpe(), o.rpe());
        assert!(l.truths.iter().all(|t| !t.wrong_accepted));
    }
    assert!(o.truths.iter().all(|t| !t.answered || t.correct), "the oracle never answers wrong");
    // the reward-best oracle never answers wrong either, skips more, and earns at least as much per energy
    let r = run_life(&c, Pol::RewardBest, None);
    assert!(r.truths.iter().all(|t| !t.answered || t.correct));
    assert!(r.rpe() >= o.rpe() - 1e-9, "{} < {}", r.rpe(), o.rpe());
    assert!(r.truths.iter().filter(|t| t.plan.is_none()).count() >= o.truths.iter().filter(|t| t.plan.is_none()).count());
}

#[test]
fn the_reward_design_is_off_by_default_and_changes_rewards_and_prices_when_on() {
    // default: no tag, so every earlier World T log keeps its meaning
    assert!(!cfg(1).tags().iter().any(|t| t.starts_with("wt_penalty") || t.starts_with("wt_cost")));
    let c = WTCfg { penalty_pm: 2000, cost_pm: 2000, ..cfg(1) };
    assert_eq!(WTCfg::from_tags(&c.tags().join(",")), Some(c.clone()));
    assert_eq!((scale_energy(1, 2000), scale_energy(3, 2000), scale_energy(3, 500), scale_energy(4, 1000)), (1, 5, 2, 4));
    // a wrong answer costs twice the listed reward; the being learns to avoid the cheap wrong plans
    let l = run_life(&c, Pol::Random, None);
    let wrong: Vec<&TrueT> = l.truths.iter().filter(|t| t.wrong).collect();
    assert!(!wrong.is_empty() && wrong.iter().all(|t| t.reward < 0));
    assert!(l.truths.iter().all(|t| !t.wrong_accepted));
    let b0 = run_life(&WTCfg { penalty_pm: 0, ..c.clone() }, Pol::Being(LearnerTCfg::default()), None);
    let b2 = run_life(&c, Pol::Being(LearnerTCfg::default()), None);
    let share = |l: &LifeT| l.truths.iter().filter(|t| t.wrong).count() as f64 / l.truths.len() as f64;
    assert!(share(&b2) < share(&b0), "{} {}", share(&b2), share(&b0));
    // the logged run with a penalty folds and replays
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let w = WorldT::new(c.clone());
    let s = run_t(&mut log, &run_cfg(c), &Policy::new(300), None, &MemSandbox(&w)).unwrap();
    let rs = rows(&log);
    let mut folded = StateT::from_events(&rs);
    folded.kinds_open = s.state.kinds_open.clone();
    assert_eq!(folded, s.state);
    assert_eq!(replay_t(&rs), (s.steps_run, 0));
}

#[test]
fn value_ucb_is_identical_without_a_penalty_explores_the_penalty_term_and_old_logs_keep_their_rule() {
    let old = LearnerTCfg { value_ucb: false, ..Default::default() };
    // logs written before the field (no wt_ucb tag) fold to the old rule; both values round-trip
    let untagged: Vec<String> = LearnerTCfg::default().tags().into_iter().filter(|t| !t.starts_with("wt_ucb")).collect();
    assert_eq!(LearnerTCfg::from_tags(&untagged.join(",")), Some(old));
    assert_eq!(LearnerTCfg::from_tags(&old.tags().join(",")), Some(old));
    assert_eq!(LearnerTCfg::from_tags(&format!("{},wt_ucb:x", untagged.join(","))), None);
    // without a penalty the two rules take exactly the same steps
    let a = run_life(&cfg(3), Pol::Being(LearnerTCfg::default()), None);
    let b = run_life(&cfg(3), Pol::Being(old), None);
    assert_eq!((a.reward, a.cost, a.truths.len()), (b.reward, b.cost, b.truths.len()));
    assert!(a.truths.iter().zip(&b.truths).all(|(x, y)| x.plan == y.plan));
    // with a harsh penalty an untried plan is no longer certain to lose to the skip bar: the bonus is on the
    // whole outcome (sd at the penalty's scale), so the old rule scores it below the new one
    let mut st = StateT::default();
    let c = WTCfg { penalty_pm: 10_000, start_kinds: 9, ..cfg(1) };
    st.start(c.clone(), LearnerTCfg::default(), "g".into());
    let t = (0..500u64).map(|s| WorldT::new(c.clone()).task(s, 0)).find(|t| t.view.tmpl == Tmpl::Lookup).unwrap();
    let (n, o) = (LearnerT { cfg: LearnerTCfg::default() }.estimate(&st, &t.view, Plan::NaiveNative), LearnerT { cfg: old }.estimate(&st, &t.view, Plan::NaiveNative));
    assert!(o.score < 0.0 && n.score > o.score, "{} {}", n.score, o.score);
    assert!(n.score <= t.view.reward as f64 / n.energy.max(1.0) + 1e-9);
    // after many wrong answers the bonus shrinks and the plan scores below 0 under both rules
    for i in 0..40 {
        st.apply(&OutcomeT { step: i, tmpl: Some(Tmpl::Lookup), ctx: Some(t.view.ctx), plan: Some(Plan::NaiveNative), result: "wrong".into(), reward: -10, cost: 1, budget_left: 1000, clock: 30, event_id: format!("w{i}"), example: None, form: None, file: None });
    }
    assert!(LearnerT { cfg: LearnerTCfg::default() }.estimate(&st, &t.view, Plan::NaiveNative).score < 0.0);
}

#[test]
fn out_of_distribution_worlds_accept_no_wrong_answer_and_the_column_search_stays_never_wrong() {
    let mut col_search_wrong = 0;
    let mut answered = 0;
    for seed in 1..=3u64 {
        let c = WTCfg { ood: true, ..WTCfg::new(seed, 1500) };
        let w = WorldT::new(c.clone());
        assert!(w.files.iter().filter(|f| f.kind_pool == "t").all(|f| f.size == 3));
        let l = run_life(&c, Pol::Being(LearnerTCfg::default()), None);
        assert!(l.truths.iter().all(|t| !t.wrong_accepted));
        col_search_wrong += l.truths.iter().filter(|t| t.plan == Some(Plan::ColSearch) && t.answered && !t.correct).count();
        answered += l.truths.iter().filter(|t| t.answered).count();
    }
    assert_eq!(col_search_wrong, 0);
    assert!(answered > 100);
}

#[test]
fn no_banned_words_in_world_t_sources_logs_or_docs() {
    const BANNED: &[&str] = &["conscious", "truly alive", "sentient", "soul", "alive", "digital life", "living ai", "artificial life", "intelligen", "thinks", "thinking", "feels"];
    let mut texts: Vec<(String, String)> = vec![];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being").join("world_t");
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = f.file_name().to_string_lossy().to_string();
        if name.ends_with(".rs") && !name.starts_with("tests") {
            texts.push((name, std::fs::read_to_string(f.path()).unwrap()));
        }
    }
    let doc = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("docs").join("world_t.md");
    if let Ok(t) = std::fs::read_to_string(&doc) {
        texts.push(("docs/world_t.md".into(), t));
    }
    let d = tempfile::tempdir().unwrap();
    let mut log = open_log(d.path());
    let c = cfg(17);
    let w = WorldT::new(c.clone());
    run_t(&mut log, &run_cfg(c), &Policy::new(300), None, &MemSandbox(&w)).unwrap();
    for e in rows(&log) {
        texts.push(("log".into(), e.text));
    }
    for (what, t) in &texts {
        let low = t.to_lowercase();
        for b in BANNED {
            assert!(!low.contains(b), "{what} contains '{b}'");
        }
    }
}


/// Merge check (brain-v9: B13 + Track W1): the B13 key and the World T keys live side by side in one
/// controller file, each is read by its own reader, and neither overwrites the other; a repeated key
/// is still refused.
#[test]
fn merged_controller_keys_b13_and_world_t_coexist() {
    use crate::being::controller::Controller;
    use crate::being::linval::LinCtl;
    let both = "context_chooser = on\nworld_t_run = on\nworld_t_sql_timeout_ms = 2000\nworld_t_sql_max_rows = 1000\n";
    assert!(Controller::parse(both).is_ok(), "both key sets must parse together");
    assert!(LinCtl::from_text(Some(both)).allowed, "B13 key read as on");
    assert!(super::ctl::allowed(Some(both)).is_ok(), "World T key read as on");
    let b13_only = "context_chooser = on\nworld_t_run = off\n";
    assert!(LinCtl::from_text(Some(b13_only)).allowed);
    assert!(super::ctl::allowed(Some(b13_only)).is_err());
    let wt_only = "context_chooser = off\nworld_t_run = on\n";
    assert!(!LinCtl::from_text(Some(wt_only)).allowed);
    assert!(super::ctl::allowed(Some(wt_only)).is_ok());
    for dup in ["context_chooser = on\ncontext_chooser = off\n", "world_t_run = on\nworld_t_run = on\n"] {
        assert!(Controller::parse(dup).is_err(), "a repeated key must be refused: {dup:?}");
        assert!(!LinCtl::from_text(Some(dup)).allowed);
        assert!(super::ctl::allowed(Some(dup)).is_err());
    }
}
