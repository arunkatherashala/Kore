//! "Engine needs" of World T (docs/world_t.md): the KORE SQL engine's behaviour that the World T tools
//! work around, pinned as it is today. If one of these starts failing, the engine changed: revisit the
//! workaround in `tools.rs` and the list in the doc. The engine crates are not modified by World T.

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
    // 1. COUNT(column) counts NULLs (standard SQL: 3); AVG ignores them. Workaround: COUNT(*) ... WHERE col IS NOT NULL.
    assert_eq!(q("SELECT COUNT(x) AS n, COUNT(*) AS c, AVG(x) AS a FROM t").unwrap(), vec![vec!["4".to_string(), "4".into(), "2.6666666666666665".into()]]);
    assert_eq!(q("SELECT COUNT(g) AS n FROM t").unwrap(), vec![vec!["4".to_string()]]);
    assert_eq!(q("SELECT COUNT(*) AS n FROM t WHERE x IS NOT NULL").unwrap(), vec![vec!["3".to_string()]]);
    // 2. an unknown function is not an error: it returns NULL for every row
    assert_eq!(q("SELECT NO_SUCH_FN(x) AS z FROM t").unwrap().len(), 4);
    assert!(q("SELECT NO_SUCH_FN(x) AS z FROM t").unwrap().iter().all(|r| r[0] == "NULL"));
    assert!(q("SELECT REGEXP_LIKE(g, 'a') AS r FROM t").unwrap().iter().all(|r| r[0] == "NULL"));
    // 3. quoted identifiers are not supported (a header with a space, a byte-order mark or a keyword cannot be named)
    assert!(q("SELECT \"d\" FROM t").is_err());
    // 4. PERCENTILE_CONT ... WITHIN GROUP does not parse (MEDIAN does)
    assert!(q("SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY x) AS m FROM t").is_err());
    assert_eq!(q("SELECT MEDIAN(x) AS m FROM t").unwrap(), vec![vec!["3".to_string()]]);
    // 5. a window function's missing values come back as NaN, not NULL
    let lag = q("SELECT x, LAG(x) OVER (ORDER BY d) AS p FROM t").unwrap();
    assert_eq!(lag[0][1], "NaN");
    // 6. groups whose sum is NULL sort first under ORDER BY ... DESC (as in PostgreSQL); NULLS LAST works.
    assert_eq!(q("SELECT g, SUM(x) AS s FROM t GROUP BY g ORDER BY s DESC").unwrap()[0][0], "b");
    assert_eq!(q("SELECT g, SUM(x) AS s FROM t GROUP BY g ORDER BY s DESC NULLS LAST").unwrap()[0][0], "a");
    // 7. a self join cannot name the second alias's columns
    assert!(q("SELECT a.x, b.x FROM t a CROSS JOIN t b").is_err());
}
