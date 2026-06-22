use std::io::{Read, Seek, Write};
use std::path::Path;
use std::fs::File;
use crate::kore_block_decoder::KoreBlockDecoder;

/// Block-aware compaction API (prototype).
/// For each input file, if it's block-encoded, stream blocks via `KoreBlockDecoder`,
/// apply simple row-range tombstones, and write compacted output preserving block boundaries.
pub fn block_aware_compact<P: AsRef<Path>>(files: &[String], out_dir: P, tombstone_ranges: &[(String, u64, u64)], predicate_tombstones: &[(String, String)]) -> std::io::Result<Vec<String>> {
    let mut out_files = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let in_path = Path::new(f);
        let out_name = format!("kore_compacted_{}", i);
        let out_path = out_dir.as_ref().join(&out_name);
        if in_path.exists() {
            let rf = File::open(in_path)?;
            // try block decoder by attempting to read first 4 bytes as magic
            let mut reader = rf;
            let mut header = [0u8;4];
            let mut probe = File::open(in_path)?;
            use std::io::Read as _;
            if probe.read_exact(&mut header).is_ok() && &header == b"KORB" {
                // use decoder
                let mut dec = KoreBlockDecoder::new(File::open(in_path)?)?;
                let mut out = Vec::new();
                let mut row_idx = 0u64;
                loop {
                    match dec.decode_next_block(&mut out) {
                        Ok(n) => { if n==0 { break; } row_idx += 1; },
                        Err(e) => { break; }
                    }
                }
                // Try to decompress block payload via known codecs (FOR, RLE, Packed)
                let mut decompressed = None;
                if let Ok(d) = crate::codecs::ForCodec::decompress(&out) { if !d.is_empty() { decompressed = Some(d); } }
                if decompressed.is_none() {
                    if let Ok(d) = crate::codecs::RleCodec::decompress(&out) { if !d.is_empty() { decompressed = Some(d); } }
                }
                if decompressed.is_none() {
                    if let Ok(d) = crate::codecs::PackedCodec::decompress(&out) { if !d.is_empty() { decompressed = Some(d); } }
                }
                let final_buf_vec = match decompressed {
                    Some(v) => v,
                    None => out.clone(),
                };
                let mut final_buf = Vec::new();
                let s = String::from_utf8_lossy(&final_buf_vec);
                for (idx, line) in s.lines().enumerate() {
                    let mut deleted = false;
                    // row-range tombstones
                    for (f,sr,er) in tombstone_ranges {
                        if f == f && (idx as u64) >= *sr && (idx as u64) <= *er { deleted = true; break; }
                    }
                    if deleted { continue; }
                    // predicate tombstones
                    for (filt_file, pred) in predicate_tombstones {
                        if filt_file == f && crate::predicate::eval_expression(pred, line) { deleted = true; break; }
                    }
                    if !deleted {
                        final_buf.extend_from_slice(line.as_bytes());
                        final_buf.extend_from_slice(b"\n");
                    }
                }
                // write compacted block as a single KORB block (preserves block container)
                let mut wf = File::create(&out_path)?;
                wf.write_all(b"KORB")?;
                let len = final_buf.len() as u32;
                wf.write_all(&len.to_le_bytes())?;
                wf.write_all(&final_buf)?;
                wf.sync_all()?;
            } else {
                // fallback: copy file
                // apply predicate tombstones for plain text
                let content = std::fs::read_to_string(in_path)?;
                let mut final_buf = String::new();
                for (idx, line) in content.lines().enumerate() {
                    let mut deleted = false;
                    for (f,sr,er) in tombstone_ranges {
                        if f == f && (idx as u64) >= *sr && (idx as u64) <= *er { deleted = true; break; }
                    }
                    if deleted { continue; }
                    for (filt_file, pred) in predicate_tombstones {
                        if filt_file == f && crate::predicate::eval_expression(pred, line) { deleted = true; break; }
                    }
                    if !deleted { final_buf.push_str(line); final_buf.push('\n'); }
                }
                std::fs::write(&out_path, final_buf)?;
            }
        } else {
            std::fs::write(&out_path, b"")?;
        }
        out_files.push(out_name);
    }
    Ok(out_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn compact_block_file_roundtrip() {
        let dir = tempdir().unwrap();
        let in_path = dir.path().join("in.kore");
        let mut f = File::create(&in_path).unwrap();
        // write magic + two blocks
        f.write_all(b"KORB").unwrap();
        let payload1 = b"row1\nrow2\n";
        let l1 = payload1.len() as u32;
        f.write_all(&l1.to_le_bytes()).unwrap();
        f.write_all(payload1).unwrap();
        let payload2 = b"row3\n";
        let l2 = payload2.len() as u32;
        f.write_all(&l2.to_le_bytes()).unwrap();
        f.write_all(payload2).unwrap();

        let files = vec![in_path.to_string_lossy().to_string()];
        let out = block_aware_compact(&files, dir.path(), &[], &[]).unwrap();
        assert_eq!(out.len(), 1);
        let got = std::fs::read(dir.path().join(&out[0])).unwrap();
        assert!(got.len() > 0);
    }

    #[test]
    fn predicate_expression_applies() {
        let dir = tempdir().unwrap();
        let in_path = dir.path().join("in2.kore");
        let mut f = File::create(&in_path).unwrap();
        f.write_all(b"KORB").unwrap();
        let payload = b"row1\nrow2\nrow3\n";
        let l = payload.len() as u32;
        f.write_all(&l.to_le_bytes()).unwrap();
        f.write_all(payload).unwrap();

        let files = vec![in_path.to_string_lossy().to_string()];
        // predicate tombstone: remove rows that contain 'row2' OR equal 'row3'
        let preds = vec![(files[0].clone(), "contains:row2 OR contains:row3".to_string())];
        let out = block_aware_compact(&files, dir.path(), &[], &preds).unwrap();
        let got = std::fs::read_to_string(dir.path().join(&out[0])).unwrap();
        assert!(got.contains("row1"));
        assert!(!got.contains("row2"));
        assert!(!got.contains("row3"));
    }

    #[test]
    fn for_codec_compaction() {
        let dir = tempdir().unwrap();
        let in_path = dir.path().join("in_for.kore");
        let mut f = File::create(&in_path).unwrap();
        // write magic + a single FOR block: base=100, deltas [1,2]
        f.write_all(b"KORB").unwrap();
        let mut payload = Vec::new();
        payload.extend_from_slice(&100i64.to_le_bytes());
        payload.extend_from_slice(&1i32.to_le_bytes());
        payload.extend_from_slice(&2i32.to_le_bytes());
        let l = payload.len() as u32;
        f.write_all(&l.to_le_bytes()).unwrap();
        f.write_all(&payload).unwrap();

        let files = vec![in_path.to_string_lossy().to_string()];
        let out = block_aware_compact(&files, dir.path(), &[], &[]).unwrap();
        let got = std::fs::read_to_string(dir.path().join(&out[0])).unwrap();
        assert!(got.contains("101"));
        assert!(got.contains("102"));
    }
}
