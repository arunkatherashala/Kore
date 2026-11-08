//! Row-level partial decode (`v4::read_rows`, `KoreReader::from_bytes_rows_with_dicts`), the pruned
//! version-4 writer search (same bytes as the b44d2a91 search), the explicit `quick()` preset, container
//! version 5 (varint directory), `DictSet` iteration and hex ids in `v4::info`.
//!
//! Property: for every file and every row range and projection, the partial decode returns exactly
//! the rows of the full decode, and it fails exactly when the full decode fails (also on corrupted,
//! truncated and re-checksummed input). It never panics.

#[path = "common/v4_cases.rs"]
mod v4_cases;

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::codec::{self, ColumnCodec, Stage, Transform};
use kore_store::{v4, CompactOptions, DType, DictSet, KoreReader, SearchMode, SharedDict};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static PANICS: AtomicUsize = AtomicUsize::new(0);

/// Count panics (also the ones `catch_unwind` inside the readers would swallow).
fn count_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        // only panics raised inside the library (test assertions live in tests/)
        std::panic::set_hook(Box::new(move |i| {
            if i.location().map_or(true, |l| !l.file().replace('\\', "/").contains("tests/")) { PANICS.fetch_add(1, Ordering::SeqCst); }
            prev(i)
        }));
    });
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize { if n == 0 { 0 } else { (self.next() % n as u64) as usize } }
}

fn cell(d: &ColumnData, i: usize) -> String {
    match d {
        ColumnData::Float64(v) => format!("{:?}", v[i].map(f64::to_bits)),
        ColumnData::StrDict { codes, dict } => format!("{:?}", if codes[i] == u8::MAX { None } else { dict.get(codes[i] as usize) }),
        _ => format!("{:?}", d.get_value(i)),
    }
}

/// `part` must be rows `r` of `full`.
fn assert_rows(full: &DataBlock, part: &DataBlock, r: &Range<usize>, what: &str) {
    assert_eq!(part.num_rows, r.end - r.start, "{what}: row count");
    assert_eq!(part.columns.len(), full.columns.len(), "{what}: column count");
    for (f, p) in full.columns.iter().zip(&part.columns) {
        assert_eq!(f.name, p.name, "{what}: column order");
        assert_eq!(p.data.len(), r.end - r.start, "{what}: column '{}' length", p.name);
        for (k, i) in r.clone().enumerate() {
            assert_eq!(cell(&f.data, i), cell(&p.data, k), "{what}: column '{}' row {i}", f.name);
        }
    }
}

fn ranges(n: usize, rng: &mut Rng) -> Vec<Range<usize>> {
    let mut v = vec![0..n, 0..0, n..n];
    if n <= 64 { v.extend((0..n).map(|i| i..i + 1)); } else { v.extend([0..1, n - 1..n, n / 2..n / 2 + 1]); }
    for _ in 0..12 { let a = rng.below(n + 1); let b = rng.below(n + 1); v.push(a.min(b)..a.max(b)); }
    for _ in 0..8 { let a = rng.below(n.max(1)); v.push(a..(a + 1).min(n)); }
    v
}

/// Full read vs every range through the public API; returns false if the full read failed.
fn check_file(data: &[u8], dicts: &DictSet, wanted: Option<&[&str]>, rng: &mut Rng, what: &str) -> bool {
    let full = match wanted {
        None => KoreReader::from_bytes_with_dicts(data, dicts),
        Some(w) => KoreReader::from_bytes_columns_with_dicts(data, w, dicts),
    };
    let full = match full {
        Ok(f) => f,
        Err(_) => {
            for r in [0..0, 0..1] { assert!(KoreReader::from_bytes_rows_with_dicts(data, wanted, r, dicts).is_err(), "{what}: partial Ok where full failed"); }
            return false;
        }
    };
    let n = full.num_rows;
    for r in ranges(n, rng) {
        let p = KoreReader::from_bytes_rows_with_dicts(data, wanted, r.clone(), dicts).unwrap_or_else(|e| panic!("{what} rows {r:?}: {e}"));
        assert_rows(&full, &p, &r, what);
        if v4::is_compact(data) {
            let p = v4::read_rows(data, wanted, r.clone(), dicts).unwrap();
            assert_rows(&full, &p, &r, what);
        }
    }
    assert!(KoreReader::from_bytes_rows_with_dicts(data, wanted, 0..n + 1, dicts).is_err(), "{what}: out of range accepted");
    #[allow(clippy::reversed_empty_ranges)]
    let backwards = 1..0;
    assert!(n == 0 || KoreReader::from_bytes_rows_with_dicts(data, wanted, backwards, dicts).is_err());
    true
}

fn fixtures() -> (Vec<(String, Vec<u8>)>, DictSet) {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut out = vec![];
    for dir in [base.join("tests").join("fixtures"), base.join("tests").join("fixtures").join("dl_log4"), base.join("..").join("conformance")] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |e| e == "kore")).collect();
        files.sort();
        for f in files { out.push((f.display().to_string(), std::fs::read(&f).unwrap())); }
    }
    let dicts = DictSet::load_dir(&base.join("tests").join("fixtures").join("dl_log4")).unwrap();
    (out, dicts)
}

fn col_names(data: &[u8], dicts: &DictSet) -> Vec<String> {
    KoreReader::from_bytes_with_dicts(data, dicts).map(|b| b.columns.into_iter().map(|c| c.name).collect()).unwrap_or_default()
}

#[test]
fn partial_equals_full_on_every_fixture() {
    count_panics();
    let (files, dicts) = fixtures();
    assert!(files.len() >= 30, "fixtures found: {}", files.len());
    let mut rng = Rng(1);
    let (mut ok, mut v4n) = (0, 0);
    for (name, data) in &files {
        if v4::is_v4(data) { v4n += 1; }
        if check_file(data, &dicts, None, &mut rng, name) { ok += 1; }
        let cols = col_names(data, &dicts);
        for c in &cols { check_file(data, &dicts, Some(&[c.as_str()]), &mut rng, &format!("{name} [{c}]")); }
        if cols.len() >= 2 {
            let w = [cols[cols.len() - 1].as_str(), cols[0].as_str()];
            check_file(data, &dicts, Some(&w), &mut rng, &format!("{name} reversed"));
        }
    }
    assert!(v4n >= 12, "Digital Life format-4 fixtures: {v4n}");
    assert!(ok >= files.len() - 1, "{ok} of {} fixtures read (only the encrypted one may fail)", files.len());
    // a format-4 events fixture needs its dictionary for `text`, not for the other columns
    let ev = &files.iter().find(|(n, _)| n.ends_with("events_0000000001.kore")).unwrap().1;
    let e = v4::read_row(ev, None, 0, &DictSet::new()).unwrap_err().to_string();
    assert!(e.contains("needs shared dictionary"), "{e}");
    assert_eq!(v4::read_row(ev, Some(&["event_id", "kind"]), 0, &DictSet::new()).unwrap().num_rows, 1);
    assert_eq!(PANICS.load(Ordering::SeqCst), 0);
}

fn dicts_of(o: &CompactOptions) -> DictSet { o.dicts.iter().map(|(_, d)| d.clone()).collect() }

#[test]
fn partial_equals_full_on_writer_cases_v4_v5_quick() {
    count_panics();
    let mut rng = Rng(2);
    for (name, b, o) in v4_cases::cases() {
        let ds = dicts_of(&o);
        for (label, opts) in [("v4", o.clone()), ("v5", o.clone().with_compact_directory(true)), ("quick", { let mut q = o.clone(); q.search = SearchMode::Quick; q })] {
            let f = v4::write(&b, &opts).unwrap();
            assert!(check_file(&f, &ds, None, &mut rng, &format!("{name} {label}")));
            for c in &b.columns { check_file(&f, &ds, Some(&[c.name.as_str()]), &mut rng, &format!("{name} {label} [{}]", c.name)); }
        }
    }
    assert_eq!(PANICS.load(Ordering::SeqCst), 0);
}

// ── random data ──────────────────────────────────────────────────────────────

fn random_str(rng: &mut Rng, kind: usize) -> String {
    match kind {
        0 => format!("{:016x}{:016x}", rng.next(), rng.next()),
        1 => format!("2026-{:02}-{:02}T{:02}:{:02}:{:02}Z", 1 + rng.below(12), 1 + rng.below(28), rng.below(24), rng.below(60), rng.below(60)),
        2 => ["a", "bb", "", "ccc", "naïve", "日本"][rng.below(6)].to_string(),
        3 => (0..rng.below(4)).map(|_| format!("t{}", rng.below(9))).collect::<Vec<_>>().join(","),
        _ => (0..rng.below(30)).map(|_| ["the", "x", "é", "😀", " ", "long-word", ","][rng.below(7)]).collect(),
    }
}

fn random_block(rng: &mut Rng, n: usize) -> DataBlock {
    let null_every = [0usize, 2, 5, 97][rng.below(4)];
    let isnull = |rng: &mut Rng| null_every > 0 && rng.below(null_every) == 0;
    let mut cols = vec![];
    for (c, kind) in (0..2 + rng.below(6)).map(|c| (c, rng.below(9))).collect::<Vec<_>>() {
        let name = format!("c{c}");
        let data = match kind {
            0 => ColumnData::Int64((0..n).map(|i| if isnull(rng) { None } else { Some(match rng.below(3) { 0 => i as i64 * 3, 1 => rng.next() as i64, _ => -(rng.below(100) as i64) }) }).collect()),
            1 => ColumnData::Float64((0..n).map(|_| if isnull(rng) { None } else { Some([f64::NAN, -0.0, 1.5, f64::INFINITY, rng.next() as f64 / 7.0][rng.below(5)]) }).collect()),
            2 => ColumnData::Bool((0..n).map(|_| if isnull(rng) { None } else { Some(rng.below(2) == 0) }).collect()),
            k => { let sk = k - 3; ColumnData::Str((0..n).map(|_| if isnull(rng) { None } else { Some(random_str(rng, sk)) }).collect()) }
        };
        cols.push(Column { name, data });
    }
    DataBlock { num_rows: n, columns: cols }
}

fn valid_transforms(d: &ColumnData) -> Vec<Transform> {
    match d {
        ColumnData::Int64(_) => vec![Transform::Plain, Transform::Delta, Transform::Varint],
        ColumnData::Str(v) => {
            let mut t = vec![Transform::Plain, Transform::Dict, Transform::Tokens];
            if codec::encode_transform(d, Transform::HexFixed).is_ok() { t.push(Transform::HexFixed); }
            if v.iter().flatten().all(|s| codec::encode_transform(&ColumnData::Str(vec![Some(s.clone())]), Transform::TsText).is_ok()) { t.push(Transform::TsText); }
            t
        }
        _ => vec![Transform::Plain],
    }
}

fn random_options(rng: &mut Rng, b: &DataBlock) -> CompactOptions {
    let mut o = CompactOptions::new();
    for c in &b.columns {
        let ts = valid_transforms(&c.data);
        let t = if rng.below(2) == 0 { Transform::Auto } else { ts[rng.below(ts.len())] };
        let s = [Stage::Auto, Stage::Auto, Stage::None, Stage::Lz4, Stage::Hz, Stage::Zstd][rng.below(6)];
        if t != Transform::Auto || s != Stage::Auto { o = o.with_column(&c.name, ColumnCodec::new(t, s)); }
        if matches!(c.data, ColumnData::Str(_)) && rng.below(3) == 0 {
            let mut content = vec![];
            if let ColumnData::Str(v) = &c.data { for s in v.iter().flatten().take(50) { content.extend_from_slice(s.as_bytes()); } }
            content.extend_from_slice(b"dictionary seed text, tags t1,t2 2026-01-01T00:00:00Z");
            o = o.with_dict(&c.name, SharedDict::from_content(&content).unwrap());
        }
    }
    if rng.below(3) == 0 { o.small_zstd_level = [1, 3, 19][rng.below(3)]; }
    if rng.below(4) == 0 { o.search = SearchMode::Quick; }
    if rng.below(2) == 0 { o.compact_directory = true; }
    o
}

#[test]
fn partial_equals_full_on_random_files() {
    count_panics();
    let mut rng = Rng(3);
    for k in 0..160 {
        let n = [0usize, 1, 2, 3, 9, 64, 257, 1200][k % 8];
        let b = random_block(&mut rng, n);
        let o = random_options(&mut rng, &b);
        let f = v4::write(&b, &o).unwrap();
        let ds = dicts_of(&o);
        let what = format!("random file {k} (n={n})");
        assert!(check_file(&f, &ds, None, &mut rng, &what));
        let c = &b.columns[rng.below(b.columns.len())].name;
        check_file(&f, &ds, Some(&[c.as_str()]), &mut rng, &what);
        // the values are the written ones
        let back = v4::read_rows(&f, None, 0..n, &ds).unwrap();
        assert_rows(&b, &back, &(0..n), &what);
    }
    assert_eq!(PANICS.load(Ordering::SeqCst), 0);
}

/// Transform level, checksums out of the way: mutated payloads. `decode_transform_rows` must fail
/// exactly when `decode_transform` fails, and otherwise return its rows.
#[test]
fn transform_rows_match_full_decoder_on_mutated_payloads() {
    count_panics();
    let mut rng = Rng(4);
    let (mut both_ok, mut both_err) = (0usize, 0usize);
    for k in 0..220 {
        let n = [0usize, 1, 2, 5, 8, 9, 33, 200][k % 8];
        let b = random_block(&mut rng, n);
        for c in &b.columns {
            for t in valid_transforms(&c.data) {
                let Ok(raw) = codec::encode_transform(&c.data, t) else { continue };
                let dt = codec::dtype_of(&c.data);
                for m in 0..24 {
                    let mut p = raw.clone();
                    let mut nn = n;
                    match m % 8 {
                        0 => {}
                        1 | 2 if !p.is_empty() => { let i = rng.below(p.len()); p[i] ^= 1 << rng.below(8); }
                        3 if !p.is_empty() => { let i = rng.below(p.len()); p[i] = rng.next() as u8; }
                        4 => { p.truncate(rng.below(p.len() + 1)); }
                        5 => { let i = rng.below(p.len() + 1); p.insert(i, rng.next() as u8); }
                        6 => { nn = if rng.below(2) == 0 { n + 1 } else { n.saturating_sub(1) }; }
                        _ => { for _ in 0..3 { if !p.is_empty() { let i = rng.below(p.len()); p[i] = [0, 0x7f, 0x80, 0xff, 0xc3][rng.below(5)]; } } }
                    }
                    let full = codec::decode_transform(&p, dt, t, nn);
                    for r in ranges(nn, &mut rng).into_iter().take(6) {
                        let part = codec::decode_transform_rows(&p, dt, t, nn, r.start, r.end);
                        match (&full, part) {
                            (Ok(f), Ok(q)) => {
                                both_ok += 1;
                                let fb = DataBlock { num_rows: nn, columns: vec![Column { name: "x".into(), data: f.clone() }] };
                                let pb = DataBlock { num_rows: r.end - r.start, columns: vec![Column { name: "x".into(), data: q }] };
                                assert_rows(&fb, &pb, &r, &format!("{t:?} mutation {m}"));
                            }
                            (Err(_), Err(_)) => both_err += 1,
                            (f, q) => panic!("{t:?} {dt:?} mutation {m} n={nn} rows {r:?}: full {:?} partial {:?}", f.as_ref().map(|_| "ok"), q.as_ref().map(|_| "ok")),
                        }
                    }
                }
            }
        }
    }
    assert!(both_ok > 1000 && both_err > 1000, "{both_ok} ok, {both_err} err");
    assert_eq!(PANICS.load(Ordering::SeqCst), 0);
}

/// Files with one byte changed (three masks), truncated at every length, or extended: the row reader
/// fails exactly when the full reader fails (every change is caught by a checksum or a length check
/// when all columns are read), never panics and never returns other values.
#[test]
fn corrupted_and_truncated_files_never_give_wrong_rows() {
    count_panics();
    let b = v4_cases::events(40, 21, 5);
    let d = SharedDict::from_content(b"the meeting moved to tuesday and Mara agreed. deploy release staging").unwrap();
    let ds = DictSet::new().with(d.clone());
    let mut files = vec![];
    for compact in [false, true] {
        files.push(v4::write(&b, &CompactOptions::new().with_dict("text", d.clone()).with_compact_directory(compact)).unwrap());
        files.push(v4::write(&b, &CompactOptions::new().with_compact_directory(compact)).unwrap());
    }
    let mut rng = Rng(5);
    for f in &files {
        let good = KoreReader::from_bytes_with_dicts(f, &ds).unwrap();
        let mut variants: Vec<Vec<u8>> = vec![];
        for i in 0..f.len() { for mask in [0x01u8, 0x80, 0xff] { let mut g = f.clone(); g[i] ^= mask; variants.push(g); } }
        for l in 0..f.len() { variants.push(f[..l].to_vec()); }
        let mut g = f.clone(); g.push(0); variants.push(g);
        for g in &variants {
            for wanted in [None, Some(&["text"][..]), Some(&["seq", "event_id"][..])] {
                let full = match wanted { None => v4::read(g, None, &ds), Some(w) => v4::read(g, Some(w), &ds) };
                if wanted.is_none() { assert!(full.is_err(), "a changed file read without error"); }
                for r in [0..1, 39..40, 5..25, 0..40] {
                    let part = v4::read_rows(g, wanted, r.clone(), &ds);
                    match (&full, part) {
                        (Err(_), Err(_)) => {}
                        (Ok(fb), Ok(p)) => {
                            assert_rows(fb, &p, &r, "projection of a file changed elsewhere");
                            // and those are the original values
                            let orig = v4::read(f, wanted, &ds).unwrap();
                            assert_rows(&orig, &p, &r, "original values");
                        }
                        (f2, p) => panic!("full {:?} vs partial {:?}", f2.is_ok(), p.is_ok()),
                    }
                }
            }
        }
        assert_eq!(good.num_rows, 40);
    }
    // re-checksummed forgeries (a writer bug or a forger): the payload decoders alone must agree
    let raw_files: Vec<Vec<u8>> = files.iter().map(|f| f.clone()).collect();
    for f in &raw_files {
        let info = v4::info(f).unwrap();
        let dir_end = f.len() - info.columns.iter().map(|c| c.5).sum::<usize>();
        for _ in 0..400 {
            let mut g = f.clone();
            let i = dir_end + rng.below(f.len() - dir_end);
            g[i] ^= 1 << rng.below(8);
            refresh_crcs(&mut g, dir_end);
            let full = v4::read(&g, None, &ds);
            for r in [0..1, 3..4, 0..40] {
                let part = v4::read_rows(&g, None, r.clone(), &ds);
                match (&full, part) {
                    (Err(_), Err(_)) => {}
                    (Ok(fb), Ok(p)) => assert_rows(fb, &p, &r, "re-checksummed forgery"),
                    (f2, p) => panic!("forgery: full {:?} vs partial {:?}", f2.is_ok(), p.is_ok()),
                }
            }
        }
    }
    assert_eq!(PANICS.load(Ordering::SeqCst), 0);
}

/// Recompute every payload CRC and the directory CRC of a version-4/5 file in place.
fn refresh_crcs(g: &mut [u8], dir_end: usize) {
    use kore_store::compress::crc32;
    let info = v4::info_unchecked(g).expect("directory");
    let mut at = dir_end;
    let mut crcs = vec![];
    for c in &info { crcs.push(crc32(&g[at..at + c.1])); at += c.1; }
    for (c, crc) in info.iter().zip(crcs) { g[c.0..c.0 + 4].copy_from_slice(&crc.to_le_bytes()); }
    let dc = crc32(&g[..dir_end - 4]);
    g[dir_end - 4..dir_end].copy_from_slice(&dc.to_le_bytes());
}

// ── writer ───────────────────────────────────────────────────────────────────

#[test]
fn pruned_search_writes_the_reference_bytes() {
    let mut rng = Rng(6);
    for (name, b, o) in v4_cases::cases() {
        let r = v4::write_reference(&b, &o).unwrap();
        assert_eq!(v4::write(&b, &o).unwrap(), r, "{name}");
        assert_eq!(v4::write_threads(&b, &o, false).unwrap(), r, "{name} on the calling thread");
        assert_eq!(v4::write_threads(&b, &o, true).unwrap(), r, "{name} with rayon");
    }
    for k in 0..300 {
        let n = [0usize, 1, 1, 2, 3, 4, 10, 50, 600][k % 9];
        let b = random_block(&mut rng, n);
        let mut o = random_options(&mut rng, &b);
        o.search = SearchMode::Full;
        o.compact_directory = false;
        let r = v4::write_reference(&b, &o).unwrap();
        assert_eq!(v4::write(&b, &o).unwrap(), r, "random block {k}");
        assert_eq!(v4::write_threads(&b, &o, false).unwrap(), r, "random block {k}, calling thread");
        assert_eq!(v4::write_threads(&b, &o, true).unwrap(), r, "random block {k}, rayon");
    }
}

/// The lower bounds the pruning relies on: no stage payload of a non-empty input is shorter.
#[test]
fn stage_payload_lower_bounds_hold() {
    let mut rng = Rng(7);
    let mut inputs: Vec<Vec<u8>> = vec![];
    for len in 1..=48usize {
        inputs.push(vec![0; len]);
        inputs.push(vec![b'a'; len]);
        inputs.push((0..len).map(|i| i as u8).collect());
        for _ in 0..6 { inputs.push((0..len).map(|_| rng.next() as u8).collect()); }
    }
    let mut min = std::collections::BTreeMap::new();
    for raw in &inputs {
        // a dictionary that contains the input itself is the best case for zstd
        let d = SharedDict::from_content(&[raw.as_slice(), raw.as_slice(), b"filler"].concat()).unwrap();
        let t = SharedDict::train(&(0..200).map(|k| format!("sample {k} {}", k * 7)).collect::<Vec<_>>(), 1024).ok();
        for &(stage, bound) in &v4::STAGE_MIN_PAYLOAD {
            for level in [1, 3, 9, 19] {
                let mut payloads = vec![];
                if stage == 4 { payloads.push(v4::stage_payload(raw, stage, level, Some(&d))); if let Some(t) = &t { payloads.push(v4::stage_payload(raw, stage, level, Some(t))); } }
                else { payloads.push(v4::stage_payload(raw, stage, level, None)); }
                for p in payloads {
                    assert!(p.len() >= bound, "stage {stage} level {level}: {} bytes for {} input bytes, bound {bound}", p.len(), raw.len());
                    let m = min.entry(stage).or_insert(usize::MAX);
                    *m = (*m).min(p.len());
                }
            }
        }
    }
    // the proven minimums (v4.rs) are 11, 15 and 24; the constants keep a margin
    assert!(min[&3] >= 11 && min[&4] >= 15 && min[&2] >= 24, "{min:?}");
}

#[test]
fn quick_preset_is_explicit_and_round_trips() {
    let q = CompactOptions::quick();
    assert_eq!((q.search, q.small_zstd_level, q.zstd_level), (SearchMode::Quick, 3, 3));
    assert_eq!(CompactOptions::new().search, SearchMode::Full);
    assert!(!CompactOptions::new().compact_directory);
    let mut differs = 0;
    for (name, b, o) in v4_cases::cases() {
        let mut oq = o.clone();
        oq.search = SearchMode::Quick;
        oq.small_zstd_level = 3;
        oq.zstd_level = 3;
        let f = v4::write(&b, &oq).unwrap();
        assert!(v4::is_v4(&f), "{name}: quick writes an ordinary version-4 file");
        assert_eq!(f, v4::write(&b, &oq).unwrap(), "{name}: deterministic");
        let back = KoreReader::from_bytes_with_dicts(&f, &dicts_of(&o)).unwrap();
        assert_rows(&b, &back, &(0..b.num_rows), &name);
        if f != v4::write(&b, &o).unwrap() { differs += 1; }
    }
    assert!(differs > 10, "quick changes the bytes of most cases ({differs})");
}

// ── container version 5 ──────────────────────────────────────────────────────

fn sha(b: &[u8]) -> String { use sha2::Digest; sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect() }

/// `tests/v5_writer_bytes.txt`: length and sha256 of every writer case as container version 5 (written
/// by the commit that introduced it). Regenerate only for a deliberate format change:
/// `KORE_REGEN_V5=1 cargo test --test v4_rows v5_`.
#[test]
fn v5_bytes_are_pinned_and_smaller_than_v4() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("v5_writer_bytes.txt");
    let mut lines = vec![];
    for (name, b, o) in v4_cases::cases() {
        let f4 = v4::write(&b, &o).unwrap();
        let f5 = v4::write(&b, &o.clone().with_compact_directory(true)).unwrap();
        assert!(v4::is_v5(&f5) && !v4::is_v4(&f5) && v4::is_compact(&f5));
        assert!(f5.len() < f4.len(), "{name}: {} vs {}", f5.len(), f4.len());
        // same payloads, same decoded values
        let (i4, i5) = (v4::info(&f4).unwrap(), v4::info(&f5).unwrap());
        assert_eq!(i4.num_rows, i5.num_rows);
        assert_eq!(i4.dict_ids, i5.dict_ids);
        assert_eq!(format!("{:?}", i4.columns), format!("{:?}", i5.columns));
        assert_eq!(f4[f4.len() - i4.columns.iter().map(|c| c.5).sum::<usize>()..], f5[f5.len() - i5.columns.iter().map(|c| c.5).sum::<usize>()..]);
        let back = KoreReader::from_bytes_with_dicts(&f5, &dicts_of(&o)).unwrap();
        assert_rows(&b, &back, &(0..b.num_rows), &name);
        lines.push(format!("{name} {} {}", f5.len(), sha(&f5)));
    }
    if std::env::var("KORE_REGEN_V5").is_ok() { std::fs::write(&p, lines.join("\n") + "\n").unwrap(); }
    let want: Vec<String> = std::fs::read_to_string(&p).unwrap().lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
    assert_eq!(lines, want);
}

#[test]
fn v5_is_refused_by_version_4_paths_and_strictly_parsed() {
    let b = v4_cases::events(5, 3, 3);
    let f5 = v4::write(&b, &CompactOptions::new().with_compact_directory(true)).unwrap();
    // what a version-4-only reader does: the version-4 header parser stops at the version field
    let mut as4 = f5.clone();
    assert!(!v4::is_v4(&as4));
    as4[4] = 4;
    assert!(v4::read(&as4, None, &DictSet::new()).is_err(), "a version-5 directory must not parse as version 4");
    // the version-2 reader (what b44d2a91's KoreReader falls back to for an unknown version)
    // (version 6 is now rule mode, docs/kore_rule_mode.md: the next unassigned version is used for the
    // unknown-version check, and a version-5 directory relabelled 6 is refused by the rule-mode reader)
    assert!(KoreReader::from_bytes(&{ let mut x = f5.clone(); x[4] = 7; x }).unwrap_err().to_string().contains("unsupported version 7"));
    assert!(KoreReader::from_bytes(&{ let mut x = f5.clone(); x[4] = 6; x }).is_err());
    // non-minimal varint (num_cols 0x05 -> 0x85 0x00) with a correct checksum: refused
    let info = v4::info(&f5).unwrap();
    let dir_end = f5.len() - info.columns.iter().map(|c| c.5).sum::<usize>();
    let mut g = f5[..6].to_vec();
    g.push(f5[6] | 0x80);
    g.push(0);
    g.extend_from_slice(&f5[7..dir_end - 4]);
    let crc = kore_store::compress::crc32(&g);
    g.extend_from_slice(&crc.to_le_bytes());
    g.extend_from_slice(&f5[dir_end..]);
    assert!(v4::read(&g, None, &DictSet::new()).unwrap_err().to_string().contains("non-minimal varint"));
    // reserved codec bit with a correct checksum: refused
    let mut h = f5.clone();
    let codec_at = 6 + 3 + 1 + "event_id".len() + 1;
    h[codec_at] |= 0x80;
    let crc = kore_store::compress::crc32(&h[..dir_end - 4]);
    h[dir_end - 4..dir_end].copy_from_slice(&crc.to_le_bytes());
    assert!(v4::read(&h, None, &DictSet::new()).unwrap_err().to_string().contains("reserved codec bit"));
    assert!(v4::read(&f5, None, &DictSet::new()).is_ok());
}

#[test]
fn dictset_iteration_and_hex_ids() {
    let a = SharedDict::from_content(b"first dictionary").unwrap();
    let b2 = SharedDict::from_content(b"second dictionary").unwrap();
    let s: DictSet = vec![a.clone(), b2.clone(), a.clone()].into_iter().collect();
    assert_eq!(s.len(), 2);
    assert_eq!(s.iter().map(|d| d.id()).collect::<Vec<_>>(), vec![a.id(), b2.id()]);
    assert_eq!(s.ids(), vec![a.id(), b2.id()]);
    assert_eq!(s.ids_hex(), vec![a.id_hex(), b2.id_hex()]);
    assert_eq!((&s).into_iter().count(), 2);
    assert!(s.contains(&a.id()));
    let mut t = DictSet::new();
    t.extend(s.iter().cloned());
    assert_eq!(t.ids(), s.ids());
    let blk = v4_cases::events(50, 5, 4);
    let mut content = vec![];
    if let ColumnData::Str(v) = &blk.columns[4].data { for s in v.iter().flatten() { content.extend_from_slice(s.as_bytes()); } }
    let a = SharedDict::from_content(&content).unwrap();
    let f = v4::write(&blk, &CompactOptions::new().with_dict("text", a.clone())).unwrap();
    let info = v4::info(&f).unwrap();
    assert_eq!(info.dict_ids_hex(), vec![a.id_hex()]);
    let per = info.column_dicts_hex();
    assert_eq!(per.iter().find(|(n, _)| n == "text").unwrap().1.as_deref(), Some(a.id_hex().as_str()));
    assert!(per.iter().filter(|(n, _)| n != "text").all(|(_, d)| d.is_none()));
    assert_eq!(a.file_name(), format!("{}.kdic", info.dict_ids_hex()[0]));
}

#[test]
fn dtype_of_rows_api_matches_types() {
    // StrDict columns (legacy layout) through the row reader
    let codes: Vec<u8> = (0..30).map(|i| if i % 7 == 0 { u8::MAX } else { (i % 3) as u8 }).collect();
    let b = DataBlock::new(vec![Column::str_dict("k", codes, vec!["x".into(), "y".into(), "z".into()])]).unwrap();
    for compact in [false, true] {
        let f = v4::write(&b, &CompactOptions::new().with_compact_directory(compact)).unwrap();
        let full = v4::read(&f, None, &DictSet::new()).unwrap();
        for i in 0..30 { assert_rows(&full, &v4::read_row(&f, None, i, &DictSet::new()).unwrap(), &(i..i + 1), "str_dict"); }
        assert_eq!(codec::dtype_of(&full.columns[0].data), DType::StrDict);
    }
}
