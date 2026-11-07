//! `measure_b11 worldo-log`: the World O being on a real versioned log, run in chunks (every chunk
//! re-opens the log and re-folds the being's state from it: a cold restart), with a KILL file test,
//! the controller's caps, and at the end: `verify`, fold of the whole log equal to the live state,
//! every decision re-derived from the log alone, consecutive steps. Data under --root only.

use crate::being::controller::Controller;
use crate::being::policy::{KillCheck, Policy, StopReason};
use crate::being::world_o::engine::{replay_o, run_o, RunOCfg, StateO};
use crate::being::world_o::world::{WOCfg, SOURCE_O};
use crate::versioned::{EventRow, LogFormat, VersionedLog};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

const CONTROLLER: &str = "max_live_rules = 200\nmax_building_blocks = 8\nmax_search_candidates = 100000\nmax_steps_per_run = 5000\nmax_energy_per_run = 100000\ncomposition = off\npruning = off\n";

pub fn cmd_worldo_log(args: &[String]) {
    let root = PathBuf::from(arg(args, "--root").expect("--root <dir>"));
    let steps: u64 = arg(args, "--steps").map_or(100_000, |x| x.parse().unwrap());
    let chunk: u64 = arg(args, "--chunk").map_or(5000, |x| x.parse().unwrap());
    let seed: u64 = arg(args, "--seed").map_or(1, |x| x.parse().unwrap());
    let kill_at: u64 = arg(args, "--kill-at").map_or(steps / 2, |x| x.parse().unwrap());
    let cond = arg(args, "--cond").unwrap_or("fix".into());
    std::fs::create_dir_all(&root).unwrap();
    let ctl = Controller::parse(CONTROLLER).expect("controller");
    let (limits, _) = ctl.limits(false, false);
    let kill = root.join("KILL");
    let _ = std::fs::remove_file(&kill);
    let wcfg = WOCfg::new(seed, 400_000_000);
    let lcfg = super::worldo::learner(&cond);
    // the log is created in format 3 (journal), as in the long run of docs/longrun.md
    let log = VersionedLog::open_or_create_with_format(&root.join("log"), "being", || "2030-01-01T00:00:00Z".to_string(), LogFormat::V3).unwrap();
    assert_eq!(log.format(), LogFormat::V3);
    let mut done = log.open_latest().rows().iter().filter(|e| e.source == SOURCE_O && e.kind == "outcome").count() as u64;
    drop(log);
    let mut killed_once = done > kill_at;
    println!("| steps | call wall s | open+fold+run of the call | live rules | families open | log files | log MB |");
    println!("|---|---|---|---|---|---|---|");
    while done < steps {
        let t = Instant::now();
        let mut log = VersionedLog::open_or_create_with_clock(&root.join("log"), "being", || "2030-01-01T00:00:00Z".to_string()).unwrap();
        let mut pol = Policy::new(chunk.min(ctl.max_steps_per_run).min(steps - done));
        pol.max_energy = Some(ctl.max_energy_per_run);
        let k2 = kill.clone();
        pol.kill = Some(KillCheck(Arc::new(move || k2.exists())));
        // the KILL test: the file appears before the call that would cross kill_at; the call must run 0 steps
        if !killed_once && done + chunk > kill_at {
            std::fs::write(&kill, "stop").unwrap();
        }
        let s = run_o(&mut log, &RunOCfg { world: Some(wcfg.clone()), learner: Some(lcfg), discover_every: 10, ctl: Some(limits.clone()) }, &pol, None).unwrap();
        if kill.exists() {
            assert_eq!((s.steps_run, s.stopped), (0, StopReason::Killed), "no step after KILL");
            println!("| {} | {:.1} | KILL file present: stopped, {} steps, stop logged | | | | |", s.state.s2.step, t.elapsed().as_secs_f64(), s.steps_run);
            std::fs::remove_file(&kill).unwrap();
            killed_once = true;
            continue;
        }
        done += s.steps_run;
        let (files, bytes) = dir_stats(&root.join("log"));
        println!("| {} | {:.1} | {:?} | {} | {} | {} | {:.1} |", s.state.s2.step, t.elapsed().as_secs_f64(), s.stopped, s.state.s2.live_rules(), s.state.known.len(), files, bytes as f64 / 1e6);
        if s.steps_run == 0 {
            break;
        }
    }
    let t = Instant::now();
    let log = VersionedLog::open_or_create_with_clock(&root.join("log"), "being", || "2030-01-01T00:00:00Z".to_string()).unwrap();
    let ok = log.verify().is_ok();
    let rows: Vec<EventRow> = log.open_latest().rows();
    let st = StateO::from_events(&rows);
    let outcomes: Vec<u64> = rows.iter().filter(|e| e.source == SOURCE_O && e.kind == "outcome").filter_map(|e| crate::being::state::tag(&e.tags, "step").and_then(|x| x.parse().ok())).collect();
    let consecutive = outcomes.iter().enumerate().all(|(i, s)| *s == i as u64);
    let (n, bad) = replay_o(&rows);
    let stops = rows.iter().filter(|e| e.source == SOURCE_O && e.tags.contains("stop:kill_switch")).count();
    let refusals = rows.iter().filter(|e| e.source == SOURCE_O && e.tags.contains("controller_refusal")).count();
    let rules = rows.iter().filter(|e| e.source == SOURCE_O && e.kind == "note" && e.tags.split(',').any(|t| t == "rule")).count();
    let fams = rows.iter().filter(|e| e.source == SOURCE_O && e.tags.contains("wo_family")).count();
    println!(
        "\nverify {}; steps {} (outcome steps consecutive from 0: {consecutive}); decisions re-derived from the log alone: {n}, mismatches {bad}; kill-switch stops logged {stops}; controller refusals {refusals}; rule notes {rules}; family notes {fams}; live rules after the fold {}; true / reported reward not separated here (no noise); checks took {:.0} s",
        if ok { "ok" } else { "FAILED" }, st.s2.step, st.s2.live_rules(), t.elapsed().as_secs_f64()
    );
}

fn dir_stats(dir: &std::path::Path) -> (u64, u64) {
    let (mut n, mut b) = (0u64, 0u64);
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let (x, y) = dir_stats(&p);
                n += x;
                b += y;
            } else {
                n += 1;
                b += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    (n, b)
}
