//! Compatibility guard for the read/write speed work (docs/fileformat_speed.md). The `events_*` fixtures
//! were written by the unmodified writer of commit a980727d from `examples/support/events_gen.rs`; they
//! are big enough that every column goes through the ZSTD stage and the text column spans several
//! ZSTD blocks, which the small fixtures in `codecs.rs` do not reach.

use kore_core::DataBlock;
use kore_store::{rowgroups, versioned, KoreReader, KoreWriter};
use std::path::PathBuf;

#[path = "../examples/support/events_gen.rs"]
mod events_gen;

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn assert_same(got: &DataBlock, want: &DataBlock) {
    assert_eq!(got.num_rows, want.num_rows);
    assert_eq!(got.columns.len(), want.columns.len());
    for (g, w) in got.columns.iter().zip(&want.columns) {
        assert_eq!(g.name, w.name);
        assert_eq!(g.data, w.data, "column {}", g.name);
    }
}

/// (start, len) of every column payload in a v2 file
fn payloads(d: &[u8]) -> Vec<(usize, usize)> {
    let nc = u32::from_le_bytes(d[6..10].try_into().unwrap()) as usize;
    let mut p = 18;
    for _ in 0..nc { p += 2 + u16::from_le_bytes(d[p..p + 2].try_into().unwrap()) as usize + 1; }
    (0..nc).map(|_| {
        let l = u64::from_le_bytes(d[p + 1..p + 9].try_into().unwrap()) as usize;
        p += 9 + l;
        (p - l, l)
    }).collect()
}

#[test]
fn events_fixture_reads_back_and_the_writer_still_produces_it() {
    let want = events_gen::events(1000, 7);
    let fx = fixture("events_1000.kore");
    assert_same(&KoreReader::from_bytes(&fx).unwrap(), &want);
    let b = KoreWriter::to_bytes(&want);
    if std::env::var_os("KORE_READABLE_MODE").is_none() && std::env::var_os("KORE_READABLE_ROWS").is_none() {
        assert!(b == fx, "default writer output differs from the a980727d bytes");
    } else {
        assert!(b.starts_with(&fx[..payloads(&fx).last().map(|(s, l)| s + l).unwrap()]));
    }
    // every column of this file is ZSTD (codec 6)
    let nc = payloads(&fx).len();
    assert_eq!(nc, 8);
    let proj = KoreReader::from_bytes_columns(&fx, &["text", "event_id"]).unwrap();
    assert_eq!(proj.columns[0].data, want.columns[5].data);
    assert_eq!(proj.columns[1].data, want.columns[0].data);
}

#[test]
fn events_log_and_row_groups_fixtures() {
    let log = fixture("events_log.kore");
    for (i, (n, ts)) in [(100usize, 100u64), (200, 200), (300, 300)].iter().enumerate() {
        let want = events_gen::events(*n, 11 + i as u64);
        assert_same(&versioned::read_at(&log, *ts).unwrap(), &want);
        assert_same(&KoreReader::read_at_version(&log, *ts + 50).unwrap(), &want);
    }
    assert_same(&KoreReader::from_bytes(&log).unwrap(), &events_gen::events(300, 13));

    let rg = fixture("events_rowgroups.kore");
    let want = events_gen::events(600, 5);
    assert_eq!(rowgroups::write(&want, 250).unwrap(), rg, "row-group writer output differs from a980727d");
    assert_same(&KoreReader::from_bytes(&rg).unwrap(), &want);
    let part = rowgroups::read(&rg, Some(&["kind", "redacted"]), None).unwrap();
    assert_eq!(part.columns[0].data, want.columns[4].data);
    assert_eq!(part.columns[1].data, want.columns[7].data);
}

#[test]
fn damaged_column_payloads_are_reported_as_checksum_mismatches() {
    let fx = fixture("events_1000.kore");
    let cols = payloads(&fx);
    for (ci, &(start, len)) in cols.iter().enumerate() {
        for off in (0..len).step_by(997).chain([len - 1]) {
            let mut m = fx.clone();
            m[start + off] ^= 0x10;
            let e = KoreReader::from_bytes(&m).expect_err("damaged payload accepted");
            assert!(e.to_string().contains("checksum mismatch"), "col {ci} off {off}: {e}");
        }
    }
    // projection checks only the columns it reads; a damaged unread column is not decoded
    let (s, _) = cols[5];
    let mut m = fx.clone();
    m[s + 3] ^= 0x01;
    assert!(KoreReader::from_bytes_columns(&m, &["kind"]).is_ok());
    assert!(KoreReader::from_bytes_columns(&m, &["kind", "text"]).unwrap_err().to_string().contains("'text'"));
}
