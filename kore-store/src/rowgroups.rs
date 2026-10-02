//! Row-group container (opt-in): a table split into independent groups with per-column min/max, so a
//! reader can skip groups that cannot match a range filter and never needs the whole table in memory.
//!
//! Layout (all integers little-endian):
//! ```text
//! "KRGP" | version:u8 = 1 | group 0 | group 1 | ... | index | index_len:u32 | "KRGE"
//! group  = a complete .kore file (no text trailer)
//! index  = ncols:u32 | (name_len:u16 name)* | ngroups:u32 |
//!          per group: offset:u64 len:u64 rows:u64 | per column: kind:u8 min:8 max:8 nulls:u64
//! kind   = 0 no stats | 1 i64 | 2 f64   (f64 stats ignore NaN and null)
//! ```
//! `offset` is counted from the start of the file.

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use crate::{KoreReader, KoreWriter};

pub const START: &[u8; 4] = b"KRGP";
pub const END: &[u8; 4] = b"KRGE";
const VERSION: u8 = 1;
const MAX_GROUPS: usize = 1 << 24;

fn bad(m: &str) -> KoreError { KoreError::InvalidArgument(m.into()) }

pub fn is_row_group_file(data: &[u8]) -> bool {
    data.len() >= 4 + 1 + 4 + 4 && &data[..4] == START && &data[data.len() - 4..] == END
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stat { None, I64 { min: i64, max: i64 }, F64 { min: f64, max: f64 } }

#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub offset: u64,
    pub len: u64,
    pub rows: u64,
    /// (statistics, null count) per column, in `Index::columns` order
    pub stats: Vec<(Stat, u64)>,
}

#[derive(Debug, Clone)]
pub struct Index {
    pub columns: Vec<String>,
    pub groups: Vec<GroupInfo>,
}

/// Inclusive range filter on one numeric column; `None` leaves that side open.
#[derive(Debug, Clone)]
pub struct Range {
    pub column: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

fn column_stats(col: &Column) -> (Stat, u64) {
    match &col.data {
        ColumnData::Int64(v) => {
            let nulls = v.iter().filter(|x| x.is_none()).count() as u64;
            let mut it = v.iter().flatten();
            match it.next() {
                None => (Stat::None, nulls),
                Some(&first) => {
                    let (mn, mx) = it.fold((first, first), |(a, b), &x| (a.min(x), b.max(x)));
                    (Stat::I64 { min: mn, max: mx }, nulls)
                }
            }
        }
        ColumnData::Float64(v) => {
            let nulls = v.iter().filter(|x| x.is_none()).count() as u64;
            let mut it = v.iter().flatten().filter(|x| !x.is_nan());
            match it.next() {
                None => (Stat::None, nulls),
                Some(&first) => {
                    let (mn, mx) = it.fold((first, first), |(a, b), &x| (a.min(x), b.max(x)));
                    (Stat::F64 { min: mn, max: mx }, nulls)
                }
            }
        }
        ColumnData::Bool(v) => (Stat::None, v.iter().filter(|x| x.is_none()).count() as u64),
        ColumnData::Str(v) => (Stat::None, v.iter().filter(|x| x.is_none()).count() as u64),
        ColumnData::StrDict { codes, .. } => (Stat::None, codes.iter().filter(|&&c| c == u8::MAX).count() as u64),
    }
}

/// Split `block` into groups of at most `group_rows` rows and encode the container.
pub fn write(block: &DataBlock, group_rows: usize) -> Result<Vec<u8>, KoreError> {
    if group_rows == 0 { return Err(bad("group_rows must be positive")); }
    let n = block.num_rows;
    let ranges: Vec<(usize, usize)> = if n == 0 { vec![(0, 0)] }
        else { (0..n).step_by(group_rows).map(|s| (s, (s + group_rows).min(n))).collect() };
    if ranges.len() > MAX_GROUPS { return Err(bad("too many row groups")); }

    let mut out = Vec::new();
    out.extend_from_slice(START);
    out.push(VERSION);
    let mut infos = Vec::with_capacity(ranges.len());
    for (s, e) in ranges {
        let idx: Vec<usize> = (s..e).collect();
        let part = if (s, e) == (0, n) { block.clone() } else { block.select_rows(&idx) };
        let bytes = KoreWriter::to_bytes_plain(&part);
        infos.push(GroupInfo {
            offset: out.len() as u64,
            len: bytes.len() as u64,
            rows: (e - s) as u64,
            stats: part.columns.iter().map(column_stats).collect(),
        });
        out.extend_from_slice(&bytes);
    }

    let mut index = Vec::new();
    index.extend_from_slice(&(block.columns.len() as u32).to_le_bytes());
    for c in &block.columns {
        index.extend_from_slice(&(c.name.len() as u16).to_le_bytes());
        index.extend_from_slice(c.name.as_bytes());
    }
    index.extend_from_slice(&(infos.len() as u32).to_le_bytes());
    for g in &infos {
        index.extend_from_slice(&g.offset.to_le_bytes());
        index.extend_from_slice(&g.len.to_le_bytes());
        index.extend_from_slice(&g.rows.to_le_bytes());
        for (stat, nulls) in &g.stats {
            let (kind, a, b) = match *stat {
                Stat::None => (0u8, [0u8; 8], [0u8; 8]),
                Stat::I64 { min, max } => (1, min.to_le_bytes(), max.to_le_bytes()),
                Stat::F64 { min, max } => (2, min.to_le_bytes(), max.to_le_bytes()),
            };
            index.push(kind);
            index.extend_from_slice(&a);
            index.extend_from_slice(&b);
            index.extend_from_slice(&nulls.to_le_bytes());
        }
    }
    out.extend_from_slice(&index);
    out.extend_from_slice(&(index.len() as u32).to_le_bytes());
    out.extend_from_slice(END);
    Ok(out)
}

struct Cursor<'a> { d: &'a [u8], pos: usize }
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], KoreError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.d.len()).ok_or_else(|| bad("truncated row-group index"))?;
        let s = &self.d[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, KoreError> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16, KoreError> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    fn u32(&mut self) -> Result<u32, KoreError> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn u64(&mut self) -> Result<u64, KoreError> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
}

/// Parse and validate the footer index (cheap: does not touch the group data).
pub fn index(data: &[u8]) -> Result<Index, KoreError> {
    if !is_row_group_file(data) { return Err(bad("not a row-group file")); }
    if data[4] != VERSION { return Err(bad("unsupported row-group version")); }
    let tail = data.len() - 8;
    let index_len = u32::from_le_bytes(data[tail..tail + 4].try_into().unwrap()) as usize;
    let index_start = tail.checked_sub(index_len).filter(|&s| s >= 5).ok_or_else(|| bad("bad row-group index length"))?;
    let mut c = Cursor { d: &data[index_start..tail], pos: 0 };

    let ncols = c.u32()? as usize;
    if ncols > c.d.len() / 2 { return Err(bad("implausible column count")); }
    let mut columns = Vec::with_capacity(ncols);
    for _ in 0..ncols {
        let l = c.u16()? as usize;
        columns.push(String::from_utf8(c.take(l)?.to_vec()).map_err(|_| bad("invalid UTF-8 column name"))?);
    }
    let ngroups = c.u32()? as usize;
    if ngroups > MAX_GROUPS || ngroups > c.d.len() / 24 { return Err(bad("implausible group count")); }
    let mut groups = Vec::with_capacity(ngroups);
    for _ in 0..ngroups {
        let offset = c.u64()?;
        let len = c.u64()?;
        let rows = c.u64()?;
        let end = offset.checked_add(len).ok_or_else(|| bad("group range overflow"))?;
        if offset < 5 || end > index_start as u64 { return Err(bad("group lies outside the data area")); }
        let mut stats = Vec::with_capacity(ncols);
        for _ in 0..ncols {
            let kind = c.u8()?;
            let a: [u8; 8] = c.take(8)?.try_into().unwrap();
            let b: [u8; 8] = c.take(8)?.try_into().unwrap();
            let nulls = c.u64()?;
            let stat = match kind {
                0 => Stat::None,
                1 => Stat::I64 { min: i64::from_le_bytes(a), max: i64::from_le_bytes(b) },
                2 => Stat::F64 { min: f64::from_le_bytes(a), max: f64::from_le_bytes(b) },
                _ => return Err(bad("unknown statistics kind")),
            };
            stats.push((stat, nulls));
        }
        groups.push(GroupInfo { offset, len, rows, stats });
    }
    Ok(Index { columns, groups })
}

/// True when a group with this statistic could contain a value inside [lo, hi]. Never wrongly says no.
fn may_overlap(stat: Stat, lo: Option<f64>, hi: Option<f64>) -> bool {
    const EXACT: u64 = 1 << 53;
    let (a, b, slack) = match stat {
        Stat::None => return true,
        Stat::I64 { min, max } => {
            // i64 -> f64 can round by up to ~1024 beyond 2^53; widen so pruning stays safe
            let big = min.unsigned_abs() > EXACT || max.unsigned_abs() > EXACT;
            (min as f64, max as f64, if big { 4096.0 } else { 0.0 })
        }
        Stat::F64 { min, max } => (min, max, 0.0),
    };
    !(lo.map_or(false, |l| b + slack < l) || hi.map_or(false, |h| a - slack > h))
}

/// Indexes of groups that may contain rows inside `range`.
pub fn matching_groups(idx: &Index, range: &Range) -> Result<Vec<usize>, KoreError> {
    let col = idx.columns.iter().position(|c| *c == range.column)
        .ok_or_else(|| bad(&format!("column not found: {}", range.column)))?;
    Ok(idx.groups.iter().enumerate()
        .filter(|(_, g)| may_overlap(g.stats[col].0, range.min, range.max))
        .map(|(i, _)| i)
        .collect())
}

/// Read the table, decoding only `columns` (all when None) from the groups that may match `range`.
/// Rows inside a surviving group are returned unfiltered: pruning is per group, not per row.
pub fn read(data: &[u8], columns: Option<&[&str]>, range: Option<&Range>) -> Result<DataBlock, KoreError> {
    let idx = index(data)?;
    if idx.groups.is_empty() { return Ok(DataBlock::empty()); }
    let keep: Vec<usize> = match range {
        Some(r) => matching_groups(&idx, r)?,
        None => (0..idx.groups.len()).collect(),
    };
    let decode = |g: &GroupInfo| -> Result<DataBlock, KoreError> {
        let bytes = &data[g.offset as usize..(g.offset + g.len) as usize];
        let b = match columns {
            Some(c) => KoreReader::from_bytes_columns(bytes, c)?,
            None => KoreReader::from_bytes(bytes)?,
        };
        if b.num_rows as u64 != g.rows { return Err(bad("row group length disagrees with the index")); }
        Ok(b)
    };
    let mut parts = Vec::with_capacity(keep.len());
    for &i in &keep { parts.push(decode(&idx.groups[i])?); }
    if parts.is_empty() {
        // everything pruned: keep the schema, return no rows
        return Ok(decode(&idx.groups[0])?.select_rows(&[]));
    }
    concat(parts)
}

/// Concatenate groups. Unlike `DataBlock::concat` this tolerates a column being dictionary-encoded in
/// one group and plain in another, and never lets a merged dictionary outgrow its 8-bit codes.
fn concat(mut parts: Vec<DataBlock>) -> Result<DataBlock, KoreError> {
    if parts.len() == 1 { return Ok(parts.pop().unwrap()); }
    let first = &parts[0];
    let mut columns = Vec::with_capacity(first.columns.len());
    for ci in 0..first.columns.len() {
        let name = first.columns[ci].name.clone();
        if parts.iter().any(|p| p.columns.len() != first.columns.len() || p.columns[ci].name != name) {
            return Err(bad("row groups disagree on the schema"));
        }
        let data = match &first.columns[ci].data {
            ColumnData::Int64(_) => ColumnData::Int64(gather(&parts, ci, |d| if let ColumnData::Int64(v) = d { Some(v) } else { None })?),
            ColumnData::Float64(_) => ColumnData::Float64(gather(&parts, ci, |d| if let ColumnData::Float64(v) = d { Some(v) } else { None })?),
            ColumnData::Bool(_) => ColumnData::Bool(gather(&parts, ci, |d| if let ColumnData::Bool(v) = d { Some(v) } else { None })?),
            ColumnData::Str(_) | ColumnData::StrDict { .. } => {
                let same_dict = match &first.columns[ci].data {
                    ColumnData::StrDict { dict, .. } => parts.iter().all(|p| matches!(&p.columns[ci].data,
                        ColumnData::StrDict { dict: d, .. } if d == dict)),
                    _ => false,
                };
                if same_dict {
                    let dict = if let ColumnData::StrDict { dict, .. } = &first.columns[ci].data { dict.clone() } else { unreachable!() };
                    let codes = parts.iter().flat_map(|p| match &p.columns[ci].data {
                        ColumnData::StrDict { codes, .. } => codes.clone(), _ => vec![],
                    }).collect();
                    ColumnData::StrDict { codes, dict }
                } else {
                    let mut out: Vec<Option<String>> = Vec::new();
                    for p in &parts {
                        match &p.columns[ci].data {
                            ColumnData::Str(v) => out.extend(v.iter().cloned()),
                            ColumnData::StrDict { codes, dict } => out.extend(codes.iter()
                                .map(|&c| if c == u8::MAX { None } else { dict.get(c as usize).cloned() })),
                            _ => return Err(bad("row groups disagree on a column type")),
                        }
                    }
                    ColumnData::Str(out)
                }
            }
        };
        columns.push(Column { name, data });
    }
    let num_rows = parts.iter().map(|p| p.num_rows).sum();
    Ok(DataBlock { columns, num_rows })
}

fn gather<T: Clone>(parts: &[DataBlock], ci: usize, pick: impl Fn(&ColumnData) -> Option<&Vec<T>>) -> Result<Vec<T>, KoreError> {
    let mut out = Vec::new();
    for p in parts {
        out.extend(pick(&p.columns[ci].data).ok_or_else(|| bad("row groups disagree on a column type"))?.iter().cloned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: usize) -> DataBlock {
        DataBlock::new(vec![
            Column::int64("id", (0..n as i64).map(Some).collect()),
            Column::float64("x", (0..n).map(|i| if i % 50 == 7 { None } else { Some(i as f64 / 4.0) }).collect()),
            Column::str_col("s", (0..n).map(|i| Some(format!("v{}", i % if i < n / 2 { 5 } else { 400 }))).collect()),
        ]).unwrap()
    }

    #[test]
    fn roundtrip_and_pruning() {
        let b = block(10_000);
        let bytes = write(&b, 1000).unwrap();
        let idx = index(&bytes).unwrap();
        assert_eq!(idx.groups.len(), 10);
        let all = read(&bytes, None, None).unwrap();
        assert_eq!(all.num_rows, 10_000);
        assert_eq!(all.columns[0].data, b.columns[0].data);
        // dictionary in the early groups, plain strings later: must still merge
        match (&all.columns[2].data, &b.columns[2].data) {
            (ColumnData::Str(x), ColumnData::Str(y)) => assert_eq!(x, y),
            _ => {}
        }
        let r = Range { column: "id".into(), min: Some(2500.0), max: Some(3100.0) };
        assert_eq!(matching_groups(&idx, &r).unwrap(), vec![2, 3]);
        let part = read(&bytes, Some(&["id"]), Some(&r)).unwrap();
        assert_eq!(part.num_rows, 2000);
        let none = Range { column: "id".into(), min: Some(1e9), max: None };
        let empty = read(&bytes, Some(&["id", "x"]), Some(&none)).unwrap();
        assert_eq!((empty.num_rows, empty.columns.len()), (0, 2));
        assert!(matching_groups(&idx, &Range { column: "nope".into(), min: None, max: None }).is_err());
    }

    #[test]
    fn large_i64_pruning_stays_safe() {
        let big = i64::MAX - 10;
        let b = DataBlock::new(vec![Column::int64("v", vec![Some(big), Some(big + 5)])]).unwrap();
        let bytes = write(&b, 10).unwrap();
        let idx = index(&bytes).unwrap();
        // the exact value must never be pruned even though f64 cannot represent it
        let r = Range { column: "v".into(), min: Some((big + 5) as f64), max: Some((big + 5) as f64) };
        assert_eq!(matching_groups(&idx, &r).unwrap(), vec![0]);
    }

    #[test]
    fn malformed_indexes_are_errors() {
        let bytes = write(&block(100), 10).unwrap();
        for cut in 0..bytes.len() { let _ = index(&bytes[..cut]); let _ = read(&bytes[..cut], None, None); }
        for i in (0..bytes.len()).step_by(7) {
            let mut m = bytes.clone();
            m[i] ^= 0xA5;
            let _ = index(&m);
            let _ = read(&m, None, None);
        }
    }
}
