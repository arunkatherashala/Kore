//! Dev probe: `cargo run --release -p kore-sql --example probe -- file.sql` runs one statement per line
//! against the same fixture tables as tests/sql_coverage.rs and prints `[types] rows`.
use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

fn s(v: &[Option<&str>]) -> Vec<Option<String>> { v.iter().map(|x| x.map(|y| y.to_string())).collect() }

fn main() {
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::int64("id", (1..=6).map(Some).collect()),
        Column::str_col("g", s(&[Some("x"), Some("x"), Some("y"), Some("y"), Some("z"), None])),
        Column::int64("v", vec![Some(10), Some(20), Some(30), None, Some(50), Some(60)]),
        Column::str_col("s", s(&[Some("Hello"), Some("  pad "), Some("abc"), None, Some("a,b,c"), Some("World")])),
        Column::float64("f", vec![Some(1.5), Some(2.5), Some(-3.5), None, Some(4.0), Some(0.0)]),
        Column::str_col("d", s(&[Some("2024-01-15"), Some("2024-02-29"), Some("2023-12-31"), None, Some("2024-03-01"), Some("2024-01-01")])),
    ]).unwrap());
    c.register("u", DataBlock::new(vec![
        Column::int64("id", vec![Some(1), Some(2), Some(2), Some(7)]),
        Column::str_col("w", s(&[Some("p"), Some("q"), Some("r"), Some("s")])),
    ]).unwrap());
    let path = std::env::args().nth(1).expect("file");
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("--") { continue; }
        let first = l.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        if ["insert", "update", "delete", "create", "drop", "merge"].contains(&first.as_str()) {
            match c.execute_dml(l) { Ok(r) => println!("{l}
   => DML {:?}", r), Err(e) => println!("{l}
   => DML ERR {e}") }
            continue;
        }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.query(l)));
        match r {
            Err(_) => println!("{l}\n   => PANIC"),
            Ok(Err(e)) => println!("{l}\n   => ERR {e}"),
            Ok(Ok(b)) => {
                let n = b.columns.first().map(|c| c.data.len()).unwrap_or(0);
                let ty: Vec<&str> = b.columns.iter().map(|c| match &c.data { ColumnData::Int64(_) => "i", ColumnData::Float64(_) => "f", ColumnData::Bool(_) => "b", _ => "s" }).collect();
                let mut rows = vec![];
                for r in 0..n {
                    let cells: Vec<String> = b.columns.iter().map(|c| match &c.data {
                        ColumnData::Int64(v) => v[r].map(|x| x.to_string()).unwrap_or("NULL".into()),
                        ColumnData::Float64(v) => v[r].map(|x| x.to_string()).unwrap_or("NULL".into()),
                        ColumnData::Bool(v) => v[r].map(|x| x.to_string()).unwrap_or("NULL".into()),
                        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
                        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
                    }).collect();
                    rows.push(cells.join(","));
                }
                println!("{l}\n   => [{}] {}", ty.join(""), rows.join(" | "));
            }
        }
    }
}
