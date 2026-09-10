//! Multi-node cluster integration tests.

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use kore_core::{Column, ColumnData, DataBlock};
use kore_coord::Coordinator;

async fn start_cluster(n_workers: usize) -> (Arc<Coordinator>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let coord = Arc::new(Coordinator::new());
    let c2 = coord.clone();
    tokio::spawn(async move { c2.run(listener).await });

    for i in 0..n_workers {
        let ca = addr.clone();
        tokio::spawn(async move {
            let w = kore_worker::Worker::new(format!("test-w-{i}"));
            let _ = w.run(&ca).await;
        });
    }

    for _ in 0..50 {
        if coord.worker_count() >= n_workers { break; }
        sleep(Duration::from_millis(50)).await;
    }
    assert!(coord.worker_count() >= n_workers,
        "only {} of {n_workers} workers registered", coord.worker_count());
    (coord, addr)
}

fn test_data(n: usize) -> DataBlock {
    DataBlock {
        num_rows: n,
        columns: vec![
            Column { name: "id".into(), data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
            Column { name: "region".into(), data: ColumnData::Str((0..n).map(|i| Some(["US","EU","AP"][i%3].into())).collect()) },
            Column { name: "amount".into(), data: ColumnData::Float64((0..n).map(|i| Some((i as f64) * 10.0 + 5.0)).collect()) },
        ],
    }
}

// ─── Test 1: 3-worker distributed filter ─────────────────────────────────────

#[tokio::test]
async fn test_3_worker_filter() {
    let (coord, _addr) = start_cluster(3).await;
    let data = test_data(90);

    let result = coord.execute_distributed(
        "SELECT * FROM data WHERE amount > 500.0",
        "data",
        data,
        None,
    ).await.unwrap();

    // amount = i*10+5; amount > 500 when i*10+5 > 500 => i > 49.5 => i >= 50
    // So rows with id 50..89 → 40 rows
    assert_eq!(result.num_rows, 40,
        "filter should yield 40 rows (id 50..89), got {}", result.num_rows);
}

// ─── Test 2: Distributed GROUP BY with reduce phase ──────────────────────────

#[tokio::test]
async fn test_distributed_group_by() {
    let (coord, _addr) = start_cluster(3).await;
    let data = test_data(90);

    let result = coord.execute_distributed(
        "SELECT region, SUM(amount) AS total FROM data GROUP BY region",
        "data",
        data,
        Some("SELECT region, SUM(total) AS total FROM merged GROUP BY region"),
    ).await.unwrap();

    assert_eq!(result.num_rows, 3,
        "GROUP BY region should produce 3 groups, got {}", result.num_rows);
}

// ─── Test 3: Broadcast join ──────────────────────────────────────────────────

#[tokio::test]
async fn test_broadcast_join() {
    std::env::set_var("KORE_CLUSTER_LOCAL", "1");
    let (coord, _addr) = start_cluster(3).await;

    let dim = DataBlock {
        num_rows: 3,
        columns: vec![
            Column { name: "region".into(),
                data: ColumnData::Str(vec![
                    Some("US".into()), Some("EU".into()), Some("AP".into()),
                ]) },
            Column { name: "manager".into(),
                data: ColumnData::Str(vec![
                    Some("Alice".into()), Some("Bob".into()), Some("Charlie".into()),
                ]) },
        ],
    };

    let fact = test_data(30);

    // Local reference: every fact row matches exactly one dim row → 30 rows
    let mut ctx = kore_sql::executor::KqlContext::new();
    ctx.register("fact", fact.clone());
    ctx.register("dim", dim.clone());
    let expected = ctx.query(
        "SELECT * FROM fact INNER JOIN dim ON fact.region = dim.region"
    ).expect("local join");

    let result = coord.execute_broadcast_join(
        "SELECT * FROM fact INNER JOIN dim ON fact.region = dim.region",
        "fact", fact,
        "dim", dim,
    ).await.expect("broadcast join");

    assert_eq!(result.num_rows, expected.num_rows,
        "broadcast join ({}) should match local join ({})",
        result.num_rows, expected.num_rows);
    std::env::remove_var("KORE_CLUSTER_LOCAL");
}

// ─── Test 4: DAG explain + planned execution (single-table) ────────────────

#[tokio::test]
async fn test_dag_planned_execution() {
    let (coord, _addr) = start_cluster(2).await;

    let data = test_data(100);

    // Verify DAG explain produces valid output
    coord.register_table_for_planning("items", data.clone());
    let explain = coord.explain_dag("SELECT region, SUM(amount) AS total FROM items GROUP BY region")
        .expect("dag explain");
    assert!(explain.contains("Stage"), "DAG explain should contain stages: {explain}");

    // Test planned execution with single-table aggregation
    let result = coord.execute_distributed(
        "SELECT region, SUM(amount) AS total FROM items GROUP BY region",
        "items",
        data,
        Some("SELECT region, SUM(total) AS total FROM merged GROUP BY region"),
    ).await.expect("planned aggregation");

    assert_eq!(result.num_rows, 3, "should have 3 regions");
}

// ─── Test 5: Concurrent queries ─────────────────────────────────────────────

#[tokio::test]
async fn test_concurrent_queries() {
    let (coord, _addr) = start_cluster(3).await;
    let data = test_data(60);

    let c1 = coord.clone();
    let c2 = coord.clone();
    let c3 = coord.clone();
    let d1 = data.clone();
    let d2 = data.clone();
    let d3 = data.clone();

    let (r1, r2, r3) = tokio::join!(
        async move {
            c1.execute_distributed(
                "SELECT * FROM data WHERE amount > 300.0",
                "data", d1, None,
            ).await
        },
        async move {
            c2.execute_distributed(
                "SELECT region, SUM(amount) AS total FROM data GROUP BY region",
                "data", d2,
                Some("SELECT region, SUM(total) AS total FROM merged GROUP BY region"),
            ).await
        },
        async move {
            c3.execute_distributed(
                "SELECT * FROM data WHERE region = 'US'",
                "data", d3, None,
            ).await
        },
    );

    let r1 = r1.expect("query 1 (filter amount > 300)");
    let r2 = r2.expect("query 2 (group by region)");
    let r3 = r3.expect("query 3 (filter region=US)");

    // amount > 300: i*10+5 > 300 => i > 29.5 => i >= 30 → rows 30..59 → 30 rows
    assert_eq!(r1.num_rows, 30,
        "concurrent query 1: expected 30, got {}", r1.num_rows);
    // 3 distinct regions
    assert_eq!(r2.num_rows, 3,
        "concurrent query 2: expected 3 groups, got {}", r2.num_rows);
    // region pattern: US at indices 0,3,6,9,... → 60/3 = 20 rows
    assert_eq!(r3.num_rows, 20,
        "concurrent query 3: expected 20 US rows, got {}", r3.num_rows);
}

// ─── Test 6: Large data (100K rows) GROUP BY ────────────────────────────────

#[tokio::test]
async fn test_large_data_100k() {
    let (coord, _addr) = start_cluster(3).await;
    let data = test_data(100_000);

    let result = coord.execute_distributed(
        "SELECT region, SUM(amount) AS total, COUNT(*) AS cnt FROM data GROUP BY region",
        "data",
        data,
        Some("SELECT region, SUM(total) AS total, SUM(cnt) AS cnt FROM merged GROUP BY region"),
    ).await.unwrap();

    assert_eq!(result.num_rows, 3,
        "100K rows GROUP BY region should produce 3 groups, got {}", result.num_rows);

    // Verify the count column sums to 100_000
    let cnt_col = result.columns.iter().find(|c| c.name == "cnt").expect("cnt column");
    let total_count: i64 = match &cnt_col.data {
        ColumnData::Int64(vals) => vals.iter().filter_map(|v| *v).sum(),
        ColumnData::Float64(vals) => vals.iter().filter_map(|v| *v).sum::<f64>() as i64,
        other => panic!("unexpected cnt column type: {:?}", other),
    };
    assert_eq!(total_count, 100_000,
        "total count across groups should be 100000, got {}", total_count);
}

// ─── Test 7: Scaling from 1 to 4 workers ────────────────────────────────────

#[tokio::test]
async fn test_scaling_1_to_4_workers() {
    let sql = "SELECT region, SUM(amount) AS total FROM data GROUP BY region";
    let reduce = "SELECT region, SUM(total) AS total FROM merged GROUP BY region";

    let mut results = Vec::new();

    for n_workers in [1, 2, 4] {
        let (coord, _addr) = start_cluster(n_workers).await;
        let data = test_data(120);

        let result = coord.execute_distributed(
            sql, "data", data, Some(reduce),
        ).await.unwrap_or_else(|e| panic!("failed with {n_workers} workers: {e}"));

        assert_eq!(result.num_rows, 3,
            "with {n_workers} workers: expected 3 groups, got {}", result.num_rows);
        results.push(result);
    }

    // All configurations should produce the same totals
    let extract_totals = |block: &DataBlock| -> Vec<f64> {
        let total_col = block.columns.iter()
            .find(|c| c.name == "total")
            .expect("total column");
        match &total_col.data {
            ColumnData::Float64(vals) => {
                let mut v: Vec<f64> = vals.iter().filter_map(|x| *x).collect();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v
            }
            other => panic!("unexpected total type: {:?}", other),
        }
    };

    let baseline = extract_totals(&results[0]);
    for (i, r) in results.iter().enumerate().skip(1) {
        let totals = extract_totals(r);
        assert_eq!(totals.len(), baseline.len(),
            "scaling test: result {i} group count mismatch");
        for (a, b) in baseline.iter().zip(totals.iter()) {
            assert!((a - b).abs() < 0.01,
                "scaling test: result {i} total mismatch {a} vs {b}");
        }
    }
}
