//! Track B12 (opt-in): learning to learn in World O. Three switches, each off by default; with all off the
//! being, its log and every measurement are exactly those of Track B11 (a test checks it).
//!
//! * **M2, hierarchical prior** (`prior_k` > 0). Without it a new family's cells start from the strategy-wide
//!   estimate pooled over everything the being has seen (all types, all ages), with prior weight 4. With
//!   M2, when a family is announced, the being forms a prior for its cells from the families it met
//!   before that had the same announced type: the latest `prior_k` of them (closed or still open, with
//!   at least `MIN_FAM_N` attempts with that strategy), as an empirical-Bayes Beta prior per strategy
//!   and per difficulty: mean = mean of their success shares, weight from their spread (method of
//!   moments, between `K_MIN` and `K_MAX`). Tiers are not announced; because families get harder in
//!   order of appearance, the latest families of a type stand in for "families like this one". The
//!   prior is formed once, at the announcement (a pure function of the state folded from the log), so
//!   the replay re-derives it.
//! * **M3, new-family exploration** (`explore_n` > 0). An option on a family with fewer than `explore_n`
//!   attempts gets an information bonus on its score, `explore_w` times the reported rate times the
//!   share of those tries still missing, so the being tries a new family a few times even when its prior
//!   is low. For a sequence family each try is also a revealed example for the rule search. The energy
//!   of the steps on which the bonus changed the choice is counted (folded from the log: the decision
//!   records it: outcome tag `b12_explore`), and the bonus stops once it reaches `explore_pm` per mille of
//!   the energy spent (the controller can lower that budget: `RunOCfg::max_explore_pm`; the refusal is
//!   logged before the first step and folded, so the replay applies it too).
//! * **M4, family-wide rules and the rule-form prior** (`form_prior` 1 or 2). The B2 search keys rules by
//!   (family, difficulty), and a family's examples are spread over five difficulties (measured: 1 to 3
//!   per signature, so the 3 + 1 support is rarely reached). Level 1: after the B2 pass, for each open
//!   family, one form is sought over ALL its examples (every difficulty; held-out split by index within
//!   the family), with the B2 guard (never applicable-and-wrong, 3 training + 1 held-out, the 3 spare
//!   equations of the self-check); the first form that passes is accepted for every difficulty of the
//!   family without a live rule (tag `b12_family`). Level 2 adds the prior over forms learned from
//!   earlier families: the forms of the family's own live rules, then the forms accepted in at least
//!   `FORM_MIN_FAMS` earlier families (most families first) are tried before the rest of the grammar, and
//!   such a likely form needs `K_TRAIN_PRIOR` + `K_VAL_PRIOR` (2 + 1) instead of 3 + 1 (tag
//!   `b12_reduced`). The measurement judges every accepted rule against the hidden rule (wrong-rule rate).
//!
//! Only observable things are used (announced type and order, the being's own outcomes, its own rules).

use super::engine::{EngineO, LearnerO, StateO};
use super::world::{Open, SKIP_COST};
use crate::being::discover::{forms, is_held_out, predict_x, Form, Found, Rule, Xf, K_TRAIN, K_VAL, MAX_CITE};
use crate::being::model::{Cell, Estimate, PRIOR_K};
use crate::being::planner::SKIP_MIN_COST;
use crate::being::solver::{Strategy, STRATEGIES_EXT};
use crate::being::world2::engine::State2;
use crate::being::world2::learner::{r3, Act2, Choice2, Offer2, LEARNER_NAME};
use crate::being::world2::model::FamModel;
use crate::being::world2::rules_use::{covered, rule_estimate};
use crate::being::world2::solve::{sig2, sig2_split};
use crate::being::world2::world::FType;
use serde_json::json;
use std::collections::BTreeMap;

/// A family needs this many attempts with a strategy to count for that strategy's prior.
pub const MIN_FAM_N: u64 = 5;
/// Families needed for a prior (fewer: the B1 pooled estimate is kept).
pub const MIN_FAMS: usize = 3;
/// Bounds of the empirical-Bayes prior weight.
pub const K_MIN: f64 = 2.0;
pub const K_MAX: f64 = 30.0;
/// Earlier families in which a form must have been accepted to count as a likely form.
pub const FORM_MIN_FAMS: u32 = 2;
pub const K_TRAIN_PRIOR: u32 = 2;
pub const K_VAL_PRIOR: u32 = 1;
pub const POLICY_EXPLORE: &str = "wo-explore";

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct L2LCfg {
    /// M2: cohort size (0: off).
    pub prior_k: u32,
    /// M3: tries per new family that get the bonus (0: off).
    pub explore_n: u32,
    /// M3: bonus weight in per mille of the reported rate.
    pub explore_w_milli: u32,
    /// M3: energy budget in per mille of the energy spent.
    pub explore_pm: u32,
    /// M4: 0 off; 1 family-wide rules (one form for all difficulties of a family, B2 support pooled over
    /// the family); 2 as 1, plus the rule-form prior (likely forms from earlier families need less support).
    pub form_prior: u8,
}

impl L2LCfg {
    pub fn any(&self) -> bool {
        self.prior_k > 0 || self.explore_n > 0 || self.form_prior > 0
    }
    /// Genesis tag (written only when a switch is on).
    pub fn tag(&self) -> String {
        format!("wo_l2l:{}/{}/{}/{}/{}", self.prior_k, self.explore_n, self.explore_w_milli, self.explore_pm, self.form_prior)
    }
    pub fn parse(v: &str) -> Option<L2LCfg> {
        let p: Vec<&str> = v.split('/').collect();
        if p.len() != 5 {
            return None;
        }
        Some(L2LCfg { prior_k: p[0].parse().ok()?, explore_n: p[1].parse().ok()?, explore_w_milli: p[2].parse().ok()?, explore_pm: p[3].parse().ok()?, form_prior: p[4].parse().ok().filter(|x| *x <= 2)? })
    }
    pub fn words(&self) -> String {
        format!(
            "; learning to learn: hierarchical prior {}, new-family exploration {}, rule-form prior {}",
            if self.prior_k > 0 { format!("over the latest {} families of the same type", self.prior_k) } else { "off".into() },
            if self.explore_n > 0 { format!("for the first {} tries (weight {:.3}, budget {} per mille of the energy)", self.explore_n, self.explore_w_milli as f64 / 1000.0, self.explore_pm) } else { "off".into() },
            match self.form_prior {
                0 => "off",
                1 => "family-wide rules",
                _ => "family-wide rules with the rule-form prior",
            }
        )
    }
    /// Whether the chooser differs from the B11 learner (M2 or M3).
    pub fn chooser_on(&self) -> bool {
        self.prior_k > 0 || self.explore_n > 0
    }
}

/// What the being keeps of a family it has met (for M2): its type, order and per-cell counts.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct FamHist {
    pub fid: u64,
    pub ftype: FType,
    pub cells: [[Cell; 4]; 5],
}

/// Prior of one cell: mean and weight.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct PCell {
    pub mean: f64,
    pub k: f64,
}

/// The prior of a new family's cells, formed at its announcement. `None` per strategy: no prior (B1 estimate).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct SlotPrior {
    pub fid: u64,
    pub fams: usize,
    /// [difficulty-1][strategy]
    pub cells: [[Option<PCell>; 4]; 5],
}

/// Empirical-Bayes Beta prior from per-family (successes, attempts).
pub fn eb_prior(xs: &[(u64, u64)]) -> Option<PCell> {
    if xs.len() < MIN_FAMS {
        return None;
    }
    let rates: Vec<f64> = xs.iter().map(|(s, n)| (*s as f64 + 0.5) / (*n as f64 + 1.0)).collect();
    let m = rates.iter().sum::<f64>() / rates.len() as f64;
    let var = rates.iter().map(|r| (r - m).powi(2)).sum::<f64>() / (rates.len() - 1) as f64;
    // sampling part of the spread
    let samp = xs.iter().map(|(_, n)| m * (1.0 - m) / (*n as f64 + 1.0)).sum::<f64>() / xs.len() as f64;
    let between = (var - samp).max(1e-6);
    let k = (m * (1.0 - m) / between - 1.0).clamp(K_MIN, K_MAX);
    Some(PCell { mean: m.clamp(0.01, 0.99), k })
}

/// The M2 prior for a family of type `ftype` announced now, from the families met before (latest first).
pub fn form_slot_prior(fid: u64, ftype: FType, before: &[FamHist], cohort: usize) -> SlotPrior {
    let co: Vec<&FamHist> = before.iter().rev().filter(|h| h.ftype == ftype && h.fid < fid).take(cohort).collect();
    let mut cells = [[None; 4]; 5];
    for s in STRATEGIES_EXT {
        let si = s.idx();
        let tot: Vec<(u64, u64)> = co.iter().map(|h| (0..5).fold((0u64, 0u64), |a, k| (a.0 + h.cells[k][si].succ, a.1 + h.cells[k][si].n))).filter(|x| x.1 >= MIN_FAM_N).collect();
        let Some(ps) = eb_prior(&tot) else { continue };
        for k in 0..5 {
            let dk: Vec<(u64, u64)> = co.iter().map(|h| (h.cells[k][si].succ, h.cells[k][si].n)).filter(|x| x.1 >= 2).collect();
            // per difficulty when enough families have it, else the strategy's prior
            cells[k][si] = Some(match eb_prior(&dk) {
                Some(pd) => PCell { mean: pd.mean, k: pd.k.min(ps.k) },
                None => ps,
            });
        }
    }
    SlotPrior { fid, fams: co.len(), cells }
}

/// The estimate of a cell with the M2 prior in place of the B1 pooled prior (the same Beta posterior shape
/// and the model's cost ratio).
pub fn estimate_h(model: &FamModel, slot: usize, d: u8, s: Strategy, prior: &SlotPrior) -> Estimate {
    let base = model.estimate(slot, d, s);
    let k = (d.clamp(1, 5) - 1) as usize;
    let Some(pc) = prior.cells[k][s.idx()] else { return base };
    // the family's own evidence at other difficulties moves the prior mean (as the B1 family level does)
    let mut fam = (0u64, 0u64);
    for dd in 1..=5u8 {
        let c = model.cell(slot, dd, s);
        if dd != d.clamp(1, 5) {
            fam = (fam.0 + c.succ, fam.1 + c.n);
        }
    }
    let own_other = if fam.1 > 0 { (fam.0 as f64 + PRIOR_K * pc.mean) / (fam.1 as f64 + PRIOR_K) - pc.mean } else { 0.0 };
    let mean = (pc.mean + own_other).clamp(0.01, 0.99);
    let c = model.cell(slot, d, s);
    let a = c.succ as f64 + pc.k * mean;
    let b = (c.n - c.succ) as f64 + pc.k * (1.0 - mean);
    let p = a / (a + b);
    let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
    Estimate { n: base.n, p, sd, cost_ratio: base.cost_ratio, group_n: base.group_n }
}

/// B12 state of the being, folded from its log (part of `StateO`).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct B12State {
    pub cfg: L2LCfg,
    /// Families met (closed ones with their final counts), in order of announcement.
    pub hist: Vec<FamHist>,
    /// slot -> the M2 prior of the family now in it.
    pub priors: BTreeMap<usize, SlotPrior>,
    /// M3: energy and steps of exploration choices (decisions of policy `wo-explore`).
    pub explore_energy: i64,
    pub explore_steps: u64,
    /// M3: the controller's cap on the budget (per mille), if lower than the learner's.
    pub explore_cap_pm: Option<u32>,
    /// M4: rules accepted with the reduced support.
    pub reduced_rules: u64,
    /// M2: slot -> (family id, announced type) of the open families.
    pub types: BTreeMap<usize, (u64, FType)>,
    /// Changes of `priors` (the chooser copies them only when this moves).
    pub version: u64,
    /// M4: form (code) -> the families in which a rule of that form was accepted.
    pub form_fams: BTreeMap<String, std::collections::BTreeSet<u64>>,
    pub form_version: u64,
}

/// Controller cap (in a `controller_refusal` note) on the exploration budget, per mille of the energy.
pub const CAP_EXPLORE: &str = "max_explore_pm";
/// Families kept in the history (the cohort reads the latest of a type).
pub const HIST_MAX: usize = 400;

impl B12State {
    pub fn note_explore(&mut self, cost: i64) {
        self.explore_energy += cost;
        self.explore_steps += 1;
    }
    /// The exploration budget in force (per mille): the learner's, lowered by the controller's cap.
    pub fn budget_pm(&self) -> u32 {
        self.explore_cap_pm.map_or(self.cfg.explore_pm, |c| c.min(self.cfg.explore_pm))
    }
}

/// What the B12 chooser reads of the World O state.
#[derive(Debug, Clone, PartialEq)]
pub struct B12View {
    pub version: u64,
    pub priors: BTreeMap<usize, SlotPrior>,
    pub explore_energy: i64,
    pub budget_pm: u32,
}

fn hist_cells(model: &FamModel, slot: usize) -> [[Cell; 4]; 5] {
    let mut c = [[Cell::default(); 4]; 5];
    for d in 1..=5u8 {
        for s in STRATEGIES_EXT {
            c[(d - 1) as usize][s.idx()] = model.cell(slot, d, s);
        }
    }
    c
}

/// M2: a family is announced; form its prior from the families met before (closed, and open ones so far).
pub fn on_open(st: &mut StateO, o: &Open) {
    let mut before: Vec<FamHist> = st.b12.hist.clone();
    for (slot, (fid, ft)) in &st.b12.types {
        if *slot != o.slot && *fid < o.fid {
            before.push(FamHist { fid: *fid, ftype: *ft, cells: hist_cells(&st.s2.model, *slot) });
        }
    }
    before.sort_by_key(|h| h.fid);
    let pr = form_slot_prior(o.fid, o.ftype, &before, st.b12.cfg.prior_k as usize);
    st.b12.types.insert(o.slot, (o.fid, o.ftype));
    st.b12.priors.insert(o.slot, pr);
    st.b12.version += 1;
}

/// M2: a family closes; keep its counts (before the slot is reset).
pub fn on_close(st: &mut StateO, slot: usize) {
    if let Some((fid, ft)) = st.b12.types.remove(&slot) {
        st.b12.hist.push(FamHist { fid, ftype: ft, cells: hist_cells(&st.s2.model, slot) });
        if st.b12.hist.len() > HIST_MAX {
            st.b12.hist.remove(0);
        }
    }
    st.b12.priors.remove(&slot);
    st.b12.version += 1;
}

/// M4: an accepted rule adds its form to the form prior (the families in which the form was accepted).
pub fn on_rule(st: &mut StateO, r: &Rule) {
    let (slot, _) = sig2_split(r.sig);
    if let Some(fid) = st.known.get(&slot) {
        if r.xf.is_id() {
            st.b12.form_fams.entry(r.form.code()).or_default().insert(*fid);
            st.b12.form_version += 1;
        }
    }
}

/// Copy what the chooser needs (the priors only when they changed).
pub fn prepare(l: &mut LearnerO, st: &StateO) {
    let b = &st.b12;
    match &mut l.view {
        Some(v) if v.version == b.version => {
            v.explore_energy = b.explore_energy;
            v.budget_pm = b.budget_pm();
        }
        _ => l.view = Some(B12View { version: b.version, priors: b.priors.clone(), explore_energy: b.explore_energy, budget_pm: b.budget_pm() }),
    }
}

/// Attempts the being has made on the family now in `slot` (its cells start empty at its announcement).
pub fn family_attempts(model: &FamModel, slot: usize) -> u64 {
    let mut n = 0;
    for d in 1..=5u8 {
        for s in STRATEGIES_EXT {
            n += model.cell(slot, d, s).n;
        }
    }
    n
}

struct Cand {
    slot: usize,
    s: Strategy,
    n: u64,
    p: f64,
    sd: f64,
    cost_est: f64,
    score: f64,
    bonus: f64,
    from_prior: bool,
    affordable: bool,
    fam_n: u64,
}

/// The B12 chooser: the B11 learner (B1 planner over families, the skip test of `skip_rule`, B7 A/A2) with
/// the M2 prior in the estimates and the M3 bonus on the scores. Policy `w2-model-ucb` unless the bonus
/// changed the choice (then `wo-explore`).
pub fn choose(l: &mut LearnerO, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
    let cfg = l.l2l;
    let inner = &l.inner;
    let view = l.view.as_ref();
    let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
    let budget_ok = cfg.explore_n > 0 && view.map_or(false, |v| (v.explore_energy as f64) < v.budget_pm as f64 / 1000.0 * st.total_cost as f64);
    let mut cands: Vec<Cand> = vec![];
    for o in offers {
        let fam_n = family_attempts(&st.model, o.fam);
        let bonus = if budget_ok && fam_n < cfg.explore_n as u64 { cfg.explore_w_milli as f64 / 1000.0 * rate * (1.0 - fam_n as f64 / cfg.explore_n as f64) } else { 0.0 };
        for s in STRATEGIES_EXT {
            if inner.use_.rule_only && s == Strategy::Discovered && !covered(st, o) {
                continue;
            }
            let prior = if cfg.prior_k > 0 { view.and_then(|v| v.priors.get(&o.fam)) } else { None };
            let mut e = match prior {
                Some(pr) => estimate_h(&st.model, o.fam, o.difficulty, s, pr),
                None => st.model.estimate(o.fam, o.difficulty, s),
            };
            let from_prior = prior.map_or(false, |pr| pr.cells[(o.difficulty.clamp(1, 5) - 1) as usize][s.idx()].is_some());
            if inner.use_.rule_est && s == Strategy::Discovered {
                if let Some(re) = rule_estimate(st, o, inner.use_.rule_k, &e) {
                    e = re.est;
                }
            }
            let cost_est = (o.listed_cost as f64 * e.cost_ratio).max(1.0);
            let pu = (e.p + inner.beta * e.sd).min(1.0);
            let score = o.listed_reward as f64 * pu / cost_est;
            cands.push(Cand { slot: o.slot, s, n: e.n, p: e.p, sd: e.sd, cost_est, score, bonus, from_prior, affordable: cost_est.round() as i64 <= st.budget_left, fam_n });
        }
    }
    let val = |c: &Cand, with_bonus: bool| c.score + if with_bonus { c.bonus } else { 0.0 };
    let pick = |with_bonus: bool| -> Option<usize> {
        let mut bi: Option<usize> = None;
        for (i, c) in cands.iter().enumerate() {
            if c.affordable && bi.map_or(true, |b| val(c, with_bonus) > val(&cands[b], with_bonus)) {
                bi = Some(i);
            }
        }
        bi
    };
    let skip_of = |c: &Cand, v: f64| -> (bool, f64) {
        let bar = l.skip_rule.bar(inner.skip_frac, rate, c.cost_est, SKIP_COST, c.n);
        (inner.skip_frac > 0.0 && st.total_cost >= SKIP_MIN_COST && v < bar && st.budget_left >= SKIP_COST, bar)
    };
    let plain_i = pick(false)?;
    let (plain_skip, _) = skip_of(&cands[plain_i], cands[plain_i].score);
    let bi = pick(true)?;
    let best = &cands[bi];
    let (skip, bar) = skip_of(best, val(best, true));
    let explore = best.bonus > 0.0 && !skip && (plain_skip || plain_i != bi);
    let policy = if explore { POLICY_EXPLORE } else { LEARNER_NAME };
    let alts: Vec<serde_json::Value> = cands
        .iter()
        .map(|c| {
            json!({ "slot": c.slot, "strategy": c.s.name(), "n_cell": c.n, "p_success": r3(c.p), "uncertainty_sd": r3(c.sd), "est_energy": r3(c.cost_est), "score": r3(c.score),
                    "affordable": c.affordable, "prior": if c.from_prior { "same-type families (B12 M2)" } else { "pooled (B1)" }, "explore_bonus": r3(c.bonus), "family_attempts": c.fam_n })
        })
        .collect();
    let rationale = json!({
        "policy": policy, "learner": "B1 world model keyed by family with the Track B12 switches", "beta": inner.beta, "skip_frac": inner.skip_frac, "skip_rule": l.skip_rule.code(), "skip_bar": r3(bar),
        "reported_reward_per_energy": r3(rate),
        "b12": { "prior_cohort": cfg.prior_k, "explore_tries": cfg.explore_n, "explore_weight": cfg.explore_w_milli as f64 / 1000.0, "explore_budget_pm": view.map_or(0, |v| v.budget_pm), "explore_energy_so_far": view.map_or(0, |v| v.explore_energy), "budget_open": budget_ok },
        "alternatives": alts,
    });
    if skip {
        return Some(Choice2 { act: Act2::Skip, policy: policy.into(), why: format!("best optimistic score {:.3} (with bonus {:.3}) is below the skip bar {:.3}: skip the menu", best.score, best.bonus, bar), p: 0.0, sd: 0.0, cost_est: SKIP_COST as f64, score: 0.0, rationale });
    }
    let why = if explore {
        format!("exploration of a new family ({} attempts so far): optimistic score {:.3} plus information bonus {:.3} beats the learner's own choice", best.fam_n, best.score, best.bonus)
    } else {
        "highest optimistic reward per estimated energy (p + beta*sd) among affordable options".into()
    };
    Some(Choice2 { act: Act2::Attempt { slot: best.slot, strategy: best.s }, policy: policy.into(), why, p: best.p, sd: best.sd, cost_est: best.cost_est, score: best.score, rationale })
}

/// How a family-wide rule (M4) was accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyHow {
    pub slot: usize,
    pub examples: usize,
    pub train: u32,
    pub val: u32,
    /// Accepted only because the form was likely (support below the B2 3 + 1).
    pub reduced: bool,
    /// Where the form's place in the order came from: "same family", "earlier families (n)", or "grammar".
    pub from: String,
}

impl FamilyHow {
    pub fn text(&self, base: &str) -> String {
        format!(
            "{base}; family-wide rule (Track B12): the form reproduced {} training and {} held-out revealed answers of this family over all its difficulties and was never applicable-and-wrong on its {} examples; form prior: {}{}",
            self.train,
            self.val,
            self.examples,
            self.from,
            if self.reduced { " (accepted with the reduced support of a likely form)" } else { "" }
        )
    }
    pub fn json(&self) -> serde_json::Value {
        json!({ "slot": self.slot, "family_examples": self.examples, "train": self.train, "val": self.val, "reduced_support": self.reduced, "form_prior": self.from })
    }
}

/// M4: family-wide rules. For each open family with revealed examples and a difficulty without a live rule,
/// the forms are tried in the order of the form prior (forms of this family's live rules, then, at level 2,
/// forms accepted in at least `FORM_MIN_FAMS` earlier families, most families first, then the grammar) over
/// ALL the family's examples (held-out split by index within the family). The first form that is never
/// applicable-and-wrong and has the support (3 + 1; 2 + 1 for a likely form at level 2) is accepted for every
/// difficulty of the family that has no live rule.
pub fn family_pass(e: &mut EngineO) -> Vec<(Found, FamilyHow)> {
    let lvl = e.state.b12.cfg.form_prior;
    let st = &e.state;
    let ex = st.s2.search_examples();
    let mut by_slot: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, x) in ex.iter().enumerate() {
        by_slot.entry(sig2_split(x.sig).0).or_default().push(i);
    }
    let grammar = forms();
    let rank = |f: &Form| grammar.iter().position(|g| g == f).unwrap_or(usize::MAX);
    let mut out = vec![];
    let mut seen: Vec<(usize, (usize, usize, u64))> = vec![];
    for (slot, idx) in by_slot {
        let Some(fid) = st.known.get(&slot).copied() else { continue };
        let live: Vec<&Rule> = st.s2.rules.iter().filter(|r| !r.retired && sig2_split(r.sig).0 == slot).collect();
        let missing: Vec<u8> = (1..=5u8).filter(|d| !live.iter().any(|r| r.sig == sig2(slot, *d))).collect();
        if missing.is_empty() {
            continue;
        }
        let key = (idx.len(), live.len(), st.b12.form_version);
        if e.fam_scan.get(&slot) == Some(&key) {
            continue;
        }
        seen.push((slot, key));
        let mut order: Vec<(Form, String, bool)> = vec![];
        for r in &live {
            if r.xf.is_id() && !order.iter().any(|o| o.0 == r.form) {
                order.push((r.form, "same family".into(), true));
            }
        }
        if lvl >= 2 {
            let mut fr: Vec<(usize, Form)> = st
                .b12
                .form_fams
                .iter()
                .filter_map(|(c, f)| {
                    let n = f.iter().filter(|x| **x != fid).count();
                    Form::parse(c).filter(|_| n >= FORM_MIN_FAMS as usize).map(|fm| (n, fm))
                })
                .collect();
            fr.sort_by(|a, b| b.0.cmp(&a.0).then(rank(&a.1).cmp(&rank(&b.1))));
            for (n, fm) in fr {
                if !order.iter().any(|o| o.0 == fm) {
                    order.push((fm, format!("earlier families ({n})"), true));
                }
            }
        }
        for fm in &grammar {
            if !order.iter().any(|o| o.0 == *fm) {
                order.push((*fm, "grammar".into(), false));
            }
        }
        for (fm, from, likely) in order {
            let (mut tr, mut va, mut wrong) = (0u32, 0u32, false);
            let mut cite = vec![];
            for (k, &i) in idx.iter().enumerate() {
                match predict_x(Xf::ID, &fm, &ex[i].terms) {
                    None => {}
                    Some(v) if v == ex[i].next => {
                        if is_held_out(k) {
                            va += 1
                        } else {
                            tr += 1
                        }
                        if cite.len() < MAX_CITE {
                            cite.push(ex[i].event_id.clone());
                        }
                    }
                    Some(_) => {
                        wrong = true;
                        break;
                    }
                }
            }
            if wrong {
                continue;
            }
            let (kt, kv) = if lvl >= 2 && likely { (K_TRAIN_PRIOR, K_VAL_PRIOR) } else { (K_TRAIN, K_VAL) };
            if tr >= kt && va >= kv {
                let reduced = !(tr >= K_TRAIN && va >= K_VAL);
                for d in &missing {
                    out.push((
                        Found { sig: sig2(slot, *d), xf: Xf::ID, form: fm, train_ok: tr, val_ok: va, new_train: tr, new_val: va, cite: cite.clone() },
                        FamilyHow { slot, examples: idx.len(), train: tr, val: va, reduced, from: from.clone() },
                    ));
                }
                break;
            }
        }
    }
    for (slot, key) in seen {
        e.fam_scan.insert(slot, key);
    }
    out
}
