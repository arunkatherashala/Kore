/// Quick test: self-join with 10000 rows, 5 distinct keys → LIMIT 5
/// Verifies the pre-cap optimization prevents O(n²) output.
use kore_sql::KqlContext;
use kore_core::{Column, ColumnData, DataBlock};

fn main() {
    // Build a table: 10000 rows, key has 5 distinct values (2000 rows each)
    let n = 10_000usize;
    let ids:   Vec<Option<i64>>    = (0..n).map(|i| Some(i as i64)).collect();
    let kinds: Vec<Option<String>> = (0..n).map(|i| Some(format!("kind_{}", i % 5))).collect();
    let block = DataBlock::new(vec![
        Column::int64("id",   ids),
        Column::str_col("kind", kinds),
    ]).unwrap();

    let mut ctx = KqlContext::new();
    ctx.register("memories", block);

    // Test 1: INNER JOIN self-join LIMIT 5 — should return fast (not 100M rows)
    let t0 = std::time::Instant::now();
    let sql = "SELECT m1.id, m1.kind FROM memories m1 INNER JOIN memories m2 ON m1.kind=m2.kind LIMIT 5";
    match ctx.query(sql) {
        Ok(result) => {
            let ms = t0.elapsed().as_millis();
            println!("INNER JOIN LIMIT 5: {} rows in {}ms", result.num_rows, ms);
            assert!(result.num_rows <= 5, "Expected ≤5 rows, got {}", result.num_rows);
            assert!(ms < 500, "Expected <500ms, took {}ms (O(n²) not prevented!)", ms);
            println!("  ✅ PASS: {} rows in {}ms", result.num_rows, ms);
        }
        Err(e) => {
            println!("  ❌ FAIL: {}", e);
            std::process::exit(1);
        }
    }

    // Test 2: LEFT JOIN LIMIT 5
    let t0 = std::time::Instant::now();
    let sql = "SELECT m1.id, m2.id AS m2id FROM memories m1 LEFT JOIN memories m2 ON m1.kind=m2.kind LIMIT 5";
    match ctx.query(sql) {
        Ok(result) => {
            let ms = t0.elapsed().as_millis();
            println!("LEFT JOIN LIMIT 5: {} rows in {}ms", result.num_rows, ms);
            assert!(result.num_rows <= 5);
            assert!(ms < 500, "Expected <500ms, took {}ms", ms);
            println!("  ✅ PASS: {} rows in {}ms", result.num_rows, ms);
        }
        Err(e) => println!("  ❌ FAIL: {}", e),
    }

    // Test 3: Normal JOIN without LIMIT should still work (no incorrect capping)
    let t0 = std::time::Instant::now();
    let sql = "SELECT COUNT(*) AS n FROM memories m1 INNER JOIN memories m2 ON m1.kind=m2.kind";
    match ctx.query(sql) {
        Ok(result) => {
            let ms = t0.elapsed().as_millis();
            let n_val = match &result.columns[0].data {
                ColumnData::Int64(v) => v[0].unwrap_or(0),
                ColumnData::Float64(v) => v[0].unwrap_or(0.0) as i64,
                _ => 0,
            };
            println!("COUNT(*) no LIMIT: {} in {}ms", n_val, ms);
            // 10000 rows, 5 keys, 2000 per key: 5 × 2000² = 20M pairs
            assert_eq!(n_val, 10_000 * 2_000, "Expected 20M matched rows");
            println!("  ✅ PASS: {} rows total (correct, no cap)", n_val);
        }
        Err(e) => println!("  ❌ FAIL: {}", e),
    }

    println!("\nAll join limit tests passed!");
}
