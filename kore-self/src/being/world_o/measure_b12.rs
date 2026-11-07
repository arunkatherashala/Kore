//! Track B12 measurement (learning to learn): the World O run of `measure.rs` with per-family records of
//! how a new family is first met (offers, attempts, skips, examples, the being's starting estimate against
//! the hidden rates, rules), the rule-search inputs, and wrong rules. Measurement only: nothing here is read
//! by the being and nothing is written to a log. With the B12 switches off the run is exactly `run_wo`
//! (a test compares the windows).

use super::engine::{EngineO, LearnerOCfg, StateO, TrueO};
use super::measure::{FamRec, RunWO, TruthO, WinO, COMP_K, COMP_LEVEL};
use super::world::{WOCfg, WorldO};
use crate::being::controller::Limits;
use crate::being::discover::{predict_x, Rule};
use crate::being::solver::{Strategy, STRATEGIES_EXT};
use crate::being::world2::learner::Chooser2;
use crate::being::world2::solve::{sig2, sig2_split};
use crate::being::world2::world::FType;
use std::collections::HashMap;

/// Menus counted as "the first offers" of a new family.
pub const FIRST_N: u64 = 100;
const FIXED: [Strategy; 3] = [Strategy::Fast, Strategy::Careful, Strategy::Deep];

/// How one family was met (measurement only).
#[derive(Debug, Clone, Default)]
pub struct FamDiag {
    pub fid: u64,
    pub tier: u32,
    pub ftype: Option<FType>,
    /// Menus on which it appeared (counted once per menu), in its first `FIRST_N` such menus:
    pub first_menus: u64,
    pub first_attempts: u64,
    pub first_skips: u64,
    /// Examples (revealed sequence answers) it gave in those menus.
    pub first_examples: u64,
    /// The being's starting estimate (first offer, all cells empty) of the strategy that is truly best at
    /// that offer, and that strategy's true success probability.
    pub start_est_p: f64,
    pub start_true_p: f64,
    /// The being's starting optimistic score of its best option on this family against the skip bar (>= 0: above).
    pub start_margin: f64,
    pub start_seen: bool,
    /// Whole life: menus, attempts, examples, rules accepted for its slot, attempts before the first rule.
    pub menus: u64,
    pub attempts: u64,
    pub examples: u64,
    pub rules: u64,
    pub attempts_before_rule: Option<u64>,
    /// Compose families: attempts on it, and how many of them had a live rule of its base slot at that difficulty.
    pub compose_attempts: u64,
    pub compose_covered: u64,
}

/// One accepted rule, judged against the hidden rule (measurement only).
#[derive(Debug, Clone)]
pub struct RuleJudge {
    pub fid: Option<u64>,
    pub step: u64,
    pub window: u64,
    /// Share right among sampled tasks of its (family, difficulty) where it applies (truth now).
    pub applies: u64,
    pub right: u64,
    /// Accepted with the B12 reduced support (from the rule-form prior).
    pub reduced: bool,
}

impl RuleJudge {
    /// Wrong: applicable on some sampled tasks and wrong on more than 5% of those (the B2 measure was right on all).
    pub fn wrong(&self) -> bool {
        self.applies > 0 && (self.right as f64) < 0.95 * self.applies as f64
    }
}

/// Rule-search inputs per window.
#[derive(Debug, Clone, Default)]
pub struct SearchWin {
    /// Signatures with examples at the window end, with at least 4 examples, with a live rule.
    pub sigs: u64,
    pub sigs4: u64,
    pub sigs_ruled: u64,
    pub examples: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RunB12 {
    pub run: RunWO,
    pub diag: Vec<FamDiag>,
    pub judged: Vec<RuleJudge>,
    pub search: Vec<SearchWin>,
    /// B12 bookkeeping of the being (exploration energy, reduced-support acceptances), when on.
    pub explore_energy: i64,
    pub explore_attempts: u64,
    pub reduced_rules: u64,
}

fn judge(w: &WorldO, rule: &Rule, fid: Option<u64>, clock: i64) -> (u64, u64) {
    let Some(j) = fid else { return (0, 0) };
    let (_, d) = sig2_split(rule.sig);
    let f = &w.fams[j as usize];
    if f.ftype != FType::Seq {
        return (0, 0);
    }
    let (mut a, mut r) = (0u64, 0u64);
    for i in 0..60u64 {
        let t = w.task((1 << 41) + i, (i % w.cfg.menu as u64) as usize, j, Some(d), clock, 0x12B);
        if let Some(x) = predict_x(rule.xf, &rule.form, &t.view.params) {
            a += 1;
            r += t.check(x) as u64;
        }
    }
    (a, r)
}

/// Run a being in World O without a log (as `run_wo`, same `EngineO::step`), with the B12 records.
pub fn run_b12(cfg: &WOCfg, learner: LearnerOCfg, chooser: Option<&mut dyn Chooser2>, discover_every: u64, limits: Option<Limits>, max_steps: u64, window: i64, competence: bool) -> RunB12 {
    let mut st = StateO::default();
    st.start(cfg.clone(), learner, String::new());
    let mut own = learner.learner();
    let own_used = chooser.is_none();
    let chooser: &mut dyn Chooser2 = match chooser {
        Some(c) => c,
        None => &mut own,
    };
    let mut e = EngineO::new(st, limits);
    let mut truth = TruthO::default();
    let mut out = RunWO::default();
    let mut fams: HashMap<u64, FamRec> = HashMap::new();
    let mut diag: HashMap<u64, FamDiag> = HashMap::new();
    let mut judged = vec![];
    let mut search = vec![];
    let mut energy = 0i64;
    let mut w = WinO::default();
    let mut tier_sum = 0u64;
    let mut opened_before = 0u64;
    let _ = own_used;
    while out.steps < max_steps {
        let clock0 = e.state.s2.clock;
        let rules_before: Vec<Rule> = if competence { e.state.s2.rules.clone() } else { vec![] };
        // what the menu holds, before the step (the engine draws the same offers)
        let menu_fids: Vec<(u64, u8, usize)> = if competence {
            let tasks = e.world.offers(e.state.s2.step, clock0);
            let mut v: Vec<(u64, u8, usize)> = vec![];
            for t in &tasks {
                if !v.iter().any(|x| x.0 == t.fid) {
                    v.push((t.fid, t.view.difficulty, t.view.slot));
                }
            }
            // the starting estimate of a family met for the first time
            for (fid, d, m) in &v {
                let dg = diag.entry(*fid).or_insert_with(|| {
                    let f = &e.world.fams[*fid as usize];
                    FamDiag { fid: *fid, tier: f.tier, ftype: Some(f.ftype), ..Default::default() }
                });
                if !dg.start_seen && e.state.s2.total_cost >= crate::being::planner::SKIP_MIN_COST {
                    dg.start_seen = true;
                    let slot = e.world.fams[*fid as usize].slot;
                    let mut best: Option<(f64, Strategy, f64)> = None;
                    for s in FIXED {
                        let p = truth.p(&e.world, *fid, *d, Some(s), clock0, &[]);
                        let x = p / s.energy(tasks[*m].view.cost) as f64;
                        if best.map_or(true, |b| x > b.0) {
                            best = Some((x, s, p));
                        }
                    }
                    let (_, s, p) = best.expect("three strategies");
                    dg.start_est_p = e.state.s2.model.estimate(slot, *d, s).p;
                    dg.start_true_p = p;
                    // the being's best optimistic score on this family's offer against the B1 bar
                    let l = learner.learner();
                    let offer = super::engine::offers_of_o(&tasks[*m..*m + 1]);
                    let cands = l.inner.candidates(&e.state.s2, &offer);
                    let top = cands.iter().filter(|c| c.affordable).map(|c| c.score).fold(f64::NEG_INFINITY, f64::max);
                    let rate = e.state.s2.total_reward as f64 / e.state.s2.total_cost.max(1) as f64;
                    dg.start_margin = top - l.inner.skip_frac * rate;
                }
            }
            v
        } else {
            vec![]
        };
        let o = match e.step(chooser) {
            Ok(o) => o,
            Err(r) => {
                out.stopped = r.as_str().into();
                break;
            }
        };
        out.steps += 1;
        let t: &TrueO = &o.truth;
        energy += o.outcome.cost;
        out.true_reward += t.true_reward;
        out.cost += o.outcome.cost;
        w.steps += 1;
        w.true_reward += t.true_reward;
        w.cost += o.outcome.cost;
        if t.attempted { w.attempts += 1 } else { w.skips += 1 }
        if t.rule_used {
            w.rule_uses += 1;
            w.rule_right += t.true_correct as u64;
        }
        if t.attempted {
            tier_sum += t.tier as u64;
        }
        if competence {
            for (fid, _, _) in &menu_fids {
                let dg = diag.get_mut(fid).expect("entered above");
                dg.menus += 1;
                let first = dg.first_menus < FIRST_N;
                if first {
                    dg.first_menus += 1;
                }
                let mine = t.attempted && t.fid == Some(*fid);
                if mine {
                    dg.attempts += 1;
                    let ex = o.outcome.example.is_some() as u64;
                    dg.examples += ex;
                    if first {
                        dg.first_attempts += 1;
                        dg.first_examples += ex;
                    }
                    if t.ftype == Some(FType::Compose) {
                        dg.compose_attempts += 1;
                        let f = &e.world.fams[*fid as usize];
                        let bs = f.base.map(|b| e.world.fams[b as usize].slot).unwrap_or(f.slot);
                        dg.compose_covered += rules_before.iter().any(|r| !r.retired && r.sig == sig2(bs, t.difficulty)) as u64;
                    }
                } else if !t.attempted && first {
                    dg.first_skips += 1;
                }
            }
            if let (Some(task), true) = (&o.task, t.attempted) {
                let fr = fams.entry(task.fid).or_insert_with(|| {
                    let f = &e.world.fams[task.fid as usize];
                    FamRec { fid: f.id, tier: f.tier, ftype: Some(f.ftype), intro: f.intro, retire: f.retire, ..Default::default() }
                });
                let (cx, cp) = truth.ceiling(&e.world, task, clock0);
                let p = truth.p(&e.world, task.fid, task.view.difficulty, t.strategy, clock0, &rules_before);
                let x = p / t.cost.max(1) as f64;
                fr.attempts += 1;
                fr.ceil_p_sum += cp;
                fr.ratios.push(if cx > 0.0 { (x / cx).min(1.0) as f32 } else { 1.0 });
                if fr.competent_after.is_none() && fr.ratios.len() >= COMP_K {
                    let m: f64 = fr.ratios[fr.ratios.len() - COMP_K..].iter().map(|r| *r as f64).sum::<f64>() / COMP_K as f64;
                    if m >= COMP_LEVEL {
                        fr.competent_after = Some((fr.attempts, clock0 / crate::being::world::SECS_PER_ENERGY - fr.intro));
                    }
                }
            }
        }
        if discover_every > 0 && e.state.s2.step % discover_every == 0 {
            let new = e.discover_pass();
            w.rules_accepted += new.len() as u64;
            if competence {
                for nr in &new {
                    let (slot, _) = sig2_split(nr.rule.sig);
                    let fid = e.state.known.get(&slot).copied();
                    let (a, r) = judge(&e.world, &nr.rule, fid, e.state.s2.clock);
                    let reduced = nr.event.tags.iter().any(|t| t == "b12_reduced");
                    judged.push(RuleJudge { fid, step: e.state.s2.step, window: w.index, applies: a, right: r, reduced });
                    if let Some(j) = fid {
                        if let Some(dg) = diag.get_mut(&j) {
                            dg.rules += 1;
                            if dg.attempts_before_rule.is_none() {
                                dg.attempts_before_rule = Some(dg.attempts);
                            }
                        }
                    }
                }
            }
        }
        let _ = e.take_refusals();
        out.max_live_rules = out.max_live_rules.max(e.state.s2.live_rules());
        if energy >= (w.index as i64 + 1) * window {
            w.families_opened = e.state.opened - opened_before;
            opened_before = e.state.opened;
            w.mean_tier_x1000 = if w.attempts > 0 { tier_sum * 1000 / w.attempts } else { 0 };
            tier_sum = 0;
            if competence {
                search.push(search_win(&e.state));
            }
            let next = w.index + 1;
            out.wins.push(std::mem::take(&mut w));
            w.index = next;
        }
    }
    if w.steps > 0 {
        w.families_opened = e.state.opened - opened_before;
        w.mean_tier_x1000 = if w.attempts > 0 { tier_sum * 1000 / w.attempts } else { 0 };
        if competence {
            search.push(search_win(&e.state));
        }
        out.wins.push(w);
    }
    out.rules_total = out.wins.iter().map(|w| w.rules_accepted).sum();
    out.refusals = e.refusals_total.len() as u64;
    let mut fv: Vec<FamRec> = fams.into_values().collect();
    fv.sort_by_key(|f| f.fid);
    out.fams = fv;
    let mut dv: Vec<FamDiag> = diag.into_values().collect();
    dv.sort_by_key(|f| f.fid);
    let (explore_energy, explore_attempts, reduced_rules) = e.state.b12_counters();
    RunB12 { run: out, diag: dv, judged, search, explore_energy, explore_attempts, reduced_rules }
}

fn search_win(st: &StateO) -> SearchWin {
    let mut per: HashMap<u8, u64> = HashMap::new();
    for x in &st.s2.examples {
        *per.entry(x.sig).or_default() += 1;
    }
    let ruled = |sig: u8| st.s2.rules.iter().any(|r| r.sig == sig && !r.retired);
    SearchWin {
        sigs: per.len() as u64,
        sigs4: per.values().filter(|n| **n >= 4).count() as u64,
        sigs_ruled: per.keys().filter(|s| ruled(**s)).count() as u64,
        examples: st.s2.examples.len() as u64,
    }
}

/// The strategies (for callers that print per-strategy tables).
pub fn strategies() -> [Strategy; 4] {
    STRATEGIES_EXT
}
