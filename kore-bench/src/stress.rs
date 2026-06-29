//! kore-stress — Push KORE to its limits.
//!
//! Tests cover: join explosion (key skew), cache thrashing, large-scale ML,
//! deep pipelines, cluster throughput, sort-merge on 500K rows, and matview
//! stale-lookup under heavy epoch pressure.
//!
//! Status thresholds (release build):
//!   BLAZING  < 10 ms
//!   FAST     < 100 ms
//!   OK       < 1 000 ms
//!   SLOW     < 10 000 ms
//!   CRITICAL ≥ 10 000 ms

use std::time::{Duration, Instant};
use kore_core::{Column, DataBlock, JoinType};
use kore_join::{BroadcastJoin, HashJoin, JoinConfig, SortMergeJoin};
use kore_cache::{MatViewRegistry};
use kore_ml2::{GaussianNaiveBayes, GradientBoostingRegressor, RandomForestClassifier, RandomForestRegressor, Rng};
use kore_pipeline::{Pipeline, StandardScaler, MinMaxScaler};
use kore_cluster::{Coordinator, Worker, protocol::Operation};

// ─── Result type ──────────────────────────────────────────────────────────────

struct SR {
    tag:      &'static str,
    name:     String,
    rows_in:  usize,
    rows_out: usize,
    dur:      Duration,
    note:     String,
}

impl SR {
    fn status(&self) -> &'static str {
        let ms = self.dur.as_millis();
        match ms {
            0..=9      => "BLAZING  ⚡",
            10..=99    => "FAST     ✓",
            100..=999  => "OK       ✓",
            1000..=9999 => "SLOW     ⚠",
            _           => "CRITICAL ✗",
        }
    }
}

fn sr(tag: &'static str, name: &str, rows_in: usize, rows_out: usize, dur: Duration, note: &str) -> SR {
    SR { tag, name: name.into(), rows_in, rows_out, dur, note: note.into() }
}

// ─── Data generators ──────────────────────────────────────────────────────────

fn gen_block(n: usize, key_range: i64) -> DataBlock {
    let mut rng = Rng::new(42);
    DataBlock::new(vec![
        Column::int64("id",    (0..n).map(|_| Some(rng.next_u64() as i64 % key_range.max(1))).collect()),
        Column::float64("val", (0..n).map(|_| Some(rng.next_f64() * 1000.0)).collect()),
        Column::float64("v2",  (0..n).map(|_| Some(rng.next_f64() * 500.0)).collect()),
    ]).unwrap()
}

fn gen_same_key_block(n: usize) -> DataBlock {
    DataBlock::new(vec![
        Column::int64("id",    vec![Some(1); n]),
        Column::float64("val", (0..n).map(|i| Some(i as f64)).collect()),
    ]).unwrap()
}

fn gen_ml_matrix(n: usize, feats: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut rng = Rng::new(7);
    let x: Vec<Vec<f64>> = (0..n).map(|_| (0..feats).map(|_| rng.next_f64() * 10.0).collect()).collect();
    let y: Vec<f64>      = x.iter().map(|r| if r.iter().sum::<f64>() > feats as f64 * 5.0 { 1.0 } else { 0.0 }).collect();
    (x, y)
}

fn gen_reg_matrix(n: usize, feats: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut rng = Rng::new(13);
    let x: Vec<Vec<f64>> = (0..n).map(|_| (0..feats).map(|_| rng.next_f64() * 100.0).collect()).collect();
    let y: Vec<f64>      = x.iter().map(|r| r.iter().sum::<f64>() / feats as f64).collect();
    (x, y)
}

// ─── Stress tests ─────────────────────────────────────────────────────────────

// ── Layer 15 — KoreJoin ────────────────────────────────────────────────────────

fn t_join_skew() -> SR {
    // All rows share key=1 → n² output explosion (1K×1K = 1M rows)
    let left  = gen_same_key_block(1_000);
    let right = gen_same_key_block(1_000);
    let cfg   = JoinConfig::inner("id", "id");
    let t     = Instant::now();
    let out   = HashJoin::join(&left, &right, &cfg).unwrap();
    sr("JOIN", "HashJoin key-skew 1K×1K (1M output)", 2_000, out.num_rows, t.elapsed(),
        &format!("output={} rows — join explosion test", out.num_rows))
}

fn t_join_large() -> SR {
    let left  = gen_block(500_000, 200_000);
    let right = gen_block(100_000, 200_000);
    let cfg   = JoinConfig::inner("id", "id");
    let t     = Instant::now();
    let out   = HashJoin::join(&left, &right, &cfg).unwrap();
    sr("JOIN", "HashJoin INNER 500K×100K random keys", 600_000, out.num_rows, t.elapsed(),
        &format!("matched={} rows", out.num_rows))
}

fn t_broadcast_large() -> SR {
    let large = gen_block(1_000_000, 500_000);
    let small = gen_block(5_000,     500_000);
    let cfg   = JoinConfig::inner("id", "id");
    let t     = Instant::now();
    let out   = BroadcastJoin::join(&large, &small, &cfg).unwrap();
    sr("JOIN", "BroadcastJoin 1M×5K", 1_005_000, out.num_rows, t.elapsed(),
        &format!("matched={} rows", out.num_rows))
}

fn t_smj_large() -> SR {
    let left  = gen_block(200_000, 100_000);
    let right = gen_block(100_000, 100_000);
    let cfg   = JoinConfig::new("id", "id", JoinType::Full);
    let t     = Instant::now();
    let out   = SortMergeJoin::join(&left, &right, &cfg).unwrap();
    sr("JOIN", "SortMergeJoin FULL OUTER 200K×100K", 300_000, out.num_rows, t.elapsed(),
        &format!("output={} rows (incl. unmatched)", out.num_rows))
}

fn t_smj_skew() -> SR {
    // Skewed: 20 groups, 200 rows each side → 20 × 200² = 800K output rows (manageable)
    let n_groups  = 20usize;
    let per_group = 200usize;
    let total     = n_groups * per_group;
    let mut rng   = Rng::new(55);
    let left = DataBlock::new(vec![
        Column::int64("id", (0..total).map(|i| Some((i / per_group) as i64)).collect()),
        Column::float64("v", (0..total).map(|_| Some(rng.next_f64())).collect()),
    ]).unwrap();
    let right = DataBlock::new(vec![
        Column::int64("id", (0..total).map(|i| Some((i / per_group) as i64)).collect()),
        Column::float64("w", (0..total).map(|_| Some(rng.next_f64())).collect()),
    ]).unwrap();
    let cfg = JoinConfig::inner("id", "id");
    let t   = Instant::now();
    let out = SortMergeJoin::join(&left, &right, &cfg).unwrap();
    // Each group: 200×200=40K → 20 groups = 800K rows
    sr("JOIN", "SortMergeJoin skewed 20 groups×200/side", total * 2, out.num_rows, t.elapsed(),
        &format!("output={} rows (group cross-product)", out.num_rows))
}

// ── Layer 16 — KoreCache ──────────────────────────────────────────────────────

fn t_lru_thrash() -> SR {
    // capacity=100, insert 50K items → constant eviction churn
    let mut cache = kore_cache::LruCache::<String, u64>::new(100);
    let t = Instant::now();
    for i in 0u64..50_000 {
        cache.put(format!("key_{}", i), i * i);
    }
    // Read back last 100 (should all hit)
    let mut hits = 0usize;
    for i in 49_900u64..50_000 {
        if cache.get(&format!("key_{}", i)).is_some() { hits += 1; }
    }
    sr("CACHE", "LRU thrash cap=100 50K puts + 100 reads", 50_100, hits,
        t.elapsed(), &format!("last-100 hit rate={}/100", hits))
}

fn t_lru_large_cap() -> SR {
    let mut cache = kore_cache::LruCache::<u64, Vec<u8>>::new(10_000);
    let payload   = vec![0u8; 1_024];  // 1 KB per entry
    let t = Instant::now();
    for i in 0u64..10_000 {
        cache.put(i, payload.clone());
    }
    let mut hits = 0usize;
    for i in 0u64..10_000 {
        if cache.get(&i).is_some() { hits += 1; }
    }
    sr("CACHE", "LRU 10K entries × 1KB payloads (10MB)", 20_000, hits,
        t.elapsed(), &format!("hit_rate={:.1}%", cache.hit_rate() * 100.0))
}

fn t_matview_pressure() -> SR {
    let mut reg = MatViewRegistry::new();
    // Create 200 views
    for i in 0..200 {
        reg.create_or_replace(&format!("view_{}", i), gen_block(10_000, 5000), Some(50), "q");
    }
    let t = Instant::now();
    // Tick 1000 times, check all views each tick
    let mut total_reads = 0usize;
    let mut stale_count = 0usize;
    for _ in 0..1_000 {
        reg.tick();
        for i in 0..200 {
            total_reads += 1;
            if reg.get(&format!("view_{}", i)).is_none() { stale_count += 1; }
        }
    }
    sr("CACHE", "MatView 200 views × 1K epoch ticks", total_reads, stale_count,
        t.elapsed(), &format!("stale_reads={}/{}", stale_count, total_reads))
}

// ── Layer 17 — KoreML2 ────────────────────────────────────────────────────────

fn t_rf_deep() -> SR {
    let (x, y) = gen_ml_matrix(1_500, 8);
    let mut rf = RandomForestClassifier::new(25, 5);
    let t      = Instant::now();
    rf.fit_raw(&x, &y);
    let preds = rf.predict_raw(&x);
    let acc   = preds.iter().zip(y.iter()).filter(|(&p, &t)| (p-t).abs() < 0.5).count() as f64 / x.len() as f64;
    sr("ML2", "RandomForest clf 25 trees depth=5 n=1.5K feats=8", 1_500, 1_500,
        t.elapsed(), &format!("accuracy={:.1}%", acc * 100.0))
}

fn t_rf_massive() -> SR {
    let (x, y) = gen_reg_matrix(2_000, 6);
    let mut rf = RandomForestRegressor::new(30, 5);
    let t      = Instant::now();
    rf.fit_raw(&x, &y);
    let preds = rf.predict_raw(&x);
    let mse: f64 = preds.iter().zip(y.iter()).map(|(&p, &t)| (p-t).powi(2)).sum::<f64>() / x.len() as f64;
    sr("ML2", "RandomForest reg 30 trees depth=5 n=2K feats=6", 2_000, 2_000,
        t.elapsed(), &format!("MSE={:.4}", mse))
}

fn t_gbm_long() -> SR {
    let (x, y) = gen_reg_matrix(1_500, 5);
    let mut gbm = GradientBoostingRegressor::new(100, 0.05, 3);
    let t       = Instant::now();
    gbm.fit_raw(&x, &y);
    let preds = gbm.predict_raw(&x);
    let rmse  = (preds.iter().zip(y.iter()).map(|(&p, &t)| (p-t).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
    sr("ML2", "GradientBoosting 100 iters depth=3 n=1.5K feats=5", 1_500, 1_500,
        t.elapsed(), &format!("RMSE={:.4}", rmse))
}

fn t_nb_million() -> SR {
    let (x, y) = gen_ml_matrix(500_000, 8);
    let mut gnb = GaussianNaiveBayes::new();
    let t       = Instant::now();
    gnb.fit_raw(&x, &y);
    let preds = gnb.predict_raw(&x);
    let acc   = preds.iter().zip(y.iter()).filter(|(&p, &t)| (p-t).abs() < 0.5).count() as f64 / x.len() as f64;
    sr("ML2", "GaussianNaiveBayes fit+predict n=500K feats=8", 500_000, 500_000,
        t.elapsed(), &format!("accuracy={:.1}%", acc * 100.0))
}

// ── Layer 18 — KorePipeline ───────────────────────────────────────────────────

fn t_pipeline_deep() -> SR {
    let n = 3_000usize;
    let mut rng = Rng::new(99);
    let block = DataBlock::new(vec![
        Column::float64("x1", (0..n).map(|_| Some(rng.next_f64() * 1000.0)).collect()),
        Column::float64("x2", (0..n).map(|_| Some(rng.next_f64() * 500.0)).collect()),
        Column::float64("x3", (0..n).map(|_| Some(rng.next_f64() * 250.0)).collect()),
        Column::float64("x4", (0..n).map(|_| Some(rng.next_f64() * 100.0)).collect()),
        Column::float64("y",  (0..n).map(|i| Some(i as f64 * 0.75 + 1.0)).collect()),
    ]).unwrap();
    let t = Instant::now();
    let mut pipe = Pipeline::new("y");
    pipe.add_transformer(Box::new(StandardScaler::all_numeric()));
    pipe.add_transformer(Box::new(MinMaxScaler::all_numeric()));
    pipe.set_estimator(Box::new(GradientBoostingRegressor::new(50, 0.1, 3)));
    pipe.fit(&block).unwrap();
    let preds = pipe.predict(&block).unwrap();
    sr("PIPE", "StandardScaler→MinMaxScaler→GBM(50) n=3K feats=4", n, preds.len(),
        t.elapsed(), "3-stage pipeline fit+predict")
}

fn t_pipeline_rf() -> SR {
    let n = 2_000usize;
    let mut rng = Rng::new(11);
    let block = DataBlock::new(vec![
        Column::float64("a", (0..n).map(|_| Some(rng.next_f64() * 200.0)).collect()),
        Column::float64("b", (0..n).map(|_| Some(rng.next_f64() * 100.0)).collect()),
        Column::float64("c", (0..n).map(|_| Some(rng.next_f64() * 50.0)).collect()),
        Column::float64("y", (0..n).map(|i| Some(if i % 2 == 0 { 0.0 } else { 1.0 })).collect()),
    ]).unwrap();
    let t = Instant::now();
    let mut pipe = Pipeline::new("y");
    pipe.add_transformer(Box::new(StandardScaler::all_numeric()));
    pipe.set_estimator(Box::new(RandomForestClassifier::new(20, 5)));
    pipe.fit(&block).unwrap();
    let preds = pipe.predict(&block).unwrap();
    sr("PIPE", "StandardScaler→RF(20) clf n=2K feats=3", n, preds.len(),
        t.elapsed(), "2-stage pipeline fit+predict")
}

// ── Layer 19 — KoreCluster ────────────────────────────────────────────────────

async fn t_cluster_large() -> SR {
    let ports = [20200u16, 20201, 20202, 20203, 20204, 20205, 20206, 20207];
    for &p in &ports {
        Worker::new(&format!("stress-w{}", p), &format!("127.0.0.1:{}", p)).start();
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let addrs: Vec<String> = ports.iter().map(|p| format!("127.0.0.1:{}", p)).collect();
    let coord = Coordinator::new(addrs.iter().map(|s| s.as_str()).collect());
    let block = gen_block(500_000, 250_000);
    let t     = Instant::now();
    let out   = coord.distribute(&block, Operation::PassThrough).await.unwrap();
    sr("CLUSTER", "8-worker distribute+merge 500K rows", 500_000, out.num_rows,
        t.elapsed(), &format!("output={} rows across 8 workers", out.num_rows))
}

async fn t_cluster_count() -> SR {
    let ports = [20210u16, 20211, 20212, 20213];
    for &p in &ports {
        Worker::new(&format!("cnt-w{}", p), &format!("127.0.0.1:{}", p)).start();
    }
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let addrs: Vec<String> = ports.iter().map(|p| format!("127.0.0.1:{}", p)).collect();
    let coord = Coordinator::new(addrs.iter().map(|s| s.as_str()).collect());
    let block = gen_block(1_000_000, 500_000);
    let t     = Instant::now();
    let out   = coord.distribute(&block, Operation::Count).await.unwrap();
    let total: i64 = (0..out.num_rows)
        .filter_map(|i| out.column("count")?.data.get_value(i).as_f64().map(|v| v as i64))
        .sum();
    sr("CLUSTER", "4-worker COUNT distributed 1M rows", 1_000_000, total as usize,
        t.elapsed(), &format!("sum(counts)={}", total))
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    use std::panic;

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║         KORE Stress / Limits Test — Layers 15–19                   ║");
    println!("║  Pushing joins, cache, ML, pipelines, cluster to their limits       ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();

    // Macro to run a test, catch panics, and return an error SR on failure
    macro_rules! run {
        ($expr:expr, $tag:expr, $name:expr) => {{
            let result = panic::catch_unwind(panic::AssertUnwindSafe(|| $expr));
            match result {
                Ok(sr) => sr,
                Err(_) => sr($tag, $name, 0, 0, std::time::Duration::ZERO,
                             "PANIC — OOM or logic error"),
            }
        }};
    }

    let mut results: Vec<SR> = Vec::new();

    // ── Layer 15 ──
    println!("  ► Layer 15 KoreJoin …");
    results.push(run!(t_join_skew(),        "JOIN", "HashJoin key-skew 1K×1K"));
    results.push(run!(t_join_large(),       "JOIN", "HashJoin 500K×100K"));
    results.push(run!(t_broadcast_large(),  "JOIN", "BroadcastJoin 1M×5K"));
    results.push(run!(t_smj_large(),        "JOIN", "SortMergeJoin FULL 200K×100K"));
    results.push(run!(t_smj_skew(),         "JOIN", "SortMergeJoin skew 20×200"));

    // ── Layer 16 ──
    println!("  ► Layer 16 KoreCache …");
    results.push(run!(t_lru_thrash(),       "CACHE", "LRU thrash 50K puts"));
    results.push(run!(t_lru_large_cap(),    "CACHE", "LRU 10K×1KB"));
    results.push(run!(t_matview_pressure(), "CACHE", "MatView 200×1K epochs"));

    // ── Layer 17 ──
    println!("  ► Layer 17 KoreML2 …");
    results.push(run!(t_rf_deep(),    "ML2", "RF clf 25t d5 n1.5K"));
    results.push(run!(t_rf_massive(), "ML2", "RF reg 30t d5 n2K"));
    results.push(run!(t_gbm_long(),   "ML2", "GBM 100i d3 n1.5K"));
    results.push(run!(t_nb_million(), "ML2", "NB n500K"));

    // ── Layer 18 ──
    println!("  ► Layer 18 KorePipeline …");
    results.push(run!(t_pipeline_deep(), "PIPE", "Scaler→GBM n3K"));
    results.push(run!(t_pipeline_rf(),   "PIPE", "Scaler→RF n2K"));

    // ── Layer 19 ──
    println!("  ► Layer 19 KoreCluster …");
    results.push(t_cluster_large().await);
    results.push(t_cluster_count().await);

    // ── Print ──
    println!();
    println!("  {:<10} {:<54} {:>10} {:>10} {:>10}  {}",
        "Layer", "Test", "rows_in", "rows_out", "ms", "Status");
    println!("  {}", "─".repeat(115));

    let mut slowest_ms   = 0u128;
    let mut slowest_name = "";
    for r in &results {
        let ms = r.dur.as_millis();
        println!("  {:<10} {:<54} {:>10} {:>10} {:>10}  {}  {}",
            r.tag, truncate(&r.name, 54), r.rows_in, r.rows_out, ms, r.status(), r.note);
        if ms > slowest_ms { slowest_ms = ms; slowest_name = &r.tag; }
    }
    println!("  {}", "─".repeat(115));

    // ── Summary ──
    let total_ms:   u128 = results.iter().map(|r| r.dur.as_millis()).sum();
    let n_panic:    usize = results.iter().filter(|r| r.note.contains("PANIC")).count();
    let n_critical: usize = results.iter().filter(|r| r.dur.as_millis() >= 10_000).count();
    let n_slow:     usize = results.iter().filter(|r| r.dur.as_millis() >= 1_000 && r.dur.as_millis() < 10_000).count();
    let n_fast:     usize = results.iter().filter(|r| r.dur.as_millis() < 1_000 && !r.note.contains("PANIC")).count();

    println!();
    println!("  Total wall time : {:.2} s", total_ms as f64 / 1000.0);
    println!("  Fast/OK         : {}  |  SLOW : {}  |  CRITICAL : {}  |  PANIC : {}",
             n_fast, n_slow, n_critical, n_panic);
    if n_panic > 0 {
        println!("  ✗  {} test(s) panicked — OOM or logic error (see notes above)", n_panic);
    } else if n_critical > 0 {
        println!("  ⚠  CRITICAL bottleneck in [{}] — top optimisation target", slowest_name);
    } else if n_slow > 0 {
        println!("  ⚠  {} slow test(s) — acceptable for pure-Rust without SIMD/BLAS", n_slow);
    } else {
        println!("  ✓  All tests FAST or better — KORE holds under load");
    }
    println!();
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
