//! World M3 source: the one real file (NYC TLC Yellow Taxi trip records, January 2024, Parquet), checked and
//! loaded into a compact read-only base table. Untrusted input: the SHA-256 of the bytes is compared with the
//! constant below before AND after the read (a mismatch refuses); the size, the footer's row count, the rows
//! decoded, the column names and types and every value are checked before the table is used (fail closed).
//! Nothing here writes anything. Values are kept as recorded (negative fares, 2002 dates, 312,722-mile
//! trips): only their representation changes (money and distance in hundredths, timestamps in seconds).

use sha2::{Digest, Sha256};
use std::path::Path;

/// SHA-256 of the file as supplied by the owner (not a verification of its origin).
pub const SOURCE_SHA256: &str = "c4d59da7bbc8abaeeeb1727947ee93d9891a71acb42854bd80db1571b2030510";
pub const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const SOURCE_ROWS: usize = 2_964_624;
pub const MAX_ROWS_READ: usize = 3_000_000;
/// 2024-01-01 00:00:00 in seconds (the file's timestamps are naive local times; read as if UTC, consistently).
pub const JAN1: i64 = 1_704_067_200;
pub const DAY: i64 = 86_400;
pub const NULLV: i64 = i64::MIN;

/// How a column is stored and rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CT {
    Int,
    Ts,
    /// Hundredths (money in cents; trip_distance in hundredths of a mile), rendered with two decimals.
    Cents,
    /// store_and_fwd_flag: Y = 1, N = 0.
    Flag,
}

/// The 19 columns of the file, in file order, with the Arrow type the file must have.
pub const COLUMNS: [(&str, &str, CT); 19] = [
    ("VendorID", "Int32", CT::Int),
    ("tpep_pickup_datetime", "Timestamp", CT::Ts),
    ("tpep_dropoff_datetime", "Timestamp", CT::Ts),
    ("passenger_count", "Int64", CT::Int),
    ("trip_distance", "Float64", CT::Cents),
    ("RatecodeID", "Int64", CT::Int),
    ("store_and_fwd_flag", "Utf8", CT::Flag),
    ("PULocationID", "Int32", CT::Int),
    ("DOLocationID", "Int32", CT::Int),
    ("payment_type", "Int64", CT::Int),
    ("fare_amount", "Float64", CT::Cents),
    ("extra", "Float64", CT::Cents),
    ("mta_tax", "Float64", CT::Cents),
    ("tip_amount", "Float64", CT::Cents),
    ("tolls_amount", "Float64", CT::Cents),
    ("improvement_surcharge", "Float64", CT::Cents),
    ("total_amount", "Float64", CT::Cents),
    ("congestion_surcharge", "Float64", CT::Cents),
    ("Airport_fee", "Float64", CT::Cents),
];
pub const C_VENDOR: usize = 0;
pub const C_PICKUP: usize = 1;
pub const C_DROPOFF: usize = 2;
pub const C_DIST: usize = 4;
pub const C_PU: usize = 7;
pub const C_FARE: usize = 10;
pub const C_TOTAL: usize = 16;
/// The eight components of total_amount (fare, extra, mta_tax, tip, tolls, improvement, congestion, airport fee).
pub const COMPONENTS: [usize; 8] = [10, 11, 12, 13, 14, 15, 17, 18];
/// The pseudo-column index of `trip_id` (the 0-based row index in the file).
pub const C_TRIP: usize = 19;
/// PULocationID of Newark, JFK and LaGuardia in the TLC zone list (from memory; consistent with the fare rules
/// visible in the file, docs/m3_design.md "Owner decisions").
pub const AIRPORT_ZONES: [i64; 3] = [1, 132, 138];

pub fn col_name(c: usize) -> &'static str {
    if c == C_TRIP { "trip_id" } else { COLUMNS[c].0 }
}
pub fn col_type(c: usize) -> CT {
    if c == C_TRIP { CT::Int } else { COLUMNS[c].2 }
}

enum Store {
    I32(Vec<i32>),
    I64(Vec<i64>),
}

/// The base table (read-only after the load) and its indexes.
pub struct Base {
    cols: Vec<Store>,
    pub rows: usize,
    /// Rows picked up on January day d (index d, 1..=31), in file order.
    pub by_day: Vec<Vec<u32>>,
    /// Rows picked up outside January 2024, and rows whose dropoff is before the pickup (file order).
    pub out_month: Vec<u32>,
    pub backwards: Vec<u32>,
    pub load_ms: u64,
    pub sha256: String,
}

impl Base {
    /// The value of column `c` on row `r` (None = NULL). `C_TRIP` is the row index.
    pub fn get(&self, c: usize, r: u32) -> Option<i64> {
        if c == C_TRIP {
            return Some(r as i64);
        }
        let v = match &self.cols[c] {
            Store::I32(v) => {
                let x = v[r as usize];
                if x == i32::MIN { NULLV } else { x as i64 }
            }
            Store::I64(v) => v[r as usize],
        };
        (v != NULLV).then_some(v)
    }
    /// Pickup day in January (1..=31), or None outside January.
    pub fn jan_day(&self, r: u32) -> Option<i64> {
        let t = self.get(C_PICKUP, r)?;
        let d = (t - JAN1).div_euclid(DAY) + 1;
        (1..=31).contains(&d).then_some(d)
    }
}

impl Base {
    /// A base table from rows given in code (tests only: an author-made table, no data from any file), with the
    /// same indexes as `load`. Values are in the stored units (cents, hundredths, seconds).
    pub fn from_rows(rows: &[[Option<i64>; 19]]) -> Base {
        let mut cols: Vec<Store> = COLUMNS.iter().map(|(_, _, ct)| if *ct == CT::Ts { Store::I64(vec![]) } else { Store::I32(vec![]) }).collect();
        for r in rows {
            for (c, v) in r.iter().enumerate() {
                match &mut cols[c] {
                    Store::I64(o) => o.push(v.unwrap_or(NULLV)),
                    Store::I32(o) => o.push(v.map_or(i32::MIN, |x| x as i32)),
                }
            }
        }
        let mut b = Base { cols, rows: rows.len(), by_day: vec![vec![]; 32], out_month: vec![], backwards: vec![], load_ms: 0, sha256: String::new() };
        b.index();
        b
    }

    fn index(&mut self) {
        for r in 0..self.rows as u32 {
            match self.jan_day(r) {
                Some(d) => self.by_day[d as usize].push(r),
                None => self.out_month.push(r),
            }
            if let (Some(p), Some(d)) = (self.get(C_PICKUP, r), self.get(C_DROPOFF, r)) {
                if d < p {
                    self.backwards.push(r);
                }
            }
        }
    }
}

pub fn sha256_file(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Read and hash the file under the size cap. Err on a missing file, a file above the cap, or a hash mismatch.
pub fn check_hash(path: &Path) -> Result<String, String> {
    let m = std::fs::metadata(path).map_err(|e| format!("the source file cannot be read: {e}"))?;
    if !m.is_file() || m.len() > MAX_SOURCE_BYTES {
        return Err(format!("the source is not a file of at most {MAX_SOURCE_BYTES} bytes"));
    }
    let b = std::fs::read(path).map_err(|e| format!("the source file cannot be read: {e}"))?;
    if b.len() as u64 > MAX_SOURCE_BYTES {
        return Err("the source grew above the size cap while it was read".into());
    }
    let h = sha256_file(&b);
    if h != SOURCE_SHA256 {
        return Err(format!("source SHA-256 mismatch: the file has {h}, the code pins {SOURCE_SHA256}; the world is refused"));
    }
    Ok(h)
}

fn type_ok(want: &str, got: &str) -> bool {
    match want {
        "Timestamp" => got.starts_with("Timestamp("),
        "Utf8" => got == "Utf8" || got == "LargeUtf8",
        w => got == w,
    }
}

/// Units per second of a timestamp type text (`Timestamp(Microsecond, None)`).
fn ts_div(t: &str) -> Option<i64> {
    if t.contains("Nanosecond") {
        Some(1_000_000_000)
    } else if t.contains("Microsecond") {
        Some(1_000_000)
    } else if t.contains("Millisecond") {
        Some(1_000)
    } else if t.contains("Second") {
        Some(1)
    } else {
        None
    }
}

/// Check and load the source file. Every check refuses (Err) rather than repairs.
pub fn load(path: &Path) -> Result<Base, String> {
    let t0 = std::time::Instant::now();
    let sha = check_hash(path)?;
    let (rows, fields) = kore_parquet::meta(path).map_err(|e| format!("the source footer cannot be read: {e}"))?;
    if rows != SOURCE_ROWS as i64 {
        return Err(format!("the source has {rows} rows; {SOURCE_ROWS} are expected"));
    }
    if fields.len() != COLUMNS.len() {
        return Err(format!("the source has {} columns; {} are expected", fields.len(), COLUMNS.len()));
    }
    let mut divs = [1i64; 19];
    for (i, ((n, t), (wn, wt, _))) in fields.iter().zip(COLUMNS.iter()).enumerate() {
        if n != wn || !type_ok(wt, t) {
            return Err(format!("column {i} is {n}: {t}; expected {wn}: {wt}"));
        }
        if *wt == "Timestamp" {
            divs[i] = ts_div(t).ok_or_else(|| format!("unknown timestamp unit {t}"))?;
        }
    }
    let mut cols: Vec<Store> = COLUMNS.iter().map(|(_, _, ct)| if *ct == CT::Ts { Store::I64(Vec::with_capacity(SOURCE_ROWS)) } else { Store::I32(Vec::with_capacity(SOURCE_ROWS)) }).collect();
    let n = kore_parquet::read_batches(path, MAX_ROWS_READ, 131_072, |b| {
        use kore_core::ColumnData;
        for (ci, col) in b.columns.iter().enumerate() {
            let ct = COLUMNS[ci].2;
            match (&mut cols[ci], &col.data, ct) {
                (Store::I64(out), ColumnData::Int64(v), CT::Ts) => {
                    for x in v {
                        out.push(match x {
                            None => NULLV,
                            Some(x) if x % divs[ci] == 0 => x / divs[ci],
                            Some(x) => return Err(format!("{}: a timestamp with a sub-second part ({x})", COLUMNS[ci].0)),
                        });
                    }
                }
                (Store::I32(out), ColumnData::Int64(v), CT::Int) => {
                    for x in v {
                        out.push(match x {
                            None => i32::MIN,
                            Some(x) if *x > i32::MIN as i64 && *x <= i32::MAX as i64 => *x as i32,
                            Some(x) => return Err(format!("{}: {x} is out of range", COLUMNS[ci].0)),
                        });
                    }
                }
                (Store::I32(out), ColumnData::Float64(v), CT::Cents) => {
                    for x in v {
                        out.push(match x {
                            None => i32::MIN,
                            Some(x) => {
                                let s = x * 100.0;
                                let r = s.round();
                                if !s.is_finite() || (s - r).abs() >= 1e-6 || r <= i32::MIN as f64 || r > i32::MAX as f64 {
                                    return Err(format!("{}: {x} is not a value with at most two decimals in range; the file is refused", COLUMNS[ci].0));
                                }
                                r as i32
                            }
                        });
                    }
                }
                (Store::I32(out), ColumnData::Str(v), CT::Flag) => {
                    for x in v {
                        out.push(match x.as_deref() {
                            None => i32::MIN,
                            Some("Y") => 1,
                            Some("N") => 0,
                            Some(s) => return Err(format!("{}: unexpected value {s:?}", COLUMNS[ci].0)),
                        });
                    }
                }
                _ => return Err(format!("{}: unexpected decoded type", COLUMNS[ci].0)),
            }
        }
        Ok(())
    })?;
    if n != SOURCE_ROWS {
        return Err(format!("{n} rows were decoded; {SOURCE_ROWS} are expected"));
    }
    // the bytes read must still be the pinned ones
    check_hash(path)?;
    let mut b = Base { cols, rows: n, by_day: vec![vec![]; 32], out_month: vec![], backwards: vec![], load_ms: 0, sha256: sha };
    b.index();
    b.load_ms = t0.elapsed().as_millis() as u64;
    Ok(b)
}

// ---- calendar ----

/// (year, month, day) of a day count since 1970-01-01 (proleptic Gregorian).
pub fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `YYYY-MM-DD hh:mm:ss`.
pub fn iso_ts(t: i64) -> String {
    let (y, m, d) = civil(t.div_euclid(DAY));
    let s = t.rem_euclid(DAY);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// `MM/DD/YYYY hh:mm:ss AM` (12-hour clock).
pub fn us_ts(t: i64) -> String {
    let (y, m, d) = civil(t.div_euclid(DAY));
    let s = t.rem_euclid(DAY);
    let h = s / 3600;
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{m:02}/{d:02}/{y:04} {h12:02}:{:02}:{:02} {}", s / 60 % 60, s % 60, if h < 12 { "AM" } else { "PM" })
}

pub fn jan_date(d: i64) -> String {
    format!("2024-01-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_and_timestamp_texts() {
        assert_eq!(iso_ts(JAN1), "2024-01-01 00:00:00");
        assert_eq!(us_ts(JAN1), "01/01/2024 12:00:00 AM");
        assert_eq!(us_ts(JAN1 + 13 * 3600 + 61), "01/01/2024 01:01:01 PM");
        assert_eq!(us_ts(JAN1 + 12 * 3600), "01/01/2024 12:00:00 PM");
        assert_eq!(iso_ts(JAN1 - DAY), "2023-12-31 00:00:00");
        assert_eq!(civil(0), (1970, 1, 1));
    }

    #[test]
    fn a_wrong_or_missing_source_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.parquet");
        std::fs::write(&p, b"not the taxi file").unwrap();
        let e = load(&p).err().unwrap();
        assert!(e.contains("SHA-256 mismatch"), "{e}");
        assert!(load(&d.path().join("missing.parquet")).err().unwrap().contains("cannot be read"));
    }
}
