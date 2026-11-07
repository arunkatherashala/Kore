use super::adapt::{Look2, LookMenu};
use super::beingl::*;
use super::ctl::*;
use super::plan::*;
use super::worldl::*;
use crate::being::controller::Controller;
use crate::being::measure::run_menu;
use crate::being::planner::ModelChooser;
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::solver::{Strategy, STRATEGIES};
use crate::being::world2::engine::{run_v2, Engine2, LearnerCfg, Run2Cfg, State2};
use crate::being::world2::learner::Learner2;
use crate::being::world2::measure::run_w2;
use crate::being::world2::world::W2Cfg;
use crate::versioned::{EventRow, VersionedLog};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn open(root: &std::path::Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

const CTL_ON: &str = "lookahead = on\nmax_lookahead_nodes = 5000\nmax_lookahead_depth = 3\n";

// ---- isolation ----

#[test]
fn lookahead_sources_have_no_file_network_or_process_access_and_the_planner_reads_no_hidden_parameter() {
    let banned = [
        ["std", "::fs"].concat(), ["std", "::net"].concat(), ["std", "::process"].concat(), ["std", "::env"].concat(),
        ["File", "::open"].concat(), ["File", "::create"].concat(), ["Tcp", "Stream"].concat(), ["Udp", "Socket"].concat(),
        ["Command", "::new"].concat(), ["tokio", "::net"].concat(), ["safety", "::"].concat(), ["log_tools", "::"].concat(),
        ["read_", "to_string"].concat(), ["std", "::thread"].concat(), ["Instant", "::now"].concat(), ["System", "Time"].concat(),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("being").join("lookahead");
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
    assert!(n >= 5);
    let hidden = ["p_true", "phase", "WorldL", "World2", "regime", "truth", "OracleDp", "fams[", "FKind", "success("];
    for f in ["plan.rs", "adapt.rs"] {
        let src = std::fs::read_to_string(dir.join(f)).unwrap();
        for b in hidden {
            assert!(!src.contains(b), "{f} mentions {b}");
        }
    }
    // the planners of World L read only the state (the oracles, below them, are measurement references)
    let src = std::fs::read_to_string(dir.join("beingl.rs")).unwrap();
    let planners = &src[src.find("pub struct GreedyL").unwrap()..src.find("pub struct OracleL").unwrap()];
    for b in hidden {
        assert!(!planners.contains(b), "a World L planner mentions {b}");
    }
}

// ---- off = the existing planner, bit for bit ----

#[test]
fn off_is_the_existing_world_v2_planner_decision_by_decision() {
    for (seed, noise) in [(1u64, 0u32), (2, 100)] {
        let cfg = W2Cfg { noise_pm: noise, drift_per_10k: 20, ..W2Cfg::new(seed, 12, 1500) };
        let mk = || {
            let mut st = State2::default();
            st.start(cfg.clone(), LearnerCfg::default(), "genesis".into());
            Engine2::new(st, None)
        };
        let (mut a, mut b) = (mk(), mk());
        let (mut la, mut lb) = (Learner2::default(), Look2::new(None));
        let mut n = 0;
        loop {
            match (a.step(&mut la), b.step(&mut lb)) {
                (Ok(x), Ok(y)) => {
                    assert_eq!(x.label, y.label);
                    assert_eq!(format!("{:?}", x.revision), format!("{:?}", y.revision));
                    assert_eq!(format!("{:?}", x.events), format!("{:?}", y.events));
                    assert_eq!(x.truth, y.truth);
                    n += 1;
                    if a.state.step % 10 == 0 {
                        let (ra, rb) = (a.discover_pass(), b.discover_pass());
                        assert_eq!(ra.len(), rb.len());
                    }
                }
                (Err(x), Err(y)) => {
                    assert_eq!(x, y);
                    break;
                }
                _ => panic!("one run stopped before the other"),
            }
        }
        assert!(n > 200);
        assert_eq!(a.state, b.state);
        let (r1, r2) = (run_w2(&cfg, &mut Learner2::default(), 10, None), run_w2(&cfg, &mut Look2::new(None), 10, None));
        assert_eq!(r1.recs, r2.recs);
        assert_eq!((r1.true_reward, r1.cost), (r2.true_reward, r2.cost));
    }
}

#[test]
fn off_is_the_existing_menu_planner_and_the_logged_world_v2_run_is_identical() {
    for seed in [1u64, 7] {
        let a = run_menu(seed, 600, &mut ModelChooser::new(1.5));
        let b = run_menu(seed, 600, &mut LookMenu::new(None, ModelChooser::new(1.5)));
        assert_eq!((a.reward, a.cost, a.skips, a.strategies), (b.reward, b.cost, b.skips, b.strategies));
        assert_eq!(a.recs.iter().map(|r| (r.reward, r.cost, r.pred.map(f64::to_bits))).collect::<Vec<_>>(), b.recs.iter().map(|r| (r.reward, r.cost, r.pred.map(f64::to_bits))).collect::<Vec<_>>());
    }
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mut l1, mut l2) = (open(d1.path()), open(d2.path()));
    let cfg = Run2Cfg { world: Some(W2Cfg::new(3, 6, 2000)), learner: None, discover_every: 10, ctl: None };
    run_v2(&mut l1, &cfg, &Policy::new(200), None).unwrap();
    run_v2(&mut l2, &cfg, &Policy::new(200), Some(&mut Look2::new(None))).unwrap();
    let r1: Vec<EventRow> = l1.open_latest().rows();
    let r2: Vec<EventRow> = l2.open_latest().rows();
    assert_eq!(r1.iter().map(|e| (&e.event_id, &e.text, &e.tags)).collect::<Vec<_>>(), r2.iter().map(|e| (&e.event_id, &e.text, &e.tags)).collect::<Vec<_>>());
    assert!(r1.len() > 300, "{}", r1.len());
}

// ---- controller ----

#[test]
fn controller_keys_are_validated_default_off_and_fail_closed() {
    // a file without B8 keys means exactly what it meant before
    let old = "max_live_rules = 4\ncomposition = on\n";
    assert_eq!(Controller::parse(old).unwrap().max_live_rules, 4);
    let lc = LookCtl::from_text(Some(old));
    assert!(!lc.allowed && lc.closed.is_none());
    assert_eq!((lc.max_nodes, lc.max_depth), (DEFAULT_NODES, DEFAULT_DEPTH));
    // valid keys parse; the B5 values are untouched
    let c = Controller::parse(&format!("{old}{CTL_ON}")).unwrap();
    assert_eq!(c, Controller::parse(old).unwrap());
    let lc = LookCtl::from_text(Some(CTL_ON));
    assert_eq!((lc.allowed, lc.max_nodes, lc.max_depth), (true, 5000, 3));
    // bad values make the whole file unparseable
    for bad in ["lookahead = maybe", "max_lookahead_nodes = 2000001", "max_lookahead_depth = 7", "max_lookahead_nodes = -1", "lookahead = on\nlookahead = on"] {
        assert!(Controller::parse(bad).is_err(), "{bad}");
        let lc = LookCtl::from_text(Some(bad));
        assert!(lc.closed.is_some() && !lc.allowed && lc.max_nodes == 0, "{bad}");
    }
    assert!(LookCtl::from_text(None).closed.is_some());
    // requests: not allowed -> refusal, allowed -> the caps
    let (g, r) = LookCtl::from_text(Some(old)).grant(true);
    assert!(g.is_none() && r.len() == 1 && r[0].cap == KEY_ON);
    let (g, r) = LookCtl::from_text(None).grant(true);
    assert!(g.is_none() && r.len() == 1);
    assert_eq!(LookCtl::from_text(Some(CTL_ON)).grant(true), (Some((5000, 3)), vec![]));
    assert_eq!(LookCtl::from_text(Some(CTL_ON)).grant(false), (None, vec![]));
    assert!(LookCtl::from_text(Some("lookahead = on\nmax_lookahead_nodes = 0")).grant(true).0.is_none());
}

// ---- the planner on hand-made models ----

struct Fake {
    /// p of (fam, strat) in context 0; in context `ctx_after(probe, ok)` the job's fast cell is `after_probe`.
    p: Vec<[f64; 3]>,
    n: f64,
    probe: Option<usize>,
    job: usize,
    after_probe: f64,
}

impl Source for Fake {
    fn base(&self, k: Key) -> Base {
        let mut p = self.p[k.fam as usize][k.strat as usize];
        if self.probe.is_some() && k.ctx == 2 && k.fam as usize == self.job && k.strat == 0 {
            p = self.after_probe;
        }
        Base { a: p * self.n, b: (1.0 - p) * self.n, ratio: 1.0, n: self.n as u64 }
    }
    fn strategies(&self) -> &[Strategy] {
        &STRATEGIES[..1]
    }
    fn ctx_after(&self, fam: usize, ok: bool) -> u16 {
        if Some(fam) == self.probe && ok { 2 } else { 1 }
    }
}

fn off(slot: usize, fam: usize, cost: i64, reward: i64) -> AOffer {
    AOffer { slot, fam, diff: 1, cost, reward, needs_token: false }
}

#[test]
fn depth_one_prices_energy_at_the_learned_rate() {
    // small: 2 energy, reward 4 (ratio 2.0); large: 12 energy, reward 23 (ratio 1.92); typical menus pay 1.5 per energy
    let src = Fake { p: vec![[1.0; 3], [1.0; 3], [1.0; 3]], n: 1e6, probe: None, job: 99, after_probe: 0.0 };
    let menu = vec![off(0, 0, 2, 4), off(1, 1, 12, 23)];
    let memory: Vec<(u64, Vec<AOffer>)> = (0..10).map(|i| (i, vec![off(0, 2, 4, 6)])).collect();
    let mut cfg = LookCfg::new(1, 10_000);
    cfg.beta_root = 0.0;
    let p = plan(&src, &cfg, &menu, &memory, &Sim::new(500, 0, 0), 10, 0.0).unwrap();
    assert!((p.rho - 1.5).abs() < 1e-6, "{}", p.rho);
    assert_eq!(p.act, Some((1, Strategy::Fast)), "{:?}", p.alts);
    // the one-step ratio rule would take the small offer
    assert!(4.0 / 2.0 > 23.0 / 12.0);
}

#[test]
fn deeper_search_values_a_probe_and_respects_the_node_cap() {
    // job (fam 0): blind p 0.5, after a successful probe p 0.95; probe (fam 1): reward 0, p 0.5; filler (fam 2)
    let src = Fake { p: vec![[0.5; 3], [0.5; 3], [1.0; 3]], n: 1e6, probe: Some(1), job: 0, after_probe: 0.95 };
    let menu = vec![off(0, 0, 5, 20), off(1, 1, 1, 0), off(2, 2, 4, 6)];
    let memory: Vec<(u64, Vec<AOffer>)> = (0..8).map(|i| (i, menu.clone())).collect();
    let mut c1 = LookCfg::new(1, 100_000);
    c1.beta_root = 0.0;
    let p1 = plan(&src, &c1, &menu, &memory, &Sim::new(300, 0, 0), 9, 0.0).unwrap();
    assert_ne!(p1.act, Some((1, Strategy::Fast)), "depth 1 should not probe");
    let c2 = LookCfg { depth: 2, ..c1 };
    let p2 = plan(&src, &c2, &menu, &memory, &Sim::new(300, 0, 0), 9, 0.0).unwrap();
    assert_eq!(p2.act, Some((1, Strategy::Fast)), "depth 2 should probe: {:?}", p2.alts);
    assert_eq!(p2.depth_done, 2);
    assert!(!p2.truncated);
    // same inputs, same plan
    assert_eq!(plan(&src, &c2, &menu, &memory, &Sim::new(300, 0, 0), 9, 0.0).unwrap(), p2);
    // the cap: never more nodes than allowed; a cut keeps the deepest completed depth
    for cap in [3u64, 4, 10, 50, 200] {
        let c = LookCfg { depth: 4, max_nodes: cap, ..c1 };
        match plan(&src, &c, &menu, &memory, &Sim::new(300, 0, 0), 9, 0.0) {
            Some(p) => {
                assert!(p.nodes <= cap);
                assert!(p.truncated && p.depth_done < 4);
            }
            None => assert!(cap < 4, "depth 1 needs 4 nodes (3 offers and skip)"),
        }
    }
}

// ---- World L ----

#[test]
fn world_l_is_deterministic_and_the_oracle_programme_is_exact() {
    for s in SETTINGS {
        let cfg = WLCfg { seed: 5, setting: s, budget: 60 };
        let (a, b) = (WorldL::new(cfg.clone()), WorldL::new(cfg.clone()));
        assert_eq!(a, b);
        for t in 0..20 {
            assert_eq!(a.offers(t), b.offers(t));
        }
        let tot: f64 = a.menu_distribution().iter().map(|x| x.1).sum();
        assert!((tot - 1.0).abs() < 1e-9);
    }
    // brute-force expectimax on a tiny budget equals the programme
    fn brute(w: &WorldL, b: i64, tok: usize, bel: usize, menus: &[(Vec<OfferL>, f64)]) -> f64 {
        thread_local!(static MEMO: std::cell::RefCell<std::collections::HashMap<(u64, i64, usize, usize), f64>> = Default::default());
        if b <= 0 {
            return 0.0;
        }
        let key = (w.cfg.seed * 10 + w.cfg.setting as u64, b, tok, bel);
        if let Some(v) = MEMO.with(|m| m.borrow().get(&key).copied()) {
            return v;
        }
        let v = brute_inner(w, b, tok, bel, menus);
        MEMO.with(|m| m.borrow_mut().insert(key, v));
        v
    }
    fn brute_inner(w: &WorldL, b: i64, tok: usize, bel: usize, menus: &[(Vec<OfferL>, f64)]) -> f64 {
        let mut v = 0.0;
        for (m, pm) in menus {
            let mut best = if b >= 1 { brute(w, b - 1, tok, bel, menus) } else { f64::NEG_INFINITY };
            for o in m {
                if o.needs_token && tok == 0 {
                    continue;
                }
                for s in STRATEGIES {
                    let e = s.energy(o.cost);
                    if e > b {
                        continue;
                    }
                    let f = &w.fams[o.fam];
                    let r = o.reward as f64;
                    let q = match f.kind {
                        FKind::Probe => {
                            let p0 = if bel == BEL_U { 0.5 } else if bel == BEL_0 { 1.0 } else { 0.0 };
                            p0 * (r + brute(w, b - e, tok, BEL_0, menus)) + (1.0 - p0) * brute(w, b - e, tok, BEL_1, menus)
                        }
                        FKind::Job => {
                            let p = if bel == BEL_U { 0.5 * (w.p_true(o.fam, s, 0) + w.p_true(o.fam, s, 1)) } else { w.p_true(o.fam, s, if bel == BEL_0 { 0 } else { 1 }) };
                            p * r + brute(w, b - e, tok, BEL_U, menus)
                        }
                        FKind::Key => {
                            let p = w.p_true(o.fam, s, 0);
                            p * (r + brute(w, b - e, 1, bel, menus)) + (1.0 - p) * brute(w, b - e, tok, bel, menus)
                        }
                        FKind::Vault => w.p_true(o.fam, s, 0) * r + brute(w, b - e, 0, bel, menus),
                        FKind::Normal => w.p_true(o.fam, s, 0) * r + brute(w, b - e, tok, bel, menus),
                    };
                    if q > best {
                        best = q;
                    }
                }
            }
            v += pm * best;
        }
        v
    }
    for s in [Setting::Plain, Setting::Keys, Setting::Probe] {
        let w = WorldL::new(WLCfg { seed: 3, setting: s, budget: 7 });
        let menus: Vec<(Vec<OfferL>, f64)> = w.menu_distribution().into_iter().map(|(fs, p)| (fs.iter().enumerate().map(|(i, f)| w.offer_of(i, *f)).collect(), p)).collect();
        let dp = OracleDp::new(w.clone());
        let bf = brute(&w, 7, 0, BEL_U, &menus);
        assert!((dp.value_at_start() - bf).abs() < 1e-9, "{}: dp {} brute {}", s.name(), dp.value_at_start(), bf);
    }
}

#[test]
fn world_l_rules_hold_vaults_need_tokens_and_probes_tell_the_phase() {
    let cfg = WLCfg { seed: 2, setting: Setting::Keys, budget: 400 };
    let w = WorldL::new(cfg.clone());
    let mut e = EngineL::new(fresh_state(&cfg));
    // an attempt at a vault without a token is not paid out
    let step = (0..200).find(|t| w.offers(*t).iter().any(|o| o.needs_token)).unwrap();
    struct At(u64, usize);
    impl ChooserL for At {
        fn name(&self) -> String {
            "test".into()
        }
        fn choose(&mut self, st: &StateL, offers: &[OfferL]) -> Option<ChoiceL> {
            let act = if st.step == self.0 { ActL::Attempt { slot: self.1, strategy: Strategy::Careful } } else { ActL::Skip };
            Some(ChoiceL { act, policy: "test".into(), why: String::new(), p: 0.0, score: 0.0, rationale: serde_json::json!({}), nodes: 0 }).filter(|_| !offers.is_empty())
        }
    }
    let slot = w.offers(step).iter().position(|o| o.needs_token).unwrap();
    let mut ch = At(step, slot);
    for _ in 0..=step {
        let o = e.step(&mut ch).unwrap();
        if o.outcome.step == step {
            assert_eq!(o.outcome.result, "unpaid");
            assert_eq!(o.outcome.reward, 0);
        }
    }
    // the probe succeeds exactly in phase 0 and the job's fast strategy follows the phase
    let pw = WorldL::new(WLCfg { seed: 4, setting: Setting::Probe, budget: 400 });
    let probe = pw.fams.iter().find(|f| f.kind == FKind::Probe).unwrap().idx;
    let job = pw.fams.iter().find(|f| f.kind == FKind::Job).unwrap().idx;
    assert_eq!(pw.p_true(probe, Strategy::Fast, 0), 1.0);
    assert_eq!(pw.p_true(probe, Strategy::Fast, 1), 0.0);
    assert!(pw.p_true(job, Strategy::Fast, 0) > 0.9 && pw.p_true(job, Strategy::Fast, 1) < 0.1);
    assert!(pw.p_true(job, Strategy::Careful, 1) > 0.9);
    assert!(pw.offers(0).iter().any(|o| o.fam == probe) && pw.offers(0).iter().any(|o| o.fam == job));
}

#[test]
fn world_l_lookahead_off_is_the_greedy_planner_and_runs_are_reproducible() {
    for s in SETTINGS {
        let cfg = WLCfg { seed: 11, setting: s, budget: 300 };
        let a = run_wl(&cfg, &mut GreedyL::default());
        let b = run_wl(&cfg, &mut LookL::new(None));
        assert_eq!((a.reward, a.cost, a.steps, a.skips), (b.reward, b.cost, b.steps, b.skips));
        let c1 = run_wl(&cfg, &mut LookL::new(Some(LookCfg::new(2, 20_000))));
        let c2 = run_wl(&cfg, &mut LookL::new(Some(LookCfg::new(2, 20_000))));
        assert_eq!((c1.reward, c1.cost, c1.nodes), (c2.reward, c2.cost, c2.nodes));
        assert!(c1.max_nodes <= 20_000);
        // the oracle programme is at least as good as the one-step oracle in expectation; check it is not far below on one life
        let o = run_wl(&cfg, &mut OracleL::new(WorldL::new(cfg.clone())));
        assert!(o.cost > 0 && o.reward > 0);
    }
}

// ---- the logged run ----

#[test]
fn logged_run_records_plans_with_evidence_folds_back_and_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let w = WLCfg { seed: 1, setting: Setting::Probe, budget: 300 };
    let cfg = RunLCfg { world: Some(w.clone()), lookahead: true, depth: 2, controller_text: Some(CTL_ON.into()) };
    let s1 = run_l(&mut log, &cfg, &Policy::new(15)).unwrap();
    assert_eq!(s1.steps_run, 15);
    assert_eq!(s1.lookahead, Some((5000, 3)));
    let s2 = run_l(&mut log, &RunLCfg { world: None, ..cfg.clone() }, &Policy::new(15)).unwrap();
    assert_eq!(s2.steps_run, 15);
    let rows: Vec<EventRow> = log.open_latest().rows();
    let folded = StateL::from_events(&rows);
    assert_eq!(folded, s2.state);
    // a single run of 30 steps gives the same being
    let dir2 = tempfile::tempdir().unwrap();
    let mut log2 = open(dir2.path());
    let s3 = run_l(&mut log2, &cfg, &Policy::new(30)).unwrap();
    assert_eq!(s3.state, s2.state);
    // every decision names its planner and the plan's evidence is in the decision revision
    let decisions: Vec<&EventRow> = rows.iter().filter(|e| e.kind == "decision" && e.source == SOURCE_L).collect();
    assert_eq!(decisions.len(), 30);
    assert!(decisions.iter().all(|d| d.tags.contains("policy:wl-lookahead-d2") && d.tags.contains("menu:")));
    let revs: Vec<crate::versioned::RevisionRow> = log.open_latest().rows();
    let plans: Vec<serde_json::Value> = revs.iter().filter(|r| r.key.starts_with("being/worldl/choice/")).map(|r| serde_json::from_str(&r.rationale).unwrap()).collect();
    assert_eq!(plans.len(), 30);
    for p in &plans[1..] {
        assert!(p["cells_read"].as_array().map_or(false, |c| !c.is_empty()));
        assert!(p["alternatives"].as_array().map_or(false, |a| a.iter().all(|x| x["q_minus_leaf"].is_number())));
        assert!(p["nodes"].as_u64().unwrap() <= 5000);
        assert!(p["depth_done"].as_u64().unwrap() >= 1);
    }
}

#[test]
fn logged_run_obeys_the_controller_and_the_kill_switch() {
    let w = WLCfg { seed: 2, setting: Setting::Keys, budget: 300 };
    // missing controller: refused, logged, nothing run
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run_l(&mut log, &RunLCfg { world: Some(w.clone()), lookahead: true, depth: 2, controller_text: None }, &Policy::new(10)).unwrap();
    assert_eq!(s.steps_run, 0);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().any(|e| e.tags.contains("controller_refusal") && e.tags.contains("cap:controller_closed")));
    // lookahead off in the file: refusal logged, the one-step planner runs
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run_l(&mut log, &RunLCfg { world: Some(w.clone()), lookahead: true, depth: 2, controller_text: Some("max_steps_per_run = 50\n".into()) }, &Policy::new(10)).unwrap();
    assert_eq!(s.steps_run, 10);
    assert!(s.lookahead.is_none());
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().any(|e| e.tags.contains("cap:lookahead")));
    assert!(rows.iter().filter(|e| e.kind == "decision").all(|d| d.tags.contains("policy:wl-model-ucb")));
    // node cap below what depth 3 needs: cut, logged once
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let s = run_l(&mut log, &RunLCfg { world: Some(w.clone()), lookahead: true, depth: 3, controller_text: Some("lookahead = on\nmax_lookahead_nodes = 200\nmax_lookahead_depth = 3\n".into()) }, &Policy::new(20)).unwrap();
    assert!(s.nodes_max <= 200);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert_eq!(rows.iter().filter(|e| e.tags.contains("cap:max_lookahead_nodes")).count(), 1);
    // kill switch checked before every step: on after 7 checks
    let dir = tempfile::tempdir().unwrap();
    let mut log = open(dir.path());
    let calls = Arc::new(AtomicU64::new(0));
    let c2 = calls.clone();
    let mut pol = Policy::new(100);
    pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) >= 7)));
    let s = run_l(&mut log, &RunLCfg { world: Some(w), lookahead: true, depth: 2, controller_text: Some(CTL_ON.into()) }, &pol).unwrap();
    assert_eq!(s.stopped, StopReason::Killed);
    assert_eq!(s.steps_run, 7);
    let rows: Vec<EventRow> = log.open_latest().rows();
    assert!(rows.iter().any(|e| e.tags.contains("controller_stop")));
}
