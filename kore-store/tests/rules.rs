//! Rule mode (version 5, `CompactOptions::with_rules()`): exact round trips for every rule family,
//! random property cases, adversarial values, corruption/truncation, old-reader refusal, and the
//! guarantee that a file without a winning rule is the version-4 file byte for byte.

#[path = "../examples/support/rules_gen.rs"]
mod gen;

use gen::Rng;
use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{rules, v4, CompactOptions, DictSet, KoreReader, KoreWriter, RuleOptions};

fn same(a: &DataBlock, b: &DataBlock) {
    assert_eq!(a.num_rows, b.num_rows);
    assert_eq!(a.columns.len(), b.columns.len());
    for (x, y) in a.columns.iter().zip(&b.columns) {
        assert_eq!(x.name, y.name);
        assert_eq!(rules::digest(&x.data), rules::digest(&y.data), "column {}", x.name);
        match (&x.data, &y.data) {
            (ColumnData::Float64(p), ColumnData::Float64(q)) => assert!(p.iter().zip(q).all(|(u, v)| u.map(f64::to_bits) == v.map(f64::to_bits)), "column {}", x.name),
            (ColumnData::Int64(p), ColumnData::Int64(q)) => assert_eq!(p, q, "column {}", x.name),
            (ColumnData::Str(p), ColumnData::Str(q)) => assert_eq!(p, q, "column {}", x.name),
            (ColumnData::Bool(p), ColumnData::Bool(q)) => assert_eq!(p, q, "column {}", x.name),
            _ => panic!("type changed in column {}", x.name),
        }
    }
}

/// Write with rules, check: never larger than version 4, reads back exactly (both reader entry points),
/// and returns the per-column choices.
fn roundtrip(b: &DataBlock) -> (Vec<u8>, Vec<rules::ColumnChoice>) {
    let v4b = KoreWriter::to_bytes_compact(b, &CompactOptions::default()).unwrap();
    let (bytes, report) = rules::write_explained(b, &RuleOptions::default()).unwrap();
    assert!(bytes.len() <= v4b.len(), "rule mode larger than v4: {} > {}", bytes.len(), v4b.len());
    if report.iter().all(|c| c.rule.is_none()) { assert_eq!(bytes, v4b, "no rule won: must be the v4 file"); }
    same(b, &KoreReader::from_bytes(&bytes).unwrap());
    same(b, &KoreReader::from_bytes_with_dicts(&bytes, &DictSet::new()).unwrap());
    assert_eq!(KoreWriter::to_bytes_rules(b, &RuleOptions::default()).unwrap(), bytes, "deterministic");
    (bytes, report)
}

fn rule_of<'a>(r: &'a [rules::ColumnChoice], name: &str) -> Option<&'a str> {
    r.iter().find(|c| c.name == name).unwrap().rule.as_ref().map(|x| x.0.as_str())
}

fn block(cols: Vec<Column>) -> DataBlock { DataBlock::new(cols).unwrap() }

#[test]
fn every_family_round_trips_and_wins_where_built_for() {
    let n = 20_000;
    let s = gen::sensors(n, 1);
    let (_, r) = roundtrip(&s);
    assert!(rule_of(&r, "ts_ms").unwrap().contains("affine"), "{:?}", rule_of(&r, "ts_ms"));
    assert!(rule_of(&r, "ts_text").unwrap().starts_with("timestamp text"));
    assert!(rule_of(&r, "temp_c").unwrap().contains("k/10^2"));
    { let c = rule_of(&r, "cycle").unwrap(); assert!(c.starts_with("int: lag ") && c.split_whitespace().nth(2).unwrap().parse::<usize>().unwrap() % 96 == 0, "{c}"); }

    let o = gen::orders(n, 2);
    let (_, r) = roundtrip(&o);
    assert!(rule_of(&r, "country").unwrap().contains("lookup(city)"), "{:?}", rule_of(&r, "country"));
    assert!(rule_of(&r, "prev_city").unwrap().contains("city shifted by 1"), "{:?}", rule_of(&r, "prev_city"));
    assert!(rule_of(&r, "line_total").unwrap().contains("qty * unit_price"), "{:?}", rule_of(&r, "line_total"));
    let sub = rule_of(&r, "subtotal_cents").unwrap();
    assert!(sub.contains("qty * unit_cents") || sub.contains("unit_cents * qty"), "{sub}");
    let tot = rule_of(&r, "total_cents").unwrap();
    assert!(tot.contains('+') && tot.contains("shipping_cents"), "{tot}");

    let l = gen::logs(n, 3);
    let (_, r) = roundtrip(&l);
    assert!(rule_of(&r, "message").unwrap().contains("template"), "{:?}", rule_of(&r, "message"));
    assert!(rule_of(&r, "host").map_or(false, |x| x.contains("lookup(service)")) || rule_of(&r, "service").map_or(false, |x| x.contains("lookup(host)")), "{:?} {:?}", rule_of(&r, "host"), rule_of(&r, "service"));

    let q = gen::sequences(n, 4);
    let (_, r) = roundtrip(&q);
    // "arithmetic" is not listed: the version-4 delta payload (24 bytes) is already as small as any rule
    for c in ["fibonacci", "affine_geometric", "quadratic", "arithmetic_1pct_exceptions"] {
        assert!(rule_of(&r, c).is_some(), "{c}: no rule");
    }
    assert!(rule_of(&r, "fibonacci").unwrap().contains("recurrence"), "{:?}", rule_of(&r, "fibonacci"));
    assert!(rule_of(&r, "affine_geometric").unwrap().contains("+3*v[-1]"), "{:?}", rule_of(&r, "affine_geometric"));
}

#[test]
fn markov_family_round_trips() {
    // a strict cycle of 5 symbols with rare jumps: the next symbol is predictable from the previous one
    let mut r = Rng::new(9);
    let syms = ["idle", "connect", "auth", "query", "close"];
    let mut k = 0;
    let v: Vec<Option<String>> = (0..30_000).map(|_| { k = if r.below(50) == 0 { r.below(5) as usize } else { (k + 1) % 5 }; Some(syms[k].to_string()) }).collect();
    let b = block(vec![Column::str_col("state", v)]);
    roundtrip(&b);
    // forced through the family decoder too: shuffled symbols with nulls
    let v: Vec<Option<String>> = (0..5000).map(|i| if i % 7 == 0 { None } else { Some(syms[r.below(5) as usize].to_string()) }).collect();
    roundtrip(&block(vec![Column::str_col("s", v)]));
}

#[test]
fn random_property_cases() {
    // structured columns with random parameters, noise rates and null rates
    for seed in 0..40u64 {
        let mut r = Rng::new(1000 + seed);
        let n = [0usize, 1, 2, 3, 17, 100, 1000, 5000][r.below(8) as usize];
        let null_rate = [0u64, 0, 5, 50][r.below(4) as usize];
        let noise = [0u64, 1, 10, 100][r.below(4) as usize];
        let nul = |r: &mut Rng| null_rate > 0 && r.below(100) < null_rate;
        let a0 = r.next() as i64 >> r.below(64);
        let st = (r.next() as i64) >> (r.below(64) as u32).max(1);
        let p = 2 + r.below(50) as usize;
        let mut ints = vec![];
        let mut lagged = vec![];
        let mut dec = vec![];
        let mut fl = vec![];
        let mut strs = vec![];
        let mut ts = vec![];
        for i in 0..n {
            let e = noise > 0 && r.below(100) < noise;
            ints.push(if nul(&mut r) { None } else { Some(a0.wrapping_add(st.wrapping_mul(i as i64)).wrapping_add(if e { r.next() as i64 } else { 0 })) });
            lagged.push(if nul(&mut r) { None } else { Some(((i % p) as i64) * 31 + if e { r.below(5) as i64 } else { 0 }) });
            dec.push(if nul(&mut r) { None } else { Some(((r.below(200_000) as i64 - 100_000) as f64) / 100.0) });
            fl.push(if nul(&mut r) { None } else { Some(f64::from_bits(r.next())) });
            strs.push(if nul(&mut r) { None } else { Some(format!("id {} at {} x{}", i, r.below(10), if e { "\u{e9}" } else { "" })) });
            ts.push(if nul(&mut r) { None } else { Some(gen::iso(1_700_000_000 + 60 * i as i64 + r.below(3) as i64)) });
        }
        let prod: Vec<Option<i64>> = ints.iter().zip(&lagged).map(|(a, b)| match (a, b) { (Some(a), Some(b)) => Some(a.wrapping_mul(*b)), _ => None }).collect();
        let b = block(vec![
            Column::int64("ints", ints), Column::int64("lagged", lagged), Column::float64("dec", dec), Column::float64("bits", fl),
            Column::str_col("text", strs), Column::str_col("ts", ts), Column::int64("prod", prod),
        ]);
        roundtrip(&b);
    }
}

#[test]
fn random_streams_through_every_int_model() {
    // the integer DSL decodes any residual stream (not only those the search picks)
    use rules::intseq::{decode, encode_model, IntModel};
    let lv_bytes = |m: &IntModel, v: &[i64]| encode_model(m, v, rules_levels());
    let models = vec![IntModel::delta(0), IntModel::delta(1), IntModel::delta(2), IntModel::delta(3), IntModel::Runs,
        IntModel::Affine { a: i64::MIN, b: i64::MAX }, IntModel::Lag { p: 1 }, IntModel::Lag { p: 7 }, IntModel::LagTrend { p: 3 },
        IntModel::Rec { c: vec![1, 1], c0: 0 }, IntModel::Rec { c: vec![0, 0, -5], c0: i64::MIN }];
    for seed in 0..20 {
        let mut r = Rng::new(seed);
        let n = r.below(3000) as usize;
        let v: Vec<i64> = (0..n).map(|_| match r.below(4) { 0 => r.next() as i64, 1 => i64::MIN, 2 => i64::MAX, _ => r.below(10) as i64 }).collect();
        for m in &models {
            let b = lv_bytes(m, &v);
            let mut rd = rules::stream::Rd::new(&b);
            assert_eq!(decode(&mut rd, v.len()).unwrap(), v, "model {m:?}");
            rd.done().unwrap();
        }
    }
}

fn rules_levels() -> rules::stream::Levels { rules::stream::Levels { large: 9, small: 19 } }

#[test]
fn adversarial_values() {
    let n = 4000;
    // a rule that almost fits: one value off in the middle and at the ends
    let mut v: Vec<Option<i64>> = (0..n as i64).map(|i| Some(5 + 3 * i)).collect();
    v[0] = Some(i64::MIN); v[n / 2] = Some(i64::MAX); v[n - 1] = Some(-1);
    // floats: NaN (two payloads), -0.0, infinities, subnormal, among decimals
    let mut f: Vec<Option<f64>> = (0..n).map(|i| Some(i as f64 / 100.0)).collect();
    f[1] = Some(f64::NAN); f[2] = Some(f64::from_bits(0x7ff8_0000_0000_beef)); f[3] = Some(-0.0); f[4] = Some(f64::INFINITY);
    f[5] = Some(f64::NEG_INFINITY); f[6] = Some(f64::from_bits(1)); f[7] = None; f[8] = Some(1e300);
    // derived float where the source has NaN / -0.0
    let g: Vec<Option<f64>> = f.iter().map(|x| x.map(|x| x * 2.0)).collect();
    let k: Vec<Option<f64>> = (0..n).map(|_| Some(2.0)).collect();
    // i64 extremes in relations
    let a: Vec<Option<i64>> = (0..n as i64).map(|i| Some(if i % 3 == 0 { i64::MAX } else { i64::MIN + i })).collect();
    let b2: Vec<Option<i64>> = (0..n as i64).map(|i| if i % 11 == 0 { None } else { Some(i64::MAX - i) }).collect();
    let s: Vec<Option<i64>> = a.iter().zip(&b2).map(|(x, y)| Some(x.unwrap().wrapping_add(y.unwrap_or(0)))).collect();
    // template edge cases: leading zeros, 19-digit runs, empty strings, unicode, 0x00 in text
    let t: Vec<Option<String>> = (0..n).map(|i| Some(match i % 6 {
        0 => format!("v{i} 007 x"), 1 => "9999999999999999999 9".to_string(), 2 => String::new(), 3 => format!("\u{1F600}{i}\u{e9}"),
        4 => format!("{i}"), _ => format!("n {i} {}", i * 2) })).collect();
    let mut t0 = t.clone();
    t0[17] = Some("a\0b1".to_string());
    let b = block(vec![
        Column::int64("almost", v), Column::float64("f", f), Column::float64("k", k), Column::float64("g", g),
        Column::int64("a", a), Column::int64("b", b2), Column::int64("s", s), Column::str_col("t", t), Column::str_col("t0", t0),
    ]);
    roundtrip(&b);
}

#[test]
fn small_and_empty_columns() {
    for n in [0usize, 1, 2] {
        let b = block(vec![
            Column::int64("i", (0..n).map(|x| Some(x as i64 * 7)).collect()),
            Column::float64("f", (0..n).map(|x| Some(x as f64 * 0.5)).collect()),
            Column::str_col("s", (0..n).map(|x| Some(format!("r{x}"))).collect()),
            Column::bool_col("b", (0..n).map(|x| Some(x % 2 == 0)).collect()),
            Column::int64("nulls", vec![None; n]),
        ]);
        roundtrip(&b);
    }
    roundtrip(&DataBlock::empty());
    // all-null columns of every type, long
    let n = 3000;
    roundtrip(&block(vec![Column::int64("i", vec![None; n]), Column::float64("f", vec![None; n]), Column::str_col("s", vec![None; n])]));
}

#[test]
fn no_rule_means_the_version4_bytes() {
    // the random control without the uniform-[0,1) doubles (those have a skewed exponent byte that the xor
    // residual layout compresses a little; see the docs)
    let mut b = gen::random(5000, 7);
    b.columns.retain(|c| c.name != "f64_unit");
    let (bytes, report) = roundtrip(&b);
    assert!(report.iter().all(|c| c.rule.is_none()), "{report:?}");
    assert!(v4::is_v4(&bytes));
    // a dictionary-using v4 options set passes through unchanged too
    let d = kore_store::SharedDict::from_content(&b"0123456789abcdef".repeat(64)).unwrap();
    let o = CompactOptions::default().with_dict("hex", d.clone());
    let mut ds = DictSet::new();
    ds.add(d);
    let want = KoreWriter::to_bytes_compact(&b, &o).unwrap();
    let got = KoreWriter::to_bytes_rules(&b, &o.with_rules()).unwrap();
    assert_eq!(got, want);
}

#[test]
fn rules_with_shared_dictionary_and_projection() {
    let mut b = gen::orders(3000, 5);
    let text: Vec<Option<String>> = (0..3000).map(|i| Some(format!("free text note number {} about the order", i * 7919 % 1000))).collect();
    b.columns.push(Column::str_col("note", text));
    let d = kore_store::SharedDict::from_content(&b"free text note number about the order ".repeat(40)).unwrap();
    let o = CompactOptions::default().with_dict("note", d.clone()).with_rules();
    let bytes = KoreWriter::to_bytes_rules(&b, &o).unwrap();
    assert!(rules::is_v5(&bytes));
    let mut ds = DictSet::new();
    ds.add(d);
    same(&b, &KoreReader::from_bytes_with_dicts(&bytes, &ds).unwrap());
    // a derived column alone (its sources are decoded internally, not returned)
    let p = KoreReader::from_bytes_columns(&bytes, &["country", "line_total"]).unwrap();
    assert_eq!(p.columns.len(), 2);
    assert_eq!(p.columns[0].name, "country");
    assert_eq!(rules::digest(&p.columns[0].data), rules::digest(&b.column("country").unwrap().data));
    assert_eq!(rules::digest(&p.columns[1].data), rules::digest(&b.column("line_total").unwrap().data));
    assert!(KoreReader::from_bytes_columns(&bytes, &["nope"]).is_err());
}

#[test]
fn old_readers_refuse_rule_mode_files() {
    let b = gen::orders(2000, 11);
    let bytes = KoreWriter::to_bytes_rules(&b, &RuleOptions::default()).unwrap();
    assert!(rules::is_v5(&bytes));
    let e = v4::read(&bytes, None, &DictSet::new()).unwrap_err().to_string();
    assert!(e.contains("unsupported version 5"), "{e}");
    let e = kore_store::v3::read(&bytes, None).unwrap_err().to_string();
    assert!(!e.is_empty());
    assert!(v4::info(&bytes).is_err());
    // the committed fixture (written by this code) still reads, and old readers refuse it too
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rules_v5.kore");
    let fx = std::fs::read(p).unwrap();
    assert!(rules::is_v5(&fx));
    same(&rules_fixture_block(), &KoreReader::from_bytes(&fx).unwrap());
    assert!(v4::read(&fx, None, &DictSet::new()).unwrap_err().to_string().contains("unsupported version 5"));
}

/// The block of tests/fixtures/rules_v5.kore.
fn rules_fixture_block() -> DataBlock {
    let n = 300;
    block(vec![
        Column::int64("id", (0..n).map(|i| Some(1000 + 3 * i as i64)).collect()),
        Column::str_col("city", (0..n).map(|i| Some(["Paris", "Lyon", "Rome"][i % 3].to_string())).collect()),
        Column::str_col("country", (0..n).map(|i| Some(["FR", "FR", "IT"][i % 3].to_string())).collect()),
        Column::float64("price", (0..n).map(|i| Some((i % 50) as f64 / 4.0)).collect()),
    ])
}

#[test]
fn fixture_bytes_are_stable() {
    let bytes = KoreWriter::to_bytes_rules(&rules_fixture_block(), &RuleOptions::default()).unwrap();
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rules_v5.kore");
    if std::env::var("KORE_WRITE_RULES_FIXTURE").is_ok() { std::fs::write(&p, &bytes).unwrap(); }
    assert_eq!(std::fs::read(p).unwrap(), bytes, "rule-mode writer output changed");
}

fn small_rule_file() -> (DataBlock, Vec<u8>) {
    let n = 400;
    let mut r = Rng::new(3);
    let city: Vec<Option<String>> = (0..n).map(|_| Some(["Paris", "Lyon", "Rome", "Oslo"][r.below(4) as usize].to_string())).collect();
    let country: Vec<Option<String>> = city.iter().map(|c| Some(match c.as_deref().unwrap() { "Rome" => "IT", "Oslo" => "NO", _ => "FR" }.to_string())).collect();
    let b = block(vec![
        Column::int64("t", (0..n).map(|i| Some(1_000_000 + 60 * i as i64 + r.below(3) as i64)).collect()),
        Column::str_col("city", city), Column::str_col("country", country),
        Column::float64("x", (0..n).map(|i| Some((i % 7) as f64 / 10.0)).collect()),
        Column::str_col("msg", (0..n).map(|i| Some(format!("job {} took {}s", 500 + i, r.below(9)))).collect()),
    ]);
    let bytes = KoreWriter::to_bytes_rules(&b, &RuleOptions::default()).unwrap();
    assert!(rules::is_v5(&bytes));
    (b, bytes)
}

#[test]
fn corruption_and_truncation_never_panic_or_return_wrong_data() {
    let (b, bytes) = small_rule_file();
    let check = |d: &[u8]| {
        if let Ok(got) = KoreReader::from_bytes(d) {
            // only a byte change that keeps every checksum valid could get here; it must then be exact
            same(&b, &got);
        }
    };
    for i in 0..bytes.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut d = bytes.clone();
            d[i] ^= mask;
            assert!(KoreReader::from_bytes(&d).is_err(), "flip at {i} mask {mask:#x} accepted");
        }
    }
    for len in 0..bytes.len() { assert!(KoreReader::from_bytes(&bytes[..len]).is_err(), "truncated to {len} accepted"); }
    let mut long = bytes.clone();
    long.push(0);
    assert!(KoreReader::from_bytes(&long).is_err());
    check(&bytes);
}

/// Rewrite the payload CRCs and the directory CRC so that a payload change reaches the rule decoder.
fn recrc(d: &mut Vec<u8>) {
    let crc32 = kore_store::compress::crc32;
    let ncols = u32::from_le_bytes(d[6..10].try_into().unwrap()) as usize;
    let nd = u16::from_le_bytes(d[18..20].try_into().unwrap()) as usize;
    let mut pos = 20 + 36 * nd;
    let mut ents = vec![];
    for _ in 0..ncols {
        let nl = u16::from_le_bytes(d[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2 + nl + 4;
        let len = u64::from_le_bytes(d[pos..pos + 8].try_into().unwrap()) as usize;
        ents.push((pos + 8, len));
        pos += 12;
    }
    let dir_end = pos;
    let mut p = dir_end + 4;
    for (crc_at, len) in ents {
        let c = crc32(&d[p..p + len]);
        d[crc_at..crc_at + 4].copy_from_slice(&c.to_le_bytes());
        p += len;
    }
    let c = crc32(&d[..dir_end]);
    d[dir_end..dir_end + 4].copy_from_slice(&c.to_le_bytes());
}

#[test]
fn payload_garbage_with_valid_crcs_is_refused_or_exact() {
    let (b, bytes) = small_rule_file();
    let ncols = b.columns.len();
    let nd = u16::from_le_bytes(bytes[18..20].try_into().unwrap()) as usize;
    let mut pos = 20 + 36 * nd;
    for _ in 0..ncols { let nl = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap()) as usize; pos += 2 + nl + 16; }
    let payload_start = pos + 4;
    let (mut ok_exact, mut refused) = (0, 0);
    for i in payload_start..bytes.len() {
        for mask in [0x01u8, 0x10, 0xff] {
            let mut d = bytes.clone();
            d[i] ^= mask;
            recrc(&mut d);
            match KoreReader::from_bytes(&d) {
                Ok(got) => { same(&b, &got); ok_exact += 1; }
                Err(_) => refused += 1,
            }
        }
    }
    assert!(refused > 0);
    let _ = ok_exact;
    // the decoded-column digest itself: change one digest byte of each rule column
    let desc = rules::describe(&bytes).unwrap();
    let mut p = payload_start;
    for (name, _, len, rule) in desc {
        if rule.is_some() {
            let mut d = bytes.clone();
            d[p + 1] ^= 1;
            recrc(&mut d);
            let e = KoreReader::from_bytes(&d).unwrap_err().to_string();
            assert!(e.contains("checksum mismatch"), "{name}: {e}");
        }
        p += len;
    }
}

#[test]
fn explicit_codecs_are_respected_and_options_checked() {
    let b = gen::sequences(2000, 1);
    let o = CompactOptions::default().with_column("arithmetic", kore_store::ColumnCodec::new(kore_store::Transform::Plain, kore_store::Stage::None)).with_rules();
    let (bytes, rep) = rules::write_explained(&b, &o).unwrap();
    assert!(rule_of(&rep, "arithmetic").is_none());
    same(&b, &KoreReader::from_bytes(&bytes).unwrap());
    let bad = CompactOptions::default().with_column("missing", kore_store::ColumnCodec::AUTO).with_rules();
    assert!(KoreWriter::to_bytes_rules(&b, &bad).is_err());
    // within only / cross only
    let o2 = gen::orders(2000, 1);
    let (_, r1) = rules::write_explained(&o2, &RuleOptions::default().within_only()).unwrap();
    assert!(rule_of(&r1, "country").is_none());
    let (x, r2) = rules::write_explained(&o2, &RuleOptions::default().cross_only()).unwrap();
    assert!(rule_of(&r2, "ordered_at").is_none());
    same(&o2, &KoreReader::from_bytes(&x).unwrap());
}
