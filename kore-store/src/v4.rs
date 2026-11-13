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
//!
//! Additions (all opt-in or byte-identical, docs/kore_speed_v4.md section 6): `read_rows` / `read_row`
//! (row-level partial decode, same checks as `read`); a pruned writer search that writes the bytes of
//! the earlier search (`write_reference`) with less work; `CompactOptions::quick()` (explicit, other
//! bytes, still version 4); container version 5 (`CompactOptions::with_compact_directory`): version 4
//! with a varint header and directory, refused by older readers at the version field.

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
    /// zstd level for payloads of 256 KiB and more (smaller payloads use `small_zstd_level`). Default 9.
    pub zstd_level: i32,
    /// zstd level for payloads below 256 KiB. Default 19, the setting every earlier version-4 file was
    /// written with (same options, same bytes). A lower level is an explicit speed/size trade-off
    /// (`CompactOptions::fast`): the result is an ordinary version-4 file, but its bytes differ.
    pub small_zstd_level: i32,
    /// How many stage candidates are tried under `Stage::Auto`. Default `Full` (the bytes of every
    /// earlier version-4 writer); `Quick` is an explicit speed/size trade-off (`CompactOptions::quick`).
    pub search: SearchMode,
    /// Write container version 5: the same columns, codecs and payloads as version 4 with a smaller
    /// (varint) header and directory. Default false (version 4). Explicit: readers that predate it
    /// (b44d2a91 and older) refuse such a file with "unsupported version 5".
    pub compact_directory: bool,
}

/// Stage search of the version-4 writer (see `CompactOptions::search`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Every applicable stage (none, lz4, zstd, hz, zstd with the dictionary); the smallest wins.
    Full,
    /// Only none, zstd and zstd with the dictionary; zstd is skipped for bytes that zstd level 1 could
    /// not shrink (unless the column has a dictionary). Other bytes than `Full`, same format.
    Quick,
}

impl Default for CompactOptions {
    fn default() -> Self {
        CompactOptions { codecs: WriteOptions::auto(), dicts: vec![], zstd_level: 9, small_zstd_level: 19, search: SearchMode::Full, compact_directory: false }
    }
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
    pub fn with_small_zstd_level(mut self, level: i32) -> Self { self.small_zstd_level = level; self }
    /// Faster writer setting (explicit; changes the bytes, not the format): payloads below 256 KiB are
    /// coded at level 9 instead of 19. Size and speed: docs/kore_speed_v4.md.
    pub fn fast() -> Self { Self::default().with_small_zstd_level(9) }
    pub fn with_search(mut self, s: SearchMode) -> Self { self.search = s; self }
    /// Write container version 5 (smaller directory; see `compact_directory`).
    pub fn with_compact_directory(mut self, on: bool) -> Self { self.compact_directory = on; self }
    /// Cheapest preset for small commits (explicit; changes the bytes, not the format): `SearchMode::Quick`
    /// with zstd level 3 for every payload. The result is an ordinary version-4 file that every
    /// version-4 reader reads. Size and speed: docs/kore_speed_v4.md section 6.
    pub fn quick() -> Self { Self::default().with_search(SearchMode::Quick).with_small_zstd_level(3).with_zstd_level(3) }
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
fn stage_bytes(raw: &[u8], stage: u8, level: i32, small: i32, dict: Option<&SharedDict>) -> Vec<u8> {
    let mut out = Vec::new();
    match stage {
        1 => return codec::encode_stage(raw, Stage::Lz4),
        2 => return codec::encode_stage(raw, Stage::Hz),
        3 => {
            let lvl = if raw.len() < SMALL { small } else { level };
            codec::put_varint(&mut out, raw.len() as u64);
            out.extend_from_slice(&zstd::bulk::compress(raw, lvl).expect("zstd in-memory compress"));
        }
        STAGE_ZSTD_DICT => {
            let lvl = if raw.len() < SMALL { small } else { level };
            codec::put_varint(&mut out, raw.len() as u64);
            out.extend_from_slice(&dict.expect("dictionary").compress(raw, lvl));
        }
        _ => unreachable!(),
    }
    out
}

// ── i64 transform selection: a second finalist (docs/kore_v4_int_select.md) ────
//
// The transform pre-score (zstd level 1 on the whole column) ranks integer layouts differently from the
// zstd level that is actually written (9 or 19 for large payloads). On real timestamps (NYC taxi, int64
// microseconds) zstd-1 prefers `Varint` (10.1 MB against 12.2 MB for `Delta`), while at level 19 `Delta`
// is 6.2 MB and `Varint` 10.5 MB. So for large i64 columns every transform is also scored on a row sample
// (`SAMPLE_WINDOWS` windows of `SAMPLE_ROWS` rows, spread over the column) with the zstd level its full
// payload would get. If that ranking has another winner, both go through the full stage search and the
// sampled one is kept only if it is strictly smaller. Hence:
// * no column is ever larger than with the zstd-1 choice alone, and when the zstd-1 choice is at least as
//   small the bytes are exactly those of the earlier writer;
// * small columns (every candidate below `INT_SAMPLE_MIN` raw bytes) are untouched;
// * no format change: the written transform ids (Plain, Delta, Varint) are the existing ones.

/// The sampled check runs when some candidate of an i64 column has at least this many raw bytes.
const INT_SAMPLE_MIN: usize = SMALL;
const SAMPLE_WINDOWS: usize = 4;
const SAMPLE_ROWS: usize = 16 << 10;

/// Up to `SAMPLE_WINDOWS` windows of `SAMPLE_ROWS` consecutive rows, evenly spread, first and last
/// included (the whole column if it is not longer).
fn int_sample(v: &[Option<i64>]) -> Vec<Option<i64>> {
    let n = v.len();
    if n <= SAMPLE_WINDOWS * SAMPLE_ROWS { return v.to_vec(); }
    let mut out = Vec::with_capacity(SAMPLE_WINDOWS * SAMPLE_ROWS);
    for k in 0..SAMPLE_WINDOWS {
        let start = k * (n - SAMPLE_ROWS) / (SAMPLE_WINDOWS - 1);
        out.extend_from_slice(&v[start..start + SAMPLE_ROWS]);
    }
    out
}

/// The index of the second finalist of an i64 column under `Transform::Auto`, or None (other types,
/// small columns, an explicit stage other than zstd, or the sample agrees with the zstd-1 winner `win`).
/// `scored` = (transform, full raw bytes, zstd-1 score) per candidate.
fn int_finalist(data: &ColumnData, codec: ColumnCodec, scored: &[(Transform, Vec<u8>, usize)], win: usize, level: i32, small: i32, par: bool) -> Result<Option<usize>, KoreError> {
    use rayon::prelude::*;
    let ColumnData::Int64(v) = data else { return Ok(None) };
    if codec.transform != Transform::Auto || !matches!(codec.stage, Stage::Auto | Stage::Zstd) { return Ok(None); }
    if scored.len() < 2 || scored.iter().all(|c| c.1.len() < INT_SAMPLE_MIN) { return Ok(None); }
    let sample = ColumnData::Int64(int_sample(v));
    let score = |c: &(Transform, Vec<u8>, usize)| -> Result<usize, KoreError> {
        let raw = codec::encode_transform(&sample, c.0)?;
        let lvl = if c.1.len() < SMALL { small } else { level };
        Ok(raw.len().min(zstd::bulk::compress(&raw, lvl).expect("zstd").len()))
    };
    let s: Vec<usize> = if par { scored.par_iter().map(score).collect::<Result<_, _>>()? } else { scored.iter().map(score).collect::<Result<_, _>>()? };
    let mut best = 0;
    for i in 1..s.len() { if s[i] < s[best] { best = i; } }
    Ok(if best != win && s[best] < s[win] { Some(best) } else { None })
}

/// Encode one column: `(transform id, stage id, uses_dict, payload)`.
pub(crate) fn encode_column(data: &ColumnData, codec: ColumnCodec, dict: Option<&SharedDict>, level: i32, small: i32) -> Result<(u8, u8, bool, Vec<u8>), KoreError> {
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
        // second finalist (i64 only, see `int_finalist`): it goes through the stage search after the
        // zstd-1 winner and is kept only if strictly smaller
        let alt = int_finalist(data, codec, &scored, win, level, small, true)?;
        let mut scored: Vec<Option<(Transform, Vec<u8>, usize)>> = scored.into_iter().map(Some).collect();
        let (t, raw, _) = scored[win].take().unwrap();
        let mut v = vec![(t, raw)];
        if let Some(j) = alt { let (t, raw, _) = scored[j].take().unwrap(); v.push((t, raw)); }
        v
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
            .map(|&s| (s, if s == 0 { None } else { Some(stage_bytes(&raw, s, level, small, dict)) })).collect();
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

// ── faster search, same result ───────────────────────────────────────────────
//
// `encode_column_pruned` makes exactly the choice of `encode_column` (the search of bbbefd2c/b44d2a91),
// with less work:
//
// * Stages that cannot win are not computed. A stage wins only with a payload strictly smaller than the
//   current winner, which is at most `raw.len()` (`None` is candidate 0). Every stage payload has a
//   format-given minimum size, so a stage whose minimum is >= `raw.len()` can never be chosen, and
//   leaving it out does not change which candidate is first among the smallest:
//   - zstd (stage 3): `varint(raw_len)` >= 1 byte + frame: magic 4 + frame header descriptor 1 + window
//     descriptor or frame content size >= 1 + block header 3 + block content >= 1 (non-empty input)
//     = 11 bytes; zstd with a dictionary (stage 4) also carries the 4-byte content checksum = 15.
//   - hz (stage 2): `varint(raw_len)` + `varint(segment_len)` + `varint(len)` >= 3 bytes + the two code
//     length tables (286 + 40 entries, each written as a 4-bit length or a 4+4-bit run of at most 16
//     zeros, so >= 286/2 + 40/2 bits) + the last-block bit: >= 164 bits = 21 bytes; 24 in total.
//   The constants below stay under these bounds (`tests/v4_rows.rs` checks them on many inputs).
//   An empty `raw` is never beaten (no payload is shorter than 0 bytes), so nothing is computed.
// * Small files are encoded on the calling thread: the candidates and the selection are the same, only
//   the rayon hand-offs (which dominate below a few KB, and much more on a loaded machine) are gone.
//   hz below its segment size is one `hz::compress` call framed exactly as `hz::compress_segmented`.

const ZSTD_MIN_PAYLOAD: usize = 10;
const ZSTD_DICT_MIN_PAYLOAD: usize = 14;
const HZ_MIN_PAYLOAD: usize = 20;
/// Below this many estimated raw bytes (sum over columns) a file is encoded on the calling thread.
const PAR_ENCODE_MIN: usize = 32 << 10;

/// Lower bounds used by the pruning (for tests).
#[doc(hidden)]
pub const STAGE_MIN_PAYLOAD: [(u8, usize); 3] = [(3, ZSTD_MIN_PAYLOAD), (STAGE_ZSTD_DICT, ZSTD_DICT_MIN_PAYLOAD), (2, HZ_MIN_PAYLOAD)];

/// True if `stage` can produce a payload shorter than `raw_len` (see above).
fn may_win(stage: u8, raw_len: usize) -> bool {
    match stage {
        0 => true,
        3 => raw_len > ZSTD_MIN_PAYLOAD,
        STAGE_ZSTD_DICT => raw_len > ZSTD_DICT_MIN_PAYLOAD,
        2 => raw_len > HZ_MIN_PAYLOAD,
        _ => raw_len > 0,
    }
}

/// `hz::compress_segmented` without rayon (same bytes).
fn hz_segmented_here(raw: &[u8]) -> Vec<u8> {
    if raw.len() > crate::hz::SEGMENT { return crate::hz::compress_segmented(raw); }
    let mut out = Vec::new();
    if raw.is_empty() { return out; }
    let p = crate::hz::compress(raw);
    codec::put_varint(&mut out, p.len() as u64);
    out.extend_from_slice(&p);
    out
}

/// `stage_bytes` (same bytes); `here` = no rayon inside.
fn stage_bytes_here(raw: &[u8], stage: u8, level: i32, small: i32, dict: Option<&SharedDict>, here: bool) -> Vec<u8> {
    if stage == 2 && here {
        let mut out = Vec::new();
        codec::put_varint(&mut out, raw.len() as u64);
        out.extend_from_slice(&hz_segmented_here(raw));
        return out;
    }
    stage_bytes(raw, stage, level, small, dict)
}

/// Stage payload exactly as the writer would store it (for tests of the lower bounds).
#[doc(hidden)]
pub fn stage_payload(raw: &[u8], stage: u8, level: i32, dict: Option<&SharedDict>) -> Vec<u8> {
    stage_bytes(raw, stage, level, level, dict)
}

/// Rough raw size of a column (decides only where the work runs, never what is written).
fn approx_bytes(data: &ColumnData) -> usize {
    match data {
        ColumnData::Str(v) => v.iter().map(|s| s.as_ref().map_or(0, |s| s.len()) + 1).sum(),
        ColumnData::StrDict { codes, dict } => codes.len() + dict.iter().map(|s| s.len() + 2).sum::<usize>(),
        other => other.len() * 8,
    }
}

/// Same result as `encode_column`; `par` = use rayon for the candidates.
fn encode_column_pruned(data: &ColumnData, codec: ColumnCodec, dict: Option<&SharedDict>, level: i32, small: i32, par: bool) -> Result<(u8, u8, bool, Vec<u8>), KoreError> {
    use rayon::prelude::*;
    let transforms = match codec.transform { Transform::Auto => candidate_transforms(data), t => vec![t] };
    let score = |t: Transform| -> Result<(Transform, Vec<u8>, usize), KoreError> {
        let raw = codec::encode_transform(data, t)?;
        let score = if raw.len() < 64 { raw.len() } else { raw.len().min(zstd::bulk::compress(&raw, 1).expect("zstd").len() + 4) };
        Ok((t, raw, score))
    };
    let (t, raw, alt) = if transforms.len() > 1 {
        let scored: Vec<(Transform, Vec<u8>, usize)> = if par {
            transforms.into_par_iter().map(score).collect::<Result<_, _>>()?
        } else {
            transforms.into_iter().map(score).collect::<Result<_, _>>()?
        };
        let mut win = 0;
        for i in 1..scored.len() { if scored[i].2 < scored[win].2 { win = i; } }
        let alt = int_finalist(data, codec, &scored, win, level, small, par)?;
        let mut scored: Vec<Option<(Transform, Vec<u8>, usize)>> = scored.into_iter().map(Some).collect();
        let (t, raw, _) = scored[win].take().unwrap();
        let alt = alt.map(|j| { let (t, raw, _) = scored[j].take().unwrap(); (t, raw) });
        (t, raw, alt)
    } else {
        let t = transforms[0];
        (t, codec::encode_transform(data, t)?, None)
    };
    let Some((t2, raw2)) = alt else { return stage_search_pruned(t, raw, codec, dict, level, small, par) };
    // two finalists: the zstd-1 winner, then the sampled one; the second is kept only if strictly smaller
    // (the order and the comparison of `encode_column`)
    let (a, b) = if par {
        rayon::join(|| stage_search_pruned(t, raw, codec, dict, level, small, par), || stage_search_pruned(t2, raw2, codec, dict, level, small, par))
    } else {
        (stage_search_pruned(t, raw, codec, dict, level, small, par), stage_search_pruned(t2, raw2, codec, dict, level, small, par))
    };
    let (a, b) = (a?, b?);
    Ok(if b.3.len() < a.3.len() { b } else { a })
}

/// The stage search of `encode_column_pruned` for one transform (same choice as `encode_column`).
fn stage_search_pruned(t: Transform, raw: Vec<u8>, codec: ColumnCodec, dict: Option<&SharedDict>, level: i32, small: i32, par: bool) -> Result<(u8, u8, bool, Vec<u8>), KoreError> {
    use rayon::prelude::*;
    let stages: Vec<u8> = match codec.stage {
        Stage::Auto => {
            let mut s = vec![0u8, 1];
            if dict.is_none() || raw.len() < SMALL { s.push(3); }
            if raw.len() < HZ_MAX { s.push(2); }
            if dict.is_some() { s.push(STAGE_ZSTD_DICT); }
            // pruning only under Auto: an explicit stage is always applied
            s.retain(|&st| may_win(st, raw.len()));
            s
        }
        Stage::Zstd if dict.is_some() => vec![STAGE_ZSTD_DICT],
        s => vec![s.id().expect("concrete stage")],
    };
    let run = |s: u8| (s, if s == 0 { None } else { Some(stage_bytes_here(&raw, s, level, small, dict, !par)) });
    let staged: Vec<(u8, Option<Vec<u8>>)> = if par && stages.len() > 1 { stages.par_iter().map(|&s| run(s)).collect() } else { stages.iter().map(|&s| run(s)).collect() };
    let size_of = |c: &(u8, Option<Vec<u8>>)| c.1.as_ref().map_or(raw.len(), |p| p.len());
    let mut win = 0;
    for i in 1..staged.len() { if size_of(&staged[i]) < size_of(&staged[win]) { win = i; } }
    let (s, p) = staged.into_iter().nth(win).unwrap();
    Ok((t.id().unwrap(), s, s == STAGE_ZSTD_DICT, p.unwrap_or(raw)))
}

/// The version-4 writer as it was at b44d2a91 (every candidate computed, always with rayon). Same bytes
/// as `write`; kept as the reference for tests and for before/after timings.
#[doc(hidden)]
pub fn write_reference(block: &DataBlock, opts: &CompactOptions) -> Result<Vec<u8>, KoreError> {
    write_impl(block, opts, Search::Reference)
}

/// `write` with the place of the work forced (`true` = rayon, `false` = calling thread). Same bytes.
#[doc(hidden)]
pub fn write_threads(block: &DataBlock, opts: &CompactOptions, parallel: bool) -> Result<Vec<u8>, KoreError> {
    write_impl(block, opts, if parallel { Search::Parallel } else { Search::Here })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Search { Reference, Parallel, Here, Auto }

/// `SearchMode::Quick` (explicit, other bytes): the transform is chosen as by the full search (zstd-1
/// score), then only `None`, zstd and zstd with the column's dictionary are tried (no lz4, no hz), and
/// zstd is not tried at all when the zstd-1 score already found the bytes incompressible and the column
/// has no dictionary. Every result is an ordinary version-4 file.
fn encode_column_quick(data: &ColumnData, codec: ColumnCodec, dict: Option<&SharedDict>, level: i32, small: i32, par: bool) -> Result<(u8, u8, bool, Vec<u8>), KoreError> {
    use rayon::prelude::*;
    if codec.stage != Stage::Auto { return encode_column_pruned(data, codec, dict, level, small, par); }
    let transforms = match codec.transform { Transform::Auto => candidate_transforms(data), t => vec![t] };
    let score = |t: Transform| -> Result<(Transform, Vec<u8>, Option<usize>), KoreError> {
        let raw = codec::encode_transform(data, t)?;
        let z = if raw.len() < 64 { None } else { Some(zstd::bulk::compress(&raw, 1).expect("zstd").len() + 4) };
        Ok((t, raw, z))
    };
    let mut scored: Vec<(Transform, Vec<u8>, Option<usize>)> = if transforms.len() > 1 {
        if par { transforms.into_par_iter().map(score).collect::<Result<_, _>>()? } else { transforms.into_iter().map(score).collect::<Result<_, _>>()? }
    } else {
        let t = transforms[0];
        vec![(t, codec::encode_transform(data, t)?, None)]
    };
    let key = |c: &(Transform, Vec<u8>, Option<usize>)| c.2.map_or(c.1.len(), |z| z.min(c.1.len()));
    let mut win = 0;
    for i in 1..scored.len() { if key(&scored[i]) < key(&scored[win]) { win = i; } }
    let (t, raw, z1) = scored.swap_remove(win);
    let incompressible = z1.map_or(false, |z| z >= raw.len());
    let mut stages = vec![0u8];
    if !incompressible && (dict.is_none() || raw.len() < SMALL) && may_win(3, raw.len()) { stages.push(3); }
    if dict.is_some() && may_win(STAGE_ZSTD_DICT, raw.len()) { stages.push(STAGE_ZSTD_DICT); }
    let run = |s: u8| (s, if s == 0 { None } else { Some(stage_bytes(&raw, s, level, small, dict)) });
    let staged: Vec<(u8, Option<Vec<u8>>)> = if par && stages.len() > 1 { stages.par_iter().map(|&s| run(s)).collect() } else { stages.iter().map(|&s| run(s)).collect() };
    let size_of = |c: &(u8, Option<Vec<u8>>)| c.1.as_ref().map_or(raw.len(), |p| p.len());
    let mut win = 0;
    for i in 1..staged.len() { if size_of(&staged[i]) < size_of(&staged[win]) { win = i; } }
    let (s, p) = staged.into_iter().nth(win).unwrap();
    Ok((t.id().unwrap(), s, s == STAGE_ZSTD_DICT, p.unwrap_or(raw)))
}

/// One column exactly as `write` encodes it with `opts` (rule mode keeps this payload when no rule is
/// smaller; same bytes as `write`, which gives the same bytes on the calling thread and under rayon).
pub(crate) fn encode_for_write(c: &Column, opts: &CompactOptions) -> Result<(u8, u8, bool, Vec<u8>), KoreError> {
    let (codec, dict) = (opts.codecs.codec_for(&c.name), opts.dict_for(&c.name));
    if opts.search == SearchMode::Quick { encode_column_quick(&c.data, codec, dict, opts.zstd_level, opts.small_zstd_level, false) }
    else { encode_column_pruned(&c.data, codec, dict, opts.zstd_level, opts.small_zstd_level, false) }
}

/// Several files at once: the blocks are encoded concurrently (more parallel work than one file
/// offers); every output is byte-identical to `write` of that block alone.
pub fn write_many(items: &[(&DataBlock, &CompactOptions)]) -> Result<Vec<Vec<u8>>, KoreError> {
    use rayon::prelude::*;
    items.par_iter().map(|(b, o)| write(b, o)).collect()
}

pub fn write(block: &DataBlock, opts: &CompactOptions) -> Result<Vec<u8>, KoreError> {
    write_impl(block, opts, Search::Auto)
}

fn write_impl(block: &DataBlock, opts: &CompactOptions, search: Search) -> Result<Vec<u8>, KoreError> {
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
    let search = match search {
        Search::Auto if block.columns.iter().map(|c| approx_bytes(&c.data)).sum::<usize>() < PAR_ENCODE_MIN => Search::Here,
        Search::Auto => Search::Parallel,
        s => s,
    };
    let quick = opts.search == SearchMode::Quick;
    let one = |c: &Column| {
        let (codec, dict) = (opts.codecs.codec_for(&c.name), opts.dict_for(&c.name));
        match search {
            Search::Reference => encode_column(&c.data, codec, dict, opts.zstd_level, opts.small_zstd_level),
            _ if quick => encode_column_quick(&c.data, codec, dict, opts.zstd_level, opts.small_zstd_level, search == Search::Parallel),
            _ => encode_column_pruned(&c.data, codec, dict, opts.zstd_level, opts.small_zstd_level, search == Search::Parallel),
        }
    };
    let encoded: Result<Vec<(u8, u8, bool, Vec<u8>)>, KoreError> = if search == Search::Here {
        block.columns.iter().map(one).collect()
    } else {
        block.columns.par_iter().map(one).collect()
    };
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
    if opts.compact_directory { return Ok(serialize_v5(block, &encoded, &used, &dict_idx)); }

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

// ── container version 5: version 4 with a varint directory ───────────────────
//
// ```text
// "KORE" | version:u16 = 5 | varint(num_cols) | varint(num_rows) | varint(num_dicts)
// per dictionary: dict_id:[32] | varint(content_len)
// per column: varint(name_len) | name | dtype:u8 | codec:u8 | [dict:u8 if stage == 4] | varint(payload_len) | payload_crc32:u32
//   codec = transform id (bits 0-3) | stage id << 4 (bits 4-6); bit 7 must be 0
// dir_crc32:u32          CRC-32 of every byte from the start of the file up to here
// payloads               concatenated in directory order, exactly as in version 4
// ```
// Varints are LEB128 and must be minimal. Everything else (ids, checks, payloads, dictionaries) is
// version 4. Readers of versions 1-4 stop at the version field ("unsupported version 5").

pub const VERSION_V5: u16 = 5;

/// True if `data` starts with the KORE magic and format version 5 (version 4 with a varint directory).
pub fn is_v5(data: &[u8]) -> bool {
    data.len() >= 6 && &data[..4] == MAGIC && u16::from_le_bytes([data[4], data[5]]) == VERSION_V5
}

/// Version 4 or 5 (the files `read`, `read_rows` and `info` accept).
pub fn is_compact(data: &[u8]) -> bool { is_v4(data) || is_v5(data) }

pub(crate) fn serialize_v5(block: &DataBlock, encoded: &[(u8, u8, bool, Vec<u8>)], used: &[SharedDict], dict_idx: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION_V5.to_le_bytes());
    codec::put_varint(&mut out, block.columns.len() as u64);
    codec::put_varint(&mut out, block.num_rows as u64);
    codec::put_varint(&mut out, used.len() as u64);
    for d in used {
        out.extend_from_slice(&d.id());
        codec::put_varint(&mut out, d.content().len() as u64);
    }
    for (i, (col, (t, s, _, payload))) in block.columns.iter().zip(encoded).enumerate() {
        codec::put_varint(&mut out, col.name.len() as u64);
        out.extend_from_slice(col.name.as_bytes());
        out.push(codec::dtype_of(&col.data) as u8);
        out.push(*t | (*s << 4));
        if *s == STAGE_ZSTD_DICT { out.push(dict_idx[i]); }
        codec::put_varint(&mut out, payload.len() as u64);
        out.extend_from_slice(&compress::crc32(payload).to_le_bytes());
    }
    let dir_crc = compress::crc32(&out);
    out.extend_from_slice(&dir_crc.to_le_bytes());
    for (_, _, _, payload) in encoded { out.extend_from_slice(payload); }
    out
}

fn take_varint(d: &[u8], pos: &mut usize) -> Result<u64, KoreError> {
    let start = *pos;
    let v = codec::get_varint(d, pos).map_err(|_| bad("truncated .kore data"))?;
    let mut canon = Vec::new();
    codec::put_varint(&mut canon, v);
    if canon.len() != *pos - start { return Err(bad("non-minimal varint in file header/directory")); }
    Ok(v)
}

fn parse_header_v5(data: &[u8]) -> Result<(u64, Vec<([u8; 32], u32)>, Vec<Raw>, usize), KoreError> {
    let mut pos = 6usize;
    let num_cols = take_varint(data, &mut pos)?;
    let num_rows = take_varint(data, &mut pos)?;
    let nd = take_varint(data, &mut pos)?;
    if nd > ((data.len() - pos) / 33) as u64 || nd > u16::MAX as u64 { return Err(bad("implausible dictionary count")); }
    let mut dicts = Vec::with_capacity(nd as usize);
    for _ in 0..nd {
        let id: [u8; 32] = take(data, &mut pos, 32)?.try_into().unwrap();
        let len = u32::try_from(take_varint(data, &mut pos)?).map_err(|_| bad("implausible dictionary length"))?;
        dicts.push((id, len));
    }
    // name_len + dtype + codec + payload_len + crc, with an empty name
    if num_cols > ((data.len() - pos) / 8) as u64 { return Err(bad("implausible column count")); }
    let mut dir = Vec::with_capacity(num_cols as usize);
    for _ in 0..num_cols {
        let name_len = usize::try_from(take_varint(data, &mut pos)?).map_err(|_| bad("implausible name length"))?;
        let name = String::from_utf8(take(data, &mut pos, name_len)?.to_vec()).map_err(|_| bad("invalid UTF-8 column name"))?;
        let hdr = take(data, &mut pos, 2)?;
        let dtype = DType::try_from(hdr[0])?;
        if hdr[1] & 0x80 != 0 { return Err(bad("reserved codec bit set")); }
        let (transform, stage) = (hdr[1] & 0x0F, hdr[1] >> 4);
        let dict = if stage == STAGE_ZSTD_DICT { take(data, &mut pos, 1)?[0] } else { NO_DICT };
        if stage == STAGE_ZSTD_DICT && dict == NO_DICT { return Err(bad("dictionary index out of range")); }
        let len = take_varint(data, &mut pos)?;
        let crc = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap());
        dir.push(Raw { name, dtype, transform, stage, dict, len, crc });
    }
    let covered = pos;
    let dir_crc = u32::from_le_bytes(take(data, &mut pos, 4)?.try_into().unwrap());
    if compress::crc32(&data[..covered]) != dir_crc { return Err(bad("checksum mismatch in file header/directory")); }
    Ok((num_rows, dicts, dir, pos))
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
    if version == VERSION_V5 {
        let (num_rows, dicts, dir, pos) = parse_header_v5(data)?;
        return check_header(num_rows, dicts, dir, pos);
    }
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
    check_header(num_rows, dicts, dir, pos)
}

/// Semantic checks of an authenticated header (versions 4 and 5).
fn check_header(num_rows: u64, dicts: Vec<([u8; 32], u32)>, dir: Vec<Raw>, pos: usize) -> Result<(usize, Vec<([u8; 32], u32)>, Vec<Raw>, usize), KoreError> {
    let nd = dicts.len();
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

pub(crate) fn decode_payload(payload: &[u8], dtype: DType, transform: u8, stage: u8, dict: Option<&SharedDict>, n: usize) -> Result<ColumnData, KoreError> {
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

impl Info {
    /// `dict_ids` as lowercase hex (the `.kdic` file stem, `SharedDict::id_hex`).
    pub fn dict_ids_hex(&self) -> Vec<String> { self.dict_ids.iter().map(|d| crate::dict::hex(d)).collect() }
    /// Per column: (name, id of its dictionary as lowercase hex, if it uses one).
    pub fn column_dicts_hex(&self) -> Vec<(String, Option<String>)> {
        self.columns.iter().map(|c| (c.0.clone(), c.4.as_ref().map(|d| crate::dict::hex(d)))).collect()
    }
}

// ── row-level partial decode ─────────────────────────────────────────────────

/// Rows `rows` of the columns `wanted` (all columns if None, in file order; else in the requested
/// order) of a version-4 or version-5 file, without materialising the other rows.
///
/// Same checks as `read`: header, directory and the exact file length are verified, every kept
/// column's CRC is verified and its payload is fully decompressed and validated (`decode_transform_rows`),
/// so `read_rows(d, w, r, s)` is `Ok` exactly when `read(d, w, s)` is, and then equals rows `r` of it.
/// What is saved is the per-row work of the rows outside `rows` (string allocation, timestamp and hex
/// formatting, dictionary lookups). Columns not in `wanted` are neither checksummed nor decompressed.
pub fn read_rows(data: &[u8], wanted: Option<&[&str]>, rows: std::ops::Range<usize>, dicts: &DictSet) -> Result<DataBlock, KoreError> {
    match std::panic::catch_unwind(|| read_rows_inner(data, wanted, rows, dicts)) {
        Ok(r) => r,
        Err(_) => Err(bad("corrupt .kore data")),
    }
}

/// One row (`read_rows` of `row..row + 1`).
pub fn read_row(data: &[u8], wanted: Option<&[&str]>, row: usize, dicts: &DictSet) -> Result<DataBlock, KoreError> {
    read_rows(data, wanted, row..row.saturating_add(1), dicts)
}

fn read_rows_inner(data: &[u8], wanted: Option<&[&str]>, rows: std::ops::Range<usize>, dicts: &DictSet) -> Result<DataBlock, KoreError> {
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
    let (lo, hi) = (rows.start, rows.end);
    if lo > hi || hi > num_rows { return Err(bad(format!("rows {lo}..{hi} out of range: the file has {num_rows} rows"))); }
    let keep = |name: &str| wanted.map_or(true, |w| w.iter().any(|x| *x == name));
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
        let data = decode_payload_rows(e.payload, e.dtype, e.transform, e.stage, d, num_rows, lo, hi)
            .map_err(|er| bad(format!("column '{}': {er}", e.name)))?;
        if data.len() != hi - lo { return Err(bad(format!("column '{}' has {} values, expected {}", e.name, data.len(), hi - lo))); }
        Ok(Column { name: e.name, data })
    };
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
    Ok(DataBlock { columns, num_rows: hi - lo })
}

#[allow(clippy::too_many_arguments)]
fn decode_payload_rows(payload: &[u8], dtype: DType, transform: u8, stage: u8, dict: Option<&SharedDict>, n: usize, lo: usize, hi: usize) -> Result<ColumnData, KoreError> {
    let t = Transform::from_id_v4(transform)?;
    if stage == STAGE_ZSTD_DICT {
        let mut pos = 0;
        let raw_len = usize::try_from(codec::get_varint(payload, &mut pos)?).map_err(|_| bad("overflow"))?;
        if raw_len > 1 << 31 { return Err(bad("implausible decoded size")); }
        let raw = dict.expect("resolved").decompress(&payload[pos..], raw_len)?;
        if raw.len() != raw_len { return Err(bad("decoded stage length mismatch")); }
        return codec::decode_transform_rows(&raw, dtype, t, n, lo, hi);
    }
    let s = Stage::from_id(stage)?;
    let raw = codec::decode_stage(payload, s)?;
    codec::decode_transform_rows(&raw, dtype, t, n, lo, hi)
}

/// Per column `(offset of its payload CRC in the file, payload length)` of a version-4/5 directory,
/// without verifying any checksum (for tests that forge files).
#[doc(hidden)]
pub fn info_unchecked(data: &[u8]) -> Option<Vec<(usize, usize)>> {
    let v5 = is_v5(data);
    if !v5 && !is_v4(data) { return None; }
    let mut pos = 6usize;
    let mut out = vec![];
    let vi = |pos: &mut usize| codec::get_varint(data, pos).ok().map(|v| v as usize);
    if v5 {
        let nc = vi(&mut pos)?; vi(&mut pos)?; let nd = vi(&mut pos)?;
        for _ in 0..nd { pos += 32; vi(&mut pos)?; }
        for _ in 0..nc {
            let nl = vi(&mut pos)?; pos += nl + 1;
            let st = *data.get(pos)? >> 4; pos += 1;
            if st == STAGE_ZSTD_DICT { pos += 1; }
            let len = vi(&mut pos)?;
            out.push((pos, len)); pos += 4;
        }
    } else {
        let nc = u32::from_le_bytes(data.get(6..10)?.try_into().ok()?) as usize;
        let nd = u16::from_le_bytes(data.get(18..20)?.try_into().ok()?) as usize;
        pos = 20 + nd * 36;
        for _ in 0..nc {
            let nl = u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?) as usize; pos += 2 + nl + 4;
            let len = u64::from_le_bytes(data.get(pos..pos + 8)?.try_into().ok()?) as usize; pos += 8;
            out.push((pos, len)); pos += 4;
        }
    }
    if pos > data.len() { return None; }
    Some(out)
}
