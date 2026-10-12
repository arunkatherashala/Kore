//! Transparent accelerator for ISO-date string columns.
//!
//! TPC-H style dates live in `ColumnData::Str(Vec<Option<String>>)`, so every filter chases a heap pointer
//! per row. For a registered base table, a string column whose values are *all* exactly `dddd-dd-dd`
//! (ASCII digits, NULLs allowed) is encoded once, lazily, as `Vec<i32>` holding `yyyy*10000 + mm*100 + dd`
//! (`NULL_DAY` for NULL). For fixed-width digit strings, byte-wise string order equals the order of that
//! integer, so comparisons can run on integers with results identical to the string path (calendar validity
//! is deliberately not required: `2020-02-31` orders exactly like the string does). A column with even one
//! non-conforming value is not accelerated and the string path is used.
//!
//! Where the cache lives: `ColumnData` cannot change, so a process-wide registry maps the address of a
//! registered `Arc<DataBlock>` to per-column lazy slots. The registry holds only a `Weak`, which keeps the
//! (small) allocation reserved, so the address cannot be reused by another block while the entry exists, and
//! blocks inside an `Arc` with outstanding `Weak`s cannot be mutated in place (`Arc::get_mut` refuses).
//! INSERT/UPDATE/DELETE and re-registration build a new `Arc<DataBlock>`, which has a fresh address and
//! therefore a fresh (empty) cache; dead entries are swept lazily.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use kore_core::{ColumnData, DataBlock};
use rayon::prelude::*;

/// Marker for a NULL cell; every real encoding is >= 0.
pub(crate) const NULL_DAY: i32 = i32::MIN;
/// Smaller columns are not worth an encoding pass.
const MIN_ROWS: usize = 1024;

type Slot = OnceLock<Option<Arc<Vec<i32>>>>;

struct Entry {
    weak: Weak<DataBlock>,
    cols: Arc<Vec<Slot>>,
}

#[derive(Default)]
struct Registry {
    map: HashMap<usize, Entry>,
    sweep_at: usize,
}

static REG: Mutex<Option<Registry>> = Mutex::new(None);
static HITS: AtomicUsize = AtomicUsize::new(0);

/// Number of times an accelerated column was handed out (testing aid).
pub fn hits() -> usize { HITS.load(Ordering::Relaxed) }

/// Wrap a block in an `Arc` and make its date-like string columns eligible for acceleration.
pub(crate) fn arc(block: DataBlock) -> Arc<DataBlock> {
    let a = Arc::new(block);
    let eligible = a.columns.iter().any(|c| matches!(&c.data, ColumnData::Str(v) if v.len() >= MIN_ROWS));
    if eligible {
        let mut g = REG.lock().unwrap();
        let reg = g.get_or_insert_with(|| Registry { map: HashMap::new(), sweep_at: 64 });
        if reg.map.len() >= reg.sweep_at {
            reg.map.retain(|_, e| e.weak.strong_count() > 0);
            reg.sweep_at = (reg.map.len() * 2).max(64);
        }
        let cols = Arc::new((0..a.columns.len()).map(|_| OnceLock::new()).collect::<Vec<Slot>>());
        reg.map.insert(Arc::as_ptr(&a) as usize, Entry { weak: Arc::downgrade(&a), cols });
    }
    a
}

/// `yyyy*10000 + mm*100 + dd` for exactly `dddd-dd-dd`.
#[inline]
fn parse_full(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' { return None; }
    let d = |i: usize| -> Option<i32> { let c = b[i]; if c.is_ascii_digit() { Some((c - b'0') as i32) } else { None } };
    let y = d(0)? * 1000 + d(1)? * 100 + d(2)? * 10 + d(3)?;
    let m = d(5)? * 10 + d(6)?;
    let dd = d(8)? * 10 + d(9)?;
    Some(y * 10000 + m * 100 + dd)
}

/// Encode a comparison literal for a column view of divisor `div`
/// (1 = whole date `dddd-dd-dd`, 100 = `dddd-dd` prefix, 10000 = `dddd` prefix).
pub(crate) fn literal(s: &str, div: i32) -> Option<i32> {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| -> Option<i32> {
        let mut v = 0i32;
        for &c in &b[r] { if !c.is_ascii_digit() { return None; } v = v * 10 + (c - b'0') as i32; }
        Some(v)
    };
    match div {
        1 => parse_full(s),
        100 => if b.len() == 7 && b[4] == b'-' { Some(digits(0..4)? * 100 + digits(5..7)?) } else { None },
        10000 => if b.len() == 4 { digits(0..4) } else { None },
        _ => None,
    }
}

fn build(v: &[Option<String>]) -> Option<Arc<Vec<i32>>> {
    const CH: usize = 32_768;
    let mut out = vec![0i32; v.len()];
    out.par_chunks_mut(CH).zip(v.par_chunks(CH)).try_for_each(|(o, i)| -> Option<()> {
        for (d, s) in o.iter_mut().zip(i) {
            *d = match s { None => NULL_DAY, Some(s) => parse_full(s)? };
        }
        Some(())
    })?;
    Some(Arc::new(out))
}

/// The encoded days of column `cd` of `block`, when `block` is a registered base table and the whole column is
/// well-formed `dddd-dd-dd` (or NULL). None = use the string path.
pub(crate) fn lookup(block: &DataBlock, cd: &ColumnData) -> Option<Arc<Vec<i32>>> {
    if crate::testing::no_date_accel() { return None; }
    let ColumnData::Str(v) = cd else { return None };
    if v.len() < MIN_ROWS || v.len() != block.num_rows { return None; }
    let cols = {
        let g = REG.lock().unwrap();
        let e = g.as_ref()?.map.get(&(block as *const DataBlock as usize))?;
        if e.weak.strong_count() == 0 { return None; }
        e.cols.clone()
    };
    let idx = block.columns.iter().position(|c| std::ptr::eq(&c.data, cd))?;
    let r = cols.get(idx)?.get_or_init(|| build(v)).clone();
    if r.is_some() { HITS.fetch_add(1, Ordering::Relaxed); }
    r
}
