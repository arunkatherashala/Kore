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

/// The controller.txt the harness writes: `CONTROLLER`, plus `checkpoint_every = N` when the environment
/// variable `LONGRUN_CKPT_EVERY` is set (Track D1b measurements; unset = no checkpoints, as before).
/// Track D1c: plus `fast_open = on` when `LONGRUN_FAST_OPEN=1` (unset = the key is absent = off).
fn controller_file_text() -> String {
    let mut t = controller_text_d1b();
    if std::env::var("LONGRUN_FAST_OPEN").map_or(false, |v| v.trim() == "1") {
        t.push_str("fast_open = on\n");
    }
    t
}

fn controller_text_d1b() -> String {
    match std::env::var("LONGRUN_CKPT_EVERY") {
        Ok(n) if !n.trim().is_empty() => format!("{CONTROLLER}checkpoint_every = {}
", n.trim()),
        _ => CONTROLLER.to_string(),
    }
}

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
    kill_ms: Option<u64>,
    /// Track D1c: `openfast` opens with `fast_open` on (1) or off (0).
    fast: u8,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut o = Opts { root: PathBuf::new(), steps: 20_000, chunk: 5000, calls_per_proc: 2, checkpoint_every: 10_000, window: 10_000, format: 3, world: 1, budget: 3000, kill_at: None, min_free_gb: 12.0, replay_n: 300, max_hours: 1e9, heavy_every: 5, kill_ms: None, fast: 1 };
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
            "--kill-ms" => o.kill_ms = Some(v.parse().unwrap()),
            "--fast" => o.fast = v.parse().unwrap(),
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
    // every checkpoint: the restart's own read (events-only fold of the whole log) equals the incremental fold
    let t = Instant::now();
    let restart_fold = BeingState::from_log(&log).expect("events-only fold");
    let restart_fold_ms = ms(t);
    let restart_fold_equal = format!("{restart_fold:?}") == format!("{:?}", an.st);
    drop(restart_fold);
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
        "fold_consistent": consistent, "restart_fold_equal": restart_fold_equal, "restart_fold_ms": restart_fold_ms, "open_at": open_at, "probe_commit_ms_median": probe[1], "probe_commit_ms": probe,
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

/// Room for a full decode of the latest snapshot in this process (measured: about 25 x the log bytes).
fn snapshot_mem_ok(o: &Opts) -> bool {
    let (t, a) = win::memory();
    t == 0 || (a as f64) > 1.3 * 25.0 * dir_stats(&o.root.join(OWNER).join("log")).0 as f64 + 3e9
}

// ─── run ─────────────────────────────────────────────────────────────────────

fn prepare(o: &Opts) {
    std::fs::create_dir_all(o.root.join(OWNER)).unwrap();
    std::fs::write(o.root.join(OWNER).join("controller.txt"), controller_file_text()).unwrap();
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
    let mut steps_done = BeingState::from_log(&VersionedLog::open(&o.root, OWNER).expect("open log")).expect("fold").step;
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
            let cp = checkpoint(&o, &mut an, &mut recorded, &mut oracle_cache, &mut out, "periodic", (n_cp % o.heavy_every == 1 || o.heavy_every == 1) && snapshot_mem_ok(&o));
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
        cps.push(checkpoint(&o, &mut an, &mut recorded, &mut oracle_cache, &mut out, "final", snapshot_mem_ok(&o)));
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
    let t = Instant::now();
    let log = VersionedLog::open(&root, OWNER).unwrap();
    let (m1, t1) = (ws(), ms(t));
    let rows = log.versions().last().map(|v| v.rows.clone()).unwrap_or_default();
    let t = Instant::now();
    let snap = log.open_latest();
    let (m2, t2) = (ws(), ms(t));
    let t = Instant::now();
    let ev: Vec<EventRow> = snap.rows();
    let (m3, t3) = (ws(), ms(t));
    let t = Instant::now();
    let st = BeingState::from_events(&ev);
    let (m4, t4) = (ws(), ms(t));
    let text: usize = ev.iter().map(|e| e.text.len() + e.tags.len() + e.event_id.len() + e.occurred_at.len() + e.recorded_at.len() + e.source.len() + e.kind.len()).sum();
    let revs: Vec<versioned::RevisionRow> = snap.rows();
    let rtext: usize = revs.iter().map(|r| r.rationale.len() + r.stance.len() + r.key.len()).sum();
    println!("{}", serde_json::to_string_pretty(&json!({ "steps": st.step, "rows_per_table": rows, "peak_ws_mb_after": { "start": m0, "open": m1, "open_latest": m2, "event_rows": m3, "fold": m4 },
        "ms": { "open": t1, "open_latest": t2, "event_rows": t3, "fold": t4 },
        "event_string_bytes": text, "revision_rationale_stance_key_bytes": rtext, "log_bytes_on_disk": dir_stats(&root.join(OWNER).join("log")).0 })).unwrap());
}

/// memfold: the restart read path of `being_run` after the restart fix (open, events-only fold), with
/// the peak working set and time after each part. Compare with `memstat` (the old path).
fn cmd_memfold(o: Opts) {
    let root = std::fs::canonicalize(&o.root).unwrap();
    let ws = || win::self_peak_ws() as f64 / 1e6;
    let m0 = ws();
    let t = Instant::now();
    let g = BeingState::genesis_peek(&root, OWNER).unwrap();
    let (mg, tg) = (ws(), ms(t));
    let t = Instant::now();
    let log = VersionedLog::open(&root, OWNER).unwrap();
    let (m1, t1) = (ws(), ms(t));
    // Track D1b: checkpoint restore (newest valid checkpoint + tail fold; the full fold if none), measured
    // before the full fold so that its peak is its own
    let t = Instant::now();
    let (rst, rinfo) = being::ckpt::restore::<BeingState>(&log).unwrap();
    let (mr, tr) = (ws(), ms(t));
    let t = Instant::now();
    let st = BeingState::from_log(&log).unwrap();
    let (m2, t2) = (ws(), ms(t));
    let restore_equal = format!("{rst:?}") == format!("{st:?}");
    // the floor of any verified open: read every segment file and hash it (no decoding)
    let t = Instant::now();
    let dir = root.join(OWNER).join("log");
    let raw = std::fs::read_to_string(dir.join("MANIFEST")).unwrap();
    let files: Vec<String> = raw.lines().skip(1).filter_map(|l| serde_json::from_str::<Value>(l).ok()).flat_map(|e| e["segments"].as_array().unwrap().iter().map(|g| g["file"].as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
    let t_manifest = ms(t);
    let t = Instant::now();
    let bytes: usize = files.iter().map(|f| { let b = std::fs::read(dir.join(f)).unwrap(); let _ = versioned::sha256_hex(&[&b]); b.len() }).sum();
    let t_read = ms(t);
    println!("{}", json!({ "type": "memfold", "steps": st.step, "events": log.event_count(), "genesis_found": g.is_some(), "restore_from_version": rinfo.from_version, "restore_rejected": rinfo.rejected, "restore_equal_full_fold": restore_equal, "peak_ws_mb_after": { "start": m0, "genesis_peek": mg, "open": m1, "restore": mr, "fold": m2 }, "ms": { "genesis_peek": tg, "open": t1, "restore": tr, "fold": t2, "manifest_json_parse_1_thread": t_manifest, "read_and_sha_all_segments_1_thread": t_read }, "segment_files": files.len(), "segment_bytes": bytes }));
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
    println!("| steps | examples | live rules | ms per step: step | discovery (amortised) | housekeeping | total | scan cache MB | peak working set MB |");
    println!("|---|---|---|---|---|---|---|---|---|");
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
            println!("| {n} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.1} | {:.0} |", e.state.examples.len(), e.state.live_rules(), t_step / b, t_disc / b, t_house / b, (t_step + t_disc + t_house) / b, e.scan.approx_bytes() as f64 / 1e6, win::self_peak_ws() as f64 / 1e6);
            t_step = 0.0;
            t_disc = 0.0;
            t_house = 0.0;
        }
    }
    // the first discovery pass of a new engine (after every restart) scans every example once
    let blocks: Vec<being::discover::Form> = e.blocks().into_iter().take(8).collect();
    let t = Instant::now();
    let mut cold = being::discover_scan::ScanCache::new();
    let o = cold.search(&e.state.examples, being::discover::K_TRAIN, being::discover::K_VAL, &blocks, 2000, &[]);
    println!("first pass of a new engine over {} examples: {:.0} ms, {} predictions, {} rules found", e.state.examples.len(), ms(t), cold.predictions, o.found.len());
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

// ─── restart fix: cut a log at a version, time one restart, compare two logs ─────

/// cut: copy of the log of `--src` holding only versions 0..=V (MANIFEST lines and their segments).
fn cmd_cut(o: Opts, src: PathBuf, upto: u64) {
    let src = std::fs::canonicalize(src).unwrap().join(OWNER).join("log");
    std::fs::create_dir_all(&o.root).unwrap();
    let root = std::fs::canonicalize(&o.root).unwrap();
    let dst = root.join(OWNER).join("log");
    assert!(!dst.join("MANIFEST").exists(), "{} already holds a log", dst.display());
    let raw = std::fs::read_to_string(src.join("MANIFEST")).unwrap();
    let mut out = String::new();
    let mut n = 0u64;
    for (i, line) in raw.split_inclusive('\n').enumerate() {
        if i == 0 {
            out.push_str(line);
            continue;
        }
        if !line.ends_with('\n') {
            break;
        }
        let e: Value = serde_json::from_str(line.trim_end()).unwrap();
        if e["v"].as_u64().unwrap() > upto {
            break;
        }
        for g in e["segments"].as_array().unwrap() {
            let f = g["file"].as_str().unwrap();
            let to = dst.join(f);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(src.join(f), &to).unwrap();
        }
        out.push_str(line);
        n += 1;
    }
    for t in ["events", "revisions", "revision_causes", "predictions", "outcomes", "schema_meta"] {
        std::fs::create_dir_all(dst.join(t)).unwrap();
    }
    std::fs::write(dst.join("MANIFEST"), out).unwrap();
    std::fs::write(root.join(OWNER).join("controller.txt"), controller_file_text()).unwrap();
    let log = VersionedLog::open(&root, OWNER).unwrap();
    println!("{}", json!({ "type": "cut", "versions": n, "latest": log.latest_version(), "events": log.event_count() }));
}

/// restart: one cold start of the binary (LONGRUN_BIN or the default) and one `being_run` call of
/// `--chunk` steps on the log under `--root`. With `--kill-ms D` the KILL file is created D ms after
/// the call was sent and the time from the file to the answer is recorded (the file is removed after).
fn cmd_restart(o: Opts) {
    let root = std::fs::canonicalize(&o.root).unwrap();
    let _ = std::fs::remove_file(root.join(OWNER).join("KILL"));
    let sys0 = win::system_times();
    let t0 = Instant::now();
    let mut srv = Server::start(&root);
    let start_ms = ms(t0);
    // --kill-ms 0: the file exists before the call is sent
    let pre = (o.kill_ms == Some(0)).then(|| {
        std::fs::write(root.join(OWNER).join("KILL"), "stop\n").unwrap();
        Instant::now()
    });
    let kill = o.kill_ms.filter(|d| *d > 0).map(|d| {
        let p = root.join(OWNER).join("KILL");
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(d));
            std::fs::write(&p, "stop\n").unwrap();
            Instant::now()
        })
    });
    let tc = Instant::now();
    let (is_err, text) = srv.call("being_run", run_args(&o, o.chunk));
    let done = Instant::now();
    let secs = tc.elapsed().as_secs_f64();
    let kill_lag_ms = kill.map(|h| h.join().unwrap()).or(pre).map(|t| done.saturating_duration_since(t).as_secs_f64() * 1000.0);
    let (peak, cpu) = srv.stop();
    let _ = std::fs::remove_file(root.join(OWNER).join("KILL"));
    let r: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let v = json!({ "type": "restart", "index_runs": r["index_runs"], "binary": binary().display().to_string(), "chunk": o.chunk, "is_error": is_err, "steps_run": r["steps_run"], "steps_total": r["total_steps"], "stopped": r["stopped"],
        "text": if r.is_null() { text.chars().take(300).collect::<String>() } else { String::new() },
        "start_ms": start_ms, "call_secs": secs, "kill_ms": o.kill_ms, "kill_lag_ms": kill_lag_ms, "peak_ws_mb": peak as f64 / 1e6, "cpu_secs": cpu, "machine_busy_share": busy_share(sys0, win::system_times()), "mem_avail_gb": gb(win::memory().1) });
    println!("{v}");
    Out::new(&root, "restart.jsonl").put(v);
}

// ─── Track D1c: format-5 preparation and the in-process open (docs/d1c_design.md section 7) ─────

fn disk_guard(o: &Opts, dir: &Path) {
    let free = win::free_bytes(dir) as f64 / 1e9;
    assert!(free >= o.min_free_gb, "disk guard: {free:.1} GB free, below {} GB, at {}", o.min_free_gb, dir.display());
}

/// d1c-prep: make the copy under `--root` the "on" variant: explicit `upgrade_to_v5`, and controller.txt
/// rewritten (`LONGRUN_FAST_OPEN=1` adds `fast_open = on`, `LONGRUN_CKPT_EVERY` as for `cut`). No run is
/// written here; the first (untimed, priming) `being_run` call with the key on writes the first run.
fn cmd_d1c_prep(o: Opts) {
    let root = std::fs::canonicalize(&o.root).unwrap();
    disk_guard(&o, &root);
    let mut log = VersionedLog::open(&root, OWNER).unwrap();
    let up = log.upgrade_to_v5().unwrap();
    std::fs::write(root.join(OWNER).join("controller.txt"), controller_file_text()).unwrap();
    println!("{}", json!({ "type": "d1c_prep", "upgraded": up, "latest": log.latest_version(), "format": log.format().number(), "controller": controller_file_text() }));
}

/// openfast: one in-process open of the log under `--root` with `fast_open` on (`--fast 1`, default) or
/// off (`--fast 0`): time, peak working set, files and bytes read, what the open used (O1, O3, O8).
fn cmd_openfast(o: Opts) {
    let root = std::fs::canonicalize(&o.root).unwrap();
    let sys0 = win::system_times();
    let m0 = win::self_peak_ws() as f64 / 1e6;
    let t = Instant::now();
    let log = VersionedLog::open_with(&root, OWNER, versioned::OpenOptions { fast_open: o.fast == 1 }).unwrap();
    let open_ms = ms(t);
    let m1 = win::self_peak_ws() as f64 / 1e6;
    let r = log.open_report().clone();
    let dir = root.join(OWNER).join("log");
    let (bytes_on_disk, files_on_disk) = dir_stats(&dir);
    let v = json!({ "type": "openfast", "fast": o.fast, "open_ms": open_ms, "fast_used": r.fast_used, "fallback_reason": r.fallback_reason, "runs_used": r.runs_used, "runs_stale": r.runs_stale,
        "tail_versions": r.tail_versions, "sampled_segments": r.sampled_segments, "files_read": r.files_read, "bytes_read": r.bytes_read, "versions": log.latest_version() + 1, "events": log.event_count(),
        "format": log.format().number(), "index_runs": log.index_runs().len(), "files_on_disk": files_on_disk, "bytes_on_disk": bytes_on_disk,
        "peak_ws_mb": { "start": m0, "after_open": m1 }, "machine_busy_share": busy_share(sys0, win::system_times()) });
    println!("{v}");
    Out::new(&root, "openfast.jsonl").put(v);
}

/// d1c-smoke: correctness only, no timing claims. On the log under `--root` (a copy; it is changed):
/// (created first with `--steps` steps if there is none) upgrade to format 5, write the first run, compare a full and a fast open (every stored event looked
/// up, latest content), then `--steps` more steps in calls of `--chunk` in-process, each call on a
/// fast-opened handle with a run maintained at its end (D-3 cadence), then compare again and `verify`.
fn cmd_d1c_smoke(o: Opts) {
    std::fs::create_dir_all(&o.root).unwrap();
    let root = std::fs::canonicalize(&o.root).unwrap();
    disk_guard(&o, &root);
    let fast = versioned::OpenOptions { fast_open: true };
    // no log yet: create a format-3 log of `--steps` steps in-process first (calls of `--chunk`)
    let mut created = 0u64;
    if !root.join(OWNER).join("log").join("MANIFEST").exists() {
        let mut log = VersionedLog::open_or_create_with_format(&root, OWNER, versioned::now_iso, LogFormat::V3).unwrap();
        while created < o.steps {
            let (cfg, policy) = cfg_in_process(&o, o.chunk.min(o.steps - created));
            let s = run_full(&mut log, &cfg, &ScriptedThinker, &policy, None, None).unwrap();
            created += s.steps_run;
            if s.steps_run == 0 {
                break;
            }
        }
    }
    let extra = 2 * o.chunk;
    let mut log = VersionedLog::open(&root, OWNER).unwrap();
    let upgraded = log.upgrade_to_v5().unwrap();
    let first = log.maintain_index_runs().unwrap();
    drop(log);
    let check = |label: &str| -> Value {
        let full = VersionedLog::open(&root, OWNER).unwrap();
        let fo = VersionedLog::open_with(&root, OWNER, fast).unwrap();
        let rows: Vec<EventRow> = full.open_latest().rows();
        let mut mismatch = 0usize;
        for e in &rows {
            if !fo.has_event(&e.event_id) || fo.get_event(&e.event_id).unwrap().as_ref() != Some(e) {
                mismatch += 1;
            }
        }
        let a = full.open_latest().content_hash();
        let b = fo.open_latest().content_hash();
        let r = fo.open_report().clone();
        json!({ "label": label, "versions": full.latest_version() + 1, "events": rows.len(), "fast_used": r.fast_used, "fallback_reason": r.fallback_reason, "runs_used": r.runs_used, "tail_versions": r.tail_versions,
            "files_read_fast_open": r.files_read, "files_read_full_open": full.open_report().files_read, "event_lookup_mismatches": mismatch, "latest_content_equal": a == b, "event_count_equal": full.event_count() == fo.event_count() })
    };
    let c0 = check("after upgrade and first run");
    let mut done = 0u64;
    let mut calls = vec![];
    while done < extra {
        let n = o.chunk.min(extra - done);
        let mut log = VersionedLog::open_with(&root, OWNER, fast).unwrap();
        let used = log.open_report().fast_used;
        let (cfg, policy) = cfg_in_process(&o, n);
        let s = run_full(&mut log, &cfg, &ScriptedThinker, &policy, None, None).unwrap();
        let m = log.maintain_index_runs().unwrap();
        calls.push(json!({ "steps_run": s.steps_run, "fast_used": used, "run_written": m.written.map(|r| r.file), "merged": m.merged.is_some(), "runs": m.runs }));
        done += s.steps_run;
        if s.steps_run == 0 {
            break;
        }
    }
    let c1 = check("after the calls");
    // the real binary with fast_open = on: one being_run call (fast open, run maintained at its end), then
    // the KILL file present (D-4: the refusal's audit row is written through the fast open)
    let ctl = root.join(OWNER).join("controller.txt");
    let old_ctl = std::fs::read(&ctl).ok();
    std::fs::write(&ctl, format!("{CONTROLLER}fast_open = on
")).unwrap();
    let audit = |l: &VersionedLog| -> usize { let n: Vec<EventRow> = l.open_latest().rows(); n.iter().filter(|e| e.source == "authorize").count() };
    let a0 = audit(&VersionedLog::open(&root, OWNER).unwrap());
    let mut srv = Server::start(&root);
    let (e1, t1) = srv.call("being_run", run_args(&o, 200));
    std::fs::write(root.join(OWNER).join("KILL"), "stop
").unwrap();
    let (e2, t2) = srv.call("being_run", run_args(&o, 200));
    let _ = srv.stop();
    let _ = std::fs::remove_file(root.join(OWNER).join("KILL"));
    match old_ctl {
        Some(b) => std::fs::write(&ctl, b).unwrap(),
        None => { let _ = std::fs::remove_file(&ctl); }
    }
    let r1: Value = serde_json::from_str(&t1).unwrap_or(Value::Null);
    let a1 = audit(&VersionedLog::open(&root, OWNER).unwrap());
    let binary = json!({ "run_is_error": e1, "steps_run": r1["steps_run"], "index_runs": r1["index_runs"], "kill_is_error": e2, "kill_text": t2.chars().take(160).collect::<String>(), "authorize_rows_added": a1 - a0 });
    let c2 = check("after the binary calls");
    let verify = VersionedLog::open(&root, OWNER).unwrap().verify().map_err(|e| e.to_string());
    let v = json!({ "type": "d1c_smoke", "steps_created": created, "steps_after": done, "upgraded": upgraded, "first_run": first.written.map(|r| r.file), "check_before": c0, "calls": calls, "check_after": c1, "binary": binary, "check_after_binary": c2, "verify": format!("{verify:?}") });
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
}

fn row_key_e(e: &EventRow) -> String {
    format!("{}|{}|{}|{}|{}|{}|{}", e.event_id, e.occurred_at, e.source, e.kind, e.text, e.tags, e.redacted)
}

/// compare: rows committed after version `--from` in two logs (roots `--root` and `--other`), field by
/// field, everything except the wall-clock `recorded_at`.
fn cmd_compare(o: Opts, other: PathBuf, from: u64) {
    let a = VersionedLog::open(&std::fs::canonicalize(&o.root).unwrap(), OWNER).unwrap();
    let b = VersionedLog::open(&std::fs::canonicalize(&other).unwrap(), OWNER).unwrap();
    let da = a.diff(from, a.latest_version()).unwrap();
    let db = b.diff(from, b.latest_version()).unwrap();
    let ev = |d: &versioned::Diff| d.events.iter().filter(|e| e.source != "authorize" && e.source != PROBE_SOURCE).map(row_key_e).collect::<Vec<_>>();
    let rv = |d: &versioned::Diff| d.revisions.iter().map(|r| format!("{}|{}|{}|{}|{:?}|{}|{:?}|{}|{}", r.rev_id, r.entity_type, r.key, r.stance, r.confidence, r.rationale, r.supersedes, r.valid_from, r.author)).collect::<Vec<_>>();
    let ca = |d: &versioned::Diff| d.causes.iter().map(|c| format!("{}|{}", c.rev_id, c.event_id)).collect::<Vec<_>>();
    let pr = |d: &versioned::Diff| d.predictions.iter().map(|p| format!("{:?}", p)).collect::<Vec<_>>();
    let oc = |d: &versioned::Diff| d.outcomes.iter().map(|p| format!("{:?}", p)).collect::<Vec<_>>();
    let cmp = |x: Vec<String>, y: Vec<String>| json!({ "a": x.len(), "b": y.len(), "identical": x == y, "first_diff": x.iter().zip(y.iter()).position(|(p, q)| p != q) });
    // Events present in only one of the two logs (multiset difference), printed in full (except recorded_at), and the
    // event sequences compared again without the checkpoint anchor notes (Track D1, source `being:checkpoint`).
    fn kept(d: &versioned::Diff) -> Vec<&EventRow> { d.events.iter().filter(|e| e.source != "authorize" && e.source != PROBE_SOURCE).collect() }
    let only = |x: &[&EventRow], y: &[&EventRow]| {
        let mut left: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for e in y { *left.entry(row_key_e(e)).or_default() += 1; }
        x.iter().filter(|e| { let c = left.entry(row_key_e(e)).or_default(); if *c > 0 { *c -= 1; false } else { true } })
            .map(|e| json!({ "source": e.source, "kind": e.kind, "occurred_at": e.occurred_at, "tags": e.tags, "text": e.text })).collect::<Vec<_>>()
    };
    let (ka, kb) = (kept(&da), kept(&db));
    let no_anchor = |x: &[&EventRow]| x.iter().filter(|e| e.source != "being:checkpoint").map(|e| row_key_e(e)).collect::<Vec<_>>();
    let v = json!({ "type": "compare", "from": from, "versions": [a.latest_version(), b.latest_version()],
        "events_except_audit_and_probes": cmp(ev(&da), ev(&db)), "revisions": cmp(rv(&da), rv(&db)), "causes": cmp(ca(&da), ca(&db)), "predictions": cmp(pr(&da), pr(&db)), "outcomes": cmp(oc(&da), oc(&db)),
        "events_only_in_root": only(&ka, &kb), "events_only_in_other": only(&kb, &ka),
        "events_except_audit_probes_and_checkpoint_anchors": cmp(no_anchor(&ka), no_anchor(&kb)) });
    println!("{v}");
}

/// digest: the being in-process with a real log, in calls of `--chunk` steps (a new engine per call, as
/// `being_run` does), then a sha256 over every being event (id, kind, world time, text, tags) and every
/// revision (type, key, stance, confidence bits, rationale, supersedes, valid from, author), in log order.
/// Two builds that print the same digest for the same world and steps wrote the same records.
fn cmd_digest(o: Opts) {
    std::fs::create_dir_all(&o.root).unwrap();
    let root = std::fs::canonicalize(&o.root).unwrap();
    let scratch = root.join(format!("digest_w{}_{}", o.world, o.steps));
    let _ = std::fs::remove_dir_all(&scratch);
    let mut log = VersionedLog::open_or_create_with_format(&scratch, OWNER, versioned::now_iso, LogFormat::V3).unwrap();
    let t0 = Instant::now();
    let mut done = 0u64;
    println!("| steps at end of call | examples | live rules / total | wall s of the call | ms per step |");
    println!("|---|---|---|---|---|");
    while done < o.steps {
        let n = o.chunk.min(o.steps - done);
        let (cfg, policy) = cfg_in_process(&o, n);
        let t = Instant::now();
        let s = run_full(&mut log, &cfg, &ScriptedThinker, &policy, None, None).unwrap();
        let secs = t.elapsed().as_secs_f64();
        done += s.steps_run;
        println!("| {} | {} | {} / {} | {:.1} | {:.2} |", s.state.step, s.state.examples.len(), s.state.live_rules(), s.state.rules.len(), secs, secs * 1000.0 / s.steps_run.max(1) as f64);
        if s.steps_run == 0 {
            break;
        }
    }
    let snap = log.open_latest();
    let ev: Vec<EventRow> = snap.rows();
    let revs: Vec<versioned::RevisionRow> = snap.rows();
    let mut parts: Vec<Vec<u8>> = vec![];
    let mut nev = 0usize;
    for e in ev.iter().filter(|e| e.source == SOURCE) {
        parts.push(format!("E|{}|{}|{}|{}|{}\n", e.event_id, e.kind, e.occurred_at, e.text, e.tags).into_bytes());
        nev += 1;
    }
    for r in &revs {
        parts.push(format!("R|{}|{}|{}|{}|{}|{:?}|{}|{}\n", r.entity_type, r.key, r.stance, r.confidence.to_bits(), r.rationale, r.supersedes, r.valid_from, r.author).into_bytes());
    }
    let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
    let digest = versioned::sha256_hex(&refs);
    let st = BeingState::from_snapshot(&snap);
    let v = json!({ "type": "digest", "world": o.world, "steps": st.step, "chunk": o.chunk, "events": nev, "revisions": revs.len(), "examples": st.examples.len(),
        "rules": st.rules.iter().map(|r| format!("{}:{}:{}:{}:{}", r.id, r.sig, r.xf.code(), r.form.code(), r.retired)).collect::<Vec<_>>(),
        "total_reward": st.total_reward, "total_cost": st.total_cost, "sha256": digest, "wall_s": t0.elapsed().as_secs_f64() });
    println!("{}", serde_json::to_string(&v).unwrap());
    Out::new(&root, "digest.jsonl").put(v);
    drop(log);
    let _ = std::fs::remove_dir_all(&scratch);
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
        "memfold" => cmd_memfold(parse_opts(&args[1..])),
        "prof" => cmd_prof(parse_opts(&args[1..])),
        "digest" => cmd_digest(parse_opts(&args[1..])),
        "rulecheck" => cmd_rulecheck(parse_opts(&args[1..])),
        "restart" => cmd_restart(parse_opts(&args[1..])),
        "d1c-prep" => cmd_d1c_prep(parse_opts(&args[1..])),
        "openfast" => cmd_openfast(parse_opts(&args[1..])),
        "d1c-smoke" => cmd_d1c_smoke(parse_opts(&args[1..])),
        "cut" | "compare" => {
            let flag = if cmd == "cut" { "--src" } else { "--other" };
            let i = args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag} <root>"));
            let src = PathBuf::from(&args[i + 1]);
            let k = args.iter().position(|a| a == "--at").expect("--at <version>");
            let at: u64 = args[k + 1].parse().unwrap();
            let mut rest: Vec<String> = args[1..].to_vec();
            for f in [flag, "--at"] {
                let j = rest.iter().position(|a| a == f).unwrap();
                rest.drain(j..j + 2);
            }
            if cmd == "cut" {
                cmd_cut(parse_opts(&rest), src, at)
            } else {
                cmd_compare(parse_opts(&rest), src, at)
            }
        }
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
