use std::path::Path;
use std::fs::{OpenOptions, File};
use std::io::{Write, Read};

/// Simple append-only WAL for staging manifest changes.
pub fn append_wal(dir: &Path, entry: &str) -> std::io::Result<()> {
    let wal_path = dir.join("kore.wal");
    let mut f = OpenOptions::new().create(true).append(true).open(&wal_path)?;
    let bytes = entry.as_bytes();
    let len = (bytes.len() as u32).to_le_bytes();
    f.write_all(&len)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

pub fn replay_wal(dir: &Path) -> std::io::Result<Vec<String>> {
    let wal_path = dir.join("kore.wal");
    if !wal_path.exists() { return Ok(vec![]); }
    let mut f = File::open(&wal_path)?;
    let mut res = Vec::new();
    loop {
        let mut lenb = [0u8;4];
        if f.read_exact(&mut lenb).is_err() { break; }
        let len = u32::from_le_bytes(lenb) as usize;
        let mut buf = vec![0u8; len];
        f.read_exact(&mut buf)?;
        res.push(String::from_utf8_lossy(&buf).into_owned());
    }
    Ok(res)
}

pub fn clear_wal(dir: &Path) -> std::io::Result<()> {
    let wal_path = dir.join("kore.wal");
    if wal_path.exists() { std::fs::remove_file(wal_path)?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn wal_roundtrip() {
        let dir = tempdir().unwrap();
        append_wal(dir.path(), "m1").unwrap();
        append_wal(dir.path(), "m2").unwrap();
        let entries = replay_wal(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], "m1");
        assert_eq!(entries[1], "m2");
    }

    #[test]
    fn wal_clear() {
        let dir = tempdir().unwrap();
        append_wal(dir.path(), "x").unwrap();
        clear_wal(dir.path()).unwrap();
        let entries = replay_wal(dir.path()).unwrap();
        assert_eq!(entries.len(), 0);
    }
}
