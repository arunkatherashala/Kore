//! Track L2 in World O: the state the meta-prior is folded into, the hooks the World O engine calls, the
//! family-wide rule pass with the form posterior, and the chooser with the rule-trust prior.
//!
//! Everything here is a pure function of the being's own log (the fold calls the same hooks as the live run),
//! so a replay re-derives every prior and every decision. Only observable things are used: the announced type
//! and order of families, the being's own revealed answers, rules and outcomes.

use super::meta::{trust_estimate, trust_prior, FormPrior, Trust};
use crate::being::discover::{forms, predict_x, Form, Found, Rule, Xf, MAX_CITE};
use crate::being::planner::SKIP_MIN_COST;
use crate::being::solver::{Strategy, STRATEGIES_EXT};
use crate::being::world2::engine::State2;
use crate::being::world2::learner::{r3, Act2, Choice2, Offer2, LEARNER_NAME};
use crate::being::world2::rules_use::covered;
use crate::being::world2::solve::{sig2, sig2_split};
use crate::being::world2::world::FType;
use crate::being::world_o::engine::{EngineO, LearnerO, StateO};
use crate::being::world_o::world::{Open, SKIP_COST};
use crate::versioned::NewEvent;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// Genesis tag of a being with the L2 meta-prior.
pub const TAG: &str = "wo_l2m";
/// Earlier families cited by name in a rule or an update (the counts cover all of them).
pub const CITE_FAMS: usize = 24;

/// The L2 switches (fixed at creation, logged in the genesis event).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct L2Cfg {
    /// Family-wide rules accepted by the form posterior.
    pub forms: bool,
    /// The rule-trust prior in the chooser.
    pub trust: bool,
    /// Accept a form when its posterior is at least 1 - delta (per mille).
    pub delta_milli: u32,
    /// Fewest reproduced answers for a form-posterior rule.
    pub min_ex: u32,
}

impl L2Cfg {
    /// The constants chosen on the tuning seeds (docs/measured.md, Track L2).
    pub fn on() -> L2Cfg {
        L2Cfg { forms: true, trust: true, delta_milli: 20, min_ex: 1 }
    }
    pub fn delta(&self) -> f64 {
        self.delta_milli as f64 / 1000.0
    }
    pub fn tag(&self) -> String {
        format!("{TAG}:{}/{}/{}/{}", self.forms as u8, self.trust as u8, self.delta_milli, self.min_ex)
    }
    pub fn parse(v: &str) -> Option<L2Cfg> {
        let p: Vec<&str> = v.split('/').collect();
        if p.len() != 4 {
            return None;
        }
        let b = |x: &str| match x {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        let c = L2Cfg { forms: b(p[0])?, trust: b(p[1])?, delta_milli: p[2].parse().ok()?, min_ex: p[3].parse().ok()? };
        (c.delta_milli >= 1 && c.delta_milli <= 500 && (1..=12).contains(&c.min_ex) && (c.forms || c.trust)).then_some(c)
    }
    pub fn words(&self) -> String {
        format!(
            "; learning to learn (Track L2): family-wide rules by a form posterior learned from earlier families {} (accept at posterior >= {:.3}, at least {} answers), rule-trust prior learned from earlier families {}",
            if self.forms { "on" } else { "off" },
            1.0 - self.delta(),
            self.min_ex,
            if self.trust { "on" } else { "off" }
        )
    }
}

/// What one family's closing added to the meta-prior (logged as an `l2_update` note).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct CloseUpdate {
    pub fid: u64,
    pub slot: usize,
    /// Sequence family: its family form (None: closed without a rule).
    pub seq: Option<Option<String>>,
    pub applicable: u64,
    pub agreed: u64,
    /// Post-rule record (successes, attempts) of its rules.
    pub trust: Option<(u64, u64)>,
}

/// The L2 state of a World O being, folded from its log (part of `StateO`; empty when L2 is off).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct L2State {
    pub cfg: Option<L2Cfg>,
    /// Open sequence families: slot -> family id.
    pub seq_slots: BTreeMap<usize, u64>,
    /// Sequence family -> (form code of its first plain rule, that rule's event id).
    pub fam_form: BTreeMap<u64, (String, String)>,
    /// Sequence families that closed without a rule.
    pub none: BTreeSet<u64>,
    /// Chance agreement: seen forms tried on the examples of closed families whose rule was another form.
    pub applicable: u64,
    pub agreed: u64,
    /// Closed families with a post-rule record: (family id, successes, attempts).
    pub trust_hist: Vec<(u64, u64, u64)>,
    pub version: u64,
    /// Rules accepted by the form posterior.
    pub accepted: u64,
    pub last_close: Option<CloseUpdate>,
}

impl L2State {
    pub fn on(&self) -> bool {
        self.cfg.is_some()
    }
    /// The prior over forms for family `fid`, from the other sequence families met before it.
    pub fn form_prior(&self, fid: u64) -> (FormPrior, Vec<u64>) {
        let mut by: BTreeMap<&str, u64> = BTreeMap::new();
        let mut fams = vec![];
        for (j, (code, _)) in &self.fam_form {
            if *j < fid {
                *by.entry(code.as_str()).or_default() += 1;
                fams.push(*j);
            }
        }
        let none = self.none.iter().filter(|j| **j < fid).count() as u64;
        fams.extend(self.none.iter().filter(|j| **j < fid));
        fams.sort_unstable();
        let mut forms: Vec<(String, u64)> = by.into_iter().map(|(c, n)| (c.to_string(), n)).collect();
        forms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        (FormPrior { forms, none, applicable: self.applicable, agreed: self.agreed }, fams)
    }
    pub fn trust(&self) -> (Option<Trust>, Vec<u64>) {
        let xs: Vec<(u64, u64)> = self.trust_hist.iter().map(|x| (x.1, x.2)).collect();
        (trust_prior(&xs), self.trust_hist.iter().map(|x| x.0).collect())
    }
}

/// A family is announced.
pub fn on_open(st: &mut StateO, o: &Open) {
    if o.ftype == FType::Seq {
        st.l2.seq_slots.insert(o.slot, o.fid);
    } else {
        st.l2.seq_slots.remove(&o.slot);
    }
}

/// A rule is accepted: the first plain rule of a sequence family gives the family its form.
pub fn on_rule(st: &mut StateO, r: &Rule) {
    let (slot, _) = sig2_split(r.sig);
    if let Some(fid) = st.l2.seq_slots.get(&slot).copied() {
        if r.xf.is_id() && !st.l2.fam_form.contains_key(&fid) {
            st.l2.fam_form.insert(fid, (r.form.code(), r.event_id.clone()));
            st.l2.version += 1;
        }
    }
}

/// A family closes (called before its slot is reset): fold what it taught into the meta-prior.
pub fn on_close(st: &mut StateO, slot: usize) {
    let Some(fid) = st.known.get(&slot).copied() else { return };
    let mut up = CloseUpdate { fid, slot, seq: None, applicable: 0, agreed: 0, trust: None };
    if st.l2.seq_slots.get(&slot) == Some(&fid) {
        st.l2.seq_slots.remove(&slot);
        match st.l2.fam_form.get(&fid).cloned() {
            Some((code, _)) => {
                let library: BTreeSet<String> = st.l2.fam_form.values().map(|v| v.0.clone()).filter(|c| *c != code).collect();
                for x in st.s2.examples.iter().filter(|x| sig2_split(x.sig).0 == slot) {
                    for c in &library {
                        let Some(fm) = Form::parse(c) else { continue };
                        if let Some(v) = predict_x(Xf::ID, &fm, &x.terms) {
                            up.applicable += 1;
                            up.agreed += (v == x.next) as u64;
                        }
                    }
                }
                up.seq = Some(Some(code));
            }
            None => {
                st.l2.none.insert(fid);
                up.seq = Some(None);
            }
        }
        st.l2.applicable += up.applicable;
        st.l2.agreed += up.agreed;
    }
    let (mut s, mut n) = (0u64, 0u64);
    for ((f, _, _), (pn, ps)) in &st.s2.rule_post {
        if *f == slot {
            n += pn;
            s += ps;
        }
    }
    if n > 0 {
        st.l2.trust_hist.push((fid, s, n));
        up.trust = Some((s, n));
    }
    st.l2.version += 1;
    st.l2.last_close = Some(up);
}

fn names(fams: &[u64]) -> String {
    let mut v: Vec<String> = fams.iter().rev().take(CITE_FAMS).map(|j| format!("o{j:04}")).collect();
    v.reverse();
    if fams.len() > CITE_FAMS {
        format!("{} earlier, then {}", fams.len() - CITE_FAMS, v.join(", "))
    } else {
        v.join(", ")
    }
}

/// The `l2_update` note of a family's closing: what it added and the prior now (with the families it rests on).
pub fn update_event(st: &StateO, up: &CloseUpdate, step: u64, clock: i64) -> NewEvent {
    let (fp, fams) = st.l2.form_prior(u64::MAX);
    let (tr, tfams) = st.l2.trust();
    let what = match &up.seq {
        Some(Some(c)) => format!("its family form {c} (one more family for that form; {} of {} answers of its examples were reproduced by another seen form)", up.agreed, up.applicable),
        Some(None) => "no rule (one more family for 'other')".into(),
        None => "no sequence evidence".into(),
    };
    let trust_txt = match up.trust {
        Some((s, n)) => format!("; its rules were reported right on {s} of {n} answers"),
        None => String::new(),
    };
    let text = format!(
        "Track L2 meta-prior update: family o{:04} (slot {}) closed: {what}{trust_txt}. Form prior now over {} sequence families ({}; other {}), chance agreement {:.4} ({} of {}); rule-trust prior {} from {} families ({})",
        up.fid,
        up.slot,
        fp.n(),
        fp.forms.iter().map(|(c, n)| format!("{c} {n}")).collect::<Vec<_>>().join(", "),
        fp.none,
        fp.eps(),
        fp.agreed,
        fp.applicable,
        tr.map_or("not formed yet".to_string(), |t| format!("mean {:.3} weight {:.1}", t.mean, t.k)),
        tfams.len(),
        names(&tfams)
    );
    let _ = fams;
    let tags = vec!["l2_update".to_string(), format!("fid:{}", up.fid), format!("slot:{}", up.slot), format!("at_step:{step}"), format!("l2_version:{}", st.l2.version)];
    NewEvent { source: crate::being::world_o::world::SOURCE_O.into(), occurred_at: crate::being::world::iso_at(clock), kind: "note".into(), text, tags, redacted: false }
}

/// How a form-posterior rule was accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct L2How {
    pub slot: usize,
    pub fid: u64,
    pub examples: usize,
    pub k: u32,
    pub posterior: f64,
    pub pi: f64,
    pub pi_other: f64,
    pub eps: f64,
    pub rivals: Vec<String>,
    /// Earlier families the prior rests on, and those with this form.
    pub prior_fams: Vec<u64>,
    pub form_fams: Vec<u64>,
    /// Rule events of the earlier families with this form (cited by the revision).
    pub form_events: Vec<String>,
}

impl L2How {
    pub fn text(&self, base: &str, delta: f64) -> String {
        format!(
            "{base}; family-wide rule by the Track L2 form posterior: the form reproduced {} revealed answers of this family over all its difficulties and contradicted none of its {} examples; prior from {} earlier sequence families (this form in {}: {}; prior {:.3}, other {:.3}), chance agreement {:.4} learned from closed families; rival seen forms still consistent: {}; posterior {:.4} >= {:.3}",
            self.k,
            self.examples,
            self.prior_fams.len(),
            self.form_fams.len(),
            names(&self.form_fams),
            self.pi,
            self.pi_other,
            self.eps,
            if self.rivals.is_empty() { "none".to_string() } else { self.rivals.join(", ") },
            self.posterior,
            1.0 - delta
        )
    }
    pub fn json(&self) -> serde_json::Value {
        json!({ "slot": self.slot, "family_id": self.fid, "family_examples": self.examples, "reproduced": self.k, "posterior": r3(self.posterior), "prior_form": r3(self.pi), "prior_other": r3(self.pi_other),
                "chance_agreement": self.eps, "rivals": self.rivals, "prior_families": self.prior_fams.len(), "form_families": self.form_fams })
    }
}

/// The family-wide pass with the form posterior: for each open sequence family with a difficulty without a live
/// rule, every form seen in earlier families is scored on ALL the family's examples; the best one is accepted for
/// every difficulty without a rule when its posterior is at least `1 - delta` and it reproduces `min_ex` answers.
pub fn family_pass(e: &EngineO) -> Vec<(Found, L2How)> {
    let st = &e.state;
    let Some(cfg) = st.l2.cfg.filter(|c| c.forms) else { return vec![] };
    let ex = st.s2.search_examples();
    let mut by_slot: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, x) in ex.iter().enumerate() {
        by_slot.entry(sig2_split(x.sig).0).or_default().push(i);
    }
    let grammar = forms();
    let mut out = vec![];
    for (slot, idx) in by_slot {
        let Some(fid) = st.l2.seq_slots.get(&slot).copied() else { continue };
        if st.known.get(&slot) != Some(&fid) {
            continue;
        }
        let missing: Vec<u8> = (1..=5u8).filter(|d| !st.s2.rules.iter().any(|r| !r.retired && r.sig == sig2(slot, *d))).collect();
        if missing.is_empty() {
            continue;
        }
        let (fp, prior_fams) = st.l2.form_prior(fid);
        if fp.forms.is_empty() {
            continue;
        }
        // score every seen form: (code, form, reproduced, consistent, cites)
        let mut scored: Vec<(String, Form, u32, Vec<String>)> = vec![];
        for (code, _) in &fp.forms {
            let Some(fm) = Form::parse(code).filter(|f| grammar.contains(f)) else { continue };
            let (mut k, mut wrong, mut cite) = (0u32, false, vec![]);
            for &i in &idx {
                match predict_x(Xf::ID, &fm, &ex[i].terms) {
                    None => {}
                    Some(v) if v == ex[i].next => {
                        k += 1;
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
            if !wrong && k > 0 {
                scored.push((code.clone(), fm, k, cite));
            }
        }
        let codes: Vec<String> = scored.iter().map(|s| s.0.clone()).collect();
        let best = scored
            .iter()
            .map(|s| (fp.posterior(&s.0, s.2, &codes), s))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1 .2.cmp(&b.1 .2)));
        let Some((post, (code, fm, k, cite))) = best else { continue };
        if post < 1.0 - cfg.delta() || *k < cfg.min_ex {
            continue;
        }
        let form_fams: Vec<u64> = st.l2.fam_form.iter().filter(|(j, v)| **j < fid && v.0 == *code).map(|(j, _)| *j).collect();
        let form_events: Vec<String> = st.l2.fam_form.iter().filter(|(j, v)| **j < fid && v.0 == *code).rev().take(MAX_CITE).map(|(_, v)| v.1.clone()).filter(|x| !x.is_empty()).collect();
        let how = L2How {
            slot,
            fid,
            examples: idx.len(),
            k: *k,
            posterior: post,
            pi: fp.pi(code),
            pi_other: fp.pi_other(),
            eps: fp.eps(),
            rivals: codes.iter().filter(|c| *c != code).cloned().collect(),
            prior_fams: prior_fams.clone(),
            form_fams,
            form_events,
        };
        for d in &missing {
            out.push((Found { sig: sig2(slot, *d), xf: Xf::ID, form: *fm, train_ok: *k, val_ok: 0, new_train: *k, new_val: 0, cite: cite.clone() }, how.clone()));
        }
    }
    out
}

/// What the L2 chooser reads of the World O state (copied when the meta-prior changed).
#[derive(Debug, Clone, PartialEq)]
pub struct L2View {
    pub version: u64,
    pub trust: Option<Trust>,
    pub fams: Vec<u64>,
}

pub fn prepare(l: &mut LearnerO, st: &StateO) {
    if l.l2view.as_ref().map_or(true, |v| v.version != st.l2.version) {
        let (trust, fams) = st.l2.trust();
        l.l2view = Some(L2View { version: st.l2.version, trust, fams });
    }
}

/// The B11 learner (B1 planner over families, the skip test of `skip_rule`) with the rule-trust prior in the
/// estimate of `discovered` where a live rule covers the offer (and B7's own rule estimate is not used).
pub fn choose(l: &mut LearnerO, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
    let inner = &l.inner;
    let view = l.l2view.as_ref();
    let trust = view.and_then(|v| v.trust);
    let mut cands = inner.candidates(st, offers);
    let mut from_trust = vec![false; cands.len()];
    if let Some(t) = trust {
        for (i, c) in cands.iter_mut().enumerate() {
            if c.strategy != Strategy::Discovered || c.rule.is_some() || c.lin.is_some() {
                continue;
            }
            let o = offers.iter().find(|o| o.slot == c.slot).expect("candidate of an offer");
            if !covered(st, o) {
                continue;
            }
            let cell = st.model.cell(o.fam, o.difficulty, Strategy::Discovered);
            let (p, sd) = trust_estimate(&t, cell.succ, cell.n);
            c.p = p;
            c.sd = sd;
            let pu = (p + inner.beta * sd).min(1.0);
            c.score = o.listed_reward as f64 * pu / c.cost_est;
            c.mean_score = o.listed_reward as f64 * p / c.cost_est;
            from_trust[i] = true;
        }
    }
    let mut bi: Option<usize> = None;
    for (i, c) in cands.iter().enumerate() {
        if c.affordable && bi.map_or(true, |b| c.score > cands[b].score) {
            bi = Some(i);
        }
    }
    let bi = bi?;
    let best = &cands[bi];
    let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
    let sf = inner.skip_frac;
    let bar = l.skip_rule.bar(sf, rate, best.cost_est, SKIP_COST, best.n);
    let skip = sf > 0.0 && st.total_cost >= SKIP_MIN_COST && best.score < bar && st.budget_left >= SKIP_COST;
    let alts: Vec<serde_json::Value> = cands
        .iter()
        .zip(&from_trust)
        .map(|(c, ft)| {
            json!({ "slot": c.slot, "strategy": c.strategy.name(), "n_cell": c.n, "p_success": r3(c.p), "uncertainty_sd": r3(c.sd), "est_energy": r3(c.cost_est), "score": r3(c.score), "affordable": c.affordable,
                    "estimate": if *ft { "rule-trust prior from earlier families (Track L2)" } else { "B1 model" } })
        })
        .collect();
    let used = from_trust[bi];
    let policy = LEARNER_NAME;
    let rationale = json!({
        "policy": policy, "learner": "B1 world model keyed by family with the Track L2 rule-trust prior", "beta": inner.beta, "skip_frac": sf, "skip_rule": l.skip_rule.code(), "skip_bar": r3(bar),
        "reported_reward_per_energy": r3(rate),
        "l2_trust": trust.map(|t| json!({ "mean": r3(t.mean), "weight": r3(t.k), "families": t.fams, "family_ids": view.map(|v| v.fams.iter().rev().take(CITE_FAMS).copied().collect::<Vec<u64>>()).unwrap_or_default() })),
        "alternatives": alts,
    });
    if skip {
        return Some(Choice2 { act: Act2::Skip, policy: policy.into(), why: format!("best optimistic score {:.3} is below the {} skip bar {:.3}: skip the menu", best.score, l.skip_rule.code(), bar), p: 0.0, sd: 0.0, cost_est: SKIP_COST as f64, score: 0.0, rationale });
    }
    let why = if used {
        let t = trust.expect("used");
        format!("highest optimistic reward per estimated energy; the estimate of this rule's answers starts from the rule-trust prior (mean {:.3}, weight {:.1}) learned from {} earlier families' rules", t.mean, t.k, t.fams)
    } else {
        "highest optimistic reward per estimated energy (p + beta*sd) among affordable options".into()
    };
    let _ = STRATEGIES_EXT;
    Some(Choice2 { act: Act2::Attempt { slot: best.slot, strategy: best.strategy }, policy: policy.into(), why, p: best.p, sd: best.sd, cost_est: best.cost_est, score: best.score, rationale })
}

/// The controlled entry for a World O run that may ask for the L2 meta-prior: the request (a new being whose
/// learner has `l2`) is granted only when `controller_text` (the owner's `controller.txt`, None: missing) says
/// `l2_meta_prior = on` and parses as a whole; otherwise the being is created without it and the refusal is
/// logged. An existing being keeps what it was created with.
pub fn run_o_ctl(
    log: &mut crate::versioned::VersionedLog,
    cfg: &crate::being::world_o::engine::RunOCfg,
    policy: &crate::being::policy::Policy,
    chooser: Option<&mut dyn crate::being::world2::learner::Chooser2>,
    controller_text: Option<&str>,
) -> Result<crate::being::world_o::engine::RunOSummary, crate::versioned::LogError> {
    let want = cfg.learner.map_or(false, |l| l.l2.is_some());
    let grant = super::ctl::grant(want, controller_text);
    crate::being::world_o::engine::run_o_granted(log, cfg, policy, chooser, Some(grant))
}
