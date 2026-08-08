//! Persistent cluster client — connect to a running kore-coord (Phase 1 distribution).

use kore_core::DataBlock;
use kore_coord::Coordinator;

use crate::plan;

/// Connect to an already-running coordinator and submit SQL (auto-planned).
pub async fn query_persistent_cluster(
    coord_addr: &str,
    sql: &str,
    table_name: &str,
    data: DataBlock,
) -> Result<DataBlock, String> {
    let p = plan(sql, table_name);
    query_persistent_cluster_planned(
        coord_addr,
        &p.map_sql,
        table_name,
        data,
        p.reduce_sql.as_deref(),
    )
    .await
}

/// Submit with an explicit map/reduce plan (no re-planning).
pub async fn query_persistent_cluster_planned(
    coord_addr: &str,
    map_sql: &str,
    table_name: &str,
    data: DataBlock,
    reduce_sql: Option<&str>,
) -> Result<DataBlock, String> {
    Coordinator::submit_query(coord_addr, map_sql, table_name, data, reduce_sql)
        .await
        .map_err(|e| e.to_string())
}

/// Blocking wrapper for sync callers.
pub fn query_persistent_cluster_blocking(
    coord_addr: &str,
    sql: &str,
    table_name: &str,
    data: DataBlock,
) -> Result<DataBlock, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(query_persistent_cluster(coord_addr, sql, table_name, data))
}

/// Blocking wrapper with explicit map/reduce plan.
pub fn query_persistent_cluster_blocking_planned(
    coord_addr: &str,
    map_sql: &str,
    table_name: &str,
    data: DataBlock,
    reduce_sql: Option<&str>,
) -> Result<DataBlock, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(query_persistent_cluster_planned(
            coord_addr,
            map_sql,
            table_name,
            data,
            reduce_sql,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::types::{Column, ColumnData, DataBlock};
    use tokio::net::TcpListener;
    use tokio::time::{sleep, Duration};

    fn sales_data() -> DataBlock {
        DataBlock {
            num_rows: 6,
            columns: vec![
                Column {
                    name: "region".into(),
                    data: ColumnData::Str(vec![
                        Some("EU".into()),
                        Some("US".into()),
                        Some("EU".into()),
                        Some("US".into()),
                        Some("AP".into()),
                        Some("EU".into()),
                    ]),
                },
                Column {
                    name: "sales".into(),
                    data: ColumnData::Float64(vec![
                        Some(100.0),
                        Some(200.0),
                        Some(150.0),
                        Some(300.0),
                        Some(120.0),
                        Some(80.0),
                    ]),
                },
            ],
        }
    }

    #[tokio::test]
    async fn persistent_cluster_submit_query() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let coord = std::sync::Arc::new(Coordinator::new());
        let c2 = coord.clone();
        tokio::spawn(async move { c2.run(listener).await });

        for i in 0..2 {
            let ca = addr.clone();
            tokio::spawn(async move {
                let w = kore_worker::Worker::new(format!("pw-{i}"));
                let _ = w.run(&ca).await;
            });
        }

        for _ in 0..40 {
            if coord.worker_count() >= 2 {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(coord.worker_count() >= 2);

        let result = query_persistent_cluster(
            &addr,
            "SELECT region, SUM(sales) AS total FROM sales GROUP BY region",
            "sales",
            sales_data(),
        )
        .await
        .unwrap();

        assert_eq!(result.num_rows, 3);
    }
}
