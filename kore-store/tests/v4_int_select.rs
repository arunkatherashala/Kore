//! Version-4 i64 transform selection (docs/kore_v4_int_select.md): large i64 columns get a second
//! finalist scored on a row sample at the real zstd level, so delta is chosen for timestamp-like columns
//! where the zstd-1 pre-score preferred varint. Round trips of timestamp-like columns (monotone,
//! near-monotone, random, NULLs, i64 extremes) through version 4, version 5 and rule mode; the selector
//! picks delta when it is smaller; the writer paths still agree byte for byte; no format change.

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{codec, v4, CompactOptions, DictSet, KoreReader, KoreWriter, RuleOptions, SearchMode, Transform};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

const N: usize = 120_000;
const US: i64 = 1_000_000;
const BASE: i64 = 1_704_067_200 * US; // 2024-01-01T00:00:00Z in microseconds

/// Trip-record-like pickup times: microseconds at whole seconds, a slow trend (about 0.9 s per row) plus
/// a uniform spread of up to an hour, so consecutive values are within +-1 h of each other, as measured
/// on the NYC taxi January 2024 file (43 % of the deltas negative, median |delta| about 20 minutes).
fn taxi_like(n: usize, seed: u64) -> Vec<Option<i64>> {
    let mut r = Rng(seed);
    (0..n as i64).map(|i| Some(BASE + (i * 9 / 10 + r.below(3600) as i64) * US)).collect()
}

fn monotone(n: usize) -> Vec<Option<i64>> { (0..n as i64).map(|i| Some(BASE + i * 37 * US)).collect() }
fn near_monotone(n: usize, seed: u64) -> Vec<Option<i64>> {
    let mut r = Rng(seed);
    (0..n as i64).map(|i| Some(BASE + i * 1_000 + r.below(5_000) as i64 - 2_500)).collect()
}
fn random(n: usize, seed: u64) -> Vec<Option<i64>> { let mut r = Rng(seed); (0..n).map(|_| Some(r.next() as i64)).collect() }
fn with_nulls(v: Vec<Option<i64>>, every: usize) -> Vec<Option<i64>> { v.into_iter().enumerate().map(|(i, x)| if i % every == 3 { None } else { x }).collect() }
fn extremes(n: usize, seed: u64) -> Vec<Option<i64>> {
    let mut r = Rng(seed);
    (0..n).map(|i| match r.below(6) {
        0 => Some(i64::MIN),
        1 => Some(i64::MAX),
        2 => Some(if i % 2 == 0 { i64::MIN + 1 } else { i64::MAX - 1 }),
        3 => None,
        4 => Some(0),
        _ => Some(BASE + i as i64 * US),
    }).collect()
}

fn block(cols: Vec<(&str, Vec<Option<i64>>)>) -> DataBlock {
    DataBlock::new(cols.into_iter().map(|(n, v)| Column { name: n.to_string(), data: ColumnData::Int64(v) }).collect()).unwrap()
}

fn all_cases() -> DataBlock {
    block(vec![
        ("taxi_like", taxi_like(N, 1)),
        ("monotone", monotone(N)),
        ("near_monotone", near_monotone(N, 2)),
        ("random", random(N, 3)),
        ("taxi_nulls", with_nulls(taxi_like(N, 4), 7)),
        ("random_nulls", with_nulls(random(N, 5), 2)),
        ("extremes", extremes(N, 6)),
        ("all_null", vec![None; N]),
        ("min_max", (0..N).map(|i| Some(if i % 2 == 0 { i64::MIN } else { i64::MAX })).collect()),
    ])
}

fn ints(b: &DataBlock, name: &str) -> Vec<Option<i64>> {
    match &b.columns.iter().find(|c| c.name == name).unwrap().data { ColumnData::Int64(v) => v.clone(), _ => panic!("not i64") }
}

fn assert_same(a: &DataBlock, b: &DataBlock) {
    assert_eq!(a.num_rows, b.num_rows);
    assert_eq!(a.columns.len(), b.columns.len());
    for c in &a.columns { assert_eq!(ints(a, &c.name), ints(b, &c.name), "column {}", c.name); }
}

/// (transform id, payload length) per column, from the directory.
fn chosen(bytes: &[u8]) -> Vec<(String, u8, usize)> {
    v4::info(bytes).unwrap().columns.iter().map(|c| (c.0.clone(), c.2, c.5)).collect()
}

#[test]
fn timestamp_like_columns_round_trip_everywhere() {
    let b = all_cases();
    for o in [CompactOptions::default(), CompactOptions::fast(), CompactOptions::quick(), CompactOptions::default().with_compact_directory(true)] {
        let bytes = KoreWriter::to_bytes_compact(&b, &o).unwrap();
        assert_same(&b, &KoreReader::from_bytes(&bytes).unwrap());
        assert_same(&b, &KoreReader::from_bytes_with_dicts(&bytes, &DictSet::new()).unwrap());
        // projection and a row range
        let one = KoreReader::from_bytes_columns(&bytes, &["taxi_nulls"]).unwrap();
        assert_eq!(ints(&one, "taxi_nulls"), ints(&b, "taxi_nulls"));
        let rows = KoreReader::from_bytes_rows_with_dicts(&bytes, None, 50_000..50_100, &DictSet::new()).unwrap();
        for c in &b.columns { assert_eq!(ints(&rows, &c.name), ints(&b, &c.name)[50_000..50_100].to_vec(), "{} rows", c.name); }
    }
    // rule mode (container version 6) on the same columns
    let bytes = KoreWriter::to_bytes_rules(&b, &RuleOptions::default()).unwrap();
    assert_same(&b, &KoreReader::from_bytes(&bytes).unwrap());
}

#[test]
fn small_and_odd_sizes_round_trip() {
    for n in [0usize, 1, 2, 3, 100, 32_767, 32_768, 32_769, 65_536, 65_537] {
        let b = block(vec![("t", taxi_like(n, 9)), ("x", extremes(n, 10)), ("r", random(n, 11))]);
        for o in [CompactOptions::default(), CompactOptions::default().with_zstd_level(3)] {
            let bytes = KoreWriter::to_bytes_compact(&b, &o).unwrap();
            assert_same(&b, &KoreReader::from_bytes(&bytes).unwrap());
        }
    }
}

/// What the pre-score of the earlier writer saw: min(raw, zstd level 1 + 4) per transform.
fn zstd1_scores(data: &ColumnData) -> Vec<(Transform, usize)> {
    [Transform::Plain, Transform::Delta, Transform::Varint].into_iter().map(|t| {
        let raw = codec::encode_transform(data, t).unwrap();
        (t, raw.len().min(zstd::bulk::compress(&raw, 1).unwrap().len() + 4))
    }).collect()
}

fn zstd1_winner(data: &ColumnData) -> Transform {
    let s = zstd1_scores(data);
    let mut w = 0;
    for i in 1..s.len() { if s[i].1 < s[w].1 { w = i; } }
    s[w].0
}

#[test]
fn selector_picks_delta_for_timestamps_when_it_is_smaller() {
    let data = ColumnData::Int64(taxi_like(N, 1));
    // the zstd-1 pre-score alone prefers varint on this column (the miss this change fixes) ...
    let s = zstd1_scores(&data);
    let z1_winner = zstd1_winner(&data);
    assert_eq!(z1_winner, Transform::Varint, "pre-scores {s:?}");
    for level in [9, 19] {
        // ... while at the level that is written, delta is the smallest
        let full: Vec<(Transform, usize)> = [Transform::Plain, Transform::Delta, Transform::Varint].into_iter()
            .map(|t| (t, v4::stage_payload(&codec::encode_transform(&data, t).unwrap(), 3, level, None).len())).collect();
        assert_eq!(full.iter().min_by_key(|x| x.1).unwrap().0, Transform::Delta, "level {level}: {full:?}");
        let b = block(vec![("pickup", taxi_like(N, 1))]);
        let o = CompactOptions::default().with_zstd_level(level);
        let bytes = KoreWriter::to_bytes_compact(&b, &o).unwrap();
        let (_, t, len) = chosen(&bytes)[0].clone();
        assert_eq!(t, Transform::Delta.id().unwrap(), "level {level}");
        // never larger than the zstd stage of the zstd-1 choice (the earlier writer's candidate)
        let old = v4::stage_payload(&codec::encode_transform(&data, z1_winner).unwrap(), 3, level, None).len();
        assert!(len < old, "level {level}: {len} vs {old}");
        assert_same(&b, &KoreReader::from_bytes(&bytes).unwrap());
    }
}

#[test]
fn choice_agrees_with_the_earlier_writer_where_it_was_right() {
    // random 64-bit values: plain (8 bytes) is the smallest and the earlier choice (kept); monotone with a
    // fixed step: delta; small-range values: whatever is smallest, never larger than the earlier choice.
    let b = block(vec![("random", random(N, 3)), ("monotone", monotone(N)), ("small", { let mut r = Rng(12); (0..N).map(|_| Some(r.below(200) as i64)).collect() })]);
    let bytes = KoreWriter::to_bytes_compact(&b, &CompactOptions::default()).unwrap();
    let c = chosen(&bytes);
    assert_eq!(c[0].1, Transform::Plain.id().unwrap());
    assert!(c[0].2 <= N * 8 + 1, "random is never expanded: {}", c[0].2);
    assert_eq!(c[1].1, Transform::Delta.id().unwrap());
    assert_eq!(c[0].1, zstd1_winner(&b.columns[0].data).id().unwrap(), "random keeps the earlier choice");
    // every column: never larger than the zstd stage of the earlier (zstd-1) choice
    for (col, got) in b.columns.iter().zip(&c) {
        let old = v4::stage_payload(&codec::encode_transform(&col.data, zstd1_winner(&col.data)).unwrap(), 3, 9, None).len();
        assert!(got.2 <= old, "{}: {} vs {old}", col.name, got.2);
    }
}

#[test]
fn all_writer_paths_write_the_same_bytes_on_large_int_columns() {
    let b = all_cases();
    for o in [CompactOptions::default(), CompactOptions::default().with_zstd_level(3), CompactOptions::default().with_compact_directory(true)] {
        let r = v4::write_reference(&b, &o).unwrap();
        assert_eq!(v4::write(&b, &o).unwrap(), r);
        assert_eq!(v4::write_threads(&b, &o, false).unwrap(), r, "calling thread");
        assert_eq!(v4::write_threads(&b, &o, true).unwrap(), r, "rayon");
        assert_eq!(KoreWriter::to_bytes_compact_many(&[(&b, &o), (&b, &o)]).unwrap(), vec![r.clone(), r.clone()]);
        // only the existing transform ids are written
        assert!(chosen(&r).iter().all(|c| c.1 <= 2));
    }
}

#[test]
fn explicit_codecs_and_quick_mode_are_unchanged() {
    use kore_store::{ColumnCodec, Stage};
    let b = block(vec![("pickup", taxi_like(N, 1))]);
    // an explicit transform is always written as asked
    for t in [Transform::Plain, Transform::Delta, Transform::Varint] {
        let o = CompactOptions::default().with_column("pickup", ColumnCodec { transform: t, stage: Stage::Auto });
        let bytes = KoreWriter::to_bytes_compact(&b, &o).unwrap();
        assert_eq!(chosen(&bytes)[0].1, t.id().unwrap());
        assert_same(&b, &KoreReader::from_bytes(&bytes).unwrap());
    }
    // quick mode keeps its cheap zstd-1 choice
    let q = KoreWriter::to_bytes_compact(&b, &CompactOptions::quick()).unwrap();
    assert_eq!(q.len(), KoreWriter::to_bytes_compact(&b, &CompactOptions::default().with_search(SearchMode::Quick).with_small_zstd_level(3).with_zstd_level(3)).unwrap().len());
    assert_eq!(chosen(&q)[0].1, Transform::Varint.id().unwrap());
}
