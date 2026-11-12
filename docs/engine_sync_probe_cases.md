# Engine sync, step 2 appendix: every failed case of master's case-list tests on dev's old engine copy

Source: master-kore-engine 236ae86c tests `ds_regress.rs`, `sql_coverage.rs`, `sql_coverage2.rs`, `sql_gaps3.rs` run against dev 8589a219's kore-sql (see docs/engine_sync.md, section 2).
Kind: **wrong** = dev returns a different result without an error (silent); **error** = dev rejects a query master answers; **accepts** = dev answers a query master rejects. Rows `|`, cells `,`, empty = no rows. Only the first failing check of each test is shown (a test stops at its first failing check).

| test file | test | kind | SQL | expected (master) | dev |
|---|---|---|---|---|---|
| ds_regress | comma_join_after_explicit_join | wrong | `select count(*) from u left join w on uid = wid, t where t.id = 6 and w.wid is null` | 1 | 3 |
| ds_regress | comma_join_after_explicit_join | error | `select uid, un, wid, id from u left join w on uid = wid, t where t.id = u.uid order by uid` | 1,'a',1,1\|2,'b',2,2 | ERR(invalid argument: column not found: id) |
| ds_regress | comma_join_after_explicit_join | error | `select uid, wid, id from u left join w on uid = wid, t where t.id = u.uid + 3 order by uid` | 1,1,4\|2,2,5 | ERR(invalid argument: column not found: id) |
| ds_regress | constant_select_item_with_group_by_keeps_type | wrong | `select 0 as gflag, g, sum(v) s from t group by g order by g nulls first` | 0,NULL,60\|0,'x',30\|0,'y',30\|0,'z',50 | '0',NULL,60\|'0','x',30\|'0','y',30\|'0','z',50 |
| ds_regress | constant_select_item_with_group_by_keeps_type | error | `select * from (select 0 as k, g, avg(v) a from t group by g union all select 1 as k, null as g, avg(v) a from t) q order by k desc, g nulls first limit 2` | 1,NULL,34\|0,NULL,60 | ERR(invalid argument: expected RParen, got Union) |
| ds_regress | exists_over_multi_table_subquery | error | `select id from t where exists (select 1 from u, w where uid = wid and uid = t.id) order by id` | 1\|2 | ERR(invalid argument: expected RParen, got Comma) |
| ds_regress | exists_over_multi_table_subquery | error | `select id from t where exists (select 1 from u, w where uid = wid and uid = t.v / 10) order by id` | 1\|2 | ERR(invalid argument: expected RParen, got Comma) |
| ds_regress | exists_over_multi_table_subquery | error | `select id from t where id > 1 and (exists (select 1 from u, w where uid = wid and uid = t.id and un = 'b') or exists (select 1 from u, w where uid = wid and uid = t.id + 1 and un = 'a')) order by id` | 2 | ERR(invalid argument: expected RParen, got Comma) |
| ds_regress | exists_over_multi_table_subquery | error | `select id from t where not exists (select 1 from u, w where uid = wid and uid = t.id) order by id` | 3\|4\|5\|6 | ERR(invalid argument: expected RParen, got Comma) |
| ds_regress | exists_over_multi_table_subquery | error | `select id from t where not exists (select 1 from u, w where uid = wid and uid = t.v / 10) order by id` | 3\|4\|5\|6 | ERR(invalid argument: expected RParen, got Comma) |
| ds_regress | exists_over_multi_table_subquery | error | `select id from t where not exists (select 1 from u, w where uid = wid and uid = t.v) order by id` | 1\|2\|3\|4\|5\|6 | ERR(invalid argument: expected RParen, got Comma) |
| ds_regress | explicit_inner_chain_and_trailing_left_join | wrong | `select count(*) from t join u on t.id = u.uid left join w on w.wid = t.id + 1 where w.wid is not null` | 1 | 2 |
| ds_regress | explicit_inner_chain_and_trailing_left_join | wrong | `select count(*) from t join u on t.id = u.uid left join w on w.wid = t.id + 1 where w.wid is null` | 1 | 2 |
| ds_regress | explicit_inner_chain_and_trailing_left_join | wrong | `select id, wid from t join u on t.id = u.uid left join w on w.wid = t.id + 1 order by id` | 1,2\|2,NULL | 1,1\|2,2 |
| ds_regress | explicit_inner_chain_and_trailing_left_join | error | `select id, un from t join u on t.id + 0 = u.uid where un <> 'a' order by id` | 2,'b' | ERR(invalid argument: expected Eq, got Plus) |
| ds_regress | explicit_inner_chain_and_trailing_left_join | error | `select id, wid from t, u left join w on w.wid = u.uid where t.id = u.uid order by id` | 1,1\|2,2 | ERR(invalid argument: column not found: wid) |
| ds_regress | explicit_inner_chain_and_trailing_left_join | error | `select uid, wid from u, t left join w on w.wid = t.id where u.uid = t.id + 5 order by uid` | 7,2 | ERR(invalid argument: column not found: wid) |
| ds_regress | group_by_expression_alias_keeps_type | wrong | `select f * 1.0001 as ff, count(*) n from t where f = 1.5 group by ff` | 1.50015,1 | '1.5002',1 |
| ds_regress | group_by_expression_alias_keeps_type | wrong | `select id % 2 as par, count(*) n from t group by par order by par` | 0,3\|1,3 | '0.0000',3\|'1.0000',3 |
| ds_regress | order_by_qualified_group_column_with_having | wrong | `select a.g gg, count(*) cnt from t a where a.g is not null group by a.g having count(*) >= 1 order by a.g desc` | 'z',1\|'y',2\|'x',2 |  |
| ds_regress | order_by_qualified_group_column_with_having | wrong | `select a.g gg, count(*) cnt from t a where a.g is not null group by a.g having count(*) >= 2 order by cnt, a.g` | 'x',2\|'y',2 |  |
| ds_regress | star_mixed_with_expressions | wrong | `select *, uid + 1 as x from u order by uid` | 1,'a',2\|2,'b',3\|7,'c',8 | 1,'a'\|2,'b'\|7,'c' |
| ds_regress | star_mixed_with_expressions | wrong | `select id * 10 as x, * from t where id < 3 order by id` | 10,1,'x',10,1.5\|20,2,'x',20,2.5 | 1,'x',10,1.5\|2,'x',20,2.5 |
| ds_regress | star_mixed_with_expressions | error | `select a, uid from (select upper(un) a, * from u) q order by uid` | 'A',1\|'B',2\|'C',7 | ERR(invalid argument: column not found: a) |
| sql_coverage | aggregate_output_naming | wrong | `select count(1), count(null) from t` | 6,0 | 6,6 |
| sql_coverage | aggregate_output_naming | wrong | `select g, sum(v*2), sum(v*3) from t group by g order by g` | NULL,120,180\|x,60,90\|y,60,90\|z,100,150 | NULL,120,120\|x,60,60\|y,60,60\|z,100,100 |
| sql_coverage | aggregate_output_naming | wrong | `select sum(v*2), sum(v*3) from t` | 340,510 | 340,340 |
| sql_coverage | aggregate_output_naming | error | `select sum(v), sum(distinct v) from t` | 170,170 | ERR(invalid argument: unexpected token in expr: Distinct) |
| sql_coverage | analytic_query_shapes | wrong | `select (select max(v) from t) - (select min(v) from t) from t where id = 1` | 50 | NULL |
| sql_coverage | analytic_query_shapes | wrong | `select coalesce(g, 'none') g2, sum(v) from t group by coalesce(g, 'none') order by g2` | none,60\|x,30\|y,30\|z,50 | x,170 |
| sql_coverage | analytic_query_shapes | wrong | `select coalesce(max(v), -1) from t where id > 100` | -1 |  |
| sql_coverage | analytic_query_shapes | wrong | `select count(*) from t a cross join t b` | 36 | 6 |
| sql_coverage | analytic_query_shapes | wrong | `select g, count(*) from t group by g having max(v) > 20 order by g` | NULL,1\|y,2\|z,1 |  |
| sql_coverage | analytic_query_shapes | wrong | `select g, round(sum(v) * 100.0 / sum(sum(v)) over (), 1) from t group by g order by g` | NULL,35.3\|x,17.6\|y,17.6\|z,29.4 | NULL,NULL\|x,NULL\|y,NULL\|z,NULL |
| sql_coverage | analytic_query_shapes | wrong | `select id, case when sum(v) over (partition by g) > 35 then 'big' else 'small' end from t order by id` | 1,small\|2,small\|3,small\|4,small\|5,big\|6,big | 1,small\|2,small\|3,small\|4,small\|5,small\|6,small |
| sql_coverage | analytic_query_shapes | wrong | `select id, lead(id) over (order by id), lag(id, 2) over (order by id) from t where id < 4 order by id` | 1,2,NULL\|2,3,NULL\|3,NULL,1 | 1,2,2\|2,3,3\|3,NULL,NULL |
| sql_coverage | analytic_query_shapes | wrong | `select id, sum(v) over (order by id) - v from t order by id` | 1,0\|2,10\|3,30\|4,NULL\|5,60\|6,110 | 1,NULL\|2,NULL\|3,NULL\|4,NULL\|5,NULL\|6,NULL |
| sql_coverage | analytic_query_shapes | wrong | `select id, sum(v) over (partition by g order by id) from t order by id` | 1,10\|2,30\|3,30\|4,30\|5,50\|6,60 | 1,30\|2,30\|3,30\|4,30\|5,50\|6,60 |
| sql_coverage | analytic_query_shapes | wrong | `select round(sum(v) * 100.0 / (select sum(v) from t), 1) from t where g = 'x'` | 17.6 | NULL\|NULL |
| sql_coverage | analytic_query_shapes | wrong | `select string_agg(g, '-') from t where g is not null` | x-x-y-y-z |  |
| sql_coverage | analytic_query_shapes | wrong | `select t.g, sum(t.v) over (order by t.id) from t where t.id < 3 order by t.id` | x,10\|x,30 | x,30\|x,30 |
| sql_coverage | analytic_query_shapes | wrong | `select upper(g) as ug, count(*) as c from t where g is not null group by upper(g) having count(*) > 1 order by ug` | X,2\|Y,2 | X,5 |
| sql_coverage | analytic_query_shapes | error | `select * from (select id, v from t) a join (select id, g from t) b on a.id = b.id where a.id = 1` | 1,10,1,x | ERR(invalid argument: unknown table: b) |
| sql_coverage | analytic_query_shapes | error | `select * from range(3)` | 0\|1\|2 | ERR(invalid argument: expected identifier, got Range) |
| sql_coverage | analytic_query_shapes | error | `select a.id, b.id from t a, t b where a.id + 1 = b.id and a.id < 3 order by a.id` | 1,2\|2,3 | ERR(invalid argument: column not found: b.id) |
| sql_coverage | analytic_query_shapes | error | `select any_value(g) from t` | x | ERR(invalid argument: unknown function: any_value) |
| sql_coverage | analytic_query_shapes | error | `select collect_set(g) from t where g is not null` | [x, y, z] | ERR(invalid argument: unknown function: collect_set) |
| sql_coverage | analytic_query_shapes | error | `select count(*) from t a right join u b on a.id = b.id` | 4 | ERR(column not found: b.id) |
| sql_coverage | analytic_query_shapes | error | `select first(g), last(g) from t` | x,NULL | ERR(invalid argument: unknown function: first) |
| sql_coverage | analytic_query_shapes | error | `select first(g, true), last(g, true) from t` | x,z | ERR(invalid argument: unknown function: first) |
| sql_coverage | analytic_query_shapes | error | `select g from t group by g order by max(v) desc limit 1` | NULL | ERR(invalid argument: ORDER BY column not found: max) |
| sql_coverage | analytic_query_shapes | error | `select g, sum(v) from t group by g order by sum(v) desc limit 1` | NULL,60 | ERR(invalid argument: ORDER BY column not found: sum) |
| sql_coverage | analytic_query_shapes | error | `select id from range(1, 8, 3)` | 1\|4\|7 | ERR(invalid argument: expected identifier, got Range) |
| sql_coverage | analytic_query_shapes | error | `select id from t where id <> all (select id from u) order by id` | 3\|4\|5\|6 | ERR(invalid argument: unexpected token in expr: All) |
| sql_coverage | analytic_query_shapes | error | `select id from t where id = any (select id from u) order by id` | 1\|2 | ERR(invalid argument: unknown function: any) |
| sql_coverage | analytic_query_shapes | error | `select id from t where id > all (select id from u where id < 3) order by id` | 3\|4\|5\|6 | ERR(invalid argument: unexpected token in expr: All) |
| sql_coverage | analytic_query_shapes | error | `select id from t where id in (select id from u union select id from t where id = 3) order by id` | 1\|2\|3 | ERR(invalid argument: expected RParen, got Union) |
| sql_coverage | analytic_query_shapes | error | `select max(a) from (values (1), (3), (2)) as q(a)` | 3 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | analytic_query_shapes | error | `select struct(1,2) from t` | {1, 2}\|{1, 2}\|{1, 2}\|{1, 2}\|{1, 2}\|{1, 2} | ERR(invalid argument: unknown function: struct) |
| sql_coverage | analytic_query_shapes | error | `select sum(id) from range(5)` | 10 | ERR(invalid argument: expected identifier, got Range) |
| sql_coverage | analytic_query_shapes | error | `select try_divide(1, 0), try_divide(6, 3) from t where id = 1` | NULL,2 | ERR(invalid argument: unknown function: try_divide) |
| sql_coverage | analytic_query_shapes | accepts | `select * from t tablesample (foo)` | ERR | 1,x,10,Hello,1.5,2024-01-15\|2,x,20,  pad ,2.5,2024-02-29\|3,y,30,abc,-3.5,2023-12-31\|4,y,NULL,NULL,NULL,NULL\|5,z,50,a,b,c,4,2024-03-01\|6,NULL,60,World,0,2024-01-01 |
| sql_coverage | analytic_query_shapes | accepts | `select 1; select 2` | ERR | 1 |
| sql_coverage | analytic_query_shapes | accepts | `select g, count(*) from t group by g having v > 1` | ERR |  |
| sql_coverage | analytic_query_shapes | accepts | `select g, v, count(*) from t group by g` | ERR | x,10,2\|y,30,2\|z,50,1\|NULL,60,1 |
| sql_coverage | array_functions | error | `select * from (select array(1, 2) as a) q` | [1, 2] | ERR(invalid argument: unknown function: array) |
| sql_coverage | array_functions | error | `select [1, 2.5] from t where id = 1` | [1.0, 2.5] | ERR(invalid argument: unexpected char '[' at pos 7) |
| sql_coverage | array_functions | error | `select a, count(*) from (select array(1, 2) as a union all select array(1, 2)) q group by a` | [1, 2],2 | ERR(invalid argument: unknown function: array) |
| sql_coverage | array_functions | error | `select array('a', null, 'c') from t where id = 1` | [a, null, c] | ERR(invalid argument: unknown function: array) |
| sql_coverage | array_functions | error | `select array(1, 2, 3) from t where id = 1` | [1, 2, 3] | ERR(invalid argument: unknown function: array) |
| sql_coverage | array_functions | error | `select array(array(1), array(2, 3)) from t where id = 1` | [[1], [2, 3]] | ERR(invalid argument: unknown function: array) |
| sql_coverage | array_functions | error | `select array_contains(1, 1) from t where id = 1` | NULL | ERR(invalid argument: unknown function: array_contains) |
| sql_coverage | array_functions | error | `select array_contains(array(1, 2, null), 2), array_contains(array(1, 2), 3), array_contains(array(1, null), 3) from t where id = 1` | true,false,NULL | ERR(invalid argument: unknown function: array_contains) |
| sql_coverage | array_functions | error | `select array_distinct(array(1, 2, 1, 3, 2)), array_union(array(1, 2), array(2, 3)), array_intersect(array(1, 2, 3), array(2, 3, 4)), array_except(array(1, 2, 3), array(2)) from t where id = 1` | [1, 2, 3],[1, 2, 3],[2, 3],[1, 3] | ERR(invalid argument: unknown function: array_distinct) |
| sql_coverage | array_functions | error | `select array_join(array('a', 'b', null), '-'), array_join(array('a', null), '-', '?'), concat_ws('+', array('x', 'y'), 'z') from t where id = 1` | a-b,a-?,x+y+z | ERR(invalid argument: unknown function: array_join) |
| sql_coverage | array_functions | error | `select array_max(array(3, 1, 2)), array_min(array(3, 1, 2)), array_position(array('a', 'b'), 'b'), array_remove(array(1, 2, 1), 1) from t where id = 1` | 3,1,2,[2] | ERR(invalid argument: unknown function: array_max) |
| sql_coverage | array_functions | error | `select arrays_overlap(array(1, 2), array(2, 3)), array_compact(array(1, null, 2)), array_append(array(1), 2), array_prepend(array(1), 0) from t where id = 1` | true,[1, 2],[1, 2],[0, 1] | ERR(invalid argument: unknown function: arrays_overlap) |
| sql_coverage | array_functions | error | `select count(*) from t where array_contains(split(s, ','), 'b')` | 1 | ERR(invalid argument: unknown function: array_contains) |
| sql_coverage | array_functions | error | `select e from t lateral view explode(array(1, 2)) q as e where id = 1` | 1\|2 | ERR(invalid argument: column not found: e) |
| sql_coverage | array_functions | error | `select e from t lateral view explode(split(s, ',')) q as e where id = 5` | a\|b\|c | ERR(invalid argument: column not found: e) |
| sql_coverage | array_functions | error | `select explode(split(s, ',')) from t where id = 5` | a\|b\|c | ERR(invalid argument: unknown function: explode) |
| sql_coverage | array_functions | error | `select g, array_join(sort_array(collect_list(s)), '\|') from t where g = 'x' group by g` | x,  pad \|Hello | ERR(invalid argument: unknown function: array_join) |
| sql_coverage | array_functions | error | `select g, collect_list(v) from t where g is not null group by g order by g` | x,[10, 20]\|y,[30]\|z,[50] | ERR(invalid argument: unknown function: collect_list) |
| sql_coverage | array_functions | error | `select g, size(collect_set(s)) from t group by g order by g` | NULL,1\|x,2\|y,1\|z,1 | ERR(invalid argument: unknown function: size) |
| sql_coverage | array_functions | error | `select id, e from (select id, split(s, ',') as parts from t where id in (1, 5)) q lateral view explode(parts) x as e order by id, e` | 1,Hello\|5,a\|5,b\|5,c | ERR(invalid argument: unknown function: split) |
| sql_coverage | array_functions | error | `select id, size(parts) from (select id, split(s, ',') as parts from t) q order by id` | 1,1\|2,1\|3,1\|4,-1\|5,3\|6,1 | ERR(invalid argument: unknown function: size) |
| sql_coverage | array_functions | error | `select map('a', 1) from t where id = 1` | {a -> 1} | ERR(invalid argument: unknown function: map) |
| sql_coverage | array_functions | error | `select sequence(1, 4), sequence(3, 1), sequence(1, 10, 4) from t where id = 1` | [1, 2, 3, 4],[3, 2, 1],[1, 5, 9] | ERR(invalid argument: unknown function: sequence) |
| sql_coverage | array_functions | error | `select sequence(to_date('2024-01-30'), to_date('2024-02-01')) from t where id = 1` | [2024-01-30, 2024-01-31, 2024-02-01] | ERR(invalid argument: unknown function: sequence) |
| sql_coverage | array_functions | error | `select size(array(1, 2, 3)), size(split(s, ',')), cardinality(array()) from t where id = 5` | 3,3,0 | ERR(invalid argument: unknown function: size) |
| sql_coverage | array_functions | error | `select size(null) from t where id = 1` | -1 | ERR(invalid argument: unknown function: size) |
| sql_coverage | array_functions | error | `select slice(array(1, 2, 3, 4), 2, 2), slice(array(1, 2, 3, 4), -2, 2), flatten(array(array(1), array(2, 3))), reverse(array(1, 2, 3)), array_repeat('x', 2) from t where id = 1` | [2, 3],[3, 4],[1, 2, 3],[3, 2, 1],[x, x] | ERR(invalid argument: unknown function: slice) |
| sql_coverage | array_functions | error | `select sort_array(array(3, 1, null, 2)), sort_array(array('b', 'a'), false) from t where id = 1` | [null, 1, 2, 3],[b, a] | ERR(invalid argument: unknown function: sort_array) |
| sql_coverage | array_functions | error | `select sort_array(collect_list(id)) from t` | [1, 2, 3, 4, 5, 6] | ERR(invalid argument: unknown function: sort_array) |
| sql_coverage | array_functions | error | `select sort_array(split(s, ',')) from t where id = 5` | [a, b, c] | ERR(invalid argument: unknown function: sort_array) |
| sql_coverage | array_functions | error | `select split(s, ',') from t where id = 5` | [a, b, c] | ERR(invalid argument: unknown function: split) |
| sql_coverage | array_functions | error | `select split(s, ',')[1], element_at(split(s, ','), -1), element_at(array(1, 2), 5) from t where id = 5` | b,c,NULL | ERR(invalid argument: unexpected char '[' at pos 20) |
| sql_coverage | array_functions | error | `select transform(array(1), x -> x) from t where id = 1` | [1] | ERR(invalid argument: unknown function: transform) |
| sql_coverage | case_insensitive_identifiers_and_null_placement | wrong | `select f from t order by f` | NULL\|-3.5\|0\|1.5\|2.5\|4 | -3.5\|0\|1.5\|2.5\|4\|NULL |
| sql_coverage | case_insensitive_identifiers_and_null_placement | wrong | `select f from t order by f desc` | 4\|2.5\|1.5\|0\|-3.5\|NULL | NULL\|4\|2.5\|1.5\|0\|-3.5 |
| sql_coverage | case_insensitive_identifiers_and_null_placement | error | `SELECT G, COUNT(*) FROM T GROUP BY G ORDER BY G` | NULL,1\|x,2\|y,2\|z,1 | ERR(invalid argument: unknown table: T) |
| sql_coverage | case_insensitive_identifiers_and_null_placement | error | `SELECT G, SUM(V) AS S FROM T GROUP BY G ORDER BY S DESC LIMIT 1` | NULL,60 | ERR(invalid argument: unknown table: T) |
| sql_coverage | case_insensitive_identifiers_and_null_placement | error | `SELECT ID FROM T WHERE ID = 1` | 1 | ERR(invalid argument: unknown table: T) |
| sql_coverage | case_insensitive_identifiers_and_null_placement | error | `SELECT T.ID, U.W FROM T JOIN U ON T.ID = U.ID ORDER BY T.ID, U.W` | 1,p\|2,q\|2,r | ERR(invalid argument: unknown table: T) |
| sql_coverage | case_insensitive_identifiers_and_null_placement | error | `select Id, Upper(G) from t where iD = 3` | 3,Y | ERR(invalid argument: column not found: Id) |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select (select sum(v) from t) as total, count(*) from t where id < 3` | 170,2 | NULL,2 |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select * from (select id, (select max(w) from u where u.id = t.id) mw from t) q where mw is not null order by id` | 1,p\|2,r |  |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select a.id, b.id from t a join t b on a.g = b.g and a.id < b.id order by a.id, b.id` | 1,2\|3,4 | 1,1\|1,2\|2,1\|2,2\|3,3\|3,4\|4,3\|4,4\|5,5\|6,6 |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select coalesce(g, 'none') k, sum(v) from t group by coalesce(g, 'none') having sum(v) > 40 order by k` | none,60\|z,50 | x,170 |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select g, (select max(w) from u where u.id = (select min(id) from t t2 where t2.g = t1.g)) m from t t1 where id < 3 order by id` | x,p\|x,p | x,NULL\|x,NULL |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select g, count(*) from t group by g having count(*) = (select max(c) from (select count(*) c from t group by g) q) order by g` | x,2\|y,2 |  |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select g, max(v) from t group by g having max(v) > (select min(v) from t) + 10 order by g` | NULL,60\|y,30\|z,50 |  |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select g, sum(v) s from t group by g having sum(v) > (select max(x) from nn) order by g` | NULL,60\|x,30\|y,30\|z,50 |  |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select id from t where exists (select 1 from t t2 where t2.g = t.g and t2.id <> t.id) order by id` | 1\|2\|3\|4 | 1\|2\|3\|4\|5\|6 |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select id from t where not exists (select 1 from t t2 where t2.g = t.g and t2.id <> t.id) order by id` | 5\|6 |  |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select id, (select count(*) from t t2 where t2.v < t1.v) as rank_ from t t1 order by id` | 1,0\|2,1\|3,2\|4,0\|5,3\|6,4 | 1,NULL\|2,NULL\|3,NULL\|4,NULL\|5,NULL\|6,NULL |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select id, sum(v) over () as total, v * 1.0 / sum(v) over () as share from t where id < 3 order by id` | 1,30,0.333333\|2,30,0.666667 | 1,30,NULL\|2,30,NULL |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select id, v, v * 100.0 / (select sum(v) from t) as pct from t where id < 3 order by id` | 1,10,5.882353\|2,20,11.764706 | 1,10,NULL\|2,20,NULL |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select length(coalesce(s, '')) l, count(*) from t group by length(coalesce(s, '')) order by l` | 0,1\|3,1\|5,3\|6,1 | 5,6 |
| sql_coverage | correlated_and_nested_subqueries | wrong | `select upper(g) u, count(*) c from t group by upper(g) having count(*) > 1 order by u` | X,2\|Y,2 | X,6 |
| sql_coverage | correlated_and_nested_subqueries | wrong | `with m as (select g, max(v) mv from t group by g) select id from t where v = (select mv from m where m.g = t.g) order by id` | 2\|3\|5 | 2 |
| sql_coverage | correlated_and_nested_subqueries | error | `select count(*) from (select id from t union all select id from u) q where id > 1` | 8 | ERR(invalid argument: expected RParen, got Union) |
| sql_coverage | correlated_and_nested_subqueries | error | `select g, sum(v) s from t group by g having sum(v) >= all (select 30) order by g` | NULL,60\|x,30\|y,30\|z,50 | ERR(invalid argument: unexpected token in expr: All) |
| sql_coverage | correlated_and_nested_subqueries | error | `select id from t where v > all (select v from t t2 where t2.g = 'x') order by id` | 3\|5\|6 | ERR(invalid argument: unexpected token in expr: All) |
| sql_coverage | correlated_and_nested_subqueries | error | `select id from t where v > any (select v from t t2 where t2.g = 'x') order by id` | 2\|3\|5\|6 | ERR(invalid argument: unknown function: any) |
| sql_coverage | correlated_and_nested_subqueries | error | `with m as (select g, max(v) mv from t group by g) select t.id from t join m on t.g = m.g and t.v = m.mv order by t.id` | 2\|3\|5 | ERR(column not found: m.g) |
| sql_coverage | correlated_scalar_subquery_regressions | wrong | `select id from t where g = (select g from t t2 where t2.id = t.id) order by id` | 1\|2\|3\|4\|5 | 1\|2 |
| sql_coverage | correlated_scalar_subquery_regressions | wrong | `select id from t where id = (select id from t t2 where t2.v = t.v) order by id` | 1\|2\|3\|5\|6 | 1 |
| sql_coverage | correlated_scalar_subquery_regressions | wrong | `select id from t where v = (select max(mv) from (select g, max(v) mv from t group by g) m where m.g = t.g) order by id` | 2\|3\|5 |  |
| sql_coverage | correlated_scalar_subquery_regressions | wrong | `select id from t where v = (select mv from (select g, max(v) mv from t group by g) m where m.g = t.g) order by id` | 2\|3\|5 |  |
| sql_coverage | correlated_scalar_subquery_regressions | wrong | `select id from t where v = (select v from t t2 where t2.id = t.id) order by id` | 1\|2\|3\|5\|6 | 1 |
| sql_coverage | correlated_scalar_subquery_regressions | wrong | `select id, (select mv from (select g, max(v) mv from t group by g) m where m.g = t.g) from t where id < 4 order by id` | 1,20\|2,20\|3,30 | 1,NULL\|2,NULL\|3,NULL |
| sql_coverage | correlated_scalar_subquery_regressions | wrong | `with m as (select g, max(v) mv from t group by g) select id from t where v = (select mv from m where m.g = t.g) order by id` | 2\|3\|5 | 2 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select (select count(*) from u) from t where id = 1` | 4 | NULL |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select a.id, b.id from t a join t b on a.id = b.id and a.id < 3 order by a.id` | 1,1\|2,2 | 1,1\|2,2\|3,3\|4,4\|5,5\|6,6 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select count(*) from t a join t b on a.id = b.id + 1` | 5 | 6 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select count(*) from t join nn on t.v = nn.x * 10` | 2 | 0 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select count(*) from t join u on t.id = u.id and u.w = 'q'` | 1 | 3 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select count(*) from t, u` | 24 | 6 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select g from t intersect select g from t` | x\|y\|z\|NULL | x\|x\|y\|y\|z\|NULL\|x\|x\|y\|y\|z\|NULL |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select g from t union select g from t` | x\|y\|z\|NULL | x\|x\|y\|y\|z\|NULL\|x\|x\|y\|y\|z\|NULL |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select g, sum(v) from t group by g having sum(v) > (select avg(v) from t) order by g` | NULL,60\|z,50 |  |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where (select count(*) from u where u.id = t.id) > 1` | 2 |  |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where id < 4 except select id from u` | 3 | 1\|2\|3\|1\|2\|2\|7 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where id < 4 intersect select id from u order by id` | 1\|2 | 1\|2\|3\|1\|2\|2\|7 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where id < 4 minus select id from u` | 3 | 1\|2\|3 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where id < 4 union distinct select id from u order by id` | 1\|2\|3\|7 | 1\|2\|3\|1\|2\|2\|7 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where id < 4 union select id from u order by id` | 1\|2\|3\|7 | 1\|2\|3\|1\|2\|2\|7 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where id not in (select x from nn)` |  | 3\|4\|5\|6 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where v = (select max(v) from t t2 where t2.g = t.g) order by id` | 2\|3\|5 | 2 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from t where v > (select min(v) from t t2 where t2.g = t.g) order by id` | 2 | 2\|3\|5\|6 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from u except all select id from t where id < 4 order by id` | 2\|7 | 1\|2\|2\|7\|1\|2\|3 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from u except select id from t where id < 4` | 7 | 1\|2\|2\|7\|1\|2\|3 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from u intersect all select id from t where id < 4 order by id` | 1\|2 | 1\|2\|2\|7\|1\|2\|3 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id from u union select id from t where id < 3 order by id desc limit 2` | 7\|2 | 1\|2\|2\|7\|2\|1 |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id, (select count(*) from u where u.id = t.id) c from t where id<=3 order by id` | 1,1\|2,2\|3,0 | 1,NULL\|2,NULL\|3,NULL |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id, (select max(w) from u where u.id = t.id) from t where id <= 3 order by id` | 1,p\|2,r\|3,NULL | 1,NULL\|2,NULL\|3,NULL |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select id, id in (select id from u) from t where id <= 3 order by id` | 1,true\|2,true\|3,false | 1,false\|2,false\|3,false |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select t.g, count(u.w) from t left join u on t.id = u.id group by t.g order by t.g` | NULL,0\|x,3\|y,0\|z,0 | NULL\|x\|y\|z |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `select t.id, u.w from t left join u on t.id = u.id and u.w = 'q' where t.id <= 3 order by t.id` | 1,NULL\|2,q\|3,NULL | 1,p\|2,q\|2,r\|3,NULL\|4,NULL\|5,NULL\|6,NULL |
| sql_coverage | ctes_joins_subqueries_set_operations | wrong | `with a as (select id from t where id < 3) select id from a union all select id from u order by id` | 1\|1\|2\|2\|2\|7 | 1\|2\|1\|2\|2\|7 |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `(select id from t where id<3) union all (select id from u)` | 1\|2\|1\|2\|2\|7 | ERR(invalid argument: expected Select, got LParen) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select a.id from (select id from t) a join (select id from u) b on a.id = b.id order by a.id` | 1\|2\|2 | ERR(invalid argument: unknown table: b) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select count(*) from t cross join u` | 24 | ERR(invalid argument: expected On, got Eof) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select count(*) from t join u on t.id < u.id` | 8 | ERR(invalid argument: expected Eq, got Lt) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select count(*) from t join u using (id)` | 3 | ERR(invalid argument: expected On, got LParen) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select count(*) from t natural join u` | 3 | ERR(invalid argument: expected On, got Eof) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select id from t left anti join u on t.id = u.id order by id` | 3\|4\|5\|6 | ERR(invalid argument: expected Join, got Ident("anti")) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select id from t left semi join u on t.id = u.id order by id` | 1\|2 | ERR(invalid argument: expected Join, got Ident("semi")) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select id from t where v > any (select x from nn) order by id` | 1\|2\|3\|5\|6 | ERR(invalid argument: unknown function: any) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select id from t where v >= all (select x from nn where x is not null) order by id` | 1\|2\|3\|5\|6 | ERR(invalid argument: unexpected token in expr: All) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select id, w from t join u using (id) order by id, w` | 1,p\|2,q\|2,r | ERR(invalid argument: expected On, got LParen) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select id, w from u union select id, g from t where id < 3 order by id, w` | 1,p\|1,x\|2,q\|2,r\|2,x\|7,s | ERR(invalid argument: ORDER BY column not found: w) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select q.g, q.c from (select g, count(*) as c from t group by g) q where q.c > 1 order by q.g` | x,2\|y,2 | ERR(invalid argument: column not found: q.g) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select t.id from t semi join u on t.id = u.id order by 1` | 1\|2 | ERR(invalid argument: expected alias name, got Int(1)) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `select t.id, u.w from t right join u on t.id = u.id order by u.w` | 1,p\|2,q\|2,r\|NULL,s | ERR(column not found: u.id) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `with a as (select id from t) select a.id from a where a.id = 1` | 1 | ERR(invalid argument: column not found: a.id) |
| sql_coverage | ctes_joins_subqueries_set_operations | error | `with a as (select id, v from t), b as (select id, w from u) select a.id, b.w from a join b on a.id = b.id order by a.id, b.w` | 1,p\|2,q\|2,r | ERR(column not found: b.id) |
| sql_coverage | dates_casts_math | wrong | `select '5' + 1 from t where id=1` | 6 | NULL |
| sql_coverage | dates_casts_math | wrong | `select 1/0 from t where id=1` | NULL | inf |
| sql_coverage | dates_casts_math | wrong | `select 7/2, 7 div 2, 6/2 from t where id=1` | 3.5,3,3 | 3.5,7 |
| sql_coverage | dates_casts_math | wrong | `select cast('x' as boolean) from t where id=1` | NULL | false |
| sql_coverage | dates_casts_math | wrong | `select cast(123.456 as decimal(10,2)) from t where id=1` | 123.46 | 123.456 |
| sql_coverage | dates_casts_math | wrong | `select cast(2.0 as string) from t where id=1` | 2.0 | 2 |
| sql_coverage | dates_casts_math | wrong | `select cast(true as int) from t where id=1` | 1 | NULL |
| sql_coverage | dates_casts_math | wrong | `select date_add('2024-01-31', 1) from t where id=1` | 2024-02-01 | -001-12-01 |
| sql_coverage | dates_casts_math | wrong | `select date_add(d, 1) from t where id=2` | 2024-03-01 | -001-12-01 |
| sql_coverage | dates_casts_math | wrong | `select dateadd('month', 1, '2024-01-31') from t where id=1` | 2024-02-29 | 2024-03-01 |
| sql_coverage | dates_casts_math | wrong | `select datediff('2024-03-01', null) from t where id=1` | NULL | 0 |
| sql_coverage | dates_casts_math | wrong | `select datediff('2024-03-01','2024-02-01') from t where id=1` | 29 | -739314 |
| sql_coverage | dates_casts_math | wrong | `select datediff(d, '2024-01-01') from t where id=2` | 59 | -739283 |
| sql_coverage | dates_casts_math | wrong | `select id from t where id = '2'` | 2 |  |
| sql_coverage | dates_casts_math | wrong | `select log(10,100) from t where id=1` | 2 | 2.302585 |
| sql_coverage | dates_casts_math | wrong | `select mod(7,0) from t where id=1` | NULL | NaN |
| sql_coverage | dates_casts_math | wrong | `select sum(v) / count(v) from t` | 34 | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | dates_casts_math | wrong | `select year(d) from t where id=4` | NULL | 0 |
| sql_coverage | dates_casts_math | error | `select -v, +v from t where id=1` | -10,10 | ERR(invalid argument: unexpected token in expr: Plus) |
| sql_coverage | dates_casts_math | error | `select add_months('2024-01-31', 1) from t where id=1` | 2024-02-29 | ERR(invalid argument: unknown function: add_months) |
| sql_coverage | dates_casts_math | error | `select atan2(0,1), sinh(0), cosh(0), tanh(0), asin(0), acos(1), atan(0) from t where id=1` | 0,0,1,0,0,0,0 | ERR(invalid argument: unknown function: atan2) |
| sql_coverage | dates_casts_math | error | `select bround(2.5) from t where id=1` | 2 | ERR(invalid argument: unknown function: bround) |
| sql_coverage | dates_casts_math | error | `select cast(v as string) \|\| 'x' from t where id=1` | 10x | ERR(invalid argument: unexpected char '\|' at pos 25) |
| sql_coverage | dates_casts_math | error | `select date '2024-01-31' + interval '1' month from t where id=1` | 2024-02-29 | ERR(invalid argument: column not found: date) |
| sql_coverage | dates_casts_math | error | `select date '2024-01-31' + interval 1 day from t where id=1` | 2024-02-01 | ERR(invalid argument: column not found: date) |
| sql_coverage | dates_casts_math | error | `select date_format('2024-03-05','yyyy/MM/dd') from t where id=1` | 2024/03/05 | ERR(invalid argument: unknown function: date_format) |
| sql_coverage | dates_casts_math | error | `select date_sub('2024-03-01', 1) from t where id=1` | 2024-02-29 | ERR(invalid argument: unknown function: date_sub) |
| sql_coverage | dates_casts_math | error | `select dayofmonth('2024-03-05'), dayofweek('2024-03-05'), dayofyear('2024-03-05'), quarter('2024-03-05') from t where id=1` | 5,3,65,1 | ERR(invalid argument: unknown function: dayofmonth) |
| sql_coverage | dates_casts_math | error | `select factorial(5) from t where id=1` | 120 | ERR(invalid argument: unknown function: factorial) |
| sql_coverage | dates_casts_math | error | `select hour('2024-03-05 13:45:10'), minute('2024-03-05 13:45:10'), second('2024-03-05 13:45:10') from t where id=1` | 13,45,10 | ERR(invalid argument: unknown function: hour) |
| sql_coverage | dates_casts_math | error | `select hypot(3,4) from t where id=1` | 5 | ERR(invalid argument: unknown function: hypot) |
| sql_coverage | dates_casts_math | error | `select last_day('2024-02-10') from t where id=1` | 2024-02-29 | ERR(invalid argument: unknown function: last_day) |
| sql_coverage | dates_casts_math | error | `select make_date(2024,3,5) from t where id=1` | 2024-03-05 | ERR(invalid argument: unknown function: make_date) |
| sql_coverage | dates_casts_math | error | `select mod(7,3), 7 % 3, -7 % 3, pmod(-7,3) from t where id=1` | 1,1,-1,2 | ERR(invalid argument: unknown function: pmod) |
| sql_coverage | dates_casts_math | error | `select months_between('2024-03-31','2024-02-29') from t where id=1` | 1 | ERR(invalid argument: unknown function: months_between) |
| sql_coverage | dates_casts_math | error | `select next_day('2024-03-05','Mon') from t where id=1` | 2024-03-11 | ERR(invalid argument: unknown function: next_day) |
| sql_coverage | dates_casts_math | error | `select shiftleft(1,3), 5 & 3, 5 \| 3, 5 ^ 3 from t where id=1` | 8,1,7,6 | ERR(invalid argument: unexpected char '&' at pos 25) |
| sql_coverage | dates_casts_math | error | `select try_cast('abc' as int) from t where id=1` | NULL | ERR(invalid argument: unknown function: try_cast) |
| sql_coverage | dates_casts_math | error | `select unix_timestamp('1970-01-02') from t where id=1` | 86400 | ERR(invalid argument: unknown function: unix_timestamp) |
| sql_coverage | dates_casts_math | error | `select weekofyear('2024-03-05') from t where id=1` | 10 | ERR(invalid argument: unknown function: weekofyear) |
| sql_coverage | dates_casts_math | error | `select width_bucket(5, 0, 10, 5) from t where id=1` | 3 | ERR(invalid argument: unknown function: width_bucket) |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select (select count(*) from e), (select max(a) from e) from t where id = 1` | 0,NULL | NULL,NULL |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select b, count(*) from e group by grouping sets ((b), ())` | NULL,0 |  |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select count(*) from e right join t on t.id = e.a` | 6 | 0 |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select count(1), count('a'), count(null) from t` | 6,6,0 | 6,6,6 |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select id, count(*) over (order by id rows between 1 following and 2 following) from t order by id` | 1,2\|2,2\|3,2\|4,2\|5,1\|6,0 | 1,6\|2,6\|3,6\|4,6\|5,6\|6,6 |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select id, first_value(v) over (order by id rows between 5 following and 6 following) from t where id < 3 order by id` | 1,NULL\|2,NULL | 1,10\|2,10 |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select id, ntile(10) over (order by id) from t where id < 4 order by id` | 1,1\|2,2\|3,3 | 1,1\|2,4\|3,7 |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select id, percent_rank() over (partition by g order by id) from t order by id` | 1,0\|2,1\|3,0\|4,1\|5,0\|6,0 | 1,1\|2,2\|3,1\|4,2\|5,1\|6,1 |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select id, sum(id) over (order by id desc) from t where id < 4 order by id` | 1,6\|2,5\|3,3 | 1,6\|2,6\|3,6 |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select id, sum(v) over (order by id range between 1 following and 2 following) from t where id < 4 order by id` | 1,50\|2,30\|3,NULL | 1,60\|2,60\|3,60 |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select max(id) - min(id) + 1 from t` | 6 | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | empty_inputs_and_window_frames | wrong | `select sum(1), sum(1.5), sum(true) from t where id < 3` | 2,3,2 | 2,2,NULL |
| sql_coverage | empty_inputs_and_window_frames | error | `select a from e union select id from u order by 1` | 1\|2\|7 | ERR(invalid argument: expected alias name, got Int(1)) |
| sql_coverage | empty_inputs_and_window_frames | error | `select count(distinct b), count(distinct a, b) from e` | 0,0 | ERR(invalid argument: expected RParen, got Comma) |
| sql_coverage | empty_inputs_and_window_frames | error | `select id from t where v > all (select a from e) order by id` | 1\|2\|3\|4\|5\|6 | ERR(invalid argument: unexpected token in expr: All) |
| sql_coverage | empty_inputs_and_window_frames | error | `select id from t where v > any (select a from e)` |  | ERR(invalid argument: unknown function: any) |
| sql_coverage | empty_inputs_and_window_frames | error | `select id, lag(v, 10, 0) over (order by id) from t where id < 3 order by id` | 1,0\|2,0 | ERR(invalid argument: expected RParen, got Comma) |
| sql_coverage | empty_inputs_and_window_frames | error | `select id, nth_value(v, 10) over (order by id) from t where id < 3 order by id` | 1,NULL\|2,NULL | ERR(invalid argument: unknown function: nth_value) |
| sql_coverage | empty_inputs_and_window_frames | error | `select stddev(a), variance(c), median(a), percentile(a, 0.5), collect_list(a), string_agg(b, ',') from e` | NULL,NULL,NULL,NULL,[],NULL | ERR(invalid argument: unknown function: percentile) |
| sql_coverage | empty_inputs_and_window_frames | accepts | `select count(distinct g) over () from t` | ERR | 5\|5\|5\|5\|5\|5 |
| sql_coverage | empty_string_is_not_null | error | `select count(distinct s) from (values ('a'), (''), (null), ('')) q(s)` | 2 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | empty_string_is_not_null | error | `select s, count(*) from (values ('a'), (''), (null), ('')) q(s) group by s order by s` | NULL,1\|,2\|a,1 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | empty_string_is_not_null | error | `select s, count(*), count(s) from (values ('a'), (''), (null), ('')) q(s) group by s order by s` | NULL,1,0\|,2,2\|a,1,1 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | empty_string_is_not_null | error | `select s, sum(n) from (values ('a', 1), ('', 2), (null, 3), ('', 4)) q(s, n) group by s order by s` | NULL,3\|,6\|a,1 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select -sum(v) from t` | -170 | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select 1 in (1, null), 3 in (1, null), 3 not in (1, null) from t where id = 1` | true,NULL,NULL | true,false,true |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select 7 % 0 from t where id = 1` | NULL | NaN |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select cast(sum(v) as string) from t` | 170 | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select coalesce(sum(v), 0) from t where id > 100` | 0 |  |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select count(*) from t, u where t.id = u.id` | 3 | 6 |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select count(distinct u.w) from t left join u on t.id = u.id` | 3 |  |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select count(u.w) from t left join u on t.id = u.id` | 3 |  |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select g from t group by g having count(*) > 1 order by g` | x\|y |  |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select g, count(*) / 2.0 from t group by g order by g` | NULL,0.5\|x,1\|y,1\|z,0.5 | NULL,NULL\|x,NULL\|y,NULL\|z,NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select g, round(avg(v), 1) from t group by g order by g` | NULL,60\|x,15\|y,30\|z,50 | NULL,NULL\|x,NULL\|y,NULL\|z,NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select g, sum(v) as s from t group by g having sum(v) > (select avg(v) from t) order by g` | NULL,60\|z,50 |  |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select id from t where (select count(*) from u where u.id = t.id) = 0 order by id` | 3\|4\|5\|6 |  |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select id from t where id not in (1,2,null)` |  | 3\|4\|5\|6 |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select id, (select count(*) from u where u.id = t.id) from t where id <= 2 order by id` | 1,1\|2,2 | 1,NULL\|2,NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select id, exists (select 1 from u where u.id = t.id) from t where id <= 3 order by id` | 1,true\|2,true\|3,false | 1,false\|2,false\|3,false |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select id, id not in (1,2,null) from t where id < 4 order by id` | 1,false\|2,false\|3,NULL | 1,false\|2,false\|3,true |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select max(s) from t` | abc | NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select max(v) - min(v) from t` | 50 | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select min(d), max(d) from t` | 2023-12-31,2024-03-01 | NULL,NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select min(g) from t` | x | NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select sum(v) * 2 from t` | 340 | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select sum(v) / count(*) from t` | 28.333333 | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select sum(v) filter (where v > 15) from t` | 160 | NULL |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select t.g, count(u.w) from t left join u on t.id = u.id group by t.g order by t.g` | NULL,0\|x,3\|y,0\|z,0 | NULL\|x\|y\|z |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select t.g, max(u.w) from t left join u on t.id = u.id group by t.g order by t.g` | NULL,NULL\|x,r\|y,NULL\|z,NULL | NULL\|x\|y\|z |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select t.g, min(t.s) from t group by t.g order by t.g` | NULL,World\|x,  pad \|y,abc\|z,a,b,c | NULL\|x\|y\|z |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select t.id from t join u on t.id = u.id group by t.id having count(*) > 1` | 2 |  |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select t.id, count(u.id) from t join u on t.id = u.id group by t.id order by t.id` | 1,1\|2,2 | 1\|2 |
| sql_coverage | grouped_queries_over_joins_and_expressions | wrong | `select upper(g) as k, count(*) from t group by upper(g) order by k` | NULL,1\|X,2\|Y,2\|Z,1 | X,6 |
| sql_coverage | grouped_queries_over_joins_and_expressions | error | `select a.g, b.cnt from (select distinct g from t) a join (select g, count(*) cnt from t group by g) b on a.g = b.g order by a.g` | x,2\|y,2\|z,1 | ERR(invalid argument: unknown table: b) |
| sql_coverage | grouped_queries_over_joins_and_expressions | error | `select case when v > 25 then 'hi' else 'lo' end as b, count(*) from t group by 1 order by b` | hi,3\|lo,3 | ERR(invalid argument: expected alias name, got Int(1)) |
| sql_coverage | grouped_queries_over_joins_and_expressions | error | `select case when v > 25 then 'hi' else 'lo' end as b, count(*) from t group by case when v > 25 then 'hi' else 'lo' end order by b` | hi,3\|lo,3 | ERR(invalid argument: expected alias name, got Case) |
| sql_coverage | grouped_queries_over_joins_and_expressions | error | `select count(distinct g, v) from t` | 4 | ERR(invalid argument: expected RParen, got Comma) |
| sql_coverage | grouped_queries_over_joins_and_expressions | error | `select g, count(*) from t group by 1` | x,2\|y,2\|z,1\|NULL,1 | ERR(invalid argument: expected alias name, got Int(1)) |
| sql_coverage | grouped_queries_over_joins_and_expressions | error | `select g, count(*) from t group by g order by 2 desc, 1` | x,2\|y,2\|NULL,1\|z,1 | ERR(invalid argument: expected alias name, got Int(2)) |
| sql_coverage | grouped_queries_over_joins_and_expressions | error | `select null = null, null <=> null, 1 <=> 2 from t where id = 1` | NULL,true,false | ERR(invalid argument: unexpected token in expr: Gt) |
| sql_coverage | grouped_queries_over_joins_and_expressions | error | `select sum(distinct v) from t` | 170 | ERR(invalid argument: unexpected token in expr: Distinct) |
| sql_coverage | int_precision | error | `select a.n, b.n from (values (9007199254740993), (9007199254740992)) a(n) join (values (9007199254740992)) b(n) on a.n = b.n` | 9007199254740992,9007199254740992 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select avg(n) from (values (9007199254740993), (9007199254740993)) q(n)` | 9007199254740992 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select cast(sum(n) as string) from (values (4611686018427387904), (4611686018427387904)) q(n)` | -9223372036854775808 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select cast(sum(n) as string), cast(min(n) as string), cast(max(n) as string) from (values (9007199254740993), (1), (9007199254740995)) q(n)` | 18014398509481989,1,9007199254740995 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select count(distinct n) from (values (9007199254740993), (9007199254740992)) q(n)` | 2 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select n + 1, n * 2 from (values (9007199254740993)) q(n)` | 9007199254740994,18014398509481986 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select n = 9007199254740992, n > 9007199254740992 from (values (9007199254740993)) q(n)` | false,true | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select n from (values (9007199254740993), (9007199254740992)) q(n) order by n desc` | 9007199254740993\|9007199254740992 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select n, count(*) from (values (9007199254740993), (9007199254740992), (9007199254740993)) q(n) group by n order by n` | 9007199254740992,1\|9007199254740993,2 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | int_precision | error | `select sum(n) over (order by n rows unbounded preceding) s from (values (9007199254740993), (1)) q(n) order by s` | 1\|9007199254740994 | ERR(invalid argument: unexpected token in expr: RParen) |
| sql_coverage | integer_aggregates_stay_integers | wrong | `select cast(count(distinct g) as string) from t` | 3 | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | integer_aggregates_stay_integers | wrong | `select cast(sum(f) as string), cast(avg(v) as string) from t` | 4.5,34.0 | NULL,NULL\|NULL,NULL\|NULL,NULL\|NULL,NULL\|NULL,NULL\|NULL,NULL |
| sql_coverage | integer_aggregates_stay_integers | wrong | `select cast(sum(v) as string) from t where id > 100` | NULL |  |
| sql_coverage | integer_aggregates_stay_integers | wrong | `select cast(sum(v) as string), cast(count(*) as string), cast(min(v) as string), cast(max(id) as string) from t` | 170,6,10,6 | NULL,NULL,NULL,NULL\|NULL,NULL,NULL,NULL\|NULL,NULL,NULL,NULL\|NULL,NULL,NULL,NULL\|NULL,NULL,NULL,NULL\|NULL,NULL,NULL,NULL |
| sql_coverage | integer_aggregates_stay_integers | wrong | `select g, cast(sum(v) as string), cast(count(*) as string) from t group by g order by g` | NULL,60,1\|x,30,2\|y,30,2\|z,50,1 | NULL,NULL,NULL\|x,NULL,NULL\|y,NULL,NULL\|z,NULL,NULL |
| sql_coverage | lateral_aliases_and_unsupported_features | wrong | `select cast('2024-03-05 10:20:30' as timestamp) - interval 1 day from t where id = 1` | 2024-03-04 10:20:30 | NULL |
| sql_coverage | lateral_aliases_and_unsupported_features | wrong | `select cast(v as decimal(18, 2)) from t where id = 1` | 10.00 | 10 |
| sql_coverage | lateral_aliases_and_unsupported_features | wrong | `select datediff(current_date, '2020-01-01') > 0, current_date() > '2020-01-01' from t where id = 1` | true,true | false,true |
| sql_coverage | lateral_aliases_and_unsupported_features | wrong | `select g, count(*) as n, n * 2 as dbl from t group by g order by g` | NULL,1,2\|x,2,4\|y,2,4\|z,1,2 | NULL,1,NULL\|x,2,NULL\|y,2,NULL\|z,1,NULL |
| sql_coverage | lateral_aliases_and_unsupported_features | wrong | `select g, sum(v) over (partition by g order by id rows between unbounded preceding and current row) - v from t where g = 'x' order by id` | x,0\|x,10 | x,NULL\|x,NULL |
| sql_coverage | lateral_aliases_and_unsupported_features | wrong | `select id + 1 as x, x * 2 as y from t where id < 3 order by id` | 2,4\|3,6 | 2,NULL\|3,NULL |
| sql_coverage | lateral_aliases_and_unsupported_features | wrong | `select id, (select 1) as one, one + 1 as two from t where id = 1` | 1,1,2 | 1,NULL,NULL |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `` select 1 as `my col`, 2 as "other col" from t where id = 1 `` | 1,2 | ERR(invalid argument: unexpected char '`' at pos 12) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select aggregate(array(1, 2), 0, (a, b) -> a + b) from t where id = 1` | 3 | ERR(invalid argument: unknown function: aggregate) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select array_join(collect_set(g), '\|') from t where g is not null` | x\|y\|z | ERR(invalid argument: unknown function: array_join) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select assert_true(1 = 1) from t where id = 1` | true | ERR(invalid argument: unknown function: assert_true) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select concat_ws(',', collect_list(g)) from t where g is not null` | x,x,y,y,z | ERR(invalid argument: unknown function: concat_ws) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select concat_ws(',', collect_list(g)) from t where id > 100` |  | ERR(invalid argument: unknown function: concat_ws) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select current_timezone() from t where id = 1` | UTC | ERR(invalid argument: unknown function: current_timezone) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select explode(split('a,b', ',')) from t where id = 1` | a\|b | ERR(invalid argument: unknown function: explode) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select from_json('{"a":1}', 'a int') from t where id = 1` | {1} | ERR(invalid argument: unknown function: from_json) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select g, concat_ws(';', collect_list(s)) from t group by g order by g` | NULL,World\|x,Hello;  pad \|y,abc\|z,a,b,c | ERR(invalid argument: unknown function: concat_ws) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select g, grouping(g) gg, sum(v) from t group by rollup(g) order by grouping(g), g` | NULL,0,60\|x,0,30\|y,0,30\|z,0,50\|NULL,1,170 | ERR(invalid argument: unknown function: grouping) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select hash('a') from t where id = 1` | 1485273170 | ERR(invalid argument: unknown function: hash) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select id from t where id = 1 /* inline */` | 1 | ERR(invalid argument: unexpected token in expr: Star) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select lower(trim(both ' ' from '  Ab  ')) from t where id = 1` | ab | ERR(invalid argument: expected RParen, got Str(" ")) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select map_keys(map('a', 1)) from t where id = 1` | [a] | ERR(invalid argument: unknown function: map_keys) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select named_struct('a', 1) from t where id = 1` | {1} | ERR(invalid argument: unknown function: named_struct) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select posexplode(split('a,b', ',')) from t where id = 1` | 0,a\|1,b | ERR(invalid argument: unknown function: posexplode) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select regexp_extract_all('a1b2', '(\\d)', 1) from t where id = 1` | [1, 2] | ERR(invalid argument: unknown function: regexp_extract_all) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select stack(2, 1, 2) from t where id = 1` | 1\|2 | ERR(invalid argument: unknown function: stack) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select to_json(named_struct('a', 1)) from t where id = 1` | {"a":1} | ERR(invalid argument: unknown function: to_json) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select transform(array(1, 2), x -> x + 1) from t where id = 1` | [2, 3] | ERR(invalid argument: unknown function: transform) |
| sql_coverage | lateral_aliases_and_unsupported_features | error | `select xxhash64('a') from t where id = 1` | -8582455328737087284 | ERR(invalid argument: unknown function: xxhash64) |
| sql_coverage | lateral_aliases_and_unsupported_features | accepts | `select max(v) keep (dense_rank first order by id) from t` | ERR | NULL |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | wrong | `select 'a%c' like 'a#%c' escape '#', 'abc' like 'a#%c' escape '#', 'a_c' like 'a!_c' escape '!' from t where id = 1` | true,false,true | false |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | wrong | `select id, sum(v) over (order by id range between current row and 1 following) from t where id < 4 order by id` | 1,30\|2,50\|3,30 | 1,60\|2,60\|3,60 |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select conv('ff', 16, 10), conv('10', 10, 2), bin(5), hex(255), unbase64('SGVsbG8=') from t where id = 1` | 255,1010,101,FF,Hello | ERR(invalid argument: unknown function: conv) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select d, count(*) over (order by d desc range between interval 14 days preceding and current row) from t where d is not null order by d` | 2023-12-31,2\|2024-01-01,2\|2024-01-15,1\|2024-02-29,2\|2024-03-01,1 | ERR(invalid argument: expected RParen, got Ident("days")) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select d, count(*) over (order by d range between interval 1 month preceding and interval 1 day following) from t where d is not null order by d` | 2023-12-31,2\|2024-01-01,2\|2024-01-15,3\|2024-02-29,2\|2024-03-01,2 | ERR(invalid argument: expected RParen, got Ident("month")) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select d, sum(v) over (order by d range between interval 30 days preceding and current row) from t where d is not null order by d` | 2023-12-31,30\|2024-01-01,90\|2024-01-15,100\|2024-02-29,20\|2024-03-01,70 | ERR(invalid argument: expected RParen, got Ident("days")) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select d2 from t lateral view explode(sequence(to_date('2024-01-30'), to_date('2024-02-02'))) q as d2 where id = 1` | 2024-01-30\|2024-01-31\|2024-02-01\|2024-02-02 | ERR(invalid argument: column not found: d2) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select d2 from t lateral view explode(sequence(to_date('2024-01-31'), to_date('2024-04-30'), interval 1 month)) q as d2 where id = 1` | 2024-01-31\|2024-02-29\|2024-03-31\|2024-04-30 | ERR(invalid argument: column not found: d2) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select decode(g, null, 'none', 'x', 'ex', 'other') from t order by id` | ex\|ex\|other\|other\|other\|none | ERR(invalid argument: unknown function: decode) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select decode(id, 1, 'one', 2, 'two', 'many') from t where id < 4 order by id` | one\|two\|many | ERR(invalid argument: unknown function: decode) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select e from t lateral view explode(sequence(1, 4)) q as e where id = 1` | 1\|2\|3\|4 | ERR(invalid argument: column not found: e) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select e from t lateral view explode(sequence(5, 1, -2)) q as e where id = 1 order by e desc` | 5\|3\|1 | ERR(invalid argument: column not found: e) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select elt(2, 'a', 'b', 'c'), elt(5, 'a'), field('b', 'a', 'b', 'c'), find_in_set('c', 'a,b,c') from t where id = 1` | b,NULL,2,3 | ERR(invalid argument: unknown function: elt) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select get_json_object('not json', '$.a'), get_json_object('{"k y":5}', '$[''k y'']') from t where id = 1` | NULL,5 | ERR(invalid argument: unknown function: get_json_object) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select get_json_object('{"a":{"b":[1,{"c":"x"}]},"n":null}', '$.a.b[1].c'), get_json_object('{"a":1}', '$.a'), get_json_object('{"a":1}', '$.z'), get_json_object('{"a":[1,2]}', '$.a') from t where id = 1` | x,1,NULL,[1,2] | ERR(invalid argument: unknown function: get_json_object) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select regexp_substr('abc123', '[0-9]+'), regexp_substr('abc', '[0-9]+') from t where id = 1` | 123,NULL | ERR(invalid argument: unknown function: regexp_substr) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select size(sequence(1, 5)) from t where id = 1` | 5 | ERR(invalid argument: unknown function: size) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | error | `select timestamp_seconds(86400), unix_date('1970-01-10'), date_from_unix_date(1) from t where id = 1` | 1970-01-02 00:00:00,9,1970-01-02 | ERR(invalid argument: unknown function: timestamp_seconds) |
| sql_coverage | like_escape_decimal_literals_json_sequence_interval_frames | accepts | `select id from t where s rlike '(?<=a)b'` | ERR |  |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select 'abc' like 'a%', 'abc' like '%c', 'abc' like 'a_c', 'abc' like 'A%', 'a%c' like 'a\%c' from t where id = 1` | true,true,true,false,true | true,true,true,false,false |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select 'it''s', 'a\tb', 'x\\y' from t where id = 1` | it's,a | b,x\y |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select 7 / 2, cast(7 as int) / 2, 7 div 2 from t where id = 1` | 3.5,3.5,3 | 3.5,3.5,7 |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select 9007199254740993 + 1 from t where id = 1` | 9007199254740994 | 9007199254740992 |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select 9007199254740993 = 9007199254740992, 9007199254740993 > 9007199254740992 from t where id = 1` | false,true | true,false |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select id from t where g not like 'x%' order by id` | 3\|4\|5 | 3\|4\|5\|6 |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select id from t where id not between 2 and 5 order by id` | 1\|6 | 1\|2\|3\|4\|5\|6 |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select id from t where not id = 3 and id < 5 order by id` | 1\|2\|4 |  |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select split_part('a-b-c', '-', -1) from t where id = 1` | c | a |
| sql_coverage | literals_precedence_and_big_integers | wrong | `select substring('hello', 2, 3), substring('hello', -2), substring('hello', 10) from t where id = 1` | ell,lo, | ell,hello, |
| sql_coverage | literals_precedence_and_big_integers | error | `select - - id from t where id = 3` | 3 | ERR(invalid argument: unexpected token in expr: Minus) |
| sql_coverage | literals_precedence_and_big_integers | error | `select ascii('a'), chr(97), hex(255), hex('Ab') from t where id = 1` | 97,a,FF,4162 | ERR(invalid argument: unknown function: hex) |
| sql_coverage | literals_precedence_and_big_integers | error | `select concat_ws('-', g, cast(id as string)) from t where id < 3 order by id` | x-1\|x-2 | ERR(invalid argument: unknown function: concat_ws) |
| sql_coverage | literals_precedence_and_big_integers | error | `select concat_ws('-', null, 'a', null, 'b') from t where id = 1` | a-b | ERR(invalid argument: unknown function: concat_ws) |
| sql_coverage | literals_precedence_and_big_integers | error | `select id from t where (g = 'x') is false order by id` | 3\|4\|5 | ERR(invalid argument: expected Null, got Ident("false")) |
| sql_coverage | literals_precedence_and_big_integers | error | `select id from t where (g = 'x') is not true order by id` | 3\|4\|5\|6 | ERR(invalid argument: expected Null, got Ident("true")) |
| sql_coverage | literals_precedence_and_big_integers | error | `select id from t where (g = 'x') is true order by id` | 1\|2 | ERR(invalid argument: expected Null, got Ident("true")) |
| sql_coverage | literals_precedence_and_big_integers | error | `select instr('hello', 'l'), locate('l', 'hello', 4), position('x' in 'hello') from t where id = 1` | 3,4,0 | ERR(invalid argument: unknown function: position) |
| sql_coverage | literals_precedence_and_big_integers | error | `select regexp_extract('abc123def', '(\\d+)', 1) from t where id = 1` | 123 | ERR(invalid argument: unknown function: regexp_extract) |
| sql_coverage | literals_precedence_and_big_integers | error | `select regexp_replace('a1b22c', '\\d+', '#') from t where id = 1` | a#b#c | ERR(invalid argument: unknown function: regexp_replace) |
| sql_coverage | literals_precedence_and_big_integers | error | `select upper(substr(s, 1, 1)) \|\| lower(substr(s, 2)) from t where id = 1` | Hello | ERR(invalid argument: unexpected char '\|' at pos 30) |
| sql_coverage | malformed_input_is_an_error_not_a_panic | error | `select * from (values (1, 2), (3, 4)) q(a, b) where a > 1` | 3,4 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select 1 < 2 and 2 < 3, not true, true or false from t where id=1` | true,false,true | true,false,NULL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select 3 = 3.0, '3' = 3, 3 = '3.0' from t where id=1` | true,true,true | true,NULL,NULL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select cast('2024-03-05 10:20:30' as timestamp) + interval 1 hour from t where id=1` | 2024-03-05 11:20:30 | NULL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select cast(1.5 as int), cast(-1.5 as int), cast('2.9' as int) from t where id=1` | 1,-1,2 | 1,-1,NULL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select cast(3 as decimal(5,2)) from t where id=1` | 3.00 | 3 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select concat(1, 2), concat('a', 1.5) from t where id=1` | 12,a1.5 | Int(1)Int(2),aFloat(1.5) |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select current_date() = current_date from t where id=1` | true | NULL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select d, date_add(d, 30) from t where id = 1` | 2024-01-15,2024-02-14 | 2024-01-15,-001-12-30 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select date_add('2024-03-01', -1) from t where id=1` | 2024-02-29 | -001-11-29 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select datediff('2024-03-05 23:59:59', '2024-03-01 00:00:00') from t where id=1` | 4 | -739343 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select greatest(1, null, 3), least('b', 'a') from t where id=1` | 3,a | 3,b |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select id from t order by v - 5 desc limit 1` | 6 | 4\|1\|2\|3\|5\|6 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select id from t where v not in (select id from u) order by id` | 1\|2\|3\|5\|6 | 1\|2\|3\|4\|5\|6 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select length(current_timestamp()) from t where id=1` | 19 | 10 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select ltrim('Sp', 'SSparkSQL') from t where id=1` | arkSQL | Sp |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select rtrim('SQL', 'SparkSQL') from t where id=1` | Spark | SQL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select s from t where s rlike '^[A-Z]' order by id` | Hello\|World |  |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select sum(v) / 0 from t` | NULL | NULL\|NULL\|NULL\|NULL\|NULL\|NULL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select t.id, u.id from t full join u on t.id = u.id and t.v > 100 order by t.id, u.id` | NULL,1\|NULL,2\|NULL,2\|NULL,7\|1,NULL\|2,NULL\|3,NULL\|4,NULL\|5,NULL\|6,NULL | 1,1\|2,2\|2,2\|3,NULL\|4,NULL\|5,NULL\|6,NULL\|NULL,7 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select t.id, u.w from t left join u on t.id = u.id and t.v > 15 order by t.id, u.w` | 1,NULL\|2,q\|2,r\|3,NULL\|4,NULL\|5,NULL\|6,NULL | 1,p\|2,q\|2,r\|3,NULL\|4,NULL\|5,NULL\|6,NULL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select to_date('03/05/2024', 'MM/dd/yyyy') from t where id=1` | 2024-03-05 | 03/05/2024 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `select trunc('2024-03-15', 'MM') from t where id=1` | 2024-03-01 | NULL |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `with c as (select id from t where id < 3) select count(*) from c c1, c c2` | 4 | 2 |
| sql_coverage | misc_functions_dates_and_subqueries | wrong | `with c as (select id from t where id < 3), d as (select id from c where id > 1) select * from d union all select * from c order by id` | 1\|2\|2 | 2\|1\|2 |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select 'a' \|\| 1 \|\| 2.0 from t where id=1` | a12.0 | ERR(invalid argument: unexpected char '\|' at pos 11) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select * from (select id from t union all select id from u) q order by id limit 3` | 1\|1\|2 | ERR(invalid argument: expected RParen, got Union) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select array_contains(split(s, ','), 'b') from t where id=5` | true | ERR(invalid argument: unknown function: array_contains) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select btrim('xxhixx', 'x') from t where id=1` | hi | ERR(invalid argument: unknown function: btrim) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select count(*) from (select id from t union select id from u) q` | 7 | ERR(invalid argument: expected RParen, got Union) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select d from t where d > date_sub(current_date(), 100000) order by d limit 1` | 2023-12-31 | ERR(invalid argument: unknown function: date_sub) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select date '2024-03-05' - interval 1 month from t where id=1` | 2024-02-05 | ERR(invalid argument: column not found: date) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select date'2024-03-05' + interval '2' day from t where id=1` | 2024-03-07 | ERR(invalid argument: column not found: date) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select date_format('2024-03-05 13:45:10', 'yyyy-MM-dd HH:mm') from t where id=1` | 2024-03-05 13:45 | ERR(invalid argument: unknown function: date_format) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select element_at(split(s, ','), 2) from t where id=5` | b | ERR(invalid argument: unknown function: element_at) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select extract(year from date '2024-03-05') from t where id=1` | 2024 | ERR(invalid argument: expected RParen, got Str("2024-03-05")) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select g, count(*) from t group by all` | x,2\|y,2\|z,1\|NULL,1 | ERR(invalid argument: expected alias name, got All) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select id as x from t order by -x limit 1` | 6 | ERR(invalid argument: expected alias name, got Minus) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select id as x from t order by x desc limit 1` | 6 | ERR(invalid argument: ORDER BY column not found: x) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select id from t order by 1 desc limit 2` | 6\|5 | ERR(invalid argument: expected alias name, got Int(1)) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select id from t order by case when g = 'x' then 0 else 1 end, id desc` | 2\|1\|6\|5\|4\|3 | ERR(invalid argument: expected alias name, got Case) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select map('a', 1) from t where id=1` | {a -> 1} | ERR(invalid argument: unknown function: map) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select mode(g) from t` | x | ERR(invalid argument: unknown function: mode) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select nvl2(null, 1, 2), nvl2(0, 1, 2) from t where id=1` | 2,1 | ERR(invalid argument: unknown function: nvl2) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select pmod(10, 3), pmod(-10, 3), mod(-10, 3) from t where id=1` | 1,2,-1 | ERR(invalid argument: unknown function: pmod) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select regexp_replace(s, '(\\w+),(\\w+)', '$2-$1') from t where id = 5` | b-a,c | ERR(invalid argument: unknown function: regexp_replace) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select size(split(s, ',')) from t where id=5` | 3 | ERR(invalid argument: unknown function: size) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select t.id, u.w from t join u on lower(cast(t.id as string)) = lower(cast(u.id as string)) order by t.id, u.w` | 1,p\|2,q\|2,r | ERR(invalid argument: expected Eq, got LParen) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `select timestampdiff(day, '2024-03-01', '2024-03-05') from t where id=1` | 4 | ERR(invalid argument: unknown function: timestampdiff) |
| sql_coverage | misc_functions_dates_and_subqueries | error | `with c(a, b) as (select id, v from t where id < 3) select a, b from c order by a` | 1,10\|2,20 | ERR(invalid argument: expected As, got LParen) |
| sql_coverage | misc_functions_dates_and_subqueries | accepts | `select id from t where id = (select id from u)` | ERR | 1 |
| sql_coverage | misc_functions_dates_and_subqueries | accepts | `select id, (select w from u where u.id = t.id) from t where id = 2` | ERR | 2,NULL |
| sql_coverage | null_group_keys_of_every_type | error | `select a, b, count(distinct n) from (values (1, null, 5), (1, '', 6), (1, null, 7)) q(a, b, n) group by a, b order by b` | 1,NULL,2\|1,,1 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | null_group_keys_of_every_type | error | `select b, count(distinct n) from (values (true, 1), (null, 2), (false, 3), (true, 4)) q(b, n) group by b order by b` | NULL,1\|false,1\|true,2 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | null_group_keys_of_every_type | error | `select c, count(*), sum(n) from (values (0.0, 1), (null, 2), (0.0, 3), (null, 4)) q(c, n) group by c order by c` | NULL,2,6\|0,2,4 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | null_group_keys_of_every_type | error | `select c, count(distinct n) from (values (0.0, 1), (null, 2), (0.0, 3)) q(c, n) group by c order by c` | NULL,1\|0,2 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | null_group_keys_of_every_type | error | `select i, count(distinct n) from (values (0, 1), (null, 2), (0, 3)) q(i, n) group by i order by i` | NULL,1\|0,2 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | null_group_keys_of_every_type | error | `select s, max(n) from (values ('', 1), (null, 2), ('', 3)) q(s, n) group by s order by s` | NULL,2\|,3 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select (v > 5) and false from t where id=4` | false | NULL |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select (v > 5) or true from t where id=4` | true | NULL |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select count(*) filter (where v > 15), sum(v) filter (where g = 'x') from t` | 4,30 | 1 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select count(*) from t having count(*) > 3` | 6 |  |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select count(*) from t where not (g = 'x' and v > 15)` | 4 | 3 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select count(*) from t where not v > 15` | 1 | 0 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select count(*) from t where s not like 'a%'` | 3 | 4 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select count(*) from t where v not between 15 and 55` | 2 | 5 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select count(*) from t where v not in (10, 20)` | 3 | 4 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select count(*) from t where v not in (10, null)` | 0 | 5 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select g, max(v) - min(v) from t group by g order by g` | NULL,0\|x,10\|y,0\|z,0 | NULL,NULL\|x,NULL\|y,NULL\|z,NULL |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select g, sum(v) from t where v is not null group by g having count(*) >= 1 order by g` | NULL,60\|x,30\|y,30\|z,50 |  |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select id from t where g not in ('x') order by id` | 3\|4\|5 | 3\|4\|5\|6 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select not (v > 5) from t where id=4` | NULL | false |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select stddev(v), stddev_samp(v), stddev_pop(v) from t` | 20.736441,20.736441,18.547237 | 20.736441,20.736441,20.736441 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select string_agg(g, ',') from t where id < 3` | x,x |  |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select sum(v) + 1, sum(v + 1) from t` | 171,175 | NULL,175 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select upper(g), count(*) from t group by upper(g) order by 1` | NULL,1\|X,2\|Y,2\|Z,1 | NULL,6 |
| sql_coverage | null_logic_ordering_aggregates | wrong | `select variance(v), var_samp(v), var_pop(v) from t` | 430,430,344 | 430,430,430 |
| sql_coverage | null_logic_ordering_aggregates | error | `select any_value(g) from t where id=2` | x | ERR(invalid argument: unknown function: any_value) |
| sql_coverage | null_logic_ordering_aggregates | error | `select approx_count_distinct(g) from t` | 3 | ERR(invalid argument: unknown function: approx_count_distinct) |
| sql_coverage | null_logic_ordering_aggregates | error | `select avg(distinct v) from t` | 34 | ERR(invalid argument: unexpected token in expr: Distinct) |
| sql_coverage | null_logic_ordering_aggregates | error | `select bool_and(v > 5), bool_or(v > 55) from t` | true,true | ERR(invalid argument: unknown function: bool_and) |
| sql_coverage | null_logic_ordering_aggregates | error | `select count(*) from t group by g having count(*) > 1 order by 1` | 2\|2 | ERR(invalid argument: expected alias name, got Int(1)) |
| sql_coverage | null_logic_ordering_aggregates | error | `select count(*) from t where v <=> null` | 1 | ERR(invalid argument: unexpected token in expr: Gt) |
| sql_coverage | null_logic_ordering_aggregates | error | `select count(*) from t where v is distinct from 10` | 5 | ERR(invalid argument: expected Null, got Distinct) |
| sql_coverage | null_logic_ordering_aggregates | error | `select count(*) from t where v is not distinct from null` | 1 | ERR(invalid argument: expected Null, got Distinct) |
| sql_coverage | null_logic_ordering_aggregates | error | `select count(distinct g, v) from t` | 4 | ERR(invalid argument: expected RParen, got Comma) |
| sql_coverage | null_logic_ordering_aggregates | error | `select count_if(v > 15) from t` | 4 | ERR(invalid argument: unknown function: count_if) |
| sql_coverage | null_logic_ordering_aggregates | error | `select first(g), last(id) from t` | x,6 | ERR(invalid argument: unknown function: first) |
| sql_coverage | null_logic_ordering_aggregates | error | `select g, count(*) filter (where v > 15) from t group by g order by g` | NULL,1\|x,1\|y,1\|z,1 | ERR(invalid argument: column not found: g) |
| sql_coverage | null_logic_ordering_aggregates | error | `select g, sum(v) from t group by 1 order by 1` | NULL,60\|x,30\|y,30\|z,50 | ERR(invalid argument: expected alias name, got Int(1)) |
| sql_coverage | null_logic_ordering_aggregates | error | `select g, sum(v) from t group by g order by sum(v) desc` | NULL,60\|z,50\|x,30\|y,30 | ERR(invalid argument: ORDER BY column not found: sum) |
| sql_coverage | null_logic_ordering_aggregates | error | `select id from t order by id limit 10 offset 10` |  | ERR(invalid argument: column not found: id) |
| sql_coverage | null_logic_ordering_aggregates | error | `select id, g from t order by 2, 1` | 6,NULL\|1,x\|2,x\|3,y\|4,y\|5,z | ERR(invalid argument: expected alias name, got Int(2)) |
| sql_coverage | null_logic_ordering_aggregates | error | `select max_by(id, v), min_by(id, v) from t` | 6,1 | ERR(invalid argument: unknown function: max_by) |
| sql_coverage | null_logic_ordering_aggregates | error | `select percentile(v, 0.4) from t` | 26 | ERR(invalid argument: unknown function: percentile) |
| sql_coverage | null_logic_ordering_aggregates | error | `select percentile(v, 0.5), median(v), percentile_approx(v, 0.5) from t` | 30,30,30 | ERR(invalid argument: unknown function: percentile) |
| sql_coverage | null_logic_ordering_aggregates | error | `select percentile_cont(0.25) within group (order by v) from t` | 20 | ERR(invalid argument: unexpected token in expr: Group) |
| sql_coverage | null_logic_ordering_aggregates | error | `select sum(distinct v) from t` | 170 | ERR(invalid argument: unexpected token in expr: Distinct) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select * from (select g, v from t where g is not null) pivot (sum(v) for g in ('x', 'y'))` | 30,30 | x,10\|x,20\|y,30\|y,NULL\|z,50 |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select cast('2024-13-01' as date) from t where id = 1` | NULL | 2024-13-01 |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select d, lag(d) over (order by d) from t where d is not null order by d limit 2` | 2023-12-31,NULL\|2024-01-01,2023-12-31 | 2023-12-31,NULL\|2024-01-01,NULL |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select date_diff('2024-03-05', '2024-03-01') from t where id = 1` | 4 | -739343 |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select date_trunc('month', d) m, count(*) from t where d is not null group by date_trunc('month', d) order by m` | 2023-12-01 00:00:00,1\|2024-01-01 00:00:00,2\|2024-02-01 00:00:00,1\|2024-03-01 00:00:00,1 | 2024-01-01,5 |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select datediff(d, lag(d) over (order by d)) from t where d is not null order by d limit 3` | NULL\|1\|14 | 0\|0\|0 |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select datediff(null, '2024-01-01') from t where id = 1` | NULL | -739283 |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select to_date('2024-02-30') from t where id = 1` | NULL | 2024-02-30 |
| sql_coverage | pivot_lateral_using_hashes_and_dates | wrong | `select year(d), count(*) from t where d is not null group by year(d) order by year(d)` | 2023,1\|2024,4 | 0,5 |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select * from t left semi join u on t.id = u.id order by t.id` | 1,x,10,Hello,1.5,2024-01-15\|2,x,20,  pad ,2.5,2024-02-29 | ERR(invalid argument: expected Join, got Ident("semi")) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select * from t natural left join u order by id limit 2` | 1,x,10,Hello,1.5,2024-01-15,p\|2,x,20,  pad ,2.5,2024-02-29,q | ERR(invalid argument: expected On, got Order) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select a.id from t a join t b using (id) where a.id < 3 order by a.id` | 1\|2 | ERR(invalid argument: expected On, got Ident("using")) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select base64('Hello') from t where id = 1` | SGVsbG8= | ERR(invalid argument: unknown function: base64) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select char_length('héllo'), length('héllo'), octet_length('héllo') from t where id = 1` | 5,5,6 | ERR(invalid argument: unknown function: octet_length) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select count(*) from t full join u using (id)` | 8 | ERR(invalid argument: expected On, got LParen) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select count(*) from t join u using (id) join nn on t.id = nn.x` | 3 | ERR(invalid argument: expected On, got LParen) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select count(*) from t left anti join u on t.id = u.id where t.v is not null` | 3 | ERR(invalid argument: expected Join, got Ident("anti")) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select count(*) from t semi join (select id from u where w = 'q') s on t.id = s.id` | 1 | ERR(invalid argument: unknown table: s) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select date_format('2024-03-05', 'EEEE, MMM d, yyyy') from t where id = 1` | Tuesday, Mar 5, 2024 | ERR(invalid argument: unknown function: date_format) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select dayofweek('2024-03-03'), weekday('2024-03-03') from t where id = 1` | 1,6 | ERR(invalid argument: unknown function: dayofweek) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select format_number(1234567.891, 2), format_number(0.5, 0) from t where id = 1` | 1,234,567.89,0 | ERR(invalid argument: unknown function: format_number) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select format_string('%05d\|%.2f\|%s', 42, 3.14159, 'x') from t where id = 1` | 00042\|3.14\|x | ERR(invalid argument: unknown function: format_string) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select hash('a') from t where id = 1` | 1485273170 | ERR(invalid argument: unknown function: hash) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select id from t natural join u order by id` | 1\|2\|2 | ERR(invalid argument: expected On, got Order) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select id from t right join u using (id) order by id` | 1\|2\|2\|7 | ERR(invalid argument: expected On, got LParen) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select id, e from t lateral view explode(array(10, 20)) x as e where id = 1 order by e` | 1,10\|1,20 | ERR(invalid argument: column not found: e) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select id, w from t full join u using (id) order by id, w` | 1,p\|2,q\|2,r\|3,NULL\|4,NULL\|5,NULL\|6,NULL\|7,s | ERR(invalid argument: expected On, got LParen) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select last_day('2023-02-10'), add_months('2024-03-31', -1) from t where id = 1` | 2023-02-28,2024-02-29 | ERR(invalid argument: unknown function: last_day) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select length(uuid()) from t where id = 1` | 36 | ERR(invalid argument: unknown function: uuid) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select make_date(2024, 2, 30) from t where id = 1` | NULL | ERR(invalid argument: unknown function: make_date) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select md5('a'), sha1('a'), sha2('a', 256), crc32('a') from t where id = 1` | 0cc175b9c0f1b6a831c399e269772661,86f7e437faa5a7fce15d1ddcb9eaeaea377667b8,ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb,3904355907 | ERR(invalid argument: unknown function: md5) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select md5(cast(id as string)) from t where id = 1` | c4ca4238a0b923820dcc509a6f75849b | ERR(invalid argument: unknown function: md5) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select months_between('2024-03-15', '2024-01-15'), months_between('2024-03-31', '2024-02-29') from t where id = 1` | 2,1 | ERR(invalid argument: unknown function: months_between) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select next_day('2024-03-05', 'Friday'), next_day('2024-03-08', 'fri') from t where id = 1` | 2024-03-08,2024-03-15 | ERR(invalid argument: unknown function: next_day) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select quarter('2024-05-05'), weekofyear('2024-12-30'), dayofyear('2024-12-31') from t where id = 1` | 2,1,366 | ERR(invalid argument: unknown function: weekofyear) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select t.id from t left anti join u on t.id = u.id order by t.id` | 3\|4\|5\|6 | ERR(invalid argument: expected Join, got Ident("anti")) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select typeof(1.5), typeof('a'), typeof(true) from t where id = 1` | double,string,boolean | ERR(invalid argument: unknown function: typeof) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select u.id, w from t right join u using (id) order by u.id, w` | 1,p\|2,q\|2,r\|7,s | ERR(invalid argument: expected On, got LParen) |
| sql_coverage | pivot_lateral_using_hashes_and_dates | error | `select unix_timestamp('2024-01-01 00:00:00'), from_unixtime(86400) from t where id = 1` | 1704067200,1970-01-02 00:00:00 | ERR(invalid argument: unknown function: unix_timestamp) |
| sql_coverage | set_operation_precedence | wrong | `select id from t where id < 4 union select id from u except select id from t where id = 1 intersect select id from u order by id` | 2\|3\|7 | 1\|2\|3\|1\|2\|2\|7\|1\|1\|2\|2\|7 |
| sql_coverage | set_operation_precedence | error | `(select 1 as x union select 2) intersect select 2` | 2 | ERR(invalid argument: expected Select, got LParen) |
| sql_coverage | set_operation_precedence | error | `select 1 as x except select 1 union select 5 order by x` | 5 | ERR(invalid argument: ORDER BY column not found: x) |
| sql_coverage | set_operation_precedence | error | `select 1 as x union all select 2 intersect select 3 order by x` | 1 | ERR(invalid argument: ORDER BY column not found: x) |
| sql_coverage | set_operation_precedence | error | `select 1 as x union select 1 except select 1 union select 7 order by x` | 7 | ERR(invalid argument: ORDER BY column not found: x) |
| sql_coverage | set_operation_precedence | error | `select 1 as x union select 2 intersect select 2 order by x` | 1\|2 | ERR(invalid argument: ORDER BY column not found: x) |
| sql_coverage | set_operation_precedence | error | `select id from t where id < 4 intersect select id from u union select x from nn where x is not null order by id` | 1\|2 | ERR(invalid argument: ORDER BY column not found: id) |
| sql_coverage | set_operations_with_order_by | wrong | `select id from t where id < 4 intersect select id from u` | 1\|2 | 1\|2\|3\|1\|2\|2\|7 |
| sql_coverage | set_operations_with_order_by | wrong | `select id from t where id < 4 intersect select id from u order by id` | 1\|2 | 1\|2\|3\|1\|2\|2\|7 |
| sql_coverage | set_operations_with_order_by | wrong | `select id from t where id < 4 union select id from u order by id` | 1\|2\|3\|7 | 1\|2\|3\|1\|2\|2\|7 |
| sql_coverage | set_operations_with_order_by | error | `select id from t where id < 4 intersect select id from u union select x from nn where x is not null order by id` | 1\|2 | ERR(invalid argument: ORDER BY column not found: id) |
| sql_coverage | set_operations_with_order_by | error | `select id from t where id < 4 union select x from nn where x is not null order by id` | 1\|2\|3 | ERR(invalid argument: ORDER BY column not found: id) |
| sql_coverage | strings_regex_conditionals | wrong | `select concat('a', null) from t where id=1` | NULL | aNull |
| sql_coverage | strings_regex_conditionals | wrong | `select concat(s,'-',g) from t where id=4` | NULL | Null-y |
| sql_coverage | strings_regex_conditionals | wrong | `select instr(s,'l') from t where id=1` | 3 | 0 |
| sql_coverage | strings_regex_conditionals | wrong | `select lpad(s,3,'*') from t where id=1` | Hel | Hello |
| sql_coverage | strings_regex_conditionals | wrong | `select substr(s,-3) from t where id=1` | llo | Hello |
| sql_coverage | strings_regex_conditionals | error | `select base64('hi') from t where id=1` | aGk= | ERR(invalid argument: unknown function: base64) |
| sql_coverage | strings_regex_conditionals | error | `select concat_ws(',', g, s) from t where id=6` | World | ERR(invalid argument: unknown function: concat_ws) |
| sql_coverage | strings_regex_conditionals | error | `select contains(s,'ell') from t where id=1` | true | ERR(invalid argument: unknown function: contains) |
| sql_coverage | strings_regex_conditionals | error | `select format_string('%d-%s', id, g) from t where id=1` | 1-x | ERR(invalid argument: unknown function: format_string) |
| sql_coverage | strings_regex_conditionals | error | `select hex(255) from t where id=1` | FF | ERR(invalid argument: unknown function: hex) |
| sql_coverage | strings_regex_conditionals | error | `select levenshtein('kitten','sitting') from t where id=1` | 3 | ERR(invalid argument: unknown function: levenshtein) |
| sql_coverage | strings_regex_conditionals | error | `select md5('a') from t where id=1` | 0cc175b9c0f1b6a831c399e269772661 | ERR(invalid argument: unknown function: md5) |
| sql_coverage | strings_regex_conditionals | error | `select nvl2(v,1,0) from t order by id` | 1\|1\|1\|0\|1\|1 | ERR(invalid argument: unknown function: nvl2) |
| sql_coverage | strings_regex_conditionals | error | `select overlay('abcdef' placing 'XY' from 2 for 2) from t where id=1` | aXYdef | ERR(invalid argument: unknown function: overlay) |
| sql_coverage | strings_regex_conditionals | error | `select position('l' in s) from t where id=1` | 3 | ERR(invalid argument: unknown function: position) |
| sql_coverage | strings_regex_conditionals | error | `select regexp_extract(s,'(l+)',1) from t where id=1` | ll | ERR(invalid argument: unknown function: regexp_extract) |
| sql_coverage | strings_regex_conditionals | error | `select regexp_extract(s,'(l+)(o)',2) from t where id=1` | o | ERR(invalid argument: unknown function: regexp_extract) |
| sql_coverage | strings_regex_conditionals | error | `select regexp_like(s,'^H') from t where id=1` | true | ERR(invalid argument: unknown function: regexp_like) |
| sql_coverage | strings_regex_conditionals | error | `select regexp_replace(s,'[aeiou]','#') from t where id=1` | H#ll# | ERR(invalid argument: unknown function: regexp_replace) |
| sql_coverage | strings_regex_conditionals | error | `select s ilike 'hello' from t where id=1` | true | ERR(invalid argument: column not found: s) |
| sql_coverage | strings_regex_conditionals | error | `select s rlike '^H.*o$' from t where id=1` | true | ERR(invalid argument: column not found: s) |
| sql_coverage | strings_regex_conditionals | error | `select s \|\| '!' from t where id=1` | Hello! | ERR(invalid argument: unexpected char '\|' at pos 9) |
| sql_coverage | strings_regex_conditionals | error | `select sha2('a',256) from t where id=1` | ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb | ERR(invalid argument: unknown function: sha2) |
| sql_coverage | strings_regex_conditionals | error | `select split(s,',')[1] from t where id=5` | b | ERR(invalid argument: unexpected char '[' at pos 19) |
| sql_coverage | strings_regex_conditionals | error | `select startswith(s,'He') from t where id=1` | true | ERR(invalid argument: unknown function: startswith) |
| sql_coverage | strings_regex_conditionals | error | `select substring(s from 2 for 2) from t where id=1` | el | ERR(invalid argument: expected RParen, got From) |
| sql_coverage | strings_regex_conditionals | error | `select trim(both 'x' from 'xxaxx') from t where id=1` | a | ERR(invalid argument: expected RParen, got Str("x")) |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | wrong | `select 1 = 1.0, '1' = 1, 'a' = 'a ', true = 1 from t where id = 1` | true,true,false,true | true,NULL,false,NULL |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | wrong | `select 5 / 0, 5 % 0, 0 / 0, -5 / 0 from t where id = 1` | NULL,NULL,NULL,NULL | inf,NaN,NaN,-inf |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | wrong | `select cast(2147483648 as int), cast(-129 as tinyint), cast(300 as smallint) from t where id = 1` | -2147483648,127,300 | 2147483648,-129,300 |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | wrong | `select count(*) from t tablesample (0 percent)` | 0 | 6 |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | wrong | `select least('b', 'a', null), greatest(2, 10, 3.5), greatest('a', 'B') from t where id = 1` | a,10,a | b,10,B |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | wrong | `select pow(2, 62), pow(2, 64), 9223372036854775807 + 1 from t where id = 1` | 4611686018427387904,18446744073709551616,-9223372036854775808 | 4611686018427387904,18446744073709551616,9223372036854775808 |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | wrong | `select round(123.456, -1), round(-123.456, 1), ceil(-0.5), floor(0.5) from t where id = 1` | 120,-123.5,0,0 | 123,-123.5,0,0 |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | error | `select a.* from t a limit 1` | 1,x,10,Hello,1.5,2024-01-15 | ERR(invalid argument: expected identifier, got Star) |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | error | `select cast('2024-03-05 10:20:30' as date), cast(date '2024-03-05' as timestamp), cast('1e3' as double), cast('0x1F' as int) from t where id = 1` | 2024-03-05,2024-03-05 00:00:00,1000,NULL | ERR(invalid argument: expected RParen, got Str("2024-03-05")) |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | error | `select cast(true as string), cast(date '2024-03-05' as string), cast(1.5e0 as string), cast(-0.0 as string), cast(100000000.0 as string), cast(0.00001 as string) from t where id = 1` | true,2024-03-05,1.5,-0.0,1.0E8,1.0E-5 | ERR(invalid argument: expected RParen, got Str("2024-03-05")) |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | error | `select map_from_arrays(array(1), array(2)) from t where id = 1` | {1 -> 2} | ERR(invalid argument: unknown function: map_from_arrays) |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | error | `select sum(v) over (order by id groups between 1 preceding and current row) from t` | 10\|30\|50\|30\|50\|110 | ERR(invalid argument: expected RParen, got Ident("groups")) |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | error | `with recursive r as (select 1 as n union all select n + 1 from r where n < 3) select * from r` | 1\|2\|3 | ERR(invalid argument: expected As, got Ident("r")) |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | accepts | `select cast(v as binary) from t` | ERR | 10\|20\|30\|NULL\|50\|60 |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | accepts | `select id from t order by id nulls` | ERR | 1\|2\|3\|4\|5\|6 |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | accepts | `select id from t where id in (1, 2) escape '#'` | ERR | 1\|2 |
| sql_coverage | unsupported_syntax_is_rejected_and_edge_arithmetic | accepts | `select sum(v) from t group by g, rollup()` | ERR | 30\|30\|50\|60 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select 'it''s' from t where id=1` | it's | it |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select (v > 5) or true from t where id=4` | true | NULL |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select 10 / 4, 10 / 5, 10 div 4 from t where id=1` | 2.5,2,2 | 2.5,2,10 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select 1L + 2, 1.5D from t where id=1` | 3,1.5 | 1 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select coalesce(g, 'ALL') as g, sum(v) from t group by rollup(g) order by g` | ALL,60\|ALL,170\|x,30\|y,30\|z,50 | NULL,60\|x,30\|y,30\|z,50 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select distinct g from t limit 2` | x\|y | x |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select g, count(*) from t group by g with rollup` | x,2\|y,2\|z,1\|NULL,1\|NULL,6 | x,2\|y,2\|z,1\|NULL,1 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select g, count(*) from t group by grouping sets ((g), ())` | x,2\|y,2\|z,1\|NULL,1\|NULL,6 | x,6 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select g, count(*) from t group by rollup(g)` | x,2\|y,2\|z,1\|NULL,1\|NULL,6 | x,2\|y,2\|z,1\|NULL,1 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select g, id, count(*) from t where id < 3 group by rollup(g, id)` | x,1,1\|x,2,1\|x,NULL,2\|NULL,NULL,2 | x,1,1\|x,2,1 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select g, sum(v) from t group by cube(g)` | x,30\|y,30\|z,50\|NULL,60\|NULL,170 | x,30\|y,30\|z,50\|NULL,60 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select g, v, count(*) from t where id < 4 group by grouping sets ((g, v), (g), ())` | x,10,1\|x,20,1\|y,30,1\|x,NULL,2\|y,NULL,1\|NULL,NULL,3 | x,10,3 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id from t qualify row_number() over (partition by g order by id desc) = 1` | 2\|4\|5\|6 |  |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, cume_dist() over (order by id) from t order by id` | 1,0.166667\|2,0.333333\|3,0.5\|4,0.666667\|5,0.833333\|6,1 | 1,1\|2,2\|3,3\|4,4\|5,5\|6,6 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, first_value(v) over (order by id rows between 1 preceding and current row) from t order by id` | 1,10\|2,10\|3,20\|4,30\|5,NULL\|6,50 | 1,10\|2,10\|3,10\|4,10\|5,10\|6,10 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, lag(g) over (partition by g order by id) from t order by id` | 1,NULL\|2,x\|3,NULL\|4,y\|5,NULL\|6,NULL | 1,NULL\|2,NULL\|3,NULL\|4,NULL\|5,NULL\|6,NULL |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, last_value(v) over (order by id) from t order by id` | 1,10\|2,20\|3,30\|4,NULL\|5,50\|6,60 | 1,60\|2,60\|3,60\|4,60\|5,60\|6,60 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, max(v) over (partition by g order by id) from t order by id` | 1,10\|2,20\|3,30\|4,30\|5,50\|6,60 | 1,20\|2,20\|3,30\|4,30\|5,50\|6,60 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, min(v) over (order by id rows between 2 preceding and current row) from t order by id` | 1,10\|2,10\|3,10\|4,20\|5,30\|6,50 | 1,10\|2,10\|3,10\|4,10\|5,10\|6,10 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, ntile(4) over (order by id) from t order by id` | 1,1\|2,1\|3,2\|4,2\|5,3\|6,4 | 1,1\|2,1\|3,2\|4,3\|5,3\|6,4 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, percent_rank() over (order by id) from t order by id` | 1,0\|2,0.2\|3,0.4\|4,0.6\|5,0.8\|6,1 | 1,1\|2,2\|3,3\|4,4\|5,5\|6,6 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, rank() over (order by g), dense_rank() over (order by g) from t order by id` | 1,2,2\|2,2,2\|3,4,3\|4,4,3\|5,6,4\|6,1,1 | 1,2,2\|2,3,3\|3,4,4\|4,5,5\|5,6,6\|6,1,1 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, row_number() over (order by g nulls last, id) from t order by id` | 1,1\|2,2\|3,3\|4,4\|5,5\|6,6 | 1,2\|2,3\|3,4\|4,5\|5,6\|6,1 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, sum(v) over (order by id range between 1 preceding and 1 following) from t order by id` | 1,30\|2,60\|3,50\|4,80\|5,110\|6,110 | 1,170\|2,170\|3,170\|4,170\|5,170\|6,170 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, sum(v) over (order by id rows between 1 preceding and 1 following) from t order by id` | 1,30\|2,60\|3,50\|4,80\|5,110\|6,110 | 1,170\|2,170\|3,170\|4,170\|5,170\|6,170 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, sum(v) over (order by id rows between current row and 1 following) from t order by id` | 1,30\|2,50\|3,30\|4,50\|5,110\|6,60 | 1,170\|2,170\|3,170\|4,170\|5,170\|6,170 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, sum(v) over (order by v desc nulls last range between 10 preceding and 10 following) from t order by id` | 1,30\|2,60\|3,50\|4,NULL\|5,110\|6,110 | 1,170\|2,170\|3,170\|4,170\|5,170\|6,170 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, sum(v) over (order by v range between unbounded preceding and current row) from t order by id` | 1,10\|2,30\|3,60\|4,NULL\|5,110\|6,170 | 1,170\|2,170\|3,170\|4,170\|5,170\|6,170 |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select id, v - lag(v) over (order by id) from t order by id` | 1,NULL\|2,10\|3,10\|4,NULL\|5,NULL\|6,10 | 1,NULL\|2,NULL\|3,NULL\|4,NULL\|5,NULL\|6,NULL |
| sql_coverage | windows_grouping_sets_and_syntax | wrong | `select x.id from t x, u y where x.id = y.id and y.w = 'p'` | 1 | 1\|2\|3\|4\|5\|6 |
| sql_coverage | windows_grouping_sets_and_syntax | error | `SELECT ID FROM T WHERE ID = 1` | 1 | ERR(invalid argument: unknown table: T) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select "dq" from t where id=1` | dq | ERR(invalid argument: unexpected char '"' at pos 7) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select * from (values (1, 'a'), (2, 'b')) as q(n, c) where n > 1` | 2,b | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select 5 & 3, 5 \| 3, 5 ^ 3, shiftleft(1,3) from t where id=1` | 1,7,6,8 | ERR(invalid argument: unexpected char '&' at pos 9) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `` select `id` from t where id=1 `` | 1 | ERR(invalid argument: unexpected char '`' at pos 7) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select g, count(*) from t group by g order by count(*) desc, g limit 2` | x,2\|y,2 | ERR(invalid argument: ORDER BY column not found: count) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select g, grouping(g), count(*) from t group by rollup(g)` | x,0,2\|y,0,2\|z,0,1\|NULL,0,1\|NULL,1,6 | ERR(invalid argument: unknown function: grouping) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select g, sum(v), rank() over (order by sum(v) desc) from t group by g` | x,30,3\|y,30,3\|z,50,2\|NULL,60,1 | ERR(invalid argument: expected RParen, got LParen) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select id as "my id" from t where id=1` | 1 | ERR(invalid argument: unexpected char '"' at pos 13) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select id, lag(v, 1, -1) over (order by id) from t order by id` | 1,-1\|2,10\|3,20\|4,30\|5,NULL\|6,50 | ERR(invalid argument: expected RParen, got Comma) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select id, nth_value(v, 2) over (order by id) from t order by id` | 1,NULL\|2,20\|3,20\|4,20\|5,20\|6,20 | ERR(invalid argument: unknown function: nth_value) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select id, sum(v) over (order by id rows unbounded preceding) from t order by id` | 1,10\|2,30\|3,60\|4,60\|5,110\|6,170 | ERR(invalid argument: unexpected token in expr: RParen) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select id, sum(v) over w from t window w as (order by id) order by id` | 1,10\|2,30\|3,60\|4,60\|5,110\|6,170 | ERR(invalid argument: expected LParen, got Ident("w")) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select id, sum(v) over w, count(*) over w2 from t window w as (order by id), w2 as (partition by g) order by id` | 1,10,2\|2,30,2\|3,60,2\|4,60,2\|5,110,1\|6,170,1 | ERR(invalid argument: expected LParen, got Ident("w")) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select n from (values (1), (2), (3)) as q(n) order by n desc limit 1` | 3 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select q.g, q.c from (select g, count(*) as c from t group by g) q where q.c > 1 order by q.g` | x,2\|y,2 | ERR(invalid argument: column not found: q.g) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select t.* from t where id = 1` | 1,x,10,Hello,1.5,2024-01-15 | ERR(invalid argument: expected identifier, got Star) |
| sql_coverage | windows_grouping_sets_and_syntax | error | `select true, false from t where id=1` | true,false | ERR(invalid argument: column not found: true) |
| sql_coverage2 | constant_limit_and_recursive_ctes | wrong | `select id from t order by id limit 1 + 1` | 1\|2 | 1 |
| sql_coverage2 | constant_limit_and_recursive_ctes | wrong | `select id from t order by id limit 2 offset 1 + 1` | 3\|4 | 2\|3 |
| sql_coverage2 | constant_limit_and_recursive_ctes | error | `with recursive fib(a, b) as (select 0, 1 union all select b, a + b from fib where a < 50) select a from fib` | 0\|1\|1\|2\|3\|5\|8\|13\|21\|34\|55 | ERR(invalid argument: expected As, got Ident("fib")) |
| sql_coverage2 | constant_limit_and_recursive_ctes | error | `with recursive r as (select 1 as n union all select n + 1 from r where n < 5) select * from r` | 1\|2\|3\|4\|5 | ERR(invalid argument: expected As, got Ident("r")) |
| sql_coverage2 | constant_limit_and_recursive_ctes | error | `with recursive r(n) as (select 1 union all select n + 1 from r where n < 5) select sum(n) from r` | 15 | ERR(invalid argument: expected As, got Ident("r")) |
| sql_coverage2 | constant_limit_and_recursive_ctes | error | `with recursive t2 as (select 1 as n union select n + 1 from t2 where n < 3 union select 1) select * from t2` | 1\|2\|3 | ERR(invalid argument: expected As, got Ident("t2")) |
| sql_coverage2 | constant_limit_and_recursive_ctes | error | `with recursive tree(id, depth) as (select 1, 0 union all select id * 2, depth + 1 from tree where depth < 3) select id, depth from tree order by id` | 1,0\|2,1\|4,2\|8,3 | ERR(invalid argument: expected As, got Ident("tree")) |
| sql_coverage2 | dml_and_ddl | wrong | `select id, v, g from w order by id` | 1,10,x\|2,99,x\|3,30,y\|4,NULL,y\|5,51,q\|6,61,q | 1,10,x\|2,20,x\|3,30,y\|4,NULL,y\|5,50,z\|6,60,NULL |
| sql_coverage2 | generators | error | `select id, e from t lateral view explode(array()) q as e where id < 3` |  | ERR(invalid argument: column not found: e) |
| sql_coverage2 | generators | error | `select id, e from t lateral view explode_outer(array()) q as e where id < 3` | 1,NULL\|2,NULL | ERR(invalid argument: column not found: e) |
| sql_coverage2 | generators | error | `select kk, vv from t lateral view explode(map('a', 1, 'b', 2)) q as kk, vv where id = 1` | a,1\|b,2 | ERR(invalid argument: column not found: kk) |
| sql_coverage2 | generators | error | `select pos, e from t lateral view posexplode(array(10, 20)) q as pos, e where id = 1` | 0,10\|1,20 | ERR(invalid argument: column not found: pos) |
| sql_coverage2 | generators | error | `select posexplode(array(10, 20, 30))` | 0,10\|1,20\|2,30 | ERR(invalid argument: unknown function: posexplode) |
| sql_coverage2 | generators | error | `select regexp_extract_all('a1b22c333', '([0-9]+)', 1)` | [1, 22, 333] | ERR(invalid argument: unknown function: regexp_extract_all) |
| sql_coverage2 | generators | error | `select regexp_extract_all('a1b22c333', '([a-z])([0-9]+)', 2), regexp_extract_all('xyz', '[0-9]+')` | [1, 22, 333],[] | ERR(invalid argument: unknown function: regexp_extract_all) |
| sql_coverage2 | higher_order_functions | wrong | `select count(*) from t where exists (select 1 from t t2 where t2.id = t.id + 5)` | 1 | 6 |
| sql_coverage2 | higher_order_functions | error | `select aggregate(array(1, 2, 3), 0, (acc, x) -> acc + x), reduce(array(1, 2, 3), 1, (acc, x) -> acc * x)` | 6,6 | ERR(invalid argument: unknown function: aggregate) |
| sql_coverage2 | higher_order_functions | error | `select aggregate(array(1, 2, 3), 0, (acc, x) -> acc + x, acc -> acc * 10)` | 60 | ERR(invalid argument: unknown function: aggregate) |
| sql_coverage2 | higher_order_functions | error | `select array_join(transform(sequence(1, 3), x -> cast(x as string)), ',')` | 1,2,3 | ERR(invalid argument: unknown function: array_join) |
| sql_coverage2 | higher_order_functions | error | `select array_sort(array(3, 1, 2))` | [1, 2, 3] | ERR(invalid argument: unknown function: array_sort) |
| sql_coverage2 | higher_order_functions | error | `select array_sort(array(3, 1, 2), (a, b) -> case when a < b then 1 when a > b then -1 else 0 end)` | [3, 2, 1] | ERR(invalid argument: unknown function: array_sort) |
| sql_coverage2 | higher_order_functions | error | `select count(*) from t where exists(array(1, 2), x -> x = id)` | 2 | ERR(invalid argument: expected Select, got Ident("array")) |
| sql_coverage2 | higher_order_functions | error | `select exists(array(1, 2, 3), x -> x > 2), forall(array(1, 2, 3), x -> x > 0), exists(array(1, 2), x -> x > 5)` | true,true,false | ERR(invalid argument: expected Select, got Ident("array")) |
| sql_coverage2 | higher_order_functions | error | `select exists(array(1, null), x -> x > 5), forall(array(1, null), x -> x > 0)` | NULL,NULL | ERR(invalid argument: expected Select, got Ident("array")) |
| sql_coverage2 | higher_order_functions | error | `select filter(array(1, 2, 3, 4), x -> x % 2 = 0)` | [2, 4] | ERR(invalid argument: unknown function: filter) |
| sql_coverage2 | higher_order_functions | error | `select id, transform(array(1, 2), x -> x + id) from t where id < 3` | 1,[2, 3]\|2,[3, 4] | ERR(invalid argument: unknown function: transform) |
| sql_coverage2 | higher_order_functions | error | `select transform(array('a', 'b'), x -> upper(x))` | [A, B] | ERR(invalid argument: unknown function: transform) |
| sql_coverage2 | higher_order_functions | error | `select transform(array(1, 2, 3), (x, i) -> x * i)` | [0, 2, 6] | ERR(invalid argument: unknown function: transform) |
| sql_coverage2 | higher_order_functions | error | `select transform(array(1, 2, 3), x -> x + 1)` | [2, 3, 4] | ERR(invalid argument: unknown function: transform) |
| sql_coverage2 | higher_order_functions | error | `select transform(array(array(1, 2), array(3)), a -> size(a))` | [2, 1] | ERR(invalid argument: unknown function: transform) |
| sql_coverage2 | higher_order_functions | error | `select zip_with(array(1, 2), array(3, 4), (a, b) -> a + b)` | [4, 6] | ERR(invalid argument: unknown function: zip_with) |
| sql_coverage2 | higher_order_functions | error | `select zip_with(array(1, 2, 3), array(10), (a, b) -> coalesce(b, 0) + a)` | [11, 2, 3] | ERR(invalid argument: unknown function: zip_with) |
| sql_coverage2 | map_values | error | `select element_at(map('a', 1), 'a'), size(map('a', 1, 'b', 2))` | 1,2 | ERR(invalid argument: unknown function: element_at) |
| sql_coverage2 | map_values | error | `select explode(map('a', 1, 'b', 2))` | a,1\|b,2 | ERR(invalid argument: unknown function: explode) |
| sql_coverage2 | map_values | error | `select from_json('[1,2,3]', 'array<int>'), from_json('{"a":1}', 'map<string,int>')` | [1, 2, 3],{a -> 1} | ERR(invalid argument: unknown function: from_json) |
| sql_coverage2 | map_values | error | `select from_json('{"a":1}', 'a int')` | {1} | ERR(invalid argument: unknown function: from_json) |
| sql_coverage2 | map_values | error | `select map('a', 1)['a'], map('a', 1)['z']` | 1,NULL | ERR(invalid argument: unexpected char '[' at pos 18) |
| sql_coverage2 | map_values | error | `select map('a', 1, 'a', 2)` | {a -> 2} | ERR(invalid argument: unknown function: map) |
| sql_coverage2 | map_values | error | `select map('a', 1, 'b', 2)` | {a -> 1, b -> 2} | ERR(invalid argument: unknown function: map) |
| sql_coverage2 | map_values | error | `select map(1, array(1, 2)), array(map(1, 2))` | {1 -> [1, 2]},[{1 -> 2}] | ERR(invalid argument: unknown function: map) |
| sql_coverage2 | map_values | error | `select map_concat(map(1, 2), map(3, 4))` | {1 -> 2, 3 -> 4} | ERR(invalid argument: unknown function: map_concat) |
| sql_coverage2 | map_values | error | `select map_contains_key(map(1, 2), 1), map_contains_key(map(1, 2), 5)` | true,false | ERR(invalid argument: unknown function: map_contains_key) |
| sql_coverage2 | map_values | error | `select map_filter(map(1, 2, 3, 4), (k, v) -> v > 2)` | {3 -> 4} | ERR(invalid argument: unknown function: map_filter) |
| sql_coverage2 | map_values | error | `select map_from_arrays(array(1, 2), array('a', 'b'))` | {1 -> a, 2 -> b} | ERR(invalid argument: unknown function: map_from_arrays) |
| sql_coverage2 | map_values | error | `select map_keys(map('a', 1)), map_values(map('a', 1))` | [a],[1] | ERR(invalid argument: unknown function: map_keys) |
| sql_coverage2 | map_values | error | `select str_to_map('a:1,b:2'), str_to_map('a:1,b:2')['b']` | {a -> 1, b -> 2},2 | ERR(invalid argument: unexpected char '[' at pos 51) |
| sql_coverage2 | map_values | error | `select to_json(array(1, 2)), to_json(map('a', 1))` | [1,2],{"a":1} | ERR(invalid argument: unknown function: to_json) |
| sql_coverage2 | map_values | error | `select transform_keys(map(1, 2), (k, v) -> k + 1)` | {2 -> 2} | ERR(invalid argument: unknown function: transform_keys) |
| sql_coverage2 | map_values | error | `select transform_values(map(1, 2, 3, 4), (k, v) -> v * 10)` | {1 -> 20, 3 -> 40} | ERR(invalid argument: unknown function: transform_values) |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | wrong | `select g, id, row_number() over (partition by g order by id desc) from t` | NULL,6,1\|x,2,1\|x,1,2\|y,4,1\|y,3,2\|z,5,1 | x,1,2\|x,2,1\|y,3,2\|y,4,1\|z,5,1\|NULL,6,1 |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | wrong | `select g, id, sum(v) from t where id < 4 group by g, rollup(id) order by g, id` | x,NULL,30\|x,1,10\|x,2,20\|y,NULL,30\|y,3,30 | x,1,30\|y,3,30 |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | wrong | `select id, row_number() over (order by id), ntile(2) over (order by id) from t` | 1,1,1\|2,2,1\|3,3,1\|4,4,2\|5,5,2\|6,6,2 | 1,1,1\|2,2,2\|3,3,3\|4,4,4\|5,5,5\|6,6,6 |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | wrong | `select id, sum(case when id > 3 then 1.5 else 1 end) over (order by id) from t` | 1,1\|2,2\|3,3\|4,4.5\|5,6\|6,7.5 | 1,NULL\|2,NULL\|3,NULL\|4,NULL\|5,NULL\|6,NULL |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | error | `select date '2024-03-15' + 1, date '2024-03-15' - 1, 1 + date '2024-03-15'` | 2024-03-16,2024-03-14,2024-03-16 | ERR(invalid argument: column not found: date) |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | error | `select format_number(0.5, 0), format_number(2.5, 0), format_number(1234.5, 0), format_number(1234567.891, 2)` | 0,2,1,234,1,234,567.89 | ERR(invalid argument: unknown function: format_number) |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | error | `select format_string('%e', 12345.678), format_string('%o', 8)` | 1.234568e+04,10 | ERR(invalid argument: unknown function: format_string) |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | error | `select g, grouping_id(), sum(v) from t group by rollup(g) order by g` | NULL,0,60\|NULL,1,170\|x,0,30\|y,0,30\|z,0,50 | ERR(invalid argument: unknown function: grouping_id) |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | error | `select grouping_id() from t group by rollup(g)` | 0\|0\|0\|0\|1 | ERR(invalid argument: unknown function: grouping_id) |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | error | `select make_timestamp(2024, 3, 15, 10, 20, 30), unix_millis(timestamp '1970-01-01 00:00:01'), json_array_length('[1,2,3]')` | 2024-03-15 10:20:30,1000,3 | ERR(invalid argument: unknown function: make_timestamp) |
| sql_coverage2 | ranking_types_window_order_and_misc_semantics | error | `select typeof(row_number() over (order by id)), typeof(rank() over (order by id)), typeof(dense_rank() over (order by id)), typeof(ntile(2) over (order by id)) from t limit 1` | bigint,bigint,bigint,bigint | ERR(invalid argument: unknown function: typeof) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | wrong | `select g, count(*) from t group by g cluster by g` | NULL,1\|x,2\|y,2\|z,1 | x,2\|y,2\|z,1\|NULL,1 |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | wrong | `select id from t sort by id desc` | 6\|5\|4\|3\|2\|1 | 1\|2\|3\|4\|5\|6 |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select bit_and(v) from t where id = 4` | NULL | ERR(invalid argument: unknown function: bit_and) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select bit_and(v), bit_or(v), bit_xor(v) from t` | 0,62,14 | ERR(invalid argument: unknown function: bit_and) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select from_utc_timestamp(null, 'UTC'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', null)` | NULL,NULL | ERR(invalid argument: unknown function: from_utc_timestamp) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select from_utc_timestamp(timestamp '2024-01-15 12:00:00', 'America/Chicago'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'America/Los_Angeles'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'Europe/Berlin'), from_utc_timestamp(timestamp '2024-01-15 12:00:00', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'Asia/Tokyo')` | 2024-01-15 06:00:00,2024-07-15 05:00:00,2024-07-15 14:00:00,2024-01-15 23:00:00,2024-07-15 22:00:00,2024-07-15 21:00:00 | ERR(invalid argument: unknown function: from_utc_timestamp) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select from_utc_timestamp(timestamp '2024-04-07 02:30:00', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-04-06 15:30:00', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-10-05 15:59:59', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-10-05 16:00:00', 'Australia/Sydney')` | 2024-04-07 12:30:00,2024-04-07 02:30:00,2024-10-06 01:59:59,2024-10-06 03:00:00 | ERR(invalid argument: unknown function: from_utc_timestamp) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select from_utc_timestamp(timestamp '2024-07-01 12:00:00', 'America/New_York'), from_utc_timestamp(timestamp '2024-03-10 06:59:59', 'America/New_York'), from_utc_timestamp(timestamp '2024-03-10 07:00:00', 'America/New_York'), from_utc_timestamp(timestamp '2024-10-27 00:59:59', 'Europe/London'), from_utc_timestamp(timestamp '2024-10-27 01:00:00', 'Europe/London'), from_utc_timestamp(timestamp '2024-01-01 12:00:00', '+05:30'), from_utc_timestamp(timestamp '2024-01-01 12:00:00', 'Asia/Kolkata')` | 2024-07-01 08:00:00,2024-03-10 01:59:59,2024-03-10 03:00:00,2024-10-27 01:59:59,2024-10-27 01:00:00,2024-01-01 17:30:00,2024-01-01 17:30:00 | ERR(invalid argument: unknown function: from_utc_timestamp) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'UTC+2'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'GMT-3')` | 2024-07-15 14:00:00,2024-07-15 09:00:00 | ERR(invalid argument: unknown function: from_utc_timestamp) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select g, bit_or(id) from t group by g order by g` | NULL,6\|x,3\|y,7\|z,5 | ERR(invalid argument: unknown function: bit_or) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select hash('Spark', array(123), 2), xxhash64('Spark', array(123), 2), hash(cast(null as int)), hash(1, cast(null as string))` | -1321691492,5602566077635097486,42,-559580957 | ERR(invalid argument: unknown function: hash) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select hash('abcdefghijklmnopqrstuvwxyz0123456789'), xxhash64('abcdefghijklmnopqrstuvwxyz0123456789'), hash(''), xxhash64(''), hash('héllo'), xxhash64('héllo')` | 46508703,2724237963427677065,142593372,-7444071767201028348,1212979866,501425390238239234 | ERR(invalid argument: unknown function: hash) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select hash(1, 'a', 2.5, true), xxhash64(1, 'a', 2.5, true), hash(array(1,2,3)), hash(named_struct('a',1,'b','x'))` | 2009709968,8123807079836635594,-912918097,-1305545736 | ERR(invalid argument: unknown function: hash) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select hash(2.5), hash(-1.25), xxhash64(2.5), hash(cast(2.5 as double))` | 77911036,-550551638,9077239597295338739,-892273744 | ERR(invalid argument: unknown function: hash) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select hash(cast(5 as int)), hash(cast(5 as bigint)), hash(-1), hash(2147483648), hash(0.0), hash(-0.0)` | 1023896466,1607884268,-1604776387,238268309,-1670924195,-1670924195 | ERR(invalid argument: unknown function: hash) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select hash(g), xxhash64(g), hash(f), xxhash64(f) from t order by id` | -80340190,-5636050478767222463,1290763749,7738255526519901366\|-80340190,-5636050478767222463,-892273744,2435211733827758672\|1475214315,-2901346384480684965,-1316292811,-4418739860910625355\|1475214315,-2901346384480684965,42,42\|1190301286,6959616394252558107,1290556682,-3054283333461073093\|42,42,-1670924195,-5252525462095825812 | ERR(invalid argument: unknown function: hash) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select histogram_numeric(f, 10) from t` | [{-3.5, 1.0}, {0.0, 1.0}, {1.5, 1.0}, {2.5, 1.0}, {4.0, 1.0}] | ERR(invalid argument: unknown function: histogram_numeric) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select histogram_numeric(f, 2) from t` | [{-3.5, 1.0}, {2.0, 4.0}] | ERR(invalid argument: unknown function: histogram_numeric) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select regr_count(f, v) from t where id = 4` | 0 | ERR(invalid argument: unknown function: regr_count) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select regr_count(f, v), regr_avgx(f, v), regr_avgy(f, v), regr_sxx(f, v), regr_syy(f, v), regr_sxy(f, v) from t` | 5,34,0.9,1720,32.7,7 | ERR(invalid argument: unknown function: regr_count) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select regr_r2(f, v) from t where id <= 2` | 1 | ERR(invalid argument: unknown function: regr_r2) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select regr_slope(f, v) from t where id = 1` | NULL | ERR(invalid argument: unknown function: regr_slope) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select regr_slope(f, v), regr_intercept(f, v), regr_r2(f, v) from t` | 0.00407,0.761628,0.000871 | ERR(invalid argument: unknown function: regr_slope) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select soundex('Robert'), soundex('Rupert'), soundex('Tymczak'), soundex('Pfister'), soundex('Honeyman'), soundex('A'), soundex(null), soundex(''), soundex('123'), soundex('a-b'), soundex('Ashcraft')` | R163,R163,T522,P236,H555,A000,NULL,,123,A100,A261 | ERR(invalid argument: unknown function: soundex) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `select to_utc_timestamp(timestamp '2024-07-15 12:00:00', 'America/New_York'), to_utc_timestamp(timestamp '2024-01-15 12:00:00', 'America/New_York'), to_utc_timestamp(timestamp '2024-11-03 01:30:00', 'America/New_York'), to_utc_timestamp(timestamp '2024-03-10 02:30:00', 'America/New_York'), to_utc_timestamp(timestamp '2024-03-31 01:30:00', 'Europe/Berlin'), to_utc_timestamp(timestamp '2024-10-27 02:30:00', 'Europe/Berlin')` | 2024-07-15 16:00:00,2024-01-15 17:00:00,2024-11-03 05:30:00,2024-03-10 07:30:00,2024-03-31 00:30:00,2024-10-27 00:30:00 | ERR(invalid argument: unknown function: to_utc_timestamp) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `set spark.sql.shuffle.partitions=4` | spark.sql.shuffle.partitions,4 | ERR(invalid argument: expected Select, got Ident("set")) |
| sql_gaps3 | aggregates_hash_zones_syntax_vs_spark | error | `use default` |  | ERR(invalid argument: expected Select, got Ident("use")) |
| sql_gaps3 | date_timestamp_columns | wrong | `select count(*) from d where dt <> '2024-03-15 00:00:00'` | 1 | 2 |
| sql_gaps3 | date_timestamp_columns | wrong | `select id from d where dt < timestamp '2024-03-15 00:00:01' order by id` | 1\|2 |  |
| sql_gaps3 | date_timestamp_columns | wrong | `select id from d where dt = timestamp '2024-03-15 00:00:00'` | 2 |  |
| sql_gaps3 | date_timestamp_columns | wrong | `select id from d where dt = ts order by id` | 2 |  |
| sql_gaps3 | date_timestamp_columns | wrong | `select id from d where ts = date '2024-03-15'` | 2 |  |
| sql_gaps3 | date_timestamp_columns | wrong | `select id from d where ts > date '2024-03-15' order by id` | 3 |  |
| sql_gaps3 | date_timestamp_comparison | wrong | `select '2024-03-15' = date '2024-03-15'` | true | NULL |
| sql_gaps3 | date_timestamp_comparison | wrong | `select cast('2024-02-30' as date)` | NULL | 2024-02-30 |
| sql_gaps3 | date_timestamp_comparison | wrong | `select cast('2024-03-15' as timestamp)` | 2024-03-15 00:00:00 | 2024-03-15 |
| sql_gaps3 | date_timestamp_comparison | wrong | `select to_date('2024-02-30')` | NULL | 2024-02-30 |
| sql_gaps3 | date_timestamp_comparison | wrong | `select to_date('2024-03-15 10:20:30')` | 2024-03-15 | 2024-03-15 10:20:30 |
| sql_gaps3 | date_timestamp_comparison | error | `select cast(date '2024-03-15' as timestamp)` | 2024-03-15 00:00:00 | ERR(invalid argument: expected RParen, got Str("2024-03-15")) |
| sql_gaps3 | date_timestamp_comparison | error | `select cast(timestamp '2024-03-15 10:20:30' as date)` | 2024-03-15 | ERR(invalid argument: expected RParen, got Str("2024-03-15 10:20:30")) |
| sql_gaps3 | date_timestamp_comparison | error | `select date '2024-01-31' + interval 1 month` | 2024-02-29 | ERR(invalid argument: column not found: date) |
| sql_gaps3 | date_timestamp_comparison | error | `select date '2024-03-15' < timestamp '2024-03-15 00:00:01'` | true | ERR(invalid argument: column not found: date) |
| sql_gaps3 | date_timestamp_comparison | error | `select date '2024-03-15' <> timestamp '2024-03-15 10:00:00'` | true | ERR(invalid argument: column not found: date) |
| sql_gaps3 | date_timestamp_comparison | error | `select date '2024-03-15' = timestamp '2024-03-15 00:00:00'` | true | ERR(invalid argument: column not found: date) |
| sql_gaps3 | date_timestamp_comparison | error | `select date '2024-03-15' >= '2024-03-15 00:00:00'` | true | ERR(invalid argument: column not found: date) |
| sql_gaps3 | date_timestamp_comparison | error | `select date '2024-03-15' in (timestamp '2024-03-15 00:00:00', timestamp '2025-01-01 00:00:00')` | true | ERR(invalid argument: column not found: date) |
| sql_gaps3 | date_timestamp_comparison | error | `select date '2024-03-16' > timestamp '2024-03-15 23:59:59'` | true | ERR(invalid argument: column not found: date) |
| sql_gaps3 | date_timestamp_comparison | error | `select date_trunc('hour', timestamp '2024-03-15 10:20:30')` | 2024-03-15 10:00:00 | ERR(invalid argument: expected RParen, got Str("2024-03-15 10:20:30")) |
| sql_gaps3 | date_timestamp_comparison | error | `select date_trunc('month', timestamp '2024-03-15 10:20:30')` | 2024-03-01 00:00:00 | ERR(invalid argument: expected RParen, got Str("2024-03-15 10:20:30")) |
| sql_gaps3 | date_timestamp_comparison | error | `select date_trunc('quarter', timestamp '2024-05-15 10:20:30')` | 2024-04-01 00:00:00 | ERR(invalid argument: expected RParen, got Str("2024-05-15 10:20:30")) |
| sql_gaps3 | date_timestamp_comparison | error | `select date_trunc('week', date '2024-03-15')` | 2024-03-11 00:00:00 | ERR(invalid argument: expected RParen, got Str("2024-03-15")) |
| sql_gaps3 | date_timestamp_comparison | error | `select date_trunc('year', date '2024-05-15')` | 2024-01-01 00:00:00 | ERR(invalid argument: expected RParen, got Str("2024-05-15")) |
| sql_gaps3 | date_timestamp_comparison | error | `select timestamp '2024-01-31 10:00:00' + interval 1 month` | 2024-02-29 10:00:00 | ERR(invalid argument: column not found: timestamp) |
| sql_gaps3 | date_timestamp_comparison | error | `select timestamp '2024-03-15 00:00:00' = date '2024-03-15'` | true | ERR(invalid argument: column not found: timestamp) |
| sql_gaps3 | date_timestamp_comparison | error | `select timestamp '2024-03-15 10:00:00' + interval '1' day` | 2024-03-16 10:00:00 | ERR(invalid argument: column not found: timestamp) |
| sql_gaps3 | date_timestamp_comparison | error | `select timestamp '2024-03-15 10:00:00' - interval 2 hours` | 2024-03-15 08:00:00 | ERR(invalid argument: column not found: timestamp) |
| sql_gaps3 | date_timestamp_comparison | error | `select timestamp '2024-03-15 10:00:00' > '2024-03-15'` | true | ERR(invalid argument: column not found: timestamp) |
| sql_gaps3 | date_timestamp_comparison | error | `select timestamp '2024-03-15 10:00:00' between date '2024-03-15' and date '2024-03-16'` | true | ERR(invalid argument: column not found: timestamp) |
| sql_gaps3 | date_timestamp_comparison | error | `select timestamp '2024-03-15 10:00:00.5' > timestamp '2024-03-15 10:00:00'` | true | ERR(invalid argument: column not found: timestamp) |
| sql_gaps3 | date_timestamp_comparison | error | `select trunc(date '2024-03-15', 'month')` | 2024-03-01 | ERR(invalid argument: expected RParen, got Str("2024-03-15")) |
| sql_gaps3 | date_timestamp_comparison | error | `select trunc(date '2024-03-15', 'year')` | 2024-01-01 | ERR(invalid argument: expected RParen, got Str("2024-03-15")) |
| sql_gaps3 | decimal_display | wrong | `select cast(-0.001 as decimal(5,2))` | 0.00 | -0.001 |
| sql_gaps3 | decimal_display | wrong | `select cast(1.5 as decimal(5,2))` | 1.50 | 1.5 |
| sql_gaps3 | decimal_display | wrong | `select cast(1.50 as decimal(5,2)), cast(v as decimal(6,3)) from t where id = 1` | 1.50,10.000 | 1.5,10 |
| sql_gaps3 | decimal_display | wrong | `select cast(123.456 as decimal(10,2))` | 123.46 | 123.456 |
| sql_gaps3 | decimal_display | wrong | `select cast(1234567.891 as decimal(5,2))` | NULL | 1234567.891 |
| sql_gaps3 | decimal_display | wrong | `select cast(2.7 as decimal)` | 3 | 2.7 |
| sql_gaps3 | decimal_display | wrong | `select cast(3 as decimal(5,2))` | 3.00 | 3 |
| sql_gaps3 | decimal_display | error | `select typeof(cast(1 as decimal(5,2)))` | decimal(5,2) | ERR(invalid argument: unknown function: typeof) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select a from t lateral view inline(array(named_struct('a', id))) i where id <= 2 order by a` | 1\|2 | ERR(invalid argument: column not found: a) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select a, b from (select inline(array(named_struct('a', 1, 'b', 'x'), named_struct('a', 2, 'b', 'y')))) q order by a desc` | 2,y\|1,x | ERR(invalid argument: unknown function: inline) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select c0 from (select json_tuple('{"k":"v"}', 'k')) q` | v | ERR(invalid argument: unknown function: json_tuple) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select col0 from (select stack(3, 'x', 'y', 'z')) q order by col0` | x\|y\|z | ERR(invalid argument: unknown function: stack) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select inline(array(named_struct('a', 1, 'b', 'x'), named_struct('a', 2, 'b', 'y')))` | 1,x\|2,y | ERR(invalid argument: unknown function: inline) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select json_tuple('not json', 'a')` | NULL | ERR(invalid argument: unknown function: json_tuple) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select json_tuple('{"a":1,"b":"x","c":[1,2]}', 'a', 'b', 'c', 'd')` | 1,x,[1,2],NULL | ERR(invalid argument: unknown function: json_tuple) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select p1, q1 from t lateral view json_tuple('{"p":1,"q":2}', 'p', 'q') j as p1, q1 where id = 1` | 1,2 | ERR(invalid argument: column not found: p1) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select stack(2, 1, 'a', 2, 'b')` | 1,a\|2,b | ERR(invalid argument: unknown function: stack) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select stack(2, 1, 2, 3)` | 1,2\|3,NULL | ERR(invalid argument: unknown function: stack) |
| sql_gaps3 | generators_inline_stack_json_tuple | error | `select x, y from t lateral view stack(2, id, g, id * 10, 'q') s as x, y where id = 1 order by x` | 1,x\|10,q | ERR(invalid argument: column not found: x) |
| sql_gaps3 | groups_frames | error | `select x, count(*) over (order by x groups between unbounded preceding and current row) from (values (1),(1),(2),(3),(3),(3),(5)) as q(x)` | 1,2\|1,2\|2,3\|3,6\|3,6\|3,6\|5,7 | ERR(invalid argument: expected RParen, got Ident("groups")) |
| sql_gaps3 | groups_frames | error | `select x, sum(x) over (order by x groups 1 preceding) from (values (1),(1),(2),(3),(3),(3),(5)) as q(x)` | 1,2\|1,2\|2,4\|3,11\|3,11\|3,11\|5,14 | ERR(invalid argument: expected RParen, got Ident("groups")) |
| sql_gaps3 | groups_frames | error | `select x, sum(x) over (order by x groups between 1 following and 2 following) from (values (1),(1),(2),(3),(3),(3),(5)) as q(x)` | 1,11\|1,11\|2,14\|3,5\|3,5\|3,5\|5,NULL | ERR(invalid argument: expected RParen, got Ident("groups")) |
| sql_gaps3 | groups_frames | error | `select x, sum(x) over (order by x groups between 1 preceding and current row) from (values (1),(1),(2),(3),(3),(3),(5)) as q(x)` | 1,2\|1,2\|2,4\|3,11\|3,11\|3,11\|5,14 | ERR(invalid argument: expected RParen, got Ident("groups")) |
| sql_gaps3 | groups_frames | error | `select x, sum(x) over (order by x groups between current row and 1 following) from (values (1),(1),(2),(3),(3),(3),(5)) as q(x)` | 1,4\|1,4\|2,11\|3,14\|3,14\|3,14\|5,5 | ERR(invalid argument: expected RParen, got Ident("groups")) |
| sql_gaps3 | groups_frames | error | `select x, sum(x) over (order by x groups between unbounded preceding and 1 preceding) from (values (1),(1),(2),(3),(3),(3),(5)) as q(x)` | 1,NULL\|1,NULL\|2,2\|3,4\|3,4\|3,4\|5,13 | ERR(invalid argument: expected RParen, got Ident("groups")) |
| sql_gaps3 | struct_values | error | `select array(named_struct('a', 1), named_struct('a', 2))` | [{1}, {2}] | ERR(invalid argument: unknown function: array) |
| sql_gaps3 | struct_values | error | `select from_json('[1,2]', 'a INT, b STRING')` | NULL | ERR(invalid argument: unknown function: from_json) |
| sql_gaps3 | struct_values | error | `select from_json('not json', 'a INT, b STRING')` | NULL | ERR(invalid argument: unknown function: from_json) |
| sql_gaps3 | struct_values | error | `select from_json('{"a":1,"b":"x"}', 'a INT, b STRING')` | {1, x} | ERR(invalid argument: unknown function: from_json) |
| sql_gaps3 | struct_values | error | `select from_json('{"a":1,"b":"x"}', 'struct<a:int,b:string>').b` | x | ERR(invalid argument: unknown function: from_json) |
| sql_gaps3 | struct_values | error | `select from_json('{"a":1.5}', 'a DOUBLE').a` | 1.5 | ERR(invalid argument: unknown function: from_json) |
| sql_gaps3 | struct_values | error | `select from_json('{"a":1}', 'a INT, b STRING')` | {1, null} | ERR(invalid argument: unknown function: from_json) |
| sql_gaps3 | struct_values | error | `select from_json('{"a":{"c":[1,2]}}', 'a struct<c:array<int>>').a.c` | [1, 2] | ERR(invalid argument: unknown function: from_json) |
| sql_gaps3 | struct_values | error | `select named_struct('a', 1) = named_struct('a', 1)` | true | ERR(invalid argument: unknown function: named_struct) |
| sql_gaps3 | struct_values | error | `select named_struct('a', 1, 'b', 'x')` | {1, x} | ERR(invalid argument: unknown function: named_struct) |
| sql_gaps3 | struct_values | error | `select named_struct('a', 1, 'b', 'x').b` | x | ERR(invalid argument: unknown function: named_struct) |
| sql_gaps3 | struct_values | error | `select named_struct('a', 1, 'b', cast(null as string))` | {1, null} | ERR(invalid argument: unknown function: named_struct) |
| sql_gaps3 | struct_values | error | `select named_struct('a', array(1, 2), 'm', map('k', 1))` | {[1, 2], {k -> 1}} | ERR(invalid argument: unknown function: named_struct) |
| sql_gaps3 | struct_values | error | `select q.s.a from (select named_struct('a', 7) as s) q` | 7 | ERR(invalid argument: column not found: q.s) |
| sql_gaps3 | struct_values | error | `select s.a, s.b from (select named_struct('a', 1, 'b', 'x') as s) q` | 1,x | ERR(invalid argument: unknown function: named_struct) |
| sql_gaps3 | struct_values | error | `select s.id, s.g from (select struct(id, g) as s from t where id = 2) q` | 2,x | ERR(invalid argument: unknown function: struct) |
| sql_gaps3 | struct_values | error | `select s.n.k from (select named_struct('n', named_struct('k', 5)) as s) q` | 5 | ERR(invalid argument: column not found: s.n) |
| sql_gaps3 | struct_values | error | `select struct(1, 'a')` | {1, a} | ERR(invalid argument: unknown function: struct) |
| sql_gaps3 | struct_values | error | `select struct(1, 2).col2` | 2 | ERR(invalid argument: unknown function: struct) |
| sql_gaps3 | struct_values | error | `select struct(id, g) from t where id = 1` | {1, x} | ERR(invalid argument: unknown function: struct) |
| sql_gaps3 | struct_values | error | `select to_json(array(named_struct('a', 1)))` | [{"a":1}] | ERR(invalid argument: unknown function: to_json) |
| sql_gaps3 | struct_values | error | `select to_json(named_struct('a', 1, 'b', 'x'))` | {"a":1,"b":"x"} | ERR(invalid argument: unknown function: to_json) |
| sql_gaps3 | struct_values | error | `select to_json(named_struct('a', 1, 'b', cast(null as int)))` | {"a":1} | ERR(invalid argument: unknown function: to_json) |
| sql_gaps3 | tablesample_and_session_statements | wrong | `select count(*) from t tablesample (0 percent)` | 0 | 6 |
| sql_gaps3 | tablesample_and_session_statements | wrong | `select count(*) from t tablesample (3 rows)` | 3 | 6 |
| sql_gaps3 | tablesample_and_session_statements | error | `select s.id from t tablesample (2 rows) s order by s.id` | 1\|2 | ERR(invalid argument: column not found: s.id) |
| sql_gaps3 | tablesample_and_session_statements | error | `select sum(c) from (select count(*) c from t tablesample (bucket 1 out of 2) repeatable (3) union all select count(*) c from t tablesample (bucket 2 out of 2) repeatable (3)) q` | 6 | ERR(invalid argument: expected RParen, got LParen) |
| sql_gaps3 | tablesample_and_session_statements | accepts | `select count(*) from t tablesample (150 percent)` | ERR | 6 |
| sql_gaps3 | tablesample_and_session_statements | accepts | `select count(*) from t tablesample (bucket 3 out of 2)` | ERR | 6 |
| sql_gaps3 | window_function_time_windows | error | `select window(ts, '1 day', '1 day', '6 hours').start from (select timestamp '2024-01-01 05:00:00' ts) q` | 2023-12-31 06:00:00 | ERR(invalid argument: unknown function: window) |
| sql_gaps3 | window_function_time_windows | error | `select window(ts, '10 minutes') from (select timestamp '2024-01-01 00:03:00' ts) q` | {2024-01-01 00:00:00, 2024-01-01 00:10:00} | ERR(invalid argument: unknown function: window) |
| sql_gaps3 | window_function_time_windows | error | `select window(ts, '10 minutes').start, window(ts, '1 hour').end from (select timestamp '2024-01-01 05:43:21' ts) q` | 2024-01-01 05:40:00,2024-01-01 06:00:00 | ERR(invalid argument: unknown function: window) |
| sql_gaps3 | window_function_time_windows | error | `select window.start, count(*) c from (values (timestamp '2024-01-01 00:03:00'), (timestamp '2024-01-01 00:09:59'), (timestamp '2024-01-01 00:10:00')) as e(ts) group by window(ts, '10 minutes') having count(*) > 1` | 2024-01-01 00:00:00,2 | ERR(invalid argument: expected Select, got Ident("values")) |
| sql_gaps3 | window_function_time_windows | error | `select window.start, window.end, count(*) from (values (timestamp '2024-01-01 00:03:00'), (timestamp '2024-01-01 00:09:59'), (timestamp '2024-01-01 00:10:00')) as e(ts) group by window(ts, '10 minutes') order by 1` | 2024-01-01 00:00:00,2024-01-01 00:10:00,2\|2024-01-01 00:10:00,2024-01-01 00:20:00,1 | ERR(invalid argument: expected identifier, got End) |
