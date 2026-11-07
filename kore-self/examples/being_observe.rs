//! The observer as a command: a readable behaviour report of the World v2 being, built only from its log
//! (Track B10, docs/observe.md). Read-only: it opens the log, reads the events and revisions, prints.
//!
//!   cargo run -p kore-self --example being_observe --offline --release -- --root <data root> --owner <owner>
//!       [--window <steps>] [--max-items <n>] [--json] [--md <file>] [--html <file>]
//!
//! The log is `<data root>/<owner>/log` (the same layout as the server's). `--md` / `--html` also write the
//! report to that file (the only file it writes). For a demonstration on a fresh directory:
//!
//!   cargo run -p kore-self --example being_observe --offline --release -- --demo <empty directory> [--seed N]
//!
//! runs a small all-skills World v2 life (synthetic world, author-defined families) into
//! `<directory>/demo/log` first and then reports on it.
//!
//! World O (Track B12): `--source being:worldo` reads the World O rows of a log (same event shapes as World
//! v2: decisions, outcomes, rule notes) through the same report; `--demo-o <empty directory> [--seed N]
//! [--steps N] [--l2l on|off]` first runs a World O life (B11 learner, or with the Track B12 switches on).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::policy::Policy;
use being::world2::all_skills::{grant, run_v2_all, Want};
use being::world2::engine::Run2Cfg;
use being::world2::observe::{observe, observe_html};
use being::world2::world::W2Cfg;
use std::path::PathBuf;
use versioned::{EventRow, RevisionRow, VersionedLog};

fn usage() -> ! {
    eprintln!("usage: being_observe --root <data root> --owner <owner> [--window N] [--max-items N] [--json] [--md FILE] [--html FILE]\n       being_observe --demo <empty directory> [--seed N]\n       being_observe --demo-o <empty directory> [--seed N] [--steps N] [--l2l on|off]   (World O; --source being:worldo for an existing log)");
    std::process::exit(2)
}

fn demo(dir: &PathBuf, seed: u64) {
    if dir.join("demo").exists() {
        eprintln!("{} already holds a demo log; use --root {} --owner demo", dir.display(), dir.display());
        return;
    }
    std::fs::create_dir_all(dir).expect("create the demo directory");
    let mut log = VersionedLog::open_or_create(dir, "demo").expect("create the demo log");
    let ctl = "max_live_rules = 16\nmax_steps_per_run = 2000\nrules_use = on\nadaptation = on\nrule_retirement = on\nexperiments = on\nmax_experiment_energy_pm = 50\nmax_experiments_per_run = 200\nlookahead = off\n";
    let g = grant(Some(ctl), Want::all(), None);
    let limits = being::controller::Controller::parse(ctl).expect("valid").limits(false, false).0;
    let world = W2Cfg { noise_pm: 100, drift_per_10k: 60, ..W2Cfg::new(seed, 12, 3000) };
    let cfg = Run2Cfg { world: Some(world), learner: Some(g.learner), discover_every: 10, ctl: Some(limits) };
    let pol = Policy::new(2000);
    let o = run_v2_all(&mut log, &cfg, &pol, &g.all).expect("demo run");
    eprintln!("demo: {} steps written to {} (lookahead refused by the demo controller: lookahead = off)", o.summary.steps_run, dir.join("demo").join("log").display());
}

fn demo_o(dir: &PathBuf, seed: u64, steps: u64, l2l: bool) {
    use being::world_o::b12::L2LCfg;
    use being::world_o::engine::{run_o, LearnerOCfg, RunOCfg};
    use being::world_o::world::WOCfg;
    if dir.join("demo").exists() {
        eprintln!("{} already holds a demo log; use --root {} --owner demo --source being:worldo", dir.display(), dir.display());
        return;
    }
    std::fs::create_dir_all(dir).expect("create the demo directory");
    let mut log = VersionedLog::open_or_create(dir, "demo").expect("create the demo log");
    let mut l = LearnerOCfg::default();
    if l2l {
        l.l2l = L2LCfg { prior_k: 8, explore_n: 10, explore_w_milli: 500, explore_pm: 50, form_prior: 2 };
    }
    let limits = being::controller::Limits { max_live_rules: 200, max_candidates: 100_000, ..Default::default() };
    let cfg = RunOCfg { world: Some(WOCfg::new(seed, 1_200_000)), learner: Some(l), discover_every: 10, ctl: Some(limits), max_explore_pm: None };
    let o = run_o(&mut log, &cfg, &Policy::new(steps), None).expect("demo run");
    eprintln!("demo: World O, {} steps written to {} (learning-to-learn switches {})", o.steps_run, dir.join("demo").join("log").display(), if l2l { "on" } else { "off" });
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (mut root, mut owner, mut window, mut max_items, mut json, mut md, mut html) = (None, None, 0u64, 20usize, false, None, None);
    let mut demo_dir: Option<PathBuf> = None;
    let mut seed = 2u64;
    let mut source: Option<String> = None;
    let (mut demo_o_dir, mut steps, mut l2l): (Option<PathBuf>, u64, bool) = (None, 20_000, false);
    let mut i = 0;
    while i < a.len() {
        let v = |i: usize| a.get(i + 1).cloned().unwrap_or_else(|| usage());
        match a[i].as_str() {
            "--root" => root = Some(PathBuf::from(v(i))),
            "--owner" => owner = Some(v(i)),
            "--window" => window = v(i).parse().unwrap_or_else(|_| usage()),
            "--max-items" => max_items = v(i).parse().unwrap_or_else(|_| usage()),
            "--md" => md = Some(PathBuf::from(v(i))),
            "--html" => html = Some(PathBuf::from(v(i))),
            "--json" => {
                json = true;
                i += 1;
                continue;
            }
            "--demo" => demo_dir = Some(PathBuf::from(v(i))),
            "--seed" => seed = v(i).parse().unwrap_or_else(|_| usage()),
            "--source" => source = Some(v(i)),
            "--demo-o" => demo_o_dir = Some(PathBuf::from(v(i))),
            "--steps" => steps = v(i).parse().unwrap_or_else(|_| usage()),
            "--l2l" => l2l = v(i) == "on",
            _ => usage(),
        }
        i += 2;
    }
    if let Some(d) = demo_dir {
        demo(&d, seed);
        root = Some(d);
        owner = Some("demo".into());
    }
    if let Some(d) = demo_o_dir {
        demo_o(&d, seed, steps, l2l);
        root = Some(d);
        owner = Some("demo".into());
        source = Some(being::world_o::world::SOURCE_O.into());
    }
    let (Some(root), Some(owner)) = (root, owner) else { usage() };
    let log = match VersionedLog::open(&root, &owner) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot open the log {}: {e}", root.join(&owner).join("log").display());
            std::process::exit(1)
        }
    };
    let snap = log.open_latest();
    let mut events: Vec<EventRow> = snap.rows();
    // another being's rows (World O): read through the same report; only this in-memory view is relabelled
    if let Some(src) = &source {
        events.retain(|e| &e.source == src);
        for e in events.iter_mut() {
            e.source = being::world2::world::SOURCE2.into();
            // the World O genesis and rule notes, under the World v2 names the report reads
            e.tags = e.tags.split(',').map(|t| match t {
                "wo_genesis" => "w2_genesis",
                "worldo" => "world2",
                x => x,
            }).collect::<Vec<_>>().join(",");
        }
    }
    let revs: Vec<RevisionRow> = snap.rows();
    let o = observe(&events, &revs, window);
    if json {
        println!("{}", serde_json::to_string_pretty(&o.to_json()).expect("json"));
    } else {
        print!("{}", o.to_md(max_items));
    }
    if let Some(p) = md {
        std::fs::write(&p, o.to_md(max_items)).expect("write the markdown report");
        eprintln!("markdown report written to {}", p.display());
    }
    if let Some(p) = html {
        std::fs::write(&p, observe_html(&events, &revs, window, max_items)).expect("write the html report");
        eprintln!("html report written to {}", p.display());
    }
}
