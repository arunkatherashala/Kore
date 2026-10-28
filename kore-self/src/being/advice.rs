//! Advice as an input (Track B, step B4): an outside source (a program the owner configures, or in
//! all measurements a simulated one of known quality) may suggest what to do; the being treats the
//! suggestion as ONE INPUT to a mind that learns from its own experience.
//!
//! * `AdviceRequest` holds only what the being may share: for each offered task its domain,
//!   difficulty, listed cost and listed reward (for sequence tasks also the visible terms), the
//!   strategy names and which rule signatures it would like proposals for. It has no field for
//!   anything hidden and none for log contents; a test scans this file for the hidden names.
//! * `Advice` is structured: a preferred option (slot + strategy, optional stated success
//!   probability) and/or a proposed rule (a form of the B2 grammar, as code or DSL text).
//! * Advice is never executed blindly in the trusting configuration: the option enters the planner as
//!   an extra prior weighted by learned trust (`planner.rs`, `trust.rs`); a proposed rule is only
//!   used if the B2 verified search accepts it on the being's own logged examples.
//! * Every piece of advice is a logged event (source `advisor`) carrying the request and the advice
//!   and an id derived from the content. What happened afterwards is logged too.
//!
//! `SimulatedAdvisor` is the advisor of every measurement and test. Its quality knob `q` is the
//! probability that it gives the oracle-correct suggestion (the best option by true reward per
//! energy); otherwise it gives a plausible wrong one. The ranking it consults is handed in by the
//! caller as a function (`Ranker`); this file never touches the world. The simulated quality is a
//! property of the simulation, NOT a statement about any real model.

use super::discover::{forms, Form};
use super::solver::Strategy;
use super::state::tag;
use super::world::{Domain, N_DOM};
use crate::versioned::{event_id, NewEvent};
use serde_json::{json, Value};

pub const ADVISOR_SOURCE: &str = "advisor";
/// Kind name used for rule proposals in the trust table.
pub const RULE_KIND: &str = "rule";
/// A proposal for rules is requested every this many steps (extended pattern world only).
pub const RULE_EVERY: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdvisorKind {
    #[default]
    None,
    Simulated,
    Command,
}

impl AdvisorKind {
    pub fn name(self) -> &'static str {
        match self {
            AdvisorKind::None => "none",
            AdvisorKind::Simulated => "simulated",
            AdvisorKind::Command => "command",
        }
    }
    pub fn parse(s: &str) -> Option<AdvisorKind> {
        match s {
            "none" => Some(AdvisorKind::None),
            "simulated" => Some(AdvisorKind::Simulated),
            "command" => Some(AdvisorKind::Command),
            _ => None,
        }
    }
}

/// Creation-time advice settings of a being (part of its genesis event).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdviceSetup {
    pub kind: AdvisorKind,
    /// Simulated advisor only: probability of the oracle-correct suggestion.
    pub quality: f64,
    /// true: the being learns how far to trust the advisor; false: it follows advice blindly.
    pub trust: bool,
}

impl AdviceSetup {
    pub fn is_none(&self) -> bool {
        self.kind == AdvisorKind::None
    }
}

impl Default for AdviceSetup {
    fn default() -> Self {
        AdviceSetup { kind: AdvisorKind::None, quality: 0.0, trust: true }
    }
}

/// What the being tells an advisor about one offered task.
#[derive(Debug, Clone, PartialEq)]
pub struct OfferView {
    pub slot: usize,
    pub domain: String,
    pub difficulty: u8,
    pub listed_cost: i64,
    pub listed_reward: i64,
    /// The visible terms of a sequence task (extended sequence world only).
    pub terms: Option<Vec<i64>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdviceRequest {
    pub step: u64,
    pub offers: Vec<OfferView>,
    pub strategies: Vec<String>,
    /// Difficulties of sequence tasks for which a rule proposal is welcome.
    pub rule_sigs_wanted: Vec<u8>,
}

impl AdviceRequest {
    pub fn to_json(&self) -> Value {
        let offers: Vec<Value> = self
            .offers
            .iter()
            .map(|o| {
                let mut v = json!({ "slot": o.slot, "domain": o.domain, "difficulty": o.difficulty, "listed_cost": o.listed_cost, "listed_reward": o.listed_reward });
                if let Some(t) = &o.terms {
                    v["terms"] = json!(t);
                }
                v
            })
            .collect();
        let mut v = json!({
            "task": "kore-being-advice", "version": 1,
            "instructions": "Reply with ONE JSON object. Optional fields: slot (integer, the offer to attempt) with strategy (one of strategies); p_success (0..1, your estimate that the attempt is right); rule {signature: integer from rule_signatures_wanted, form: one code or dsl text from rule_grammar}; note (short text). The goal is reward per energy; strategy energy: fast about half of listed_cost, careful and discovered listed_cost, deep twice listed_cost.",
            "step": self.step, "offers": offers, "strategies": self.strategies, "rule_signatures_wanted": self.rule_sigs_wanted,
        });
        if !self.rule_sigs_wanted.is_empty() {
            v["rule_grammar"] = json!(forms().iter().map(|f| json!({ "code": f.code(), "dsl": f.dsl() })).collect::<Vec<_>>());
        }
        v
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuAdvice {
    pub slot: usize,
    pub strategy: Strategy,
    pub p_success: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleAdvice {
    pub sig: u8,
    pub form: Form,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Advice {
    pub menu: Option<MenuAdvice>,
    pub rule: Option<RuleAdvice>,
    pub note: String,
    /// Set when the advisor failed (timeout, bad reply, ...): no usable advice, the reason is logged.
    pub error: Option<String>,
    /// (prompt, reply) as sent and received by a command advisor, already redacted.
    pub transcript: Option<(String, String)>,
}

impl Advice {
    pub fn to_json(&self) -> Value {
        let mut v = json!({});
        if let Some(m) = &self.menu {
            v["slot"] = json!(m.slot);
            v["strategy"] = json!(m.strategy.name());
            if let Some(p) = m.p_success {
                v["p_success"] = json!(p);
            }
        }
        if let Some(r) = &self.rule {
            v["rule"] = json!({ "signature": r.sig, "form": r.form.code(), "dsl": r.form.dsl() });
        }
        if !self.note.is_empty() {
            v["note"] = json!(self.note);
        }
        if let Some(e) = &self.error {
            v["error"] = json!(e);
        }
        v
    }

    /// Strict parse of an advisor reply against the request it answers. Everything is validated; any
    /// problem is an error (the caller logs it and goes on without advice).
    pub fn parse_reply(bytes: &[u8], req: &AdviceRequest) -> Result<Advice, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("the reply is not valid JSON: {e}"))?;
        let o = v.as_object().ok_or("the reply must be a JSON object")?;
        let mut a = Advice::default();
        match (o.get("slot").filter(|x| !x.is_null()), o.get("strategy").filter(|x| !x.is_null())) {
            (None, None) => {}
            (Some(s), Some(t)) => {
                let slot = s.as_u64().ok_or("'slot' must be a non-negative integer")? as usize;
                if slot >= req.offers.len() {
                    return Err(format!("'slot' {slot} is not one of the {} offers", req.offers.len()));
                }
                let name = t.as_str().ok_or("'strategy' must be a string")?;
                if !req.strategies.iter().any(|x| x == name) {
                    return Err(format!("'strategy' must be one of {:?}", req.strategies));
                }
                let strategy = Strategy::parse(name).ok_or("unknown strategy")?;
                let p_success = match o.get("p_success") {
                    None | Some(Value::Null) => None,
                    Some(p) => Some(p.as_f64().filter(|p| (0.0..=1.0).contains(p)).ok_or("'p_success' must be a number in 0..1")?),
                };
                a.menu = Some(MenuAdvice { slot, strategy, p_success });
            }
            _ => return Err("'slot' and 'strategy' must be given together".into()),
        }
        if let Some(r) = o.get("rule").filter(|x| !x.is_null()) {
            let sig = r.get("signature").and_then(|x| x.as_u64()).filter(|s| (1..=5).contains(s)).ok_or("'rule.signature' must be an integer 1..5")? as u8;
            if !req.rule_sigs_wanted.contains(&sig) {
                return Err(format!("'rule.signature' {sig} was not asked for"));
            }
            let text = r.get("form").and_then(|x| x.as_str()).ok_or("'rule.form' must be a string (form code or DSL text)")?;
            let form = parse_form_text(text).ok_or_else(|| format!("'rule.form' is not a form of the grammar: {}", cut(text, 60)))?;
            a.rule = Some(RuleAdvice { sig, form });
        }
        match o.get("note") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) => a.note = cut(s.trim(), 300),
            Some(_) => return Err("'note' must be a string".into()),
        }
        if a.menu.is_none() && a.rule.is_none() && a.note.is_empty() {
            return Err("the reply names no option, no rule and has no note".into());
        }
        Ok(a)
    }
}

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// A form of the B2 grammar from its code (`lin.12.c`) or its DSL text (`next = a1*t[-1] + b`).
pub fn parse_form_text(s: &str) -> Option<Form> {
    let s = s.trim();
    if let Some(f) = Form::parse(s) {
        return Some(f);
    }
    let norm = |t: &str| -> String { t.split('(').next().unwrap_or("").chars().filter(|c| !c.is_whitespace()).collect() };
    let want = norm(s);
    if want.is_empty() {
        return None;
    }
    forms().into_iter().find(|f| norm(&f.dsl()) == want)
}

pub trait Advisor {
    /// Stable identity of the advisor (goes into the log and keys the trust table).
    fn id(&self) -> String;
    fn advise(&self, req: &AdviceRequest) -> Advice;
}

// ---------------------------------------------------------------------------------------------
// The simulated advisor

/// One option as the hidden ranking sees it.
#[derive(Debug, Clone, Copy)]
pub struct Ranked {
    pub slot: usize,
    pub strategy: Strategy,
    /// True expected reward per energy.
    pub value: f64,
    /// True success probability of the option.
    pub p: f64,
}

/// Options of a request ordered best first by true reward per energy. Supplied by the measurement code.
pub type Ranker = Box<dyn Fn(&AdviceRequest) -> Vec<Ranked>>;

pub struct SimulatedAdvisor {
    pub seed: u64,
    /// Probability of the oracle-correct suggestion.
    pub q: f64,
    /// Optional quality per domain (indexed like `Domain::idx`), replacing `q`.
    pub q_by_domain: Option<[f64; N_DOM]>,
    /// Always states a success probability of 0.95, right or wrong.
    pub confidently_wrong: bool,
    /// From this step on the quality is the second value (a mid-life degradation).
    pub degrade: Option<(u64, f64)>,
    ranker: Ranker,
    /// Correct (signature, form code) pairs the advisor knows, for rule proposals.
    rule_table: Vec<(u8, String)>,
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

struct Stream(u64);
impl Stream {
    fn new(parts: &[u64]) -> Stream {
        Stream(parts.iter().fold(0x1234_5678_9ABC_DEF1u64, |h, p| splitmix(h ^ splitmix(*p))))
    }
    fn next(&mut self) -> u64 {
        self.0 = splitmix(self.0);
        self.0
    }
    fn f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

impl SimulatedAdvisor {
    pub fn new(seed: u64, q: f64, ranker: Ranker) -> SimulatedAdvisor {
        SimulatedAdvisor { seed, q: q.clamp(0.0, 1.0), q_by_domain: None, confidently_wrong: false, degrade: None, ranker, rule_table: vec![] }
    }

    pub fn with_rules(mut self, table: Vec<(u8, String)>) -> SimulatedAdvisor {
        self.rule_table = table;
        self
    }

    /// Quality that applies to a request at `step` whose best option is in `domain`.
    pub fn quality_at(&self, step: u64, domain: Option<Domain>) -> f64 {
        if let Some((at, q2)) = self.degrade {
            if step >= at {
                return q2;
            }
        }
        match (self.q_by_domain, domain) {
            (Some(t), Some(d)) => t[d.idx()],
            _ => self.q,
        }
    }
}

impl Advisor for SimulatedAdvisor {
    fn id(&self) -> String {
        format!("simulated:q{:.2}{}", self.q, if self.confidently_wrong { "cw" } else { "" })
    }

    fn advise(&self, req: &AdviceRequest) -> Advice {
        let mut adv = Advice::default();
        let ranked = (self.ranker)(req);
        let mut rng = Stream::new(&[self.seed, req.step, 0xAD71]);
        if let Some(best) = ranked.first() {
            let dom = req.offers.get(best.slot).and_then(|o| Domain::parse(&o.domain));
            let q = self.quality_at(req.step, dom);
            let right = rng.f64() < q;
            let pick = if right {
                *best
            } else {
                // plausible wrong suggestion: an option worth less than 90% of the best (else the runner-up)
                let worse: Vec<&Ranked> = ranked[1..].iter().filter(|r| r.value < 0.9 * best.value).collect();
                if worse.is_empty() {
                    ranked.get(1).copied().unwrap_or(*best)
                } else {
                    *worse[rng.below(worse.len())]
                }
            };
            let stated = if self.confidently_wrong { 0.95 } else { (pick.p * 100.0).round() / 100.0 };
            adv.menu = Some(MenuAdvice { slot: pick.slot, strategy: pick.strategy, p_success: Some(stated) });
        }
        if !req.rule_sigs_wanted.is_empty() && !self.rule_table.is_empty() {
            let known: Vec<u8> = req.rule_sigs_wanted.iter().copied().filter(|s| self.rule_table.iter().any(|(k, _)| k == s)).collect();
            if !known.is_empty() {
                let sig = known[rng.below(known.len())];
                let good: Vec<&String> = self.rule_table.iter().filter(|(k, _)| *k == sig).map(|(_, c)| c).collect();
                let q = self.quality_at(req.step, None);
                let form = if rng.f64() < q {
                    Form::parse(good[rng.below(good.len())])
                } else {
                    let bad: Vec<Form> = forms().into_iter().filter(|f| !good.contains(&&f.code())).collect();
                    Some(bad[rng.below(bad.len())])
                };
                if let Some(form) = form {
                    adv.rule = Some(RuleAdvice { sig, form });
                }
            }
        }
        adv
    }
}

// ---------------------------------------------------------------------------------------------
// Log records

/// The option an advisor named, as the log records it.
#[derive(Debug, Clone, PartialEq)]
pub struct AdvisedOption {
    pub slot: usize,
    pub strategy: Strategy,
    pub task_id: String,
    pub domain: Domain,
    pub difficulty: u8,
    pub p_stated: Option<f64>,
}

/// Advice read from its log event; waits in the state until the step's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingAdvice {
    pub id: String,
    pub advisor: String,
    pub step: u64,
    pub menu: Option<AdvisedOption>,
}

/// Build the advice event of one step. Returns the event, its content-derived id and the pending advice.
pub fn advice_event(advisor: &str, at: String, req: &AdviceRequest, advice: &Advice, task_ids: &[String]) -> (NewEvent, String, PendingAdvice) {
    let mut body = json!({ "advisor": advisor, "step": req.step, "request": req.to_json(), "advice": advice.to_json() });
    if let Some((p, r)) = &advice.transcript {
        body["transcript"] = json!({ "prompt": p, "reply": r });
    }
    let mut tags = vec![format!("step:{}", req.step), format!("advisor:{advisor}")];
    if let Some(m) = &advice.menu {
        let o = &req.offers[m.slot];
        tags.extend([
            format!("slot:{}", m.slot),
            format!("strategy:{}", m.strategy.name()),
            format!("task:{}", task_ids.get(m.slot).cloned().unwrap_or_default()),
            format!("domain:{}", o.domain),
            format!("difficulty:{}", o.difficulty),
        ]);
        if let Some(p) = m.p_success {
            tags.push(format!("p_stated:{p:.3}"));
        }
    }
    if let Some(r) = &advice.rule {
        tags.extend([format!("rule_sig:{}", r.sig), format!("rule_form:{}", r.form.code())]);
    }
    if advice.error.is_some() {
        tags.push("error:1".into());
    }
    tags.push("ev:advice".into());
    let ev = NewEvent { source: ADVISOR_SOURCE.into(), occurred_at: at, kind: "statement".into(), text: body.to_string(), tags, redacted: false };
    let id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    let pending = pending_from_tags(&id, &ev.tags.join(",")).expect("advice event tags are well formed");
    (ev, id, pending)
}

/// The advisor event type (advice, verdict, result, alarm) of an event of source `advisor`; the log kinds are only note/statement.
pub fn ev_kind(tags: &str) -> Option<&str> {
    tag(tags, "ev")
}

/// Read the pending advice back from the tags of an advice event.
pub fn pending_from_tags(id: &str, tags: &str) -> Option<PendingAdvice> {
    let step: u64 = tag(tags, "step")?.parse().ok()?;
    let advisor = tag(tags, "advisor")?.to_string();
    let menu = match (tag(tags, "slot"), tag(tags, "strategy")) {
        (Some(slot), Some(strategy)) => Some(AdvisedOption {
            slot: slot.parse().ok()?,
            strategy: Strategy::parse(strategy)?,
            task_id: tag(tags, "task")?.to_string(),
            domain: Domain::parse(tag(tags, "domain")?)?,
            difficulty: tag(tags, "difficulty")?.parse().ok()?,
            p_stated: tag(tags, "p_stated").and_then(|p| p.parse().ok()),
        }),
        _ => None,
    };
    Some(PendingAdvice { id: id.to_string(), advisor, step, menu })
}

/// What the planner did with the advice of one step.
#[derive(Debug, Clone, PartialEq)]
pub struct AdviceTrace {
    pub id: String,
    pub advisor: String,
    /// "off" (advice seen but not used), "blind" or "trust".
    pub mode: &'static str,
    /// Label of the advised action ("slot:strategy"), or "-" when advice named no usable option.
    pub advised: String,
    /// The advised option could be attempted (exists, affordable).
    pub valid: bool,
    /// What the model alone would have done, and the reward per energy it predicted for its own best option.
    pub alone: String,
    pub ref_score: f64,
    /// The decision equals the model-alone decision.
    pub agree: bool,
    /// The advised option was chosen.
    pub followed: bool,
    /// Mixing weight used (1 for blind following, 0 when ignored).
    pub w: f64,
    pub trust_mean: f64,
    pub trust_sd: f64,
}

impl AdviceTrace {
    pub fn round6(x: f64) -> f64 {
        (x * 1e6).round() / 1e6
    }

    /// Tags of the decision event (parsed back by the state; compared by replay).
    pub fn tags(&self) -> Vec<String> {
        vec![
            format!("advice:{}", self.id),
            format!("adv_followed:{}", self.followed as u8),
            format!("adv_agree:{}", self.agree as u8),
            format!("adv_ref:{:.6}", self.ref_score),
            format!("adv_w:{:.3}", self.w),
        ]
    }

    pub fn to_json(&self) -> Value {
        json!({
            "advice_id": self.id, "advisor": self.advisor, "mode": self.mode, "advised": self.advised, "advised_option_usable": self.valid,
            "model_alone_would_choose": self.alone, "model_alone_predicted_reward_per_energy": self.ref_score,
            "decision_equals_model_alone": self.agree, "advice_followed": self.followed, "mixing_weight": (self.w * 1000.0).round() / 1000.0,
            "trust_mean": (self.trust_mean * 1000.0).round() / 1000.0, "trust_sd": (self.trust_sd * 1000.0).round() / 1000.0,
        })
    }
}

/// What the state keeps of the decision about the pending advice.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionAdvice {
    pub id: String,
    pub followed: bool,
    pub agree: bool,
    pub ref_score: f64,
    pub w: f64,
}

pub fn decision_from_tags(tags: &str) -> Option<DecisionAdvice> {
    Some(DecisionAdvice {
        id: tag(tags, "advice")?.to_string(),
        followed: tag(tags, "adv_followed")? == "1",
        agree: tag(tags, "adv_agree")? == "1",
        ref_score: tag(tags, "adv_ref")?.parse().ok()?,
        w: tag(tags, "adv_w")?.parse().ok()?,
    })
}

/// Compact text of the advice part of a decision, used to compare a recorded decision with its replay.
pub fn trace_suffix(followed: bool, agree: bool, ref_s: &str, w: &str) -> String {
    format!("|adv f{} a{} r{ref_s} w{w}", followed as u8, agree as u8)
}

pub fn suffix_of_tags(tags: &str) -> Option<String> {
    Some(trace_suffix(tag(tags, "adv_followed")? == "1", tag(tags, "adv_agree")? == "1", tag(tags, "adv_ref")?, tag(tags, "adv_w")?))
}

pub fn suffix_of_trace(t: &AdviceTrace) -> String {
    trace_suffix(t.followed, t.agree, &format!("{:.6}", t.ref_score), &format!("{:.3}", t.w))
}

/// Verdict on a proposed rule (accepted or rejected by the verified search, or already held).
pub fn verdict_event(advisor: &str, at: String, advice_id: &str, sig: u8, form: &Form, verdict: &str, detail: &str) -> NewEvent {
    NewEvent {
        source: ADVISOR_SOURCE.into(),
        occurred_at: at,
        kind: "note".into(),
        text: format!(
            "advisor {advisor} proposed for sequence tasks of difficulty {sig} the rule `{}` ({}); the being's verified search over its own logged examples says: {verdict}. {detail}",
            form.dsl(),
            form.code()
        ),
        tags: vec!["ev:verdict".into(), format!("advice:{advice_id}"), format!("advisor:{advisor}"), format!("verdict:{verdict}"), format!("sig:{sig}"), format!("form:{}", form.code())],
        redacted: false,
    }
}

/// What happened after the advice of one step, in words, with the numbers the being has now.
pub fn result_event(at: String, trace: &AdviceTrace, step: u64, text: String, result: &str) -> NewEvent {
    NewEvent {
        source: ADVISOR_SOURCE.into(),
        occurred_at: at,
        kind: "note".into(),
        text,
        tags: vec!["ev:result".into(), format!("advice:{}", trace.id), format!("advisor:{}", trace.advisor), format!("step:{step}"), format!("result:{result}")],
        redacted: false,
    }
}

pub fn alarm_event(at: String, step: u64, a: &super::trust::TrustAlarm) -> NewEvent {
    NewEvent {
        source: ADVISOR_SOURCE.into(),
        occurred_at: at,
        kind: "note".into(),
        text: format!(
            "advisor {} looks less reliable than before for {} advice (step {step}): its advice beat the being's own model in fewer tests than expected (trust was {:.2} after {} tests; CUSUM statistic {:.2}); the older evidence is cut to {:.0}% of its weight and the advisor will be tested again",
            a.advisor, a.kind, a.mean_before, a.tests, a.statistic, 100.0 * super::trust::KEEP_AFTER_ALARM
        ),
        tags: vec!["ev:alarm".into(), format!("advisor:{}", a.advisor), format!("kind:{}", a.kind), format!("step:{step}"), "advisor_alarm".into()],
        redacted: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> AdviceRequest {
        AdviceRequest {
            step: 3,
            offers: (0..4).map(|i| OfferView { slot: i, domain: "algebra".into(), difficulty: 2, listed_cost: 4, listed_reward: 10 + i as i64, terms: None }).collect(),
            strategies: vec!["fast".into(), "careful".into(), "deep".into()],
            rule_sigs_wanted: vec![3],
        }
    }

    #[test]
    fn reply_parsing_is_strict() {
        let r = req();
        let a = Advice::parse_reply(br#"{"slot":2,"strategy":"deep","p_success":0.8,"rule":{"signature":3,"form":"poly.2"},"note":"x"}"#, &r).unwrap();
        assert_eq!(a.menu.unwrap().slot, 2);
        assert_eq!(a.rule.unwrap().form, Form::Poly(2));
        for bad in [
            r#"{"slot":9,"strategy":"deep"}"#,
            r#"{"slot":1,"strategy":"slow"}"#,
            r#"{"slot":1}"#,
            r#"{"slot":1,"strategy":"fast","p_success":1.5}"#,
            r#"{"rule":{"signature":2,"form":"poly.2"}}"#,
            r#"{"rule":{"signature":3,"form":"cubic spline"}}"#,
            r#"{}"#,
            r#"[]"#,
            "not json",
        ] {
            assert!(Advice::parse_reply(bad.as_bytes(), &r).is_err(), "{bad}");
        }
        assert_eq!(parse_form_text("next = a1*t[-1] + a2*t[-2] + b").map(|f| f.code()), Some("lin.12.c".into()));
        assert_eq!(parse_form_text("lin.1"), Some(Form::Lin { mask: 1, constant: false }));
    }

    #[test]
    fn request_json_has_only_shareable_fields() {
        let j = req().to_json();
        let keys: Vec<&str> = j.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        for k in keys {
            assert!(["task", "version", "instructions", "step", "offers", "strategies", "rule_signatures_wanted", "rule_grammar"].contains(&k), "{k}");
        }
        for o in j["offers"].as_array().unwrap() {
            let ks: Vec<&str> = o.as_object().unwrap().keys().map(|k| k.as_str()).collect();
            assert_eq!(ks.len(), 5, "{ks:?}");
        }
    }

    #[test]
    fn simulated_advisor_is_deterministic_and_q_is_respected() {
        let ranker = || -> Ranker {
            Box::new(|r: &AdviceRequest| {
                let mut v: Vec<Ranked> = r.offers.iter().flat_map(|o| [Strategy::Fast, Strategy::Careful, Strategy::Deep].map(|s| Ranked { slot: o.slot, strategy: s, value: o.listed_reward as f64 / (s.energy(o.listed_cost) as f64), p: 0.9 })).collect();
                v.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap());
                v
            })
        };
        let a = SimulatedAdvisor::new(7, 0.7, ranker());
        let b = SimulatedAdvisor::new(7, 0.7, ranker());
        let (mut hits, n) = (0, 2000);
        for step in 0..n {
            let mut r = req();
            r.step = step;
            assert_eq!(a.advise(&r), b.advise(&r));
            let best = ranker()(&r)[0];
            let m = a.advise(&r).menu.unwrap();
            hits += (m.slot == best.slot && m.strategy == best.strategy) as u32;
        }
        let rate = hits as f64 / n as f64;
        assert!((rate - 0.7).abs() < 0.05, "rate {rate}");
        let always = SimulatedAdvisor::new(1, 1.0, ranker());
        assert_eq!(always.advise(&req()).menu.unwrap().slot, 3);
    }
}
