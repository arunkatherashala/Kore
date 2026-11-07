//! Experiments (Track B9): the being chooses some tasks on purpose to test its own hypotheses.
//!
//! Without this file the rule search (`discover.rs`) learns only from whatever examples the
//! planner's reward-seeking choices happen to reveal. Here, for every signature the being keeps the
//! set of LIVE HYPOTHESES: every candidate (transform, form) of the search space that has never been
//! applicable-and-wrong on a revealed example of that signature (exactly the hypotheses the search
//! could still accept). Each gets a weight
//!
//! ```text
//! w(h) = 2^-(size + transform cost) * NA_LIKELIHOOD^(examples of the signature on which h was not applicable)
//! ```
//!
//! (simplest first; a hypothesis that keeps failing to apply to its own signature's prefixes loses
//! weight; a wrong prediction removes it). For an offered sequence task the being SEES the prefix
//! (the prompt), so it can compute what every live hypothesis predicts for it before choosing. The
//! predictions split the hypotheses into groups G_v (predict v) and N (not applicable). If the right hypothesis
//! is in G_v the revealed answer is v, which removes the other groups; if it is in N the answer (almost
//! surely) matches no group and removes all of them. The expected information gain in bits is then
//!
//! ```text
//! EIG = sum_v P(G_v) * log2(1 / (P(G_v) + P(N)))  +  P(N) * log2(1 / P(N))
//! ```
//!
//! plus a SUPPORT term: the expected share of the remaining minimum support (K_TRAIN training and
//! K_VAL held-out reproductions) that this example would supply to the hypothesis that turns out
//! true (the acceptance rule needs support, not only elimination). value = EIG + support, in bits.
//! A signature whose own accepted rule applies to the prefix has value 0 (the being already predicts it).
//!
//! The planner (unchanged) proposes its best option. The experimenter turns that value into reward
//! units, `VOI = kappa * value * listed_reward`, and for each offered sequence task and strategy scores
//! `(p * listed_reward + VOI) / estimated_energy` with the planner's own estimates. It runs the
//! experiment only if that beats the best EXPLOIT option (highest p * reward / energy of all
//! affordable options) and the experiment budget allows it (energy share of the life budget, number
//! of experiments). The budget comes from the owner's controller.txt (fail closed: no line, no experiments).
//! Acceptance of rules is NOT changed: the same search, the same support, the same held-out split,
//! the same never-applicable-and-wrong guard; experiments only change which examples are revealed.
//!
//! A control ("random probe") with the same kind of budget picks a uniformly random offered sequence
//! task instead of the most informative one (seeded, deterministic).
//!
//! Everything here is a pure function of the revealed examples, the accepted rules, the visible
//! offers and the planner's estimates: no hidden world parameter (a test scans this file).

use super::discover::{self, candidates_for, is_held_out, predict_x, Example, Form, Rule, Xf, K_TRAIN, K_VAL};
use super::solver::Strategy;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Likelihood factor of one example on which a hypothesis was not applicable.
pub const NA_LIKELIHOOD: f64 = 0.5;
/// Reward units per bit of value, as a multiple of the offer's listed reward. Chosen on tuning seeds 101..120 only (docs/measured.md, Track B9).
pub const DEFAULT_KAPPA: f64 = 0.5;
/// Hypotheses shown in a decision record (the heaviest first); the counts always cover all of them.
pub const SHOW_HYPS: usize = 8;

/// Settings of the experimenter. `energy_pm` and `max_experiments` are the budget (from controller.txt).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpCfg {
    pub kappa: f64,
    /// Experiments may spend at most this share (per mille) of the life budget in total.
    pub energy_pm: u32,
    /// At most this many experiments (per run).
    pub max_experiments: u64,
    /// Some(seed): the random-probe control (same budget, random sequence offer); None: information gain.
    pub random: Option<u64>,
    /// Random-probe control only: pace the spending so that experiments stay at most `energy_pm` of the energy spent so far.
    pub paced: bool,
    /// Which parts of the value count: bit 1 = the split (EIG), bit 2 = the support term (3 = both, the default; ablations only).
    pub parts: u8,
}

impl ExpCfg {
    pub fn eig(kappa: f64, energy_pm: u32, max_experiments: u64) -> ExpCfg {
        ExpCfg { kappa, energy_pm, max_experiments, random: None, paced: false, parts: 3 }
    }
    pub fn random(seed: u64, energy_pm: u32, max_experiments: u64) -> ExpCfg {
        ExpCfg { kappa: 0.0, energy_pm, max_experiments, random: Some(seed), paced: true, parts: 3 }
    }
    pub fn tags(&self) -> Vec<String> {
        vec![
            format!("exp_kappa_milli:{}", (self.kappa * 1000.0).round() as i64),
            format!("exp_energy_pm:{}", self.energy_pm),
            format!("exp_max:{}", self.max_experiments),
            format!("exp_random:{}", self.random.map_or("none".to_string(), |s| s.to_string())),
            format!("exp_paced:{}", self.paced as u8),
            format!("exp_parts:{}", self.parts),
        ]
    }
    pub fn from_tags(tags: &str) -> Option<ExpCfg> {
        let t = |k: &str| super::state::tag(tags, k);
        let random = match t("exp_random")? {
            "none" => None,
            s => Some(s.parse().ok()?),
        };
        Some(ExpCfg {
            kappa: t("exp_kappa_milli")?.parse::<i64>().ok()? as f64 / 1000.0,
            energy_pm: t("exp_energy_pm")?.parse().ok()?,
            max_experiments: t("exp_max")?.parse().ok()?,
            random,
            paced: t("exp_paced")? == "1",
            parts: t("exp_parts")?.parse().ok()?,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct HStat {
    right_tr: u32,
    right_va: u32,
    wrong: u32,
    na: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct SigHyps {
    n: usize,
    stats: Vec<HStat>,
}

/// Per-signature hypothesis statistics, updated incrementally as examples are revealed (examples only grow).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HypCache {
    cands: Vec<(Xf, Form)>,
    blocks: Vec<Form>,
    seen: usize,
    sigs: BTreeMap<u8, SigHyps>,
    ready: bool,
}

/// One live hypothesis as shown in a decision record.
#[derive(Debug, Clone, PartialEq)]
pub struct HypView {
    pub code: String,
    pub weight: f64,
    pub pred: Option<i64>,
    pub right: u32,
    pub not_applicable: u32,
}

/// What one candidate example is worth to the being.
#[derive(Debug, Clone, PartialEq)]
pub struct Assess {
    pub sig: u8,
    pub examples: usize,
    pub live: usize,
    /// Heaviest live hypotheses, with their prediction for the prefix.
    pub hyps: Vec<HypView>,
    /// The predicted split: (predicted value or None = not applicable, posterior mass, number of hypotheses).
    pub groups: Vec<(Option<i64>, f64, usize)>,
    pub eig_bits: f64,
    pub support: f64,
    /// eig_bits + support.
    pub value: f64,
    /// An accepted rule of this signature applies to the prefix (value 0).
    pub resolved_by: Option<String>,
}

impl Assess {
    pub fn to_json(&self) -> Value {
        json!({
            "signature": self.sig, "examples_of_signature": self.examples, "live_hypotheses": self.live,
            "heaviest_hypotheses": self.hyps.iter().map(|h| json!({ "hypothesis": h.code, "posterior": r4(h.weight), "predicts": h.pred, "right_so_far": h.right, "not_applicable_so_far": h.not_applicable })).collect::<Vec<_>>(),
            "predicted_split": self.groups.iter().map(|g| json!({ "outcome": g.0.map_or("not applicable".to_string(), |v| v.to_string()), "mass": r4(g.1), "hypotheses": g.2 })).collect::<Vec<_>>(),
            "expected_information_bits": r4(self.eig_bits), "expected_support_gain": r4(self.support), "value_bits": r4(self.value),
            "resolved_by_rule": self.resolved_by,
        })
    }
}

pub fn r4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

pub fn hyp_code(xf: Xf, f: &Form) -> String {
    if xf.is_id() { f.code() } else { format!("{}+{}", xf.code(), f.code()) }
}

impl HypCache {
    pub fn new() -> HypCache {
        HypCache::default()
    }

    /// Take in the examples revealed since the last call (the search space depends on the building blocks used for composition).
    pub fn update(&mut self, examples: &[Example], blocks: &[Form]) {
        if !self.ready || blocks != self.blocks.as_slice() || examples.len() < self.seen {
            *self = HypCache { cands: candidates_for(blocks), blocks: blocks.to_vec(), seen: 0, sigs: BTreeMap::new(), ready: true };
        }
        let nc = self.cands.len();
        for e in &examples[self.seen..] {
            let s = self.sigs.entry(e.sig).or_insert_with(|| SigHyps { n: 0, stats: vec![HStat::default(); nc] });
            let held = is_held_out(s.n);
            for (k, (xf, f)) in self.cands.iter().enumerate() {
                let st = &mut s.stats[k];
                match predict_x(*xf, f, &e.terms) {
                    None => st.na += 1,
                    Some(v) if v == e.next => {
                        if held {
                            st.right_va += 1
                        } else {
                            st.right_tr += 1
                        }
                    }
                    Some(_) => st.wrong += 1,
                }
            }
            s.n += 1;
        }
        self.seen = examples.len();
    }

    pub fn examples_of(&self, sig: u8) -> usize {
        self.sigs.get(&sig).map_or(0, |s| s.n)
    }

    /// Live hypotheses of a signature with their normalised weights.
    fn live(&self, sig: u8) -> Vec<(usize, f64, HStat)> {
        let empty = HStat::default();
        let mut v: Vec<(usize, f64, HStat)> = vec![];
        for (k, (xf, f)) in self.cands.iter().enumerate() {
            let st = self.sigs.get(&sig).map_or(empty, |s| s.stats[k]);
            if st.wrong > 0 {
                continue;
            }
            let lw = -((f.size() + xf.cost()) as f64) * std::f64::consts::LN_2 + st.na as f64 * NA_LIKELIHOOD.ln();
            v.push((k, lw, st));
        }
        let mx = v.iter().map(|x| x.1).fold(f64::NEG_INFINITY, f64::max);
        let tot: f64 = v.iter().map(|x| (x.1 - mx).exp()).sum();
        for x in &mut v {
            x.1 = (x.1 - mx).exp() / tot;
        }
        v
    }

    /// The value of revealing the answer of a task of signature `sig` with prefix `prefix`.
    pub fn assess(&self, sig: u8, prefix: &[i64], rules: &[Rule]) -> Assess {
        let n = self.examples_of(sig);
        if let Some((_, r)) = discover::apply(rules, sig, prefix) {
            return Assess { sig, examples: n, live: 0, hyps: vec![], groups: vec![], eig_bits: 0.0, support: 0.0, value: 0.0, resolved_by: Some(r.id.clone()) };
        }
        let live = self.live(sig);
        let held = is_held_out(n);
        let mut groups: Vec<(Option<i64>, f64, usize)> = vec![];
        let mut support = 0.0;
        let mut views: Vec<HypView> = vec![];
        for (k, w, st) in &live {
            let (xf, f) = self.cands[*k];
            let pred = predict_x(xf, &f, prefix);
            match groups.iter_mut().find(|g| g.0 == pred) {
                Some(g) => {
                    g.1 += w;
                    g.2 += 1;
                }
                None => groups.push((pred, *w, 1)),
            }
            if pred.is_some() {
                let need_tr = K_TRAIN.saturating_sub(st.right_tr);
                let need_va = K_VAL.saturating_sub(st.right_va);
                let need = need_tr + need_va;
                let fills = if held { need_va > 0 } else { need_tr > 0 };
                if need > 0 && fills {
                    support += w / need as f64;
                }
            }
            views.push(HypView { code: hyp_code(xf, &f), weight: *w, pred, right: st.right_tr + st.right_va, not_applicable: st.na });
        }
        let pn: f64 = groups.iter().filter(|g| g.0.is_none()).map(|g| g.1).sum();
        let mut eig = 0.0;
        for g in groups.iter().filter(|g| g.0.is_some()) {
            if g.1 > 0.0 {
                eig += g.1 * (1.0 / (g.1 + pn)).log2();
            }
        }
        if pn > 0.0 {
            eig += pn * (1.0 / pn).log2();
        }
        let eig = eig.max(0.0);
        groups.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
        views.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal).then(a.code.cmp(&b.code)));
        views.truncate(SHOW_HYPS);
        Assess { sig, examples: n, live: live.len(), hyps: views, groups, eig_bits: eig, support, value: eig + support, resolved_by: None }
    }

    /// What a revealed answer did to the live hypotheses of a signature: (kept, rejected) codes.
    /// Call BEFORE `update` takes the new example in.
    pub fn verdict(&self, sig: u8, prefix: &[i64], next: i64) -> (Vec<String>, Vec<String>) {
        let (mut kept, mut rejected) = (vec![], vec![]);
        for (k, _, _) in self.live(sig) {
            let (xf, f) = self.cands[k];
            match predict_x(xf, &f, prefix) {
                Some(v) if v != next => rejected.push(hyp_code(xf, &f)),
                Some(_) => kept.push(hyp_code(xf, &f)),
                None => {}
            }
        }
        (kept, rejected)
    }
}

/// One option the planner would consider, with the planner's own estimates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opt {
    pub slot: usize,
    pub strategy: Strategy,
    pub p: f64,
    pub cost_est: f64,
    pub mean_score: f64,
    pub affordable: bool,
    /// Energy the attempt will actually take (`Strategy::energy` of the listed cost).
    pub energy: i64,
}

/// An offered task whose answer would be revealed as an example, with what the being sees of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub slot: usize,
    pub sig: u8,
    pub prefix: Vec<i64>,
    pub listed_reward: i64,
}

/// The experiment chosen for a step.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub slot: usize,
    /// The prefix the being was shown (what the experiment asks about).
    pub prefix: Vec<i64>,
    pub strategy: Strategy,
    pub sig: u8,
    pub energy: i64,
    pub assess: Assess,
    pub voi: f64,
    pub score: f64,
    pub exploit_score: f64,
    pub why: String,
}

/// Budget and counters of the experimenter (per run; spent energy per life is passed in).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExpBook {
    pub n: u64,
    pub energy: i64,
    /// Experiments refused by the budget (the first one of each kind is logged as a controller refusal).
    pub refused_energy: u64,
    pub refused_count: u64,
    /// Sums over the experiments run of the expected split (bits) and the expected support gain (as assessed).
    pub eig_sum: f64,
    pub support_sum: f64,
}

impl ExpBook {
    /// Count an experiment that is being run.
    pub fn take(&mut self, p: &Pick) {
        self.n += 1;
        self.energy += p.energy;
        self.eig_sum += p.assess.eig_bits;
        self.support_sum += p.assess.support;
    }
}

/// Of the experiments `(sig, prefix)` (in order), how many had a revealed example that rejected at least one
/// hypothesis live just before it. `examples`: the being's revealed examples in log order.
pub fn count_rejecting(examples: &[Example], exps: &[(u8, Vec<i64>)]) -> u64 {
    let mut cache = HypCache::new();
    let (mut n, mut from) = (0u64, 0usize);
    for (sig, prefix) in exps {
        if let Some(off) = examples[from..].iter().position(|e| e.sig == *sig && &e.terms == prefix) {
            let i = from + off;
            cache.update(&examples[..i], &[]);
            let ex = &examples[i];
            if !cache.verdict(ex.sig, &ex.terms, ex.next).1.is_empty() {
                n += 1;
            }
            from = i + 1;
        }
    }
    n
}

/// Deterministic hash for the random-probe control.
fn mixh(parts: &[u64]) -> u64 {
    let mut h: u64 = 0xA076_1D64_78BD_642F;
    for p in parts {
        h ^= p.wrapping_add(0x9E37_79B9_7F4A_7C15).wrapping_add(h << 6).wrapping_add(h >> 2);
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 31;
    }
    h
}

pub enum Decision {
    /// Keep the planner's choice (with the best experiment that was considered, if any).
    Exploit { considered: Option<Pick>, refused: Option<&'static str> },
    Experiment(Pick),
}

/// The decision of the experimenter for one step. `life_budget`: energy of the whole life; `spent_total`:
/// energy the being has spent so far (pacing of the random control); `step`: for the random control's hash.
#[allow(clippy::too_many_arguments)]
pub fn decide(cfg: &ExpCfg, book: &ExpBook, cache: &HypCache, rules: &[Rule], probes: &[Probe], opts: &[Opt], life_budget: i64, spent_total: i64, step: u64) -> Decision {
    let exploit_score = opts.iter().filter(|o| o.affordable).map(|o| o.mean_score).fold(f64::NEG_INFINITY, f64::max);
    if probes.is_empty() || !exploit_score.is_finite() {
        return Decision::Exploit { considered: None, refused: None };
    }
    let best_strategy = |slot: usize, voi: f64, reward: i64| -> Option<(Opt, f64)> {
        let mut b: Option<(Opt, f64)> = None;
        for o in opts.iter().filter(|o| o.slot == slot && o.affordable) {
            let s = (o.p * reward as f64 + voi) / o.cost_est;
            if b.map_or(true, |x| s > x.1) {
                b = Some((*o, s));
            }
        }
        b
    };
    let pick = match cfg.random {
        Some(seed) => {
            let h = mixh(&[seed, step, 0xB9]);
            if h % 2 == 1 {
                return Decision::Exploit { considered: None, refused: None };
            }
            let pr = &probes[((h >> 8) % probes.len() as u64) as usize];
            let Some((o, s)) = best_strategy(pr.slot, 0.0, pr.listed_reward) else {
                return Decision::Exploit { considered: None, refused: None };
            };
            let assess = cache.assess(pr.sig, &pr.prefix, rules);
            Pick { slot: pr.slot, prefix: pr.prefix.clone(), strategy: o.strategy, sig: pr.sig, energy: o.energy, assess, voi: 0.0, score: s, exploit_score, why: "random probe: a uniformly random offered sequence task (control arm)".into() }
        }
        None => {
            let mut best: Option<Pick> = None;
            for pr in probes {
                let mut a = cache.assess(pr.sig, &pr.prefix, rules);
                a.value = if cfg.parts & 1 != 0 { a.eig_bits } else { 0.0 } + if cfg.parts & 2 != 0 { a.support } else { 0.0 };
                if a.value <= 0.0 {
                    continue;
                }
                let voi = cfg.kappa * a.value * pr.listed_reward as f64;
                if let Some((o, s)) = best_strategy(pr.slot, voi, pr.listed_reward) {
                    if best.as_ref().map_or(true, |b| s > b.score) {
                        best = Some(Pick { slot: pr.slot, prefix: pr.prefix.clone(), strategy: o.strategy, sig: pr.sig, energy: o.energy, assess: a, voi, score: s, exploit_score, why: String::new() });
                    }
                }
            }
            let Some(mut b) = best else {
                return Decision::Exploit { considered: None, refused: None };
            };
            if b.score <= exploit_score {
                return Decision::Exploit { considered: Some(b), refused: None };
            }
            b.why = format!("experiment: expected value of information {:.3} reward units ({:.3} bits: split {:.3} + support {:.3}) makes {:.3} per energy, above the best exploit option {:.3}", b.voi, b.assess.value, b.assess.eig_bits, b.assess.support, b.score, exploit_score);
            b
        }
    };
    if book.n >= cfg.max_experiments {
        return Decision::Exploit { considered: Some(pick), refused: Some("max_experiments_per_run") };
    }
    let cap = cfg.energy_pm as i64 * life_budget / 1000;
    let over = if cfg.paced { (book.energy + pick.energy) * 1000 > cfg.energy_pm as i64 * (spent_total + pick.energy) } else { book.energy + pick.energy > cap };
    if over {
        return Decision::Exploit { considered: Some(pick), refused: Some("max_experiment_energy_pm") };
    }
    Decision::Experiment(pick)
}

/// The JSON a decision record carries about the experiment (chosen or only considered).
pub fn pick_json(p: &Pick, chosen: bool, book: &ExpBook, cfg: &ExpCfg, refused: Option<&str>) -> Value {
    json!({
        "chosen": chosen, "slot": p.slot, "strategy": p.strategy.name(), "energy": p.energy, "hypotheses": p.assess.to_json(),
        "value_of_information_reward_units": r4(p.voi), "experiment_score": r4(p.score), "best_exploit_score": r4(p.exploit_score),
        "why": p.why, "refused_by": refused,
        "budget": { "kappa": cfg.kappa, "energy_pm_of_life": cfg.energy_pm, "max_experiments": cfg.max_experiments, "random_control": cfg.random.is_some(),
                    "experiments_so_far": book.n, "experiment_energy_so_far": book.energy },
    })
}
