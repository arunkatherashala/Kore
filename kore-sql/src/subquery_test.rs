// Minimal test of scalar subquery in kore-sql
// Add this as a unit test in executor.rs

#[cfg(test)]
mod subquery_tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn make_test_ctx() -> KqlContext {
        let mut ctx = KqlContext::new();
        let block = DataBlock::new(vec![
            Column::str_col("kind",      vec![Some("decision".into()), Some("decision".into()), Some("insight".into())]),
            Column::float64("importance",vec![Some(1.0), Some(0.9), Some(0.85)]),
        ]).unwrap();
        ctx.register("t", block);
        ctx
    }

    #[test]
    fn test_scalar_subquery_in_where() {
        let ctx = make_test_ctx();
        // SELECT importance FROM t WHERE importance = (SELECT MAX(importance) FROM t)
        let result = ctx.query("SELECT importance FROM t WHERE importance = (SELECT MAX(importance) FROM t)").unwrap();
        assert_eq!(result.num_rows, 1, "should return exactly the row with importance=1.0");
    }

    #[test]
    fn test_scalar_subquery_gt_avg() {
        let ctx = make_test_ctx();
        let result = ctx.query("SELECT importance FROM t WHERE importance > (SELECT AVG(importance) FROM t)").unwrap();
        // AVG = (1.0 + 0.9 + 0.85) / 3 = 0.9166..., so only 1.0 is > avg
        assert_eq!(result.num_rows, 1, "only importance=1.0 > avg(0.916)");
    }

    #[test]
    fn test_in_subquery() {
        let ctx = make_test_ctx();
        let result = ctx.query("SELECT importance FROM t WHERE kind IN (SELECT DISTINCT kind FROM t WHERE importance > 0.95)").unwrap();
        // Only kind='decision' has importance > 0.95 (the 1.0 row). So both decision rows returned.
        assert_eq!(result.num_rows, 2, "both decision rows should be returned via IN subquery");
    }

    #[test]
    fn test_exists_subquery() {
        let ctx = make_test_ctx();
        let result = ctx.query("SELECT importance FROM t WHERE EXISTS (SELECT 1 FROM t t2 WHERE t2.kind = t.kind AND t2.importance > 0.95)").unwrap();
        // Both decision rows have a matching row with importance > 0.95
        assert!(result.num_rows > 0, "EXISTS should return rows");
    }
}
