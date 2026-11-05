//! Long-run harness for the being loop (synthetic world, author-defined rewards; see docs/longrun.md).
//!
//! The being runs through the REAL `kore-self` binary (stdio MCP, tool `being_run`), one process per
//! chunk, so every chunk is a cold restart that re-reads the log. Between chunks this harness opens the
//! same log read-mostly and checks it: hash chain (`verify`), consecutive steps, replay of logged
//! decisions from the log alone, `open_at` of earlier versions against what was recorded then, caps,
//! kill switch, audit rows; and it measures learning (reward per energy per window, regret against an
//! oracle that knows the hidden success rates, repeated mistakes, rule counts) and resources (log
//! bytes, files, open / verify / commit time, peak working set of each child process, CPU load).
//!
//!   cargo build -p kore-self --release --offline
//!   cargo run -p kore-self --release --offline --example longrun_being -- run --root <dir> --steps 1000000
//!   cargo run ... --example longrun_being -- torn --src <root> --root <scratch>
//!   cargo run ... --example longrun_being -- control --root <root> --steps 20000
//!   cargo run ... --example longrun_being -- authz --root <dir>
//!
//! Results go to `<root>/results.jsonl` (one JSON object per chunk / checkpoint / test) and a summary
//! in Markdown on stdout. Nothing is written outside `--root`. Run data is not committed.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::controller::Controller;
use being::measure::{run_menu_ext, truth_in, OracleMenu, Truth};
use being::planner::ModelChooser;
use being::run_loop::{offers_of, run_full, world_for, RunCfg};
use being::solver::{Strategy, STRATEGIES, STRATEGIES_EXT};
use being::state::{tag, BeingState, Chooser, SOURCE};
use being::world::{Domain, World};
use being::{Policy, ScriptedThinker};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;
use versioned::{AsOf, EventRow, LogFormat, NewEvent, VersionedLog};

const OWNER: &str = "longrun";
const PROBE_SOURCE: &str = "longrun_probe";

// ─── Windows process / system counters (no extra crates) ─────────────────────

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    pub struct Pmc {
        pub cb: u32,
        pub page_fault_count: u32,
        pub peak_working_set: usize,
        pub working_set: usize,
        pub qpp: usize,
        pub qp: usize,
        pub qpnp: usize,
        pub qnp: usize,
        pub pagefile: usize,
        pub peak_pagefile: usize,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct FileTime {
        lo: u32,
        hi: u32,
    }
    impl FileTime {
        pub fn ticks(&self) -> u64 {
            ((self.hi as u64) << 32) | self.lo as u64
        }
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn K32GetProcessMemoryInfo(h: *mut c_void, p: *mut Pmc, cb: u32) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
        fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        fn GetProcessTimes(h: *mut c_void, c: *mut FileTime, e: *mut FileTime, k: *mut FileTime, u: *mut FileTime) -> i32;
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
        fn GlobalMemoryStatusEx(m: *mut MemStatus) -> i32;
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct MemStatus {
        pub len: u32,
        pub load: u32,
        pub total_phys: u64,
        pub avail_phys: u64,
        pub total_pf: u64,
        pub avail_pf: u64,
        pub total_virt: u64,
        pub avail_virt: u64,
        pub avail_ext: u64,
    }
    /// (total, available) physical memory in bytes.
    pub fn memory() -> (u64, u64) {
        let mut m = MemStatus { len: std::mem::size_of::<MemStatus>() as u32, ..Default::default() };
        unsafe { GlobalMemoryStatusEx(&mut m) };
        (m.total_phys, m.avail_phys)
    }
    /// Peak working set in bytes of a process handle (works after the process exited while the handle is open).
    pub fn peak_ws(h: *mut c_void) -> u64 {
        let mut p = Pmc { cb: std::mem::size_of::<Pmc>() as u32, ..Default::default() };
        if unsafe { K32GetProcessMemoryInfo(h, &mut p, p.cb) } != 0 {
            p.peak_working_set as u64
        } else {
            0
        }
    }
    pub fn self_peak_ws() -> u64 {
        peak_ws(unsafe { GetCurrentProcess() })
    }
    /// (idle, kernel+user) in 100 ns ticks, all CPUs.
    pub fn system_times() -> (u64, u64) {
        let (mut i, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default());
        unsafe { GetSystemTimes(&mut i, &mut k, &mut u) };
        (i.ticks(), k.ticks() + u.ticks())
    }
    /// CPU seconds (kernel + user) of a process handle.
    pub fn cpu_secs(h: *mut c_void) -> f64 {
        let (mut c, mut e, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
        unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) };
        (k.ticks() + u.ticks()) as f64 / 1e7
    }
    pub fn free_bytes(dir: &std::path::Path) -> u64 {
        use std::os::windows::ffi::OsStrExt;
        let w: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
        unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) };
        a
    }
}
#[cfg(not(windows))]
mod win {
    pub fn self_peak_ws() -> u64 {
        0
    }
    pub fn system_times() -> (u64, u64) {
        (0, 0)
    }
    pub fn free_bytes(_: &std::path::Path) -> u64 {
        u64::MAX
    }
    pub fn memory() -> (u64, u64) {
        (0, 0)
    }
}

fn child_peak_ws(c: &Child) -> u64 {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        return win::peak_ws(c.as_raw_handle() as *mut _);
    }
    #[allow(unreachable_code)]
    {
        let _ = c;
        0
    }
}
fn child_cpu(c: &Child) -> f64 {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        return win::cpu_secs(c.as_raw_handle() as *mut _);
    }
    #[allow(unreachable_code)]
    {
        let _ = c;
        0.0
    }
}

/// Share of CPU time busy over all cores between two `system_times` readings.
fn busy_share(a: (u64, u64), b: (u64, u64)) -> f64 {
    let (idle, total) = (b.0.saturating_sub(a.0), b.1.saturating_sub(a.1));
    if total == 0 {
        0.0
    } else {
        1.0 - idle as f64 / total as f64
    }
}

// ─── The real binary over stdio MCP ──────────────────────────────────────────

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    out: BufReader<ChildStdout>,
    next_id: u64,
}

fn binary() -> PathBuf {
    if let Ok(p) = std::env::var("LONGRUN_BIN") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().expect("current exe");
    let dir = exe.parent().and_then(|p| p.parent()).expect("target dir");
    dir.join(if cfg!(windows) { "kore-self.exe" } else { "kore-self" })
}

impl Server {
    fn start(root: &Path) -> Server {
        let fake = root.join("fakehome");
        let _ = std::fs::create_dir_all(&fake);
        let mut child = Command::new(binary())
            .arg(OWNER)
            .env("KORE_SELF_HOME", root)
            .env("HOME", &fake)
            .env("USERPROFILE", &fake)
            .env_remove("KORE_SELF_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn kore-self (build it first: cargo build -p kore-self --release --offline)");
        let stdin = child.stdin.take();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server { child, stdin, out, next_id: 1 };
        s.rpc("initialize", json!({}));
        s
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin.as_mut().unwrap(), "{line}").unwrap();
        self.stdin.as_mut().unwrap().flush().unwrap();
        let mut resp = String::new();
        self.out.read_line(&mut resp).unwrap();
        serde_json::from_str(&resp).unwrap_or_else(|e| panic!("bad response {resp:?}: {e}"))
    }
    /// (is_error, text)
    fn call(&mut self, tool: &str, args: Value) -> (bool, String) {
        let v = self.rpc("tools/call", json!({"name": tool, "arguments": args}));
        let r = &v["result"];
        (r["isError"].as_bool().unwrap_or(false), r["content"][0]["text"].as_str().unwrap_or("").to_string())
    }
    /// Close stdin, wait; returns (peak working set bytes, CPU seconds).
    fn stop(mut self) -> (u64, f64) {
        drop(self.stdin.take());
        let _ = self.child.wait();
        (child_peak_ws(&self.child), child_cpu(&self.child))
    }
}

// ─── Settings ────────────────────────────────────────────────────────────────

/// controller.txt of the long run: the documented safe defaults, with composition and pruning allowed.
const CONTROLLER: &str = "# long-run harness (written by the harness into a temp data root)
max_live_rules        = 16
max_building_blocks   = 8
max_search_candidates = 2000
max_steps_per_run     = 5000
max_energy_per_run    = 100000
composition           = on
pruning               = on
";

#[derive(Clone)]
struct Opts {
    root: PathBuf,
    steps: u64,
    chunk: u64,
    calls_per_proc: u64,
    checkpoint_every: u64,
    window: u64,
    format: u8,
    world: u64,
    budget: u64,
    kill_at: Option<u64>,
    min_free_gb: f64,
    replay_n: usize,
    max_hours: f64,
    heavy_every: u64,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut o = Opts { root: PathBuf::new(), steps: 20_000, chunk: 5000, calls_per_proc: 2, checkpoint_every: 10_000, window: 10_000, format: 3, world: 1, budget: 3000, kill_at: None, min_free_gb: 12.0, replay_n: 300, max_hours: 1e9, heavy_every: 5 };
    let mut i = 0;
    while i < args.len() {
        let v = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--root" => o.root = PathBuf::from(&v),
            "--steps" => o.steps = v.parse().unwrap(),
            "--chunk" => o.chunk = v.parse().unwrap(),
            "--calls-per-proc" => o.calls_per_proc = v.parse().unwrap(),
            "--checkpoint-every" => o.checkpoint_every = v.parse().unwrap(),
            "--window" => o.window = v.parse().unwrap(),
            "--format" => o.format = v.parse().unwrap(),
            "--world" => o.world = v.parse().unwrap(),
            "--budget" => o.budget = v.parse().unwrap(),
            "--kill-at" => o.kill_at = Some(v.parse().unwrap()),
            "--min-free-gb" => o.min_free_gb = v.parse().unwrap(),
            "--replay-n" => o.replay_n = v.parse().unwrap(),
            "--max-hours" => o.max_hours = v.parse().unwrap(),
            "--heavy-every" => o.heavy_every = v.parse().unwrap(),
            other => panic!("unknown option {other}"),
        }
        i += 2;
    }
    assert!(!o.root.as_os_str().is_empty(), "--root is required");
    o
}

fn run_args(o: &Opts, steps: u64) -> Value {
    json!({
        "steps": steps, "stop_after_budget": false, "budget": o.budget, "chooser": "model", "pattern_families": "extended2",
        "discover": true, "discover_every": 10, "compose": true, "prune": true, "world_seed": o.world, "seed": 1,
    })
}

/// The same being in-process (control runs): cfg and limits as the tool builds them from `run_args`.
fn cfg_in_process(o: &Opts, steps: u64) -> (RunCfg, Policy) {
    let ctl = Controller::parse(CONTROLLER).expect("controller text");
    let (limits, _) = ctl.limits(true, true);
    let cfg = RunCfg {
        ctl: Some(limits),
        ext2: Some(true),
        extended: Some(true),
        discover_every: Some(10),
        chooser: Some(Chooser::Model),
        world_seed: Some(o.world),
        loop_seed: Some(1),
        budget: Some(o.budget as i64),
        steps,
        refill_when_exhausted: true,
        ..Default::default()
    };
    let mut p = Policy::new(steps);
    p.refill_when_exhausted = true;
    (cfg, p)
}

// ─── Results file ────────────────────────────────────────────────────────────

struct Out {
    f: std::fs::File,
}
impl Out {
    fn new(root: &Path, name: &str) -> Out {
        Out { f: std::fs::OpenOptions::new().create(true).append(true).open(root.join(name)).unwrap() }
    }
    fn put(&mut self, v: Value) {
        writeln!(self.f, "{v}").unwrap();
        self.f.flush().unwrap();
    }
}

fn dir_stats(dir: &Path) -> (u64, u64) {
    let (mut bytes, mut files) = (0u64, 0u64);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if let Ok(m) = e.metadata() {
                    if m.is_dir() {
                        stack.push(p);
                    } else {
                        bytes += m.len();
                        files += 1;
                    }
                }
            }
        }
    }
    (bytes, files)
}

fn gb(b: u64) -> f64 {
    b as f64 / 1e9
}

// ─── Analyzer: everything computed from the log ──────────────────────────────

#[derive(Default, Clone, Debug)]
struct Win {
    steps: u64,
    attempts: u64,
    skips: u64,
    correct: u64,
    reward: i64,
    cost: i64,
    strat: BTreeMap<String, u64>,
    rule_answers: u64,
    rule_wrong: u64,
    rule_wrong_repeat: u64,
    /// attempts on a (domain, difficulty, strategy) whose own logged record before the attempt was >= 10 tries and <= 10% right
    known_bad: u64,
    known_bad_wrong: u64,
    /// attempts whose hidden success probability (measurement only) is <= 0.10
    truly_bad: u64,
    wrong: u64,
    rules_accepted: u64,
    rules_retired: u64,
    refusals: u64,
}

struct Analyzer {
    st: BeingState,
    world: Option<World>,
    last_version: u64,
    events_seen: u64,
    next_step: u64,
    step_errors: Vec<String>,
    cells: HashMap<(String, String, String), (u32, u32)>,
    rules_ever_wrong: HashSet<String>,
    windows: BTreeMap<u64, Win>,
    window: u64,
    live_max: usize,
    live_cap: usize,
    cap_violations: Vec<String>,
    blocks_max: u32,
    refusals_by_cap: BTreeMap<String, u64>,
    stops: Vec<(u64, String)>,
    sources: BTreeMap<String, u64>,
    audit_rows: Vec<String>,
    replay_checked: u64,
    replay_mismatch: Vec<String>,
    truth_plain: Truth,
}

fn key3(t: &str) -> Option<(String, String, String)> {
    Some((tag(t, "domain")?.to_string(), tag(t, "difficulty")?.to_string(), tag(t, "strategy")?.to_string()))
}

impl Analyzer {
    fn new(window: u64, world_seed: u64) -> Analyzer {
        let w = World::with_ext2(world_seed, None);
        let truth_plain = truth_in(&w, &[]);
        Analyzer {
            st: BeingState::default(),
            world: None,
            last_version: 0,
            events_seen: 0,
            next_step: 0,
            step_errors: vec![],
            cells: HashMap::new(),
            rules_ever_wrong: HashSet::new(),
            windows: BTreeMap::new(),
            window,
            live_max: 0,
            live_cap: 16,
            cap_violations: vec![],
            blocks_max: 0,
            refusals_by_cap: BTreeMap::new(),
            stops: vec![],
            sources: BTreeMap::new(),
            audit_rows: vec![],
            replay_checked: 0,
            replay_mismatch: vec![],
            truth_plain,
        }
    }

    fn win(&mut self, step: u64) -> &mut Win {
        let w = self.window;
        self.windows.entry(step / w).or_default()
    }

    /// Re-derive the decision the being would take now from the folded log state alone.
    fn rederive(&mut self) -> Option<String> {
        let g = self.st.genesis.clone()?;
        if self.world.is_none() {
            self.world = Some(world_for(&g));
        }
        let offers = offers_of(&self.world.as_ref().unwrap().offers_at(self.st.step, self.st.clock));
        let mut mc = ModelChooser::for_genesis(&g);
        use being::planner::MenuChooser;
        mc.choose(&self.st, &offers).map(|c| c.action.label())
    }

    /// Feed events (log order). `replay_budget`: decisions to re-derive in this batch.
    fn feed(&mut self, rows: &[EventRow], mut replay_budget: usize) {
        for e in rows {
            self.events_seen += 1;
            *self.sources.entry(e.source.clone()).or_default() += 1;
            if e.source == "authorize" {
                self.audit_rows.push(e.text.clone());
                continue;
            }
            if e.source != SOURCE {
                continue;
            }
            let t = &e.tags;
            if e.kind == "decision" && replay_budget > 0 {
                if let (Some(rec), Some(step)) = (tag(t, "action"), tag(t, "step")) {
                    let rec = rec.to_string();
                    let got = self.rederive();
                    self.replay_checked += 1;
                    replay_budget -= 1;
                    if got.as_deref() != Some(rec.as_str()) && self.replay_mismatch.len() < 20 {
                        self.replay_mismatch.push(format!("step {step}: logged {rec}, re-derived {got:?}"));
                    }
                }
            }
            if e.kind == "outcome" {
                let step: u64 = tag(t, "step").and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
                if step != self.next_step && self.step_errors.len() < 20 {
                    self.step_errors.push(format!("expected outcome of step {}, found {step}", self.next_step));
                }
                self.next_step = step.wrapping_add(1);
                let result = tag(t, "result").unwrap_or("");
                let reward: i64 = tag(t, "reward").and_then(|s| s.parse().ok()).unwrap_or(0);
                let cost: i64 = tag(t, "cost").and_then(|s| s.parse().ok()).unwrap_or(0);
                let attempted = result == "correct" || result == "wrong";
                let k = key3(t);
                let rule = tag(t, "rule_used").map(|s| s.to_string());
                let (prior, truly_bad) = match &k {
                    Some(k) if attempted => {
                        let p = self.cells.get(k).copied().unwrap_or((0, 0));
                        let tb = Domain::parse(&k.0).zip(k.1.parse::<u8>().ok()).zip(Strategy::parse(&k.2)).map_or(false, |((d, dk), s)| s != Strategy::Discovered && self.truth_plain.p[d.idx()][dk as usize - 1][s.idx()] <= 0.10);
                        (Some(p), tb)
                    }
                    _ => (None, false),
                };
                let ever_wrong = rule.as_ref().map_or(false, |r| self.rules_ever_wrong.contains(r));
                let w = self.win(step);
                w.steps += 1;
                w.reward += reward;
                w.cost += cost;
                if attempted {
                    w.attempts += 1;
                    if result == "correct" {
                        w.correct += 1;
                    } else {
                        w.wrong += 1;
                    }
                    if let Some(k) = &k {
                        *w.strat.entry(k.2.clone()).or_default() += 1;
                    }
                    if let Some((n, s)) = prior {
                        if n >= 10 && (s as f64) <= 0.10 * n as f64 {
                            w.known_bad += 1;
                            if result == "wrong" {
                                w.known_bad_wrong += 1;
                            }
                        }
                    }
                    if truly_bad {
                        w.truly_bad += 1;
                    }
                    if rule.is_some() {
                        w.rule_answers += 1;
                        if result == "wrong" {
                            w.rule_wrong += 1;
                            if ever_wrong {
                                w.rule_wrong_repeat += 1;
                            }
                        }
                    }
                } else {
                    w.skips += 1;
                }
                if attempted {
                    if let Some(k) = k {
                        let c = self.cells.entry(k).or_insert((0, 0));
                        c.0 += 1;
                        c.1 += (result == "correct") as u32;
                    }
                    if let (Some(r), "wrong") = (rule, result) {
                        self.rules_ever_wrong.insert(r);
                    }
                }
            }
            if e.kind == "note" && t.split(',').any(|x| x == "controller_refusal") {
                *self.refusals_by_cap.entry(tag(t, "cap").unwrap_or("?").to_string()).or_default() += 1;
                let s = self.st.step;
                self.win(s).refusals += 1;
            }
            if e.kind == "note" && t.split(',').any(|x| x == "controller_stop") {
                self.stops.push((tag(t, "step").and_then(|s| s.parse().ok()).unwrap_or(0), tag(t, "stop").unwrap_or("").to_string()));
            }
            let is_rule = e.kind == "note" && t.split(',').any(|x| x == "rule");
            let is_retire = e.kind == "note" && t.split(',').any(|x| x == "rule_retired");
            if is_rule {
                let b: u32 = tag(t, "blocks").and_then(|s| s.parse().ok()).unwrap_or(0);
                self.blocks_max = self.blocks_max.max(b);
                let s = self.st.step;
                self.win(s).rules_accepted += 1;
            }
            if is_retire {
                let s = self.st.step;
                self.win(s).rules_retired += 1;
            }
            self.st.fold_event(e);
            if is_rule || is_retire {
                let live = self.st.live_rules();
                self.live_max = self.live_max.max(live);
                if live > self.live_cap && self.cap_violations.len() < 20 {
                    self.cap_violations.push(format!("{live} live rules at step {} (cap {})", self.st.step, self.live_cap));
                }
            }
            if self.blocks_max > 8 && self.cap_violations.len() < 20 {
                self.cap_violations.push(format!("composed rule used {} blocks (cap 8)", self.blocks_max));
            }
        }
    }
}

/// Oracle that knows the hidden success rate of every strategy (given a rule book) and the real
/// energy; it takes the best (offer, strategy) by expected reward per energy and skips the menu when
/// that value is below `lambda`. Measurement only.
struct OracleSkip {
    truth: Truth,
    strategies: Vec<Strategy>,
    lambda: f64,
}
impl being::planner::MenuChooser for OracleSkip {
    fn name(&self) -> String {
        "oracle-skip".into()
    }
    fn choose(&mut self, st: &BeingState, offers: &[being::planner::Offer]) -> Option<being::planner::Choice> {
        let mut o = OracleMenu { truth: Truth { p: self.truth.p }, strategies: self.strategies.clone() };
        let c = o.choose(st, offers);
        match c {
            Some(c) if c.score < self.lambda && st.budget_left >= being::world::SKIP_COST => Some(being::planner::Choice { action: being::planner::Action::Skip, policy: "oracle-skip".into(), why: "below lambda".into(), p: 0.0, sd: 0.0, cost_est: 0.0, score: c.score, rationale: json!({}), advice: None }),
            None if st.budget_left >= being::world::SKIP_COST => None,
            c => c,
        }
    }
}

/// Oracle reward per energy in the same world for a rule book: (never skips, best skip threshold, that threshold).
/// The skip threshold is the best of a grid 0, 0.25, .., 8 (same budget, same world).
fn oracle_rate(world_seed: u64, rules: &[being::discover::Rule], budget: i64) -> (f64, f64, f64) {
    use being::planner::MenuChooser;
    let w = World::with_ext2(world_seed, None);
    let truth = truth_in(&w, rules);
    let strategies: Vec<Strategy> = if rules.is_empty() { STRATEGIES.to_vec() } else { STRATEGIES_EXT.to_vec() };
    let mut o = OracleMenu { truth: Truth { p: truth.p }, strategies: strategies.clone() };
    let plain = run_menu_ext(World::with_ext2(world_seed, None), budget, &mut o, 0, rules).run.per_budget();
    let _ = o.name();
    let (mut best, mut best_l) = (plain, 0.0);
    for i in 1..=32 {
        let l = i as f64 * 0.25;
        let mut s = OracleSkip { truth: Truth { p: truth.p }, strategies: strategies.clone(), lambda: l };
        let r = run_menu_ext(World::with_ext2(world_seed, None), budget, &mut s, 0, rules).run.per_budget();
        if r > best {
            best = r;
            best_l = l;
        }
    }
    (plain, best, best_l)
}

// ─── Checkpoints ─────────────────────────────────────────────────────────────

struct Recorded {
    v: u64,
    steps: u64,
    total_reward: i64,
    rules: usize,
    events: usize,
    content_hash: String,
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// One checkpoint: verify, incremental fold + checks, open_at re-checks, probe commits, sizes.
fn checkpoint(o: &Opts, an: &mut Analyzer, recorded: &mut Vec<Recorded>, oracle_cache: &mut HashMap<String, (f64, f64, f64)>, out: &mut Out, label: &str, heavy: bool) -> Value {
    let t = Instant::now();
    let mut log = VersionedLog::open(&o.root, OWNER).expect("open log");
    let open_ms = ms(t);
    let t = Instant::now();
    let verify = log.verify();
    let verify_ms = ms(t);
    let latest = log.latest_version();
    let t = Instant::now();
    let d = log.diff(an.last_version, latest).expect("diff");
    let diff_ms = ms(t);
    let t = Instant::now();
    an.feed(&d.events, o.replay_n);
    drop(d);
    let feed_ms = ms(t);
    an.last_version = latest;
    // heavy: the full latest snapshot (what a restart folds), a re-fold, and its content hash for later open_at checks
    let (mut decode_ms, mut fold_ms, mut consistent, mut ch) = (0.0, 0.0, Value::Null, String::new());
    if heavy {
        let t = Instant::now();
        let snap = log.open_latest();
        let rows: Vec<EventRow> = snap.rows();
        decode_ms = ms(t);
        let t = Instant::now();
        let refold = BeingState::from_events(&rows);
        fold_ms = ms(t);
        consistent = json!(refold.step == an.st.step && refold.total_reward == an.st.total_reward && refold.rules.len() == an.st.rules.len() && refold.live_rules() == an.st.live_rules());
        ch = snap.content_hash();
    }
    // open_at of earlier checkpoints must give what was recorded then (first, previous, one rotating)
    let mut open_at = vec![];
    let n = recorded.len();
    let mut picks: Vec<usize> = vec![];
    if n > 0 {
        picks.push(0);
        if heavy {
            picks.push(n - 1);
            picks.push((n * 7 + 3) % n);
        }
    }
    picks.sort_unstable();
    picks.dedup();
    for i in picks {
        let r = &recorded[i];
        if r.content_hash.is_empty() {
            continue;
        }
        let t = Instant::now();
        let s = log.open_at(AsOf::Version(r.v)).expect("open_at");
        let rows: Vec<EventRow> = s.rows();
        let st = BeingState::from_events(&rows);
        let same = s.content_hash() == r.content_hash && st.step == r.steps && st.total_reward == r.total_reward && st.rules.len() == r.rules && rows.len() == r.events;
        open_at.push(json!({ "v": r.v, "same": same, "ms": ms(t) }));
    }
    recorded.push(Recorded { v: latest, steps: an.st.step, total_reward: an.st.total_reward, rules: an.st.rules.len(), events: an.events_seen as usize, content_hash: ch });
    // commit time at this log size: three one-event probe commits (source longrun_probe, ignored by the being)
    let mut probe = vec![];
    for i in 0..3 {
        let t = Instant::now();
        log.append_events(&[NewEvent { source: PROBE_SOURCE.into(), occurred_at: versioned::now_iso(), kind: "note".into(), text: format!("long-run probe commit {} {i} at step {}", recorded.len(), an.st.step), tags: vec!["probe".into()], redacted: false }]).expect("probe commit");
        probe.push(ms(t));
    }
    probe.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // the probe events go through the analyzer too (so the next diff starts after them)
    let latest2 = log.latest_version();
    let d2 = log.diff(an.last_version, latest2).expect("diff");
    an.feed(&d2.events, 0);
    an.last_version = latest2;
    drop(log);
    let (bytes, files) = dir_stats(&o.root.join(OWNER).join("log"));
    let live: Vec<being::discover::Rule> = an.st.rules.iter().filter(|r| !r.retired).cloned().collect();
    let key = live.iter().map(|r| r.id.clone()).collect::<Vec<_>>().join(",");
    let orc = *oracle_cache.entry(key).or_insert_with(|| oracle_rate(o.world, &live, 30_000));
    let v = json!({
        "type": "checkpoint", "label": label, "steps": an.st.step, "version": latest2, "events": an.events_seen,
        "verify_ok": verify.is_ok(), "verify_err": verify.err().map(|e| e.to_string()), "verify_ms": verify_ms,
        "open_ms": open_ms, "diff_ms": diff_ms, "feed_ms": feed_ms, "decode_latest_ms": decode_ms, "fold_ms": fold_ms,
        "fold_consistent": consistent, "open_at": open_at, "probe_commit_ms_median": probe[1], "probe_commit_ms": probe,
        "log_bytes": bytes, "log_files": files, "bytes_per_event": bytes as f64 / an.events_seen.max(1) as f64, "bytes_per_step": bytes as f64 / an.st.step.max(1) as f64,
        "rules_total": an.st.rules.len(), "rules_live": an.st.live_rules(), "live_max": an.live_max, "examples": an.st.examples.len(),
        "oracle_never_skip_live_rules": orc.0, "oracle_skip_live_rules": orc.1, "oracle_lambda": orc.2, "heavy": heavy,
        "mem_total_gb": gb(win::memory().0), "mem_avail_gb": gb(win::memory().1),
        "total_reward_per_energy": if an.st.total_cost > 0 { an.st.total_reward as f64 / an.st.total_cost as f64 } else { 0.0 },
        "replay_checked": an.replay_checked, "replay_mismatch": an.replay_mismatch.len(), "step_errors": an.step_errors.len(), "cap_violations": an.cap_violations.len(),
        "free_gb": gb(win::free_bytes(&o.root)), "harness_peak_ws_mb": win::self_peak_ws() as f64 / 1e6,
    });
    out.put(v.clone());
    v
}

/// A heavy checkpoint decodes the whole latest snapshot in this process (about the child's peak again).
fn mem_ok(last_peak: u64) -> bool {
    let (t, a) = win::memory();
    t == 0 || (a as f64) > 1.5 * last_peak as f64 + 3e9
}

// ─── run ─────────────────────────────────────────────────────────────────────

fn prepare(o: &Opts) {
    std::fs::create_dir_all(o.root.join(OWNER)).unwrap();
    std::fs::write(o.root.join(OWNER).join("controller.txt"), CONTROLLER).unwrap();
    let fmt = match o.format {
        1 => LogFormat::V1,
        2 => LogFormat::V2,
        _ => LogFormat::V3,
    };
    if !o.root.join(OWNER).join("log").join("MANIFEST").exists() {
        let log = VersionedLog::open_or_create_with_format(&o.root, OWNER, versioned::now_iso, fmt).expect("create log");
        assert_eq!(log.format(), fmt);
    }
}

fn cmd_run(o: Opts) {
    std::fs::create_dir_all(&o.root).unwrap();
    let o = Opts { root: std::fs::canonicalize(&o.root).unwrap(), ..o };
    prepare(&o);
    let mut out = Out::new(&o.root, "results.jsonl");
    let started = Instant::now();
    out.put(json!({ "type": "start", "opts": format!("{:?}", (o.steps, o.chunk, o.calls_per_proc, o.checkpoint_every, o.window, o.format, o.world, o.budget, o.kill_at)), "binary": binary().display().to_string(), "free_gb": gb(win::free_bytes(&o.root)), "date": versioned::now_iso() }));
    let mut an = Analyzer::new(o.window, o.world);
    let oracle_plain = oracle_rate(o.world, &[], 30_000);
    out.put(json!({ "type": "oracle", "rules": 0, "never_skip": oracle_plain.0, "best_skip": oracle_plain.1, "lambda": oracle_plain.2 }));
    let mut recorded: Vec<Recorded> = vec![];
    let mut oracle_cache: HashMap<String, (f64, f64, f64)> = HashMap::new();
    let mut n_cp = 0u64;
    // resume: --steps and --kill-at count the being's total steps
    let mut steps_done = BeingState::from_events(&VersionedLog::open(&o.root, OWNER).expect("open log").open_latest().rows::<EventRow>()).step;
    let mut kill_done = o.kill_at.map_or(true, |k| k < steps_done);
    let mut next_cp = (steps_done / o.checkpoint_every + 1) * o.checkpoint_every;
    let mut proc_i = 0u64;
    let mut call_times: Vec<(u64, f64)> = vec![];
    let mut stop_reason = "steps reached".to_string();
    let mut cps: Vec<Value> = vec![];
    let ctl = Controller::parse(CONTROLLER).unwrap();
    let mut violations: Vec<String> = vec![];
    let mut last_peak = 0u64;
    out.put(json!({ "type": "machine", "mem_total_gb": gb(win::memory().0), "mem_avail_gb": gb(win::memory().1), "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0) }));
    while steps_done < o.steps {
        // below the disk floor nothing runs: pause (no being process) until the space is back
        let mut free = gb(win::free_bytes(&o.root));
        if free < o.min_free_gb {
            let tp = Instant::now();
            out.put(json!({ "type": "pause", "steps_done": steps_done, "free_gb": free, "date": versioned::now_iso() }));
            eprintln!("[{:.2} h] paused at {steps_done} steps: free disk {free:.1} GB", started.elapsed().as_secs_f64() / 3600.0);
            while free < o.min_free_gb + 0.5 && started.elapsed().as_secs_f64() / 3600.0 <= o.max_hours {
                std::thread::sleep(std::time::Duration::from_secs(60));
                free = gb(win::free_bytes(&o.root));
            }
            out.put(json!({ "type": "resume", "steps_done": steps_done, "free_gb": free, "paused_secs": tp.elapsed().as_secs_f64(), "date": versioned::now_iso() }));
            if free < o.min_free_gb + 0.5 {
                stop_reason = format!("free disk {free:.1} GB below {} GB until the time limit", o.min_free_gb);
                break;
            }
        }
        let (mt, ma) = win::memory();
        // the next restart needs a little more than the last one; keep 2 GB for the rest of the machine
        if mt > 0 && (ma as f64) < 1.3 * last_peak as f64 + 2e9 {
            stop_reason = format!("available memory {:.1} GB below 1.3 x the last child peak ({:.1} GB) + 2 GB", gb(ma), gb(last_peak));
            break;
        }
        if mt > 0 && last_peak as f64 > 0.5 * mt as f64 {
            stop_reason = format!("child peak working set {:.1} GB above half of the {:.1} GB RAM", gb(last_peak), gb(mt));
            break;
        }
        if started.elapsed().as_secs_f64() / 3600.0 > o.max_hours {
            stop_reason = format!("time limit {} h", o.max_hours);
            break;
        }
        let sys0 = win::system_times();
        let tp = Instant::now();
        let mut srv = Server::start(&o.root);
        let start_ms = ms(tp);
        let mut calls = vec![];
        for _ in 0..o.calls_per_proc {
            if steps_done >= o.steps {
                break;
            }
            let want = o.chunk.min(o.steps - steps_done);
            let kill_now = !kill_done && steps_done + want > o.kill_at.unwrap();
            let kill_thread = if kill_now {
                // create the KILL file while the call runs: after about half the last call's duration
                let delay = call_times.last().map_or(1.0, |c| c.1 * 0.5);
                let p = o.root.join(OWNER).join("KILL");
                Some(std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs_f64(delay));
                    std::fs::write(&p, "stop\n").unwrap();
                    Instant::now()
                }))
            } else {
                None
            };
            let tc = Instant::now();
            let (is_err, text) = srv.call("being_run", run_args(&o, want));
            let done_at = Instant::now();
            let secs = tc.elapsed().as_secs_f64();
            if is_err {
                out.put(json!({ "type": "error", "steps_done": steps_done, "text": text }));
                stop_reason = format!("being_run failed: {text}");
                break;
            }
            let r: Value = serde_json::from_str(&text).unwrap();
            let ran = r["steps_run"].as_u64().unwrap();
            steps_done += ran;
            call_times.push((ran, secs));
            // caps from the tool's own answer
            if ran > ctl.max_steps_per_run {
                violations.push(format!("steps_run {ran} > max_steps_per_run"));
            }
            let energy = r["energy_spent_this_run"].as_i64().unwrap_or(0);
            if energy > ctl.max_energy_per_run + 20 {
                violations.push(format!("energy {energy} > cap + one step"));
            }
            if r["rules_live"].as_u64().unwrap_or(0) as usize > ctl.max_live_rules {
                violations.push(format!("rules_live {} > cap", r["rules_live"]));
            }
            let audited = r["controller"]["refusals_audited"].as_array().map_or(0, |a| a.len());
            let refused = r["controller_refusals_this_run"].as_array().map_or(0, |a| a.len());
            let mut cv = json!({ "type": "call", "proc": proc_i, "steps_run": ran, "steps_total": r["total_steps"], "stopped": r["stopped"], "secs": secs, "ms_per_step": secs * 1000.0 / ran.max(1) as f64, "energy": energy, "reward": r["reward_this_run"], "rules_live": r["rules_live"], "rules_total": r["rules_total"], "refusals": refused, "audited": audited, "retired": r["rules_retired_this_run"].as_array().map_or(0, |a| a.len()) });
            if let Some(th) = kill_thread {
                let created = th.join().unwrap();
                let lag_ms = done_at.saturating_duration_since(created).as_secs_f64() * 1000.0;
                let stopped_by_kill = r["stopped"] == "kill switch";
                // a second call while the file exists must be refused before any step
                let (e2, t2) = srv.call("being_run", run_args(&o, 10));
                let _ = std::fs::remove_file(o.root.join(OWNER).join("KILL"));
                let kv = json!({ "type": "kill_test", "at_steps_total": r["total_steps"], "steps_run_in_killed_call": ran, "requested": want, "stopped_by_kill": stopped_by_kill, "ms_from_file_to_answer": lag_ms, "ms_per_step_before": call_times.iter().rev().nth(1).map(|c| c.1 * 1000.0 / c.0.max(1) as f64), "refused_while_present": e2, "refusal_text": t2.chars().take(200).collect::<String>() });
                out.put(kv.clone());
                cv["kill_test"] = kv;
                kill_done = true;
            }
            out.put(cv.clone());
            calls.push(cv);
            if ran == 0 && r["stopped"] != "kill switch" {
                stop_reason = "a call ran 0 steps".into();
                break;
            }
        }
        let (peak, cpu) = srv.stop();
        last_peak = peak;
        let sys1 = win::system_times();
        out.put(json!({ "type": "proc", "proc": proc_i, "start_ms": start_ms, "peak_ws_mb": peak as f64 / 1e6, "cpu_secs": cpu, "wall_secs": tp.elapsed().as_secs_f64(), "machine_busy_share": busy_share(sys0, sys1), "steps_done": steps_done }));
        proc_i += 1;
        if stop_reason != "steps reached" && !stop_reason.is_empty() && stop_reason != "steps reached" {
            break;
        }
        if steps_done >= next_cp || steps_done >= o.steps {
            n_cp += 1;
            let cp = checkpoint(&o, &mut an, &mut recorded, &mut oracle_cache, &mut out, "periodic", (n_cp % o.heavy_every == 1 || o.heavy_every == 1) && mem_ok(last_peak));
            eprintln!("[{:.2} h] checkpoint at {} steps: verify {} rpe {:.3} free {:.1} GB log {:.1} MB", started.elapsed().as_secs_f64() / 3600.0, cp["steps"], cp["verify_ok"], cp["total_reward_per_energy"].as_f64().unwrap_or(0.0), cp["free_gb"].as_f64().unwrap_or(0.0), cp["log_bytes"].as_f64().unwrap_or(0.0) / 1e6);
            cps.push(cp);
            while next_cp <= steps_done {
                next_cp += o.checkpoint_every;
            }
        }
    }
    if last_peak == 0 {
        // resumed with nothing to run: estimate the restart memory from the log size (measured ratio about 25)
        last_peak = dir_stats(&o.root.join(OWNER).join("log")).0 * 25;
    }
    if cps.last().map_or(true, |c| c["steps"].as_u64() != Some(steps_done)) || an.last_version == 0 {
        cps.push(checkpoint(&o, &mut an, &mut recorded, &mut oracle_cache, &mut out, "final", mem_ok(last_peak)));
    }
    // windows
    for (k, w) in &an.windows {
        out.put(json!({ "type": "window", "from": k * o.window, "to": (k + 1) * o.window, "steps": w.steps, "attempts": w.attempts, "skips": w.skips, "correct": w.correct, "wrong": w.wrong, "reward": w.reward, "cost": w.cost,
            "rpe": if w.cost > 0 { w.reward as f64 / w.cost as f64 } else { 0.0 }, "strategies": w.strat, "rule_answers": w.rule_answers, "rule_wrong": w.rule_wrong, "rule_wrong_repeat": w.rule_wrong_repeat,
            "known_bad": w.known_bad, "known_bad_wrong": w.known_bad_wrong, "truly_bad": w.truly_bad, "rules_accepted": w.rules_accepted, "rules_retired": w.rules_retired, "refusals": w.refusals }));
    }
    let summary = json!({
        "type": "summary", "stop_reason": stop_reason, "steps": an.st.step, "hours": started.elapsed().as_secs_f64() / 3600.0, "processes": proc_i,
        "step_errors": an.step_errors, "replay_checked": an.replay_checked, "replay_mismatch": an.replay_mismatch, "cap_violations_log": an.cap_violations, "cap_violations_answers": violations,
        "live_max": an.live_max, "blocks_max": an.blocks_max, "refusals_by_cap": an.refusals_by_cap, "stops": an.stops, "sources": an.sources, "audit_rows": an.audit_rows.len(),
        "audit_sample": an.audit_rows.iter().take(5).cloned().collect::<Vec<_>>(), "oracle_plain": oracle_plain, "oracle_by_rulebook": oracle_cache.values().cloned().collect::<Vec<_>>(),
        "date_end": versioned::now_iso(),
    });
    out.put(summary.clone());
    print_tables(&o, &an, &cps, &summary, oracle_plain);
}

fn print_tables(o: &Opts, an: &Analyzer, cps: &[Value], summary: &Value, oracle_plain: (f64, f64, f64)) {
    println!("\n## Long run: world {}, format {}, {} steps\n", o.world, o.format, an.st.step);
    println!("| steps | verify | verify ms | open ms | decode+fold ms | probe commit ms | log MB | files | bytes/event | bytes/step | rules live / total | oracle (live rules) | cumulative rpe | open_at same | harness peak MB | free GB |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for c in cps {
        let oa = c["open_at"].as_array().map(|a| format!("{}/{}", a.iter().filter(|x| x["same"] == true).count(), a.len())).unwrap_or_default();
        println!(
            "| {} | {} | {:.0} | {:.0} | {:.0} | {:.1} | {:.1} | {} | {:.0} | {:.0} | {} / {} | {:.3} | {:.3} | {} | {:.0} | {:.1} |",
            c["steps"], if c["verify_ok"] == true { "ok" } else { "FAIL" }, c["verify_ms"].as_f64().unwrap_or(0.0), c["open_ms"].as_f64().unwrap_or(0.0),
            c["decode_latest_ms"].as_f64().unwrap_or(0.0) + c["fold_ms"].as_f64().unwrap_or(0.0), c["probe_commit_ms_median"].as_f64().unwrap_or(0.0),
            c["log_bytes"].as_f64().unwrap_or(0.0) / 1e6, c["log_files"], c["bytes_per_event"].as_f64().unwrap_or(0.0), c["bytes_per_step"].as_f64().unwrap_or(0.0),
            c["rules_live"], c["rules_total"], c["oracle_skip_live_rules"].as_f64().unwrap_or(0.0), c["total_reward_per_energy"].as_f64().unwrap_or(0.0), oa,
            c["harness_peak_ws_mb"].as_f64().unwrap_or(0.0), c["free_gb"].as_f64().unwrap_or(0.0)
        );
    }
    println!("\nOracle without rules (fast/careful/deep, hidden success rates): never skips {:.3}; best skip threshold {:.2} gives {:.3}\n", oracle_plain.0, oracle_plain.2, oracle_plain.1);
    println!("| steps | rpe | regret vs skip oracle (no rules) | attempts | skips | right share | known-bad attempts per 1000 | of them wrong | truly bad per 1000 | rule answers | rule wrong | rule wrong repeated | accepted | retired | refusals | fast / careful / deep / discovered |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for (k, w) in &an.windows {
        let rpe = if w.cost > 0 { w.reward as f64 / w.cost as f64 } else { 0.0 };
        let s = |n: &str| w.strat.get(n).copied().unwrap_or(0);
        println!(
            "| {}-{} | {:.3} | {:+.3} | {} | {} | {:.3} | {:.1} | {} | {:.1} | {} | {} | {} | {} | {} | {} | {} / {} / {} / {} |",
            k * o.window, (k + 1) * o.window, rpe, oracle_plain.1 - rpe, w.attempts, w.skips, w.correct as f64 / w.attempts.max(1) as f64,
            1000.0 * w.known_bad as f64 / w.attempts.max(1) as f64, w.known_bad_wrong, 1000.0 * w.truly_bad as f64 / w.attempts.max(1) as f64,
            w.rule_answers, w.rule_wrong, w.rule_wrong_repeat, w.rules_accepted, w.rules_retired, w.refusals, s("fast"), s("careful"), s("deep"), s("discovered")
        );
    }
    println!("\nSummary: {}", serde_json::to_string_pretty(summary).unwrap());
}

// ─── control: same being in-process, one run, compare with the chunked log ──

fn being_tuples(rows: &[EventRow], upto_step: u64) -> Vec<String> {
    let mut v = vec![];
    for e in rows.iter().filter(|e| e.source == SOURCE) {
        if e.kind == "note" && e.tags.split(',').any(|t| t == "audit") {
            continue; // refusals and stops depend on run boundaries
        }
        if let Some(s) = tag(&e.tags, "step").and_then(|s| s.parse::<u64>().ok()) {
            if s >= upto_step {
                break;
            }
        }
        if e.kind == "note" && e.tags.split(',').any(|t| t == "refill") {
            v.push(format!("refill|{}|{}", e.occurred_at, e.tags));
            continue;
        }
        v.push(format!("{}|{}|{}|{}|{}", e.event_id, e.kind, e.occurred_at, e.text, e.tags));
    }
    v
}

fn cmd_control(o: Opts) {
    let root = std::fs::canonicalize(&o.root).unwrap();
    let scratch = root.join("control");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let mut out = Out::new(&root, "results.jsonl");
    let t = Instant::now();
    let mut log = VersionedLog::open_or_create_with_format(&scratch, OWNER, versioned::now_iso, LogFormat::V3).unwrap();
    let (cfg, policy) = cfg_in_process(&o, o.steps);
    let s = run_full(&mut log, &cfg, &ScriptedThinker, &policy, None, None).unwrap();
    let secs = t.elapsed().as_secs_f64();
    let a = being_tuples(&log.open_latest().rows::<EventRow>(), o.steps);
    let main = VersionedLog::open(&root, OWNER).unwrap();
    let b = being_tuples(&main.open_latest().rows::<EventRow>(), o.steps);
    let first_diff = a.iter().zip(b.iter()).position(|(x, y)| x != y);
    let v = json!({ "type": "control", "steps": o.steps, "in_process_secs": secs, "steps_run": s.steps_run, "tuples_in_process": a.len(), "tuples_chunked": b.len(),
        "identical": first_diff.is_none() && a.len() == b.len(), "first_diff": first_diff.map(|i| json!({ "index": i, "in_process": a[i].chars().take(300).collect::<String>(), "chunked": b[i].chars().take(300).collect::<String>() })) });
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
    out.put(v);
    let _ = std::fs::remove_dir_all(&scratch);
}

// ─── torn: crash-like damage on a COPY of the log ────────────────────────────

fn copy_dir(a: &Path, b: &Path) {
    std::fs::create_dir_all(b).unwrap();
    for e in std::fs::read_dir(a).unwrap().flatten() {
        let p = e.path();
        let q = b.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &q);
        } else {
            std::fs::copy(&p, &q).unwrap();
        }
    }
}

fn cmd_torn(o: Opts, src: PathBuf) {
    let src = std::fs::canonicalize(src).unwrap();
    std::fs::create_dir_all(&o.root).unwrap();
    let root = std::fs::canonicalize(&o.root).unwrap();
    let mut out = Out::new(&root, "torn.jsonl");
    let fresh = |name: &str| -> PathBuf {
        let r = root.join(name);
        let _ = std::fs::remove_dir_all(&r);
        copy_dir(&src.join(OWNER), &r.join(OWNER));
        let _ = std::fs::remove_file(r.join(OWNER).join("KILL"));
        r
    };
    let steps_of = |r: &Path| -> (u64, u64) {
        let log = VersionedLog::open(r, OWNER).unwrap();
        let st = BeingState::from_events(&log.open_latest().rows::<EventRow>());
        (st.step, log.latest_version())
    };
    let run_on = |r: &Path, steps: u64| -> (bool, String) {
        let mut s = Server::start(r);
        let res = s.call("being_run", run_args(&o, steps));
        s.stop();
        res
    };
    let mut results = vec![];
    // 1. torn journal append: half a MANIFEST line at the end (a crash during the append)
    {
        let r = fresh("t1_torn_tail");
        let (s0, v0) = steps_of(&r);
        let m = r.join(OWNER).join("log").join("MANIFEST");
        let raw = std::fs::read(&m).unwrap();
        let last_line_start = raw[..raw.len() - 1].iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        let half = &raw[last_line_start..last_line_start + (raw.len() - last_line_start) / 2].to_vec();
        let mut f = std::fs::OpenOptions::new().append(true).open(&m).unwrap();
        f.write_all(half).unwrap();
        drop(f);
        let verify = versioned::verify_dir(&r.join(OWNER).join("log"));
        let open = VersionedLog::open(&r, OWNER).map(|l| l.latest_version());
        let (e, t) = run_on(&r, 200);
        let (s1, _) = steps_of(&r);
        let after = versioned::verify_dir(&r.join(OWNER).join("log"));
        let v = json!({ "test": "torn manifest tail (half a journal line appended)", "verify_before_restart": format!("{verify:?}"), "open_version": format!("{open:?}"), "version_before": v0, "being_run_error": e, "being_run": t.chars().take(160).collect::<String>(), "steps_before": s0, "steps_after": s1, "verify_after": after.is_ok() });
        results.push(v);
    }
    // 2. crash after a segment was written but before the MANIFEST line: a stray segment file
    {
        let r = fresh("t2_stray_segment");
        let (s0, v0) = steps_of(&r);
        let dir = r.join(OWNER).join("log").join("events");
        let any = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).max().unwrap();
        let stray = dir.join(format!("{:010}.kore", v0 + 1));
        std::fs::copy(&any, &stray).unwrap();
        let verify = versioned::verify_dir(&r.join(OWNER).join("log"));
        let (e, t) = run_on(&r, 200);
        let (s1, _) = steps_of(&r);
        let after = versioned::verify_dir(&r.join(OWNER).join("log"));
        results.push(json!({ "test": "stray segment of an uncommitted version", "verify_before_restart": format!("{verify:?}"), "being_run_error": e, "being_run": t.chars().take(160).collect::<String>(), "steps_before": s0, "steps_after": s1, "verify_after": after.is_ok(), "verify_after_err": after.err().map(|x| x.to_string()) }));
    }
    // 3. lost last commit (MANIFEST cut at a line boundary): the being redoes the lost steps; they must be identical
    {
        let r = fresh("t3_lost_commit");
        let (s0, _) = steps_of(&r);
        let log = VersionedLog::open(&r, OWNER).unwrap();
        let head = log.snapshot_hash(log.latest_version()).unwrap();
        let before: Vec<EventRow> = log.open_latest().rows();
        drop(log);
        let m = r.join(OWNER).join("log").join("MANIFEST");
        let raw = std::fs::read(&m).unwrap();
        // drop the last entry line whose commit holds being steps: cut lines from the end until the step count falls
        let mut cut = raw.clone();
        let mut lost_lines = 0;
        loop {
            let end = cut.len() - 1;
            let start = cut[..end].iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            cut.truncate(start);
            lost_lines += 1;
            std::fs::write(&m, &cut).unwrap();
            let (s, _) = steps_of(&r);
            if s < s0 || lost_lines > 20 {
                break;
            }
        }
        let (s_cut, _) = steps_of(&r);
        let verify = versioned::verify_dir(&r.join(OWNER).join("log"));
        let verify_against = versioned::verify_dir(&r.join(OWNER).join("log")).and_then(|_| {
            let l = VersionedLog::open(&r, OWNER).map_err(|e| versioned::VerifyError::Io(e.to_string()))?;
            l.verify_against(&head)
        });
        let (e, t) = run_on(&r, s0 - s_cut);
        let (s1, _) = steps_of(&r);
        let after: Vec<EventRow> = VersionedLog::open(&r, OWNER).unwrap().open_latest().rows();
        let a = being_tuples(&before, s0);
        let b = being_tuples(&after, s0);
        let same = a == b;
        results.push(json!({ "test": "last commit lost (MANIFEST cut at a line boundary), steps redone", "manifest_lines_cut": lost_lines, "steps_before": s0, "steps_after_cut": s_cut, "verify_after_cut": format!("{verify:?}"), "verify_against_recorded_head": format!("{verify_against:?}"), "being_run_error": e, "being_run": t.chars().take(160).collect::<String>(), "steps_after_redo": s1, "redone_events_identical": same, "tuples": [a.len(), b.len()] }));
    }
    // 4. a flipped byte in a committed segment: verify must fail, the binary must refuse to run on it
    {
        let r = fresh("t4_flip");
        let dir = r.join(OWNER).join("log").join("events");
        let seg = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "kore")).min().unwrap();
        let mut b = std::fs::read(&seg).unwrap();
        let i = b.len() / 2;
        b[i] ^= 0x01;
        std::fs::write(&seg, &b).unwrap();
        let verify = versioned::verify_dir(&r.join(OWNER).join("log"));
        let (e, t) = run_on(&r, 10);
        results.push(json!({ "test": "one flipped byte in the oldest events segment", "verify": format!("{verify:?}"), "being_run_error": e, "being_run": t.chars().take(200).collect::<String>() }));
    }
    for v in &results {
        out.put(v.clone());
    }
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
    for n in ["t1_torn_tail", "t2_stray_segment", "t3_lost_commit", "t4_flip"] {
        let _ = std::fs::remove_dir_all(root.join(n));
    }
}

// ─── authz: the authorize() path of being_run (advisor_query), audit rows ────

fn cmd_authz(o: Opts) {
    std::fs::create_dir_all(&o.root).unwrap();
    let root = std::fs::canonicalize(&o.root).unwrap();
    let _ = std::fs::remove_dir_all(root.join(OWNER));
    let o = Opts { root: root.clone(), format: 3, ..o };
    prepare(&o);
    let args = |steps: u64| json!({ "steps": steps, "stop_after_budget": false, "budget": 3000, "chooser": "model", "advisor": "simulated", "advisor_quality": 0.9, "world_seed": o.world, "seed": 1 });
    let mut res = vec![];
    // no autonomy_policy.txt: default deny, nothing runs, one audit row
    let mut s = Server::start(&root);
    let (e1, t1) = s.call("being_run", args(50));
    s.stop();
    let audit = |r: &Path| -> Vec<String> { VersionedLog::open(r, OWNER).unwrap().open_latest().rows::<EventRow>().into_iter().filter(|e| e.source == "authorize").map(|e| e.text).collect() };
    let steps = |r: &Path| -> u64 { BeingState::from_events(&VersionedLog::open(r, OWNER).unwrap().open_latest().rows::<EventRow>()).step };
    res.push(json!({ "case": "advisor run without autonomy_policy.txt", "error": e1, "text": t1.chars().take(200).collect::<String>(), "audit_rows": audit(&root).len(), "steps": steps(&root) }));
    std::fs::write(root.join(OWNER).join("autonomy_policy.txt"), "[L1]\nadvisor_query\n").unwrap();
    let mut s = Server::start(&root);
    let n_calls = 10;
    let mut ok = 0;
    for _ in 0..n_calls {
        let (e, _) = s.call("being_run", args(200));
        ok += (!e) as u32;
    }
    s.stop();
    let rows = audit(&root);
    let allow = rows.iter().filter(|t| t.contains("\"decision\":\"allow\"") && t.contains("advisor_query")).count();
    res.push(json!({ "case": "advisor_query listed under L1, 10 calls of 200 steps", "calls_ok": ok, "audit_allow_rows": allow, "audit_rows_total": rows.len(), "steps": steps(&root) }));
    std::fs::write(root.join(OWNER).join("KILL"), "x").unwrap();
    let mut s = Server::start(&root);
    let (e3, t3) = s.call("being_run", args(50));
    s.stop();
    let _ = std::fs::remove_file(root.join(OWNER).join("KILL"));
    res.push(json!({ "case": "KILL file present", "error": e3, "text": t3.chars().take(200).collect::<String>(), "audit_rows_total": audit(&root).len(), "steps": steps(&root) }));
    println!("{}", serde_json::to_string_pretty(&res).unwrap());
    let mut out = Out::new(&root, "authz.jsonl");
    for v in res {
        out.put(v);
    }
}

/// scan: the same being in-process on several world seeds (temp logs), to pick a world where rules are found.
fn cmd_scan(o: Opts) {
    std::fs::create_dir_all(&o.root).unwrap();
    let root = std::fs::canonicalize(&o.root).unwrap();
    for seed in 1..=o.world {
        let scratch = root.join(format!("scan{seed}"));
        let _ = std::fs::remove_dir_all(&scratch);
        let t = Instant::now();
        let mut log = VersionedLog::open_or_create_with_format(&scratch, OWNER, versioned::now_iso, LogFormat::V3).unwrap();
        let (cfg, policy) = cfg_in_process(&Opts { world: seed, ..o.clone() }, o.steps);
        let s = run_full(&mut log, &cfg, &ScriptedThinker, &policy, None, None).unwrap();
        println!("world {seed}: {} steps in {:.1}s, rpe {:.3}, examples {}, rules {} live {}, refusals {}", s.steps_run, t.elapsed().as_secs_f64(), s.state.total_reward as f64 / s.state.total_cost.max(1) as f64, s.state.examples.len(), s.state.rules.len(), s.state.live_rules(), s.refusals.len());
        drop(log);
        let _ = std::fs::remove_dir_all(&scratch);
    }
}

/// memstat: rows per table and the memory a restart needs (open, decode the latest snapshot, event rows, fold).
fn cmd_memstat(o: Opts) {
    let root = std::fs::canonicalize(&o.root).unwrap();
    let ws = || win::self_peak_ws() as f64 / 1e6;
    let m0 = ws();
    let log = VersionedLog::open(&root, OWNER).unwrap();
    let m1 = ws();
    let rows = log.versions().last().map(|v| v.rows.clone()).unwrap_or_default();
    let snap = log.open_latest();
    let m2 = ws();
    let ev: Vec<EventRow> = snap.rows();
    let m3 = ws();
    let st = BeingState::from_events(&ev);
    let m4 = ws();
    let text: usize = ev.iter().map(|e| e.text.len() + e.tags.len() + e.event_id.len() + e.occurred_at.len() + e.recorded_at.len() + e.source.len() + e.kind.len()).sum();
    let revs: Vec<versioned::RevisionRow> = snap.rows();
    let rtext: usize = revs.iter().map(|r| r.rationale.len() + r.stance.len() + r.key.len()).sum();
    println!("{}", serde_json::to_string_pretty(&json!({ "steps": st.step, "rows_per_table": rows, "peak_ws_mb_after": { "start": m0, "open": m1, "open_latest": m2, "event_rows": m3, "fold": m4 },
        "event_string_bytes": text, "revision_rationale_stance_key_bytes": rtext, "log_bytes_on_disk": dir_stats(&root.join(OWNER).join("log")).0 })).unwrap());
}

/// prof: where the time of one step goes as the history grows (in-process engine, no log): the step
/// itself (decide, act, check), the discovery pass (every 10 steps) and housekeeping (pruning, every step).
fn cmd_prof(o: Opts) {
    use being::planner::MenuChooser;
    use being::state::Genesis;
    let g = Genesis { world_seed: o.world, loop_seed: 1, budget: o.budget as i64, explore_c: being::planner::DEFAULT_BETA, chooser: Chooser::Model, extended: true, ext2: true, rule_drift: false, drift: false, adapt: Default::default(), advice: Default::default(), transfer: false, learn_transfer: false };
    let mut st = BeingState::default();
    st.start(g.clone(), String::new());
    let (limits, _) = Controller::parse(CONTROLLER).unwrap().limits(true, true);
    let th = ScriptedThinker;
    let mut e = being::run_loop::Engine::new(world_for(&g), st, &th).with_limits(Some(limits));
    let mut mc = ModelChooser::for_genesis(&g);
    let _ = mc.name();
    let (mut t_step, mut t_disc, mut t_house) = (0.0f64, 0.0f64, 0.0f64);
    println!("| steps | examples | live rules | ms per step: step | discovery (amortised) | housekeeping | total |");
    println!("|---|---|---|---|---|---|---|");
    let bucket = o.window.max(1);
    let mut n = 0u64;
    while n < o.steps {
        let t = Instant::now();
        match e.step_menu(&mut mc) {
            Ok(_) => {}
            Err(being::StopReason::BudgetExhausted) => {
                let c = e.state.clock;
                e.state.apply_refill(g.budget, c + 86_400);
                continue;
            }
            Err(_) => break,
        }
        t_step += ms(t);
        n += 1;
        if e.state.step % 10 == 0 {
            let t = Instant::now();
            let _ = e.discover_pass();
            t_disc += ms(t);
        }
        let t = Instant::now();
        let _ = e.housekeeping();
        t_house += ms(t);
        let _ = e.take_refusals();
        if n % bucket == 0 {
            let b = bucket as f64;
            println!("| {n} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} |", e.state.examples.len(), e.state.live_rules(), t_step / b, t_disc / b, t_house / b, (t_step + t_disc + t_house) / b);
            t_step = 0.0;
            t_disc = 0.0;
            t_house = 0.0;
        }
    }
}

/// rulecheck: for every wrong answer that came from a rule, was the rule itself wrong on that task (its
/// prediction differs from the revealed next term) or was it an execution slip (the rule's prediction is right)?
fn cmd_rulecheck(o: Opts) {
    let root = std::fs::canonicalize(&o.root).unwrap();
    let log = VersionedLog::open(&root, OWNER).unwrap();
    let rows: Vec<EventRow> = log.open_latest().rows();
    let mut st = BeingState::default();
    let mut per: BTreeMap<String, (u64, u64, u64, u64)> = BTreeMap::new(); // (uses, wrong, rule wrong on the task, slips)
    for e in rows.iter().filter(|e| e.source == SOURCE) {
        if e.kind == "outcome" {
            if let (Some(id), Some(out)) = (tag(&e.tags, "rule_used"), being::state::outcome_from_event(e)) {
                let c = per.entry(id.to_string()).or_default();
                c.0 += 1;
                if !out.correct && !out.skipped {
                    c.1 += 1;
                    let rule = st.rules.iter().find(|r| r.id == id);
                    match (rule, &out.example) {
                        (Some(r), Some(ex)) if being::discover::predict_x(r.xf, &r.form, &ex.terms) == Some(ex.next) => c.3 += 1,
                        (Some(_), Some(_)) => c.2 += 1,
                        _ => {}
                    }
                }
            }
        }
        st.fold_event(e);
    }
    println!("| rule | form | difficulty | uses | wrong answers | rule wrong on the task | execution slips |");
    println!("|---|---|---|---|---|---|---|");
    for (id, c) in &per {
        let r = st.rules.iter().find(|r| &r.id == id);
        println!("| {id} | {} | {} | {} | {} | {} | {} |", r.map(|r| format!("{} {}", r.xf.code(), r.form.code())).unwrap_or_default(), r.map(|r| r.sig).unwrap_or(0), c.0, c.1, c.2, c.3);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().cloned().unwrap_or_default();
    match cmd.as_str() {
        "run" => cmd_run(parse_opts(&args[1..])),
        "control" => cmd_control(parse_opts(&args[1..])),
        "authz" => cmd_authz(parse_opts(&args[1..])),
        "scan" => cmd_scan(parse_opts(&args[1..])),
        "memstat" => cmd_memstat(parse_opts(&args[1..])),
        "prof" => cmd_prof(parse_opts(&args[1..])),
        "rulecheck" => cmd_rulecheck(parse_opts(&args[1..])),
        "torn" => {
            let i = args.iter().position(|a| a == "--src").expect("--src <root of a run>");
            let src = PathBuf::from(&args[i + 1]);
            let mut rest: Vec<String> = args[1..].to_vec();
            let j = rest.iter().position(|a| a == "--src").unwrap();
            rest.drain(j..j + 2);
            cmd_torn(parse_opts(&rest), src)
        }
        _ => eprintln!("usage: longrun_being run|control|torn|authz --root <dir> [options]"),
    }
}
