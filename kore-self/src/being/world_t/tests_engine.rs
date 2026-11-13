//! "Engine needs" of World T (docs/world_t.md): the KORE SQL engine's behaviour that the World T tools
//! depend on or work around, pinned as it is today. If one of these starts failing, the engine changed: revisit
//! `tools.rs` and the list in the doc. Items 1, 2 and 5 were fixed in this copy of the engine (branch dl-sql-fix,
//! NULL semantics as on master-kore-engine) and are pinned at the correct behaviour; the World T COUNT workaround
//! was removed. Items 2b, 3, 4, 6 and 7 changed with the engine sync from master-kore-engine 236ae86c (2026-11-13,
//! docs/engine_sync.md); `tools.rs` relies on none of the old behaviours (it never names a quoted column, never
//! sorts a NULL sum, never self-joins).

use super::tools::run_sql;
use kore_core::{Column, DataBlock};

fn t() -> DataBlock {
    DataBlock::new(vec![
        Column::float64("x", vec![Some(1.0), None, Some(3.0), Some(4.0)]),
        Column::str_col("g", vec![Some("a".into()), Some("b".into()), Some("a".into()), None]),
        Column::int64("d", vec![Some(1), Some(2), Some(3), Some(4)]),
    ])
    .unwrap()
}

fn q(sql: &str) -> Result<Vec<Vec<String>>, String> {
    run_sql(t(), sql).map(|b| crate::versioned::sql::block_strings(&b))
}

#[test]
fn engine_behaviour_the_world_t_tools_work_around() {
    // 1. (fixed) COUNT(column) counts the non-NULL values (standard SQL: 3), COUNT(*) the rows; AVG ignores NULLs.
    assert_eq!(q("SELECT COUNT(x) AS n, COUNT(*) AS c, AVG(x) AS a FROM t").unwrap(), vec![vec!["3".to_string(), "4".into(), "2.6666666666666665".into()]]);
    assert_eq!(q("SELECT COUNT(g) AS n FROM t").unwrap(), vec![vec!["3".to_string()]]);
    assert_eq!(q("SELECT COUNT(*) AS n FROM t WHERE x IS NOT NULL").unwrap(), vec![vec!["3".to_string()]]);
    // the mean plan's query (Tmpl::ColMean) without the old workaround
    assert_eq!(q("SELECT SUM(x) AS s, COUNT(x) AS n FROM t").unwrap(), vec![vec!["8".to_string(), "3".into()]]);
    // 2. (fixed) an unknown function is an error that names it as written, not NULL on every row.
    for (sql, f) in [("SELECT NO_SUCH_FN(x) AS z FROM t", "NO_SUCH_FN"), ("SELECT d FROM t WHERE no_such_fn(x) = 1", "no_such_fn")] {
        let e = q(sql).unwrap_err();
        assert!(e.contains(&format!("unknown function: {f}")), "{sql}: {e}");
    }
    // 2b. (engine sync, master 236ae86c) REGEXP_LIKE exists; NULL input gives NULL
    assert_eq!(q("SELECT REGEXP_LIKE(g, 'a') AS r FROM t ORDER BY d").unwrap().concat(), ["true", "false", "true", "NULL"]);
    // 3. (engine sync) a double-quoted token is a string literal (Spark style), not a column name: quoted
    // identifiers are still not supported, so a header with a space, a byte-order mark or a keyword cannot be named
    assert_eq!(q("SELECT \"d\" AS v FROM t ORDER BY d").unwrap().concat(), ["d", "d", "d", "d"]);
    // 4. (engine sync) PERCENTILE_CONT ... WITHIN GROUP parses; MEDIAN still works
    assert_eq!(q("SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY x) AS m FROM t").unwrap(), vec![vec!["3".to_string()]]);
    assert_eq!(q("SELECT MEDIAN(x) AS m FROM t").unwrap(), vec![vec!["3".to_string()]]);
    // 5. (fixed) a window function's missing values are NULL, not NaN
    let lag = q("SELECT x, LAG(x) OVER (ORDER BY d) AS p FROM t ORDER BY d").unwrap();
    assert_eq!(lag.iter().map(|r| r[1].as_str()).collect::<Vec<_>>(), ["NULL", "1", "NULL", "3"]);
    // 6. (engine sync) NULL is the smallest value (Spark default): a NULL sum sorts last under DESC; NULLS FIRST works
    assert_eq!(q("SELECT g, SUM(x) AS s FROM t GROUP BY g ORDER BY s DESC").unwrap()[2][0], "b");
    assert_eq!(q("SELECT g, SUM(x) AS s FROM t GROUP BY g ORDER BY s DESC NULLS FIRST").unwrap()[0][0], "b");
    assert_eq!(q("SELECT g, SUM(x) AS s FROM t GROUP BY g ORDER BY s DESC NULLS LAST").unwrap()[2][0], "b");
    // 7. (engine sync) a self CROSS JOIN works and can name both aliases' columns
    assert_eq!(q("SELECT COUNT(*) AS n FROM t a CROSS JOIN t b").unwrap(), vec![vec!["16".to_string()]]);
    assert_eq!(q("SELECT a.x AS ax, b.x AS bx FROM t a CROSS JOIN t b ORDER BY a.d, b.d LIMIT 2").unwrap(), vec![vec!["1".to_string(), "1".into()], vec!["1".to_string(), "NULL".into()]]);
}
