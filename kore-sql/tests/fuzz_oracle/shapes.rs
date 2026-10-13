// Included by fuzz_oracle.rs (`include!`): query shapes for the join / aggregate machinery
// (aggregate push-down, wide group-by, cost-based join order, SIP bitmaps, composite keys, dates).

/// Relation pool of the chain generator: (table, alias). `c2` / `d2` repeat a table under a second alias.
const POOL: [(&str, &str); 7] = [("ta", "a"), ("tb", "b"), ("tc", "c"), ("td", "d"), ("te", "e"), ("tc", "c2"), ("td", "d2")];

fn is_many(al: &str) -> bool { al == "a" || al == "b" }

impl<'a> G<'a> {
    fn date(&mut self) -> String { iso_date(self.r) }

    fn pred2(&mut self, a: &str) -> String {
        let cmp = *self.r.pick(&["=", "<>", "<", "<=", ">", ">="]);
        match self.r.below(11) {
            0 => { let (d1, d2) = (self.date(), self.date()); let (lo, hi) = if d1 <= d2 { (d1, d2) } else { (d2, d1) }; format!("({a}.dt >= '{lo}' and {a}.dt < '{hi}')") }
            1 => format!("substr({a}.dt, 1, 4) {cmp} '{}'", 1992 + self.r.below(7)),
            2 => { let (d1, d2) = (self.date(), self.date()); let (lo, hi) = if d1 <= d2 { (d1, d2) } else { (d2, d1) }; format!("{a}.dt between '{lo}' and '{hi}'") }
            3 => { let d = self.date(); format!("{a}.dt {cmp} '{d}'") }
            4 => format!("{a}.s2 {} '{}'", self.r.pick(&["=", "<>", ">", "<"]), self.r.pick(&STRS2)),
            5 => format!("{a}.s2 like '{}'", self.r.pick(&["\u{dc}%", "%\u{672c}", "\u{65e5}%", "a%", "%"])),
            6 => format!("{a}.fg {cmp} {}", self.r.pick(&["0", "0.5", "-1.5", "2.25"])),
            7 => format!("{a}.dt is not null"),
            8 => format!("{a}.s2 in ('x', 'ab', '\u{e9}')"),
            _ => self.tab_pred_a(a),
        }
    }

    fn agg2(&mut self, als: &[String]) -> String {
        let a = self.r.pick(als).clone();
        let b = self.r.pick(als).clone();
        match self.r.below(20) {
            0 => "count(*)".into(),
            1 => format!("count({})", self.col_a(&a, "int")),
            2 => format!("count({})", self.col_a(&a, "str2")),
            3 => format!("count(distinct {})", self.col_a(&a, "int")),
            4 => format!("count(distinct {})", self.col_a(&a, "str2")),
            5 => format!("sum({})", self.col_a(&a, "sumint")),
            6 => format!("sum({})", self.col_a(&a, "float2")),
            7 => format!("avg({})", self.col_a(&a, "sumint")),
            8 => format!("avg({})", self.col_a(&a, "float2")),
            9 => format!("min({})", self.col_a(&a, "int")),
            10 => format!("max({})", self.col_a(&a, "int")),
            11 => format!("min({})", self.col_a(&a, "str2")),
            12 => format!("max({})", self.col_a(&a, "str2")),
            13 => format!("min({})", self.col_a(&a, "float2")),
            14 => format!("max({})", self.col_a(&a, "float2")),
            15 => format!("sum({} * {})", self.col_a(&a, "float2"), self.col_a(&b, "sumint")),
            16 => format!("sum({} + {})", self.col_a(&a, "sumint"), self.col_a(&b, "sumint")),
            17 => format!("sum(case when {} then 1 else 0 end)", self.pred2(&a)),
            18 => format!("sum(case when {} then {} else 0 end)", self.pred2(&a), self.col_a(&b, "float2")),
            _ => format!("count({})", self.col_a(&b, "key")),
        }
    }

    /// Comma-join chain over 2..6 relations of POOL: several link kinds (single key, composite k1+k2, k1+s2, date,
    /// k1=k2), extra links to a second parent (a table with several links), repeated aliases of the unique-key tables.
    /// `star`: the many side is `a` only and every other relation is a unique-key dimension.
    /// Returns (aliases in join order, FROM list, conjuncts).
    fn chain(&mut self, nt: usize, star: bool) -> (Vec<String>, String, Vec<String>) {
        let big = self.n >= 200;
        let mut idx: Vec<usize> = if star { vec![0, 2, 3, 4, 5, 6] } else { (0..7).collect() };
        // small data: te has 0..5 rows and empties every chain it is in; keep it in only sometimes
        if !big && self.r.chance(75) { idx.retain(|&i| i != 4); }
        for i in (1..idx.len()).rev() { let j = self.r.below(i + 1); idx.swap(i, j); }
        idx.truncate(nt.min(idx.len()));
        // root: the fact table most of the time (the push-down precondition is a many-side join key)
        if self.r.chance(75) {
            let pos = idx.iter().position(|&i| i == 0 || (i == 1 && !star));
            match pos { Some(p) => idx.swap(0, p), None => idx[0] = 0 }
        }
        let als: Vec<String> = idx.iter().map(|&i| POOL[i].1.to_string()).collect();
        let mut conds: Vec<String> = Vec::new();
        for i in 1..als.len() {
            let (t, par) = (als[i].clone(), als[self.r.below(i)].clone());
            let both_many = is_many(&t) && is_many(&par);
            let kind = if both_many { self.r.below(3) } else if !big && self.r.chance(55) { 0 } else { self.r.below(10) };
            let small_pair = t == "e" || par == "e" || (!is_many(&t) && !is_many(&par));
            let c = match kind {
                0..=4 => format!("{t}.k1 = {par}.k1"),
                5 | 6 => format!("{t}.k1 = {par}.k1 and {t}.k2 = {par}.k2"),
                7 => format!("{t}.k1 = {par}.k1 and {t}.s2 = {par}.s2"),
                8 if !big || small_pair => format!("{t}.dt = {par}.dt"),
                8 => format!("{t}.k1 = {par}.k1 and {t}.k2 = {par}.k2"),
                _ => if !big || small_pair { format!("{t}.k1 = {par}.k2") } else { format!("{par}.k1 = {t}.k1") },
            };
            conds.push(c);
            // second link to another earlier relation (cycle): a table with several links
            if i >= 2 && self.r.chance(35) {
                let p2 = als[self.r.below(i)].clone();
                if p2 != par && !(is_many(&t) && is_many(&p2)) {
                    conds.push(if self.r.chance(50) { format!("{t}.k1 = {p2}.k1") } else { format!("{t}.k1 = {p2}.k1 and {t}.k2 = {p2}.k2") });
                }
            }
        }
        let mut order = idx.clone();
        for i in (1..order.len()).rev() { if self.r.chance(40) { let j = self.r.below(i + 1); order.swap(i, j); } }
        let from = order.iter().map(|&i| format!("{} {}", POOL[i].0, POOL[i].1)).collect::<Vec<_>>().join(", ");
        for al in &als { if self.r.chance(self.fpct) { let p = self.pred2(al); conds.push(p); } }
        (als, from, conds)
    }

    /// up to `max` distinct group-key expressions over `als`; `must` (if any) is forced in first.
    fn group_keys(&mut self, als: &[String], max: usize, must: Option<String>, only: Option<&[String]>) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        if let Some(m) = must { keys.push(m); }
        let pool: Vec<String> = only.map(|o| o.to_vec()).unwrap_or_else(|| als.to_vec());
        for _ in 0..max {
            let t = self.r.pick(&pool).clone();
            let k = self.col_a(&t, "grp2");
            if !keys.contains(&k) { keys.push(k); }
        }
        keys.truncate(max.max(1));
        keys
    }

    /// select-list / group-by / having / order-by tail shared by the aggregate shapes. Returns (sql, ordered).
    fn agg_query(&mut self, als: &[String], from: &str, conds: &[String], keys: &[String], max_aggs: usize) -> (String, bool) {
        let na = 1 + self.r.below(max_aggs);
        let aggs: Vec<String> = (0..na).map(|_| self.agg2(als)).collect();
        let sel: Vec<String> = keys.iter().enumerate().map(|(i, k)| format!("{k} as k{i}")).chain(aggs.iter().enumerate().map(|(i, a)| format!("{a} as m{i}"))).collect();
        let gb = if keys.is_empty() { String::new() } else { format!(" group by {}", keys.join(", ")) };
        let having = if keys.is_empty() { String::new() } else {
            match self.r.below(8) {
                0 => format!(" having count(*) {} {}", self.r.pick(&[">", ">=", "<"]), self.r.below(6)),
                1 => format!(" having {} is not null", aggs[0]),
                2 => { let a = self.r.pick(als).clone(); format!(" having max({}) is not null and count(*) > 1", self.col_a(&a, "int")) }
                _ => String::new(),
            }
        };
        let w = Self::where_clause(conds);
        if !keys.is_empty() && self.r.chance(30) {
            let ob = (0..keys.len()).map(|i| format!("k{i} {}", self.r.pick(&["asc nulls first", "desc nulls last", "asc nulls last", "desc nulls first"]))).collect::<Vec<_>>().join(", ");
            let off = if self.r.chance(30) { format!(" offset {}", self.r.below(5)) } else { String::new() };
            (format!("select {} from {from}{w}{gb}{having} order by {ob} limit {}{off}", sel.join(", "), 1 + self.r.below(60)), true)
        } else {
            (format!("select {} from {from}{w}{gb}{having}", sel.join(", ")), false)
        }
    }

    fn query2(&mut self) -> (String, bool) {
        let big = self.n >= 200;
        match self.r.below(16) {
            // aggregate over a 2..6 table chain; GROUP BY contains / does not contain the many-side join key
            0..=3 => {
                let nt = 2 + self.r.below(5);
                let (als, from, conds) = self.chain(nt, false);
                let many: Vec<String> = als.iter().filter(|a| is_many(a)).cloned().collect();
                let must = if !many.is_empty() && self.r.chance(50) { Some(format!("{}.k1", self.r.pick(&many))) } else { None };
                let other: Vec<String> = als.iter().filter(|a| !is_many(a)).cloned().collect();
                let only = if must.is_none() && !other.is_empty() && self.r.chance(60) { Some(other) } else { None };
                let mk = 1 + self.r.below(8);
                let keys = self.group_keys(&als, mk, must, only.as_deref());
                self.agg_query(&als, &from, &conds, &keys, 5)
            }
            // star: fact table + unique-key dimension tables with filters (SIP / push-down / lookup-join shape)
            4 | 5 => {
                let nt = 2 + self.r.below(4);
                let (als, from, mut conds) = self.chain(nt, true);
                if let Some(d) = als.iter().find(|a| !is_many(a)).cloned() { let p = self.pred2(&d); conds.push(p); }
                let keys = if self.r.chance(15) { Vec::new() } else { let mk = 1 + self.r.below(4); self.group_keys(&als, mk, None, None) };
                self.agg_query(&als, &from, &conds, &keys, 4)
            }
            // wide GROUP BY (1..8 keys) on one big table: fused aggregate windows / wide keys
            6 => {
                let (t, al) = *self.r.pick(&[("ta", "a"), ("tb", "b"), ("ta", "a")]);
                let als = vec![al.to_string()];
                let nk = 1 + self.r.below(8);
                let keys = self.group_keys(&als, nk, None, None);
                let nc = self.r.below(3);
                let conds: Vec<String> = (0..nc).map(|_| self.pred2(al)).collect();
                self.agg_query(&als, &format!("{t} {al}"), &conds, &keys, 5)
            }
            // comma-joined tables followed by an explicit JOIN chain (the parser has no `JOIN .. ON .., table` form)
            7 | 8 => {
                let mut idx: Vec<usize> = vec![0, 2, 3, 4, 5, 6];
                for i in (1..idx.len()).rev() { let j = self.r.below(i + 1); idx.swap(i, j); }
                if big && !idx[..3].contains(&0) { idx[0] = 0; }
                let ncomma = self.r.below(3);
                let nj = 1 + self.r.below(3);
                let als: Vec<String> = idx.iter().take(1 + ncomma + nj).map(|&i| POOL[i].1.to_string()).collect();
                let mut from = format!("{} {}", POOL[idx[0]].0, POOL[idx[0]].1);
                let mut conds: Vec<String> = Vec::new();
                for i in 1..=ncomma {
                    from.push_str(&format!(", {} {}", POOL[idx[i]].0, POOL[idx[i]].1));
                    let par = als[self.r.below(i)].clone();
                    conds.push(format!("{}.k1 = {par}.k1", als[i]));
                }
                for i in 1 + ncomma..1 + ncomma + nj {
                    let (t, par) = (als[i].clone(), als[self.r.below(i)].clone());
                    let kind = *self.r.pick(&["join", "left join", "join", "inner join"]);
                    let key = match self.r.below(4) { 0 => format!("{t}.k1 = {par}.k1 and {t}.k2 = {par}.k2"), 1 => format!("{par}.k1 = {t}.k1 and {t}.s2 = {par}.s2"), _ => format!("{t}.k1 = {par}.k1") };
                    let extra = if self.r.chance(30) { format!(" and {}", self.pred2(&t)) } else { String::new() };
                    from.push_str(&format!(" {kind} {} {t} on {key}{extra}", POOL[idx[i]].0));
                }
                for al in &als { if self.r.chance(self.fpct) { let p = self.pred2(al); conds.push(p); } }
                let keys = if self.r.chance(15) { Vec::new() } else { let mk = 1 + self.r.below(5); self.group_keys(&als, mk, None, None) };
                self.agg_query(&als, &from, &conds, &keys, 4)
            }
            // derived tables / CTEs feeding joins (pre-aggregated by the join key, as the push-down would)
            9 | 10 => {
                let p = self.pred2("a");
                let p = if self.r.chance(50) { format!(" where {p}") } else { String::new() };
                let (dim, da) = *self.r.pick(&[("tc", "c"), ("td", "d"), ("tc", "c")]);
                match self.r.below(5) {
                    0 => (format!("select {da}.g, sum(q.m), count(*), max(q.n) from (select a.k1 as kk, sum(a.v) as m, count(*) as n from ta a{p} group by a.k1) q, {dim} {da} where q.kk = {da}.k1 group by {da}.g"), false),
                    1 => (format!("select q.gg, count(*), sum(q.m), min({da}.s2) from (select a.k1 as kk, a.g as gg, sum(a.f) as m from ta a{p} group by a.k1, a.g) q, {dim} {da} where q.kk = {da}.k1 group by q.gg"), false),
                    2 => (format!("with q as (select b.k1 as kk, count(*) as n, max(b.f) as mf from tb b group by b.k1) select count(*), sum(q1.n), sum(q2.mf) from q q1, q q2, {dim} {da} where q1.kk = {da}.k1 and q2.kk = q1.kk"), false),
                    3 => (format!("select {da}.g, count(*), count(q.kk), sum(q.m) from {dim} {da} left join (select a.k1 as kk, sum(a.v) as m from ta a{p} group by a.k1) q on q.kk = {da}.k1 group by {da}.g"), false),
                    _ => (format!("select q.s2, q.fg, count(*), sum(q.m) from (select a.s2 as s2, a.fg as fg, a.k1 as kk, sum(a.f) as m from ta a{p} group by a.s2, a.fg, a.k1) q group by q.s2, q.fg"), false),
                }
            }
            // self-joins and repeated aliases of the same table
            11 => {
                let t = if big { *self.r.pick(&["tc", "td"]) } else { *self.r.pick(&["ta", "tb", "tc", "td"]) };
                let key = *self.r.pick(&["k1", "k1", "k2"]);
                let key = if big && key == "k2" { "k1" } else { key };
                let extra = if !big || key == "k1" { self.r.pick(&[" and x.v < y.v", " and x.k2 <> y.k2", " and x.s2 = y.s2", ""]).to_string() } else { String::new() };
                let g = *self.r.pick(&["x.g", "y.g", "x.fg", "substr(x.dt, 1, 4)", "x.s2"]);
                (format!("select {g}, count(*), sum(y.v), max(x.dt), count(distinct y.s2) from {t} x join {t} y on x.{key} = y.{key}{extra} group by {g}"), false)
            }
            // dates: filters, group keys, join keys, malformed values must fall back cleanly
            12 | 13 => {
                let (d1, d2) = (self.date(), self.date());
                let (lo, hi) = if d1 <= d2 { (d1, d2) } else { (d2, d1) };
                match self.r.below(5) {
                    0 => (format!("select substr(a.dt, 1, 4) as y, count(*), sum(a.v), min(a.dt), max(a.dt) from ta a where a.dt >= '{lo}' and a.dt < '{hi}' group by 1"), false),
                    1 => (format!("select count(*), sum(x.v), min(x.dt), count(distinct y.dt) from tc x, td y where x.dt = y.dt and {}", self.pred2("x")), false),
                    2 => (format!("select substr(a.dt, 1, 4) as y, substr(c.dt, 1, 7) as m, count(*), sum(a.f), avg(c.v) from ta a, tc c where a.k1 = c.k1 and a.dt < '{hi}' group by substr(a.dt, 1, 4), substr(c.dt, 1, 7)"), false),
                    3 => {
                        let lim = if big { 1 + self.r.below(5000) } else { 1 + self.r.below(20) };
                        (format!("select a.id, a.dt, a.g from ta a where a.dt between '{lo}' and '{hi}' order by a.dt {} nulls last, a.id limit {lim}", self.r.pick(&["asc", "desc"])), true)
                    }
                    _ => ("select a.dt, count(*), sum(a.v) from ta a, te e where a.dt = e.dt group by a.dt".to_string(), false),
                }
            }
            // short chains: aggregate directly over a join with NULL / duplicate keys on both sides
            14 => {
                let nt = 2 + self.r.below(2);
                let (als, from, conds) = self.chain(nt, false);
                let keys = self.group_keys(&als, 2, None, None);
                self.agg_query(&als, &from, &conds, &keys, 3)
            }
            // ORDER BY + LIMIT over a wide join projection
            _ => {
                let nt = 2 + self.r.below(3);
                let (als, from, conds) = self.chain(nt, false);
                let nc = 1 + self.r.below(3);
                let mut cols: Vec<String> = Vec::new();
                for _ in 0..nc { let t = self.r.pick(&als).clone(); let k = *self.r.pick(&["int", "float2", "str2", "key"]); let c = self.col_a(&t, k); if !cols.contains(&c) { cols.push(c); } }
                let dirs: Vec<String> = cols.iter().map(|c| format!("{c} {} {}", self.r.pick(&["asc", "desc"]), self.r.pick(&["nulls first", "nulls last"]))).collect();
                let m = if self.r.chance(30) { 70_000 } else { 120 };
                let lim = if big { 1 + self.r.below(m) } else { 1 + self.r.below(30) };
                (format!("select {} from {from}{} order by {} limit {lim}", cols.join(", "), Self::where_clause(&conds), dirs.join(", ")), true)
            }
        }
    }
}
