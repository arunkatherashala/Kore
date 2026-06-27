use crate::manifest::{Manifest, Tombstone};
use std::path::Path;
use std::fs::File;
use std::io::{Write, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom};

/// Compaction that applies tombstone row-range deletions for simple line-oriented
/// `.kore` data files. For each file listed in the manifest we read it as text
/// lines (each line = logical row), skip rows covered by tombstones that target
/// that file by row index, and write a compacted file into `out_dir`.
///
/// This is a pragmatic prototype: it supports only row-range tombstones and
/// treats files as newline-delimited rows. It updates `manifest.files` to the
/// new compacted filenames and clears tombstones that were applied.
pub fn compact_manifest(manifest: &Manifest, out_dir: &Path) -> std::io::Result<Manifest> {
    // Delegate to the library block-aware compaction when possible.
    let mut new = manifest.clone();
    new.parent_commit_id = Some(manifest.commit_id.clone());

    // collect simple (file,row_start,row_end) ranges
    let mut ranges: Vec<(String,u64,u64)> = Vec::new();
    let mut preds: Vec<(String,String)> = Vec::new();
    for t in &manifest.tombstones {
        if let Some((s,e)) = t.row_id_range { ranges.push((t.file_path.clone(), s, e)); }
        if let Some(p) = &t.predicate { preds.push((t.file_path.clone(), p.clone())); }
    }

    // Fallback: apply predicates locally (substring evaluator) and perform compaction
    let mut new_files: Vec<String> = Vec::new();
    for (i, fpath) in manifest.files.iter().enumerate() {
        let commit_id_short = if manifest.commit_id.len() >= 8 {
            &manifest.commit_id[..8]
        } else {
            &manifest.commit_id
        };
        let compact_name = format!("compacted-{}-{}.kore", i, commit_id_short);
        let out_path = out_dir.join(&compact_name);
        let original_path = std::path::Path::new(fpath);
        if original_path.exists() {
            let mut ranges_for_file: Vec<(u64,u64)> = ranges.iter().filter(|r| &r.0 == fpath).map(|(_,s,e)| (*s,*e)).collect();
            ranges_for_file.sort_unstable();
            let mut merged: Vec<(u64,u64)> = Vec::new();
            for r in ranges_for_file { if let Some(last)=merged.last_mut() { if r.0<=last.1+1 { if r.1>last.1 { last.1=r.1; } continue; } } merged.push(r); }
            let rf = File::open(&original_path)?;
            let mut br = BufReader::new(rf);
            let cur = br.seek(std::io::SeekFrom::Current(0))?;
            let mut peek = [0u8;4];
            if br.read_exact(&mut peek).is_ok() && &peek==b"KORB" {
                br.seek(std::io::SeekFrom::Start(cur))?;
                let inner = br.into_inner();
                let mut dec = crate::block_stream_decoder::BlockDecoder::new(inner)?;
                let wf = File::create(&out_path)?;
                let mut writer = BufWriter::new(wf);
                let mut row_idx: u64=0;
                dec.decode_all(|line| {
                    let mut deleted=false;
                    for (s,e) in &merged { if row_idx>=*s && row_idx<=*e { deleted=true; break; } }
                    if !deleted {
                        // predicate checks (substring fallback)
                        for (pf, pred) in &preds { if pf==fpath && line.contains(pred) { deleted=true; break; } }
                    }
                    if !deleted { let _ = writeln!(writer, "{}", line); }
                    row_idx+=1;
                })?;
                writer.flush()?;
            } else {
                br.seek(std::io::SeekFrom::Start(cur))?;
                let wf = File::create(&out_path)?;
                let mut writer = BufWriter::new(wf);
                for (idx, line_res) in br.lines().enumerate() {
                    let line = line_res?;
                    let row = idx as u64;
                    let mut deleted=false;
                    for (s,e) in &merged { if row>=*s && row<=*e { deleted=true; break; } }
                    if !deleted { for (pf, pred) in &preds { if pf==fpath && line.contains(pred) { deleted=true; break; } } }
                    if !deleted { writeln!(writer, "{}", line)?; }
                }
                writer.flush()?;
            }
        } else {
            let mut fh = File::create(&out_path)?;
            write!(fh, "compacted placeholder for missing {}\n", fpath)?;
            fh.sync_all()?;
        }
        new_files.push(compact_name);
    }
    new.files = new_files;
    // remove applied row-range tombstones
    new.tombstones.retain(|t| t.row_id_range.is_none());
    Ok(new)
}
