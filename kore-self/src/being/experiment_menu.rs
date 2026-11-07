//! Track B9 in the first (menu) world: the B1 planner (`ModelChooser`, unchanged) plus experiments
//! (`experiment.rs`), for the B2 extended and B5 extended2 worlds. Opt-in, measurement and tests only
//! (the logged being of the first world is unchanged; the World v2 being has the logged version).
//!
//! An exploit step is exactly the planner's choice (policy `model-ucb`); an experiment step has policy
//! `experiment` and its decision rationale carries the live hypotheses, the predicted split, the value
//! of information and the budget (the engine stores the rationale in the decision revision). The
//! prefix of an offered pattern task is read from the offers rebuilt from the world (what the being is
//! shown); only `domain`, `difficulty`, `reward` and `params` of an offer are read.

use super::experiment::{decide, pick_json, Decision, ExpBook, ExpCfg, HypCache, Opt, Pick, Probe};
use super::discover::sig_of;
use super::planner::{Action, Choice, MenuChooser, ModelChooser, Offer};
use super::state::BeingState;
use super::world::{Domain, World};
use serde_json::json;

pub const EXP_POLICY1: &str = "experiment";

#[derive(Debug, Clone, PartialEq)]
pub struct ExpRec1 {
    pub step: u64,
    pub energy_before: i64,
    pub pick: Pick,
}

pub struct Exp1 {
    pub base: ModelChooser,
    pub cfg: ExpCfg,
    world: World,
    pub cache: HypCache,
    pub book: ExpBook,
    pub life_budget: i64,
    pub recs: Vec<ExpRec1>,
    /// Use the being's verified forms as building blocks of the hypothesis space (B5 composition on).
    pub compose_blocks: bool,
}

impl Exp1 {
    pub fn new(base: ModelChooser, cfg: ExpCfg, world: World, life_budget: i64) -> Exp1 {
        Exp1 { base, cfg, world, cache: HypCache::new(), book: ExpBook::default(), life_budget, recs: vec![], compose_blocks: false }
    }
}

fn blocks_of(st: &BeingState) -> Vec<super::discover::Form> {
    let mut v = vec![];
    for r in st.rules.iter().filter(|r| r.xf.is_id() && !r.retired) {
        if !v.contains(&r.form) {
            v.push(r.form);
        }
    }
    v
}

impl MenuChooser for Exp1 {
    fn name(&self) -> String {
        EXP_POLICY1.into()
    }
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        let base = self.base.choose(st, offers)?;
        let blocks = if self.compose_blocks { blocks_of(st) } else { vec![] };
        self.cache.update(&st.examples, &blocks);
        let tasks = self.world.offers_at(st.step, st.clock);
        let probes: Vec<Probe> = offers
            .iter()
            .filter(|o| matches!(o.domain, Domain::Pattern | Domain::PatternB))
            .filter_map(|o| tasks.get(o.slot).filter(|t| t.view.domain == o.domain && t.view.difficulty == o.difficulty).map(|t| Probe { slot: o.slot, sig: sig_of(o.domain, o.difficulty), prefix: t.view.params.clone(), listed_reward: t.view.reward }))
            .collect();
        let cands = self.base.candidates(st, offers);
        let opts: Vec<Opt> = cands
            .iter()
            .map(|c| {
                let listed = offers.iter().find(|o| o.slot == c.slot).map_or(0, |o| o.listed_cost);
                Opt { slot: c.slot, strategy: c.strategy, p: c.p, cost_est: c.cost_est, mean_score: c.mean_score, affordable: c.affordable && c.strategy.energy(listed) <= st.budget_left, energy: c.strategy.energy(listed) }
            })
            .collect();
        match decide(&self.cfg, &self.book, &self.cache, &st.rules, &probes, &opts, self.life_budget, st.total_cost, st.step) {
            Decision::Exploit { considered, refused } => {
                if refused == Some("max_experiments_per_run") {
                    self.book.refused_count += 1;
                } else if refused.is_some() {
                    self.book.refused_energy += 1;
                }
                let mut ch = base;
                if let (Some(p), Some(obj)) = (considered, ch.rationale.as_object_mut()) {
                    obj.insert("experiment_considered".into(), pick_json(&p, false, &self.book, &self.cfg, refused));
                }
                Some(ch)
            }
            Decision::Experiment(p) => {
                let c = cands.iter().find(|c| c.slot == p.slot && c.strategy == p.strategy).expect("candidate");
                self.book.take(&p);
                let mut rationale = json!({ "policy": EXP_POLICY1, "planner_choice": base.action.label(), "planner_score": base.score });
                rationale["experiment"] = pick_json(&p, true, &self.book, &self.cfg, None);
                self.recs.push(ExpRec1 { step: st.step, energy_before: st.total_cost, pick: p.clone() });
                Some(Choice { action: Action::Attempt { slot: p.slot, strategy: p.strategy }, policy: EXP_POLICY1.into(), why: p.why.clone(), p: c.p, sd: c.sd, cost_est: c.cost_est, score: p.score, rationale, advice: None })
            }
        }
    }
}
