//! Short read-path profile on a Digital Life `events`-shaped table (8 string columns + bool), written by
//! the default v2 writer. Prints per-stage times so the cost of each read step can be attributed.
//!
//! Usage: prof_read <rows> <segment_rows> <reps>
//!   rows          total rows
//!   segment_rows  rows per file (kore-self log format 1 writes one file per table per batch of 1000)

use kore_core::DataBlock;
use kore_store::{compress, KoreReader, KoreWriter};
use std::time::Instant;

#[path = "support/events_gen.rs"]
mod events_gen;
use events_gen::events;

fn med(mut v: Vec<f64>) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2] }

/// (codec, payload) per column of a v2 file
fn chunks(d: &[u8]) -> Vec<(u8, &[u8])> {
    let nc = u32::from_le_bytes(d[6..10].try_into().unwrap()) as usize;
    let mut p = 18;
    for _ in 0..nc { let l = u16::from_le_bytes(d[p..p + 2].try_into().unwrap()) as usize; p += 2 + l + 1; }
    let mut out = vec![];
    for _ in 0..nc {
        let c = d[p]; let l = u64::from_le_bytes(d[p + 1..p + 9].try_into().unwrap()) as usize; p += 9;
        out.push((c, &d[p..p + l])); p += l;
    }
    out
}

fn main() {
    let a: Vec<usize> = std::env::args().skip(1).map(|x| x.parse().unwrap()).collect();
    let (rows, seg, reps) = (a[0], a[1], a[2]);
    let full = events(rows, 7);
    let segs: Vec<DataBlock> = (0..rows).step_by(seg).map(|s| full.select_rows(&(s..(s + seg).min(rows)).collect::<Vec<_>>())).collect();
    let t = Instant::now();
    let files: Vec<Vec<u8>> = segs.iter().map(KoreWriter::to_bytes).collect();
    let wr = t.elapsed().as_secs_f64() * 1e3;
    let total: usize = files.iter().map(|f| f.len()).sum();
    let mut codecs = std::collections::BTreeMap::new();
    for (c, p) in chunks(&files[0]) { *codecs.entry(c).or_insert(0usize) += p.len(); }
    println!("rows={rows} seg={seg} files={} bytes={total} write_ms={wr:.1} codecs(first file, codec->bytes)={codecs:?}", files.len());

    // per-file micro-profile: the same first file decoded `reps` times, min over reps (least disturbed)
    let f0 = &files[0];
    let mn = |v: Vec<f64>| v.into_iter().fold(f64::INFINITY, f64::min);
    let tm = |f: &mut dyn FnMut()| { let t = Instant::now(); f(); t.elapsed().as_secs_f64() * 1e6 };
    let (mut tfull, mut tcrc, mut tz_all, mut tz_fast, mut tlz4, mut tstr) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    for _ in 0..reps {
        tfull.push(tm(&mut || { std::hint::black_box(KoreReader::from_bytes(f0).unwrap()); }));
        tcrc.push(tm(&mut || { for (_, p) in chunks(f0) { std::hint::black_box(compress::crc32(p)); } }));
        tz_all.push(tm(&mut || { for (c, p) in chunks(f0) { if c == 6 { std::hint::black_box(zstd::decode_all(&p[1..]).unwrap()); } } }));
        tz_fast.push(tm(&mut || { for (c, p) in chunks(f0) { if c == 6 { std::hint::black_box(compress::zstd_decode_all(&p[1..], 0).unwrap()); } } }));
        let raws: Vec<(u8, Vec<u8>)> = chunks(f0).into_iter().filter(|(c, _)| *c == 6).map(|(_, p)| (p[0], compress::zstd_decode_all(&p[1..], 0).unwrap())).collect();
        tstr.push(tm(&mut || { for (ic, r) in &raws { if *ic == 0 { std::hint::black_box(compress::decode_strs(r)); } } }));
        let l4: Vec<Vec<u8>> = raws.iter().map(|(_, r)| lz4_flex::compress_prepend_size(r)).collect();
        tlz4.push(tm(&mut || { for l in &l4 { std::hint::black_box(lz4_flex::decompress_size_prepended(l).unwrap()); } }));
    }
    println!("per-file us (min of {reps}): from_bytes={:.0} crc={:.0} zstd_decode_all={:.0} zstd_fast={:.0} lz4_same_bytes={:.0} decode_strs={:.0}",
        mn(tfull), mn(tcrc), mn(tz_all), mn(tz_fast), mn(tlz4), mn(tstr));
}

