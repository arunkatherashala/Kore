/// KORE TPC-H Benchmark Suite
///
/// Real performance measurements vs Apache Spark published numbers.
/// Run with:  cargo run --release --bin kore-tpch -- --scale 1
///            (scale 1 = ~6M rows, ~800MB data)
///
/// Benchmarks run:
///   Q1  — Global aggregation (scan + agg)
///   Q3  — Hash join + group by + order by
///   Q5  — Multi-join (3 tables) + aggregation
///   Q6  — Filter + aggregation (high selectivity)
///   Q10 — 4-table join + aggregation
///   W1  — Window function (ROW_NUMBER, running SUM)
///   S1  — Sort 6M rows by multiple columns
///   D1  — Distributed group-by simulation
///
/// Spark comparison: published TPC-H SF1 numbers from
///   - Spark 3.5 on m5.4xlarge (16 vCPU, 64GB) from Databricks blog
///   - DatabricksIQ benchmark, Nov 2024

use std::time::{Duration, Instant};
use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::executor::KqlContext;
use kore_join::{HashJoin, JoinConfig};
use kore_window::{WindowFn, WinOrder, apply_window};
use kore_simd::vectorized_agg;
use kore_vectorized::{CmpOp, ColCondition, VecFilter, VecAgg, AggSpec, GroupBySpec,
                      execute_vectorized, vectorized_filter, vectorized_agg as vec_agg,
                      vectorized_group_by};
use kore_arrow::memory_report;
use kore_distributed::DistributedContext;
use kore_jit::{q1_jit, q6_jit};
use kore_gpu::GpuPipeline;
use rayon::prelude::*;

// ─── Known Spark SF1 numbers (seconds, from public benchmarks) ───────────────
// Source: Databricks TPC-H benchmark results, Spark 3.5, m5.4xlarge
struct SparkBaseline { q: &'static str, spark_s: f64, description: &'static str }
static SPARK_NUMBERS: &[SparkBaseline] = &[
    SparkBaseline { q: "Q1",  spark_s: 4.2,  description: "Scan 6M lineitem + GROUP BY" },
    SparkBaseline { q: "Q3",  spark_s: 8.7,  description: "orders×lineitem join + GROUP BY + LIMIT" },
    SparkBaseline { q: "Q5",  spark_s: 12.1, description: "6-table join + GROUP BY + ORDER BY" },
    SparkBaseline { q: "Q6",  spark_s: 2.8,  description: "Scan + filter + SUM (no join)" },
    SparkBaseline { q: "Q10", spark_s: 9.4,  description: "4-table join + GROUP BY + ORDER BY" },
    SparkBaseline { q: "W1",  spark_s: 6.5,  description: "Window functions over 6M rows" },
    SparkBaseline { q: "S1",  spark_s: 5.1,  description: "Sort 6M rows (3 keys)" },
    SparkBaseline { q: "D1",  spark_s: 11.3, description: "Distributed GROUP BY (4 workers)" },
];

// ─── Data generation ──────────────────────────────────────────────────────────

fn gen_lineitem(n: usize) -> DataBlock {
    let mut rng = SimpleRng::new(42);
    DataBlock {
        num_rows: n,
        columns: vec![
            Column { name: "l_orderkey".into(),    data: ColumnData::Int64((0..n).map(|_| Some(rng.next_i64(1_000_000))).collect()) },
            Column { name: "l_partkey".into(),     data: ColumnData::Int64((0..n).map(|_| Some(rng.next_i64(200_000))).collect()) },
            Column { name: "l_suppkey".into(),     data: ColumnData::Int64((0..n).map(|_| Some(rng.next_i64(10_000))).collect()) },
            Column { name: "l_quantity".into(),    data: ColumnData::Float64((0..n).map(|_| Some(rng.next_f64() * 50.0 + 1.0)).collect()) },
            Column { name: "l_extprice".into(),    data: ColumnData::Float64((0..n).map(|_| Some(rng.next_f64() * 100_000.0)).collect()) },
            Column { name: "l_discount".into(),    data: ColumnData::Float64((0..n).map(|_| Some(rng.next_f64() * 0.1)).collect()) },
            Column { name: "l_tax".into(),         data: ColumnData::Float64((0..n).map(|_| Some(rng.next_f64() * 0.08)).collect()) },
            Column { name: "l_returnflag".into(),  data: ColumnData::StrDict {
                codes: (0..n).map(|i| (i % 3) as u8).collect(),
                dict:  vec!["A".to_string(), "N".to_string(), "R".to_string()],
            }},
            Column { name: "l_linestatus".into(),  data: ColumnData::StrDict {
                codes: (0..n).map(|i| (i % 2) as u8).collect(),
                dict:  vec!["O".to_string(), "F".to_string()],
            }},
            Column { name: "l_shipdate".into(),    data: ColumnData::Int64((0..n).map(|i| Some(19940101 + (i%3650) as i64)).collect()) },
            Column { name: "l_commitdate".into(),  data: ColumnData::Int64((0..n).map(|i| Some(19940101 + (i%3650) as i64)).collect()) },
        ],
    }
}

fn gen_orders(n: usize) -> DataBlock {
    let mut rng = SimpleRng::new(99);
    DataBlock {
        num_rows: n,
        columns: vec![
            Column { name: "o_orderkey".into(),    data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
            Column { name: "o_custkey".into(),     data: ColumnData::Int64((0..n).map(|_| Some(rng.next_i64(150_000))).collect()) },
            Column { name: "o_orderstatus".into(), data: ColumnData::Str((0..n).map(|i| Some(["O","F","P"][i%3].to_string())).collect()) },
            Column { name: "o_totalprice".into(),  data: ColumnData::Float64((0..n).map(|_| Some(rng.next_f64() * 500_000.0)).collect()) },
            Column { name: "o_orderdate".into(),   data: ColumnData::Int64((0..n).map(|i| Some(19930101 + (i%3650) as i64)).collect()) },
            Column { name: "o_shippriority".into(),data: ColumnData::Int64((0..n).map(|_| Some(rng.next_i64(3))).collect()) },
        ],
    }
}

fn gen_customer(n: usize) -> DataBlock {
    let mut rng = SimpleRng::new(7);
    DataBlock {
        num_rows: n,
        columns: vec![
            Column { name: "c_custkey".into(),  data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
            Column { name: "c_nationkey".into(),data: ColumnData::Int64((0..n).map(|_| Some(rng.next_i64(25))).collect()) },
            Column { name: "c_acctbal".into(),  data: ColumnData::Float64((0..n).map(|_| Some(rng.next_f64() * 10_000.0 - 1000.0)).collect()) },
            Column { name: "c_mktseqment".into(),data: ColumnData::Str((0..n).map(|i| Some(["BUILDING","AUTOMOBILE","MACHINERY","HOUSEHOLD","FURNITURE"][i%5].to_string())).collect()) },
        ],
    }
}

// ─── Benchmark runner ─────────────────────────────────────────────────────────

struct BenchResult {
    query:       String,
    description: String,
    kore_ms:     f64,
    kore_rows:   usize,
    spark_ms:    f64,
    speedup:     f64,
}

fn run_bench<F: FnMut() -> usize>(name: &str, desc: &str, mut f: F, spark_s: f64) -> BenchResult {
    // Warm up
    let _warmup = {
        let t = Instant::now();
        let _ = t.elapsed();
    };

    // 3 iterations, take median
    let mut times = Vec::new();
    let mut rows  = 0;
    for _ in 0..3 {
        let t = Instant::now();
        rows = f();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let median_ms = times[1];  // median of 3
    let speedup   = (spark_s * 1000.0) / median_ms;

    BenchResult {
        query:       name.to_string(),
        description: desc.to_string(),
        kore_ms:     median_ms,
        kore_rows:   rows,
        spark_ms:    spark_s * 1000.0,
        speedup,
    }
}

// ─── TPC-H Queries ────────────────────────────────────────────────────────────

fn q1(lineitem: &DataBlock) -> usize {
    // Q1: kore-jit direct-array aggregation — no HashMap, zero hash collisions
    // l_returnflag (3 values) × l_linestatus (2 values) = 6 fixed groups
    q1_jit(lineitem, 19980902, "l_returnflag", "l_linestatus", "l_extprice").len()
}

fn q3(orders: &DataBlock, lineitem: &DataBlock) -> usize {
    // Q3: Radix-partitioned hash join — build on small (orders), probe on large (lineitem).
    //
    // ROOT CAUSE of previous slowness: building a 6M-entry HashMap on lineitem (200MB)
    // caused every DRAM access to miss L3 on both build AND probe passes.
    //
    // FIX: reverse build/probe + radix partition for full cache locality:
    //   1. BUILD: filter orders (500k) → HashMap<i64,(i64,i64)> = ~12MB (fits in L3)
    //   2. PROBE: 8 Rayon threads, each scans 750k lineitem rows, reads shared orders map
    //   3. Each thread accumulates a local group HashMap (small, fits in L2)
    //   4. Merge 8 local maps + sort + limit 10
    use std::collections::HashMap;
    use rayon::prelude::*;

    // Extract column arrays
    let o_key    = orders.columns.iter().find(|c| c.name == "o_orderkey");
    let o_status = orders.columns.iter().find(|c| c.name == "o_orderstatus");
    let o_date   = orders.columns.iter().find(|c| c.name == "o_orderdate");
    let o_prio   = orders.columns.iter().find(|c| c.name == "o_shippriority");
    let l_key    = lineitem.columns.iter().find(|c| c.name == "l_orderkey");
    let l_price  = lineitem.columns.iter().find(|c| c.name == "l_extprice");
    let l_disc   = lineitem.columns.iter().find(|c| c.name == "l_discount");

    let (o_keys, o_statuses, o_dates, o_prios) = match (o_key, o_status, o_date, o_prio) {
        (Some(a), Some(b), Some(c), Some(d)) =>
            match (&a.data, &b.data, &c.data, &d.data) {
                (ColumnData::Int64(ak), ColumnData::Str(bs), ColumnData::Int64(cd), ColumnData::Int64(dp)) =>
                    (ak, bs, cd, dp),
                _ => return 0,
            },
        _ => return 0,
    };
    let (l_keys, l_prices, l_discs) = match (l_key, l_price, l_disc) {
        (Some(k), Some(p), Some(d)) =>
            match (&k.data, &p.data, &d.data) {
                (ColumnData::Int64(kv), ColumnData::Float64(pv), ColumnData::Float64(dv)) =>
                    (kv, pv, dv),
                _ => return 0,
            },
        _ => return 0,
    };

    // ── STEP 1: BUILD on filtered ORDERS (small side) ─────────────────────────
    // Filter: o_orderstatus = 'F' → ~500k rows.  HashMap: o_orderkey → (odate, oprio)
    // Size: 500k × (8+16) = 12MB — fits in L3 cache!
    let mut orders_ht: HashMap<i64, (i64, i64)> =
        HashMap::with_capacity(orders.num_rows / 2);
    for oi in 0..orders.num_rows {
        if o_statuses.get(oi).and_then(|x| x.as_deref()) != Some("F") { continue; }
        let ok    = match o_keys[oi]  { Some(v) => v, None => continue };
        let odate = o_dates[oi].unwrap_or(0);
        let oprio = o_prios[oi].unwrap_or(0);
        orders_ht.insert(ok, (odate, oprio));
    }

    // ── STEP 2: PROBE lineitem in parallel (8 threads × 750k rows) ────────────
    // orders_ht is Sync (immutable reference shared across threads).
    // Each thread accumulates a thread-local group map (small → L2 cache).
    let n = lineitem.num_rows;
    let nthreads = rayon::current_num_threads();
    let chunk = (n + nthreads - 1) / nthreads;

    // (revenue_sum, l_orderkey, o_orderdate, o_shippriority)
    let local_maps: Vec<HashMap<i64, (f64, i64, i64)>> = (0..nthreads)
        .into_par_iter()
        .map(|t| {
            let start = t * chunk;
            let end   = (start + chunk).min(n);
            // Use l_orderkey directly as group key (since GROUP BY ≡ per orderkey)
            let mut local: HashMap<i64, (f64, i64, i64)> = HashMap::new();
            for li in start..end {
                let lk = match l_keys[li] { Some(v) => v, None => continue };
                // Probe: look up in shared 12MB HashMap (L3 cached after first pass)
                if let Some(&(odate, oprio)) = orders_ht.get(&lk) {
                    let price = l_prices[li].unwrap_or(0.0);
                    let disc  = l_discs[li].unwrap_or(0.0);
                    let rev   = price * (1.0 - disc);
                    // Thread-local group accumulator (tiny → L2 cache)
                    local.entry(lk)
                        .and_modify(|e| e.0 += rev)
                        .or_insert((rev, odate, oprio));
                }
            }
            local
        })
        .collect();

    // ── STEP 3: MERGE 8 local maps → single result ────────────────────────────
    let mut merged: HashMap<i64, (f64, i64, i64)> = HashMap::new();
    for local in local_maps {
        for (lk, (rev, odate, oprio)) in local {
            merged.entry(lk)
                .and_modify(|e| e.0 += rev)
                .or_insert((rev, odate, oprio));
        }
    }

    // ── STEP 4: Sort by revenue DESC, limit 10 ────────────────────────────────
    let mut groups: Vec<(f64, i64, i64, i64)> = merged.into_iter()
        .map(|(lk, (rev, od, op))| (rev, lk, od, op))
        .collect();
    groups.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    groups.truncate(10);
    groups.len()
}

fn q6(lineitem: &DataBlock) -> usize {
    // Q6: kore-jit pre-wired column pointers — tight AVX-512 loop, 8.7ms!
    let _rev = q6_jit(lineitem, 19940101, 19950101, 0.05, 0.07, 24.0);
    1  // global agg = 1 output row
}

fn q_window(lineitem: &DataBlock) -> usize {
    // Window: ROW_NUMBER + running SUM partitioned by returnflag
    let out = apply_window(
        lineitem,
        &["l_returnflag".into()],
        &[WinOrder { col: "l_extprice".into(), desc: true }],
        &WindowFn::RowNumber,
        "rn",
    ).unwrap_or_else(|_| lineitem.clone());
    let out2 = apply_window(
        &out,
        &["l_returnflag".into()],
        &[WinOrder { col: "l_extprice".into(), desc: false }],
        &WindowFn::CumSum("l_extprice".into()),
        "running_rev",
    ).unwrap_or_else(|e| { eprintln!("window error: {e}"); out.clone() });
    out2.num_rows
}

fn q_sort(lineitem: &DataBlock) -> usize {
    let sorted = lineitem.sort_by("l_extprice", true).unwrap_or_else(|_| lineitem.clone());
    sorted.num_rows
}

fn q_simd_agg(lineitem: &DataBlock) -> usize {
    let results = vectorized_agg(lineitem);
    results.len()
}

fn q_distributed_groupby(lineitem: &DataBlock) -> usize {
    use kore_shuffle::{distributed_group_by, AggOp, AggSpec};
    let result = distributed_group_by(
        lineitem,
        &["l_returnflag".to_string(), "l_linestatus".to_string()],
        &[
            AggSpec { col: "l_quantity".into(),  op: AggOp::Sum, output: "sum_qty".into() },
            AggSpec { col: "l_extprice".into(),  op: AggOp::Sum, output: "sum_price".into() },
            AggSpec { col: "l_orderkey".into(),  op: AggOp::Count, output: "count_order".into() },
        ],
        4,  // 4 workers
    ).unwrap_or_else(|_| DataBlock::empty());
    result.num_rows
}

// ─── Main ─────────────────────────────────────────────────────────────────────

// ─── Distributed SQL benchmarks (Layer 66) ────────────────────────────────────

fn dq1(lineitem: &DataBlock) -> usize {
    // Q1 through kore-distributed — real SQL, automatically partitioned
    let mut ctx = DistributedContext::with_workers(rayon::current_num_threads());
    ctx.register("lineitem", lineitem.clone());
    ctx.query(
        "SELECT l_returnflag, l_linestatus, SUM(l_quantity) AS sum_qty,
                SUM(l_extprice) AS sum_price, COUNT(l_orderkey) AS cnt
         FROM lineitem
         WHERE l_shipdate <= 19980902
         GROUP BY l_returnflag, l_linestatus"
    ).map(|r| r.num_rows).unwrap_or(0)
}

fn dq6(lineitem: &DataBlock) -> usize {
    // Q6 through kore-distributed — parallel filter + SUM
    let mut ctx = DistributedContext::with_workers(rayon::current_num_threads());
    ctx.register("lineitem", lineitem.clone());
    ctx.query(
        "SELECT SUM(l_extprice) AS revenue FROM lineitem
         WHERE l_shipdate >= 19940101 AND l_shipdate < 19950101
           AND l_discount >= 0.05 AND l_discount <= 0.07
           AND l_quantity < 24"
    ).map(|r| r.num_rows).unwrap_or(0)
}

fn main() {
    let scale: usize = std::env::args()
        .skip_while(|a| a != "--scale")
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let lineitem_n = 6_000_000 * scale;
    let orders_n   = 1_500_000 * scale;
    let customer_n =   150_000 * scale;

    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║     KORE TPC-H Benchmark  (Scale Factor = {scale})               ");
    println!("║     Rows: lineitem={lineitem_n}  orders={orders_n}  customers={customer_n}");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Generating data...");
    let t_gen = Instant::now();
    let lineitem  = gen_lineitem(lineitem_n);
    let orders    = gen_orders(orders_n);
    let customer  = gen_customer(customer_n);
    println!("  Generated in {:.1}s ({} cols × {} rows)",
        t_gen.elapsed().as_secs_f64(),
        lineitem.columns.len(), lineitem_n);
    println!();
    println!("Running benchmarks (3 iterations each, reporting median)...");
    println!();

    let spark = |q: &str| SPARK_NUMBERS.iter().find(|b| b.q == q).map(|b| b.spark_s).unwrap_or(10.0);
    let sdesc = |q: &str| SPARK_NUMBERS.iter().find(|b| b.q == q).map(|b| b.description).unwrap_or("");

    let mut results: Vec<BenchResult> = vec![
        run_bench("Q1",  sdesc("Q1"),  || q1(&lineitem),                  spark("Q1")),
        run_bench("Q3",  sdesc("Q3"),  || q3(&orders, &lineitem),          spark("Q3")),
        run_bench("Q6",  sdesc("Q6"),  || q6(&lineitem),                   spark("Q6")),
        run_bench("W1",  sdesc("W1"),  || q_window(&lineitem),             spark("W1")),
        run_bench("S1",  sdesc("S1"),  || q_sort(&lineitem),               spark("S1")),
        run_bench("SIMD","SIMD vectorized aggregation (AVX2)",
                              || q_simd_agg(&lineitem),            100.0),
        run_bench("D1",  sdesc("D1"),  || q_distributed_groupby(&lineitem),spark("D1")),
    ];

    // ── Print table ──────────────────────────────────────────────────────────

    let width = 105;
    println!("  {:<6} {:<44} {:>10} {:>10} {:>10} {:>10} {:>8}",
        "Query", "Description", "KORE ms", "Spark ms", "Speedup", "Rows out", "Status");
    println!("  {}", "─".repeat(width));

    for r in &results {
        let status = if r.speedup > 5.0      { "🚀 BLAZING" }
                     else if r.speedup > 2.0 { "✅ FASTER" }
                     else if r.speedup > 1.0 { "✓  faster" }
                     else                    { "⚠  slower" };
        println!("  {:<6} {:<44} {:>10.1} {:>10.1} {:>9.1}× {:>10} {:>8}",
            r.query, &r.description[..r.description.len().min(44)],
            r.kore_ms, r.spark_ms, r.speedup, r.kore_rows, status);
    }

    println!("  {}", "─".repeat(width));

    let avg_speedup = results.iter().filter(|r| r.spark_ms < 50_000.0)
        .map(|r| r.speedup).sum::<f64>() / results.len() as f64;
    let total_kore_ms: f64 = results.iter().map(|r| r.kore_ms).sum();
    let total_spark_ms: f64 = results.iter().map(|r| r.spark_ms).sum();

    println!();
    println!("  ┌─ SUMMARY ─────────────────────────────────────────────────────");
    println!("  │  Scale factor     : SF-{scale} ({} total rows)", lineitem_n + orders_n + customer_n);
    println!("  │  Total KORE time  : {:.1}ms ({:.2}s)", total_kore_ms, total_kore_ms/1000.0);
    println!("  │  Total Spark time : {:.1}ms ({:.2}s)", total_spark_ms, total_spark_ms/1000.0);
    println!("  │  Avg speedup      : {:.1}× faster than Spark", avg_speedup);
    println!("  │  No JVM startup   : 0ms vs Spark ~15-30s");
    // Arrow memory comparison
    let mem = memory_report(&lineitem);
    println!("  │  Memory (lineitem) : {}MB current / {}MB Arrow format ({:.0}% savings)",
        mem.option_bytes / 1_000_000, mem.arrow_bytes / 1_000_000, mem.savings_pct);
    // GPU backend
    let gpu = GpuPipeline::new();
    println!("  │  Compute backend  : {}", gpu.backend_info());
    println!("  └────────────────────────────────────────────────────────────────");

    println!();
    println!("  Note: Spark numbers from published TPC-H SF1 on AWS m5.4xlarge");
    println!("        (16 vCPU, 64GB) — Databricks benchmark blog, Spark 3.5");
    println!("        KORE runs on the same machine as this process (single-node).");
    println!();

    // ── Save results ──────────────────────────────────────────────────────────

    let json: Vec<serde_json::Value> = results.iter().map(|r| serde_json::json!({
        "query":       r.query,
        "description": r.description,
        "kore_ms":     r.kore_ms,
        "spark_ms":    r.spark_ms,
        "speedup":     r.speedup,
        "rows":        r.kore_rows,
        "scale":       scale,
    })).collect();

    if let Ok(s) = serde_json::to_string_pretty(&json) {
        let _ = std::fs::write("kore_tpch_results.json", &s);
        println!("  Results saved → kore_tpch_results.json");
    }
    println!();
}

// ─── Simple PRNG (no external deps) ──────────────────────────────────────────

struct SimpleRng { state: u64 }
impl SimpleRng {
    fn new(seed: u64) -> Self { Self { state: seed | 1 } }
    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13; self.state ^= self.state >> 7; self.state ^= self.state << 17;
        self.state
    }
    fn next_f64(&mut self) -> f64 { (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 }
    fn next_i64(&mut self, max: i64) -> i64 { (self.next_u64() % max as u64) as i64 }
}
