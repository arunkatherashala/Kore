//! Tests of Track B5: composition, pruning, controller, kill switch.

use super::discover::*;
use super::world::{Kind, World};

fn ex_of(world_seed: u64, kinds: &[Kind], n: u64, offset: u64) -> Vec<Example> {
    let w = World::with_ext2(world_seed, None);
    let mut v = vec![];
    for i in 0..n {
        for k in kinds {
            let t = w.task_of_kind(offset + i, *k);
            v.push(Example { event_id: format!("ex-{}-{i}", *k as u64), sig: t.view.difficulty, terms: t.view.params.clone(), next: t.expected() });
        }
    }
    v
}

const COMPOSABLE: [Kind; 4] = [Kind::Ext2SplitQuad, Kind::Ext2ParityCubic, Kind::Ext2CubeGeom, Kind::Ext2AltCube];
const NEG2: [Kind; 4] = [Kind::NegLcg, Kind::NegMod7, Kind::NegProd, Kind::NegPrimes];

fn lin(mask: u8, constant: bool) -> Form {
    Form::Lin { mask, constant }
}

#[test]
fn transforms_invert_exactly() {
    // second differences of a cubic are linear: dd then poly 1 predicts the next cubic term
    let t: Vec<i64> = (0..12).map(|n: i64| n * n * n - 2 * n + 5).collect();
    let v = predict_x(Xf::parse("dd").unwrap(), &Form::Poly(1), &t);
    assert_eq!(v, Some(12 * 12 * 12 - 24 + 5));
    // parity split of an interleaved pair of quadratics
    let f = |n: i64| if n % 2 == 0 { (n / 2) * (n / 2) + 3 } else { 2 * ((n - 1) / 2) - 1 };
    let t: Vec<i64> = (0..12).map(f).collect();
    assert_eq!(predict_x(Xf::parse("p").unwrap(), &Form::Poly(2), &t), Some(f(12)));
    // running sums of an arithmetic sequence: the sum is quadratic, poly 2 predicts the next sum
    let t: Vec<i64> = (0..8).map(|n| 3 * n + 1).collect();
    assert_eq!(predict_x(Xf::parse("s").unwrap(), &Form::Poly(2), &t), Some(3 * 8 + 1));
    // identity is the plain grammar
    assert_eq!(predict_x(Xf::ID, &Form::Poly(1), &[1, 3, 5, 7, 9, 11]), predict(&Form::Poly(1), &[1, 3, 5, 7, 9, 11]));
}

#[test]
fn composable_families_are_outside_the_plain_grammar() {
    for k in COMPOSABLE {
        let ex = ex_of(3, &[k], 60, 1000);
        let f = search(&ex);
        assert!(f.is_empty(), "{k:?}: the plain grammar accepted {:?}", f.iter().map(|f| f.form.code()).collect::<Vec<_>>());
    }
}

#[test]
fn composition_finds_them_from_verified_blocks_and_not_without() {
    let blocks = [Form::Poly(2), Form::Poly(3), lin(7, false)];
    for k in COMPOSABLE {
        let mut found_w = 0;
        for seed in 1..=5u64 {
            let ex = ex_of(seed, &[k], 40, 2000);
            let w = World::with_ext2(seed, None);
            let none = search_composed(&ex, &[], usize::MAX);
            assert!(none.found.iter().all(|f| f.xf.is_id()), "no blocks, no composed rule");
            assert!(none.found.is_empty(), "{k:?} seed {seed}: plain grammar accepted {:?}", none.found.iter().map(|f| f.form.code()).collect::<Vec<_>>());
            let out = search_composed(&ex, &blocks, usize::MAX);
            let book: Vec<Rule> = out.found.iter().map(|f| Rule::new_x(f.sig, f.xf, f.form, f.train_ok, f.val_ok, 0, 3)).collect();
            let ok = (0..100u64).filter(|i| {
                let t = w.task_of_kind(900_000 + i, k);
                apply(&book, t.view.difficulty, &t.view.params).map_or(false, |(v, _)| t.check(v))
            }).count();
            found_w += (ok >= 90) as u32;
            println!("{k:?} seed {seed}: rules {:?} fresh exact {ok}/100", out.found.iter().map(|f| format!("{}+{}", f.xf.code(), f.form.code())).collect::<Vec<_>>());
        }
        assert!(found_w >= 4, "{k:?}: found in only {found_w} of 5 worlds");
    }
}

#[test]
fn negative_controls_never_accepted_even_with_composition() {
    let blocks = [Form::Poly(1), Form::Poly(2), Form::Poly(3), lin(1, false), lin(1, true), lin(2, true), lin(3, false), lin(7, false), lin(7, true)];
    for k in NEG2 {
        for seed in 1..=10u64 {
            let ex = ex_of(seed, &[k], 200, 3000);
            let out = search_composed(&ex, &blocks, usize::MAX);
            assert!(out.found.is_empty(), "{k:?} seed {seed}: accepted {:?}", out.found.iter().map(|f| format!("{}+{}", f.xf.code(), f.form.code())).collect::<Vec<_>>());
        }
    }
}

#[test]
fn capped_search_is_a_prefix_of_the_full_search() {
    let blocks = [Form::Poly(2), Form::Poly(3), lin(7, false)];
    let mut ex = ex_of(2, &COMPOSABLE, 40, 4000);
    ex.extend(ex_of(2, &[Kind::PatQuad, Kind::ExtTribo], 30, 4000));
    let full = search_composed(&ex, &blocks, usize::MAX);
    assert!(!full.truncated && full.candidates == full.wanted);
    for cap in [0usize, 10, 40, 77, 130, 200] {
        let c = search_composed(&ex, &blocks, cap);
        assert!(c.candidates <= cap, "cap {cap} exceeded: {}", c.candidates);
        assert_eq!(c.truncated, cap < full.wanted);
        assert!(c.found.len() <= full.found.len());
        for (a, b) in c.found.iter().zip(&full.found) {
            assert_eq!((a.sig, a.xf, a.form), (b.sig, b.xf, b.form));
        }
    }
}
