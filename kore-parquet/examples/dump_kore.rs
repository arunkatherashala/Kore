//! Dump a KORE file's per-column summary (dtype, null count, sum / non-null).
//!
//! Usage: cargo run -p kore-parquet --release --example dump_kore -- <file.kore>

use kore_core::ColumnData;
use kore_store::KoreReader;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_kore <file.kore>");
    let block = KoreReader::read_file(std::path::Path::new(&path)).expect("read kore");
    println!("{} rows x {} cols", block.num_rows, block.columns.len());
    for col in &block.columns {
        let s = match &col.data {
            ColumnData::Int64(v) => {
                let nn: Vec<i64> = v.iter().filter_map(|x| *x).collect();
                let sum: i128 = nn.iter().map(|&x| x as i128).sum();
                format!("i64 nulls={} non_null={} sum={}", v.len() - nn.len(), nn.len(), sum)
            }
            ColumnData::Float64(v) => {
                let nn: Vec<f64> = v.iter().filter_map(|x| *x).collect();
                let sum: f64 = nn.iter().sum();
                format!("f64 nulls={} non_null={} sum={}", v.len() - nn.len(), nn.len(), sum)
            }
            ColumnData::Bool(v) => {
                let nn: Vec<bool> = v.iter().filter_map(|x| *x).collect();
                let trues = nn.iter().filter(|&&b| b).count();
                format!("bool nulls={} non_null={} trues={}", v.len() - nn.len(), nn.len(), trues)
            }
            ColumnData::Str(v) => {
                let nn = v.iter().filter(|x| x.is_some()).count();
                let vals: Vec<&str> = v.iter().filter_map(|x| x.as_deref()).collect();
                format!("str nulls={} non_null={} values={:?}", v.len() - nn, nn, vals)
            }
            ColumnData::StrDict { codes, dict } => {
                let nulls = codes.iter().filter(|&&c| c == u8::MAX).count();
                format!("strdict nulls={} non_null={} dict={}", nulls, codes.len() - nulls, dict.len())
            }
        };
        println!("  {} | {}", col.name, s);
    }
}
