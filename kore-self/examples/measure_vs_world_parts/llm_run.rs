//! `measure_vs_world llm`: the language-model player (Claude CLI, haiku alias) in World v2, small clean, a
//! short budget, a few worlds. SMALL n: indicative only.
//!
//! The program (venv python, -I, the wrapper `claude_player.py`) is run WITHOUT a shell, with a cleared
//! environment (only PATH, SystemRoot, TEMP, TMP, which the B4b pilot found sufficient), working directory
//! `<out>/scratch`, a 90 s timeout and a 64 KiB output cap. Every request is scanned before it is sent and
//! every request and reply before it is written to `<out>/calls.jsonl`: for the home path (single
//! backslash, doubled backslash and slash forms), the user name and credential words; replies are redacted
//! (both path forms, see docs/measured.md Track B4b on the doubled-backslash gap).

use crate::being::measure::mean_ci;
use crate::being::vs_world::llm::{CallRec, LlmPlayer};
use crate::being::world2::measure::run_w2;
use crate::being::world2::world::{W2Cfg, World2};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_PYTHON: &str = r"C:\Users\skathera\Downloads\asistent\.venv\Scripts\python.exe";
const TIMEOUT_S: u64 = 90;
const MAX_OUT: usize = 65536;

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter().find_map(|a| a.strip_prefix(&format!("{key}=")).map(|v| v.to_string()))
}

/// The home path in its three textual forms, the user name, and credential words.
pub fn forbidden() -> Vec<String> {
    let mut v: Vec<String> = vec![];
    for k in ["USERPROFILE", "HOME", "APPDATA", "LOCALAPPDATA"] {
        if let Ok(x) = std::env::var(k) {
            if x.len() >= 5 {
                v.push(x.clone());
                v.push(x.replace('\\', "/"));
                v.push(x.replace('\\', "\\\\"));
            }
        }
    }
    if let Ok(x) = std::env::var("USERNAME") {
        if x.len() >= 3 {
            v.push(x);
        }
    }
    for s in ["sk-ant", ".credentials", "oauth", "bearer ", "api_key", "apikey", "password", "c:\\users", "c:\\\\users", "c:/users"] {
        v.push(s.to_string());
    }
    v.sort();
    v.dedup();
    v
}

fn hits(text: &str, forb: &[String]) -> usize {
    let low = text.to_lowercase();
    forb.iter().filter(|f| low.contains(&f.to_lowercase())).count()
}

/// Replace every forbidden string (case-insensitive) by [REDACTED]; returns (text, number of replacements).
fn redact(text: &str, forb: &[String]) -> (String, usize) {
    let mut out = text.to_string();
    let mut n = 0;
    let mut fs: Vec<&String> = forb.iter().collect();
    fs.sort_by_key(|f| std::cmp::Reverse(f.len()));
    for f in fs {
        loop {
            let low = out.to_lowercase();
            let Some(i) = low.find(&f.to_lowercase()) else { break };
            out.replace_range(i..i + f.len(), "[REDACTED]");
            n += 1;
        }
    }
    (out, n)
}

/// Run the wrapper once: no shell, cleared environment, timeout, output cap.
fn run_program(python: &str, script: &str, scratch: &Path, input: &str) -> Result<String, String> {
    let mut cmd = Command::new(python);
    cmd.arg("-I").arg(script).current_dir(scratch).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    for k in ["PATH", "SystemRoot", "TEMP", "TMP"] {
        if let Ok(v) = std::env::var(k) {
            cmd.env(k, v);
        }
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot start: {e}"))?;
    child.stdin.take().ok_or("no stdin")?.write_all(input.as_bytes()).map_err(|e| e.to_string())?;
    let mut out = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut buf = vec![];
        let _ = out.by_ref().take(MAX_OUT as u64 + 1).read_to_end(&mut buf);
        buf
    });
    let t0 = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(st) => {
                let buf = reader.join().unwrap_or_default();
                if buf.len() > MAX_OUT {
                    return Err("printed more than the output cap".into());
                }
                if !st.success() {
                    return Err(format!("exited with {st}"));
                }
                return Ok(String::from_utf8_lossy(&buf).to_string());
            }
            None if t0.elapsed() > Duration::from_secs(TIMEOUT_S) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("did not exit within {TIMEOUT_S} s"));
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

pub fn cfg_of(seed: u64, budget: i64) -> W2Cfg {
    W2Cfg::new(seed, 6, budget)
}

pub fn main_llm(args: &[String]) {
    let out = PathBuf::from(arg(args, "out").expect("out=DIR is required"));
    let (a, b) = match arg(args, "seeds").unwrap_or("1-5".into()).split_once('-') {
        Some((a, b)) => (a.parse::<u64>().unwrap_or(1), b.parse::<u64>().unwrap_or(5)),
        None => (1, 5),
    };
    let budget: i64 = arg(args, "budget").and_then(|x| x.parse().ok()).unwrap_or(80);
    let max_calls: u64 = arg(args, "calls").and_then(|x| x.parse().ok()).unwrap_or(60);
    let python = arg(args, "python").unwrap_or(DEFAULT_PYTHON.into());
    let script = arg(args, "script").unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join("llm_advisor").join("claude_player.py").to_string_lossy().to_string());
    let scratch = out.join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    let done: Vec<u64> = std::fs::read_to_string(out.join("worlds.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| v["seed"].as_u64()).collect();
    let forb = forbidden();
    eprintln!("LLM player (small n, indicative only): World v2 small clean, budget {budget}, seeds {a}..{b}, at most {max_calls} calls per world; out {}", out.display());
    for seed in a..=b {
        if done.contains(&seed) {
            eprintln!("seed {seed}: done already");
            continue;
        }
        let cfg = cfg_of(seed, budget);
        let mut sink = std::fs::OpenOptions::new().create(true).append(true).open(out.join("calls.jsonl")).expect("calls.jsonl");
        let mut req_hits = 0usize;
        let mut lat: Vec<f64> = vec![];
        let t0 = Instant::now();
        let (run, calls, stopped) = {
            let mut caller = |req: &Value| -> Result<String, String> {
                let text = req.to_string();
                let h = hits(&text, &forb);
                req_hits += h;
                if h > 0 {
                    return Err("request held a forbidden string: not sent".into());
                }
                let t = Instant::now();
                let r = run_program(&python, &script, &scratch, &text);
                lat.push(t.elapsed().as_secs_f64());
                eprintln!("  seed {seed}: call {:>3} {:>5.1} s {}", lat.len(), lat[lat.len() - 1], match &r { Ok(x) => x.trim().chars().take(80).collect::<String>(), Err(e) => format!("ERROR {e}") });
                r
            };
            let mut p = LlmPlayer::new(World2::new(cfg.clone()), Box::new(&mut caller), max_calls);
            let run = run_w2(&cfg, &mut p, 0, None);
            (run, p.calls.clone(), p.stopped.clone())
        };
        let wall = t0.elapsed().as_secs_f64();
        let mut redactions = 0;
        let mut reply_hits = 0;
        let mut li = 0usize;
        for c in &calls {
            reply_hits += hits(&c.reply, &forb);
            let (rep, n) = redact(&c.reply, &forb);
            redactions += n;
            let latency = if c.class != "not_asked" {
                li += 1;
                lat.get(li - 1).copied()
            } else {
                None
            };
            let line = json!({ "seed": seed, "step": c.step, "class": c.class, "detail": c.detail, "latency_s": latency.map(|x| (x * 100.0).round() / 100.0), "choice": c.choice, "own_right": c.own_right, "request_as_sent": c.request, "reply_redacted": rep });
            let _ = writeln!(sink, "{line}");
        }
        // counterfactual: the model's own answers instead of the careful execution of those steps
        let mut cf_reward = run.true_reward;
        for c in calls.iter().filter(|c| c.own_right.is_some()) {
            if let Some(r) = run.recs.iter().find(|r| r.step == c.step) {
                cf_reward -= r.true_reward;
            }
            if c.own_right == Some(true) {
                cf_reward += c.own_task.map_or(0, |t| t.0);
            }
        }
        let count = |k: &str| calls.iter().filter(|c| c.class == k).count();
        let strategies: std::collections::BTreeMap<String, usize> = ["fast", "careful", "deep", "own"].iter().map(|s| (s.to_string(), calls.iter().filter(|c| c.class == "valid" && c.choice.ends_with(&format!(":{s}"))).count())).collect();
        let w = json!({
            "seed": seed, "budget": budget, "steps": run.steps, "rpe": run.per_energy(), "rpe_own_answers_counterfactual": cf_reward as f64 / run.cost.max(1) as f64,
            "calls": count("valid") + count("invalid") + count("failed"), "valid": count("valid"), "invalid": count("invalid"), "failed": count("failed"), "not_asked": count("not_asked"),
            "stopped": stopped, "own_answers": calls.iter().filter(|c| c.own_right.is_some()).count(), "own_right": calls.iter().filter(|c| c.own_right == Some(true)).count(),
            "skips": calls.iter().filter(|c| c.choice == "skip").count(),
            "strategies": strategies,
            "latency_s": lat, "wall_s": wall, "request_forbidden_hits": req_hits, "reply_forbidden_hits": reply_hits, "redactions": redactions,
        });
        let mut ws = std::fs::OpenOptions::new().create(true).append(true).open(out.join("worlds.jsonl")).expect("worlds.jsonl");
        let _ = writeln!(ws, "{w}");
        eprintln!("seed {seed}: rpe {:.3}, {} calls, wall {:.0} s, stopped {:?}", run.per_energy(), lat.len(), wall, stopped);
    }
}

fn med(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

pub fn report(args: &[String]) {
    let out = PathBuf::from(arg(args, "out").expect("out=DIR is required"));
    let ws: Vec<Value> = std::fs::read_to_string(out.join("worlds.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    if ws.is_empty() {
        println!("no finished worlds in {}", out.display());
        return;
    }
    let seeds: Vec<u64> = ws.iter().filter_map(|w| w["seed"].as_u64()).collect();
    let budget = ws[0]["budget"].as_i64().unwrap_or(80);
    let llm: Vec<f64> = ws.iter().map(|w| w["rpe"].as_f64().unwrap_or(0.0)).collect();
    let cf: Vec<f64> = ws.iter().map(|w| w["rpe_own_answers_counterfactual"].as_f64().unwrap_or(0.0)).collect();
    println!("\n## LLM player, World v2 small clean (6 families), budget {budget}, seeds {:?} (n = {}; SMALL n, indicative only)\n", seeds, seeds.len());
    // the same worlds and budget for every other policy
    let ms = super::chosen_v2();
    let mut rows: Vec<(String, Vec<f64>)> = vec![];
    for o in super::OURS2 {
        rows.push((o.name().to_string(), seeds.iter().map(|s| super::run_ours2(o, &cfg_of(*s, budget))).collect()));
    }
    for m in &ms {
        rows.push((m.label(), seeds.iter().map(|s| super::run_m2(m, &cfg_of(*s, budget))).collect()));
    }
    println!("| policy | true reward per energy, mean [95% t interval, df = n-1] | LLM minus policy (better/worse/tie) |");
    println!("|---|---|---|");
    println!("| LLM player (haiku) | {} | - |", super::fci(&llm));
    println!("| LLM player, its own answers counted (per-step counterfactual) | {} | - |", super::fci(&cf));
    for (n, v) in &rows {
        println!("| {n} | {} | {} |", super::fci(v), super::fdiff(&llm, v));
    }
    let lat: Vec<f64> = ws.iter().flat_map(|w| w["latency_s"].as_array().cloned().unwrap_or_default()).filter_map(|x| x.as_f64()).collect();
    let sum = |k: &str| ws.iter().map(|w| w[k].as_u64().unwrap_or(0)).sum::<u64>();
    println!("\nper world: {}", ws.iter().map(|w| format!("seed {} rpe {:.3} steps {} calls {} valid {} invalid {} failed {} own {}/{} skips {} stopped {}", w["seed"], w["rpe"].as_f64().unwrap_or(0.0), w["steps"], w["calls"], w["valid"], w["invalid"], w["failed"], w["own_right"], w["own_answers"], w["skips"], w["stopped"])).collect::<Vec<_>>().join("; "));
    println!(
        "\ncalls {} (valid {}, invalid {}, failed {}), latency median {:.1} s, max {:.1} s, total wall {:.0} min; own answers {} of which right {}; forbidden strings in requests {} (requests with a hit are not sent), in replies {}, redactions {}",
        sum("calls"), sum("valid"), sum("invalid"), sum("failed"), med(lat.clone()), lat.iter().cloned().fold(0.0, f64::max),
        ws.iter().map(|w| w["wall_s"].as_f64().unwrap_or(0.0)).sum::<f64>() / 60.0, sum("own_answers"), sum("own_right"), sum("request_forbidden_hits"), sum("reply_forbidden_hits"), sum("redactions")
    );
    let strat: Vec<String> = ws.iter().map(|w| w["strategies"].to_string()).collect();
    println!("strategies chosen (valid replies) per world: {}", strat.join("; "));
    let _ = mean_ci;
    let _: Option<CallRec> = None;
}
