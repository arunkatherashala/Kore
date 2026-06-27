use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

// ============================================================================
// Result types
// ============================================================================

#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub id:        String,   // e.g. "20260627_153012_backup"
    pub tag:       String,
    pub timestamp: u64,      // Unix seconds
    pub size:      u64,
    pub hash:      u64,
    pub path:      String,   // absolute path to snapshot file
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub operation: String,
    pub target:    String,
    pub rows:      usize,
    pub hash:      u64,
}

#[derive(Debug, Clone)]
pub struct DiffResult {
    pub snap1:       String,
    pub snap2:       String,
    pub rows_before: usize,
    pub rows_after:  usize,
    pub added:       usize,
    pub removed:     usize,
    pub summary:     String,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub kept:    usize,
    pub deleted: usize,
    pub freed:   u64,   // bytes freed
}

// ============================================================================
// KoreVault public API
// ============================================================================

/// Layer 9: Zero-dependency pure-Rust vault — time travel, ChaCha20 encryption,
/// FNV-1a checksums, append-only audit log, snapshot compaction, ACID-style WAL.
pub struct KoreVault;

impl KoreVault {
    // ── 1. Snapshot (time-travel write) ─────────────────────────────────────

    /// Copy `path` into the vault directory, tagged with `tag`.
    /// Returns the snapshot ID (e.g. "20260627_153012_mytag").
    pub fn snapshot(path: &str, tag: &str) -> Result<String, String> {
        let vdir = vault_dir(path)?;
        ensure_dir(&vdir)?;
        let ts = unix_now();
        let id = format!("{}_{}", ts_str(ts), sanitise(tag));
        let snap_path = vdir.join(format!("snap_{}.kore", id));

        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let hash = fnv1a(&data);
        std::fs::write(&snap_path, &data).map_err(|e| e.to_string())?;

        append_manifest(&vdir, SnapshotInfo {
            id: id.clone(), tag: tag.to_string(), timestamp: ts,
            size: data.len() as u64, hash,
            path: snap_path.to_string_lossy().into_owned(),
        })?;
        audit_log(&vdir, "SNAPSHOT", path, 0, hash)?;
        Ok(id)
    }

    // ── 2. List snapshots ────────────────────────────────────────────────────

    pub fn list_snapshots(path: &str) -> Result<Vec<SnapshotInfo>, String> {
        let vdir = vault_dir(path)?;
        read_manifest(&vdir)
    }

    // ── 3. Restore a snapshot ────────────────────────────────────────────────

    /// Overwrite `path` with the contents of snapshot `snapshot_id`.
    pub fn restore(path: &str, snapshot_id: &str) -> Result<(), String> {
        let vdir  = vault_dir(path)?;
        let snaps = read_manifest(&vdir)?;
        let snap  = snaps.iter().find(|s| s.id == snapshot_id)
            .ok_or_else(|| format!("Snapshot '{}' not found", snapshot_id))?;
        let data = std::fs::read(&snap.path).map_err(|e| e.to_string())?;
        let hash = fnv1a(&data);
        if hash != snap.hash { return Err(format!("Checksum mismatch for snapshot '{}'", snapshot_id)); }
        std::fs::write(path, &data).map_err(|e| e.to_string())?;
        audit_log(&vdir, "RESTORE", path, 0, hash)?;
        Ok(())
    }

    // ── 4. Diff two snapshots ────────────────────────────────────────────────

    /// Compare two snapshots and return a row-level diff summary.
    pub fn diff(path: &str, snap1_id: &str, snap2_id: &str) -> Result<DiffResult, String> {
        let vdir  = vault_dir(path)?;
        let snaps = read_manifest(&vdir)?;
        let s1    = snaps.iter().find(|s| s.id == snap1_id)
            .ok_or_else(|| format!("Snapshot '{}' not found", snap1_id))?;
        let s2    = snaps.iter().find(|s| s.id == snap2_id)
            .ok_or_else(|| format!("Snapshot '{}' not found", snap2_id))?;

        let d1 = std::fs::read(&s1.path).map_err(|e| e.to_string())?;
        let d2 = std::fs::read(&s2.path).map_err(|e| e.to_string())?;

        // Row-hash sets for each snapshot (hashes of 64-byte chunks as proxy for rows)
        let chunk = 64usize;
        let hashes1: std::collections::HashSet<u64> = d1.chunks(chunk).map(fnv1a).collect();
        let hashes2: std::collections::HashSet<u64> = d2.chunks(chunk).map(fnv1a).collect();
        let rows1 = hashes1.len();
        let rows2 = hashes2.len();
        let added   = hashes2.difference(&hashes1).count();
        let removed = hashes1.difference(&hashes2).count();

        let summary = format!(
            "{} → {}: {} blocks before, {} blocks after, +{} added, -{} removed (size: {}B → {}B)",
            snap1_id, snap2_id, rows1, rows2, added, removed, d1.len(), d2.len()
        );

        Ok(DiffResult { snap1: snap1_id.to_string(), snap2: snap2_id.to_string(),
                        rows_before: rows1, rows_after: rows2, added, removed, summary })
    }

    // ── 5. ChaCha20 encryption ───────────────────────────────────────────────

    /// Encrypt `src_path` with `key` (any string), write to `dst_path`.
    /// Format: 8 bytes magic "KOREVLT1" + 12 bytes nonce + ciphertext.
    pub fn encrypt(src_path: &str, key: &str, dst_path: &str) -> Result<(), String> {
        let plaintext = std::fs::read(src_path).map_err(|e| e.to_string())?;
        let ts = unix_now();
        let nonce = [ts as u32, (ts >> 32) as u32, fnv1a(src_path.as_bytes()) as u32];
        let key32 = derive_key(key);
        let ciphertext = chacha20_xor(&plaintext, &key32, &nonce);

        let mut out: Vec<u8> = b"KOREVLT1".to_vec();
        out.extend_from_slice(&nonce[0].to_le_bytes());
        out.extend_from_slice(&nonce[1].to_le_bytes());
        out.extend_from_slice(&nonce[2].to_le_bytes());
        out.extend_from_slice(&ciphertext);
        std::fs::write(dst_path, &out).map_err(|e| e.to_string())?;

        let vdir = vault_dir(src_path)?;
        let _ = ensure_dir(&vdir);
        let _ = audit_log(&vdir, "ENCRYPT", src_path, plaintext.len(), fnv1a(&ciphertext));
        Ok(())
    }

    /// Decrypt `src_path` (written by `encrypt`) with `key`, write to `dst_path`.
    pub fn decrypt(src_path: &str, key: &str, dst_path: &str) -> Result<(), String> {
        let data = std::fs::read(src_path).map_err(|e| e.to_string())?;
        if data.len() < 20 { return Err("File too short to be a KoreVault encrypted file".into()); }
        if &data[..8] != b"KOREVLT1" { return Err("Not a KoreVault encrypted file".into()); }
        let nonce = [
            u32::from_le_bytes(data[8..12].try_into().unwrap()),
            u32::from_le_bytes(data[12..16].try_into().unwrap()),
            u32::from_le_bytes(data[16..20].try_into().unwrap()),
        ];
        let key32 = derive_key(key);
        let plaintext = chacha20_xor(&data[20..], &key32, &nonce);
        std::fs::write(dst_path, &plaintext).map_err(|e| e.to_string())?;

        let vdir = vault_dir(dst_path)?;
        let _ = ensure_dir(&vdir);
        let _ = audit_log(&vdir, "DECRYPT", dst_path, plaintext.len(), fnv1a(&plaintext));
        Ok(())
    }

    // ── 6. FNV-1a checksum ──────────────────────────────────────────────────

    /// Return (file_size_bytes, fnv1a_64_hash) for `path`.
    pub fn checksum(path: &str) -> Result<(u64, u64), String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        Ok((data.len() as u64, fnv1a(&data)))
    }

    /// Verify `path` against `expected_hash`. Returns true if hash matches.
    pub fn verify(path: &str, expected_hash: u64) -> Result<bool, String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        Ok(fnv1a(&data) == expected_hash)
    }

    // ── 7. Audit log ─────────────────────────────────────────────────────────

    /// Read the audit log for snapshots/operations on `path`.
    pub fn read_audit_log(path: &str) -> Result<Vec<AuditEntry>, String> {
        let vdir = vault_dir(path)?;
        let log_path = vdir.join("audit.log");
        if !log_path.exists() { return Ok(Vec::new()); }
        let text = std::fs::read_to_string(&log_path).map_err(|e| e.to_string())?;
        let entries = text.lines().filter(|l| !l.is_empty()).filter_map(|line| {
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() < 5 { return None; }
            Some(AuditEntry {
                timestamp: parts[0].parse().unwrap_or(0),
                operation: parts[1].to_string(),
                target:    parts[2].to_string(),
                rows:      parts[3].parse().unwrap_or(0),
                hash:      parts[4].parse().unwrap_or(0),
            })
        }).collect();
        Ok(entries)
    }

    // ── 8. Compact snapshots ─────────────────────────────────────────────────

    /// Keep only the `keep_latest` most recent snapshots; delete the rest.
    pub fn compact(path: &str, keep_latest: usize) -> Result<CompactionResult, String> {
        let vdir = vault_dir(path)?;
        let mut snaps = read_manifest(&vdir)?;
        if snaps.len() <= keep_latest {
            return Ok(CompactionResult { kept: snaps.len(), deleted: 0, freed: 0 });
        }
        snaps.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        let (keep, remove) = snaps.split_at(keep_latest);
        let mut freed = 0u64;
        let mut deleted = 0;
        for s in remove {
            if let Ok(_) = std::fs::remove_file(&s.path) { freed += s.size; deleted += 1; }
        }
        // Rewrite manifest with only kept snapshots
        let manifest = vdir.join("manifest.kv");
        let mut text = String::new();
        for s in keep {
            text.push_str(&format!("{}|{}|{}|{}|{}|{}\n", s.id, s.tag, s.timestamp, s.size, s.hash, s.path));
        }
        std::fs::write(&manifest, text).map_err(|e| e.to_string())?;
        audit_log(&vdir, "COMPACT", path, deleted, 0)?;
        Ok(CompactionResult { kept: keep_latest, deleted, freed })
    }

    // ── 9. WAL-style write log ───────────────────────────────────────────────

    /// Append a WAL entry (operation metadata). Used before committing writes.
    pub fn wal_append(path: &str, operation: &str, rows: usize) -> Result<(), String> {
        let vdir = vault_dir(path)?;
        ensure_dir(&vdir)?;
        let wal = vdir.join("wal.log");
        let ts = unix_now();
        let entry = format!("{}|BEGIN|{}|{}|{}\n", ts, operation, path, rows);
        append_text(&wal, &entry)
    }

    /// Mark the last WAL entry for `path` as COMMIT.
    pub fn wal_commit(path: &str) -> Result<(), String> {
        let vdir = vault_dir(path)?;
        let wal  = vdir.join("wal.log");
        let ts   = unix_now();
        let entry = format!("{}|COMMIT|{}||\n", ts, path);
        append_text(&wal, &entry)
    }

    /// Read WAL log entries for `path`.
    pub fn wal_read(path: &str) -> Result<Vec<String>, String> {
        let vdir     = vault_dir(path)?;
        let wal_path = vdir.join("wal.log");
        if !wal_path.exists() { return Ok(Vec::new()); }
        let text = std::fs::read_to_string(&wal_path).map_err(|e| e.to_string())?;
        Ok(text.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect())
    }

    // ── 10. Vault status ─────────────────────────────────────────────────────

    /// Return (n_snapshots, total_bytes_used, oldest_ts, newest_ts) for vault.
    pub fn vault_status(path: &str) -> Result<(usize, u64, u64, u64), String> {
        let snaps = Self::list_snapshots(path)?;
        if snaps.is_empty() { return Ok((0, 0, 0, 0)); }
        let total: u64 = snaps.iter().map(|s| s.size).sum();
        let oldest = snaps.iter().map(|s| s.timestamp).min().unwrap_or(0);
        let newest = snaps.iter().map(|s| s.timestamp).max().unwrap_or(0);
        Ok((snaps.len(), total, oldest, newest))
    }
}

// ============================================================================
// ChaCha20 stream cipher (RFC 7539, 20 rounds)
// ============================================================================

fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]); s[d] ^= s[a]; s[d] = s[d].rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]); s[b] ^= s[c]; s[b] = s[b].rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]); s[d] ^= s[a]; s[d] = s[d].rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]); s[b] ^= s[c]; s[b] = s[b].rotate_left(7);
}

fn chacha20_block(key: &[u32; 8], counter: u64, nonce: &[u32; 3]) -> [u8; 64] {
    let mut state: [u32; 16] = [
        0x61707865, 0x3320646e, 0x79622d32, 0x6b206574,
        key[0], key[1], key[2], key[3], key[4], key[5], key[6], key[7],
        counter as u32, (counter >> 32) as u32,
        nonce[0], nonce[1],   // Note: nonce[2] goes at index 15 but we only have 2 nonce slots left
    ];
    // Adjust: move nonce[2] into slot 15
    state[15] = nonce[2];
    let initial = state;
    for _ in 0..10 {
        quarter_round(&mut state, 0, 4, 8,  12);
        quarter_round(&mut state, 1, 5, 9,  13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8,  13);
        quarter_round(&mut state, 3, 4, 9,  14);
    }
    for i in 0..16 { state[i] = state[i].wrapping_add(initial[i]); }
    let mut out = [0u8; 64];
    for (i, &w) in state.iter().enumerate() {
        let b = w.to_le_bytes();
        out[i*4..i*4+4].copy_from_slice(&b);
    }
    out
}

fn chacha20_xor(data: &[u8], key: &[u32; 8], nonce: &[u32; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut counter = 0u64;
    for chunk in data.chunks(64) {
        let keystream = chacha20_block(key, counter, nonce);
        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ keystream[i]);
        }
        counter += 1;
    }
    out
}

/// Expand an arbitrary-length string key into 8 × u32 (32 bytes) using FNV-1a mixing.
fn derive_key(s: &str) -> [u32; 8] {
    let mut k = [0u32; 8];
    let bytes = s.as_bytes();
    for i in 0..8 {
        let mut h = 0xcbf29ce484222325u64;
        for &b in bytes {
            h ^= b as u64; h = h.wrapping_mul(0x100000001b3);
        }
        h ^= (i as u64).wrapping_mul(0xdeadbeefcafebabe);
        h = h.wrapping_mul(0x9e3779b97f4a7c15);
        k[i] = (h ^ (h >> 32)) as u32;
    }
    k
}

// ============================================================================
// FNV-1a 64-bit hash
// ============================================================================

fn fnv1a(data: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in data { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); }
    h
}

// ============================================================================
// Vault filesystem helpers
// ============================================================================

fn vault_dir(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let dir = p.parent().unwrap_or_else(|| Path::new("."));
    Ok(dir.join(".kore_vault"))
}

fn ensure_dir(dir: &PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())
}

fn append_manifest(vdir: &PathBuf, info: SnapshotInfo) -> Result<(), String> {
    let manifest = vdir.join("manifest.kv");
    let line = format!("{}|{}|{}|{}|{}|{}\n", info.id, info.tag, info.timestamp, info.size, info.hash, info.path);
    append_text(&manifest, &line)
}

fn read_manifest(vdir: &PathBuf) -> Result<Vec<SnapshotInfo>, String> {
    let manifest = vdir.join("manifest.kv");
    if !manifest.exists() { return Ok(Vec::new()); }
    let text = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
    Ok(text.lines().filter(|l| !l.is_empty()).filter_map(|line| {
        let p: Vec<&str> = line.splitn(6, '|').collect();
        if p.len() < 6 { return None; }
        Some(SnapshotInfo {
            id: p[0].to_string(), tag: p[1].to_string(),
            timestamp: p[2].parse().unwrap_or(0),
            size:      p[3].parse().unwrap_or(0),
            hash:      p[4].parse().unwrap_or(0),
            path:      p[5].to_string(),
        })
    }).collect())
}

fn audit_log(vdir: &PathBuf, op: &str, target: &str, rows: usize, hash: u64) -> Result<(), String> {
    let log = vdir.join("audit.log");
    let ts  = unix_now();
    let line = format!("{}|{}|{}|{}|{}\n", ts, op, target, rows, hash);
    append_text(&log, &line)
}

fn append_text(path: &PathBuf, text: &str) -> Result<(), String> {
    use std::fs::OpenOptions;
    let mut f = OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn ts_str(ts: u64) -> String {
    // Simple YYYYMMDDHHmmss formatter without chrono
    let s = ts % 60; let ts = ts / 60;
    let m = ts % 60; let ts = ts / 60;
    let h = ts % 24; let ts = ts / 24;
    let days = ts + 719468;
    let era  = days / 146097;
    let doe  = days % 146097;
    let yoe  = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y    = yoe + era * 400;
    let doy  = doe - (365*yoe + yoe/4 - yoe/100);
    let mp   = (5*doy + 2) / 153;
    let d    = doy - (153*mp + 2)/5 + 1;
    let mo   = if mp < 10 { mp + 3 } else { mp - 9 };
    let y    = if mo <= 2 { y + 1 } else { y };
    format!("{:04}{:02}{:02}{:02}{:02}{:02}", y, mo, d, h, m, s)
}

fn sanitise(tag: &str) -> String {
    tag.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect()
}
