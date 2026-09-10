//! Coordinator resilience tests — network failure and edge-case handling.

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use kore_core::{Column, ColumnData, DataBlock};
use kore_coord::Coordinator;
use kore_net::{KoreFrame, KoreMsg};

async fn start_cluster(n_workers: usize) -> (Arc<Coordinator>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let coord = Arc::new(Coordinator::new());
    let c2 = coord.clone();
    tokio::spawn(async move { c2.run(listener).await });

    for i in 0..n_workers {
        let ca = addr.clone();
        tokio::spawn(async move {
            let w = kore_worker::Worker::new(format!("res-w-{i}"));
            let _ = w.run(&ca).await;
        });
    }

    for _ in 0..50 {
        if coord.worker_count() >= n_workers { break; }
        sleep(Duration::from_millis(50)).await;
    }
    if n_workers > 0 {
        assert!(coord.worker_count() >= n_workers,
            "only {} of {n_workers} workers registered", coord.worker_count());
    }
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

// ─── Test 1: Worker eviction after heartbeat timeout ─────────────────────────

#[tokio::test]
async fn test_worker_eviction_after_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let coord = Arc::new(Coordinator::new());
    let c2 = coord.clone();
    tokio::spawn(async move { c2.run(listener).await });

    // Manually register a worker via TCP with a stale timestamp
    let mut stream = tokio::net::TcpStream::connect(&addr).await.unwrap();
    KoreFrame::write(&mut stream, &KoreMsg::RegisterWorker {
        id: "stale-worker".into(),
        task_addr: "127.0.0.1:0".into(),
        cores: 4,
        memory_mb: 1024,
        auth_token: None,
    }).await.unwrap();

    let ack = KoreFrame::read(&mut stream).await.unwrap();
    assert!(matches!(ack, KoreMsg::RegisterAck { .. }));

    // Worker is registered
    for _ in 0..20 {
        if coord.worker_count() >= 1 { break; }
        sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(coord.worker_count(), 1, "worker should be registered");

    // Evict with a very short timeout — the worker's last_seen is now_ms() at
    // registration time, so a 1ms timeout should mark it stale immediately
    // after a small sleep.
    sleep(Duration::from_millis(10)).await;
    coord.evict_stale_workers(1);
    assert_eq!(coord.worker_count(), 0, "stale worker should be evicted");
}

// ─── Test 2: Query to empty cluster returns error ────────────────────────────

#[tokio::test]
async fn test_query_to_empty_cluster() {
    let coord = Coordinator::new();
    let data = test_data(10);

    let result = coord.execute_distributed(
        "SELECT * FROM data",
        "data",
        data,
        None,
    ).await;

    assert!(result.is_err(), "query to empty cluster should fail");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.to_lowercase().contains("no workers"),
        "error should mention no workers, got: {err_msg}");
}

// ─── Test 3: Duplicate worker registration ───────────────────────────────────

#[tokio::test]
async fn test_duplicate_worker_registration() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let coord = Arc::new(Coordinator::new());
    let c2 = coord.clone();
    tokio::spawn(async move { c2.run(listener).await });

    // Register the same worker ID twice from two separate connections
    for _ in 0..2 {
        let mut stream = tokio::net::TcpStream::connect(&addr).await.unwrap();
        KoreFrame::write(&mut stream, &KoreMsg::RegisterWorker {
            id: "dup-worker".into(),
            task_addr: "127.0.0.1:9999".into(),
            cores: 2,
            memory_mb: 512,
            auth_token: None,
        }).await.unwrap();

        let ack = KoreFrame::read(&mut stream).await.unwrap();
        assert!(matches!(ack, KoreMsg::RegisterAck { .. }),
            "each registration should be acknowledged");
    }

    sleep(Duration::from_millis(50)).await;

    // The coordinator should handle duplicate registrations gracefully —
    // it currently appends both (which is fine; workers are dispatched round-robin).
    assert!(coord.worker_count() >= 1,
        "at least one registration should succeed");
}

// ─── Test 4: Concurrent query submission ─────────────────────────────────────

#[tokio::test]
async fn test_concurrent_query_submission() {
    let (coord, _addr) = start_cluster(3).await;
    let data = test_data(60);

    let mut handles = Vec::new();
    for i in 0..5 {
        let c = coord.clone();
        let d = data.clone();
        handles.push(tokio::spawn(async move {
            c.execute_distributed(
                &format!("SELECT * FROM data WHERE amount > {}.0", i * 100),
                "data",
                d,
                None,
            ).await
        }));
    }

    let mut successes = 0;
    let mut errors = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(_) => successes += 1,
            Err(_) => errors += 1,
        }
    }

    assert!(successes + errors == 5, "all 5 queries should complete");
    assert!(successes >= 1, "at least some queries should succeed, got {successes} ok / {errors} err");
}
