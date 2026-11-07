//! Track B11 tests: the skip test switch of the planner is off by default (the B1 numbers are unchanged
//! bit for bit), its bars, and that the settled skip test removes the over-skipping of the long run.

use super::measure::run_menu;
use super::measure_b11::{decline_run, DeclineCfg};
use super::planner::{MenuChooser, ModelChooser, Offer, SkipRule, DEFAULT_BETA, DEFAULT_SETTLE_K};
use super::world::{Domain, SKIP_COST};

#[test]
fn the_skip_switch_is_off_by_default_and_the_b1_numbers_are_unchanged() {
    assert_eq!(ModelChooser::new(DEFAULT_BETA).skip_rule, SkipRule::Rate);
    assert_eq!(SkipRule::default(), SkipRule::Rate);
    // the B1 report numbers of docs/measured.md (budget 600 and 3000, seeds 1..20), to three decimals
    for (budget, want) in [(600i64, 3.117f64), (3000, 3.377)] {
        let m: f64 = (1..=20u64).map(|s| run_menu(s, budget, &mut ModelChooser::new(DEFAULT_BETA)).per_budget()).sum::<f64>() / 20.0;
        assert!((m - want).abs() < 0.0005, "budget {budget}: {m:.4} vs {want}");
    }
    // the default decision carries no trace of the switch
    let mut st = super::state::BeingState::default();
    st.start(super::measure_b11::genesis(1, 3000), String::new());
    let offers = [Offer { slot: 0, domain: Domain::Arithmetic, difficulty: 2, listed_cost: 6, listed_reward: 10 }];
    let c = ModelChooser::new(DEFAULT_BETA).choose(&st, &offers).unwrap();
    assert!(c.rationale.get("skip_rule").is_none() && c.rationale.get("skip_bar").is_none());
}

#[test]
fn skip_bars() {
    let (sf, rate, e) = (1.0, 4.0, 4.0);
    assert_eq!(SkipRule::Rate.bar(sf, rate, e, SKIP_COST, 7), 4.0);
    assert!((SkipRule::Calibrated.bar(sf, rate, e, SKIP_COST, 7) - 3.0).abs() < 1e-12);
    let s = SkipRule::Settled(100);
    assert_eq!(s.bar(sf, rate, e, SKIP_COST, 0), 4.0);
    assert!((s.bar(sf, rate, e, SKIP_COST, 100) - 3.5).abs() < 1e-12);
    assert!((s.bar(sf, rate, e, SKIP_COST, 1_000_000_000) - 3.0).abs() < 1e-6);
    // an option that costs no more than a skip never has a positive bar under the calibrated test
    assert_eq!(SkipRule::Calibrated.bar(sf, rate, 1.0, SKIP_COST, 7), 0.0);
    for r in [SkipRule::Rate, SkipRule::Calibrated, SkipRule::Settled(DEFAULT_SETTLE_K), SkipRule::Settled(7)] {
        assert_eq!(SkipRule::parse(&r.code()), Some(r));
    }
    assert_eq!(SkipRule::parse("settled"), Some(SkipRule::Settled(DEFAULT_SETTLE_K)));
    assert_eq!(SkipRule::parse("nope"), None);
}

#[test]
fn the_settled_skip_test_skips_less_and_earns_more_in_world_1() {
    let run = |k: SkipRule| decline_run(&DeclineCfg { world_seed: 1, steps: 12_000, window: 6000, budget: 3000, skip_rule: k, lambda: -1.0, rho: 4.22, discover_every: 10 });
    let (a, b) = (run(SkipRule::Rate), run(SkipRule::Settled(DEFAULT_SETTLE_K)));
    let (x, y) = (&a[1], &b[1]);
    assert!(y.attempts > x.attempts, "{} vs {}", y.attempts, x.attempts);
    assert!(y.over_skip < x.over_skip);
    assert!(y.rpe() > x.rpe(), "{} vs {}", y.rpe(), x.rpe());
}
