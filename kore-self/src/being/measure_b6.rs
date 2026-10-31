//! Track B6 measurement: transfer between domains. Everything runs on the synthetic `transfer` world
//! (author-defined families and rewards) without a log, with the same engine as the logged loop. Hidden
//! truth (the schedule of the world, the exact answers) is used ONLY here, to score the being; the being
//! never sees it.

use super::controller::Limits;
use super::discover::{self, Example, Rule};
use super::measure::{truth_in, OracleMenu};
use super::planner::{ModelChooser, DEFAULT_BETA, DEFAULT_CURIOSITY};
use super::policy::Policy;
use super::run_loop::Engine;
use super::solver::{ScriptedThinker, Strategy, STRATEGIES_EXT};
use super::state::{BeingState, Chooser, Genesis};
use super::transfer::{XferCfg, XferMode, XferRec};
use super::world::{Domain, World};

const ORACLE_OFFSET: u64 = 20_000_000;
/// Examples per (domain, difficulty) the oracle's rule search is given.
const ORACLE_EXAMPLES: u64 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// The B5 learner (discovery with composition, no transfer).
    NoTransfer,
    /// Links by rule-fit similarity, with the guard and the transferred search order.
    Transfer,
    /// Links only: no transferred search order (separates the effect of the links from that of the search order).
    LinkOnly,
    /// Control: the right source domain, the wrong cells (each difficulty borrows the next one).
    ShiftedCells,
    /// As `ShiftedCells`, guard off.
    ShiftedNoGuard,
    /// Control: links to the most dissimilar learned domain, guard on.
    WrongSim,
    /// Control: as `WrongSim`, guard off.
    WrongSimNoGuard,
    /// Knows hidden success rates and holds rules found with plenty of data.
    Oracle,
}

impl Arm {
    pub fn name(self) -> &'static str {
        match self {
            Arm::NoTransfer => "no transfer",
            Arm::Transfer => "transfer",
            Arm::LinkOnly => "links only (no search order)",
            Arm::ShiftedCells => "right domain, shifted cells, guard on",
            Arm::ShiftedNoGuard => "right domain, shifted cells, guard off",
            Arm::WrongSim => "wrong similarity, guard on",
            Arm::WrongSimNoGuard => "wrong similarity, guard off",
            Arm::Oracle => "oracle",
        }
    }
    fn mode(self) -> XferMode {
        match self {
            Arm::Transfer | Arm::LinkOnly => XferMode::On,
            Arm::ShiftedCells => XferMode::Shifted,
            Arm::ShiftedNoGuard => XferMode::ShiftedNoGuard,
            Arm::WrongSim => XferMode::WrongSim,
            Arm::WrongSimNoGuard => XferMode::WrongSimNoGuard,
            _ => XferMode::Off,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StepRec {
    pub clock0: i64,
    pub step: u64,
    /// The domain attempted (None: the menu was skipped or nothing was paid).
    pub domain: Option<Domain>,
    pub strategy: Option<Strategy>,
    pub reward: i64,
    pub cost: i64,
}

#[derive(Debug, Clone)]
pub struct B6Out {
    pub recs: Vec<StepRec>,
    /// World-clock second at which `pattern_b` and `control` start to appear.
    pub t_b: i64,
    /// (step count when accepted, rule)
    pub accepted: Vec<(u64, Rule)>,
    pub links: Vec<XferRec>,
    pub refusals: Vec<super::controller::Refusal>,
    pub state: BeingState,
}

/// Tunable settings of one run (the defaults are those chosen on the tuning seeds).
#[derive(Debug, Clone, Copy)]
pub struct B6Cfg {
    pub tau: f64,
    pub guard_nats: f64,
    pub xfer_k: f64,
    pub every: u64,
    /// Transfer settings of the controller for this run (None: no controller).
    pub max_links: Option<usize>,
}

impl Default for B6Cfg {
    fn default() -> B6Cfg {
        B6Cfg { tau: super::transfer::TAU_DEFAULT, guard_nats: super::model::GUARD_NATS, xfer_k: super::model::XFER_K, every: 10, max_links: None }
    }
}

/// Rules a data-rich discoverer finds on both sequence domains (the oracle's rule book; hidden-truth sampling, measurement only).
pub fn oracle_rules(world: &World) -> Vec<Rule> {
    let mut ex: Vec<Example> = vec![];
    for dom in [Domain::Pattern, Domain::PatternB] {
        for k in 1..=5u8 {
            for i in 0..ORACLE_EXAMPLES {
                let t = world.task_forced(ORACLE_OFFSET + i, dom, k);
                ex.push(Example { event_id: format!("oracle-{}-{k}-{i}", dom.idx()), sig: discover::sig_of(dom, k), terms: t.view.params.clone(), next: t.expected() });
            }
        }
    }
    let plain = discover::search(&ex);
    let mut blocks: Vec<discover::Form> = vec![];
    for f in &plain {
        if f.xf.is_id() && !blocks.contains(&f.form) {
            blocks.push(f.form);
        }
    }
    let found = discover::search_composed(&ex, &blocks, usize::MAX).found;
    found.iter().map(|f| Rule::new_x(f.sig, f.xf, f.form, f.train_ok, f.val_ok, 0, blocks.len() as u32)).collect()
}

pub fn run_b6(seed: u64, budget: i64, arm: Arm, cfg: B6Cfg, policy: &Policy) -> B6Out {
    let world = World::with_transfer(seed, budget);
    let t_b = world.transfer.as_ref().map_or(0, |x| x.at);
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed: seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model, extended: true, ext2: true, rule_drift: false, drift: false, adapt: Default::default(), advice: Default::default(), transfer: true, learn_transfer: false }, String::new());
    st.xfer.cfg = XferCfg { mode: arm.mode(), tau: cfg.tau, reorder: arm != Arm::LinkOnly, ..XferCfg::default() };
    st.model.xfer_k = cfg.xfer_k;
    st.model.guard_nats = cfg.guard_nats;
    let mut oracle: Option<OracleMenu> = None;
    if arm == Arm::Oracle {
        let rules = oracle_rules(&world);
        for r in &rules {
            st.apply_rule(r.clone());
        }
        oracle = Some(OracleMenu { truth: truth_in(&world, &rules), strategies: STRATEGIES_EXT.to_vec() });
    }
    let limits = Limits { compose: true, transfer: true, max_links: cfg.max_links.unwrap_or(usize::MAX), ..Default::default() };
    let mut e = Engine::new(world, st, &th).with_limits(Some(limits));
    let mut learner = ModelChooser { curiosity: DEFAULT_CURIOSITY, ..ModelChooser::new(DEFAULT_BETA) };
    let (mut recs, mut accepted) = (vec![], vec![]);
    let (mut done, mut cost) = (0u64, 0i64);
    loop {
        if policy.gate_cost(done, cost).is_some() {
            break;
        }
        let clock0 = e.state.clock;
        let r = match oracle.as_mut() {
            Some(o) => e.step_menu(o),
            None => e.step_menu(&mut learner),
        };
        let Ok(o) = r else { break };
        let a = o.outcome;
        done += 1;
        cost += a.cost;
        recs.push(StepRec { clock0, step: a.step, domain: if a.skipped { None } else { Some(a.domain) }, strategy: a.strategy, reward: a.reward, cost: a.cost });
        if cfg.every > 0 && arm != Arm::Oracle && e.state.step % cfg.every == 0 {
            for nr in e.discover_pass() {
                accepted.push((e.state.step, nr.rule));
            }
        }
    }
    let links = e.state.xfer.log.clone();
    B6Out { recs, t_b, accepted, links, refusals: e.refusals_total.clone(), state: e.state }
}

fn rate(r: i64, c: i64) -> f64 {
    if c > 0 { r as f64 / c as f64 } else { 0.0 }
}

impl B6Out {
    pub fn total_rate(&self) -> f64 {
        let (r, c) = self.recs.iter().fold((0, 0), |a, x| (a.0 + x.reward, a.1 + x.cost));
        rate(r, c)
    }
    /// Steps that started at or after `t_b`.
    pub fn after(&self) -> impl Iterator<Item = &StepRec> + '_ {
        self.recs.iter().filter(move |s| s.clock0 >= self.t_b)
    }
    /// Reward per energy of the first `n` steps after the new domains appear (all domains, skips included).
    pub fn rate_first(&self, n: usize) -> f64 {
        let (r, c) = self.after().take(n).fold((0, 0), |a, x| (a.0 + x.reward, a.1 + x.cost));
        rate(r, c)
    }
    /// Reward per energy of all steps after the new domains appear.
    pub fn rate_after(&self) -> f64 {
        let (r, c) = self.after().fold((0, 0), |a, x| (a.0 + x.reward, a.1 + x.cost));
        rate(r, c)
    }
    /// (attempts, reward, energy) on `d` in the first `n` steps after the new domains appear.
    pub fn domain_first(&self, d: Domain, n: usize) -> (usize, i64, i64) {
        self.after().take(n).filter(|s| s.domain == Some(d)).fold((0, 0, 0), |a, x| (a.0 + 1, a.1 + x.reward, a.2 + x.cost))
    }
    /// Reward per energy on attempts at `d` over the whole time after the new domains appear.
    pub fn domain_rate_after(&self, d: Domain) -> (usize, f64) {
        let (n, r, c) = self.domain_first(d, usize::MAX);
        (n, rate(r, c))
    }
    /// First time the last `w` attempts on `d` (after `t_b`) reach `frac * target` reward per energy:
    /// (steps since `t_b` up to and including that attempt, energy since `t_b`).
    pub fn time_to(&self, d: Domain, target: f64, frac: f64, w: usize) -> Option<(usize, i64)> {
        let mut win: std::collections::VecDeque<(i64, i64)> = Default::default();
        let (mut steps, mut energy) = (0usize, 0i64);
        for s in self.after() {
            steps += 1;
            energy += s.cost;
            if s.domain == Some(d) {
                win.push_back((s.reward, s.cost));
                if win.len() > w {
                    win.pop_front();
                }
                if win.len() == w {
                    let (r, c) = win.iter().fold((0, 0), |a, x| (a.0 + x.0, a.1 + x.1));
                    if rate(r, c) >= frac * target {
                        return Some((steps, energy));
                    }
                }
            }
        }
        None
    }
    /// Rules accepted for `pattern_b` signatures: (steps after `t_b` at which the first rule of each difficulty was accepted).
    pub fn b_rule_steps(&self) -> Vec<usize> {
        let step_tb = self.recs.iter().position(|s| s.clock0 >= self.t_b).unwrap_or(self.recs.len()) as u64;
        let mut first: std::collections::BTreeMap<u8, u64> = Default::default();
        for (st, r) in &self.accepted {
            if r.sig >= 10 {
                first.entry(r.sig).or_insert(*st);
            }
        }
        first.values().map(|s| s.saturating_sub(step_tb) as usize).collect()
    }
    /// Rules for `pattern` accepted before the new domains appeared.
    pub fn a_rules_before_tb(&self) -> usize {
        let step_tb = self.recs.iter().position(|s| s.clock0 >= self.t_b).unwrap_or(self.recs.len()) as u64;
        self.accepted.iter().filter(|(st, r)| r.sig < 10 && *st <= step_tb).count()
    }
    pub fn n_links(&self) -> usize {
        self.links.iter().filter(|l| matches!(l.kind, super::transfer::XferKind::Link { .. })).count()
    }
    pub fn n_revoked(&self) -> usize {
        self.links.iter().filter(|l| matches!(l.kind, super::transfer::XferKind::Revoke { .. })).count()
    }
    /// Links per target domain: (pattern_b, control).
    pub fn links_by_target(&self) -> (usize, usize) {
        let l = self.links.iter().filter(|l| matches!(l.kind, super::transfer::XferKind::Link { .. }));
        let (mut b, mut c) = (0, 0);
        for x in l {
            match x.target {
                Domain::PatternB => b += 1,
                Domain::Control => c += 1,
                _ => {}
            }
        }
        (b, c)
    }
}

/// Rules of a run scored on 300 fresh tasks of their own (domain, difficulty). Returns (exact, wrong, inert, details):
/// exact = never applicable-and-wrong and right at least once; wrong = applicable-and-wrong at least once;
/// inert = never applicable on any of the fresh tasks (accepted on a coincidence of its examples, never wrong).
pub fn wrong_rules(seed: u64, budget: i64, out: &B6Out) -> (usize, usize, usize, Vec<String>) {
    let w = World::with_transfer(seed, budget);
    let (mut exact, mut wrong, mut inert) = (0, 0, 0);
    let mut det = vec![];
    for (_, r) in &out.accepted {
        let (dom, k) = discover::sig_split(r.sig);
        let (mut ok, mut bad) = (0u32, 0u32);
        for j in 0..300u64 {
            let t = w.task_forced(ORACLE_OFFSET + 1000 + j, dom, k);
            match discover::predict_x(r.xf, &r.form, &t.view.params) {
                Some(v) if t.check(v) => ok += 1,
                Some(_) => bad += 1,
                None => {}
            }
        }
        if bad == 0 && ok > 0 {
            exact += 1;
        } else if bad > 0 {
            wrong += 1;
            det.push(format!("WRONG {} sig {} {}+{} applicable-and-wrong on {bad} of 300", dom.name(), r.sig, r.xf.code(), r.form.code()));
        } else {
            inert += 1;
            det.push(format!("INERT {} sig {} {}+{} never applicable on 300 fresh tasks", dom.name(), r.sig, r.xf.code(), r.form.code()));
        }
    }
    (exact, wrong, inert, det)
}
