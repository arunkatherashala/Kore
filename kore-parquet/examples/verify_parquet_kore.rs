//! Verify a converted KORE file against its source Parquet, cell by cell.
//!
//! Usage: cargo run -p kore-parquet --release --example verify_parquet_kore -- <input.parquet> <output.kore>

use kore_core::{ColumnData, DataBlock};
use kore_parquet::ParquetReader;
use kore_store::KoreReader;

fn col_summary(data: &ColumnData) -> String {
    match data {
        ColumnData::Int64(v) => {
            let nn: Vec<i64> = v.iter().filter_map(|x| *x).collect();
            let nulls = v.len() - nn.len();
            let sum: i128 = nn.iter().map(|&x| x as i128).sum();
            format!("i64 nulls={nulls} non_null={} sum={sum}", nn.len())
        }
        ColumnData::Float64(v) => {
            let nn: Vec<f64> = v.iter().filter_map(|x| *x).collect();
            let nulls = v.len() - nn.len();
            let sum: f64 = nn.iter().sum();
            format!("f64 nulls={nulls} non_null={} sum={sum:.4}", nn.len())
        }
        ColumnData::Bool(v) => {
            let nn: Vec<bool> = v.iter().filter_map(|x| *x).collect();
            let nulls = v.len() - nn.len();
            let trues = nn.iter().filter(|&&b| b).count();
            format!("bool nulls={nulls} non_null={} trues={trues}", nn.len())
        }
        ColumnData::Str(v) => {
            let nn = v.iter().filter(|x| x.is_some()).count();
            format!("str nulls={} non_null={nn}", v.len() - nn)
        }
        ColumnData::StrDict { codes, dict } => {
            let nulls = codes.iter().filter(|&&c| c == u8::MAX).count();
            format!("strdict nulls={nulls} non_null={} dict={}", codes.len() - nulls, dict.len())
        }
    }
}

fn compare_cells(a: &ColumnData, b: &ColumnData) -> Option<(usize, String, String)> {
    let n = a.len().min(b.len());
    for i in 0..n {
        let (va, vb) = (a.get_value(i), b.get_value(i));
        if va != vb {
            return Some((i, format!("{va:?}"), format!("{vb:?}")));
        }
    }
    None
}

fn main() {
    let mut args = std::env::args().skip(1);
    let parquet = args.next().expect("parquet path");
    let kore = args.next().expect("kore path");

    let pq: DataBlock = ParquetReader::new(&parquet).read().expect("read parquet");
    let kr: DataBlock = KoreReader::read_file(std::path::Path::new(&kore)).expect("read kore");

    println!("parquet: {} rows x {} cols", pq.num_rows, pq.columns.len());
    println!("kore:    {} rows x {} cols", kr.num_rows, kr.columns.len());
    println!("row_count_match: {}", pq.num_rows == kr.num_rows);
    println!("col_count_match: {}", pq.columns.len() == kr.columns.len());
    println!();

    let mut all_match = true;
    for (ca, cb) in pq.columns.iter().zip(kr.columns.iter()) {
        let name_ok = ca.name == cb.name;
        let dtype_ok = ca.data.dtype() == cb.data.dtype();
        let mismatch = compare_cells(&ca.data, &cb.data);
        let ok = name_ok && dtype_ok && mismatch.is_none();
        all_match &= ok;
        println!(
            "[{}] {} | parquet: {} | kore: {}",
            if ok { "OK " } else { "BAD" },
            ca.name,
            col_summary(&ca.data),
            col_summary(&cb.data),
        );
        if !name_ok {
            println!("      name mismatch: parquet='{}' kore='{}'", ca.name, cb.name);
        }
        if !dtype_ok {
            println!("      dtype mismatch: parquet={:?} kore={:?}", ca.data.dtype(), cb.data.dtype());
        }
        if let Some((i, va, vb)) = mismatch {
            println!("      first cell mismatch at row {i}: parquet={va} kore={vb}");
        }
    }

    println!();
    println!("ALL_COLUMNS_MATCH: {all_match}");
}
