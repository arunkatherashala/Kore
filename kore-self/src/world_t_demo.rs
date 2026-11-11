//! Demo (not a report result): one unattended World T life with `wt_file_key = on`, through the real gate
//! (controller.txt, KILL file, `authorize()`), in a temp home. Synthetic World T, penalty 0, energy weight 1,
//! default budget. Prints evidence rows, a replay check, a determinism digest, and a kill-switch check.
//!
//!   cargo test -p kore-self --offline world_t_demo -- --ignored --nocapture

use crate::safety;
use crate::versioned::{sha256_hex, CauseRow, EventRow, RevisionRow, VersionedLog};
use crate::world_t_tools::{run_world_t, status_world_t};

const SEED: u64 = 9001; // outside every tuning (101..120) and report seed range (1..60, 201..260, 261..320)

/// Formula of `world_t/tests_golden.rs::log_digest` (content of events and revisions; no `recorded_at`), but without the
/// `authorize` audit rows: they carry the wall-clock time and a nonce of the gate call, so they differ between lives by design.
fn log_digest(log: &VersionedLog) -> String {
    let snap = log.open_latest();
    let ev: Vec<EventRow> = snap.rows();
    let rv: Vec<RevisionRow> = snap.rows();
    let mut s = String::new();
    for e in ev.iter().filter(|e| e.source != crate::authorize::SOURCE) {
        s.push_str(&format!("{}|{}|{}|{}|{}|{}
", e.event_id, e.occurred_at, e.source, e.kind, e.text, e.tags));
    }
    for r in &rv {
        s.push_str(&format!("{}|{}|{}|{}|{:.6}|{}|{}
", r.rev_id, r.entity_type, r.key, r.stance, r.confidence, r.rationale, r.author));
    }
    sha256_hex(&[s.as_bytes()])[..16].to_string()
}

fn setup(owner: &str) -> std::path::PathBuf {
    safety::test_support::init();
    let dir = safety::owner_dir(owner);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("controller.txt"), "world_t_run = on\nwt_file_key = on\nmax_steps_per_run = 5000\nmax_energy_per_run = 100000\n").unwrap();
    std::fs::write(dir.join("autonomy_policy.txt"), "[L2]\nworld_t_run\n").unwrap();
    dir
}

/// Run chunks of 5000 steps until the life ends (budget exhausted); returns (chunks, last stop reason).
fn life(owner: &str) -> (usize, String) {
    let mut chunks = 0;
    loop {
        let v = run_world_t(owner, 5000, SEED, None).unwrap();
        chunks += 1;
        let stop = v["stopped"].as_str().unwrap().to_string();
        if stop != "step limit reached" || chunks >= 20 {
            return (chunks, stop);
        }
    }
}

fn short(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if t.len() < s.len() { format!("{t}...") } else { t }
}

#[test]
#[ignore]
fn world_t_demo() {
    println!("# demo, synthetic World T, not a report result (seed {SEED}, penalty 0, energy weight 1, wt_file_key on)");
    let a = "demo-w2-a";
    setup(a);
    let (chunks, stop) = life(a);
    let log = crate::log_tools::open(a).unwrap();
    let snap = log.open_latest();
    let ev: Vec<EventRow> = snap.rows();
    let rv: Vec<RevisionRow> = snap.rows();
    let cs: Vec<CauseRow> = snap.rows();
    let genesis_on = ev.iter().any(|e| e.tags.contains("wt_genesis") && e.tags.contains("wt_file_key:on"));
    let st = crate::being::world_t::engine::StateT::from_events(&ev);
    let tasks = st.step.max(1) as f64;
    println!("life: {chunks} runs, stopped: {stop}; steps {}; genesis records wt_file_key:on: {genesis_on}", st.step);
    println!("reward {} energy {} -> reward per energy {:.3}; right {} wrong {} no_answer {} skips {} -> wrong per 100 tasks {:.2}",
        st.total_reward, st.total_cost, st.total_reward as f64 / st.total_cost.max(1) as f64, st.right, st.wrong, st.no_answer, st.skips, 100.0 * st.wrong as f64 / tasks);
    let dec = rv.iter().filter(|r| r.entity_type == "decision").count();
    let cited = rv.iter().filter(|r| r.entity_type == "decision" && cs.iter().any(|c| c.rev_id == r.rev_id)).count();
    println!("decision revisions {dec}, with >= 1 cited event {cited}; cause links {}", cs.len());
    // 3 example decisions late in the life (they cite earlier outcomes)
    let by_id: std::collections::HashMap<&str, &EventRow> = ev.iter().map(|e| (e.event_id.as_str(), e)).collect();
    let picks: Vec<&RevisionRow> = rv.iter().filter(|r| r.entity_type == "decision" && cs.iter().any(|c| c.rev_id == r.rev_id)).collect();
    for r in [picks.len() / 2, picks.len() * 3 / 4, picks.len() - 1].iter().map(|&i| picks[i]) {
        let d = ev.iter().find(|e| e.kind == "decision" && e.tags.contains(&format!("rev_key:{}", r.key)));
        println!("- decision {}: {} (confidence {:.3}, author {})", r.key, r.stance, r.confidence, r.author);
        if let Some(d) = d {
            println!("    event: {}", short(&d.text, 260));
        }
        let c: Vec<&CauseRow> = cs.iter().filter(|c| c.rev_id == r.rev_id).collect();
        println!("    cites {} events, first 2:", c.len());
        for c in c.iter().take(2) {
            if let Some(e) = by_id.get(c.event_id.as_str()) {
                println!("      [{}] {}", e.kind, short(&e.text, 160));
            }
        }
    }
    let s = status_world_t(a).unwrap();
    println!("replay from the log alone: decisions replayed {}, mismatches {}", s["decisions_replayed"], s["replay_mismatches"]);
    let da = log_digest(&log);
    drop(log);
    // the same life again in a fresh owner directory: same log content (recorded_at excluded)
    let b = "demo-w2-b";
    setup(b);
    life(b);
    let db = log_digest(&crate::log_tools::open(b).unwrap());
    let ea: Vec<EventRow> = crate::log_tools::open(a).unwrap().open_latest().rows();
    let eb: Vec<EventRow> = crate::log_tools::open(b).unwrap().open_latest().rows();
    let nd = ea.iter().zip(&eb).filter(|(x, y)| (&x.event_id, &x.text, &x.tags) != (&y.event_id, &y.text, &y.tags)).count();
    println!("event rows A {}, B {}; rows that differ {nd} (the authorize audit row: wall clock and nonce)", ea.len(), eb.len());
    println!("log digest without audit rows: life A {da}, life B (fresh, same seed) {db}: equal {}", da == db);
    // after a cold re-open the folded state is the same
    let st2 = crate::being::world_t::engine::StateT::from_events(&crate::log_tools::open(a).unwrap().open_latest().rows::<EventRow>());
    println!("fold after re-open: steps {} reward {} energy {} (equal {})", st2.step, st2.total_reward, st2.total_cost, st2.step == st.step && st2.total_reward == st.total_reward && st2.total_cost == st.total_cost);
    assert_eq!(s["replay_mismatches"], 0);
    assert_eq!(da, db);

    // KILL: a third being; the KILL file is written while it runs
    let c = "demo-w2-c";
    let dir = setup(c);
    let kd = dir.clone();
    let h = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(3000));
        std::fs::write(kd.join("KILL"), "").unwrap();
    });
    let v = run_world_t(c, 5000, SEED, None).unwrap();
    h.join().unwrap();
    let steps_logged = status_world_t(c).unwrap()["steps"].clone();
    let rows_before = crate::log_tools::open(c).unwrap().open_latest().rows::<EventRow>().len();
    let again = run_world_t(c, 5000, SEED, None);
    let status_after = status_world_t(c).unwrap();
    let ev_c: Vec<EventRow> = crate::log_tools::open(c).unwrap().open_latest().rows();
    let new_steps = ev_c.iter().skip(rows_before).filter(|e| e.source != crate::authorize::SOURCE && (e.kind == "decision" || e.kind == "action")).count();
    println!("KILL during run: stopped '{}', steps_run {} (of 5000 asked), steps in log {}; run after KILL: {}; steps in log after {}; decision/action rows added after KILL {new_steps}",
        v["stopped"], v["steps_run"], steps_logged, match &again { Ok(_) => "RAN (bad)".to_string(), Err(e) => format!("refused ({})", short(e, 70)) }, status_after["steps"]);
    assert_eq!(v["stopped"], "kill switch");
    assert_eq!(v["steps_run"], steps_logged);
    assert!(again.is_err());
    assert_eq!(new_steps, 0);
}
