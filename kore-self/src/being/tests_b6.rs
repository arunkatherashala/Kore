//! Tests of Track B6: transfer.
use super::controller::Limits;
use super::measure_b5::run_b5;
use super::policy::Policy;

fn digest(seed: u64, drift: bool, comp: bool) -> (i64, i64, u64, usize, String) {
    let lim = if comp { Some(Limits { compose: true, prune: true, ..Default::default() }) } else { None };
    let o = run_b5(seed, 1200, drift, lim, 10, &Policy::new(100_000));
    let counts: Vec<String> = o.state.model.counts().iter().map(|c| format!("{}{}:{}:{}", c.0.idx(), c.1.idx(), c.2, c.3)).collect();
    (o.reward, o.cost, o.steps, o.state.rules.len(), counts.join(","))
}

#[test]
fn print_golden() {
    for s in [1u64, 2, 3] {
        for (d, c) in [(false, false), (true, true)] {
            println!("GOLDEN {s} {d} {c} {:?}", digest(s, d, c));
        }
    }
}
