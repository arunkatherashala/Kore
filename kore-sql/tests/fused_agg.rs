//! The fused filter+aggregate path (fusedagg.rs) must give the same answer, in the same group order, as the
//! ordinary path on tables big enough to use it, including the hash-partitioned many-groups mode.
//! Own test binary because the switch is process-wide.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::{testing, KqlContext};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

fn table(n: usize) -> KqlContext {
    let mut r = Rng(7);
    let modes = ["AIR", "MAIL", "SHIP", "RAIL", "TRUCK"];
    let flags = ["A", "N", "R"];
    let key: Vec<Option<i64>> = (0..n).map(|_| if r.below(50) == 0 { None } else { Some(r.below(n / 4) as i64) }).collect();
    let kd: Vec<Option<i64>> = (0..n).map(|_| Some(1000 + r.below(n / 4) as i64)).collect();
    let k2: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(3) as i64)).collect();
    let qty: Vec<Option<f64>> = (0..n).map(|_| if r.below(20) == 0 { None } else { Some((r.below(50) + 1) as f64) }).collect();
    let price: Vec<Option<f64>> = (0..n).map(|_| Some(r.below(100_000) as f64 / 100.0)).collect();
    let disc: Vec<Option<f64>> = (0..n).map(|_| Some(r.below(11) as f64 / 100.0)).collect();
    let ival: Vec<Option<i64>> = (0..n).map(|_| if r.below(30) == 0 { None } else { Some(r.below(1000) as i64 - 500) }).collect();
    let date: Vec<Option<String>> = (0..n).map(|_| Some(format!("199{}-{:02}-{:02}", 2 + r.below(7), 1 + r.below(12), 1 + r.below(28)))).collect();
    let mode: Vec<Option<String>> = (0..n).map(|_| Some(modes[r.below(5)].to_string())).collect();
    let flag_codes: Vec<u8> = (0..n).map(|_| r.below(3) as u8).collect();
    let skey: Vec<Option<String>> = (0..n).map(|_| Some(format!("k{}", r.below(n / 8)))).collect();
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::int64("k", key), Column::int64("kd", kd), Column::int64("k2", k2), Column::float64("qty", qty), Column::float64("price", price),
        Column::float64("disc", disc), Column::int64("ival", ival), Column::str_col("d", date), Column::str_col("mode", mode),
        Column::str_dict("flag", flag_codes, flags.iter().map(|s| s.to_string()).collect()), Column::str_col("sk", skey),
    ]).unwrap());
    c
}

fn cell(c: &Column, r: usize) -> String {
    match &c.data {
        ColumnData::Int64(v) => v[r].map_or("NULL".into(), |x| x.to_string()),
        ColumnData::Float64(v) => v[r].map_or("NULL".into(), |x| format!("{:.4e}", x)),
        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
        ColumnData::Bool(v) => format!("{:?}", v[r]),
    }
}

fn rows(c: &KqlContext, sql: &str) -> Vec<String> {
    let b = c.query(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    (0..b.num_rows).map(|r| b.columns.iter().map(|c| cell(c, r)).collect::<Vec<_>>().join("|")).collect()
}

#[test]
fn fused_matches_ordinary_path() {
    let ctx = table(300_000);
    let queries = [
        "select flag, mode, sum(qty) as sq, sum(price * (1 - disc)) as sd, sum(price * (1 - disc) * (1 + disc)) as sc, avg(qty) as aq, avg(disc) as ad, count(*) as n from t where d <= '1997-09-02' group by flag, mode",
        "select flag, sum(ival) as s, min(ival) as mn, max(ival) as mx, count(ival) as c from t where d >= '1994-01-01' and d < '1995-01-01' and disc between 0.05 and 0.07 and qty < 24 group by flag",
        "select sum(price * disc) as revenue from t where d >= '1994-01-01' and d < '1995-01-01' and disc between 0.05 and 0.07 and qty < 24",
        "select count(*) as n, sum(qty) as s, avg(price) as a, min(price) as lo, max(price) as hi from t",
        "select sum(price) as s from t where qty > 1000",
        "select k, sum(qty) as sq, count(*) as n from t group by k",
        "select k, sum(price * (1 - disc)) as rev, count(qty) as c, min(ival) as mn, max(qty) as mx, avg(disc) as a from t where d < '1996-01-01' and mode in ('AIR', 'MAIL') group by k",
        "select kd, sum(qty) as sq, count(*) as n, avg(price) as ap, min(qty) as mn from t group by kd",
        "select kd, sum(price * (1 - disc)) as rev, count(qty) as c from t where d < '1995-01-01' and mode = 'AIR' group by kd",
        "select kd, sum(qty) as sq from t group by kd having sum(qty) > 150",
        "select k, k2, sum(qty) as sq from t group by k, k2",
        "select sk, count(*) as n, sum(case when mode = 'AIR' then 1 else 0 end) as air from t group by sk",
        "select k, sum(qty) as sq from t group by k having sum(qty) > 150",
        "select k2, sum(qty) as sq, count(*) as n from t where not (mode = 'AIR') group by k2 order by k2",
        "select t.flag, sum(t.qty) from t where t.d <= '1996-06-01' group by t.flag",
    ];
    for q in queries {
        testing::set_no_fused(true);
        let slow = rows(&ctx, q);
        testing::set_no_fused(false);
        let fast = rows(&ctx, q);
        assert_eq!(slow.len(), fast.len(), "row count differs for {q}");
        for (i, (a, b)) in slow.iter().zip(&fast).enumerate() {
            assert_eq!(a, b, "row {i} differs for {q}");
        }
    }
}
