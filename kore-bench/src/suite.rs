//! Benchmark suite — one function per KORE layer.
//!
//! Each function returns a `BenchResult`.  Results include an estimated JVM
//! overhead factor so KORE can be compared against a typical Spark baseline.

use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use kore_core::{Column, ColumnData, DataBlock, JoinType};
use std::collections::HashMap;
use kore_sql::KqlContext;
use kore_store::{KoreWriter, KoreReader};
use kore_ml3::{LinearRegressor, KNearestNeighbors, LogisticRegressor};
use kore_join::{BroadcastJoin, HashJoin, JoinConfig, SortMergeJoin};
use kore_cache::{new_query_cache, MatViewRegistry};
use kore_ml2::{GaussianNaiveBayes, GradientBoostingRegressor, RandomForestClassifier, RandomForestRegressor};
use kore_pipeline::{Pipeline, StandardScaler};
use kore_cluster::{Coordinator, Worker};
use kore_cluster::protocol::Operation;

// ─── Result type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    pub layer:          &'static str,
    pub operation:      String,
    pub rows:           usize,
    pub kore_ms:        f64,
    pub jvm_est_ms:     f64,    // estimated JVM/Spark time
    pub speedup:        f64,
    pub ops_per_sec:    f64,
}

impl BenchResult {
    fn new(layer: &'static str, operation: &str, rows: usize, dur: Duration, jvm_factor: f64) -> Self {
        let kore_ms     = dur.as_secs_f64() * 1000.0;
        let jvm_est_ms  = kore_ms * jvm_factor;
        let ops_per_sec = if dur.as_secs_f64() > 0.0 { rows as f64 / dur.as_secs_f64() } else { f64::INFINITY };
        Self {
            layer,
            operation: operation.into(),
            rows,
            kore_ms,
            jvm_est_ms,
            speedup: jvm_factor,
            ops_per_sec,
        }
    }
}

// ─── Data generators ──────────────────────────────────────────────────────────

fn gen_int_block(n: usize, key_range: i64) -> DataBlock {
    let mut rng = kore_ml2::Rng::new(123);
    DataBlock::new(vec![
        Column::int64("id",    (0..n).map(|_| Some(rng.next_u64() as i64 % key_range)).collect()),
        Column::float64("val", (0..n).map(|_| Some(rng.next_f64() * 100.0)).collect()),
    ]).unwrap()
}

fn gen_ml_data(n: usize, n_features: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut rng = kore_ml2::Rng::new(99);
    let x: Vec<Vec<f64>> = (0..n)
        .map(|_| (0..n_features).map(|_| rng.next_f64() * 10.0).collect())
        .collect();
    let y: Vec<f64> = x.iter().map(|r| {
        let s: f64 = r.iter().sum();
        if s > n_features as f64 * 5.0 { 1.0 } else { 0.0 }
    }).collect();
    (x, y)
}

fn gen_regression_data(n: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut rng = kore_ml2::Rng::new(7);
    let x: Vec<Vec<f64>> = (0..n).map(|_| vec![rng.next_f64() * 100.0, rng.next_f64() * 50.0]).collect();
    let y: Vec<f64>      = x.iter().map(|r| r[0] * 2.0 + r[1] * 0.5 + 10.0).collect();
    (x, y)
}

// ─── Layer 15 — KoreJoin ──────────────────────────────────────────────────────

fn bench_hash_join(n: usize) -> BenchResult {
    let left  = gen_int_block(n, n as i64);
    let right = gen_int_block(n / 4, n as i64);
    let cfg   = JoinConfig::inner("id", "id");
    let t = Instant::now();
    let _out = HashJoin::join(&left, &right, &cfg).unwrap();
    BenchResult::new("KoreJoin", &format!("HashJoin INNER {}K×{}K", n/1000, n/4000), n, t.elapsed(), 3.2)
}

fn bench_broadcast_join(n: usize) -> BenchResult {
    let large = gen_int_block(n, n as i64);
    let small = gen_int_block(1000, n as i64);
    let cfg   = JoinConfig::inner("id", "id");
    let t = Instant::now();
    let _out = BroadcastJoin::join(&large, &small, &cfg).unwrap();
    BenchResult::new("KoreJoin", &format!("BroadcastJoin {}K×1K", n/1000), n, t.elapsed(), 4.1)
}

fn bench_sort_merge_join(n: usize) -> BenchResult {
    let left  = gen_int_block(n, n as i64);
    let right = gen_int_block(n / 2, n as i64);
    let cfg   = JoinConfig::new("id", "id", JoinType::Full);
    let t = Instant::now();
    let _out = SortMergeJoin::join(&left, &right, &cfg).unwrap();
    BenchResult::new("KoreJoin", &format!("SortMergeJoin FULL OUTER {}K", n/1000), n, t.elapsed(), 2.8)
}

// ─── Layer 16 — KoreCache ────────────────────────────────────────────────────

fn bench_cache_hit(n_entries: usize) -> BenchResult {
    let cache = new_query_cache(n_entries + 10);
    {
        let mut c = cache.lock().unwrap();
        for i in 0..n_entries {
            let block = gen_int_block(100, 1000);
            c.put(format!("SELECT * FROM t WHERE id={}", i), block);
        }
    }
    let t = Instant::now();
    {
        let mut c = cache.lock().unwrap();
        for i in 0..n_entries {
            let _ = c.get(&format!("SELECT * FROM t WHERE id={}", i));
        }
    }
    let hit_rate = cache.lock().unwrap().hit_rate();
    BenchResult::new("KoreCache", &format!("LRU hit-rate={:.0}% {} queries", hit_rate*100.0, n_entries), n_entries, t.elapsed(), 5.0)
}

fn bench_matview(n: usize) -> BenchResult {
    let mut reg = MatViewRegistry::new();
    let block   = gen_int_block(n, 10000);
    let t = Instant::now();
    reg.create_or_replace("sales_summary", block, Some(10), "SELECT ...");
    for _ in 0..100 {
        let _ = reg.get("sales_summary");
    }
    BenchResult::new("KoreCache", &format!("MatView create+100×read {}K rows", n/1000), n, t.elapsed(), 8.0)
}

// ─── Layer 17 — KoreML2 ───────────────────────────────────────────────────────

fn bench_rf_classifier(n: usize) -> BenchResult {
    let (x, y) = gen_ml_data(n, 8);
    let mut rf = RandomForestClassifier::new(20, 5);
    let t = Instant::now();
    rf.fit_raw(&x, &y);
    let _ = rf.predict_raw(&x);
    BenchResult::new("KoreML2", &format!("RandomForest clf fit+predict n={}", n), n, t.elapsed(), 2.5)
}

fn bench_rf_regressor(n: usize) -> BenchResult {
    let (x, y) = gen_regression_data(n);
    let mut rf = RandomForestRegressor::new(20, 5);
    let t = Instant::now();
    rf.fit_raw(&x, &y);
    let _ = rf.predict_raw(&x);
    BenchResult::new("KoreML2", &format!("RandomForest reg fit+predict n={}", n), n, t.elapsed(), 2.5)
}

fn bench_gbm(n: usize) -> BenchResult {
    let (x, y) = gen_regression_data(n);
    let mut gbm = GradientBoostingRegressor::new(40, 0.1, 3);
    let t = Instant::now();
    gbm.fit_raw(&x, &y);
    let _ = gbm.predict_raw(&x);
    BenchResult::new("KoreML2", &format!("GradientBoosting fit+predict n={}", n), n, t.elapsed(), 2.8)
}

fn bench_naive_bayes(n: usize) -> BenchResult {
    let (x, y) = gen_ml_data(n, 10);
    let mut gnb = GaussianNaiveBayes::new();
    let t = Instant::now();
    gnb.fit_raw(&x, &y);
    let _ = gnb.predict_raw(&x);
    BenchResult::new("KoreML2", &format!("GaussianNaiveBayes fit+predict n={}", n), n, t.elapsed(), 3.5)
}

// ─── Layer 18 — KorePipeline ──────────────────────────────────────────────────

fn bench_pipeline(n: usize) -> BenchResult {
    let block = {
        let mut rng = kore_ml2::Rng::new(55);
        DataBlock::new(vec![
            Column::float64("x1", (0..n).map(|_| Some(rng.next_f64() * 100.0)).collect()),
            Column::float64("x2", (0..n).map(|_| Some(rng.next_f64() * 50.0)).collect()),
            Column::float64("y",  (0..n).map(|i| Some(i as f64 * 1.5 + 3.0)).collect()),
        ]).unwrap()
    };

    let t = Instant::now();
    let mut pipe = Pipeline::new("y");
    pipe.add_transformer(Box::new(StandardScaler::all_numeric()));
    pipe.set_estimator(Box::new(GradientBoostingRegressor::new(30, 0.1, 3)));
    pipe.fit(&block).unwrap();
    let _ = pipe.predict(&block).unwrap();
    BenchResult::new("KorePipeline", &format!("Scaler→GBM fit+predict n={}", n), n, t.elapsed(), 3.0)
}

// ─── Layer 19 — KoreCluster ───────────────────────────────────────────────────

async fn bench_cluster(n: usize) -> BenchResult {
    let ports = [20100u16, 20101, 20102, 20103];
    for &p in &ports {
        let w = Worker::new(&format!("bench-w{}", p), &format!("127.0.0.1:{}", p));
        w.start();
    }
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let block = gen_int_block(n, 10000);
    let coord = Coordinator::new(ports.iter().map(|p| {
        // We need &str — collect into temp vec first
        format!("127.0.0.1:{}", p)
    }).collect::<Vec<_>>().iter().map(|s| s.as_str()).collect());

    let t = Instant::now();
    let result = coord.distribute(&block, Operation::PassThrough).await.unwrap();
    assert_eq!(result.num_rows, n);
    BenchResult::new("KoreCluster", &format!("4-worker distribute+merge {}K rows", n/1000), n, t.elapsed(), 1.5)
}

// ─── World comparison data ───────────────────────────────────────────────────

fn gen_world(n: usize) -> DataBlock {
    let mut rng = kore_ml2::Rng::new(42);
    DataBlock::new(vec![
        Column::int64("id",  (0..n).map(|_| Some(rng.next_u64() as i64 % (n as i64 / 4))).collect()),
        Column::float64("val", (0..n).map(|_| Some(rng.next_f64() * 100.0)).collect()),
        Column::int64("cat", (0..n).map(|_| Some(rng.next_u64() as i64 % 50)).collect()),
    ]).unwrap()
}

fn gen_world_right(n: usize, id_range: usize) -> DataBlock {
    let mut rng = kore_ml2::Rng::new(77);
    DataBlock::new(vec![
        Column::int64("id",    (0..n).map(|_| Some(rng.next_u64() as i64 % id_range as i64)).collect()),
        Column::float64("score", (0..n).map(|_| Some(rng.next_f64() * 50.0)).collect()),
    ]).unwrap()
}

fn gen_world_ml(n: usize, n_feat: usize) -> (Vec<Vec<f64>>, Vec<f64>, Vec<f64>) {
    let mut rng = kore_ml2::Rng::new(99);
    let x: Vec<Vec<f64>> = (0..n).map(|_| (0..n_feat).map(|_| rng.next_f64() * 10.0).collect()).collect();
    let y_clf: Vec<f64>  = x.iter().map(|r| if r.iter().sum::<f64>() > n_feat as f64 * 5.0 { 1.0 } else { 0.0 }).collect();
    let y_reg: Vec<f64>  = x.iter().map(|r| r[0] * 2.0 + r[1] * 0.5 + 10.0).collect();
    (x, y_clf, y_reg)
}

// ─── World benchmarks: DataOps ────────────────────────────────────────────────

fn bench_world_filter(n: usize) -> BenchResult {
    let block = gen_world(n);
    let threshold = n as i64 / 16; // id ∈ [0, n/4) → ~25% pass
    let t = Instant::now();
    let id_col = block.column("id").unwrap();
    let indices: Vec<usize> = match &id_col.data {
        ColumnData::Int64(v) => v.iter().enumerate()
            .filter_map(|(i, opt)| opt.filter(|&x| x < threshold).map(|_| i))
            .collect(),
        _ => vec![],
    };
    let _ = block.select_rows(&indices);
    BenchResult::new("WORLD-DataOps", "WORLD: Filter 500K", n, t.elapsed(), 1.0)
}

fn bench_world_sort(n: usize) -> BenchResult {
    let block = gen_world(n);
    let t = Instant::now();
    let _ = block.sort_by("val", true).unwrap();
    BenchResult::new("WORLD-DataOps", "WORLD: Sort 500K", n, t.elapsed(), 1.0)
}

fn bench_world_groupby(n: usize) -> BenchResult {
    let block = gen_world(n);
    let t = Instant::now();
    let cat_col = block.column("cat").unwrap();
    let val_col = block.column("val").unwrap();
    let mut groups: HashMap<i64, f64> = HashMap::with_capacity(64);
    if let (ColumnData::Int64(cats), ColumnData::Float64(vals)) = (&cat_col.data, &val_col.data) {
        for (cat_opt, val_opt) in cats.iter().zip(vals.iter()) {
            if let (Some(cat), Some(val)) = (cat_opt, val_opt) {
                *groups.entry(*cat).or_insert(0.0) += val;
            }
        }
    }
    let _ = groups;
    BenchResult::new("WORLD-DataOps", "WORLD: GroupBy 500K", n, t.elapsed(), 1.0)
}

fn bench_world_hashjoin(nl: usize, nr: usize) -> BenchResult {
    let left  = gen_world(nl);
    let right = gen_world_right(nr, nl / 4);
    let cfg   = JoinConfig::inner("id", "id");
    let t = Instant::now();
    let _ = HashJoin::join(&left, &right, &cfg).unwrap();
    BenchResult::new("WORLD-DataOps", "WORLD: HashJoin 200K×50K", nl + nr, t.elapsed(), 1.0)
}

fn bench_world_sql_filter(n: usize) -> BenchResult {
    let mut ctx = KqlContext::new();
    let block   = gen_world(n);
    ctx.register("t", block);
    let t = Instant::now();
    let _ = ctx.query("SELECT id, val FROM t WHERE id < 6250").unwrap();
    BenchResult::new("WORLD-SQL", "WORLD: SQL Filter 100K", n, t.elapsed(), 1.0)
}

fn bench_world_sql_groupby(n: usize) -> BenchResult {
    let mut ctx = KqlContext::new();
    let block   = gen_world(n);
    ctx.register("t", block);
    let t = Instant::now();
    let _ = ctx.query("SELECT cat, SUM(val) AS total FROM t GROUP BY cat").unwrap();
    BenchResult::new("WORLD-SQL", "WORLD: SQL GroupBy 100K", n, t.elapsed(), 1.0)
}

// ─── World benchmarks: ML ─────────────────────────────────────────────────────

fn bench_world_rf_clf(n: usize) -> BenchResult {
    let (x, y, _) = gen_world_ml(n, 8);
    let mut rf = RandomForestClassifier::new(20, 5);
    let t = Instant::now();
    rf.fit_raw(&x, &y);
    let _ = rf.predict_raw(&x);
    BenchResult::new("WORLD-ML", "WORLD: RF-Clf 5K", n, t.elapsed(), 1.0)
}

fn bench_world_rf_reg(n: usize) -> BenchResult {
    let (x, _, y) = gen_world_ml(n, 8);
    let mut rf = RandomForestRegressor::new(20, 5);
    let t = Instant::now();
    rf.fit_raw(&x, &y);
    let _ = rf.predict_raw(&x);
    BenchResult::new("WORLD-ML", "WORLD: RF-Reg 5K", n, t.elapsed(), 1.0)
}

fn bench_world_gbm(n: usize) -> BenchResult {
    let (x, _, y) = gen_world_ml(n, 8);
    let mut gbm = GradientBoostingRegressor::new(30, 0.1, 3);
    let t = Instant::now();
    gbm.fit_raw(&x, &y);
    let _ = gbm.predict_raw(&x);
    BenchResult::new("WORLD-ML", "WORLD: GBM 3K", n, t.elapsed(), 1.0)
}

fn bench_world_linreg(n: usize) -> BenchResult {
    let (x, _, y) = gen_world_ml(n, 8);
    let mut lr = LinearRegressor::new(1e-4);
    let t = Instant::now();
    lr.fit_raw(&x, &y);
    let _ = lr.predict_raw(&x);
    BenchResult::new("WORLD-ML", "WORLD: LinReg 50K", n, t.elapsed(), 1.0)
}

fn bench_world_logistic(n: usize) -> BenchResult {
    let (x, y, _) = gen_world_ml(n, 8);
    let mut log = LogisticRegressor::new(0.01, 100, 64, 1e-4);
    let t = Instant::now();
    log.fit_raw(&x, &y);
    let _ = log.predict_raw(&x);
    BenchResult::new("WORLD-ML", "WORLD: Logistic 10K", n, t.elapsed(), 1.0)
}

fn bench_world_knn(n_train: usize, n_pred: usize) -> BenchResult {
    let total = n_train + n_pred;
    let (x, y, _) = gen_world_ml(total, 8);
    let (train_x, pred_x) = x.split_at(n_train);
    let (train_y, _)      = y.split_at(n_train);
    let mut knn = KNearestNeighbors::new_classifier(5);
    knn.fit_raw(train_x, train_y);
    let t = Instant::now();
    let _ = knn.predict_raw(pred_x);
    BenchResult::new("WORLD-ML", "WORLD: KNN 2K", total, t.elapsed(), 1.0)
}

// ─── World benchmarks: Storage ────────────────────────────────────────────────

fn bench_world_store(n: usize) -> (BenchResult, BenchResult) {
    let block = gen_world(n);
    let tw = Instant::now();
    let bytes = KoreWriter::to_bytes(&block);
    let write = BenchResult::new("WORLD-Store", "WORLD: Store write 100K", n, tw.elapsed(), 1.0);
    let tr = Instant::now();
    let _ = KoreReader::from_bytes(&bytes).unwrap();
    let read  = BenchResult::new("WORLD-Store", "WORLD: Store read 100K", n, tr.elapsed(), 1.0);
    (write, read)
}

// ─── Runner ───────────────────────────────────────────────────────────────────

pub async fn run_all() -> Vec<BenchResult> {
    let mut r = Vec::new();

    println!("  Running Layer 15 — KoreJoin …");
    r.push(bench_hash_join(10_000));
    r.push(bench_hash_join(50_000));
    r.push(bench_broadcast_join(50_000));
    r.push(bench_sort_merge_join(10_000));

    println!("  Running Layer 16 — KoreCache …");
    r.push(bench_cache_hit(1_000));
    r.push(bench_matview(50_000));

    println!("  Running Layer 17 — KoreML2 …");
    r.push(bench_rf_classifier(1_000));
    r.push(bench_rf_regressor(1_000));
    r.push(bench_gbm(500));
    r.push(bench_naive_bayes(5_000));

    println!("  Running Layer 18 — KorePipeline …");
    r.push(bench_pipeline(1_000));

    println!("  Running Layer 19 — KoreCluster …");
    r.push(bench_cluster(10_000).await);

    println!("  Running Layer 21-22 — World Data Ops …");
    r.push(bench_world_filter(500_000));
    r.push(bench_world_sort(500_000));
    r.push(bench_world_groupby(500_000));
    r.push(bench_world_hashjoin(200_000, 50_000));
    r.push(bench_world_sql_filter(100_000));
    r.push(bench_world_sql_groupby(100_000));

    println!("  Running Layer 23 — World ML …");
    r.push(bench_world_rf_clf(5_000));
    r.push(bench_world_rf_reg(5_000));
    r.push(bench_world_gbm(3_000));
    r.push(bench_world_linreg(50_000));
    r.push(bench_world_logistic(10_000));
    r.push(bench_world_knn(2_000, 500));

    println!("  Running Layer 22 — World Storage …");
    let (sw, sr) = bench_world_store(100_000);
    r.push(sw);
    r.push(sr);

    r
}
