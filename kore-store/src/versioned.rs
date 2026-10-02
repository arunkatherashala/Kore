//! Append-only version log for time travel.
//!
//! Layout: `KVLG | count:u32 | (timestamp:u64 | len:u64 | .kore bytes)*`
//! Every entry is a complete, independently readable .kore file.

use kore_core::{DataBlock, KoreError};
use crate::{KoreReader, KoreWriter};

pub const LOG_MAGIC: &[u8; 4] = b"KVLG";

fn bad(m: &str) -> KoreError { KoreError::InvalidArgument(m.into()) }

pub fn is_version_log(data: &[u8]) -> bool { data.len() >= 8 && &data[..4] == LOG_MAGIC }

/// Parse entries as (timestamp, bytes), validating all bounds.
fn entries(data: &[u8]) -> Result<Vec<(u64, &[u8])>, KoreError> {
    if !is_version_log(data) { return Err(bad("not a version log")); }
    let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let mut pos = 8usize;
    let mut out = Vec::new();
    for _ in 0..count {
        let hdr = data.get(pos..pos + 16).ok_or_else(|| bad("truncated version log"))?;
        let ts = u64::from_le_bytes(hdr[..8].try_into().unwrap());
        let len = usize::try_from(u64::from_le_bytes(hdr[8..].try_into().unwrap()))
            .map_err(|_| bad("version length overflow"))?;
        pos += 16;
        let end = pos.checked_add(len).filter(|&e| e <= data.len()).ok_or_else(|| bad("truncated version log"))?;
        out.push((ts, &data[pos..end]));
        pos = end;
    }
    Ok(out)
}

/// Append a block as a new version (see `append_raw`).
pub fn append_version(existing: Option<&[u8]>, block: &DataBlock, timestamp: u64) -> Result<Vec<u8>, KoreError> {
    if let Some(d) = existing { if !is_version_log(d) { KoreReader::from_bytes(d)?; } }
    append_raw(existing, &KoreWriter::to_bytes(block), timestamp)
}

/// Append opaque entry bytes (any .kore flavour) as a new version. `existing` is the current
/// log, a plain file (kept as the oldest version, timestamp 0), or None to start a new log.
/// Timestamps must be strictly increasing.
pub fn append_raw(existing: Option<&[u8]>, entry: &[u8], timestamp: u64) -> Result<Vec<u8>, KoreError> {
    let mut versions: Vec<(u64, &[u8])> = match existing {
        None => vec![],
        Some(d) if is_version_log(d) => entries(d)?,
        Some(d) => vec![(0, d)],
    };
    if let Some((last, _)) = versions.last() {
        if timestamp <= *last { return Err(bad("version timestamp must be greater than the latest version")); }
    }
    versions.push((timestamp, entry));
    let mut out = Vec::new();
    out.extend_from_slice(LOG_MAGIC);
    out.extend_from_slice(&(versions.len() as u32).to_le_bytes());
    for (ts, bytes) in versions {
        out.extend_from_slice(&ts.to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

/// Raw bytes of the newest version with timestamp <= `target`.
pub fn select_raw(data: &[u8], target: u64) -> Result<&[u8], KoreError> {
    entries(data)?.into_iter().rev().find(|(ts, _)| *ts <= target).map(|(_, b)| b)
        .ok_or_else(|| bad("no version at or before the requested timestamp"))
}

/// List (timestamp, row_count) for every version, oldest first.
pub fn list_versions(data: &[u8]) -> Result<Vec<(u64, usize)>, KoreError> {
    entries(data)?.into_iter()
        .map(|(ts, b)| Ok((ts, KoreReader::from_bytes(b)?.num_rows)))
        .collect()
}

/// Read the newest version with timestamp <= `target`.
pub fn read_at(data: &[u8], target: u64) -> Result<DataBlock, KoreError> {
    KoreReader::from_bytes(select_raw(data, target)?)
}

/// Read the latest version.
pub fn read_latest(data: &[u8]) -> Result<DataBlock, KoreError> {
    let e = entries(data)?;
    let (_, bytes) = e.last().ok_or_else(|| bad("empty version log"))?;
    KoreReader::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::Column;

    fn blk(n: i64) -> DataBlock {
        DataBlock::new(vec![Column::int64("x", (0..n).map(Some).collect())]).unwrap()
    }

    #[test]
    fn time_travel() {
        let l = append_version(None, &blk(1), 100).unwrap();
        let l = append_version(Some(&l), &blk(2), 200).unwrap();
        let l = append_version(Some(&l), &blk(3), 300).unwrap();
        assert_eq!(list_versions(&l).unwrap(), vec![(100, 1), (200, 2), (300, 3)]);
        assert_eq!(read_at(&l, 250).unwrap().num_rows, 2);
        assert_eq!(read_at(&l, 100).unwrap().num_rows, 1);
        assert_eq!(read_at(&l, u64::MAX).unwrap().num_rows, 3);
        assert!(read_at(&l, 99).is_err());
        assert_eq!(read_latest(&l).unwrap().num_rows, 3);
    }

    #[test]
    fn rejects_non_monotonic_and_adopts_plain_file() {
        let plain = KoreWriter::to_bytes(&blk(5));
        let l = append_version(Some(&plain), &blk(6), 10).unwrap();
        assert_eq!(read_at(&l, 5).unwrap().num_rows, 5);
        assert!(append_version(Some(&l), &blk(7), 10).is_err());
    }

    #[test]
    fn truncated_log_is_error() {
        let l = append_version(None, &blk(4), 1).unwrap();
        for cut in 0..l.len() { let _ = read_latest(&l[..cut]); }
    }
}
