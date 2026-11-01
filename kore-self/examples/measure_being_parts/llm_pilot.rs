//! Track B4b: a small PILOT of a real language model (the Claude CLI) as the being's advisor.
//!
//!   cargo run -p kore-self --example measure_being --offline --release -- llm-pilot [seeds=1-5] [budget=300] [out=DIR] [python=PATH] [script=PATH]
//!   cargo run -p kore-self --example measure_being --offline --release -- llm-pilot-report [out=DIR]
//!
//! Per world seed, IDENTICAL worlds run in these arms (B4 settings, trust learner at its defaults):
//! no advisor; simulated advisors q = 0.5 and q = 0.9; a simulated advisor at the real advisor's measured
//! q-hat (context); the REAL advisor through the real `CommandAdvisor` (the owner-configured program, run
//! without a shell, cleared environment, timeout, output cap, 3-failures-in-a-row rule); the oracle.
//! The harness writes its OWN advisor.txt inside a temporary data root (`<out>/home`), never the owner's.
//!
//! n = 5 worlds, one model, one run per world: this is a pilot. Intervals are only indicative.
//! Results are appended to `<out>/worlds.jsonl` (one line per finished world) and every call to
//! `<out>/calls.jsonl` as it happens, so partial results are usable; finished seeds are skipped on a re-run.

use super::advice_mode::{met, Met};
use super::being::advice::{advice_event, Advice, AdviceRequest, Advisor, Ranker};
use super::being::measure::*;
use super::being::planner::AdviceMode;
use super::being::solver::{Strategy, STRATEGIES};
use super::being::world::World;
use super::chat::advisor_cmd::{load_config, CommandAdvisor};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

const OWNER: &str = "pilot";
const DEFAULT_PYTHON: &str = r"C:\Users\skathera\Downloads\asistent\.venv\Scripts\python.exe";
const MODEL_NOTE: &str = "claude CLI, --model haiku (alias), tools denied, one turn";

#[derive(Clone, Debug)]
struct Call {
    step: u64,
    class: &'static str,
    detail: String,
    latency_s: f64,
    oracle: Option<(usize, Strategy)>,
    advised: Option<(usize, Strategy)>,
    value_ratio: Option<f64>,
    p_stated: Option<f64>,
    leak_hits: usize,
    redactions: usize,
}

/// Wraps the real advisor: records every call (latency, validity, agreement with the oracle's choice) and the
/// logged advice event text, and scans what was logged for home paths and credentials.
struct Recording {
    inner: CommandAdvisor,
    ranker: Ranker,
    seed: u64,
    calls: RefCell<Vec<Call>>,
    sink: RefCell<std::fs::File>,
    forbidden: Vec<String>,
    consecutive_fail: Cell<u32>,
}

fn classify(a: &Advice) -> (&'static str, String) {
    match &a.error {
        None => if a.menu.is_some() { ("valid", String::new()) } else { ("no_option", String::new()) },
        Some(e) if e.starts_with("not asked") => ("not_asked", e.clone()),
        Some(e) if e.contains("timed out") || e.contains("did not exit within") => ("timeout", e.clone()),
        Some(e) if e.contains("exit code: 2") => ("cli_failed", e.clone()),
        Some(e) if e.contains("exit code: 3") => ("no_json_in_model_text", e.clone()),
        Some(e) if e.contains("exited with") || e.contains("cannot start") || e.contains("printed more than") => ("process_failed", e.clone()),
        Some(e) if e.contains("not valid JSON") => ("bad_json", e.clone()),
        Some(e) => ("invalid_content", e.clone()),
    }
}

impl Advisor for Recording {
    fn id(&self) -> String {
        self.inner.id()
    }
    fn advise(&self, req: &AdviceRequest) -> Advice {
        let t0 = Instant::now();
        let a = self.inner.advise(req);
        let latency_s = t0.elapsed().as_secs_f64();
        let ranked = (self.ranker)(req);
        let best = ranked.first().copied();
        let (class, detail) = classify(&a);
        let advised = a.menu.as_ref().map(|m| (m.slot, m.strategy));
        let value_ratio = match (&a.menu, best) {
            (Some(m), Some(b)) if b.value > 0.0 => ranked.iter().find(|r| r.slot == m.slot && r.strategy == m.strategy).map(|r| r.value / b.value),
            _ => None,
        };
        // the text that the being would log for this advice (request as sent, redacted; reply, redacted)
        let (ev, _, _) = advice_event(&self.inner.id(), "2000-01-01T00:00:00Z".into(), req, &a, &[]);
        let logged = ev.text.clone();
        let low = logged.to_lowercase();
        let leak_hits = self.forbidden.iter().filter(|f| low.contains(&f.to_lowercase())).count();
        let redactions = logged.matches("[REDACTED").count();
        if class == "not_asked" {
            // not a call
        } else if class == "valid" || class == "no_option" {
            self.consecutive_fail.set(0);
        } else {
            self.consecutive_fail.set(self.consecutive_fail.get() + 1);
        }
        let c = Call { step: req.step, class, detail, latency_s, oracle: best.map(|b| (b.slot, b.strategy)), advised, value_ratio, p_stated: a.menu.as_ref().and_then(|m| m.p_success), leak_hits, redactions };
        let line = json!({
            "seed": self.seed, "step": c.step, "class": c.class, "detail": c.detail, "latency_s": (c.latency_s * 100.0).round() / 100.0,
            "oracle": c.oracle.map(|(s, st)| format!("{s}:{}", st.name())), "advised": c.advised.map(|(s, st)| format!("{s}:{}", st.name())),
            "match": c.advised.is_some() && c.advised == c.oracle, "value_ratio": c.value_ratio, "p_stated": c.p_stated,
            "leak_hits": c.leak_hits, "redactions": c.redactions, "logged_advice_event_text": logged,
        });
        {
            let mut f = self.sink.borrow_mut();
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
        eprintln!("  seed {} step {:>3}: {:<18} {:>5.1}s advised {:?} oracle {:?}", self.seed, c.step, c.class, c.latency_s, c.advised.map(|(s, st)| format!("{s}:{}", st.name())), c.oracle.map(|(s, st)| format!("{s}:{}", st.name())));
        self.calls.borrow_mut().push(c);
        a
    }
}

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter().find_map(|a| a.strip_prefix(&format!("{key}=")).map(|v| v.to_string()))
}

fn med(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

fn met_json(m: &Met) -> Value {
    json!({
        "rpe": m.rpe, "energy": m.energy, "attempts": m.attempts, "advice_steps": m.advice_steps, "followed": m.followed,
        "tests": m.tests, "test_wins": m.test_wins, "trust": m.trust, "weight": m.weight, "thirds": m.thirds,
    })
}

fn trust_series(r: &AdvRun) -> Value {
    // pooled running Beta(1,1) mean of the test outcomes (no discounting after alarms), read at quarters of the run
    let n = r.info.len();
    let marks: Vec<usize> = (1..=4).map(|k| (n * k + 3) / 4).collect();
    let (mut t, mut w) = (0u64, 0u64);
    let mut out = vec![];
    for (i, s) in r.info.iter().enumerate() {
        if s.tested {
            t += 1;
            if s.win == Some(true) {
                w += 1;
            }
        }
        if marks.contains(&(i + 1)) {
            out.push(json!({"step": i + 1, "tests": t, "wins": w, "mean": (1.0 + w as f64) / (2.0 + t as f64)}));
        }
    }
    json!(out)
}

fn final_cells(r: &AdvRun, beta: f64) -> Value {
    json!(r.state.trust.cells().map(|((adv, kind), c)| json!({"advisor": adv, "kind": kind, "tests": c.tests, "wins": c.test_wins, "given": c.given, "followed_against_model": c.followed_against_model, "mean": c.mean(), "weight": c.weight(beta), "alarms": c.alarms})).collect::<Vec<_>>())
}

fn forbidden_strings() -> Vec<String> {
    let mut v: Vec<String> = vec![];
    for k in ["USERPROFILE", "HOME", "APPDATA", "LOCALAPPDATA"] {
        if let Ok(x) = std::env::var(k) {
            if x.len() >= 5 {
                v.push(x.replace('\\', "/"));
                v.push(x.replace('/', "\\\\")); // as escaped inside logged JSON text
                v.push(x);
            }
        }
    }
    if let Ok(x) = std::env::var("USERNAME") {
        if x.len() >= 3 {
            v.push(x);
        }
    }
    for s in ["sk-ant", ".credentials", "oauth", "bearer ", "api_key", "apikey", "password", "c:\\\\users", "c:/users"] {
        v.push(s.to_string());
    }
    v.sort();
    v.dedup();
    v
}

fn setup_home(out: &Path, python: &str, script: &str) -> Result<PathBuf, String> {
    let home = out.join("home");
    let od = home.join(OWNER);
    std::fs::create_dir_all(&od).map_err(|e| e.to_string())?;
    // SAFETY of the real owner dir: everything below goes to the temporary data root only
    std::env::set_var("KORE_SELF_HOME", &home);
    // Minimal environment, found by testing on this machine (see docs/measured.md): the CLI found the owner's login with NO
    // variable beyond the ones the runner always passes (PATH, SystemRoot, TEMP, TMP). So env_allow is empty.
    let cfg = format!("# harness configuration of the B4b pilot (temporary data root; not the owner's file)\ncommand = {python}\narg = -I\narg = {script}\ntimeout_secs = 90\nmax_output_bytes = 65536\n");
    std::fs::write(od.join("advisor.txt"), cfg).map_err(|e| e.to_string())?;
    // the redactor of the owner dir also removes the real home path and user name, should a reply ever echo them
    let mut pats: Vec<String> = vec![];
    for k in ["USERPROFILE", "APPDATA", "LOCALAPPDATA", "USERNAME"] {
        if let Ok(x) = std::env::var(k) {
            if x.len() >= 3 {
                pats.push(x.clone());
                pats.push(x.replace('\\', "/"));
            }
        }
    }
    pats.sort();
    pats.dedup();
    std::fs::write(od.join("redact_patterns.txt"), format!("# harness: real home path and user name must never be logged\n{}\n", pats.join("\n"))).map_err(|e| e.to_string())?;
    Ok(home)
}

pub fn main_pilot(args: &[String]) {
    let out = PathBuf::from(arg(args, "out").unwrap_or_else(|| std::env::temp_dir().join("kore-llm-pilot").to_string_lossy().to_string()));
    if args.get(1).map(|s| s.as_str()) == Some("llm-pilot-report") {
        report(&out);
        return;
    }
    let (a, b) = match arg(args, "seeds").unwrap_or("1-5".into()).split_once('-') {
        Some((a, b)) => (a.parse::<u64>().unwrap_or(1), b.parse::<u64>().unwrap_or(5)),
        None => (1, 5),
    };
    let budget: i64 = arg(args, "budget").and_then(|x| x.parse().ok()).unwrap_or(300);
    let python = arg(args, "python").unwrap_or(DEFAULT_PYTHON.into());
    let script = arg(args, "script").unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join("llm_advisor").join("claude_advisor.py").to_string_lossy().to_string());
    std::fs::create_dir_all(&out).expect("out dir");
    setup_home(&out, &python, &script).expect("setup");
    let done: Vec<u64> = std::fs::read_to_string(out.join("worlds.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| v["seed"].as_u64()).collect();
    eprintln!("PILOT (n <= {} worlds, budget {budget}, one model, one run per world): results are indicative only.", b - a + 1);
    eprintln!("advisor: {MODEL_NOTE}; out dir {}", out.display());
    let spec = TrustSpec::default();
    for seed in a..=b {
        if done.contains(&seed) {
            eprintln!("seed {seed}: already in worlds.jsonl, skipped");
            continue;
        }
        let w = World::new(seed);
        let truth = truth_in(&w, &[]);
        let oracle_run = run_menu(seed, budget, &mut OracleMenu { truth: truth.clone(), strategies: STRATEGIES.to_vec() });
        let target = oracle_run.per_budget();
        let m = |r: &AdvRun| met(r, target, &spec, (budget / 8) as usize);
        let run = |mode: AdviceMode, adv: Option<&dyn Advisor>| run_adv(World::new(seed), budget, mode, &spec, adv, 0, false);
        let none = run(AdviceMode::Off, None);
        let s5 = sim_advisor(&truth, false, seed, 0.5);
        let s9 = sim_advisor(&truth, false, seed, 0.9);
        let sim5 = run(AdviceMode::Trust, Some(&s5));
        let sim9 = run(AdviceMode::Trust, Some(&s9));
        // the real advisor, one fresh CommandAdvisor (and so a fresh failure counter) per world, one call at a time
        let cfg = load_config(OWNER).expect("advisor.txt");
        let inner = CommandAdvisor::new(cfg, OWNER).expect("advisor");
        let sink = std::fs::OpenOptions::new().create(true).append(true).open(out.join("calls.jsonl")).expect("calls.jsonl");
        let rec = Recording { inner, ranker: truth_ranker(truth.clone(), STRATEGIES.to_vec()), seed, calls: RefCell::new(vec![]), sink: RefCell::new(sink), forbidden: forbidden_strings(), consecutive_fail: Cell::new(0) };
        eprintln!("seed {seed}: real advisor arm starts (one call per step, about {} steps)", none.run.recs.len());
        let t0 = Instant::now();
        let real = run(AdviceMode::Trust, Some(&rec));
        let wall = t0.elapsed().as_secs_f64();
        let calls = rec.calls.borrow().clone();
        let asked: Vec<&Call> = calls.iter().filter(|c| c.class != "not_asked").collect();
        let valid: Vec<&Call> = calls.iter().filter(|c| c.class == "valid").collect();
        let matches = valid.iter().filter(|c| c.advised == c.oracle).count();
        let same_slot = valid.iter().filter(|c| c.advised.map(|x| x.0) == c.oracle.map(|x| x.0)).count();
        let q_hat = if valid.is_empty() { None } else { Some(matches as f64 / valid.len() as f64) };
        // simulated advisor at the real advisor's measured q-hat (context), same world
        let simq = q_hat.map(|q| {
            let s = sim_advisor(&truth, false, seed, q);
            run(AdviceMode::Trust, Some(&s))
        });
        let mut classes = serde_json::Map::new();
        for c in &calls {
            let n = classes.get(c.class).and_then(|x| x.as_u64()).unwrap_or(0);
            classes.insert(c.class.to_string(), json!(n + 1));
        }
        let lat: Vec<f64> = asked.iter().map(|c| c.latency_s).collect();
        let vr: Vec<f64> = valid.iter().filter_map(|c| c.value_ratio).collect();
        let vr_mean: Option<f64> = if vr.is_empty() { None } else { Some(vr.iter().sum::<f64>() / vr.len() as f64) };
        let lat_max: Option<f64> = lat.iter().cloned().fold(None, |a: Option<f64>, x| Some(a.map_or(x, |y| y.max(x))));
        let line = json!({
            "seed": seed, "budget": budget, "steps": none.run.recs.len(), "wall_s": wall,
            "arms": {
                "none": met_json(&m(&none)), "sim_q0.5": met_json(&m(&sim5)), "sim_q0.9": met_json(&m(&sim9)),
                "sim_qhat": simq.as_ref().map(|r| met_json(&m(r))), "real": met_json(&m(&real)),
                "oracle": json!({"rpe": target, "energy": oracle_run.cost}),
            },
            "real_advisor": {
                "model": MODEL_NOTE, "calls_asked": asked.len(), "not_asked": calls.len() - asked.len(), "classes": classes,
                "valid_option": valid.len(), "matches_oracle": matches, "same_slot_as_oracle": same_slot, "q_hat": q_hat,
                "q_hat_all_calls": if asked.is_empty() { Value::Null } else { json!(matches as f64 / asked.len() as f64) },
                "value_ratio_mean": vr_mean,
                "latency_median_s": med(lat.clone()), "latency_max_s": lat_max,
                "stopped_by_failure_rule": calls.iter().any(|c| c.class == "not_asked"),
                "leak_hits": calls.iter().map(|c| c.leak_hits).sum::<usize>(), "redaction_markers": calls.iter().map(|c| c.redactions).sum::<usize>(),
                "trust_series": trust_series(&real), "final_trust_cells": final_cells(&real, spec.beta),
            },
        });
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(out.join("worlds.jsonl")).expect("worlds.jsonl");
        writeln!(f, "{line}").unwrap();
        eprintln!("seed {seed}: finished in {wall:.0}s; rpe none {:.3} real {:.3} oracle {target:.3}; q-hat {:?}; classes {}", none.run.per_budget(), real.run.per_budget(), q_hat, Value::Object(classes));
    }
    report(&out);
}

/// Two-sided 95% t quantile for small df (df = n - 1).
fn tq(df: usize) -> f64 {
    [0.0, 12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228][df.clamp(1, 10)]
}

fn ci(xs: &[f64]) -> (f64, f64, f64) {
    let n = xs.len();
    let m = xs.iter().sum::<f64>() / n.max(1) as f64;
    if n < 2 {
        return (m, f64::NAN, f64::NAN);
    }
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64;
    let h = tq(n - 1) * (var / n as f64).sqrt();
    (m, m - h, m + h)
}

fn wilson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (f64::NAN, f64::NAN);
    }
    let (p, z, n) = (k as f64 / n as f64, 1.96, n as f64);
    let d = 1.0 + z * z / n;
    let c = p + z * z / (2.0 * n);
    let h = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt();
    ((c - h) / d, (c + h) / d)
}

fn f(v: &Value) -> Option<f64> {
    v.as_f64()
}

pub fn report(out: &Path) {
    let rows: Vec<Value> = std::fs::read_to_string(out.join("worlds.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let n = rows.len();
    println!("\n### Track B4b pilot: real model as advisor ({n} worlds finished; budget {})\n", rows.first().map_or(0, |r| r["budget"].as_i64().unwrap_or(0)));
    println!("**PILOT. n = {n} worlds, one model, one run per world: nothing here can establish anything; intervals (t, df = n-1) are only indicative.**\n");
    if n == 0 {
        println!("no finished world in {}", out.join("worlds.jsonl").display());
        return;
    }
    let arm = |r: &Value, k: &str| f(&r["arms"][k]["rpe"]);
    println!("Reward per energy per world (trust learner in all advisor arms):\n");
    println!("| seed | steps | none | sim q=0.5 | sim q=0.9 | sim q=q-hat | REAL | oracle |");
    println!("|---|---|---|---|---|---|---|---|");
    let g = |x: Option<f64>| x.map_or("-".into(), |v| format!("{v:.3}"));
    for r in &rows {
        println!("| {} | {} | {} | {} | {} | {} | {} | {} |", r["seed"], r["steps"], g(arm(r, "none")), g(arm(r, "sim_q0.5")), g(arm(r, "sim_q0.9")), g(arm(r, "sim_qhat")), g(arm(r, "real")), g(arm(r, "oracle")));
    }
    println!("\nPaired differences against no advisor (same worlds; mean [indicative 95% t interval]; wins / losses / ties):\n");
    println!("| arm | mean difference | interval | w / l / t |");
    println!("|---|---|---|---|");
    for (k, name) in [("sim_q0.5", "simulated q = 0.5"), ("sim_q0.9", "simulated q = 0.9"), ("sim_qhat", "simulated q = q-hat of the real advisor"), ("real", "REAL advisor (trust learner)")] {
        let d: Vec<f64> = rows.iter().filter_map(|r| Some(arm(r, k)? - arm(r, "none")?)).collect();
        if d.is_empty() {
            continue;
        }
        let (m, lo, hi) = ci(&d);
        let w = d.iter().filter(|x| **x > 1e-9).count();
        let l = d.iter().filter(|x| **x < -1e-9).count();
        println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} / {l} / {} |", d.len() - w - l);
    }
    println!("\nThe real advisor, per world:\n");
    println!("| seed | calls asked | valid option | failures (by kind) | not asked (3-in-a-row rule) | q-hat (valid) | q-hat (all calls) | same slot as oracle | mean value / best value | followed (share of steps with advice) | tests (followed against model) / wins | final trust (pooled) | latency median / max (s) | wall (min) | leak hits |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let (mut tk, mut tn, mut tall, mut tslot, mut tvalid) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut alllat: Vec<f64> = vec![];
    let mut leaks = 0;
    for r in &rows {
        let a = &r["real_advisor"];
        let real = &r["arms"]["real"];
        let cl = a["classes"].as_object().map(|o| o.iter().filter(|(k, _)| !matches!(k.as_str(), "valid" | "not_asked")).map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let k = a["matches_oracle"].as_u64().unwrap_or(0) as usize;
        let nv = a["valid_option"].as_u64().unwrap_or(0) as usize;
        let na = a["calls_asked"].as_u64().unwrap_or(0) as usize;
        tk += k;
        tvalid += nv;
        tn += na;
        tall += na;
        tslot += a["same_slot_as_oracle"].as_u64().unwrap_or(0) as usize;
        leaks += a["leak_hits"].as_u64().unwrap_or(0);
        println!(
            "| {} | {na} | {nv} | {} | {} | {} | {} | {}/{nv} | {} | {}/{} | {}/{} | {} | {} / {} | {:.1} | {} |",
            r["seed"], if cl.is_empty() { "none".into() } else { cl }, a["not_asked"], g(f(&a["q_hat"])), g(f(&a["q_hat_all_calls"])), a["same_slot_as_oracle"], g(f(&a["value_ratio_mean"])),
            real["followed"], real["advice_steps"], real["tests"], real["test_wins"], g(f(&real["trust"])), g(f(&a["latency_median_s"])), g(f(&a["latency_max_s"])), r["wall_s"].as_f64().unwrap_or(0.0) / 60.0, a["leak_hits"]
        );
    }
    let _ = tall;
    for l in std::fs::read_to_string(out.join("calls.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        if l["class"] != "not_asked" {
            if let Some(x) = l["latency_s"].as_f64() {
                alllat.push(x);
            }
        }
    }
    let (lo, hi) = wilson(tk, tvalid);
    let (lo2, hi2) = wilson(tk, tn);
    println!("\nPooled over all calls (calls within a world are not independent; the interval is a Wilson interval treating them as such, so it is too narrow):");
    println!("- calls asked: {tn}; with a valid option: {tvalid}");
    println!("- q-hat (share of VALID advice equal to the oracle's best slot+strategy): {tk}/{tvalid} = {:.3} [{lo:.3}, {hi:.3}]", tk as f64 / tvalid.max(1) as f64);
    println!("- q-hat counting every asked call (failures as wrong): {tk}/{tn} = {:.3} [{lo2:.3}, {hi2:.3}]", tk as f64 / tn.max(1) as f64);
    println!("- same slot as oracle (any strategy): {tslot}/{tvalid} = {:.3}", tslot as f64 / tvalid.max(1) as f64);
    println!("- latency per call: median {} s, max {} s (n = {})", g(med(alllat.clone())), g(alllat.iter().cloned().fold(None, |a: Option<f64>, x| Some(a.map_or(x, |y| y.max(x))))), alllat.len());
    println!("- logged advice events (request as sent + reply, redacted) containing a forbidden string (home path, user name, credential words): {leaks}");
    println!("\nWhere q-hat sits: the simulated grid of B4 used q = 0.5, 0.9, 1.0; blind following broke even only at q of about 0.93 to 0.97 (docs/measured.md, Track B4).");
    println!("\nLearned trust over time (pooled running Beta(1,1) mean of followed-against-model tests, no discount after alarms; step: mean after tests):\n");
    println!("| seed | trajectory |");
    println!("|---|---|");
    for r in &rows {
        let s = r["real_advisor"]["trust_series"].as_array().map(|a| a.iter().map(|p| format!("{}: {:.2} ({} tests)", p["step"], p["mean"].as_f64().unwrap_or(f64::NAN), p["tests"])).collect::<Vec<_>>().join("; ")).unwrap_or_default();
        println!("| {} | {s} |", r["seed"]);
    }
    println!("\nFinal trust cells of the real advisor (per task domain; mean = posterior mean of the win rate, weight = optimistic mixing weight):\n");
    println!("| seed | domain | tests | wins | given | followed against model | mean | weight |");
    println!("|---|---|---|---|---|---|---|---|");
    for r in &rows {
        for c in r["real_advisor"]["final_trust_cells"].as_array().cloned().unwrap_or_default() {
            println!("| {} | {} | {} | {} | {} | {} | {:.3} | {:.3} |", r["seed"], c["kind"].as_str().unwrap_or(""), c["tests"], c["wins"], c["given"], c["followed_against_model"], c["mean"].as_f64().unwrap_or(f64::NAN), c["weight"].as_f64().unwrap_or(f64::NAN));
        }
    }
}
