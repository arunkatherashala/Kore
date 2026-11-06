//! Tests of the rule scan fix (`discover_scan`): the incremental scan gives exactly the answers of the
//! full rescan (the code before the fix, kept as `reference_scan`), example by example and run by run.

use super::controller::{Controller, Limits, PruneCfg};
use super::discover::{self, predict_x, search_transfer, Example, Form, Xf, K_TRAIN, K_VAL};
use super::discover_scan::{self, ScanCache};
use super::measure_b5::run_b5w;
use super::measure_b6::{run_b6, Arm, B6Cfg};
use super::world::{Domain, Kind, World};
use super::Policy;

fn lin(mask: u8, constant: bool) -> Form {
    Form::Lin { mask, constant }
}

/// Examples as a being sees them in the extended2 world: every kind, interleaved, including the negative
/// controls (applicable-and-wrong candidates) and kinds that share a signature.
fn mixed_examples(seed: u64, n: u64) -> Vec<Example> {
    let w = World::with_ext2(seed, None);
    let mut kinds: Vec<Kind> = vec![];
    for drifted in [false, true] {
        for d in 1..=5u8 {
            kinds.extend(Kind::options_ext2w(Domain::Pattern, d, drifted, true));
        }
    }
    let mut v = vec![];
    for i in 0..n {
        let k = kinds[(i as usize * 7 + seed as usize) % kinds.len()];
        let t = w.task_of_kind(i, k);
        v.push(Example { event_id: format!("ex-{seed}-{i}"), sig: t.view.difficulty, terms: t.view.params.clone(), next: t.expected() });
    }
    v
}

/// Hand-made examples with long prefixes, so that many forms apply and agree (arithmetic, quadratic, geometric,
/// Fibonacci-like, and a few that nothing fits), mixed over five signatures by a fixed pseudo-random sequence.
fn crafted_examples(seed: u64, n: usize) -> Vec<Example> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut rnd = |m: u64| {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) % m
    };
    let mut v = vec![];
    for i in 0..n {
        let sig = 1 + rnd(5) as u8;
        let len = 7 + rnd(8) as usize;
        let (a, b, c) = (rnd(7) as i64 - 3, rnd(9) as i64 + 1, rnd(5) as i64);
        let mut t: Vec<i64> = vec![];
        let kind = rnd(5);
        for k in 0..=len as i64 {
            let next = match kind {
                0 => a + b * k,
                1 => c * k * k + b * k + a,
                2 => if k == 0 { b } else { 2 * t[k as usize - 1] + a },
                3 => if k < 2 { b + k } else { t[k as usize - 1] + t[k as usize - 2] },
                _ => (k * k * k + 7 * k + a) % 11,
            };
            t.push(next);
        }
        let next = t.pop().unwrap();
        v.push(Example { event_id: format!("cr-{seed}-{i}"), sig, terms: t, next });
    }
    v
}

fn same(a: &discover::SearchOut, b: &discover::SearchOut) -> bool {
    a.found == b.found && a.candidates == b.candidates && a.wanted == b.wanted && a.truncated == b.truncated
}

#[test]
fn incremental_search_equals_the_full_rescan_after_every_new_example() {
    let blocks_sets: [&[Form]; 3] = [&[], &[Form::Poly(2), lin(1, false)], &[Form::Poly(2), Form::Poly(3), lin(7, false), lin(3, true), Form::Poly(1)]];
    for seed in 1..=3u64 {
        let ex = if seed == 3 { crafted_examples(seed, 120) } else { mixed_examples(seed, 120) };
        let mut cache = ScanCache::new();
        for n in 0..=ex.len() {
            for (bi, blocks) in blocks_sets.iter().enumerate() {
                for cap in [usize::MAX, 40] {
                    let prefer: Vec<(u8, Xf, Form)> = if n % 2 == 0 { vec![] } else { vec![(3, Xf::ID, Form::Poly(2)), (2, Xf::parse("d").unwrap(), Form::Poly(1)), (1, Xf::ID, Form::Poly(1))] };
                    let inc = cache.search(&ex[..n], K_TRAIN, K_VAL, blocks, cap, &prefer);
                    if n > 40 && n % 3 != 0 {
                        continue; // the cache still takes in every example; the full rescan is compared on every third
                    }
                    let full = search_transfer(&ex[..n], blocks, cap, &prefer);
                    assert!(same(&full, &inc), "seed {seed}, {n} examples, blocks {bi}, cap {cap}: {:?} vs {:?}", full.found, inc.found);
                }
            }
        }
    }
}

#[test]
fn incremental_search_starts_over_when_the_examples_are_not_an_extension() {
    let a = mixed_examples(4, 90);
    let b = mixed_examples(5, 90);
    let mut cache = ScanCache::new();
    let blocks = [Form::Poly(2), Form::Poly(3)];
    for ex in [&a[..60], &b[..30], &b[..], &a[..10], &a[..]] {
        assert!(same(&search_transfer(ex, &blocks, usize::MAX, &[]), &cache.search(ex, K_TRAIN, K_VAL, &blocks, usize::MAX, &[])));
    }
    // a shorter list that is a prefix of the scanned one is not an extension either: start over
    assert!(same(&search_transfer(&a[..20], &blocks, usize::MAX, &[]), &cache.search(&a[..20], K_TRAIN, K_VAL, &blocks, usize::MAX, &[])));
}

#[test]
fn incremental_search_does_not_predict_an_example_twice() {
    let ex = mixed_examples(6, 300);
    let blocks = [Form::Poly(2), lin(1, false)];
    let mut cache = ScanCache::new();
    let _ = cache.search(&ex[..200], K_TRAIN, K_VAL, &blocks, usize::MAX, &[]);
    let p1 = cache.predictions;
    let _ = cache.search(&ex[..200], K_TRAIN, K_VAL, &blocks, usize::MAX, &[]);
    assert_eq!(cache.predictions, p1, "no new example, no new prediction");
    let _ = cache.search(&ex[..201], K_TRAIN, K_VAL, &blocks, usize::MAX, &[]);
    let cands = discover::candidates_for(&blocks).len() as u64;
    assert!(cache.predictions - p1 <= cands, "one new example: at most one prediction per candidate ({} made)", cache.predictions - p1);
    // a full rescan would make (candidates x examples) predictions per pass; the cache made at most that once
    assert!(p1 <= cands * 200);
}

#[test]
fn merge_check_answers_equal_the_full_rescan() {
    for seed in 1..=3u64 {
        let ex = if seed == 3 { crafted_examples(seed, 150) } else { mixed_examples(seed, 150) };
        let keys: Vec<(Xf, Form)> = vec![(Xf::ID, Form::Poly(1)), (Xf::ID, Form::Poly(2)), (Xf::ID, Form::Poly(3)), (Xf::ID, lin(1, false)), (Xf::ID, lin(1, true)), (Xf::ID, lin(3, false)), (Xf::parse("d").unwrap(), Form::Poly(1)), (Xf::parse("dd").unwrap(), Form::Poly(1)), (Xf::parse("p").unwrap(), Form::Poly(2))];
        let mut cache = ScanCache::new();
        for n in (0..=ex.len()).step_by(3) {
            let pre = &ex[..n];
            for sig in 1..=5u8 {
                let exs: Vec<&Example> = pre.iter().filter(|e| e.sig == sig).collect();
                for a in &keys {
                    let applies: Vec<String> = exs.iter().filter(|e| predict_x(a.0, &a.1, &e.terms).is_some()).map(|e| e.event_id.clone()).collect();
                    let (na, cites) = cache.applies(pre, sig, a.0, &a.1);
                    assert_eq!(na, applies.len());
                    assert_eq!(cites, applies.iter().take(discover::MAX_CITE).cloned().collect::<Vec<_>>());
                    for b in &keys {
                        let full = exs.iter().all(|e| match predict_x(a.0, &a.1, &e.terms) {
                            Some(v) => predict_x(b.0, &b.1, &e.terms) == Some(v),
                            None => true,
                        });
                        assert_eq!(cache.agrees(pre, sig, *a, *b), full, "seed {seed} n {n} sig {sig} {a:?} {b:?}");
                    }
                }
            }
        }
    }
}

fn limits_from_controller(compose: bool, prune: bool) -> Limits {
    let text = "max_live_rules = 16\nmax_building_blocks = 8\nmax_search_candidates = 2000\nmax_steps_per_run = 5000\nmax_energy_per_run = 100000\ncomposition = on\npruning = on\n";
    Controller::parse(text).expect("controller").limits(compose, prune).0
}

/// Same seeds and arms as the B5 measurement: the whole output (accepted rules with their support, retirements,
/// live counts, rule uses, reward trace, refusals, final state) is identical with the incremental scan and the full rescan.
#[test]
fn b5_runs_are_identical_with_the_full_rescan() {
    let p = Policy::new(1_000_000);
    let ctl = Some(limits_from_controller(true, true));
    let tight = Some(Limits { compose: true, prune: true, prune_cfg: PruneCfg { window: 10, min_n: 2, threshold: 0.9, merge_min: 2 }, ..Limits::default() });
    let capped = Some(Limits { compose: true, prune: true, prune_cfg: PruneCfg { window: 10, min_n: 2, threshold: 0.9, merge_min: 2 }, max_live_rules: 4, max_candidates: 150, ..Limits::default() });
    // (seed, limits, rule drift, wide drift); seeds 4 and 6 retire a rule after the drift with the tight setting
    let cases: Vec<(u64, Option<Limits>, bool, bool)> = vec![
        (1, None, false, false),
        (1, Some(Limits::default()), false, false),
        (1, ctl.clone(), false, false),
        (4, ctl.clone(), true, true),
        (4, tight.clone(), true, false),
        (6, tight.clone(), true, false),
        (6, capped.clone(), true, true),
        (7, capped.clone(), false, false),
    ];
    let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
    for (seed, l, drift, wide) in cases {
        let new = run_b5w(seed, 1500, drift, wide, l.clone(), 10, &p);
        let old = discover_scan::with_reference(true, || run_b5w(seed, 1500, drift, wide, l.clone(), 10, &p));
        assert_eq!(format!("{new:?}"), format!("{old:?}"), "seed {seed} limits {l:?} drift {drift} wide {wide}");
        for r in &new.retired {
            *seen.entry(format!("retired:{}", r.2)).or_default() += 1;
        }
        for r in &new.refusals {
            *seen.entry(format!("refusal:{}", r.cap)).or_default() += 1;
        }
        *seen.entry("composed rules".into()).or_default() += new.accepted.iter().filter(|a| !a.1.xf.is_id()).count();
    }
    // the runs above did exercise retirements, both caps and composition (merges: the next test)
    for k in ["retired:low_success", "refusal:max_search_candidates", "refusal:max_live_rules", "composed rules"] {
        assert!(seen.get(k).copied().unwrap_or(0) > 0, "{k} never happened: {seen:?}");
    }
}

#[test]
fn b6_runs_are_identical_with_the_full_rescan() {
    let p = Policy::new(1_000_000);
    for seed in [1u64, 3] {
        for arm in [Arm::NoTransfer, Arm::Transfer, Arm::LinkOnly] {
            let new = run_b6(seed, 2000, arm, B6Cfg::default(), &p);
            let old = discover_scan::with_reference(true, || run_b6(seed, 2000, arm, B6Cfg::default(), &p));
            assert_eq!(format!("{new:?}"), format!("{old:?}"), "seed {seed} arm {arm:?}");
        }
    }
}

/// Everything of a World v2 run except the order of a hash map inside the state (its Debug order is not fixed).
fn w2_key(r: &super::world2::measure::W2Run) -> String {
    let st = &r.state;
    format!(
        "{} {} {} {} {} {:?} {:?} {} | {} {} {} {} {} {} {:?} {:?} {:?} {:?}",
        r.true_reward, r.reported_reward, r.cost, r.steps, r.skips, r.recs, r.rules, r.refusals, st.step, st.clock, st.budget_left, st.total_reward, st.total_cost, st.skips, st.model, st.arms, st.examples, st.rules
    )
}

/// World v2 measurement arms that run discovery (`learner`, `learner-default-caps`, `learner-compose`).
#[test]
fn world_v2_runs_are_identical_with_the_full_rescan() {
    use super::world2::learner::Learner2;
    use super::world2::measure::run_w2;
    use super::world2::world::W2Cfg;
    for seed in [1u64, 2] {
        for (noise, drift) in [(0u32, 0u32), (100, 20)] {
            let cfg = W2Cfg { noise_pm: noise, drift_per_10k: drift, curve_pct: if drift > 0 { 100 } else { 0 }, ..W2Cfg::new(seed, 12, 1500) };
            for compose in [None, Some(false), Some(true)] {
                let lim = compose.map(|c| {
                    let mut ctl = Controller::defaults();
                    ctl.composition = c;
                    ctl.limits(c, false).0
                });
                let new = run_w2(&cfg, &mut Learner2::default(), 10, lim.clone());
                let old = discover_scan::with_reference(true, || run_w2(&cfg, &mut Learner2::default(), 10, lim.clone()));
                assert_eq!(w2_key(&new), w2_key(&old), "seed {seed} noise {noise} drift {drift} compose {compose:?}");
            }
        }
    }
}

/// Through the run loop and the log (as `being_run` drives it, in calls of a few hundred steps): every being
/// event and every revision is the same with the incremental scan and with the full rescan.
#[test]
fn logged_runs_are_identical_with_the_full_rescan() {
    use super::run_loop::{run, RunCfg};
    use super::state::Chooser;
    use super::ScriptedThinker;
    use crate::versioned::{EventRow, RevisionRow, VersionedLog};
    let records = |reference: bool, world: u64, drift: bool, lim: &Limits| -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let mut log = VersionedLog::open_or_create_with_clock(dir.path(), "being", || "2030-01-01T00:00:00Z".to_string()).unwrap();
        for _ in 0..4 {
            let cfg = RunCfg { world_seed: Some(world), loop_seed: Some(1), budget: Some(1500), chooser: Some(Chooser::Model), steps: 350, ext2: Some(true), rule_drift: drift.then_some(true), discover_every: Some(10), ctl: Some(lim.clone()), refill_when_exhausted: true, ..Default::default() };
            let mut p = Policy::new(350);
            p.refill_when_exhausted = true;
            discover_scan::with_reference(reference, || run(&mut log, &cfg, &ScriptedThinker, &p)).unwrap();
        }
        let snap = log.open_latest();
        let mut v: Vec<String> = snap.rows::<EventRow>().iter().map(|e| format!("{}|{}|{}|{}|{}|{}", e.event_id, e.source, e.kind, e.occurred_at, e.text, e.tags)).collect();
        v.extend(snap.rows::<RevisionRow>().iter().map(|r| format!("{}|{}|{}|{}|{}|{:?}|{}", r.key, r.stance, r.confidence.to_bits(), r.rationale, r.author, r.supersedes, r.valid_from)));
        v
    };
    let lims = [limits_from_controller(true, true), Limits { compose: true, prune: true, prune_cfg: PruneCfg { window: 6, min_n: 2, threshold: 0.9, merge_min: 2 }, max_live_rules: 5, max_candidates: 120, ..Limits::default() }];
    for (world, drift, li) in [(4u64, false, 0usize), (4, true, 1), (3, true, 0)] {
        {
            {
                let lim = &lims[li];
                let a = records(false, world, drift, lim);
                let b = records(true, world, drift, lim);
                assert_eq!(a.len(), b.len(), "world {world} drift {drift} limits {li}");
                if let Some(i) = a.iter().zip(&b).position(|(x, y)| x != y) {
                    panic!("world {world} drift {drift} limits {li}: record {i} differs:\n{}\n{}", a[i], b[i]);
                }
            }
        }
    }
}

/// The merge check with many live rules that agree on some signatures: after every new example, housekeeping
/// with the incremental scan and with the full rescan retires the same rules with the same records.
#[test]
fn housekeeping_merges_are_identical_with_the_full_rescan() {
    use super::discover::{Rule, XFORMS};
    use super::run_loop::Engine;
    use super::state::{BeingState, Chooser, Genesis};
    use super::ScriptedThinker;
    let th = ScriptedThinker;
    let lim = Limits { prune: true, prune_cfg: PruneCfg { window: 6, min_n: 4, threshold: 0.5, merge_min: 2 }, ..Limits::default() };
    let mut merges = 0;
    for seed in 1..=2u64 {
        let ex = crafted_examples(seed, 150);
        let mut st = BeingState::default();
        st.start(Genesis { world_seed: seed, loop_seed: 0, budget: 100, explore_c: 0.0, chooser: Chooser::Model, extended: true, ext2: true, rule_drift: false, drift: false, adapt: Default::default(), advice: Default::default(), transfer: false, learn_transfer: false }, String::new());
        // a crowded rule book: every form, and a few transforms of small ones, for every difficulty
        for sig in 1..=5u8 {
            for f in discover::forms() {
                st.apply_rule(Rule::new(sig, f, 3, 1, 0));
            }
            for x in 1..XFORMS.len() {
                st.apply_rule(Rule::new_x(sig, Xf(x as u8), Form::Poly(1), 3, 1, 0, 1));
            }
        }
        let mut a = Engine::new(World::with_ext2(seed, None), st.clone(), &th).with_limits(Some(lim.clone()));
        let mut b = Engine::new(World::with_ext2(seed, None), st, &th).with_limits(Some(lim.clone()));
        b.reference_scan = true;
        for e in &ex {
            a.state.examples.push(e.clone());
            b.state.examples.push(e.clone());
            let (x, y) = (a.housekeeping(), b.housekeeping());
            assert_eq!(format!("{x:?}"), format!("{y:?}"), "seed {seed}, {} examples", a.state.examples.len());
            merges += x.len();
            assert_eq!(format!("{:?}", a.state.rules), format!("{:?}", b.state.rules));
        }
    }
    assert!(merges >= 10, "only {merges} merges: the test did not exercise the merge check");
}


/// A new engine on a long history: the first pass scans every example, on several threads; same answer.
#[test]
fn first_pass_on_a_long_history_equals_the_full_rescan() {
    let mut ex = mixed_examples(8, 500);
    ex.extend(crafted_examples(9, 300));
    let blocks = [Form::Poly(2), Form::Poly(3), lin(7, false), lin(1, true)];
    let prefer = vec![(4u8, Xf::ID, lin(3, false)), (2, Xf::parse("pd").unwrap(), Form::Poly(1))];
    for (cap, pre) in [(usize::MAX, &[][..]), (60, &[][..]), (usize::MAX, &prefer[..])] {
        let mut cache = ScanCache::new();
        let inc = cache.search(&ex, K_TRAIN, K_VAL, &blocks, cap, pre);
        assert!(same(&search_transfer(&ex, &blocks, cap, pre), &inc), "cap {cap}");
        // and it goes on incrementally from there
        let more = mixed_examples(10, 40);
        let mut ex2 = ex.clone();
        ex2.extend(more);
        assert!(same(&search_transfer(&ex2, &blocks, cap, pre), &cache.search(&ex2, K_TRAIN, K_VAL, &blocks, cap, pre)));
    }
}

#[test]
fn a_stashed_scan_is_reused_only_for_exactly_the_same_examples() {
    let ex = mixed_examples(11, 120);
    let blocks = [Form::Poly(2)];
    let mut c = ScanCache::new();
    let _ = c.search(&ex, K_TRAIN, K_VAL, &blocks, usize::MAX, &[]);
    assert!(c.predictions > 0);
    // same examples, or more of them: reused (no prediction for the examples already scanned)
    discover_scan::stash(c.clone(), &ex);
    let mut more = ex.clone();
    more.extend(mixed_examples(12, 5));
    let mut a = discover_scan::adopt(&more);
    assert_eq!(a.predictions, c.predictions);
    assert!(same(&a.search(&more, K_TRAIN, K_VAL, &blocks, usize::MAX, &[]), &search_transfer(&more, &blocks, usize::MAX, &[])));
    // the stash is emptied by adopt
    assert_eq!(discover_scan::adopt(&ex).predictions, 0);
    // one example different in its next term, or fewer examples: not reused
    for bad in [{
        let mut b = ex.clone();
        b[17].next += 1;
        b
    }, ex[..100].to_vec()] {
        discover_scan::stash(c.clone(), &ex);
        assert_eq!(discover_scan::adopt(&bad).predictions, 0);
    }
}
