//! Track S1 (`docs/s1_self_tune_prereg.md`): the being tunes its own optimism `beta` during its life.
//!
//! Opt-in (controller key `wt_self_tune`, values `on` / `off`, default off, read only when `wt_file_key` is on;
//! recorded at the genesis as the tag `wt_self_tune:on`, fixed for the life). Base: B1 of Track W2 (file-keyed
//! estimate, prior weight 2, chooser, skip rule and `value_ucb` unchanged). The only setting that moves is `beta`,
//! and only inside the fixed grid {0.5, 1, 2, 4, 8, 16}:
//!
//! * The life is split into blocks of `BLOCK` = 200 steps (block k = steps 200k .. 200k + 199).
//! * At the start of each block one beta is picked. Blocks 0..5 play each beta once, in grid order starting at 2
//!   (2, 4, 8, 16, 0.5, 1). After that: UCB1 over the grid, the arm value being the realized reward per energy of
//!   the blocks played with that beta (their reward sum over their energy sum), plus `C` x sqrt(2 ln N / n_a)
//!   (N = blocks played, n_a = blocks with that beta), C = 0.1; ties go to the lower grid value.
//! * Incumbent: beta 2 at the genesis. At each block start, before the pick, a beta with at least `MIN_BLOCKS` = 3
//!   blocks whose mean block reward per energy is above the incumbent's by more than one pooled standard error
//!   (pooled sd of the two sets of block values, x sqrt(1/n_a + 1/n_inc)) is promoted (several: the highest mean;
//!   ties: the lower grid value).
//! * After a promotion, 3 of every 4 blocks use the incumbent: counting the blocks from the promotion (j = 0, 1,
//!   ...), block j uses UCB1 when j mod 4 = 3 and the incumbent otherwise (a new promotion restarts the count).
//!
//! The block's reward and energy come from the being's own outcome events only (the fold adds each outcome to the
//! block of its step). Every pick is a `decision` event (tags `self_tune`, `st_block:<k>`, `beta_milli:<b>`,
//! `mode:forced|ucb|incumbent`, `policy:wt-self-tune`) with a `decision` revision (key
//! `being/worldt/self_tune/block/<k>`) whose `caused_by` cites the outcome events of the blocks it used (forced
//! blocks use none and cite the genesis). Every promotion is a note event (tags `self_tune`, `st_promote`) with a
//! `rule` revision under the key `being/worldt/self_tune/beta` citing the outcome events of the two betas compared
//! (and the note); block 0 writes the first revision of that key (the initial incumbent 2, citing the genesis), so
//! the key's history (time travel) lists every incumbent, and a rollback is the re-instating of an earlier revision.
//! The self-tune state is folded from these events (`fold_event`) and goes into the World T checkpoint payload
//! (snapshot format 3); `replay` re-derives every pick and every promotion from the log alone.
//!
//! Bounds: the grid, the block length, C, the 3-block minimum and the 3-of-4 schedule are constants of this file;
//! nothing in the log or the controller can change them. Nothing here reads hidden world information.

use super::engine::{ChooserT, StateT};
use super::learner::{ChoiceT, LearnerT, LearnerTCfg, LEARNER_T};
use super::world::{ViewT, SKIP_COST, SOURCE_T};
use crate::being::state::tag;
use crate::being::world::iso_at;
use crate::versioned::{event_id, EventRow, NewEvent, NewRevision};
use serde_json::json;

pub const KEY_SELF_TUNE: &str = "wt_self_tune";
pub const GRID: [u32; 6] = [500, 1000, 2000, 4000, 8000, 16000];
pub const BLOCK: u64 = 200;
pub const C: f64 = 0.1;
pub const MIN_BLOCKS: usize = 3;
/// Index of beta 2 in `GRID`: the first forced block and the initial incumbent.
pub const START: usize = 2;
pub const POLICY: &str = "wt-self-tune";
pub const REV_KEY: &str = "being/worldt/self_tune/beta";
pub const GENESIS_TAG: &str = "wt_self_tune:on";

/// One block as folded: its beta, how it was picked, and the realized reward and energy of its outcomes.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Block {
    pub k: u64,
    pub beta_milli: u32,
    pub mode: String,
    pub reward: i64,
    pub cost: i64,
    pub steps: u32,
    /// The outcome events of this block (cited by later picks).
    pub outcomes: Vec<String>,
    pub event_id: String,
}

impl Block {
    pub fn rpe(&self) -> f64 {
        self.reward as f64 / self.cost.max(1) as f64
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Promotion {
    /// The block whose start the promotion happened at.
    pub k: u64,
    pub from_milli: u32,
    pub to_milli: u32,
    pub event_id: String,
}

/// The self-tune state of a World T being (all default / off for every being without `wt_self_tune:on`).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct SelfTune {
    pub on: bool,
    pub blocks: Vec<Block>,
    pub incumbent_milli: u32,
    pub promotions: Vec<Promotion>,
}

impl SelfTune {
    pub fn on() -> SelfTune {
        SelfTune { on: true, blocks: vec![], incumbent_milli: GRID[START], promotions: vec![] }
    }

    /// Block index of the first block after the latest promotion.
    pub fn since_promotion(&self) -> Option<u64> {
        self.promotions.last().map(|p| p.k)
    }

    /// Add one outcome (step, reward, energy, event) to the block of its step.
    pub fn add_outcome(&mut self, step: u64, reward: i64, cost: i64, event_id: &str) {
        if let Some(b) = self.blocks.get_mut((step / BLOCK) as usize) {
            b.reward += reward;
            b.cost += cost;
            b.steps += 1;
            b.outcomes.push(event_id.to_string());
        }
    }

    /// Blocks per grid beta (counts).
    pub fn blocks_per_beta(&self) -> [usize; 6] {
        let mut n = [0usize; 6];
        for b in &self.blocks {
            if let Some(i) = GRID.iter().position(|g| *g == b.beta_milli) {
                n[i] += 1;
            }
        }
        n
    }
}

/// Why a new being asking for both `wt_self_tune = on` and `wt_file_beta = table` is refused: the combination is
/// not defined by either pre-registration (S1: the base is B1, beta 2 from the fixed grid; W2b: the beta comes from
/// its table), so it fails closed.
pub const COMBO_REFUSAL: &str = "wt_self_tune = on together with wt_file_beta = table is not defined (Track S1 tunes the fixed-grid beta of B1; Track W2b takes beta from its table); a new being with both is refused (fail closed)";

/// The `wt_self_tune` setting of a genesis: Some(false) without the tag (or `off`), Some(true) with `on` and the
/// file key on (and not the W2b table: that combination is not defined), None (not a valid genesis: fail closed)
/// for any other value, `on` without the file key, or `on` with `wt_file_beta:table`.
pub fn genesis_setting(tags: &str) -> Option<bool> {
    match tag(tags, KEY_SELF_TUNE) {
        None | Some("off") => Some(false),
        Some("on") if tag(tags, "wt_file_key") == Some("on") && tag(tags, "wt_file_beta") != Some("table") => Some(true),
        _ => None,
    }
}

/// The genesis event of a self-tuned being: the W2 genesis plus the tag and a sentence (only when on).
pub fn mark_genesis(mut ev: NewEvent) -> (NewEvent, String) {
    ev.tags.push(GENESIS_TAG.into());
    ev.text.push_str(&format!(
        "; self-tuned beta on (Track S1: blocks of {BLOCK} steps, UCB1 with c {C} over the fixed grid 0.5 1 2 4 8 16, the first 6 blocks each beta once starting at 2, promotion of a new incumbent after at least {MIN_BLOCKS} blocks and more than one pooled standard error, then 3 of every 4 blocks the incumbent; fixed for this being's life)"
    ));
    let id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    (ev, id)
}

/// The value of a controller text's `wt_self_tune` (strict: `on` or `off` only).
pub fn parse_value(v: &str) -> Option<bool> {
    match v {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

pub fn is_key(k: &str) -> bool {
    k == KEY_SELF_TUNE
}

pub fn validate_key(k: &str, v: &str) -> Result<(), String> {
    parse_value(v).map(|_| ()).ok_or_else(|| format!("'{k}' needs on or off"))
}

/// The `wt_self_tune` request of a controller text: Some(on/off) when set in a valid file; None when the file is
/// missing or invalid or the key is absent (fail closed: a new being is created with it off).
pub fn request(text: Option<&str>) -> Option<bool> {
    let text = text?;
    crate::being::controller::Controller::parse(text).ok()?;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == KEY_SELF_TUNE {
                return parse_value(v.trim());
            }
        }
    }
    None
}

impl StateT {
    /// The learner constants of the current step: the genesis constants, with the current block's beta when the
    /// being is self-tuned (exactly `self.learner` otherwise).
    pub fn learner_now(&self) -> LearnerTCfg {
        let mut l = self.learner;
        if self.self_tune.on {
            if let Some(b) = self.self_tune.blocks.last() {
                l.beta_milli = b.beta_milli;
            }
        }
        l
    }
}

/// The chooser of a self-tuned being: the World T learner with the current block's beta (policy `wt-model-ucb`, so
/// `replay_t` checks its decisions like any other).
pub struct SelfTunedT;

impl ChooserT for SelfTunedT {
    fn name(&self) -> String {
        LEARNER_T.into()
    }
    fn choose(&mut self, st: &StateT, v: &ViewT) -> ChoiceT {
        LearnerT { cfg: st.learner_now() }.choose(st, v)
    }
}

// ---- the block rule (pure: from the folded state only) ----

#[derive(Debug, Clone, PartialEq)]
pub struct ArmStat {
    pub beta_milli: u32,
    pub n: usize,
    pub reward: i64,
    pub cost: i64,
    /// Mean and sample sd of the per-block reward per energy.
    pub mean: f64,
    pub sd: f64,
}

pub fn arm_stats(st: &SelfTune) -> Vec<ArmStat> {
    GRID.iter()
        .map(|g| {
            let bs: Vec<&Block> = st.blocks.iter().filter(|b| b.beta_milli == *g).collect();
            let n = bs.len();
            let v: Vec<f64> = bs.iter().map(|b| b.rpe()).collect();
            let mean = if n > 0 { v.iter().sum::<f64>() / n as f64 } else { 0.0 };
            let sd = if n > 1 { (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n - 1) as f64).sqrt() } else { 0.0 };
            ArmStat { beta_milli: *g, n, reward: bs.iter().map(|b| b.reward).sum(), cost: bs.iter().map(|b| b.cost).sum(), mean, sd }
        })
        .collect()
}

/// Pooled standard error of the difference of two means (None when there are no degrees of freedom).
pub fn pooled_se(a: &ArmStat, b: &ArmStat) -> Option<f64> {
    if a.n == 0 || b.n == 0 || a.n + b.n < 3 {
        return None;
    }
    let sp2 = ((a.n - 1) as f64 * a.sd * a.sd + (b.n - 1) as f64 * b.sd * b.sd) / (a.n + b.n - 2) as f64;
    Some((sp2 * (1.0 / a.n as f64 + 1.0 / b.n as f64)).sqrt())
}

/// What the rule does at the start of block `k`.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryPlan {
    pub k: u64,
    /// (from, to, mean to, mean from, pooled se)
    pub promote: Option<(u32, u32, f64, f64, f64)>,
    pub beta_milli: u32,
    pub mode: &'static str,
    pub stats: Vec<ArmStat>,
    /// UCB1 scores (NaN when the pick is not by UCB1).
    pub ucb: Vec<f64>,
}

pub fn plan(st: &SelfTune, k: u64) -> BoundaryPlan {
    let stats = arm_stats(st);
    let inc_i = GRID.iter().position(|g| *g == st.incumbent_milli).unwrap_or(START);
    let inc = &stats[inc_i];
    let mut promote = None;
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, a) in stats.iter().enumerate() {
        if i == inc_i || a.n < MIN_BLOCKS {
            continue;
        }
        let Some(se) = pooled_se(a, inc) else { continue };
        if a.mean - inc.mean > se && best.map_or(true, |(_, m, _)| a.mean > m) {
            best = Some((i, a.mean, se));
        }
    }
    if let Some((i, m, se)) = best {
        promote = Some((st.incumbent_milli, GRID[i], m, inc.mean, se));
    }
    let incumbent = promote.map_or(st.incumbent_milli, |p| p.1);
    let since = if promote.is_some() { Some(k) } else { st.since_promotion() };
    let ucb = |stats: &[ArmStat]| -> Vec<f64> {
        let n_all: usize = stats.iter().map(|a| a.n).sum();
        stats.iter().map(|a| if a.n == 0 { f64::INFINITY } else { a.reward as f64 / a.cost.max(1) as f64 + C * (2.0 * (n_all.max(1) as f64).ln() / a.n as f64).sqrt() }).collect()
    };
    let (beta_milli, mode, scores) = if k < GRID.len() as u64 {
        (GRID[(START + k as usize) % GRID.len()], "forced", vec![f64::NAN; 6])
    } else if since.map_or(false, |s| (k - s) % 4 != 3) {
        (incumbent, "incumbent", vec![f64::NAN; 6])
    } else {
        let s = ucb(&stats);
        let mut bi = 0;
        for i in 1..s.len() {
            if s[i] > s[bi] {
                bi = i;
            }
        }
        (GRID[bi], "ucb", s)
    };
    BoundaryPlan { k, promote, beta_milli, mode, stats, ucb: scores }
}

/// Events and revisions of one block start (written by the logged run, counted by the measurement).
#[derive(Debug, Clone, Default)]
pub struct BoundaryT {
    pub events: Vec<NewEvent>,
    pub revisions: Vec<NewRevision>,
}

impl BoundaryT {
    pub fn cause_links(&self) -> usize {
        self.revisions.iter().map(|r| { let mut c = r.caused_by.clone(); c.sort(); c.dedup(); c.len() }).sum()
    }
}

fn r4(x: f64) -> f64 {
    if x.is_finite() { (x * 10_000.0).round() / 10_000.0 } else { -1.0 }
}

fn outcomes_of(st: &SelfTune, betas: &[u32]) -> Vec<String> {
    st.blocks.iter().filter(|b| betas.contains(&b.beta_milli)).flat_map(|b| b.outcomes.iter().cloned()).collect()
}

fn b(x: u32) -> f64 {
    x as f64 / 1000.0
}

/// Before a step: at the start of a block (and only if the step can run), pick the block's beta, apply it (and a
/// promotion) to the state, and return the events and revisions to log. None otherwise, and always None for a
/// being that is not self-tuned.
pub fn before_step(st: &mut StateT) -> Option<BoundaryT> {
    let s = &st.self_tune;
    if !s.on || st.step % BLOCK != 0 || s.blocks.len() as u64 != st.step / BLOCK || st.budget_left < SKIP_COST {
        return None;
    }
    let k = st.step / BLOCK;
    let p = plan(s, k);
    let at = iso_at(st.clock);
    let author = format!("rule:being-{POLICY}");
    let mut out = BoundaryT::default();
    if k == 0 {
        out.revisions.push(NewRevision {
            entity_type: "rule".into(),
            key: REV_KEY.into(),
            stance: format!("beta {:.1} (initial incumbent, from the genesis)", b(GRID[START])),
            confidence: 0.5,
            rationale: json!({ "incumbent_beta": b(GRID[START]), "why": "the base B1 of Track W2 (beta 2); recorded at the genesis" }).to_string(),
            supersedes: None,
            valid_from: at.clone(),
            author: author.clone(),
            caused_by: vec![st.genesis.clone()],
        });
    }
    let rat_stats: Vec<serde_json::Value> = p.stats.iter().zip(&p.ucb).map(|(a, u)| json!({ "beta": b(a.beta_milli), "blocks": a.n, "reward": a.reward, "energy": a.cost, "rpe": r4(a.reward as f64 / a.cost.max(1) as f64), "mean_block_rpe": r4(a.mean), "sd_block_rpe": r4(a.sd), "ucb": r4(*u) })).collect();
    if let Some((from, to, mt, mf, se)) = p.promote {
        let text = format!("world T self-tune at the start of block {k} (step {}): promotes beta {:.1} over the incumbent {:.1}: mean block reward per energy {mt:.4} vs {mf:.4}, above by more than one pooled standard error {se:.4}", st.step, b(to), b(from));
        let tags = vec!["self_tune".to_string(), "st_promote".into(), format!("st_block:{k}"), format!("from_milli:{from}"), format!("to_milli:{to}"), format!("step:{}", st.step)];
        let ev = NewEvent { source: SOURCE_T.into(), occurred_at: at.clone(), kind: "note".into(), text, tags, redacted: false };
        let id = event_id(&ev.source, &ev.occurred_at, &ev.text);
        let mut cites = outcomes_of(s, &[from, to]);
        cites.push(id.clone());
        out.revisions.push(NewRevision {
            entity_type: "rule".into(),
            key: REV_KEY.into(),
            stance: format!("beta {:.1}", b(to)),
            confidence: 0.5,
            rationale: json!({ "block": k, "from_beta": b(from), "to_beta": b(to), "mean_block_rpe_new": r4(mt), "mean_block_rpe_incumbent": r4(mf), "pooled_se": r4(se), "min_blocks": MIN_BLOCKS, "arms": rat_stats }).to_string(),
            supersedes: None,
            valid_from: at.clone(),
            author: author.clone(),
            caused_by: cites,
        });
        out.events.push(ev);
        apply_promotion(&mut st.self_tune, Promotion { k, from_milli: from, to_milli: to, event_id: id });
    }
    let s = &st.self_tune;
    let cites: Vec<String> = match p.mode {
        "forced" => vec![st.genesis.clone()],
        "incumbent" => {
            let mut v = vec![st.genesis.clone()];
            v.extend(s.promotions.last().map(|x| x.event_id.clone()));
            v.extend(outcomes_of(s, &[p.beta_milli]));
            v
        }
        _ => {
            let mut v = vec![st.genesis.clone()];
            v.extend(outcomes_of(s, &GRID));
            v
        }
    };
    let why = match p.mode {
        "forced" => format!("block {k} of the first {}: each beta once in grid order starting at 2", GRID.len()),
        "incumbent" => format!("3 of every 4 blocks after the promotion at block {}: the incumbent", s.since_promotion().unwrap_or(0)),
        _ => format!("UCB1 over the grid (realized reward per energy + {C} x sqrt(2 ln N / n)): highest {:.4}", p.ucb.iter().cloned().fold(f64::MIN, f64::max)),
    };
    let text = format!("world T self-tune: block {k} (steps {}..{}) plays beta {:.1} ({}); incumbent {:.1}; cites {} events", k * BLOCK, k * BLOCK + BLOCK - 1, b(p.beta_milli), why, b(s.incumbent_milli), cites.len());
    let tags = vec!["self_tune".to_string(), format!("st_block:{k}"), format!("beta_milli:{}", p.beta_milli), format!("mode:{}", p.mode), format!("policy:{POLICY}"), format!("step:{}", st.step), format!("incumbent_milli:{}", s.incumbent_milli)];
    let ev = NewEvent { source: SOURCE_T.into(), occurred_at: at.clone(), kind: "decision".into(), text, tags, redacted: false };
    let id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    out.revisions.push(NewRevision {
        entity_type: "decision".into(),
        key: format!("being/worldt/self_tune/block/{k:04}"),
        stance: format!("beta {:.1}", b(p.beta_milli)),
        confidence: 0.5,
        rationale: json!({ "block": k, "beta": b(p.beta_milli), "mode": p.mode, "incumbent_beta": b(s.incumbent_milli), "c": C, "arms": rat_stats }).to_string(),
        supersedes: None,
        valid_from: at,
        author,
        caused_by: cites,
    });
    out.events.push(ev);
    apply_block(&mut st.self_tune, k, p.beta_milli, p.mode, id);
    Some(out)
}

fn apply_promotion(s: &mut SelfTune, p: Promotion) {
    s.incumbent_milli = p.to_milli;
    s.promotions.push(p);
}

fn apply_block(s: &mut SelfTune, k: u64, beta_milli: u32, mode: &str, event_id: String) {
    if s.blocks.len() as u64 == k && GRID.contains(&beta_milli) {
        s.blocks.push(Block { k, beta_milli, mode: mode.into(), event_id, ..Default::default() });
    }
}

/// Fold one self-tune event (tag `self_tune`) of a self-tuned being. A value outside the grid is not folded.
pub fn fold_event(st: &mut StateT, e: &EventRow) {
    let n = |k| tag(&e.tags, k).and_then(|x| x.parse::<u64>().ok());
    let Some(k) = n("st_block") else { return };
    if e.kind == "note" && e.tags.split(',').any(|t| t == "st_promote") {
        if let (Some(f), Some(t)) = (n("from_milli"), n("to_milli")) {
            if GRID.contains(&(t as u32)) && st.self_tune.promotions.iter().all(|p| p.event_id != e.event_id) {
                apply_promotion(&mut st.self_tune, Promotion { k, from_milli: f as u32, to_milli: t as u32, event_id: e.event_id.clone() });
            }
        }
    } else if e.kind == "decision" && tag(&e.tags, "policy") == Some(POLICY) {
        if let (Some(b), Some(m)) = (n("beta_milli"), tag(&e.tags, "mode")) {
            apply_block(&mut st.self_tune, k, b as u32, m, e.event_id.clone());
        }
    }
}

/// Track S1 replay: re-derive every block pick and every promotion of a World T log from the events before it.
/// Returns (picks and promotions checked, mismatches). A mismatch is a logged pick or promotion the rule does
/// not give, a promotion the rule gives that is not logged, or a learner decision at a block start without a pick.
pub fn replay(rows: &[EventRow]) -> (u64, u64) {
    let mut st = StateT::default();
    let (mut n, mut bad) = (0u64, 0u64);
    let mut expected: Option<BoundaryPlan> = None;
    let mut promoted_here = false;
    for e in rows.iter().filter(|e| e.source == SOURCE_T) {
        let is_st = e.tags.split(',').any(|t| t == "self_tune");
        if st.self_tune.on && is_st {
            if let Some(k) = tag(&e.tags, "st_block").and_then(|x| x.parse::<u64>().ok()) {
                if expected.as_ref().map_or(true, |p| p.k != k) {
                    expected = Some(plan(&st.self_tune, k));
                    promoted_here = false;
                }
                let p = expected.as_ref().unwrap();
                let g = |key| tag(&e.tags, key).and_then(|x| x.parse::<u32>().ok());
                n += 1;
                if e.kind == "note" {
                    promoted_here = true;
                    if p.promote.map(|x| (x.0, x.1)) != g("from_milli").zip(g("to_milli")) {
                        bad += 1;
                    }
                } else if e.kind == "decision" {
                    let ok = k == st.step / BLOCK && st.step % BLOCK == 0 && Some(p.beta_milli) == g("beta_milli") && Some(p.mode) == tag(&e.tags, "mode") && promoted_here == p.promote.is_some();
                    if !ok {
                        bad += 1;
                    }
                }
            }
        } else if st.self_tune.on && e.kind == "decision" && tag(&e.tags, "policy") == Some(LEARNER_T) && st.self_tune.blocks.len() as u64 <= st.step / BLOCK {
            n += 1;
            bad += 1;
        }
        st.fold_event(e);
    }
    (n, bad)
}

// ---- measurement (in memory, no log; the logged run takes exactly the same steps, a test checks it) ----

/// What a self-tuned life did with its beta (for the measurement).
#[derive(Debug, Clone, Default)]
pub struct TuneLog {
    pub tune: SelfTune,
    /// Extra revisions (block picks, promotions, the initial incumbent) and their cause links.
    pub revisions: usize,
    pub cause_links: usize,
    /// The beta of every step.
    pub betas: Vec<u32>,
}

/// The B1 learner constants a self-tuned being starts from (W2's B1: file key on, beta 2).
pub fn s1_learner() -> LearnerTCfg {
    LearnerTCfg { file_key: true, ..Default::default() }
}

/// One self-tuned life in the measurement loop (`measure::run_life_cached`: engine with `measure`, a B2 discovery
/// pass every 10 steps), reading files from `sb` (None: the generator's bytes).
pub fn run_life_s1(cfg: &super::world::WTCfg, sb: Option<&dyn super::engine::Sandbox>, cache: &mut super::engine::ToolCache) -> (super::measure::LifeT, TuneLog) {
    use super::engine::{EngineT, MemSandbox, DEFAULT_DISCOVER_EVERY};
    let mut st = StateT::default();
    st.start(cfg.clone(), s1_learner(), "genesis".into());
    st.self_tune = SelfTune::on();
    let mut eng = EngineT::new(st, None);
    eng.measure = true;
    eng.cache = std::mem::take(cache);
    let world = super::world::WorldT::new(cfg.clone());
    let mem = MemSandbox(&world);
    let sb: &dyn super::engine::Sandbox = sb.unwrap_or(&mem);
    let mut tl = TuneLog::default();
    let (mut truths, mut rew, mut cost) = (vec![], 0, 0);
    loop {
        if let Some(bt) = before_step(&mut eng.state) {
            tl.revisions += bt.revisions.len();
            tl.cause_links += bt.cause_links();
        }
        let beta = eng.state.learner_now().beta_milli;
        let Ok(s) = eng.step(&mut SelfTunedT, sb) else { break };
        tl.betas.push(beta);
        rew += s.truth.reward;
        cost += s.truth.cost;
        truths.push(s.truth);
        if eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    *cache = std::mem::take(&mut eng.cache);
    let rules = eng.state.rules.len();
    let rules_noisy = eng.state.rules.iter().filter(|r| r.sig >= 120).count();
    let order = world.order.iter().enumerate().map(|(i, k)| (*k, world.intro_energy(i))).collect();
    tl.tune = eng.state.self_tune.clone();
    (super::measure::LifeT { truths, reward: rew, cost, rules, rules_noisy, order }, tl)
}

/// Why a run on an existing being must be refused for its `wt_self_tune` request (None: no refusal). The key is
/// read only when the being's `wt_file_key` is on; then the request must equal the genesis setting.
pub fn refusal(state: &StateT, request: Option<bool>) -> Option<String> {
    let r = request?;
    if state.cfg.is_none() || !state.learner.file_key || r == state.self_tune.on {
        return None;
    }
    let on = |b: bool| if b { "on" } else { "off" };
    Some(format!("this World T being was created with {KEY_SELF_TUNE} = {} (recorded at its genesis); the request says {}. The setting is fixed for the being's life", on(state.self_tune.on), on(r)))
}
