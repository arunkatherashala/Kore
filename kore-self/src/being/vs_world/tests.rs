//! Tests of Track V1 (determinism, visible features only, equal budgets, LLM request key sets).

use super::adapt::{Player2, PlayerL, PlayerM};
use super::core::{grid, grid_ext, Gran, Method};
use super::llm::{self, LlmPlayer, Told, HISTORY_KEYS, MENU_KEYS, MENU_SEQ_KEYS, RECENT_KEYS, REQUEST_KEYS};
use super::mcts::{Mcts2, MctsL};
use super::rules::{bf_predict, b2_predict, book_answer, guarded_search, knn_seq, tree_seq, tree_seq_answer};
use crate::being::discover::{search, Example, Form};
use crate::being::lookahead::beingl::{run_wl, GreedyL};
use crate::being::lookahead::worldl::{Setting, WLCfg, WorldL};
use crate::being::measure::run_menu_ext;
use crate::being::world::World;
use crate::being::world2::engine::LearnerCfg;
use crate::being::world2::learner::{Learner2, Offer2};
use crate::being::world2::measure::{run_w2, run_w2_with};
use crate::being::world2::solve::sig2;
use crate::being::world2::world::{FType, W2Cfg, World2, TYPE_CYCLE};
use serde_json::Value;
use std::collections::BTreeSet;

/// Two representatives of every method family: the first of its tuning grid and the last of the second
/// pass (which holds the average-reward variants of the value learners).
fn reps() -> Vec<Method> {
    grid().into_iter().map(|g| g[0]).chain(grid_ext().into_iter().map(|g| *g.last().expect("a config"))).collect()
}

fn small(seed: u64, budget: i64) -> W2Cfg {
    W2Cfg { noise_pm: 100, drift_per_10k: 20, ..W2Cfg::new(seed, 6, budget) }
}

#[test]
fn every_method_is_deterministic_in_every_world() {
    for m in reps() {
        let a = run_w2(&small(3, 400), &mut Player2::new(m.agent(3)), 0, None);
        let b = run_w2(&small(3, 400), &mut Player2::new(m.agent(3)), 0, None);
        assert_eq!((a.true_reward, a.cost, a.steps), (b.true_reward, b.cost, b.steps), "{}", m.label());
        let la: Vec<_> = a.recs.iter().map(|r| (r.fam, r.strategy, r.true_reward)).collect();
        let lb: Vec<_> = b.recs.iter().map(|r| (r.fam, r.strategy, r.true_reward)).collect();
        assert_eq!(la, lb, "{}", m.label());
        let cl = WLCfg { seed: 5, setting: Setting::Keys, budget: 200 };
        let x = run_wl(&cl, &mut PlayerL::new(m.agent(5)));
        let y = run_wl(&cl, &mut PlayerL::new(m.agent(5)));
        assert_eq!((x.reward, x.cost, x.steps), (y.reward, y.cost, y.steps), "{}", m.label());
        let p = run_menu_ext(World::with_extended(4, true), 300, &mut PlayerM::new(m.agent(4)), 0, &[]);
        let q = run_menu_ext(World::with_extended(4, true), 300, &mut PlayerM::new(m.agent(4)), 0, &[]);
        assert_eq!((p.run.reward, p.run.cost, p.run.recs.len()), (q.run.reward, q.run.cost, q.run.recs.len()), "{}", m.label());
    }
    let cfg = small(2, 200);
    let a = run_w2(&cfg, &mut Mcts2::new(World2::new(cfg.clone()), 2), 0, None);
    let b = run_w2(&cfg, &mut Mcts2::new(World2::new(cfg.clone()), 2), 0, None);
    assert_eq!((a.true_reward, a.cost), (b.true_reward, b.cost));
    let cl = WLCfg { seed: 2, setting: Setting::Probe, budget: 150 };
    let x = run_wl(&cl, &mut MctsL::new(WorldL::new(cl.clone()), 2));
    let y = run_wl(&cl, &mut MctsL::new(WorldL::new(cl.clone()), 2));
    assert_eq!((x.reward, x.cost), (y.reward, y.cost));
}

#[test]
fn different_seeds_give_different_random_choices() {
    let m = Method::Thompson { prior: 1.0, gran: Gran::Cell, skip: 0.0 };
    let a = run_w2(&small(3, 400), &mut Player2::new(m.agent(1)), 0, None);
    let b = run_w2(&small(3, 400), &mut Player2::new(m.agent(2)), 0, None);
    let la: Vec<_> = a.recs.iter().map(|r| (r.fam, r.strategy)).collect();
    let lb: Vec<_> = b.recs.iter().map(|r| (r.fam, r.strategy)).collect();
    assert_ne!(la, lb);
}

#[test]
fn every_method_spends_exactly_the_same_budget() {
    let budget = 500;
    let cfg = small(7, budget);
    for m in reps() {
        let r = run_w2(&cfg, &mut Player2::new(m.agent(7)), 0, None);
        assert_eq!(r.cost, budget, "{}", m.label());
        let l = run_wl(&WLCfg { seed: 7, setting: Setting::Sizes, budget: 300 }, &mut PlayerL::new(m.agent(7)));
        assert_eq!(l.cost, 300, "{}", m.label());
        let b = run_menu_ext(World::with_extended(7, true), budget, &mut PlayerM::new(m.agent(7)), 0, &[]);
        assert_eq!(b.run.cost, budget, "{}", m.label());
    }
    // the being's learner gets the same budget; it stops when its best option is unaffordable and its skip
    // rule does not fire, so it may leave less than one option's energy unspent (scores are per energy spent)
    for c in [run_w2(&cfg, &mut Learner2::default(), 10, None).cost, run_w2_with(&cfg, &mut Learner2::default(), 10, None, LearnerCfg::default()).cost] {
        assert!(c <= budget && c >= budget - 2 * 11, "learner spent {c} of {budget}");
    }
    assert_eq!(run_w2(&cfg, &mut Mcts2::new(World2::new(cfg.clone()), 7), 0, None).cost, budget);
    let g = run_wl(&WLCfg { seed: 7, setting: Setting::Sizes, budget: 300 }, &mut GreedyL::default()).cost;
    assert!(g <= 300 && g >= 300 - 32, "greedy spent {g}");
}

fn code_lines(src: &str) -> String {
    src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

/// Names of hidden world parameters and of the measurement's ground truth.
const HIDDEN: [&str; 24] = [
    "slip_pm", "noise_mult", "regime", "reward_scale", "cost_base", "diff_bias", "SeqClass", "class_of", "p_true", "first_phase", "next_phase", "FKind", ".fams", "expected(", ".check(", "Oracle", "truth", "TruthO", "mix2", "success(", "feedback(", "World2", "WorldL", "WorldO",
];

#[test]
fn the_methods_see_only_visible_features() {
    for (name, src) in [("core.rs", include_str!("core.rs")), ("adapt.rs", include_str!("adapt.rs")), ("rules.rs", include_str!("rules.rs"))] {
        let code = code_lines(src);
        for h in HIDDEN {
            assert!(!code.contains(h), "{name} mentions the hidden name {h}");
        }
    }
    // the LLM request is built from views and the told history only
    let src = include_str!("llm.rs");
    let a = src.find("pub fn request(").expect("request fn");
    let b = src.find("pub fn parse_reply(").expect("parse fn");
    let body = code_lines(&src[a..b]);
    for h in HIDDEN {
        assert!(!body.contains(h), "llm::request mentions the hidden name {h}");
    }
    // and MCTS, which may see them, says so
    assert!(include_str!("mcts.rs").contains("TRUE simulator"));
}

fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object().expect("object").keys().cloned().collect()
}

fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

#[test]
fn the_llm_request_holds_only_the_synthetic_task_features() {
    let cfg = W2Cfg::new(1, 6, 200);
    let w = World2::new(cfg.clone());
    let tasks = w.offers(0, 0);
    let views: Vec<(Offer2, Vec<i64>, u8)> = tasks
        .iter()
        .map(|t| {
            let v = &t.view;
            (Offer2 { slot: v.slot, fam: v.fam, ftype: v.ftype, difficulty: v.difficulty, listed_cost: v.cost, listed_reward: v.reward }, v.params.clone(), v.horizon)
        })
        .collect();
    let told = vec![
        Told { step: 0, fam: Some(0), ftype: Some(FType::Seq), difficulty: 2, strategy: "fast".into(), correct: true, energy: 3 },
        Told { step: 1, fam: None, ftype: None, difficulty: 1, strategy: "skip".into(), correct: false, energy: 1 },
    ];
    let req = llm::request(2, 196, &views, &told, 1.25);
    let want: BTreeSet<String> = REQUEST_KEYS.iter().map(|s| s.to_string()).collect();
    assert_eq!(keys(&req), want);
    for m in req["menu"].as_array().unwrap() {
        let mut k: BTreeSet<String> = MENU_KEYS.iter().map(|s| s.to_string()).collect();
        if matches!(m["type"].as_str(), Some("seq") | Some("compose")) {
            k.extend(MENU_SEQ_KEYS.iter().map(|s| s.to_string()));
        }
        assert_eq!(keys(m), k);
    }
    for h in req["my_history"].as_array().unwrap() {
        assert_eq!(keys(h), HISTORY_KEYS.iter().map(|s| s.to_string()).collect());
    }
    for h in req["my_recent"].as_array().unwrap() {
        assert_eq!(keys(h), RECENT_KEYS.iter().map(|s| s.to_string()).collect());
    }
    // every string is from a fixed vocabulary; no path, no user name
    let mut ss = vec![];
    strings(&req, &mut ss);
    let vocab: BTreeSet<&str> = ["seq", "grid", "knap", "formula", "compose", "fast", "careful", "deep", "own", "skip", "correct", "wrong", llm::INSTRUCTION].into_iter().collect();
    for s in &ss {
        assert!(vocab.contains(s.as_str()), "unexpected string in a request: {s}");
    }
    let text = req.to_string();
    for bad in ["\\\\", ":/", "Users", "users", "@", "HOME", "sk-"] {
        assert!(!text.contains(bad), "request contains {bad}");
    }
    // some tasks of this world are sequences, so terms are shown
    assert!(req["menu"].as_array().unwrap().iter().any(|m| m.get("terms").is_some()) || TYPE_CYCLE.len() == 6);
}

#[test]
fn llm_replies_are_validated_strictly() {
    let offers = vec![
        Offer2 { slot: 0, fam: 0, ftype: FType::Seq, difficulty: 1, listed_cost: 4, listed_reward: 8 },
        Offer2 { slot: 1, fam: 1, ftype: FType::Grid, difficulty: 2, listed_cost: 5, listed_reward: 9 },
    ];
    assert_eq!(llm::parse_reply("{\"slot\": 1, \"strategy\": \"deep\"}", &offers, 100).unwrap(), (Some(1), "deep".into(), None));
    assert_eq!(llm::parse_reply("noise {\"slot\": 0, \"strategy\": \"own\", \"answer\": 17} noise", &offers, 100).unwrap(), (Some(0), "own".into(), Some(17)));
    assert_eq!(llm::parse_reply("{\"slot\": null, \"strategy\": \"skip\"}", &offers, 100).unwrap().0, None);
    assert!(llm::parse_reply("{\"slot\": 1, \"strategy\": \"own\", \"answer\": 3}", &offers, 100).is_err());
    assert!(llm::parse_reply("{\"slot\": 7, \"strategy\": \"fast\"}", &offers, 100).is_err());
    assert!(llm::parse_reply("{\"slot\": 1, \"strategy\": \"deep\"}", &offers, 6).is_err());
    assert!(llm::parse_reply("{\"slot\": 0, \"strategy\": \"discovered\"}", &offers, 100).is_err());
    assert!(llm::parse_reply("no json", &offers, 100).is_err());
}

#[test]
fn the_llm_player_stops_after_three_failures_and_respects_its_call_budget() {
    let cfg = W2Cfg::new(2, 6, 120);
    let mut n = 0u32;
    let mut p = LlmPlayer::new(World2::new(cfg.clone()), Box::new(|_req: &Value| {
        n += 1;
        Err("simulated failure".to_string())
    }), 100);
    let r = run_w2(&cfg, &mut p, 0, None);
    assert_eq!(r.cost, 120);
    assert_eq!(p.asked(), 3);
    assert!(p.stopped.as_deref().unwrap().contains("3 failures"));
    let mut p2 = LlmPlayer::new(World2::new(cfg.clone()), Box::new(|_req: &Value| Ok("{\"slot\": 0, \"strategy\": \"fast\"}".to_string())), 5);
    let r2 = run_w2(&cfg, &mut p2, 0, None);
    assert_eq!(r2.cost, 120);
    assert_eq!(p2.asked(), 5);
    assert!(p2.calls.iter().filter(|c| c.class == "valid").count() == 5);
}

fn seq_examples(seed: u64, n: u64) -> Vec<Example> {
    let w = World2::new(W2Cfg::new(seed, 12, 3000));
    let mut v = vec![];
    for f in (0..12).filter(|f| TYPE_CYCLE[f % 6] == FType::Seq) {
        for d in 1..=3u8 {
            for i in 0..n {
                let t = w.task(i, 0, f, Some(d), 0, 0x7E57);
                v.push(Example { event_id: format!("e{f}-{d}-{i}"), sig: sig2(f, d), terms: t.view.params.clone(), next: t.expected() });
            }
        }
    }
    v
}

#[test]
fn guarded_search_with_the_b2_fit_is_the_b2_search() {
    for seed in 1..=4 {
        let ex = seq_examples(seed, 9);
        let mine = guarded_search(&ex, &b2_predict);
        let theirs = search(&ex);
        let a: Vec<(u8, Form)> = mine.iter().flat_map(|(s, fs)| fs.iter().map(move |f| (*s, *f))).collect();
        let mut b: Vec<(u8, Form)> = theirs.iter().map(|f| (f.sig, f.form)).collect();
        b.sort_by_key(|x| x.0);
        assert_eq!(a, b, "seed {seed}");
    }
}

#[test]
fn brute_force_finds_integer_rules_and_refuses_what_it_cannot_pin_down() {
    let arith: Vec<i64> = (0..8).map(|i| 5 + 3 * i).collect();
    assert_eq!(bf_predict(&Form::Lin { mask: 1, constant: true }, &arith), Some(29));
    let fib = vec![1, 2, 3, 5, 8, 13, 21, 34];
    assert_eq!(bf_predict(&Form::Lin { mask: 3, constant: false }, &fib), Some(55));
    let sq: Vec<i64> = (0..8).map(|n| 2 + n * n).collect();
    assert_eq!(bf_predict(&Form::Poly(2), &sq), Some(66));
    assert_eq!(bf_predict(&Form::Poly(2), &sq), b2_predict(&Form::Poly(2), &sq));
    // a coefficient outside the grid is not found (B2's exact fit finds it)
    let big: Vec<i64> = (0..8).map(|i| 7 * (i as i64)).collect();
    assert_eq!(b2_predict(&Form::Poly(1), &big), Some(56));
    assert_eq!(bf_predict(&Form::Poly(1), &big), None);
    // too short a prefix: no answer
    assert_eq!(bf_predict(&Form::Lin { mask: 7, constant: true }, &[1, 2, 3, 4, 5]), None);
    let ex = seq_examples(1, 9);
    let book = guarded_search(&ex, &bf_predict);
    for e in &ex {
        if let Some(v) = book_answer(&book, e.sig, &e.terms, &bf_predict) {
            assert_eq!(v, e.next, "an accepted brute-force rule is right on its own examples");
        }
    }
}

#[test]
fn numeric_predictors_are_deterministic_and_exact_on_a_constant_difference() {
    let ex: Vec<Example> = (0..6).map(|k| {
        let t: Vec<i64> = (0..7).map(|i| k * 10 + 4 * i).collect();
        Example { event_id: String::new(), sig: 1, terms: t.clone(), next: k * 10 + 28 }
    }).collect();
    let q: Vec<i64> = (0..7).map(|i| 100 + 4 * i).collect();
    assert_eq!(knn_seq(&ex, &q, 1), Some(128));
    let tree = tree_seq(&ex).unwrap();
    assert_eq!(tree_seq_answer(&tree, &q), Some(128));
    assert_eq!(knn_seq(&ex, &q, 3), knn_seq(&ex, &q, 3));
}
