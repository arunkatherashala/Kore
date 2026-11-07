//! The LLM player (Track V1): a language model chooses (slot, strategy) every step of a World v2 life from
//! the menu and a compact summary of ITS OWN past outcomes; for sequence and compose tasks it may instead
//! propose the answer itself (`strategy: "own"`).
//!
//! What is sent is built here and only here, from the synthetic task features (family index, type,
//! difficulty, listed cost and reward, the visible terms of sequence tasks, strategy names) and the
//! player's own told outcomes; the key sets are fixed and asserted by a test. No path, no user data and
//! no hidden world parameter is ever part of a request. The call itself (a program run by the harness) is
//! a closure, so this file contains no process, file or network code.
//!
//! Execution: an `own` answer is executed by the engine as `careful` (same energy: the listed cost), so the
//! life stays on the engine's exact path; whether the model's own answer was right is checked afterwards
//! against the checker, for the measurement only (never sent), and reported separately as a per-step
//! counterfactual. On a failed or invalid reply the step is played by a fixed fallback (best listed reward
//! per listed cost, careful) and counted; after 3 failures in a row, or when the call budget is used up,
//! the model is not asked again in that life.

use super::adapt::ctx2;
use crate::being::solver::{Strategy, STRATEGIES};
use crate::being::world2::engine::State2;
use crate::being::world2::learner::{plain, Act2, Choice2, Chooser2, Offer2};
use crate::being::world2::world::{FType, World2};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const MAX_FAILS_IN_A_ROW: u32 = 3;
pub const HISTORY_CELLS: usize = 24;
pub const RECENT: usize = 5;

pub const INSTRUCTION: &str = "You play a simulated task world. Each step you see a menu of tasks. Choose ONE task (slot) and ONE strategy, or skip the menu (slot null), to maximise total reward per energy over the whole life. A task pays its listed reward only if the answer is right. Strategies: fast costs half the listed cost, careful the listed cost, deep twice the listed cost; which strategy is right more often depends on the task family and you must learn it from your own history. For a seq or compose task you may instead give your own answer (strategy own, costs the listed cost): seq asks for the next term of terms, compose for the term steps_ahead places after the last term. Reply with ONE JSON object and nothing else: {\"slot\": <int or null>, \"strategy\": \"fast|careful|deep|own\", \"answer\": <int, only with own>}.";

/// Top-level keys of every request (asserted by a test).
pub const REQUEST_KEYS: [&str; 9] = ["instruction", "step", "budget_left", "skip_cost", "strategies", "menu", "my_history", "my_recent", "my_reward_per_energy"];
pub const MENU_KEYS: [&str; 6] = ["slot", "family", "type", "difficulty", "listed_cost", "listed_reward"];
pub const MENU_SEQ_KEYS: [&str; 2] = ["terms", "steps_ahead"];
pub const HISTORY_KEYS: [&str; 7] = ["family", "type", "difficulty", "strategy", "attempts", "told_correct", "energy"];
pub const RECENT_KEYS: [&str; 6] = ["step", "family", "difficulty", "strategy", "result", "energy"];

/// One step as the model's own history keeps it (what it was told).
#[derive(Debug, Clone, PartialEq)]
pub struct Told {
    pub step: u64,
    pub fam: Option<usize>,
    pub ftype: Option<FType>,
    pub difficulty: u8,
    /// fast / careful / deep / own / skip
    pub strategy: String,
    pub correct: bool,
    pub energy: i64,
}

/// One call as the measurement records it.
#[derive(Debug, Clone, PartialEq)]
pub struct CallRec {
    pub step: u64,
    /// valid / invalid / failed / not_asked
    pub class: String,
    pub detail: String,
    pub request: String,
    pub reply: String,
    pub choice: String,
    /// The model's own answer and whether the checker accepts it (measurement only; None: no own answer).
    pub own_right: Option<bool>,
    /// Listed reward and cost of the task it answered itself (for the counterfactual).
    pub own_task: Option<(i64, i64)>,
}

pub type Caller<'a> = Box<dyn FnMut(&Value) -> Result<String, String> + 'a>;

pub struct LlmPlayer<'a> {
    /// The world, for the visible task views (terms) and for checking own answers afterwards. Hidden
    /// parameters of it never enter a request (`request` takes only views and the told history).
    world: World2,
    call: Caller<'a>,
    pub max_calls: u64,
    pub calls: Vec<CallRec>,
    told: Vec<Told>,
    pending: Option<(Told, i64, i64)>,
    fails: u32,
    pub stopped: Option<String>,
}

pub fn type_name(t: FType) -> &'static str {
    t.name()
}

/// The request for one step: only synthetic task features and the player's own told outcomes.
pub fn request(step: u64, budget_left: i64, offers: &[(Offer2, Vec<i64>, u8)], told: &[Told], rate: f64) -> Value {
    let menu: Vec<Value> = offers
        .iter()
        .map(|(o, terms, h)| {
            let mut m = json!({ "slot": o.slot, "family": o.fam, "type": type_name(o.ftype), "difficulty": o.difficulty, "listed_cost": o.listed_cost, "listed_reward": o.listed_reward });
            if matches!(o.ftype, FType::Seq | FType::Compose) {
                m["terms"] = json!(terms);
                m["steps_ahead"] = json!(h);
            }
            m
        })
        .collect();
    // aggregated history per (family, difficulty, strategy), most recently used cells first
    let mut cells: BTreeMap<(usize, u8, String), (u64, u64, i64, u64, FType)> = BTreeMap::new();
    for t in told.iter().filter(|t| t.fam.is_some()) {
        let e = cells.entry((t.fam.unwrap(), t.difficulty, t.strategy.clone())).or_insert((0, 0, 0, 0, t.ftype.unwrap_or(FType::Seq)));
        e.0 += 1;
        e.1 += t.correct as u64;
        e.2 += t.energy;
        e.3 = t.step;
    }
    let mut hv: Vec<(&(usize, u8, String), &(u64, u64, i64, u64, FType))> = cells.iter().collect();
    hv.sort_by(|a, b| b.1 .3.cmp(&a.1 .3).then(a.0.cmp(b.0)));
    let history: Vec<Value> = hv
        .iter()
        .take(HISTORY_CELLS)
        .map(|(k, v)| json!({ "family": k.0, "type": type_name(v.4), "difficulty": k.1, "strategy": k.2, "attempts": v.0, "told_correct": v.1, "energy": v.2 }))
        .collect();
    let recent: Vec<Value> = told
        .iter()
        .rev()
        .take(RECENT)
        .map(|t| json!({ "step": t.step, "family": t.fam, "difficulty": t.difficulty, "strategy": t.strategy, "result": if t.strategy == "skip" { "skip" } else if t.correct { "correct" } else { "wrong" }, "energy": t.energy }))
        .collect();
    json!({
        "instruction": INSTRUCTION,
        "step": step,
        "budget_left": budget_left,
        "skip_cost": 1,
        "strategies": ["fast", "careful", "deep", "own"],
        "menu": menu,
        "my_history": history,
        "my_recent": recent,
        "my_reward_per_energy": (rate * 1000.0).round() / 1000.0,
    })
}

/// The model's reply, validated against the menu. Ok((slot or None for skip, strategy name, own answer)).
pub fn parse_reply(text: &str, offers: &[Offer2], budget_left: i64) -> Result<(Option<usize>, String, Option<i64>), String> {
    let start = text.find('{').ok_or("no JSON object in the reply")?;
    let end = text.rfind('}').ok_or("no JSON object in the reply")?;
    if end < start {
        return Err("no JSON object in the reply".into());
    }
    let v: Value = serde_json::from_str(&text[start..=end]).map_err(|e| format!("not valid JSON: {e}"))?;
    let slot = match &v["slot"] {
        Value::Null => None,
        x => Some(x.as_u64().ok_or("slot is not an integer or null")? as usize),
    };
    let strat = v["strategy"].as_str().unwrap_or("").to_lowercase();
    let Some(slot) = slot else {
        return Ok((None, "skip".into(), None));
    };
    let o = offers.iter().find(|o| o.slot == slot).ok_or("slot not on the menu")?;
    match strat.as_str() {
        "fast" | "careful" | "deep" => {
            let s = Strategy::parse(&strat).ok_or("unknown strategy")?;
            if s.energy(o.listed_cost) > budget_left {
                return Err("option not affordable".into());
            }
            Ok((Some(slot), strat, None))
        }
        "own" => {
            if !matches!(o.ftype, FType::Seq | FType::Compose) {
                return Err("own answer for a task that is not seq or compose".into());
            }
            let a = v["answer"].as_i64().ok_or("own answer without an integer answer")?;
            if o.listed_cost > budget_left {
                return Err("option not affordable".into());
            }
            Ok((Some(slot), strat, Some(a)))
        }
        "skip" => Ok((None, "skip".into(), None)),
        _ => Err("unknown strategy".into()),
    }
}

/// The fixed fallback: best listed reward per listed cost, careful (skip if none affordable).
pub fn fallback(offers: &[Offer2], budget_left: i64) -> (Option<usize>, String) {
    let mut best: Option<&Offer2> = None;
    for o in offers.iter().filter(|o| o.listed_cost <= budget_left) {
        if best.map_or(true, |b| o.listed_reward * b.listed_cost > b.listed_reward * o.listed_cost) {
            best = Some(o);
        }
    }
    match best {
        Some(o) => (Some(o.slot), "careful".into()),
        None => (None, "skip".into()),
    }
}

impl<'a> LlmPlayer<'a> {
    pub fn new(world: World2, call: Caller<'a>, max_calls: u64) -> LlmPlayer<'a> {
        LlmPlayer { world, call, max_calls, calls: vec![], told: vec![], pending: None, fails: 0, stopped: None }
    }
    pub fn asked(&self) -> u64 {
        self.calls.iter().filter(|c| c.class != "not_asked").count() as u64
    }
}

impl<'a> Chooser2 for LlmPlayer<'a> {
    fn name(&self) -> String {
        "llm-player".into()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let cx = ctx2(st);
        if let Some((mut t, r0, c0)) = self.pending.take() {
            t.correct = st.total_reward > r0;
            t.energy = st.total_cost - c0;
            self.told.push(t);
        }
        if st.budget_left <= 0 {
            return None;
        }
        // visible task views: the terms the solver would see (and the steps ahead of compose tasks)
        let tasks = self.world.offers(st.step, st.clock);
        let views: Vec<(Offer2, Vec<i64>, u8)> = offers.iter().map(|o| {
            let t = tasks.iter().find(|t| t.view.slot == o.slot).expect("the offer's task");
            (*o, if matches!(o.ftype, FType::Seq | FType::Compose) { t.view.params.clone() } else { vec![] }, t.view.horizon)
        }).collect();
        let mut rec = CallRec { step: st.step, class: "not_asked".into(), detail: String::new(), request: String::new(), reply: String::new(), choice: String::new(), own_right: None, own_task: None };
        let mut pick: Option<(Option<usize>, String, Option<i64>)> = None;
        if self.stopped.is_none() && self.asked() >= self.max_calls {
            self.stopped = Some("call budget used up".into());
        }
        if self.stopped.is_none() {
            let req = request(st.step, st.budget_left, &views, &self.told, cx.rate());
            rec.request = req.to_string();
            match (self.call)(&req) {
                Err(e) => {
                    rec.class = "failed".into();
                    rec.detail = e;
                }
                Ok(text) => {
                    rec.reply = text.chars().take(2000).collect();
                    match parse_reply(&text, offers, st.budget_left) {
                        Ok(p) => {
                            rec.class = "valid".into();
                            pick = Some(p);
                        }
                        Err(e) => {
                            rec.class = "invalid".into();
                            rec.detail = e;
                        }
                    }
                }
            }
            if rec.class == "valid" {
                self.fails = 0;
            } else {
                self.fails += 1;
                if self.fails >= MAX_FAILS_IN_A_ROW {
                    self.stopped = Some(format!("{MAX_FAILS_IN_A_ROW} failures in a row"));
                }
            }
        }
        let (slot, strat, own) = pick.unwrap_or_else(|| {
            let (s, n) = fallback(offers, st.budget_left);
            (s, n, None)
        });
        let o = slot.and_then(|s| offers.iter().find(|o| o.slot == s)).copied();
        if let (Some(a), Some(o)) = (own, o) {
            let t = tasks.iter().find(|t| t.view.slot == o.slot).expect("the offer's task");
            rec.own_right = Some(t.check(a));
            rec.own_task = Some((o.listed_reward, o.listed_cost));
        }
        rec.choice = match slot {
            None => "skip".into(),
            Some(s) => format!("{s}:{strat}"),
        };
        self.calls.push(rec);
        let act = match (o, strat.as_str()) {
            (None, _) => Act2::Skip,
            (Some(o), "own") => Act2::Attempt { slot: o.slot, strategy: Strategy::Careful },
            (Some(o), s) => Act2::Attempt { slot: o.slot, strategy: Strategy::parse(s).unwrap_or(Strategy::Careful) },
        };
        let told = Told { step: st.step, fam: o.map(|o| o.fam), ftype: o.map(|o| o.ftype), difficulty: o.map_or(1, |o| o.difficulty), strategy: if o.is_none() { "skip".into() } else { strat.clone() }, correct: false, energy: 0 };
        self.pending = Some((told, st.total_reward, st.total_cost));
        let _ = STRATEGIES;
        Some(plain("llm-player", "language model choice (or the fixed fallback)", act, 0.0, 0.0))
    }
}
