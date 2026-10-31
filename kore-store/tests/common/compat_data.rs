//! Deterministic datasets shared by the fixture generator and the compatibility tests.
#![allow(dead_code)]
use kore_core::{Column, ColumnData, DataBlock};

pub fn mixed() -> DataBlock {
    let n = 300usize;
    let ints = (0..n).map(|k| if k % 10 == 3 { None } else { Some((k as i64 * 7919) % 1000 - 500) }).collect();
    let floats = (0..n).map(|k| if k % 13 == 2 { None } else if k % 50 == 5 { Some(f64::NAN) } else { Some(k as f64 / 8.0) }).collect();
    let bools = (0..n).map(|k| if k % 7 == 0 { None } else { Some(k % 3 == 0) }).collect();
    let strs = (0..n).map(|k| if k % 11 == 0 { None } else if k % 17 == 1 { Some(String::new()) }
        else { Some(format!("row-{k} caf\u{e9} \u{1f600} {}", "x".repeat(k % 40))) }).collect();
    DataBlock { num_rows: n, columns: vec![
        Column { name: "i".into(), data: ColumnData::Int64(ints) },
        Column { name: "f".into(), data: ColumnData::Float64(floats) },
        Column { name: "b".into(), data: ColumnData::Bool(bools) },
        Column { name: "s".into(), data: ColumnData::Str(strs) },
    ] }
}

pub fn sorted_ints() -> DataBlock {
    let n = 5000usize;
    let ts = (0..n).map(|k| Some(1_700_000_000_000 + (k as i64) * 1000 + (k as i64 % 7))).collect();
    DataBlock { num_rows: n, columns: vec![Column { name: "ts".into(), data: ColumnData::Int64(ts) }] }
}

pub fn lowcard_strs() -> DataBlock {
    let n = 2500usize; // > 1000 and < 255 distinct -> native dictionary path
    let tags = (0..n).map(|k| if k % 97 == 0 { None } else { Some(["alpha", "beta", "gamma", "delta"][k % 4].to_string()) }).collect();
    let dict_vals: Vec<String> = vec!["a".into(), "bb".into(), "ccc".into()];
    let codes = (0..n).map(|k| if k % 5 == 0 { u8::MAX } else { (k % 3) as u8 }).collect();
    DataBlock { num_rows: n, columns: vec![
        Column { name: "tag".into(), data: ColumnData::Str(tags) },
        Column { name: "sd".into(), data: ColumnData::StrDict { codes, dict: dict_vals } },
    ] }
}

pub fn text() -> DataBlock {
    let n = 400usize;
    let s = (0..n).map(|k| Some(format!("user {} said: the quick brown fox jumps over the lazy dog {} times", k % 17, k % 5))).collect();
    let id = (0..n).map(|k| Some(format!("{:016x}{:016x}", (k as u64).wrapping_mul(0x9E3779B97F4A7C15), (k as u64).wrapping_mul(31)))).collect();
    DataBlock { num_rows: n, columns: vec![
        Column { name: "text".into(), data: ColumnData::Str(s) },
        Column { name: "id".into(), data: ColumnData::Str(id) },
    ] }
}
