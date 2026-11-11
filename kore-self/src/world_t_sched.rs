//! Unattended-run scheduler of the World T being (Digital Life item 4). In-process only.
//!
//! The scheduler calls THE SAME gate as a single run (`world_t_tools::run_world_t_with`: controller.txt,
//! the KILL file before every step, `authorize()` tier L2, the per-run caps), again and again in chunks,
//! within budgets the owner writes in `controller.txt` (`being/sched_ctl.rs`; `sched_run = on` is needed,
//! default off). Before every chunk it re-reads the controller (fail closed), checks KILL, the chunk / step /
//! energy budgets and the free disk of the owner directory. The energy left in the budget is passed to the
//! gate, which can only lower its own `max_energy_per_run` with it.
//!
//! After every chunk: one JSON line in `<owner dir>/sched_health.jsonl` and one `being:sched` note in the
//! log (a new source; no World T reader folds it). At the end: a summary line (`"final": true`) and note,
//! and the stop reason is audited (source `authorize`, tool `controller_sched_<reason>`).
//! The scheduler never starts a chunk after the KILL file appeared, and the gate stops a running chunk
//! before its next step.

#![allow(dead_code)]

use crate::authorize;
use crate::being::sched_ctl::{self, SchedCfg};
use crate::world_t_tools::{controller_text, run_world_t_with};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

pub const HEALTH_FILE: &str = "sched_health.jsonl";
pub const SOURCE: &str = "being:sched";

/// Why the scheduler stopped: (reason tag, limit, requested/reached, text).
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub reason: &'static str,
    pub limit: u64,
    pub value: u64,
    pub what: String,
}

fn stop(reason: &'static str, limit: u64, value: u64, what: impl Into<String>) -> Stop {
    Stop { reason, limit, value, what: what.into() }
}

/// Free space in MB available to the caller on the volume of `dir` (None: unknown; the scheduler then stops).
#[cfg(windows)]
pub fn free_disk_mb(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let w: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
    // SAFETY: `w` is a NUL-terminated wide string that lives across the call; the out pointers are valid u64s.
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) };
    (ok != 0).then_some(a / (1024 * 1024))
}

#[cfg(not(windows))]
pub fn free_disk_mb(dir: &Path) -> Option<u64> {
    let out = std::process::Command::new("df").arg("-Pk").arg(dir).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let kb: u64 = s.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()?;
    Some(kb / 1024)
}

fn dir_bytes(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

fn append_health(owner: &str, line: &Value) {
    use std::io::Write;
    let p = crate::safety::owner_dir(owner).join(HEALTH_FILE);
    let new = !p.exists();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{line}");
    }
    if new {
        crate::safety::set_private(&p);
    }
}

fn note(owner: &str, text: &str, tags: Vec<String>) {
    let _g = crate::log_tools::LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let _ = authorize::append_event_locked(owner, SOURCE, "note", text, tags);
}

fn audit(owner: &str, s: &Stop) {
    let _g = crate::log_tools::LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let _ = authorize::audit_controller_locked(owner, &format!("sched_{}", s.reason), s.limit, s.value, &s.what);
}

/// The controller's scheduler settings, or the refusal (audited).
fn settings(owner: &str) -> Result<SchedCfg, Stop> {
    let text = controller_text(owner);
    let c = sched_ctl::from_text(text.as_deref()).map_err(|e| stop("controller_closed", 0, 0, format!("controller refused the scheduler (fail closed): {e}")))?;
    if !c.run {
        return Err(stop("sched_run_off", 0, 1, format!("{} is not on in controller.txt (default off)", sched_ctl::KEY_RUN)));
    }
    Ok(c)
}

/// Run the World T being of `owner` unattended (created in world `seed` with `budget` energy if new).
pub fn run_sched(owner: &str, seed: u64, budget: Option<i64>) -> Result<Value, String> {
    run_sched_with(owner, seed, budget, &free_disk_mb)
}

/// Same, with the free-disk function injected (tests).
pub fn run_sched_with(owner: &str, seed: u64, budget: Option<i64>, disk: &dyn Fn(&Path) -> Option<u64>) -> Result<Value, String> {
    if let Err(s) = settings(owner) {
        audit(owner, &s);
        return Err(format!("the scheduler did not start: {}", s.what));
    }
    let dir = crate::safety::owner_dir(owner);
    let t0 = Instant::now();
    let (mut chunks, mut steps, mut reward, mut energy) = (0u64, 0u64, 0i64, 0i64);
    let mut last = json!({});
    let fin: Stop = 'run: loop {
        let cfg = match settings(owner) {
            Ok(c) => c,
            Err(s) => break s,
        };
        if chunks >= cfg.max_chunks {
            break stop("max_chunks", cfg.max_chunks, chunks, format!("budget reached: {chunks} chunks (sched_max_chunks {})", cfg.max_chunks));
        }
        if steps >= cfg.max_steps_total {
            break stop("max_steps_total", cfg.max_steps_total, steps, format!("budget reached: {steps} steps (sched_max_steps_total {})", cfg.max_steps_total));
        }
        if energy.max(0) as u64 >= cfg.max_energy_total {
            break stop("max_energy_total", cfg.max_energy_total, energy.max(0) as u64, format!("budget reached: {energy} energy (sched_max_energy_total {})", cfg.max_energy_total));
        }
        if authorize::kill_active(owner) {
            break stop("kill", 0, chunks, format!("stopped by KILL: the KILL file exists; no chunk is started (after {chunks} chunks, {steps} steps)"));
        }
        let free = disk(&dir);
        match free {
            None => break stop("disk_unknown", cfg.min_free_disk_mb, 0, "the free disk space could not be read; no chunk is started (fail closed)"),
            Some(f) if f < cfg.min_free_disk_mb => break stop("disk_low", cfg.min_free_disk_mb, f, format!("free disk {f} MB is below sched_min_free_disk_mb {}; no chunk is started", cfg.min_free_disk_mb)),
            _ => {}
        }
        let ask = cfg.chunk_steps.min(cfg.max_steps_total - steps);
        let cap = (cfg.max_energy_total - energy.max(0) as u64).min(i64::MAX as u64) as i64;
        let tc = Instant::now();
        let r = {
            let _g = crate::log_tools::LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            run_world_t_with(owner, ask, seed, budget, Some(cap))
        };
        let chunk_ms = tc.elapsed().as_millis() as u64;
        let v = match r {
            Ok(v) => v,
            Err(e) if authorize::kill_active(owner) => break stop("kill", 0, chunks, format!("stopped by KILL: the gate refused the chunk ({e})")),
            Err(e) => break stop("gate_refused", 0, chunks, format!("the World T gate refused chunk {}: {e}", chunks + 1)),
        };
        chunks += 1;
        let n = v["steps_run"].as_u64().unwrap_or(0);
        steps += n;
        reward += v["reward"].as_i64().unwrap_or(0);
        energy += v["energy"].as_i64().unwrap_or(0);
        let chunk_stop = v["stopped"].as_str().unwrap_or("").to_string();
        let end = match chunk_stop.as_str() {
            "kill switch" => Some(stop("kill", 0, chunks, format!("stopped by KILL during chunk {chunks} after {n} of {ask} steps; no further chunk is started"))),
            "budget exhausted" => Some(stop("life_over", 0, steps, format!("the being's life ended (budget exhausted) in chunk {chunks}"))),
            "stop flag set" => Some(stop("stop_flag", 0, steps, "the run was stopped by its stop flag")),
            _ if n == 0 => Some(stop("no_progress", ask, 0, format!("chunk {chunks} ran no step ({chunk_stop}); the scheduler stops"))),
            _ => None,
        };
        let line = json!({
            "ts": crate::versioned::now_iso(), "chunk": chunks, "steps": n, "steps_asked": ask, "reward": v["reward"], "energy": v["energy"],
            "steps_total": steps, "reward_total": reward, "energy_total": energy,
            "right_life": v["right"], "wrong_life": v["wrong"], "no_answer_life": v["no_answer"], "skips_life": v["skips"],
            "open_ms": v["open_ms"], "chunk_ms": chunk_ms, "log_bytes": dir_bytes(&dir.join("log")), "free_disk_mb": free,
            "chunk_stop": chunk_stop, "stop": end.as_ref().map(|s| s.reason),
        });
        append_health(owner, &line);
        note(owner, &format!("sched chunk {chunks}: {n} steps, reward {}, energy {}, chunk {chunk_ms} ms, stop {chunk_stop}", v["reward"], v["energy"]),
            vec!["sched".into(), "sched_chunk".into(), format!("chunk:{chunks}"), format!("steps:{n}"), format!("chunk_ms:{chunk_ms}"), format!("stop:{}", chunk_stop.replace(' ', "_"))]);
        last = line;
        if let Some(s) = end {
            break s;
        }
        // the pause between chunks; KILL ends it early (the next loop turn then stops)
        let until = Instant::now() + Duration::from_millis(cfg.interval_ms);
        while Instant::now() < until {
            if authorize::kill_active(owner) {
                continue 'run;
            }
            std::thread::sleep(Duration::from_millis(50).min(until.saturating_duration_since(Instant::now())));
        }
    };
    audit(owner, &fin);
    let wall_ms = t0.elapsed().as_millis() as u64;
    let summary = json!({
        "ts": crate::versioned::now_iso(), "final": true, "stop": fin.reason, "stop_text": fin.what, "chunks": chunks, "steps_total": steps,
        "reward_total": reward, "energy_total": energy, "wall_ms": wall_ms,
        "wrong_life": last["wrong_life"], "skips_life": last["skips_life"], "log_bytes": dir_bytes(&dir.join("log")),
    });
    append_health(owner, &summary);
    note(owner, &format!("sched stopped ({}): {}; {chunks} chunks, {steps} steps, reward {reward}, energy {energy}, {wall_ms} ms", fin.reason, fin.what),
        vec!["sched".into(), "sched_summary".into(), format!("stop:{}", fin.reason), format!("chunks:{chunks}"), format!("steps:{steps}")]);
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety;
    use crate::versioned::EventRow;
    use crate::world_t_tools::status_world_t;

    const BASE: &str = "world_t_run = on\nmax_steps_per_run = 5000\nmax_energy_per_run = 100000\n";

    fn setup(owner: &str, extra: &str) -> std::path::PathBuf {
        safety::test_support::init();
        let dir = safety::owner_dir(owner);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("autonomy_policy.txt"), "[L2]\nworld_t_run\n").unwrap();
        std::fs::write(dir.join("controller.txt"), format!("{BASE}{extra}")).unwrap();
        dir
    }

    fn plenty(_: &Path) -> Option<u64> {
        Some(1_000_000)
    }

    fn rows(owner: &str) -> Vec<EventRow> {
        crate::log_tools::open(owner).unwrap().open_latest().rows()
    }

    fn audited(owner: &str, tool: &str) -> bool {
        rows(owner).iter().any(|e| e.source == authorize::SOURCE && e.tags.contains(&format!("tool:controller_{tool}")))
    }

    fn health(dir: &Path) -> Vec<Value> {
        std::fs::read_to_string(dir.join(HEALTH_FILE)).unwrap_or_default().lines().map(|l| serde_json::from_str(l).expect("well-formed JSON line")).collect()
    }

    #[test]
    fn the_scheduler_is_off_by_default_and_an_invalid_key_fails_closed() {
        let o = "sched-off";
        let dir = setup(o, "");
        let e = run_sched_with(o, 3, Some(800), &plenty).unwrap_err();
        assert!(e.contains("sched_run"), "{e}");
        assert!(audited(o, "sched_sched_run_off"));
        assert!(!status_world_t(o).unwrap()["exists"].as_bool().unwrap());
        std::fs::write(dir.join("controller.txt"), format!("{BASE}sched_run = on\nsched_max_chunks = 0\n")).unwrap();
        let e = run_sched_with(o, 3, Some(800), &plenty).unwrap_err();
        assert!(e.contains("fail closed"), "{e}");
        std::fs::write(dir.join("controller.txt"), format!("{BASE}sched_run = on\nsched_interval_ms = soon\n")).unwrap();
        assert!(run_sched_with(o, 3, Some(800), &plenty).unwrap_err().contains("fail closed"));
        std::fs::remove_file(dir.join("controller.txt")).unwrap();
        assert!(run_sched_with(o, 3, Some(800), &plenty).unwrap_err().contains("fail closed"));
        assert!(audited(o, "sched_controller_closed"));
        assert!(!status_world_t(o).unwrap()["exists"].as_bool().unwrap());
        assert!(!dir.join(HEALTH_FILE).exists());
    }

    #[test]
    fn budgets_stop_the_scheduler_and_the_health_lines_are_well_formed() {
        // chunk budget
        let o = "sched-chunks";
        let dir = setup(o, "sched_run = on\nsched_chunk_steps = 20\nsched_max_chunks = 3\n");
        let v = run_sched_with(o, 3, Some(100_000), &plenty).unwrap();
        assert_eq!(v["stop"], "max_chunks");
        assert_eq!((v["chunks"].as_u64(), v["steps_total"].as_u64()), (Some(3), Some(60)));
        let s = status_world_t(o).unwrap();
        assert_eq!((s["steps"].as_u64(), s["replay_mismatches"].as_u64()), (Some(60), Some(0)));
        let h = health(&dir);
        assert_eq!(h.len(), 4);
        for (i, l) in h[..3].iter().enumerate() {
            assert_eq!(l["chunk"], i as u64 + 1);
            assert_eq!(l["steps"], 20);
            for k in ["reward", "energy", "wrong_life", "skips_life", "open_ms", "chunk_ms", "log_bytes", "free_disk_mb", "chunk_stop", "steps_total"] {
                assert!(!l[k].is_null(), "{k} missing in {l}");
            }
            assert!(l["log_bytes"].as_u64().unwrap() > 0);
        }
        assert_eq!(h[3]["final"], true);
        assert_eq!(h[3]["stop"], "max_chunks");
        assert!(audited(o, "sched_max_chunks"));
        let ev = rows(o);
        assert_eq!(ev.iter().filter(|e| e.source == SOURCE && e.tags.contains("sched_chunk")).count(), 3);
        assert_eq!(ev.iter().filter(|e| e.source == SOURCE && e.tags.contains("sched_summary")).count(), 1);
        // step budget: the last chunk is cut to what is left
        let o = "sched-steps";
        setup(o, "sched_run = on\nsched_chunk_steps = 20\nsched_max_steps_total = 50\n");
        let v = run_sched_with(o, 3, Some(100_000), &plenty).unwrap();
        assert_eq!(v["stop"], "max_steps_total");
        assert_eq!((v["chunks"].as_u64(), v["steps_total"].as_u64()), (Some(3), Some(50)));
        assert_eq!(status_world_t(o).unwrap()["steps"], 50);
        // energy budget: the remaining energy caps the chunk inside the gate
        let o = "sched-energy";
        setup(o, "sched_run = on\nsched_chunk_steps = 200\nsched_max_energy_total = 300\n");
        let v = run_sched_with(o, 3, Some(100_000), &plenty).unwrap();
        assert_eq!(v["stop"], "max_energy_total", "{v}");
        let e = v["energy_total"].as_i64().unwrap();
        assert!(e >= 300 && e < 400, "energy {e}");
        assert!(v["steps_total"].as_u64().unwrap() < 200 * v["chunks"].as_u64().unwrap());
        assert!(audited(o, "sched_max_energy_total"));
        // life over: a small life budget ends the being before any scheduler budget
        let o = "sched-life";
        setup(o, "sched_run = on\nsched_chunk_steps = 100\n");
        let v = run_sched_with(o, 3, Some(300), &plenty).unwrap();
        assert_eq!(v["stop"], "life_over", "{v}");
        assert!(audited(o, "sched_life_over"));
        assert_eq!(status_world_t(o).unwrap()["replay_mismatches"], 0);
    }

    #[test]
    fn the_disk_guard_stops_the_scheduler() {
        let o = "sched-disk";
        let dir = setup(o, "sched_run = on\nsched_chunk_steps = 10\nsched_min_free_disk_mb = 4096\n");
        let calls = std::sync::atomic::AtomicU64::new(0);
        // 2 chunks with enough space, then the disk is "full"
        let disk = |_: &Path| Some(if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2 { 5000 } else { 4095 });
        let v = run_sched_with(o, 3, Some(100_000), &disk).unwrap();
        assert_eq!(v["stop"], "disk_low", "{v}");
        assert_eq!(v["chunks"], 2);
        assert_eq!(status_world_t(o).unwrap()["steps"], 20);
        assert!(audited(o, "sched_disk_low"));
        assert_eq!(health(&dir).last().unwrap()["stop"], "disk_low");
        // an unknown free space also stops it (fail closed)
        let v = run_sched_with(o, 3, None, &|_: &Path| None).unwrap();
        assert_eq!((v["stop"].as_str(), v["chunks"].as_u64()), (Some("disk_unknown"), Some(0)));
        assert_eq!(status_world_t(o).unwrap()["steps"], 20);
    }

    #[test]
    fn kill_stops_the_running_chunk_and_no_further_chunk_starts() {
        // KILL between chunks (written while the disk is checked before chunk 3): the gate refuses, no chunk 3
        let o = "sched-kill-between";
        let dir = setup(o, "sched_run = on\nsched_chunk_steps = 10\n");
        let calls = std::sync::atomic::AtomicU64::new(0);
        let kd = dir.clone();
        let disk = |_: &Path| {
            if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 2 {
                std::fs::write(kd.join("KILL"), "").unwrap();
            }
            Some(1_000_000)
        };
        let v = run_sched_with(o, 3, Some(100_000), &disk).unwrap();
        assert_eq!((v["stop"].as_str(), v["chunks"].as_u64()), (Some("kill"), Some(2)));
        assert!(v["stop_text"].as_str().unwrap().contains("stopped by KILL"));
        assert_eq!(status_world_t(o).unwrap()["steps"], 20);
        assert!(audited(o, "sched_kill"));
        // with KILL present a new scheduler run starts no chunk
        let n0 = rows(o).iter().filter(|e| e.kind == "decision" && e.source != authorize::SOURCE).count();
        let v = run_sched_with(o, 3, None, &plenty).unwrap();
        assert_eq!((v["stop"].as_str(), v["chunks"].as_u64()), (Some("kill"), Some(0)));
        assert_eq!(rows(o).iter().filter(|e| e.kind == "decision" && e.source != authorize::SOURCE).count(), n0);

        // KILL during a chunk: the gate stops before its next step, the scheduler starts no further chunk
        let o = "sched-kill-mid";
        let dir = setup(o, "sched_run = on\nsched_chunk_steps = 5000\nsched_max_chunks = 50\n");
        let kd = dir.clone();
        let h = std::thread::spawn(move || {
            // wait for the first chunk's health line, then KILL during the second chunk
            let t = Instant::now();
            while !kd.join(HEALTH_FILE).exists() && t.elapsed() < Duration::from_secs(600) {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(300));
            std::fs::write(kd.join("KILL"), "").unwrap();
        });
        let v = run_sched_with(o, 3, Some(1_000_000), &plenty).unwrap();
        h.join().unwrap();
        assert_eq!(v["stop"], "kill", "{v}");
        let hl = health(&dir);
        let chunks = hl.iter().filter(|l| l["final"].is_null()).collect::<Vec<_>>();
        let lastc = chunks.last().unwrap();
        // the run either stopped inside a chunk (that chunk reports the kill switch) or before starting the next one
        assert!(lastc["chunk_stop"] == "kill switch" || v["chunks"] == chunks.len() as u64);
        if lastc["chunk_stop"] == "kill switch" {
            assert!(lastc["steps"].as_u64().unwrap() < 5000);
        }
        // every step the scheduler counted is in the log, and the log replays; nothing ran after KILL
        let s = status_world_t(o).unwrap();
        assert_eq!(s["steps"], v["steps_total"]);
        assert_eq!(s["replay_mismatches"], 0);
        let ev = rows(o);
        let kill_at = ev.iter().position(|e| e.tags.contains("controller_stop") || (e.source == authorize::SOURCE && e.tags.contains("tool:controller_sched_kill"))).unwrap();
        assert!(ev[kill_at..].iter().all(|e| !(e.source == crate::being::world_t::world::SOURCE_T && e.kind == "decision")));
        assert!(hl.last().unwrap()["stop"] == "kill");
    }
}
