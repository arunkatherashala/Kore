//! Deterministic version-4 writer cases. `tests/v4_writer_bytes.txt` holds the sha256 of every case as
//! written by the unmodified version-4 writer of bbbefd2c (generated from a detached bbbefd2c checkout
//! with a scratch example that includes this file; only the API of bbbefd2c is used here).
#![allow(dead_code)]
use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{ColumnCodec, CompactOptions, SharedDict, Stage, Transform};

fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut x = seed;
    move || {
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

const WORDS: [&str; 24] = ["the", "meeting", "moved", "to", "tuesday", "and", "Mara", "agreed", "deploy", "release",
    "staging", "smoke", "tests", "passed", "battery", "sensor", "book", "chapter", "systems", "notes", "call", "plan", "done", "later"];

fn sentence(r: &mut impl FnMut() -> u64, words: usize) -> String {
    let mut s = String::new();
    for j in 0..words { if j > 0 { s.push(' '); } s.push_str(WORDS[(r() % WORDS.len() as u64) as usize]); }
    s.push('.');
    s
}

fn ts(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Events-like block; `words` controls the text size (12 words * 1000 rows is about 80 KB of text).
pub fn events(n: usize, seed: u64, words: usize) -> DataBlock {
    let mut r = rng(seed);
    let ids: Vec<Option<String>> = (0..n).map(|_| Some(format!("{:016x}{:016x}{:016x}{:016x}", r(), r(), r(), r()))).collect();
    let mut r = rng(seed + 1);
    let text: Vec<Option<String>> = (0..n).map(|k| if k % 41 == 7 { None } else { { let w = 3 + (r() as usize % words.max(1)); Some(sentence(&mut r, w)) } }).collect();
    let mut r = rng(seed + 2);
    DataBlock { num_rows: n, columns: vec![
        Column { name: "event_id".into(), data: ColumnData::Str(ids) },
        Column { name: "occurred_at".into(), data: ColumnData::Str((0..n).map(|k| Some(ts(1_767_225_600 + 37 * k as i64 + (r() % 30) as i64))).collect()) },
        Column { name: "recorded_at".into(), data: ColumnData::Str((0..n).map(|_| Some("2026-01-01T00:00:00Z".to_string())).collect()) },
        Column { name: "kind".into(), data: ColumnData::Str((0..n).map(|k| if k % 19 == 4 { None } else { Some(["note", "action", "decision", "observation"][(r() % 4) as usize].to_string()) }).collect()) },
        Column { name: "text".into(), data: ColumnData::Str(text) },
        Column { name: "tags".into(), data: ColumnData::Str((0..n).map(|k| match k % 6 { 0 => Some(String::new()), 3 => None, _ => Some((0..1 + r() % 3).map(|_| format!("tag{:02}", r() % 30)).collect::<Vec<_>>().join(",")) }).collect()) },
        Column { name: "conf".into(), data: ColumnData::Float64((0..n).map(|k| if k % 9 == 0 { None } else { Some((r() % 1000) as f64 / 1000.0) }).collect()) },
        Column { name: "seq".into(), data: ColumnData::Int64((0..n as i64).map(|k| Some(k * 3 + (r() % 3) as i64)).collect()) },
        Column { name: "flag".into(), data: ColumnData::Bool((0..n).map(|k| if k % 13 == 0 { None } else { Some(r() % 2 == 0) }).collect()) },
    ] }
}

/// One large comma-list column (tokens should win over plain) plus one large free-text column with
/// commas (plain should win), both above the speculation threshold.
pub fn lists(n: usize, seed: u64) -> DataBlock {
    let mut r = rng(seed);
    DataBlock { num_rows: n, columns: vec![
        Column { name: "list".into(), data: ColumnData::Str((0..n).map(|_| Some((0..2 + r() % 6).map(|_| format!("label-{:03}", r() % 200)).collect::<Vec<_>>().join(","))).collect()) },
        Column { name: "prose".into(), data: ColumnData::Str((0..n).map(|_| Some(format!("{}, {} {:x}", sentence(&mut r, 6), sentence(&mut r, 5), r()))).collect()) },
    ] }
}

fn dict_of(b: &DataBlock, col: usize, max: usize) -> SharedDict {
    let mut c = Vec::new();
    if let ColumnData::Str(v) = &b.columns[col].data { for s in v.iter().flatten() { c.extend_from_slice(s.as_bytes()); c.push(b'\n'); } }
    let start = c.len().saturating_sub(max);
    SharedDict::from_content(&c[start..]).unwrap()
}

/// (name, block, options) for every case.
pub fn cases() -> Vec<(String, DataBlock, CompactOptions)> {
    let small = events(300, 11, 6);
    let seg = events(1000, 12, 12);
    let big = events(4000, 13, 14);
    let d = dict_of(&events(1000, 99, 12), 4, 64 << 10);
    let l = lists(6000, 14);
    let empty = events(0, 15, 4);
    let mut v: Vec<(String, DataBlock, CompactOptions)> = vec![
        ("small_auto".into(), small.clone(), CompactOptions::new()),
        ("small_dict".into(), small.clone(), CompactOptions::new().with_dict("text", d.clone())),
        ("seg_auto".into(), seg.clone(), CompactOptions::new()),
        ("seg_dict".into(), seg.clone(), CompactOptions::new().with_dict("text", d.clone())),
        ("seg_dict_lvl3".into(), seg.clone(), CompactOptions::new().with_dict("text", d.clone()).with_zstd_level(3)),
        ("big_auto".into(), big.clone(), CompactOptions::new()),
        ("big_dict".into(), big.clone(), CompactOptions::new().with_dict("text", d.clone())),
        ("big_lvl19".into(), big.clone(), CompactOptions::new().with_zstd_level(19).with_dict("text", d.clone())),
        ("lists_auto".into(), l.clone(), CompactOptions::new()),
        ("lists_dict".into(), l.clone(), CompactOptions::new().with_dict("prose", dict_of(&l, 1, 32 << 10)).with_dict("list", dict_of(&l, 0, 16 << 10))),
        ("empty".into(), empty, CompactOptions::new()),
    ];
    // explicit codecs (single transform and/or single stage)
    let combos = [
        (Transform::Plain, Stage::Zstd), (Transform::Plain, Stage::Hz), (Transform::Tokens, Stage::Auto),
        (Transform::Auto, Stage::Zstd), (Transform::Auto, Stage::Lz4), (Transform::Plain, Stage::None), (Transform::Auto, Stage::Hz),
    ];
    for (i, (t, s)) in combos.iter().enumerate() {
        v.push((format!("seg_text_{i}"), seg.clone(), CompactOptions::new().with_dict("text", d.clone()).with_column("text", ColumnCodec::new(*t, *s))));
    }
    v.push(("seg_ts_hex".into(), seg.clone(), CompactOptions::new()
        .with_column("occurred_at", ColumnCodec::new(Transform::TsText, Stage::Auto))
        .with_column("event_id", ColumnCodec::new(Transform::HexFixed, Stage::Zstd))
        .with_column("seq", ColumnCodec::new(Transform::Delta, Stage::Auto))));
    v
}
