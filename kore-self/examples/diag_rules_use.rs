//! Diagnostics for Track B7 (using discovered rules): where rules are accepted, how the planner sees
//! them, why the `discovered` strategy is rarely chosen. Measurement knows the world; the learner does not.
//!   cargo run -p kore-self --example diag_rules_use --offline --release -- [tune|report]
//! Default: tuning seeds 101..=120.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::discover::apply;
use being::solver::Strategy;
use being::world2::engine::State2;
use being::world2::learner::{Choice2, Chooser2, Learner2, Offer2};
use being::world2::measure::run_w2_with;
use being::world2::engine::LearnerCfg;
use being::world2::rules_use::UseCfg;
use being::world2::solve::sig2;
use being::world2::world::{FType, W2Cfg, World2};
use std::collections::HashMap;

#[derive(Default, Debug)]
struct Diag {
    /// offers of a seq/compose cell where a live rule exists for the (base) signature
    offers_with_rule: u64,
    /// ... where the rule applies to the prefix
    offers_rule_applies: u64,
    /// ... and the rule's answer is right (no slip)
    rule_right: u64,
    /// ... and careful is right / deep is right on that task (no slip)
    careful_right: u64,
    deep_right: u64,
    /// the learner chose this slot
    slot_chosen: u64,
    /// with discovered
    disc_chosen: u64,
    /// discovered's optimistic score rank in that slot (0 = best)
    disc_rank_sum: u64,
    /// discovered estimate p, n_cell, and best other p in slot
    disc_p_sum: f64,
    disc_n_sum: u64,
    best_other_p_sum: f64,
    /// discovered score / best overall score in the menu
    score_ratio_sum: f64,
    /// the cell has never been tried with discovered
    disc_n0: u64,
}

struct Spy {
    l: Learner2,
    world: World2,
    d: Diag,
}

impl Chooser2 for Spy {
    fn name(&self) -> String {
        self.l.name()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let ch = self.l.choose(st, offers)?;
        let cands = self.l.candidates(st, offers);
        let best_all = cands.iter().filter(|c| c.affordable).map(|c| c.score).fold(0.0, f64::max);
        let tasks = self.world.offers(st.step, st.clock);
        for o in offers {
            if !matches!(o.ftype, FType::Seq | FType::Compose) {
                continue;
            }
            let t = &tasks[o.slot];
            let base = t.view.base.unwrap_or(o.fam);
            let sig = sig2(base, o.difficulty);
            if !st.rules.iter().any(|r| r.sig == sig && !r.retired) {
                continue;
            }
            self.d.offers_with_rule += 1;
            let rp = being::world2::solve::solve(&t.view, Strategy::Discovered, &st.rules);
            if rp.rule_id.is_some() {
                self.d.offers_rule_applies += 1;
                self.d.rule_right += t.check(rp.answer) as u64;
            }
            self.d.careful_right += t.check(being::world2::solve::solve(&t.view, Strategy::Careful, &[]).answer) as u64;
            self.d.deep_right += t.check(being::world2::solve::solve(&t.view, Strategy::Deep, &[]).answer) as u64;
            let mine: Vec<_> = cands.iter().filter(|c| c.slot == o.slot).collect();
            let disc = mine.iter().find(|c| c.strategy == Strategy::Discovered).unwrap();
            let rank = mine.iter().filter(|c| c.score > disc.score).count();
            self.d.disc_rank_sum += rank as u64;
            self.d.disc_p_sum += disc.p;
            self.d.disc_n_sum += disc.n;
            self.d.disc_n0 += (disc.n == 0) as u64;
            self.d.best_other_p_sum += mine.iter().filter(|c| c.strategy != Strategy::Discovered).map(|c| c.p).fold(0.0, f64::max);
            self.d.score_ratio_sum += disc.score / best_all.max(1e-9);
            if let being::world2::learner::Act2::Attempt { slot, strategy } = ch.act {
                if slot == o.slot {
                    self.d.slot_chosen += 1;
                    self.d.disc_chosen += (strategy == Strategy::Discovered) as u64;
                }
            }
        }
        Some(ch)
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "tune".into());
    let seeds: Vec<u64> = if mode == "report" { (1..=20).collect() } else { (101..=120).collect() };
    let use_ = UseCfg { rule_est: std::env::args().any(|a| a == "A"), robust_milli: if std::env::args().any(|a| a == "B") { 600 } else { 0 }, fit_const: std::env::args().any(|a| a == "C"), ..UseCfg::default() };
    let lc = LearnerCfg { use_, ..LearnerCfg::default() };
    let conds = [("small clean", 6, 1500i64, 0u32), ("medium clean", 12, 3000, 0), ("medium noisy", 12, 3000, 100), ("large clean", 24, 6000, 0)];
    for (name, fams, budget, noise) in conds {
        let mut tot = Diag::default();
        let (mut rules, mut useful, mut redundant, mut wrong) = (0, 0, 0, 0);
        let mut by_class: HashMap<String, (u64, u64)> = HashMap::new();
        let mut disc_att = 0;
        let mut groups: std::collections::BTreeMap<String, [i64; 4]> = Default::default();
        let mut rule_att = 0;
        for &s in &seeds {
            let cfg = W2Cfg { noise_pm: noise, ..W2Cfg::new(s, fams, budget) };
            let w = World2::new(cfg.clone());
            let mut spy = Spy { l: lc.learner(), world: w.clone(), d: Diag::default() };
            let run = run_w2_with(&cfg, &mut spy, 10, None, lc);
            disc_att += run.recs.iter().filter(|x| x.attempted && x.strategy == Some(Strategy::Discovered)).count();
            rule_att += run.recs.iter().filter(|x| x.attempted && x.rule_used).count();
            for rc in &run.rules {
                rules += 1;
                wrong += rc.wrong_at_accept as usize;
                let (fam, _) = being::world2::solve::sig2_split(rc.rule.sig);
                let cls = w.regime(fam, rc.clock).class;
                let need = !matches!(cls.name(), "arith" | "geom" | "quad" | "fib");
                if need { useful += 1 } else { redundant += 1 }
                let e = by_class.entry(cls.name().to_string()).or_default();
                e.0 += 1;
            }
            let orun = run_w2_with(&cfg, &mut being::world2::baselines::Oracle2::new(w.clone(), true), 0, None, LearnerCfg::default());
            for (k, rr) in [(0usize, &run), (1, &orun)] {
                for x in rr.recs.iter().filter(|x| x.attempted) {
                    let (Some(f), Some(t)) = (x.fam, x.ftype) else { continue };
                    let g = match t {
                        FType::Seq | FType::Compose => {
                            let c = w.class_of(f, x.clock).name();
                            if matches!(c, "arith" | "geom" | "quad" | "fib") { "seq-easy" } else if matches!(c, "primes" | "lcg") { "seq-outside" } else { "seq-hard" }
                        }
                        FType::Formula => if w.regime(f, x.clock).g == 10 { "formula-g10" } else { "formula-other-g" },
                        FType::Grid => "grid",
                        FType::Knap => "knap",
                    };
                    let e = groups.entry(g.to_string()).or_insert([0i64; 4]);
                    e[2 * k] += x.cost;
                    e[2 * k + 1] += x.true_reward;
                    if k == 0 && g.starts_with("seq") {
                        let key = format!("{g} / {} / {}", x.strategy.map_or("oracle", |s| s.name()), if x.rule_used { "rule" } else { "no rule" });
                        let e = groups.entry(key).or_insert([0i64; 4]);
                        e[0] += x.cost;
                        e[1] += x.true_reward;
                    }
                }
                let tot_c: i64 = rr.recs.iter().map(|x| x.cost).sum();
                groups.entry("total".into()).or_insert([0i64; 4])[2 * k] += tot_c;
            }
            // seq families whose class needs a rule (not solved by careful) and how many attempts the learner made there
            for f in 0..fams {
                if w.fams[f].ftype != FType::Seq {
                    continue;
                }
                let cls = w.regime(f, 0).class.name().to_string();
                let att = run.recs.iter().filter(|x| x.attempted && x.fam == Some(f)).count() as u64;
                by_class.entry(cls).or_default().1 += att;
            }
            let d = spy.d;
            tot.offers_with_rule += d.offers_with_rule;
            tot.offers_rule_applies += d.offers_rule_applies;
            tot.rule_right += d.rule_right;
            tot.careful_right += d.careful_right;
            tot.deep_right += d.deep_right;
            tot.slot_chosen += d.slot_chosen;
            tot.disc_chosen += d.disc_chosen;
            tot.disc_rank_sum += d.disc_rank_sum;
            tot.disc_p_sum += d.disc_p_sum;
            tot.disc_n_sum += d.disc_n_sum;
            tot.disc_n0 += d.disc_n0;
            tot.best_other_p_sum += d.best_other_p_sum;
            tot.score_ratio_sum += d.score_ratio_sum;
        }
        let n = tot.offers_with_rule.max(1) as f64;
        println!("\n## {name} (seeds {:?}..)", seeds[0]);
        println!("rules accepted {rules}: {useful} in classes careful cannot solve, {redundant} in classes careful solves (arith/geom/quad/fib); wrong at acceptance {wrong}");
        println!("by class (rules accepted, learner attempts on seq families of that class at start): {:?}", { let mut v: Vec<_> = by_class.iter().collect(); v.sort(); v });
        for (g, v) in &groups {
            println!("  group {g}: learner energy {} rate {:.3} | oracle-knows-rules energy {} rate {:.3}", v[0], v[1] as f64 / v[0].max(1) as f64, v[2], v[3] as f64 / v[2].max(1) as f64);
        }
        println!("attempts with discovered {disc_att}, answered by a rule {rule_att}");
        println!("offers in a cell with a live rule: {}; rule applies {} ({:.3}); rule right {} ; careful right {} ; deep right {}", tot.offers_with_rule, tot.offers_rule_applies, tot.offers_rule_applies as f64 / n, tot.rule_right, tot.careful_right, tot.deep_right);
        println!("  slot chosen {} ; with discovered {} ; discovered's mean rank in its slot {:.2} (0 best of 4); mean p(discovered) {:.3} (n_cell mean {:.2}, never tried {:.3}); mean best other p {:.3}; discovered score / best menu score {:.3}",
            tot.slot_chosen, tot.disc_chosen, tot.disc_rank_sum as f64 / n, tot.disc_p_sum / n, tot.disc_n_sum as f64 / n, tot.disc_n0 as f64 / n, tot.best_other_p_sum / n, tot.score_ratio_sum / n);
    }
}
