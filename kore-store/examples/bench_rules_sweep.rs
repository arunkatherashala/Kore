//! Exception-rate sweep for rule mode (docs/kore_rule_mode.md): `line_total = round(qty * price, 2)` and
//! `country = lookup(city)` with 0 to 50 % of the rows replaced by unrelated values, plus float edge cases.
//! Per rate: the version-4 payload of the column (default and level 19), the size of the best candidate
//! rule for it (whether it wins or not), and what the rule-mode writer (level 19) stored.
//!
//!   bench_rules_sweep --out RESULT.json [--rows 200000]

#[path = "support/rules_gen.rs"]
mod gen;

use gen::Rng;
use kore_core::{Column, DataBlock};
use kore_store::{rules, CompactOptions, KoreReader};
use serde_json::{json, Value};

const CITIES: [(&str, &str); 8] = [("Paris", "FR"), ("Lyon", "FR"), ("Berlin", "DE"), ("Rome", "IT"), ("Oslo", "NO"), ("Lima", "PE"), ("Pune", "IN"), ("Osaka", "JP")];

fn round2(x: f64) -> f64 { (x * 100.0).round() / 100.0 }

/// `kind`: "plain" catalog prices, "ties" prices whose products sit on 2-decimal rounding ties in decimal,
/// "huge" prices near 1e13 (k = 100 * value close to 2^53).
fn block(n: usize, rate_permille: u64, kind: &str, seed: u64) -> DataBlock {
    let mut r = Rng::new(seed);
    let (mut city, mut country, mut qty, mut price, mut total) = (vec![], vec![], vec![], vec![], vec![]);
    for _ in 0..n {
        let (c, k) = CITIES[r.below(8) as usize];
        city.push(Some(c.to_string()));
        let exc = |r: &mut Rng| r.below(1000) < rate_permille;
        country.push(Some(if exc(&mut r) { format!("Z{:02}", r.below(100)) } else { k.to_string() }));
        let q = 1 + r.below(9) as i64;
        let p = match kind {
            "ties" => (r.below(2000) as f64 * 2.0 + 1.0) / 200.0 * 1.0, // odd multiples of 0.005 -> qty*p often x.xx5
            "huge" => 1e13 + r.below(1_000_000) as f64 / 100.0,
            _ => (99 + r.below(20_000)) as f64 / 100.0,
        };
        qty.push(Some(q));
        price.push(Some(p));
        total.push(Some(if exc(&mut r) { round2(r.unit() * 2000.0) } else { round2(q as f64 * p) }));
    }
    DataBlock::new(vec![Column::str_col("city", city), Column::str_col("country", country), Column::int64("qty", qty),
        Column::float64("price", price), Column::float64("line_total", total)]).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).map(|i| args[i + 1].clone());
    let out = get("--out").expect("--out FILE");
    let n: usize = get("--rows").map(|s| s.parse().unwrap()).unwrap_or(200_000);
    let o19 = CompactOptions::default().with_zstd_level(19);
    let mut rows: Vec<Value> = vec![];
    for kind in ["plain", "ties", "huge"] {
        let rates: &[u64] = if kind == "plain" { &[0, 1, 10, 50, 100, 200, 250, 300, 350, 400, 450, 500] } else { &[0, 10] };
        for &rate in rates {
            let b = block(n, rate, kind, 7 + rate);
            let one = |name: &str, o: &CompactOptions| {
                let c = DataBlock::new(vec![b.column(name).unwrap().clone()]).unwrap();
                rules::write_explained(&c, &o.clone().with_rules().cross_only()).unwrap().1[0].v4_bytes
            };
            let (bytes, rep) = rules::write_explained(&b, &o19.clone().with_rules()).unwrap();
            let back = KoreReader::from_bytes(&bytes).unwrap();
            for (x, y) in b.columns.iter().zip(&back.columns) { assert_eq!(rules::digest(&x.data), rules::digest(&y.data)); }
            let cands = rules::cross_candidates(&b, &o19.clone().with_rules());
            for col in ["line_total", "country"] {
                let best = cands.iter().filter(|c| c.0 == col).min_by_key(|c| c.2);
                let ch = rep.iter().find(|c| c.name == col).unwrap();
                let stored = ch.rule.as_ref().map_or(ch.v4_bytes, |r| r.1);
                assert!(stored <= ch.v4_bytes);
                let row = json!({"kind": kind, "rate_pct": rate as f64 / 10.0, "column": col,
                    "v4_default": one(col, &CompactOptions::default()), "v4_19": ch.v4_bytes,
                    "best_candidate": best.map(|c| c.1.clone()), "best_candidate_bytes": best.map(|c| c.2),
                    "stored": stored, "stored_rule": ch.rule.as_ref().map(|r| r.0.clone())});
                eprintln!("{row}");
                rows.push(row);
            }
        }
    }
    std::fs::write(out, serde_json::to_string_pretty(&rows).unwrap()).unwrap();
}
