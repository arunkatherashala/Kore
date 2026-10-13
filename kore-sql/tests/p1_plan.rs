//! Join planner (executor.rs comma-join branch): every planner optimisation (sideways-information-passing key sets and
//! derived equalities, statistics-driven join order, lazy base table) must give exactly the answers of the
//! conservative planner, on random comma joins of 2-6 tables with unique / duplicate / NULL keys, empty tables, OR
//! predicates across tables, and tables around the SIP thresholds (100k rows to start, 400k keys cap).
//! Own test binary because the switches are process-wide; one test function at a time (`LOCK`).

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::{testing, KqlContext};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n.max(1) as u64) as usize }
    fn chance(&mut self, pct: usize) -> bool { self.below(100) < pct }
}

/// Table `ti` with columns k{i}a (unique, sparse, optionally one NULL), k{i}b (duplicates, NULLs), k{i}c (tiny domain),
/// v{i} (int), f{i} (float, multiples of 0.25 so sums are exact), s{i} (string).
fn make_table(r: &mut Rng, ti: usize, n: usize, unique_nulls: bool, dup_domain: usize) -> DataBlock {
    let stride = 1 + r.below(4) as i64;
    // a unique permutation of 0..n scaled by `stride` (a cheap shuffle: multiply by an odd number modulo a power of two >= n)
    let m = n.next_power_of_two().max(2);
    let mult = (r.below(m / 2) * 2 + 1) as u64;
    let mut ids: Vec<i64> = Vec::with_capacity(n);
    let mut x = 0u64;
    while ids.len() < n {
        let v = (x.wrapping_mul(mult)) as usize % m;
        if v < n { ids.push(v as i64 * stride); }
        x += 1;
    }
    let a: Vec<Option<i64>> = ids.iter().map(|&v| if unique_nulls && r.chance(1) { None } else { Some(v) }).collect();
    let b: Vec<Option<i64>> = (0..n).map(|_| if r.chance(3) { None } else { Some(r.below(dup_domain) as i64) }).collect();
    let c: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(6) as i64)).collect();
    let v: Vec<Option<i64>> = (0..n).map(|_| if r.chance(5) { None } else { Some(r.below(1000) as i64) }).collect();
    let f: Vec<Option<f64>> = (0..n).map(|_| Some(r.below(400) as f64 * 0.25)).collect();
    let s: Vec<Option<String>> = (0..n).map(|_| if r.chance(5) { None } else { Some(format!("s{}", r.below(8))) }).collect();
    DataBlock::new(vec![
        Column::int64(&format!("k{ti}a"), a), Column::int64(&format!("k{ti}b"), b), Column::int64(&format!("k{ti}c"), c),
        Column::int64(&format!("v{ti}"), v), Column::float64(&format!("f{ti}"), f), Column::str_col(&format!("s{ti}"), s),
    ]).unwrap()
}

fn cell(c: &Column, r: usize) -> String {
    match &c.data {
        ColumnData::Int64(v) => v[r].map_or("NULL".into(), |x| x.to_string()),
        ColumnData::Float64(v) => v[r].map_or("NULL".into(), |x| format!("{x}")),
        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
        ColumnData::Bool(v) => format!("{:?}", v[r]),
    }
}

fn rows(c: &KqlContext, sql: &str) -> Vec<String> {
    let b = c.query(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    let mut v: Vec<String> = (0..b.num_rows).map(|r| b.columns.iter().map(|c| cell(c, r)).collect::<Vec<_>>().join("|")).collect();
    v.sort();
    v
}

#[derive(Clone, Copy)]
enum Mode { Plan, NoPlan, NoSip, NoOrder, NoLazyBase, NoDefer, NoPack, NoSetsFirst }

fn set_mode(m: Mode) {
    testing::set_no_plan(false);
    match m {
        Mode::Plan => {}
        Mode::NoPlan => testing::set_no_plan(true),
        Mode::NoSip => testing::set_no_plan_sip(true),
        Mode::NoOrder => testing::set_no_plan_order(true),
        Mode::NoLazyBase => testing::set_no_plan_lazy_base(true),
        Mode::NoDefer => testing::set_no_plan_defer(true),
        Mode::NoPack => testing::set_no_plan_pack(true),
        Mode::NoSetsFirst => testing::set_no_plan_sets_first(true),
    }
}

struct Gen { tables: Vec<(usize, bool)> /* (rows, large) */ }

impl Gen {
    /// One random comma join over `k` of the tables; returns SQL.
    fn query(&self, r: &mut Rng, allow_dup_edges: impl Fn(usize) -> bool) -> String {
        let nt = self.tables.len();
        let k = 2 + r.below(5.min(nt - 1));
        // random subset / order of k tables
        let mut idx: Vec<usize> = (0..nt).collect();
        for i in (1..nt).rev() { idx.swap(i, r.below(i + 1)); }
        let from: Vec<usize> = idx[..k].to_vec();
        let qualify = r.chance(40);
        let col = |t: usize, name: &str| if qualify { format!("t{t}.{name}{t}") } else { format!("{name}{t}") };
        let colk = |t: usize, kind: char| if qualify { format!("t{t}.k{t}{kind}") } else { format!("k{t}{kind}") };
        let mut conj: Vec<String> = Vec::new();
        // spanning tree: table i links to a random earlier table
        for i in 1..k {
            let (x, y) = (from[i], from[r.below(i)]);
            let dup_ok = allow_dup_edges(x) && allow_dup_edges(y);
            let kinds: &[char] = if dup_ok { &['a', 'a', 'a', 'b'] } else { &['a'] };
            let (kx, ky) = (kinds[r.below(kinds.len())], kinds[r.below(kinds.len())]);
            let (kx, ky) = if dup_ok { (kx, ky) } else { ('a', 'a') };
            conj.push(format!("{} = {}", colk(x, kx), colk(y, ky)));
        }
        // an extra equality (cycle / composite link)
        if k >= 3 && r.chance(40) {
            let (x, y) = (from[r.below(k)], from[r.below(k)]);
            if x != y && allow_dup_edges(x) && allow_dup_edges(y) {
                conj.push(format!("{} = {}", colk(x, 'b'), colk(y, 'b')));
            }
        }
        // local filters
        for &t in &from {
            if r.chance(45) {
                let f = match r.below(5) {
                    0 => format!("{} < {}", col(t, "v"), r.below(1000)),
                    1 => format!("{} >= {}", col(t, "v"), r.below(1000)),
                    2 => format!("{} = 's{}'", col(t, "s"), r.below(8)),
                    3 => format!("{} between {} and {}", col(t, "v"), r.below(300), 300 + r.below(700)),
                    _ => format!("{} in (1, 2, 3, {})", colk(t, 'c'), r.below(6)),
                };
                conj.push(f);
            }
        }
        // an OR across two tables (deferred predicate)
        if k >= 2 && r.chance(25) {
            let (x, y) = (from[0], from[1]);
            conj.push(format!("({} < {} or {} > {})", col(x, "v"), r.below(500), col(y, "v"), 500 + r.below(500)));
        }
        let tables = from.iter().map(|t| if qualify { format!("t{t}") } else { format!("t{t}") }).collect::<Vec<_>>().join(", ");
        let wh = if conj.is_empty() { String::new() } else { format!(" where {}", conj.join(" and ")) };
        let gcol = from[r.below(k)];
        match r.below(4) {
            0 => format!("select count(*) as n, sum({}) as sv, sum({}) as sf from {tables}{wh}", col(from[0], "v"), col(from[k - 1], "f")),
            1 => format!("select {}, count(*) as n, sum({}) as sf from {tables}{wh} group by {}", colk(gcol, 'c'), col(from[k - 1], "f"), colk(gcol, 'c')),
            2 => format!("select {}, {}, {} from {tables}{wh}", colk(from[0], 'a'), col(from[k - 1], "v"), col(from[k / 2], "s")),
            _ => format!("select {}, sum({}) as sv, min({}) as mn from {tables}{wh} group by {}", col(gcol, "s"), col(from[0], "v"), col(from[k - 1], "f"), col(gcol, "s")),
        }
    }
}

fn check(ctx: &KqlContext, sql: &str, modes: &[Mode]) {
    set_mode(Mode::NoPlan);
    let want = rows(ctx, sql);
    for &m in modes {
        set_mode(m);
        let got = rows(ctx, sql);
        set_mode(Mode::Plan);
        assert_eq!(want.len(), got.len(), "row count differs for {sql}");
        for (i, (a, b)) in want.iter().zip(&got).enumerate() {
            assert_eq!(a, b, "row {i} differs for {sql}");
        }
    }
}

const ALL: &[Mode] = &[Mode::Plan, Mode::NoSip, Mode::NoOrder, Mode::NoLazyBase, Mode::NoDefer, Mode::NoPack, Mode::NoSetsFirst];

fn run_scenario(seed: u64, sizes: &[usize], large_from: usize, queries: usize) {
    let mut r = Rng(seed);
    let mut ctx = KqlContext::new();
    let mut tables = Vec::new();
    for (ti, &n) in sizes.iter().enumerate() {
        let large = n >= large_from;
        let dup = if large { (n / 4).max(1) } else { (n / 3).max(2) };
        ctx.register(&format!("t{ti}"), make_table(&mut r, ti, n, ti % 3 == 2, dup));
        tables.push((n, large));
    }
    let g = Gen { tables };
    let large: Vec<bool> = g.tables.iter().map(|t| t.1).collect();
    for _ in 0..queries {
        let sql = g.query(&mut r, |t| !large[t]);
        check(&ctx, &sql, ALL);
    }
    set_mode(Mode::Plan);
}

#[test]
fn small_random_joins_match() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (deferred0, picks0) = (testing::plan_deferred_tables(), testing::plan_estimated_picks());
    for seed in 0..12u64 {
        let mut r = Rng(seed * 977 + 1);
        let n = 3 + r.below(5);
        let sizes: Vec<usize> = (0..n).map(|_| [0usize, 1, 2, 5, 20, 60, 150][r.below(7)]).collect();
        // tables of 0 rows or more: the generator needs at least two
        run_scenario(seed + 100, &sizes, usize::MAX, 14);
    }
    assert!(testing::plan_deferred_tables() > deferred0, "no table was ever gathered late");
    assert!(testing::plan_estimated_picks() > picks0, "no join step was ordered by estimate");
}

#[test]
fn empty_and_null_key_tables() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    run_scenario(7, &[0, 50, 50, 0, 30, 30], usize::MAX, 20);
    run_scenario(8, &[1, 1, 1, 1], usize::MAX, 20);
}

#[test]
fn tables_around_the_sip_threshold() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 100k rows is SIP_MIN_ROWS: just below, at and above it, next to small dimension tables
    run_scenario(21, &[99_999, 100_000, 100_001, 40, 2_000, 300], 90_000, 8);
    run_scenario(22, &[120_000, 8, 5_000, 100_000, 60], 90_000, 8);
}

#[test]
fn key_sets_around_the_400k_cap() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // a 450k-row table whose filtered key set sits around SIP_MAX_KEYS next to a big fact table
    let sets0 = testing::plan_sip_sets();
    run_scenario(31, &[450_000, 300_000, 90, 500_000, 7], 90_000, 8);
    assert!(testing::plan_sip_sets() > sets0, "SIP key sets were never applied");
}

/// Hand-written shapes with the structure of TPC-H Q3/Q5/Q9 (3, 6 and 5 tables, transitive equalities).
#[test]
fn tpch_shaped_chains() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(5);
    let mut ctx = KqlContext::new();
    let (nn, nc, no, nl, np, nps) = (25usize, 3_000usize, 30_000usize, 130_000usize, 4_000usize, 16_000usize);
    let ints = |v: Vec<i64>| v.into_iter().map(Some).collect::<Vec<_>>();
    ctx.register("nation", DataBlock::new(vec![
        Column::int64("n_nationkey", ints((0..nn as i64).collect())), Column::int64("n_regionkey", ints((0..nn as i64).map(|i| i % 5).collect())),
    ]).unwrap());
    ctx.register("region", DataBlock::new(vec![
        Column::int64("r_regionkey", ints((0..5).collect())),
        Column::str_col("r_name", (0..5).map(|i| Some(format!("R{i}"))).collect()),
    ]).unwrap());
    ctx.register("customer", DataBlock::new(vec![
        Column::int64("c_custkey", ints((0..nc as i64).collect())), Column::int64("c_nationkey", (0..nc).map(|_| Some(r.below(nn) as i64)).collect()),
        Column::int64("c_seg", (0..nc).map(|_| Some(r.below(5) as i64)).collect()),
    ]).unwrap());
    ctx.register("supplier", DataBlock::new(vec![
        Column::int64("s_suppkey", ints((0..400i64).collect())), Column::int64("s_nationkey", (0..400).map(|_| Some(r.below(nn) as i64)).collect()),
    ]).unwrap());
    ctx.register("orders", DataBlock::new(vec![
        Column::int64("o_orderkey", ints((0..no as i64).map(|i| i * 3 + 1).collect())),
        Column::int64("o_custkey", (0..no).map(|_| if r.chance(2) { None } else { Some(r.below(nc) as i64) }).collect()),
        Column::int64("o_date", (0..no).map(|_| Some(r.below(2000) as i64)).collect()),
    ]).unwrap());
    ctx.register("lineitem", DataBlock::new(vec![
        Column::int64("l_orderkey", (0..nl).map(|_| if r.chance(1) { None } else { Some((r.below(no + 500) * 3 + 1) as i64) }).collect()),
        Column::int64("l_partkey", (0..nl).map(|_| Some(r.below(np) as i64)).collect()),
        Column::int64("l_suppkey", (0..nl).map(|_| Some(r.below(400) as i64)).collect()),
        Column::float64("l_price", (0..nl).map(|_| Some(r.below(4000) as f64 * 0.25)).collect()),
        Column::int64("l_ship", (0..nl).map(|_| Some(r.below(2000) as i64)).collect()),
    ]).unwrap());
    ctx.register("part", DataBlock::new(vec![
        Column::int64("p_partkey", ints((0..np as i64).collect())), Column::int64("p_kind", (0..np).map(|_| Some(r.below(20) as i64)).collect()),
    ]).unwrap());
    ctx.register("partsupp", DataBlock::new(vec![
        Column::int64("ps_partkey", (0..nps).map(|i| Some((i % np) as i64)).collect()),
        Column::int64("ps_suppkey", (0..nps).map(|i| Some(((i / np + i * 7) % 400) as i64)).collect()),
        Column::float64("ps_cost", (0..nps).map(|_| Some(r.below(400) as f64 * 0.25)).collect()),
    ]).unwrap());
    let queries = [
        "select o_orderkey, sum(l_price) as rev, o_date from customer, orders, lineitem where c_seg = 1 and c_custkey = o_custkey and l_orderkey = o_orderkey and o_date < 1000 and l_ship > 1000 group by o_orderkey, o_date",
        "select n_regionkey, sum(l_price) as rev from customer, orders, lineitem, supplier, nation, region where c_custkey = o_custkey and l_orderkey = o_orderkey and l_suppkey = s_suppkey and c_nationkey = s_nationkey and s_nationkey = n_nationkey and n_regionkey = r_regionkey and r_name = 'R2' and o_date >= 500 and o_date < 900 group by n_regionkey",
        "select n_nationkey, count(*) as n, sum(l_price - ps_cost) as amount from part, supplier, lineitem, partsupp, orders, nation where s_suppkey = l_suppkey and ps_suppkey = l_suppkey and ps_partkey = l_partkey and p_partkey = l_partkey and o_orderkey = l_orderkey and s_nationkey = n_nationkey and p_kind = 3 group by n_nationkey",
        "select count(*) as n, sum(l_price) as s from lineitem, orders, customer where l_orderkey = o_orderkey and o_custkey = c_custkey and c_nationkey = 4",
        "select count(*) as n from lineitem, orders where l_orderkey = o_orderkey and o_date < 3",
    ];
    let (sets0, eqs0) = (testing::plan_sip_sets(), testing::plan_derived_equalities());
    for q in queries {
        assert!(!rows(&ctx, q).is_empty(), "test data should give a non-empty answer for {q}");
        check(&ctx, q, ALL);
    }
    assert!(testing::plan_sip_sets() > sets0, "SIP key sets were never applied");
    assert!(testing::plan_derived_equalities() > eqs0, "no transitive equality was derived");
}

/// Multi-column Int64 keys: negative values, NULL components, duplicates, spans too wide to pack exactly.
#[test]
fn packed_composite_keys() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let packed0 = testing::plan_packed_joins();
    for (seed, wide) in [(1u64, false), (2, false), (3, true), (4, true)] {
        let mut r = Rng(seed);
        let mut ctx = KqlContext::new();
        for (name, n) in [("ta", 400usize), ("tb", 300), ("tc", 150)] {
            let col = |r: &mut Rng, dom: usize, null_pct: usize| -> Vec<Option<i64>> {
                (0..n).map(|_| if r.chance(null_pct) { None } else {
                    let v = r.below(dom) as i64 - (dom as i64) / 2;
                    // wide: values near the ends of the i64 range, so the combined range cannot be packed
                    Some(if wide && v % 3 == 0 { v.wrapping_mul(1 << 60) } else { v })
                }).collect()
            };
            let (x, y, z) = (col(&mut r, 9, 6), col(&mut r, 7, 6), col(&mut r, 5, 3));
            let pay: Vec<Option<f64>> = (0..n).map(|_| Some(r.below(100) as f64 * 0.5)).collect();
            let c = |s: &str| format!("{s}_{name}");
            ctx.register(name, DataBlock::new(vec![
                Column::int64(&c("x"), x), Column::int64(&c("y"), y), Column::int64(&c("z"), z), Column::float64(&c("p"), pay),
            ]).unwrap());
        }
        let qs = [
            "select count(*) as n, sum(p_ta) as s, sum(p_tb) as t from ta, tb where x_ta = x_tb and y_ta = y_tb",
            "select count(*) as n, sum(p_ta) as s from ta, tb where x_ta = x_tb and y_ta = y_tb and z_ta = z_tb",
            "select x_ta, count(*) as n, sum(p_tc) as s from ta, tb, tc where x_ta = x_tb and y_ta = y_tb and x_tb = x_tc and y_tb = y_tc and z_tc = z_ta group by x_ta",
            "select ta.x_ta, tb.y_tb, tc.p_tc from ta, tb, tc where ta.x_ta = tb.x_tb and ta.y_ta = tb.y_tb and tb.z_tb = tc.z_tc and tb.y_tb = tc.y_tc and ta.p_ta < 30",
        ];
        for q in qs { check(&ctx, q, ALL); }
    }
    assert!(testing::plan_packed_joins() > packed0, "no join used the packed key");
}

/// A big fact table whose foreign key is restricted to a few rows of a filtered dimension: the key set is selective, so
/// it runs before the fact table's own predicate (numeric, string and OR shaped predicates).
#[test]
fn selective_key_sets_run_first() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let first0 = testing::plan_sets_first_loads();
    let mut r = Rng(11);
    let mut ctx = KqlContext::new();
    let (nd, nf) = (400usize, 260_000usize);
    ctx.register("dim", DataBlock::new(vec![
        Column::int64("d_k", (0..nd as i64).map(Some).collect()),
        Column::int64("d_grp", (0..nd).map(|_| Some(r.below(40) as i64)).collect()),
    ]).unwrap());
    ctx.register("fact", DataBlock::new(vec![
        Column::int64("fk", (0..nf).map(|_| if r.chance(2) { None } else { Some(r.below(nd + 20) as i64) }).collect()),
        Column::int64("v", (0..nf).map(|_| if r.chance(3) { None } else { Some(r.below(1000) as i64) }).collect()),
        Column::float64("f", (0..nf).map(|_| Some(r.below(400) as f64 * 0.25)).collect()),
        Column::str_col("s", (0..nf).map(|_| if r.chance(3) { None } else { Some(format!("s{}", r.below(8))) }).collect()),
        Column::str_col("d", (0..nf).map(|_| Some(format!("199{}-{:02}-{:02}", r.below(9), 1 + r.below(12), 1 + r.below(28)))).collect()),
    ]).unwrap());
    let qs = [
        "select count(*) as n, sum(f) as t from dim, fact where d_k = fk and d_grp = 3 and v < 500",
        "select count(*) as n, sum(f) as t from dim, fact where d_k = fk and d_grp < 2 and s = 's3'",
        "select d_grp, count(*) as n, sum(f) as t from dim, fact where d_k = fk and d_grp = 7 and (v < 100 or s = 's1') group by d_grp",
        "select count(*) as n from dim, fact where d_k = fk and d_grp = 5 and d >= '1994-01-01' and d < '1996-01-01'",
        "select count(*) as n from dim, fact where d_k = fk and d_grp = 99 and v < 500",
    ];
    for q in qs { check(&ctx, q, ALL); }
    assert!(testing::plan_sets_first_loads() > first0, "no load ran its key sets before the predicate");
}

/// `select *` and explicit column lists must keep their column order and names whatever the planner does with payload
/// columns (late gather appends them after the join keys).
#[test]
fn star_column_order_is_stable() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(3);
    let mut ctx = KqlContext::new();
    ctx.register("t0", make_table(&mut r, 0, 300, false, 50));
    ctx.register("t1", make_table(&mut r, 1, 200, false, 50));
    ctx.register("t2", make_table(&mut r, 2, 120_000, false, 30_000));
    let queries = [
        "select * from t0, t1 where k0a = k1a and v0 < 700",
        "select * from t1, t0 where k0b = k1b",
        "select * from t0, t1, t2 where k0a = k1a and k2a = k0a and v2 < 500",
        "select t1.*, t0.v0 from t0, t1 where k0a = k1a",
        "select s1, k0a, f0, v1 from t0, t1 where k0a = k1a",
    ];
    for q in queries {
        set_mode(Mode::NoPlan);
        let want = ctx.query(q).unwrap();
        let want_names: Vec<String> = want.columns.iter().map(|c| c.name.clone()).collect();
        let mut want_rows = rows(&ctx, q);
        want_rows.sort();
        for m in [Mode::Plan, Mode::NoDefer, Mode::NoLazyBase, Mode::NoSip] {
            set_mode(m);
            let got = ctx.query(q).unwrap();
            let names: Vec<String> = got.columns.iter().map(|c| c.name.clone()).collect();
            assert_eq!(want_names, names, "column order/names differ for {q}");
            let mut got_rows = rows(&ctx, q);
            got_rows.sort();
            assert_eq!(want_rows, got_rows, "rows differ for {q}");
        }
    }
    set_mode(Mode::Plan);
}
