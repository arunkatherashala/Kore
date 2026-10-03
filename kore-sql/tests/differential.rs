//! Differential test: random queries are answered three ways (the specialised executor paths, the general
//! row-based tail only, and with the column-at-a-time predicate/arithmetic evaluator switched off) and the
//! answers must agree. A disagreement means one of the paths computes a wrong result.
//!
//! Lives in its own test binary because the switches are process-wide.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::{testing, KqlContext};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
    fn chance(&mut self, pct: usize) -> bool { self.below(100) < pct }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T { &v[self.below(v.len())] }
}

fn opt<T>(r: &mut Rng, pct_null: usize, v: T) -> Option<T> { if r.chance(pct_null) { None } else { Some(v) } }

fn tables(r: &mut Rng) -> KqlContext {
    let n = 40 + r.below(25);
    let strs = ["ab", "abc", "b", "", "Ab", "ba"];
    let dates = ["2024-01-05", "2024-02-29", "2023-12-31", "2024-03-01"];
    let floats = [-1.5, 0.0, 0.5, 2.0, 2.5, 3.5];
    let mut c = KqlContext::new();
    c.register("t1", DataBlock::new(vec![
        Column::int64("id", (1..=n as i64).map(Some).collect()),
        Column::int64("a", (0..n).map(|_| { let v = r.below(6) as i64; opt(r, 15, v) }).collect()),
        Column::int64("b", (0..n).map(|_| { let v = r.below(12) as i64 - 3; opt(r, 15, v) }).collect()),
        Column::float64("c", (0..n).map(|_| { let v = *r.pick(&floats); opt(r, 15, v) }).collect()),
        Column::str_col("s", (0..n).map(|_| { let v = r.pick(&strs).to_string(); opt(r, 15, v) }).collect()),
        Column::str_col("d", (0..n).map(|_| { let v = r.pick(&dates).to_string(); opt(r, 15, v) }).collect()),
    ]).unwrap());
    let m = 12 + r.below(15);
    c.register("t2", DataBlock::new(vec![
        Column::int64("x", (0..m).map(|_| { let v = r.below(7) as i64; opt(r, 15, v) }).collect()),
        Column::str_col("y", (0..m).map(|_| { let v = r.pick(&strs).to_string(); opt(r, 15, v) }).collect()),
        Column::float64("z", (0..m).map(|_| { let v = *r.pick(&floats); opt(r, 15, v) }).collect()),
    ]).unwrap());
    let k = 8 + r.below(10);
    c.register("t3", DataBlock::new(vec![
        Column::int64("a", (0..k).map(|_| { let v = r.below(7) as i64; opt(r, 15, v) }).collect()),
        Column::str_col("s", (0..k).map(|_| { let v = r.pick(&strs).to_string(); opt(r, 15, v) }).collect()),
        Column::float64("w", (0..k).map(|_| { let v = *r.pick(&floats); opt(r, 15, v) }).collect()),
    ]).unwrap());
    c
}

// ── expression generators ──

fn int_expr(r: &mut Rng, depth: usize) -> String {
    if depth == 0 || r.chance(35) {
        return match r.below(5) { 0 => "a".into(), 1 => "b".into(), 2 => "id".into(), 3 => r.below(9).to_string(), _ => "a".into() };
    }
    let (x, y) = (int_expr(r, depth - 1), int_expr(r, depth - 1));
    match r.below(9) {
        0 => format!("({x} + {y})"),
        1 => format!("({x} - {y})"),
        2 => format!("({x} * {})", r.below(4)),
        3 => format!("abs({x})"),
        4 => format!("coalesce({x}, {})", r.below(5)),
        5 => format!("(case when {} then {x} else {y} end)", bool_expr(r, depth - 1)),
        6 => "length(s)".into(),
        7 => format!("nullif({x}, {})", r.below(4)),
        _ => format!("({x} % 3)"),
    }
}

fn num_expr(r: &mut Rng, depth: usize) -> String {
    match r.below(6) {
        0 => "c".into(),
        1 => format!("(c * {})", r.below(3) + 1),
        2 => format!("({} / {})", int_expr(r, depth), r.below(3) + 1),
        3 => format!("round(c, 0)"),
        _ => int_expr(r, depth),
    }
}

fn str_expr(r: &mut Rng) -> String {
    match r.below(5) {
        0 => "s".into(),
        1 => "upper(s)".into(),
        2 => "substr(s, 1, 2)".into(),
        3 => "concat(s, 'x')".into(),
        _ => "coalesce(s, '?')".into(),
    }
}

fn bool_expr(r: &mut Rng, depth: usize) -> String {
    if depth > 0 && r.chance(30) {
        let (x, y) = (bool_expr(r, depth - 1), bool_expr(r, depth - 1));
        return match r.below(3) { 0 => format!("({x} and {y})"), 1 => format!("({x} or {y})"), _ => format!("(not {x})") };
    }
    let cmp = *r.pick(&["=", "<>", "<", "<=", ">", ">="]);
    match r.below(12) {
        0 => format!("{} {cmp} {}", int_expr(r, 1), r.below(6)),
        1 => format!("{} {cmp} {}", int_expr(r, 1), int_expr(r, 1)),
        2 => format!("{} {cmp} {}.5", num_expr(r, 1), r.below(3)),
        3 => format!("s {} 'ab'", *r.pick(&["=", "<>", "<", ">"])),
        4 => format!("s like '{}'", r.pick(&["a%", "%b", "_b%", "ab", "%"])),
        5 => format!("s not like '{}'", r.pick(&["a%", "%b"])),
        6 => format!("a in ({})", (0..1 + r.below(3)).map(|_| r.below(6).to_string()).collect::<Vec<_>>().join(", ")),
        7 => format!("a not in ({}, null)", r.below(6)),
        8 => format!("b between {} and {}", r.below(4), r.below(8)),
        9 => format!("{} is {}null", int_expr(r, 1), if r.chance(50) { "not " } else { "" }),
        10 => format!("s in ('ab', 'b', null)"),
        _ => format!("c {cmp} {}", *r.pick(&["0", "0.5", "2", "2.5"])),
    }
}

fn agg_expr(r: &mut Rng) -> String {
    match r.below(14) {
        0 => "count(*)".into(),
        1 => "count(a)".into(),
        2 => format!("sum({})", int_expr(r, 1)),
        3 => "avg(b)".into(),
        4 => "min(a)".into(),
        5 => "max(c)".into(),
        6 => "min(s)".into(),
        7 => "max(s)".into(),
        8 => "count(distinct a)".into(),
        9 => "count(distinct s)".into(),
        10 => format!("sum(case when {} then 1 else 0 end)", bool_expr(r, 1)),
        11 => "avg(c)".into(),
        12 => "sum(c)".into(),
        _ => "max(b) - min(b)".into(),
    }
}

fn query(r: &mut Rng) -> String {
    let wh = if r.chance(65) { format!(" where {}", bool_expr(r, 2)) } else { String::new() };
    match r.below(43) {
        0 | 1 => {
            let k = 1 + r.below(3);
            let items: Vec<String> = (0..k).map(|i| match r.below(3) { 0 => format!("{} as e{i}", int_expr(r, 2)), 1 => format!("{} as e{i}", num_expr(r, 1)), _ => format!("{} as e{i}", str_expr(r)) }).collect();
            let lim = if r.chance(30) { format!(" limit {}", 1 + r.below(8)) } else { String::new() };
            format!("select id, {} from t1{wh} order by id{lim}", items.join(", "))
        }
        2 | 3 => {
            let key = *r.pick(&["a", "b", "s", "d"]);
            let n = 1 + r.below(3);
            let aggs: Vec<String> = (0..n).map(|i| format!("{} as m{i}", agg_expr(r))).collect();
            let having = if r.chance(30) { format!(" having count(*) > {}", r.below(4)) } else { String::new() };
            format!("select {key}, {} from t1{wh} group by {key}{having} order by {key}", aggs.join(", "))
        }
        4 => {
            let n = 1 + r.below(3);
            let aggs: Vec<String> = (0..n).map(|i| format!("{} as m{i}", agg_expr(r))).collect();
            format!("select {} from t1{wh}", aggs.join(", "))
        }
        5 => {
            let kind = *r.pick(&["join", "left join"]);
            let extra = if r.chance(30) { format!(" and t2.z > {}", r.pick(&["0", "1", "2"])) } else { String::new() };
            format!("select t1.id, t2.y, t2.z from t1 {kind} t2 on t1.a = t2.x{extra}{wh} order by t1.id, t2.y, t2.z")
        }
        6 => {
            let col = *r.pick(&["a", "b", "s", "d", "c"]);
            let lim = if r.chance(30) { format!(" limit {}", 1 + r.below(4)) } else { String::new() };
            format!("select distinct {col} from t1{wh} order by {col}{lim}")
        }
        7 => {
            let p = *r.pick(&["a in", "a not in", "b > any", "b >= all", "exists"]);
            let inner = if r.chance(50) { " where x is not null".to_string() } else { String::new() };
            match p {
                "exists" => format!("select id from t1 where {}exists (select 1 from t2 where t2.x = t1.a{}){} order by id", if r.chance(50) { "not " } else { "" }, if r.chance(50) { " and t2.z > 0" } else { "" }, wh.replace(" where", " and")),
                "b > any" | "b >= all" => format!("select id from t1 where {p} (select x from t2{inner}){} order by id", wh.replace(" where", " and")),
                _ => format!("select id from t1 where {p} (select x from t2{inner}){} order by id", wh.replace(" where", " and")),
            }
        }
        8 => {
            let op = *r.pick(&["union", "union all", "intersect", "except", "intersect all", "except all"]);
            format!("select a from t1{wh} {op} select x from t2 order by 1")
        }
        9 => {
            let k = *r.pick(&["a", "b", "s"]);
            format!("select {k}, count(*) as n, (select count(*) from t2 where t2.x = t1.a) as m from t1 group by {k}, a order by {k}, n, m")
        }
        10 => {
            let nf = *r.pick(&["nulls first", "nulls last", ""]);
            let dir = *r.pick(&["asc", "desc"]);
            let lim = if r.chance(50) { format!(" limit {} offset {}", 1 + r.below(10), r.below(5)) } else { String::new() };
            format!("select id, a, b, c, s from t1{wh} order by a {dir} {nf}, c {} {nf}, s {dir}, id{lim}", *r.pick(&["asc", "desc"]))
        }
        11 => {
            let (k1, k2) = (*r.pick(&["a", "s"]), *r.pick(&["b", "d", "c"]));
            let n = 1 + r.below(3);
            let aggs: Vec<String> = (0..n).map(|i| format!("{} as m{i}", agg_expr(r))).collect();
            format!("select {k1}, {k2}, {} from t1{wh} group by {k1}, {k2} order by {k1}, {k2}", aggs.join(", "))
        }
        12 => {
            let kind = *r.pick(&["join", "left join"]);
            format!("select t1.s, count(t2.y) as c1, sum(t2.z) as s1, max(t2.y) as m1, count(*) as c2 from t1 {kind} t2 on t1.a = t2.x{wh} group by t1.s order by t1.s")
        }
        13 => {
            let cond = bool_expr(r, 1);
            format!("select case when {cond} then 'hi' else 'lo' end as k, count(*) as n, sum(b) as sb from t1{wh} group by k order by k")
        }
        14 => {
            let sub = *r.pick(&["b > (select avg(z) from t2)", "c > (select min(z) from t2 where t2.x = t1.a)", "a = (select max(x) from t2)", "b < (select count(*) from t2 where t2.x = t1.a) + 3"]);
            format!("select id from t1 where {sub} order by id")
        }
        15 => {
            let w = if r.chance(50) { format!(" where x.b > {} or y.s like 'a%'", r.below(6)) } else { String::new() };
            format!("select count(*) as n, sum(x.b) as sb, max(y.s) as ms from t1 x join t1 y on x.a = y.b{w}")
        }
        16 => {
            format!("select t1.id, t2.y from t1, t2 where t1.a = t2.x and t2.z > {} order by t1.id, t2.y", r.pick(&["0", "1", "2"]))
        }
        17 | 18 => {
            let x = *r.pick(&["a", "b", "c", "s", "id"]);
            let ranking = r.chance(35);
            let f = if ranking {
                r.pick(&["rank()", "dense_rank()", "percent_rank()", "cume_dist()", "ntile(3)", "ntile(1)", "row_number()"]).to_string()
            } else {
                match r.below(11) {
                    0 => format!("lag({x})"), 1 => format!("lag({x}, 2, 99)"), 2 => format!("lead({x})"), 3 => format!("lead({x}, 2)"),
                    4 => format!("first_value({x})"), 5 => format!("last_value({x})"), 6 => format!("nth_value({x}, 2)"),
                    7 => format!("sum({})", *r.pick(&["a", "b", "id"])), 8 => format!("avg({})", *r.pick(&["a", "b", "c"])),
                    9 => format!("count({x})"), _ => format!("{}({x})", r.pick(&["min", "max"])),
                }
            };
            let part = *r.pick(&["", "partition by a", "partition by s", "partition by a, s"]);
            let key = *r.pick(&["a", "b", "c", "s", "a desc", "b desc nulls last", "c nulls last"]);
            // a unique final key keeps position-dependent functions deterministic
            let order = if ranking && f != "row_number()" { format!("order by {key}") } else { format!("order by {key}, id") };
            let numeric_key = matches!(key, "a" | "b" | "c" | "a desc");
            let frame = match r.below(7) {
                0 => String::new(),
                1 => format!(" rows between {} preceding and current row", r.below(3)),
                2 => " rows between unbounded preceding and unbounded following".to_string(),
                3 => format!(" rows between {} preceding and {} following", r.below(3), r.below(3)),
                4 => " range between unbounded preceding and current row".to_string(),
                5 if numeric_key => format!(" range between {} preceding and {} following", r.below(3), r.below(3)),
                _ => format!(" rows between current row and {} following", r.below(3)),
            };
            // frames only apply to value / aggregate functions; ranking and offset functions take the default
            let frame = if ranking || f.starts_with("lag") || f.starts_with("lead") { String::new() } else { frame };
            // a frame needs an ORDER BY made of the key only when RANGE is used with offsets
            let order = if frame.contains("range between") && frame.contains(" preceding and ") && !frame.contains("unbounded") { format!("order by {}", key) } else { order };
            format!("select id, {f} over ({part} {order}{frame}) as w from t1{wh} order by id")
        }
        19 => {
            let k = *r.pick(&["a", "b", "s"]);
            format!("select {k}, sum(b) as sb, count(*) as n, rank() over (order by sum(b) desc, {k}) as rk from t1{wh} group by {k} order by {k}")
        }
        20 => {
            let p = *r.pick(&["a", "s", "b"]);
            format!("select id from (select id, row_number() over (partition by {p} order by c desc, id) as rn from t1{wh}) q where rn <= 2 order by id")
        }
        21 => {
            // GROUP BY an expression / an ordinal
            let e = match r.below(6) { 0 => "a % 2".to_string(), 1 => "coalesce(a, -1)".to_string(), 2 => "length(s)".to_string(), 3 => "upper(s)".to_string(), 4 => "(a + b)".to_string(), _ => "abs(b) % 3".to_string() };
            if r.chance(50) {
                format!("select {e} as k, count(*) as n, sum(b) as sb from t1{wh} group by {e} order by k")
            } else {
                format!("select {e} as k, count(*) as n, max(c) as mc from t1{wh} group by 1 order by 1")
            }
        }
        22 => {
            let k = *r.pick(&["a", "s"]);
            let agg = r.pick(&["sum(b)", "count(*)", "max(c)", "count(distinct b)", "sum(distinct b)", "avg(distinct b)"]).to_string();
            format!("select {k}, {agg} as v from t1{wh} group by {k} having {agg} {} {} order by {agg} desc, {k}", *r.pick(&[">", "<", ">="]), r.below(8))
        }
        23 => {
            let kind = *r.pick(&["join", "left join", "right join", "full join"]);
            format!("select t1.id, t2.x, t2.y from t1 {kind} t2 on t1.a = t2.x and t1.s = t2.y order by t1.id, t2.x, t2.y")
        }
        24 => {
            format!("select q.a, q.n, t2.y from (select a, count(*) as n from t1{wh} group by a) q join t2 on q.a = t2.x order by q.a, t2.y, q.n")
        }
        25 => {
            let sub = *r.pick(&[
                "a in (select x from t2 where z > 0)", "exists (select 1 from t2 where t2.x = t1.a and t2.y = t1.s)",
                "not exists (select 1 from t2 where t2.x = t1.a and t2.y = t1.s)", "a = (select max(x) from t2)",
                "a not in (select x from t2 where x is not null)", "s in (select y from t2)", "s not in (select y from t2)",
                "b > (select min(x) from t2 where t2.y = t1.s)", "(select count(*) from t2 where t2.x = t1.a) > 1",
            ]);
            format!("select id from t1 where {sub}{} order by id", wh.replace(" where", " and"))
        }
        26 => {
            format!("select id, (select max(z) from t2 where t2.x = t1.a) as mz, (select count(*) from t2) as nt from t1{wh} order by id")
        }
        27 => {
            let op = *r.pick(&["union", "union all", "intersect", "except"]);
            format!("select s, count(*) from t1 group by s {op} select y, count(*) from t2 group by y order by 1, 2")
        }
        28 => {
            format!("select id from t1 where a is {}distinct from b order by id", if r.chance(50) { "not " } else { "" })
        }
        29 => {
            let e = int_expr(r, 2);
            format!("select id, {e} as v from t1{wh} order by case when {e} is null then 1 else 0 end, {e} desc, id")
        }
        33 | 34 => {
            let (k1, k2) = (*r.pick(&["join", "left join"]), *r.pick(&["join", "left join"]));
            let extra = if r.chance(40) { format!(" and t1.b > {}", r.below(6)) } else { String::new() };
            format!("select t1.id, t2.y, t3.w from t1 {k1} t2 on t1.a = t2.x {k2} t3 on t2.x = t3.a{extra}{wh} order by t1.id, t2.y, t3.w")
        }
        35 | 36 => {
            // USING / NATURAL joins; explicit select list because SELECT * orders columns differently
            let kind = *r.pick(&["join", "left join", "right join", "full join"]);
            let how = *r.pick(&["using (a)", "using (a, s)", "using (s)"]);
            format!("select a, t1.id, w from t1 {kind} t3 {how} order by a, t1.id, w")
        }
        37 => {
            format!("select t1.id, w from t1 natural join t3 order by t1.id, w")
        }
        38 => {
            format!("select x.id, y.id from t1 x join t1 y on x.a < y.a and x.b = y.b{} order by x.id, y.id", if r.chance(50) { " where x.c > 0" } else { "" })
        }
        40 | 41 => {
            // self-correlated subqueries: the inner table has the same name as the outer one
            let sub = *r.pick(&[
                "b > (select avg(b) from t1 c where c.s = o.s)", "o.c = (select max(c) from t1 d where d.a = o.a)",
                "exists (select 1 from t1 e where e.s = o.s and e.id <> o.id)", "not exists (select 1 from t1 e where e.a = o.a and e.id < o.id)",
                "o.b = (select b from t1 f where f.id = o.id)", "o.id = (select min(id) from t1 h where h.a = o.a)",
                "(select count(*) from t1 g where g.b < o.b) > 3", "o.a in (select a from t1 i where i.b > o.b)",
                "b = (select max(b) from t1 j where j.s = o.s and j.a = o.a)",
            ]);
            format!("select id from t1 o where {sub}{} order by id", wh.replace(" where", " and").replace("(not ", "(not o.").replace(" s ", " o.s ").replace("(s ", "(o.s ").replace(" id", " o.id").replace("(id", "(o.id"))
        }
        42 => {
            let sel = *r.pick(&["(select count(*) from t1 g where g.b < o.b)", "(select max(g.c) from t1 g where g.s = o.s)", "(select sum(g.b) from t1 g where g.a = o.a and g.id <> o.id)"]);
            format!("select id, {sel} as v from t1 o order by id")
        }
        _ => {
            // scalar functions whose behaviour is the same in Spark and SQLite
            let fs = [
                "upper(s)", "lower(s)", "length(s)", "substr(s, 2)", "substr(s, 1, 2)", "substr(s, -1)", "replace(s, 'a', 'x')",
                "instr(s, 'b')", "trim(s)", "ltrim(s)", "rtrim(s)", "abs(c)", "round(c, 1)", "round(c)", "coalesce(c, a, 7)",
                "nullif(a, 2)", "ifnull(s, '?')", "cast(c as integer)", "cast(a as text)", "(s || '-' || coalesce(s, '?'))",
                "(a * 2 + coalesce(b, 0))", "(c * 2)", "(case when a > 2 then a else c end)", "coalesce(a, c)", "coalesce(c, a)", "(case when b > 3 then s else a end)", "coalesce(s, a, c)", "cast(b as real)", "(a % 4)", "(case when a > 2 then s else upper(s) end)",
            ];
            let n = 1 + r.below(3);
            let items: Vec<String> = (0..n).map(|i| format!("{} as f{i}", r.pick(&fs))).collect();
            format!("select id, {} from t1{wh} order by id", items.join(", "))
        }
    }
}

// ── comparison ──

fn cell(c: &Column, r: usize) -> String {
    let num = |x: f64| -> String {
        if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{:.5}", x) }
    };
    match &c.data {
        ColumnData::Int64(v) => v[r].map(|x| num(x as f64)).unwrap_or("NULL".into()),
        ColumnData::Float64(v) => v[r].map(num).unwrap_or("NULL".into()),
        ColumnData::Bool(v) => v[r].map(|x| x.to_string()).unwrap_or("NULL".into()),
        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
    }
}

fn answer(c: &KqlContext, sql: &str) -> Result<Vec<String>, String> {
    let b = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.query(sql))) {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => return Err(format!("error: {e}")),
        Err(_) => return Err("panic".into()),
    };
    let mut rows: Vec<String> = (0..b.num_rows).map(|r| b.columns.iter().map(|c| cell(c, r)).collect::<Vec<_>>().join(",")).collect();
    // queries ordered by a unique-enough key compare in order; the rest as sets
    if !sql.contains(" order by ") { rows.sort(); }
    Ok(rows)
}

#[test]
fn random_queries_agree_across_execution_paths() {
    let mut mismatches: Vec<String> = Vec::new();
    let mut total = 0;
    let (mut answered, mut errored) = (0usize, 0usize);
    let seeds: u64 = std::env::var("DIFF_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    let per: usize = std::env::var("DIFF_PER").ok().and_then(|v| v.parse().ok()).unwrap_or(250);
    for seed in 1..=seeds {
        let mut r = Rng(seed * 7919);
        let ctx = tables(&mut r);
        for _ in 0..per {
            let sql = query(&mut r);
            total += 1;
            testing::set_force_general(false);
            testing::set_no_vecexpr(false);
            let fast = answer(&ctx, &sql);
            testing::set_force_general(true);
            let general = answer(&ctx, &sql);
            testing::set_force_general(false);
            testing::set_no_vecexpr(true);
            let rows_only = answer(&ctx, &sql);
            testing::set_no_vecexpr(false);
            // an error in one path and an answer in another also counts, but two errors are fine (unsupported shape)
            if fast.is_ok() { answered += 1; } else { errored += 1; }
            let agree = |a: &Result<Vec<String>, String>, b: &Result<Vec<String>, String>| match (a, b) {
                (Ok(x), Ok(y)) => x == y,
                (Err(_), Err(_)) => true,
                _ => false,
            };
            if !agree(&fast, &general) || !agree(&fast, &rows_only) {
                mismatches.push(format!("seed {seed}: {sql}\n   fast    {}\n   general {}\n   rowonly {}",
                    show(&fast), show(&general), show(&rows_only)));
            }
        }
    }
    testing::set_force_general(false);
    testing::set_no_vecexpr(false);
    println!("answered {answered}, errored {errored}");
    assert!(mismatches.is_empty(), "{} of {} random queries disagree:\n{}", mismatches.len(), total,
        mismatches.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}

fn show(a: &Result<Vec<String>, String>) -> String {
    match a { Ok(v) => format!("[{}]", v.iter().take(8).cloned().collect::<Vec<_>>().join(" | ")), Err(e) => e.clone() }
}
