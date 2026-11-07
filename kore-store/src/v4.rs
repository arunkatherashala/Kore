//! Version-4 container ("compact"): opt-in. Version 3 plus timestamp and token transforms, a stronger
//! zstd setting and shared dictionaries that live in separate, checksummed objects (see `dict`).
//!
//! ```text
//! "KORE" | version:u16 = 4 | num_cols:u32 | num_rows:u64
//! num_dicts:u16, then per dictionary:  dict_id:[32] (sha256 of the KDIC object) | content_len:u32
//! directory, one entry per column:
//!   name_len:u16 | name | dtype:u8 | transform:u8 | stage:u8 | dict:u8 | payload_len:u64 | payload_crc32:u32
//! dir_crc32:u32          CRC-32 of every byte from the start of the file up to here
//! payloads               concatenated in directory order
//! ```
//! `dict` is 0xFF (none) or an index into the dictionary list; it is only allowed with stage 4.
//! Stage ids: 0 none, 1 lz4, 2 hz, 3 zstd (any level), 4 zstd with the referenced shared dictionary
//! (payload `varint(raw_len)` + one zstd frame with content checksum).
//! Transform ids: those of version 3 plus 5 `TsText` and 6 `Tokens`.
//!
//! Checksums: the header, the dictionary ids and the directory are covered by `dir_crc32`, each payload by
//! its CRC, the dictionary object by its sha256 (and the id in this header pins the exact object), and
//! the file must end exactly after the last payload. Readers of version 1/2/3 stop at the version field
//! ("unsupported version 4").

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use crate::codec::{self, ColumnCodec, Stage, Transform, WriteOptions};
use crate::dict::{DictSet, SharedDict};
use crate::{compress, DType, MAGIC};

pub const VERSION_V4: u16 = 4;
pub const NO_DICT: u8 = 0xFF;
pub const STAGE_ZSTD_DICT: u8 = 4;

/// Payloads below this many raw bytes are zstd-coded at level 19 (cheap at that size); larger ones at
/// `CompactOptions::zstd_level`.
const SMALL: usize = 256 << 10;
/// Hz is only tried below this raw size (it is about 10x slower than zstd level 3 on large text).
const HZ_MAX: usize = 128 << 10;
const MAX_ROWS: usize = 1 << 28;
/// Below this many kept payload bytes a file is decoded on the calling thread.
const PAR_MIN_BYTES: usize = 64 * 1024;
const MIN_ENTRY: usize = 2 + 1 + 1 + 1 + 1 + 8 + 4;

fn bad(m: impl Into<String>) -> KoreError { KoreError::InvalidArgument(m.into()) }

/// Options for `KoreWriter::to_bytes_compact`.
#[derive(Debug, Clone)]
pub struct CompactOptions {
    /// Per-column transform/stage requests (default: `Auto` for both). `Stage::Zstd` on a column that has
    /// a dictionary means "zstd with that dictionary".
    pub codecs: WriteOptions,
    /// Shared dictionaries by column name. A column with a dictionary may use it (always under `Auto`
    /// if it is smaller); the file then references the dictionary by id.
    pub dicts: Vec<(String, SharedDict)>,
    /// zstd level for payloads of 256 KiB and more (smaller payloads use 19). Default 9.
    pub zstd_level: i32,
}

impl Default for CompactOptions {
    fn default() -> Self { CompactOptions { codecs: WriteOptions::auto(), dicts: vec![], zstd_level: 9 } }
}

impl CompactOptions {
    pub fn new() -> Self { Self::default() }
    pub fn with_dict(mut self, column: &str, d: SharedDict) -> Self {
        self.dicts.retain(|(n, _)| n != column);
        self.dicts.push((column.to_string(), d));
        self
    }
    pub fn with_column(mut self, name: &str, c: ColumnCodec) -> Self { self.codecs = self.codecs.with_column(name, c); self }
    pub fn with_zstd_level(mut self, level: i32) -> Self { self.zstd_level = level; self }
    fn dict_for(&self, name: &str) -> Option<&SharedDict> { self.dicts.iter().find(|(n, _)| n == name).map(|(_, d)| d) }
}

/// True if `data` starts with the KORE magic and format version 4.
pub fn is_v4(data: &[u8]) -> bool {
    data.len() >= 6 && &data[..4] == MAGIC && u16::from_le_bytes([data[4], data[5]]) == VERSION_V4
}

fn candidate_transforms(data: &ColumnData) -> Vec<Transform> {
    let mut c = match data {
        ColumnData::Int64(_) => vec![Transform::Plain, Transform::Delta, Transform::Varint],
        ColumnData::Str(v) => {
            let mut c = vec![Transform::Plain];
            if v.len() >= 2 && dict_probe(v, v.len() / 2) { c.push(Transform::Dict); }
            if codec::hex_width(v).map_or(false, |w| w > 0) { c.push(Transform::HexFixed); }
            c
        }
        _ => vec![Transform::Plain],
    };
    if let ColumnData::Str(v) = data {
        if v.iter().flatten().next().is_some() && v.iter().flatten().all(|s| codec::parse_ts(s).is_some()) { c.push(Transform::TsText); }
        if v.iter().flatten().any(|s| s.contains(',')) && tokens_probe(v, v.len() / 2 + 16) { c.push(Transform::Tokens); }
    }
    c
}

fn dict_probe(v: &[Option<String>], cap: usize) -> bool {
    let mut seen = std::collections::HashSet::new();
    for s in v.iter().flatten() { if seen.insert(s.as_str()) && seen.len() > cap { return false; } }
    true
}

fn tokens_probe(v: &[Option<String>], cap: usize) -> bool {
    let cap = cap.min(65_536);
    let mut seen = std::collections::HashSet::new();
    for s in v.iter().flatten() {
        if s.is_empty() { continue; }
        for t in s.split(',') { if seen.insert(t) && seen.len() > cap { return false; } }
    }
    true
}

/// (stage id, payload) for one concrete stage. `None` stage returns None (caller keeps the raw bytes).
fn stage_bytes(raw: &[u8], stage: u8, level: i32, dict: Option<&SharedDict>) -> Vec<u8> {
    let mut out = Vec::new();
    match stage {
        1 => return codec::encode_stage(raw, Stage::Lz4),
        2 => return codec::encode_stage(raw, Stage::Hz),
        3 => {
            let lvl = if raw.len() < SMALL { 19 } else { level };
            codec::put_varint(&mut out, raw.len() as u64);
            out.extend_from_slice(&zstd::bulk::compress(raw, lvl).expect("zstd in-memory compress"));
        }
        STAGE_ZSTD_DICT => {
            let lvl = if raw.len() < SMALL { 19 } else { level };
            codec::put_varint(&mut out, raw.len() as u64);
            out.extend_from_slice(&dict.expect("dictionary").compress(raw, lvl));
        }
        _ => unreachable!(),
    }
    out
}

/// Encode one column: `(transform id, stage id, uses_dict, payload)`.
fn encode_column(data: &ColumnData, codec: ColumnCodec, dict: Option<&SharedDict>, level: i32) -> Result<(u8, u8, bool, Vec<u8>), KoreError> {
    use rayon::prelude::*;
    let transforms = match codec.transform { Transform::Auto => candidate_transforms(data), t => vec![t] };
    // Two phases (cost): with several transforms, each is scored by min(raw, zstd level 1) and only the
    // best scoring one goes through the stage search. Ties keep the earlier candidate.
    let transforms: Vec<(Transform, Vec<u8>)> = if transforms.len() > 1 {
        let scored: Result<Vec<(Transform, Vec<u8>, usize)>, KoreError> = transforms.into_par_iter().map(|t| {
            let raw = codec::encode_transform(data, t)?;
            let score = if raw.len() < 64 { raw.len() } else { raw.len().min(zstd::bulk::compress(&raw, 1).expect("zstd").len() + 4) };
            Ok((t, raw, score))
        }).collect();
        let scored = scored?;
        let mut win = 0;
        for i in 1..scored.len() { if scored[i].2 < scored[win].2 { win = i; } }
        let (t, raw, _) = scored.into_iter().nth(win).unwrap();
        vec![(t, raw)]
    } else {
        let t = transforms[0];
        vec![(t, codec::encode_transform(data, t)?)]
    };
    let mut best: Option<(u8, u8, bool, Vec<u8>)> = None;
    for (t, raw) in transforms {
        let stages: Vec<u8> = match codec.stage {
            Stage::Auto => {
                // with a dictionary, plain zstd is only tried on small payloads (on large ones it costs as
                // much as the dictionary variant and practically never wins)
                let mut s = vec![0u8, 1];
                if dict.is_none() || raw.len() < SMALL { s.push(3); }
                if raw.len() < HZ_MAX { s.push(2); }
                if dict.is_some() { s.push(STAGE_ZSTD_DICT); }
                s
            }
            Stage::Zstd if dict.is_some() => vec![STAGE_ZSTD_DICT],
            s => vec![s.id().expect("concrete stage")],
        };
        let staged: Vec<(u8, Option<Vec<u8>>)> = stages.par_iter()
            .map(|&s| (s, if s == 0 { None } else { Some(stage_bytes(&raw, s, level, dict)) })).collect();
        let size_of = |c: &(u8, Option<Vec<u8>>)| c.1.as_ref().map_or(raw.len(), |p| p.len());
        let mut win = 0;
        for i in 1..staged.len() { if size_of(&staged[i]) < size_of(&staged[win]) { win = i; } }
        if best.as_ref().map_or(true, |b| size_of(&staged[win]) < b.3.len()) {
            let (s, p) = staged.into_iter().nth(win).unwrap();
            best = Some((t.id().unwrap(), s, s == STAGE_ZSTD_DICT, p.unwrap_or(raw)));
        }
    }
    best.ok_or_else(|| bad("no codec candidate"))
}

pub fn write(block: &DataBlock, opts: &CompactOptions) -> Result<Vec<u8>, KoreError> {
    use rayon::prelude::*;
    for col in &block.columns {
        if col.data.len() != block.num_rows {
            return Err(bad(format!("column '{}' has {} values, block has {} rows", col.name, col.data.len(), block.num_rows)));
        }
        if col.name.len() > u16::MAX as usize { return Err(bad("column name too long")); }
    }
    for (name, _) in opts.codecs.columns.iter().map(|(n, c)| (n, c)).chain(opts.dicts.iter().map(|(n, _)| (n, &ColumnCodec::AUTO))) {
        if !block.columns.iter().any(|c| &c.name == name) { return Err(bad(format!("option given for unknown column '{name}'"))); }
    }
    let encoded: Result<Vec<(u8, u8, bool, Vec<u8>)>, KoreError> = block.columns.par_iter()
        .map(|c| encode_column(&c.data, opts.codecs.codec_for(&c.name), opts.dict_for(&c.name), opts.zstd_level))
        .collect();
    let encoded = encoded?;

    // dictionaries actually used, in column order
    let mut used: Vec<SharedDict> = vec![];
    let mut dict_idx = vec![NO_DICT; block.columns.len()];
    for (i, (col, e)) in block.columns.iter().zip(&encoded).enumerate() {
        if !e.2 { continue; }
        let d = opts.dict_for(&col.name).expect("dictionary");
        let k = match used.iter().position(|u| u.id() == d.id()) { Some(k) => k, None => { used.push(d.clone()); used.len() - 1 } };
        if k >= NO_DICT as usize { return Err(bad("too many dictionaries in one file")); }
        dict_idx[i] = k as u8;
    }

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION_V4.to_le_bytes());
    out.extend_from_slice(&(block.columns.len() as u32).to_le_bytes());
    out.extend_from_slice(&(block.num_rows as u64).to_le_bytes());
    out.extend_from_slice(&(used.len() as u16).to_le_bytes());
    for d in &used {
        out.extend_from_slice(&d.id());
        out.extend_from_slice(&(d.content().len() as u32).to_le_bytes());
    }
    for (i, (col, (t, s, _, payload))) in block.columns.iter().zip(&encoded).enumerate() {
        out.extend_from_slice(&(col.name.len() as u16).to_le_bytes());
        out.extend_from_slice(col.name.as_bytes());
        out.push(codec::dtype_of(&col.data) as u8);
        out.push(*t);
        out.push(*s);
        out.push(dict_idx[i]);
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&compress::crc32(payload).to_le_bytes());
    }
    let dir_crc = compress::crc32(&out);
    out.extend_from_slice(&dir_crc.to_le_bytes());
    for (_, _, _, payload) in &encoded { out.extend_from_slice(payload); }
    Ok(out)
}

/// Header facts of a version-4 file (directory checksum verified, payloads not read).
#[derive(Debug, Clone)]
pub struct Info {
    pub num_rows: usize,
    /// `(column name, dtype, transform id, stage id, dictionary id if any, payload bytes)`
    pub columns: Vec<(String, DType, u8, u8, Option<[u8; 32]>, usize)>,
    /// ids of the dictionaries the file references (needed to read the columns that use them)
    pub dict_ids: Vec<[u8; 32]>,
}

struct Raw { name: String, dtype: DType, transform: u8, stage: u8, dict: u8, len: u64, crc: u32 }

fn take<'a>(d: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], KoreError> {
    let end = pos.checked_add(n).filter(|&e| e <= d.len()).ok_or_else(|| bad("truncated .kore data"))?;
    let s = &d[*pos..end];
    *pos = end;
    Ok(s)
}

/// Parse and authenticate the header + directory. Returns (rows, dict list, directory, payload start).
fn parse_header(data: &[u8]) -> Result<(usize, Vec<([u8; 32], u32)>, Vec<Raw>, usize), KoreError> {
    let mut pos = 0usize;
    if take(data, &mut pos, 4)? != MAGIC { return Err(bad("invalid KORE magic bytes")); }
    let version = u16::from_le_bytes(take(data, &mut pos, 2)?.try_into().unwrap());
    if version != VERSION_V4 { return Err(bad(format!("unsupported version {version}"))); }
    let num_cols = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap()) as usize;
    let num_rows = u64::from_le_bytes(take(data, &mut pos, 8)?.try_into().unwrap());
    let nd = u16::from_le_bytes(take(data, &mut pos, 2)?.try_into().unwrap()) as usize;
    if nd > (data.len() - pos) / 36 { return Err(bad("implausible dictionary count")); }
    let mut dicts = Vec::with_capacity(nd);
    for _ in 0..nd {
        let id: [u8; 32] = take(data, &mut pos, 32)?.try_into().unwrap();
        let len = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap());
        dicts.push((id, len));
    }
    if num_cols > (data.len() - pos) / MIN_ENTRY { return Err(bad("implausible column count")); }
    let mut dir = Vec::with_capacity(num_cols);
    for _ in 0..num_cols {
        let name_len = u16::from_le_bytes(take(data, &mut pos, 2)?.try_into().unwrap()) as usize;
        let name = String::from_utf8(take(data, &mut pos, name_len)?.to_vec()).map_err(|_| bad("invalid UTF-8 column name"))?;
        let hdr = take(data, &mut pos, 4)?;
        let dtype = DType::try_from(hdr[0])?;
        let len = u64::from_le_bytes(take(data, &mut pos, 8)?.try_into().unwrap());
        let crc = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap());
        dir.push(Raw { name, dtype, transform: hdr[1], stage: hdr[2], dict: hdr[3], len, crc });
    }
    let covered = pos;
    let dir_crc = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap());
    if compress::crc32(&data[..covered]) != dir_crc { return Err(bad("checksum mismatch in file header/directory")); }
    // authenticated from here on; still validate the semantics
    let num_rows = usize::try_from(num_rows).ok().filter(|&r| r <= MAX_ROWS).ok_or_else(|| bad("implausible row count"))?;
    let mut referenced = vec![false; nd];
    for r in &dir {
        Transform::from_id_v4(r.transform)?;
        match (r.stage, r.dict) {
            (0..=3, NO_DICT) => {}
            (STAGE_ZSTD_DICT, k) if (k as usize) < nd => referenced[k as usize] = true,
            (STAGE_ZSTD_DICT, _) => return Err(bad("dictionary index out of range")),
            (0..=3, _) => return Err(bad("dictionary given for a stage that does not use one")),
            (s, _) => return Err(bad(format!("unknown stage {s}"))),
        }
    }
    if referenced.iter().any(|r| !r) { return Err(bad("dictionary listed but not used")); }
    Ok((num_rows, dicts, dir, pos))
}

/// Header facts without decoding any payload (and without needing the dictionaries).
pub fn info(data: &[u8]) -> Result<Info, KoreError> {
    let (num_rows, dicts, dir, _) = parse_header(data)?;
    Ok(Info {
        num_rows,
        dict_ids: dicts.iter().map(|d| d.0).collect(),
        columns: dir.iter().map(|r| (r.name.clone(), r.dtype, r.transform, r.stage,
            if r.dict == NO_DICT { None } else { Some(dicts[r.dict as usize].0) }, r.len as usize)).collect(),
    })
}

pub fn read(data: &[u8], wanted: Option<&[&str]>, dicts: &DictSet) -> Result<DataBlock, KoreError> {
    match std::panic::catch_unwind(|| read_inner(data, wanted, dicts)) {
        Ok(r) => r,
        Err(_) => Err(bad("corrupt .kore data")),
    }
}

fn decode_payload(payload: &[u8], dtype: DType, transform: u8, stage: u8, dict: Option<&SharedDict>, n: usize) -> Result<ColumnData, KoreError> {
    let t = Transform::from_id_v4(transform)?;
    if stage == STAGE_ZSTD_DICT {
        let mut pos = 0;
        let raw_len = usize::try_from(codec::get_varint(payload, &mut pos)?).map_err(|_| bad("overflow"))?;
        if raw_len > 1 << 31 { return Err(bad("implausible decoded size")); }
        let raw = dict.expect("resolved").decompress(&payload[pos..], raw_len)?;
        if raw.len() != raw_len { return Err(bad("decoded stage length mismatch")); }
        return codec::decode_transform(&raw, dtype, t, n);
    }
    let s = Stage::from_id(stage)?;
    let raw = codec::decode_stage(payload, s)?;
    codec::decode_transform(&raw, dtype, t, n)
}

fn read_inner(data: &[u8], wanted: Option<&[&str]>, dicts: &DictSet) -> Result<DataBlock, KoreError> {
    let (num_rows, dict_list, dir, mut pos) = parse_header(data)?;
    struct Entry<'a> { name: String, dtype: DType, transform: u8, stage: u8, dict: u8, crc: u32, payload: &'a [u8] }
    let mut entries: Vec<Entry> = Vec::with_capacity(dir.len());
    for r in dir {
        let len = usize::try_from(r.len).map_err(|_| bad("column length overflow"))?;
        let payload = take(data, &mut pos, len)?;
        entries.push(Entry { name: r.name, dtype: r.dtype, transform: r.transform, stage: r.stage, dict: r.dict, crc: r.crc, payload });
    }
    if pos != data.len() { return Err(bad("trailing bytes after last column")); }
    if let Some(w) = wanted {
        for name in w {
            if !entries.iter().any(|c| c.name == *name) { return Err(bad(format!("column not found: {name}"))); }
        }
    }
    let keep = |name: &str| wanted.map_or(true, |w| w.iter().any(|x| *x == name));
    // resolve the dictionaries of the kept columns only (a projection that avoids them needs none)
    let mut resolved: Vec<Option<&SharedDict>> = vec![None; dict_list.len()];
    for e in entries.iter().filter(|e| keep(&e.name) && e.dict != NO_DICT) {
        let (id, len) = dict_list[e.dict as usize];
        let d = dicts.get(&id).ok_or_else(|| bad(format!(
            "column '{}' needs shared dictionary {} (not supplied; use KoreReader::from_bytes_with_dicts)", e.name, crate::dict::hex(&id))))?;
        if d.content().len() != len as usize { return Err(bad("dictionary length does not match the file")); }
        resolved[e.dict as usize] = Some(d);
    }

    use rayon::prelude::*;
    let kept: Vec<Entry> = entries.into_iter().filter(|e| keep(&e.name)).collect();
    let kept_bytes: usize = kept.iter().map(|e| e.payload.len()).sum();
    let decode = |e: Entry| -> Result<Column, KoreError> {
        if compress::crc32(e.payload) != e.crc { return Err(bad(format!("checksum mismatch in column '{}'", e.name))); }
        let d = if e.dict == NO_DICT { None } else { resolved[e.dict as usize] };
        let data = decode_payload(e.payload, e.dtype, e.transform, e.stage, d, num_rows)
            .map_err(|er| bad(format!("column '{}': {er}", e.name)))?;
        if data.len() != num_rows { return Err(bad(format!("column '{}' has {} values, expected {num_rows}", e.name, data.len()))); }
        Ok(Column { name: e.name, data })
    };
    // one column or a small file: decode on the calling thread (the rayon hand-off costs more than it
    // saves, and much more on a loaded machine); same rule as the version-2 reader
    let cols: Result<Vec<Column>, KoreError> = if kept.len() <= 1 || kept_bytes < PAR_MIN_BYTES {
        kept.into_iter().map(decode).collect()
    } else {
        kept.into_par_iter().map(decode).collect()
    };
    let mut columns = cols?;
    if let Some(w) = wanted {
        let mut ordered: Vec<Column> = Vec::with_capacity(w.len());
        for name in w {
            if ordered.iter().any(|c| c.name == *name) { continue; }
            if let Some(p) = columns.iter().position(|c| c.name == *name) { ordered.push(columns.swap_remove(p)); }
        }
        columns = ordered;
    }
    Ok(DataBlock { columns, num_rows })
}
