//! "Engine needs" of World T (docs/world_t.md): the KORE SQL engine's behaviour that the World T tools
//! depend on or work around, pinned as it is today. If one of these starts failing, the engine changed: revisit
//! `tools.rs` and the list in the doc. Items 1, 2 and 5 were fixed in this copy of the engine (branch dl-sql-fix,
//! NULL semantics as on master-kore-engine) and are pinned at the correct behaviour; the World T COUNT workaround
//! was removed.

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
    // 2. (fixed) an unknown function is an error that names it as written, not NULL on every row. There is still
    // no regular expression function in this copy.
    for (sql, f) in [("SELECT NO_SUCH_FN(x) AS z FROM t", "NO_SUCH_FN"), ("SELECT REGEXP_LIKE(g, 'a') AS r FROM t", "REGEXP_LIKE"), ("SELECT d FROM t WHERE no_such_fn(x) = 1", "no_such_fn")] {
        let e = q(sql).unwrap_err();
        assert!(e.contains(&format!("unknown function: {f}")), "{sql}: {e}");
    }
    // 3. quoted identifiers are not supported (a header with a space, a byte-order mark or a keyword cannot be named)
    assert!(q("SELECT \"d\" FROM t").is_err());
    // 4. PERCENTILE_CONT ... WITHIN GROUP does not parse (MEDIAN does)
    assert!(q("SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY x) AS m FROM t").is_err());
    assert_eq!(q("SELECT MEDIAN(x) AS m FROM t").unwrap(), vec![vec!["3".to_string()]]);
    // 5. (fixed) a window function's missing values are NULL, not NaN
    let lag = q("SELECT x, LAG(x) OVER (ORDER BY d) AS p FROM t ORDER BY d").unwrap();
    assert_eq!(lag.iter().map(|r| r[1].as_str()).collect::<Vec<_>>(), ["NULL", "1", "NULL", "3"]);
    // 6. groups whose sum is NULL sort first under ORDER BY ... DESC (as in PostgreSQL); NULLS LAST works.
    assert_eq!(q("SELECT g, SUM(x) AS s FROM t GROUP BY g ORDER BY s DESC").unwrap()[0][0], "b");
    assert_eq!(q("SELECT g, SUM(x) AS s FROM t GROUP BY g ORDER BY s DESC NULLS LAST").unwrap()[0][0], "a");
    // 7. a self join cannot name the second alias's columns
    assert!(q("SELECT a.x, b.x FROM t a CROSS JOIN t b").is_err());
}
