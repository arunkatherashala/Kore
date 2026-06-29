use kore_dist_net::distribute_query;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    
    let workers_str = args.iter()
        .skip_while(|a| *a != "--workers")
        .nth(1)
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1:9001,127.0.0.1:9002");
    
    let sql = args.iter()
        .skip_while(|a| *a != "--sql")
        .nth(1)
        .map(|s| s.as_str())
        .unwrap_or("SELECT cat, SUM(amount) FROM sales GROUP BY cat");

    let worker_addrs: Vec<&str> = workers_str.split(',').collect();
    
    eprintln!("[kore-coordinator] Workers: {:?}", worker_addrs);
    eprintln!("[kore-coordinator] SQL: {sql}");
    
    // For demo: generate test data
    use kore_core::types::{Column, ColumnData, DataBlock};
    let n = 1_000_000usize;
    let data = DataBlock {
        num_rows: n,
        columns: vec![
            Column { name: "amount".into(), data: ColumnData::Float64(
                (0..n).map(|i| Some(i as f64)).collect()
            )},
            Column { name: "cat".into(), data: ColumnData::Str(
                (0..n).map(|i| Some(["A","B","C"][i%3].to_string())).collect()
            )},
        ],
    };
    
    eprintln!("[kore-coordinator] Distributing {} rows...", n);
    match distribute_query(sql, "sales", &data, &worker_addrs) {
        Ok(result) => {
            eprintln!("[kore-coordinator] Result: {} rows", result.num_rows);
            for col in &result.columns {
                eprintln!("  Column: {} ({:?})", col.name, std::mem::discriminant(&col.data));
            }
        }
        Err(e) => eprintln!("[kore-coordinator] Error: {e}"),
    }
}
