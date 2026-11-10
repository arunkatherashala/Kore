//! Track B6: transfer between domains, derived from the being's own log only.
//!
//! Question: when the being has learned in one domain, does what it learned speed up learning in a new
//! domain, and can it avoid transfer that hurts? Mechanism, in two parts.
//!
//! 1. A LINK (target domain, difficulty) <- (source domain, same difficulty). While a link is active, the
//!    cells of the target start from the posterior of the same (difficulty, strategy) cell of the source
//!    (provided that cell holds enough evidence) instead of from the strategy-wide estimate only. The
//!    arithmetic is in `model.rs`; this file decides WHEN a link is made.
//! 2. A GUARD in the model: after a few attempts the link influenced, the log-likelihood of the observed
//!    outcomes under the transferred prior is compared with that under the plain prior; a link that did
//!    worse by more than `GUARD_NATS` is switched off for good. Every link and every revocation is a log
//!    note plus a revision citing the events it rests on.
//!
//! WHEN (mode `On`): the link is made only on evidence read from logged outcomes: the revealed examples
//! (prefix and exact next term) of the target, tested against the rules the being
//! has ALREADY ACCEPTED (after full verification) in the source domain at the same difficulty, and the same
//! rules with the constant term toggled (a block shared up to an offset). The similarity is the share
//! of the target's examples on which the first applicable such rule gives the exact next term; there
//! must be no wrong application, at least `min_right` right ones, at least `min_examples` examples, and
//! the share must reach `tau`. Nothing here reads how tasks are generated or which domain is "meant" to
//! be similar to which: a test scans this file for those names.
//!
//! Control arms (measurement only, not offered by the tools): `WrongSim` links a new domain to the MOST
//! DISSIMILAR learned domain (largest mean distance between pooled success rates of the fixed
//! strategies, again from the log), with the guard on; `WrongSimNoGuard` does the same with the guard off.
//! They exist to show what bad transfer would cost and whether the guard stops it.

use super::discover::{self, Rule};
use super::solver::{Strategy, STRATEGIES};
use super::state::{BeingState, SOURCE};
use super::world::{iso_at, Domain, ALL_DOMAINS_X};
use crate::versioned::{event_id, EventRow, NewEvent, NewRevision};
use serde_json::{json, Value};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XferMode {
    #[default]
    Off,
    On,
    WrongSim,
    WrongSimNoGuard,
    /// Control: same trigger and same source domain as `On`, but each difficulty borrows the cell of the NEXT difficulty (cyclic): the right domain, the wrong cells.
    Shifted,
    /// As `Shifted`, with the guard off.
    ShiftedNoGuard,
}

impl XferMode {
    pub fn name(self) -> &'static str {
        match self {
            XferMode::Off => "off",
            XferMode::On => "on",
            XferMode::WrongSim => "wrong-sim",
            XferMode::WrongSimNoGuard => "wrong-sim-noguard",
            XferMode::Shifted => "shifted-cells",
            XferMode::ShiftedNoGuard => "shifted-cells-noguard",
        }
    }
}

/// Minimum share of the target's examples that a source rule must explain. Chosen on tuning seeds 101..120 only.
pub const TAU_DEFAULT: f64 = 0.5;
pub const MIN_EXAMPLES: usize = 3;
pub const MIN_RIGHT: u32 = 2;
/// A source domain must have this many attempts, and its cell at the difficulty this many, to count as learned.
pub const SRC_MIN_ATTEMPTS: f64 = 20.0;
pub const SRC_MIN_CELL: f64 = 4.0;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct XferCfg {
    pub mode: XferMode,
    pub tau: f64,
    pub min_examples: usize,
    pub min_right: u32,
    /// Whether the transferred search order is used (the search is the same in every other respect). Measurement switch.
    pub reorder: bool,
}

impl Default for XferCfg {
    fn default() -> XferCfg {
        XferCfg { mode: XferMode::Off, tau: TAU_DEFAULT, min_examples: MIN_EXAMPLES, min_right: MIN_RIGHT, reorder: true }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub enum XferKind {
    Link { guard: bool },
    Revoke { delta: f64, n: u32 },
}

/// One transfer decision (a link or a revocation) with its evidence, before it is written to the log.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct XferRec {
    pub kind: XferKind,
    pub index: usize,
    pub step: u64,
    pub clock: i64,
    pub target: Domain,
    pub difficulty: u8,
    pub src: Option<Domain>,
    /// Difficulty of the source cell lent to `difficulty`.
    pub src_k: u8,
    pub evidence: Value,
    pub cites: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct XferState {
    pub cfg: XferCfg,
    /// Links made (live or replayed).
    pub made: usize,
    /// Records produced by the engine so far, in order.
    pub log: Vec<XferRec>,
    /// Revocations noted by the model since the engine last looked (also filled during replay, where they are not written).
    pub pending: Vec<XferRec>,
    /// Records numbered so far (the index of the next record).
    pub next_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub src: Domain,
    /// None: the same difficulty as the target.
    pub src_k: Option<u8>,
    pub evidence: Value,
    pub cites: Vec<String>,
}

fn hypotheses(rules: &[Rule], sig: u8) -> Vec<(discover::Xf, discover::Form)> {
    let mut own: Vec<&Rule> = rules.iter().filter(|r| r.sig == sig && !r.retired).collect();
    own.sort_by_key(|r| (r.key(), r.id.clone()));
    let mut v: Vec<(discover::Xf, discover::Form)> = own.iter().map(|r| (r.xf, r.form)).collect();
    for r in &own {
        if let Some(f) = discover::toggle_constant(&r.form) {
            if !v.contains(&(r.xf, f)) {
                v.push((r.xf, f));
            }
        }
    }
    v
}

/// The source rules of one signature as hypotheses for another (same transforms and forms, plus the constant-toggled forms),
/// for the transferred search order.
pub fn preferred(rules: &[Rule], src_sig: u8, dst_sig: u8) -> Vec<(u8, discover::Xf, discover::Form)> {
    hypotheses(rules, src_sig).into_iter().map(|(x, f)| (dst_sig, x, f)).collect()
}

/// Rule-fit evidence of `target` at `difficulty` against the rules of `src`: (right, wrong, n examples, cited event ids).
pub fn rule_fit(st: &BeingState, target: Domain, difficulty: u8, src: Domain) -> (u32, u32, usize, Vec<String>) {
    let t_sig = discover::sig_of(target, difficulty);
    let hyps = hypotheses(&st.rules, discover::sig_of(src, difficulty));
    let exs: Vec<&discover::Example> = st.examples.iter().filter(|e| e.sig == t_sig).collect();
    let (mut right, mut wrong) = (0u32, 0u32);
    let mut cites = vec![];
    for e in &exs {
        let v = hyps.iter().find_map(|(x, f)| discover::predict_x(*x, f, &e.terms));
        match v {
            Some(v) if v == e.next => {
                right += 1;
                if cites.len() < discover::MAX_CITE {
                    cites.push(e.event_id.clone());
                }
            }
            Some(_) => wrong += 1,
            None => {}
        }
    }
    (right, wrong, exs.len(), cites)
}

fn pooled(st: &BeingState, d: Domain, s: Strategy) -> (f64, f64) {
    let (n, succ) = st.model.domain_strategy(d, s);
    (n, (succ + 1.0) / (n + 2.0))
}

/// Mean distance between the pooled success rates of the fixed strategies of two domains (None if no strategy is comparable).
pub fn profile_distance(st: &BeingState, a: Domain, b: Domain) -> Option<f64> {
    let mut tot = 0.0;
    let mut m = 0;
    for s in STRATEGIES {
        let ((na, pa), (nb, pb)) = (pooled(st, a, s), pooled(st, b, s));
        if na >= 2.0 && nb >= 2.0 {
            tot += (pa - pb).abs();
            m += 1;
        }
    }
    (m > 0).then(|| tot / m as f64)
}

struct Pooled {
    right: u32,
    wrong: u32,
    n: usize,
    cites: Vec<String>,
    per_k: Vec<Value>,
}

/// Rule-fit evidence of `target` against `src`, pooled over the difficulties.
fn pooled_fit(st: &BeingState, target: Domain, src: Domain) -> Pooled {
    let mut p = Pooled { right: 0, wrong: 0, n: 0, cites: vec![], per_k: vec![] };
    for k in 1..=5u8 {
        let (r, w, m, c) = rule_fit(st, target, k, src);
        p.right += r;
        p.wrong += w;
        p.n += m;
        for x in c {
            if p.cites.len() < discover::MAX_CITE {
                p.cites.push(x);
            }
        }
        p.per_k.push(json!({ "difficulty": k, "right": r, "wrong": w, "examples": m }));
    }
    p
}

fn passes(p: &Pooled, cfg: &XferCfg) -> bool {
    p.n >= cfg.min_examples && p.wrong == 0 && p.right >= cfg.min_right && (p.right as f64 / p.n as f64) >= cfg.tau
}

/// Which difficulties of domain `d` should be linked now, and to what? Pure function of the state (the log).
/// Mode `On` decides for the whole domain at once: the evidence is pooled over the difficulties for which `d` has
/// logged examples and the source has accepted rules; every difficulty at which the source cell holds evidence
/// and `d` has no link yet is then linked (the guard checks each difficulty on its own afterwards).
/// The control modes fire on exactly the same evidence (so at the same moments) but choose the source differently:
/// the learned domain with the largest distance between pooled success rates of the fixed strategies, leaving out
/// the target and, for the sequence domain, the domain whose rules explained it; they link the sequence domain
/// and the control domain.
pub fn decide(st: &BeingState, d: Domain) -> Vec<(u8, Decision)> {
    let cfg = st.xfer.cfg;
    let mut out = vec![];
    if cfg.mode == XferMode::Off || d == Domain::Pattern {
        return out;
    }
    let true_src = Domain::Pattern;
    let p = pooled_fit(st, Domain::PatternB, true_src);
    if !passes(&p, &cfg) {
        return out;
    }
    let (src, ev) = match cfg.mode {
        XferMode::On | XferMode::Shifted | XferMode::ShiftedNoGuard => {
            if d != Domain::PatternB {
                return out;
            }
            let ev = json!({
                "similarity": "share of the target's logged examples on which the first applicable rule of the source at the same difficulty (accepted there after full verification), or its constant-toggled form, gives the exact next term; pooled over the difficulties with examples",
                "source": true_src.name(), "right": p.right, "wrong": p.wrong, "examples": p.n, "share": p.right as f64 / p.n as f64, "tau": cfg.tau,
                "min_examples": cfg.min_examples, "min_right": cfg.min_right, "per_difficulty": p.per_k,
            });
            (true_src, ev)
        }
        _ => {
            let mut best: Option<(f64, Domain)> = None;
            for s in ALL_DOMAINS_X {
                if s == d || (d == Domain::PatternB && s == true_src) || st.model.domain_attempts(s) < SRC_MIN_ATTEMPTS {
                    continue;
                }
                if let Some(dist) = profile_distance(st, d, s) {
                    if best.map_or(true, |b| dist > b.0) {
                        best = Some((dist, s));
                    }
                }
            }
            let Some((dist, src)) = best else {
                return out;
            };
            let ev = json!({
                "similarity": "control arm: fires on the same evidence as the real rule, but links the learned domain with the LARGEST mean distance between pooled success rates of the fixed strategies (not the domain whose rules explained the target)",
                "source": src.name(), "mean_distance": dist, "trigger_right": p.right, "trigger_examples": p.n,
            });
            (src, ev)
        }
    };
    for k in 1..=5u8 {
        let sk = if matches!(cfg.mode, XferMode::Shifted | XferMode::ShiftedNoGuard) { k % 5 + 1 } else { k };
        if st.model.link(d, k).is_none() && st.model.cell_attempts(src, sk) >= SRC_MIN_CELL {
            out.push((k, Decision { src, src_k: if sk == k { None } else { Some(sk) }, evidence: ev.clone(), cites: p.cites.clone() }));
        }
    }
    out
}

fn dname(d: Option<Domain>) -> &'static str {
    d.map_or("-", |d| d.name())
}

/// The log records of one transfer decision: a note event and a revision of the belief `being/transfer/NNNN` citing the evidence.
pub fn records(r: &XferRec) -> (NewEvent, NewRevision) {
    let at = iso_at(r.clock);
    let (word, tag0) = match &r.kind {
        XferKind::Link { .. } => ("link", "transfer_link"),
        XferKind::Revoke { .. } => ("revoke", "transfer_revoke"),
    };
    let text = match &r.kind {
        XferKind::Link { guard } => format!(
            "transfer {} at step {}: the cells of domain {} at difficulty {} now start from the learned cells of domain {} at the same difficulty (guard {}); evidence {}",
            r.index, r.step, r.target.name(), r.difficulty, dname(r.src), if *guard { "on" } else { "off" }, r.evidence
        ),
        XferKind::Revoke { delta, n } => format!(
            "transfer {} at step {}: the link of domain {} at difficulty {} to domain {} is switched off: over {n} attempts it changed, the outcomes were less likely under the transferred prior than under the plain prior (log-likelihood difference {delta:.2}); the cells keep only their own evidence",
            r.index, r.step, r.target.name(), r.difficulty, dname(r.src)
        ),
    };
    let key = format!("being/transfer/{:04}", r.index);
    let mut tags = vec![tag0.to_string(), format!("xfer_index:{}", r.index), format!("step:{}", r.step), format!("domain:{}", r.target.name()), format!("difficulty:{}", r.difficulty), format!("from:{}", dname(r.src)), format!("from_k:{}", r.src_k), format!("rev_key:{key}")];
    if let XferKind::Link { guard } = &r.kind {
        tags.push(format!("guard:{}", if *guard { "on" } else { "off" }));
    }
    let ev = NewEvent { source: SOURCE.into(), occurred_at: at.clone(), kind: "note".into(), text, tags, redacted: false };
    let ev_id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    let mut caused_by = r.cites.clone();
    caused_by.push(ev_id);
    let rationale = json!({
        "action": word, "target": r.target.name(), "difficulty": r.difficulty, "source": dname(r.src), "evidence": r.evidence,
        "guard_nats": super::model::GUARD_NATS, "guard_min_attempts": super::model::GUARD_MIN_N, "cited_events": r.cites.len(),
        "effect": "the starting estimate of each cell (domain, difficulty, strategy) is the source's posterior for the same cell instead of the strategy-wide estimate; the cell's own evidence still counts in full",
    });
    let rev = NewRevision {
        entity_type: "belief".into(),
        key,
        stance: match &r.kind {
            XferKind::Link { .. } => format!("{} difficulty {} borrows its starting estimates from {}", r.target.name(), r.difficulty, dname(r.src)),
            XferKind::Revoke { .. } => format!("{} difficulty {} no longer borrows from {}", r.target.name(), r.difficulty, dname(r.src)),
        },
        confidence: 0.6,
        rationale: rationale.to_string(),
        supersedes: None,
        valid_from: at,
        author: "rule:being-transfer".into(),
        caused_by,
    };
    (ev, rev)
}

/// A link note read back from the log (replay): (target, difficulty, source, guard).
pub fn link_from_event(e: &EventRow) -> Option<(Domain, u8, Domain, u8, bool)> {
    use super::state::tag;
    if e.kind != "note" || !e.tags.split(',').any(|t| t == "transfer_link") {
        return None;
    }
    let k: u8 = tag(&e.tags, "difficulty")?.parse().ok()?;
    Some((Domain::parse(tag(&e.tags, "domain")?)?, k, Domain::parse(tag(&e.tags, "from")?)?, tag(&e.tags, "from_k").and_then(|x| x.parse().ok()).unwrap_or(k), tag(&e.tags, "guard") != Some("off")))
}
