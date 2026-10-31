//! Explicit migration of an existing log to format 2 (a new directory; the
//! source is only read). The migrated log has the same versions 0..=N with the
//! same `recorded_at` and identical row contents (checked version by version via
//! `Snapshot::content_hash`), re-encoded as v2 segments. Manifest hashes differ
//! from the source (segment bytes differ), forget records are carried over
//! verbatim (their `prior_head` still names the source chain), and one final
//! commit N+1 records `schema_meta.format_version = 2`.
//!
//! In-place alternative without rewriting anything: `VersionedLog::upgrade_to_v2`.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MigrateReport {
    /// Versions copied (0..=latest of the source); the destination has one more (the format commit).
    pub versions_copied: u64,
    pub segments: usize,
    pub src_bytes: u64,
    pub dst_bytes: u64,
    pub src_head: String,
    pub dst_head: String,
}

fn dir_bytes(dir: &Path) -> u64 {
    let mut t = 0;
    for tname in ALL_TABLES.iter().map(|t| t.name()).chain(["."]) {
        let d = if tname == "." { dir.to_path_buf() } else { dir.join(tname) };
        if let Ok(rd) = fs::read_dir(d) {
            for f in rd.flatten() {
                if let Ok(m) = f.metadata() {
                    if m.is_file() {
                        t += m.len();
                    }
                }
            }
        }
    }
    t
}

/// Copy `<src_root>/<owner>/log` to `<dst_root>/<owner>/log` as a format-2 log.
/// Refuses if the destination already has a MANIFEST or the source fails verification.
pub fn migrate_to_v2(src_root: &Path, dst_root: &Path, owner: &str) -> Result<MigrateReport, LogError> {
    let src = VersionedLog::open(src_root, owner)?;
    src.verify()?;
    if !valid_owner(owner) {
        return inv("owner must be 1-64 chars of [A-Za-z0-9_-]");
    }
    let dir = dst_root.join(owner).join("log");
    if dir.join("MANIFEST").exists() {
        return Err(LogError::Conflict("destination log already exists".into()));
    }
    fs::create_dir_all(&dir)?;
    for t in ALL_TABLES {
        fs::create_dir_all(dir.join(t.name()))?;
    }
    let mut dst = VersionedLog {
        dir: dir.clone(),
        entries: vec![],
        manifest_raw: format!("{}\n", manifest::HEADER).into_bytes(),
        head: Mutex::new(None),
        index: Index::default(),
        ev_segs: vec![],
        format: LogFormat::V2,
        clock: Box::new(now_iso),
    };
    let mut nseg = 0usize;
    for e in &src.entries {
        let mut new = Vec::new();
        for g in &e.segments {
            let t = seg_table(g)?;
            new.push((t, load_segment(&src.dir, g)?));
            nseg += 1;
        }
        dst.write_commit(new, e.recorded_at.clone(), LogFormat::V2, e.forget.clone())?;
    }
    let last_at = src.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
    dst.set_clock(move || last_at.clone());
    // The copy is complete; reopen from disk (verifies chain + checksums) and record the format.
    let mut dst = VersionedLog::open(dst_root, owner)?;
    let last_at = src.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
    dst.set_clock(move || last_at.clone());
    dst.upgrade_to_v2()?;
    for v in 0..=src.latest_version() {
        let a = src.open_at(AsOf::Version(v))?.content_hash();
        let b = dst.open_at(AsOf::Version(v))?.content_hash();
        if a != b {
            return Err(LogError::Io(format!("migration check failed: content differs at version {v}; destination must be discarded")));
        }
    }
    dst.verify()?;
    Ok(MigrateReport {
        versions_copied: src.latest_version() + 1,
        segments: nseg,
        src_bytes: dir_bytes(&src.dir),
        dst_bytes: dir_bytes(&dst.dir),
        src_head: src.snapshot_hash(src.latest_version())?,
        dst_head: dst.snapshot_hash(dst.latest_version())?,
    })
}
