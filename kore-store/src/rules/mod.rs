//! KORE rule mode (opt-in): lossless compression by discovered rules. Container version 5.
//!
//! When a column, or a relation between columns, follows a rule, the file stores the rule plus its
//! exceptions/residuals instead of the values; the reader regenerates the exact values and checks them
//! against a digest of the original column (a mismatch is an error, never silent). See
//! docs/kore_rule_mode.md for the DSL, the measurements and the limits.
//!
//! ```text
//! "KORE" | version:u16 = 5 | num_cols:u32 | num_rows:u64
//! num_dicts:u16, per dictionary: dict_id:[32] | content_len:u32                       (as version 4)
//! per column: name_len:u16 | name | dtype:u8 | transform:u8 | stage:u8 | dict:u8 | payload_len:u64 | payload_crc32:u32
//! dir_crc32:u32
//! payloads
//! ```
//! Exactly the version-4 layout; in addition `transform` may be `0x10` (rule), then `stage` = 0 and
//! `dict` = 0xFF and the payload is
//! ```text
//! family:u8 | digest:u64 (CRC-64/XZ of the decoded column, `stream::digest`) | n_deps:u8 | deps: varint column
//! indices | body (see `within` for families 1-6, `cross` for 7-9)
//! ```
//! Families 1-6 use no other column (n_deps = 0); 7-9 name their source columns (1 or 2). A source may itself be
//! derived; the reader decodes in dependency order and refuses cycles. Every other column payload is a
//! version-4 payload, decoded by the version-4 code.
//!
//! Writer rule: every column is encoded by the version-4 writer and by every applicable rule; the smaller
//! payload is kept (ties keep version 4). If no column uses a rule, the output is the version-4 file,
//! byte for byte. Readers that predate version 5 stop at the version field ("unsupported version 5").

pub mod stream;
pub mod intseq;
mod within;
mod cross;

pub use stream::digest;

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use crate::codec::{self, ColumnCodec, Transform};
use crate::dict::{DictSet, SharedDict};
use crate::v4::{NO_DICT, STAGE_ZSTD_DICT};
use crate::{compress, CompactOptions, DType, MAGIC};
use stream::{bad, put_varint, Levels, Rd};

pub const VERSION_V5: u16 = 5;
/// Directory transform id of a rule payload (version 5 only).
pub const TRANSFORM_RULE: u8 = 0x10;
const MAX_ROWS: usize = 1 << 28;
const MIN_ENTRY: usize = 2 + 1 + 1 + 1 + 1 + 8 + 4;
const PAR_MIN_BYTES: usize = 64 * 1024;

/// Options of the rule-mode writer: the version-4 options (levels, codecs, shared dictionaries) plus
/// which rule searches run.
#[derive(Debug, Clone)]
pub struct RuleOptions {
    pub compact: CompactOptions,
    /// rules inside one column (families 1-6)
    pub within: bool,
    /// relations between columns (families 7-9)
    pub cross: bool,
}

impl Default for RuleOptions {
    fn default() -> Self { CompactOptions::default().with_rules() }
}

impl CompactOptions {
    /// Opt into rule mode (version-5 files when a rule wins; see `rules`).
    pub fn with_rules(self) -> RuleOptions { RuleOptions { compact: self, within: true, cross: true } }
}

impl RuleOptions {
    pub fn new() -> Self { Self::default() }
    pub fn within_only(mut self) -> Self { self.cross = false; self }
    pub fn cross_only(mut self) -> Self { self.within = false; self }
}

/// True if `data` starts with the KORE magic and format version 5.
pub fn is_v5(data: &[u8]) -> bool {
    data.len() >= 6 && &data[..4] == MAGIC && u16::from_le_bytes([data[4], data[5]]) == VERSION_V5
}

fn dict_for<'a>(o: &'a CompactOptions, name: &str) -> Option<&'a SharedDict> { o.dicts.iter().find(|(n, _)| n == name).map(|(_, d)| d) }

fn rule_payload(family: u8, dg: u64, deps: &[usize], body: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(body.len() + 16);
    p.push(family);
    p.extend_from_slice(&dg.to_le_bytes());
    p.push(deps.len() as u8);
    for &d in deps { put_varint(&mut p, d as u64); }
    p.extend_from_slice(body);
    p
}

/// (family, digest, deps, body offset) of a rule payload.
fn rule_header(p: &[u8], num_cols: usize) -> Result<(u8, u64, Vec<usize>, usize), KoreError> {
    let mut rd = Rd::new(p);
    let family = rd.u8()?;
    let dg = rd.u64le()?;
    let nd = rd.u8()? as usize;
    if nd > 2 { return Err(bad("too many rule source columns")); }
    let mut deps = Vec::with_capacity(nd);
    for _ in 0..nd { deps.push(rd.count(num_cols.saturating_sub(1))?); }
    match family {
        1..=6 if nd == 0 => {}
        7..=9 if nd >= 1 => {}
        1..=9 => return Err(bad("rule family with the wrong number of source columns")),
        f => return Err(bad(format!("unknown rule family {f}"))),
    }
    Ok((family, dg, deps, rd.pos))
}

fn decode_rule(p: &[u8], dtype: DType, n: usize, deps: &[&ColumnData], num_cols: usize) -> Result<ColumnData, KoreError> {
    let (family, dg, _, off) = rule_header(p, num_cols)?;
    let mut rd = Rd::new(&p[off..]);
    if deps.iter().any(|d| d.len() != n) { return Err(bad("source column length mismatch")); }
    let d = if family <= 6 { within::decode(family, &mut rd, dtype, n)? } else { cross::decode(family, &mut rd, dtype, n, deps)? };
    rd.done()?;
    if d.len() != n { return Err(bad("rule decoded the wrong number of values")); }
    if codec::dtype_of(&d) != dtype { return Err(bad("rule decoded the wrong type")); }
    if digest(&d) != dg { return Err(bad("rule-mode checksum mismatch: decoded values differ from the written column")); }
    Ok(d)
}

/// Bit-exact equality (floats by bits, nulls by position).
fn same(a: &ColumnData, b: &ColumnData) -> bool {
    match (a, b) {
        (ColumnData::Int64(x), ColumnData::Int64(y)) => x == y,
        (ColumnData::Float64(x), ColumnData::Float64(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.map(f64::to_bits) == q.map(f64::to_bits)),
        (ColumnData::Bool(x), ColumnData::Bool(y)) => x == y,
        (ColumnData::Str(x), ColumnData::Str(y)) => x == y,
        _ => false,
    }
}

enum Choice { V4, Rule { payload: Vec<u8>, deps: Vec<usize> } }

/// What the writer chose per column (for reports and tests).
#[derive(Debug, Clone)]
pub struct ColumnChoice { pub name: String, pub v4_bytes: usize, pub rule: Option<(String, usize)> }

pub fn write(block: &DataBlock, opts: &RuleOptions) -> Result<Vec<u8>, KoreError> { write_explained(block, opts).map(|x| x.0) }

/// `write` plus, per column, the version-4 size and the rule that won (description, payload bytes).
pub fn write_explained(block: &DataBlock, opts: &RuleOptions) -> Result<(Vec<u8>, Vec<ColumnChoice>), KoreError> {
    use rayon::prelude::*;
    let o = &opts.compact;
    for col in &block.columns {
        if col.data.len() != block.num_rows {
            return Err(bad(format!("column '{}' has {} values, block has {} rows", col.name, col.data.len(), block.num_rows)));
        }
        if col.name.len() > u16::MAX as usize { return Err(bad("column name too long")); }
    }
    for name in o.codecs.columns.iter().map(|(n, _)| n).chain(o.dicts.iter().map(|(n, _)| n)) {
        if !block.columns.iter().any(|c| &c.name == name) { return Err(bad(format!("option given for unknown column '{name}'"))); }
    }
    let lv = Levels { large: o.zstd_level, small: o.small_zstd_level };
    let n = block.num_rows;
    let ncols = block.columns.len();
    let base: Vec<(u8, u8, bool, Vec<u8>)> = block.columns.par_iter()
        .map(|c| crate::v4::encode_column(&c.data, o.codecs.codec_for(&c.name), dict_for(o, &c.name), o.zstd_level, o.small_zstd_level))
        .collect::<Result<_, _>>()?;
    // a column with an explicit codec keeps it; columns of a block with too many rows are not searched
    let eligible: Vec<bool> = block.columns.iter().map(|c| o.codecs.codec_for(&c.name) == ColumnCodec::AUTO && n <= MAX_ROWS && !matches!(c.data, ColumnData::StrDict { .. } | ColumnData::Bool(_))).collect();
    let digests: Vec<u64> = block.columns.par_iter().map(|c| digest(&c.data)).collect();

    // within-column rules
    let mut choice: Vec<Choice> = (0..ncols).map(|_| Choice::V4).collect();
    let mut size: Vec<usize> = base.iter().map(|b| b.3.len()).collect();
    let mut desc: Vec<Option<String>> = vec![None; ncols];
    if opts.within {
        let found: Vec<Option<Vec<u8>>> = (0..ncols).into_par_iter().map(|i| {
            if !eligible[i] { return None; }
            let c = &block.columns[i];
            let mut best: Option<Vec<u8>> = None;
            for (family, body) in within::encode(&c.data, lv) {
                let p = rule_payload(family, digests[i], &[], &body);
                if p.len() >= best.as_ref().map_or(size[i], |b| b.len()) { continue; }
                // the writer checks its own output: a rule is used only if it decodes to the exact column
                match decode_rule(&p, codec::dtype_of(&c.data), n, &[], ncols) {
                    Ok(d) if same(&d, &c.data) => best = Some(p),
                    _ => debug_assert!(false, "rule family {family} failed its own check on column {}", c.name),
                }
            }
            best
        }).collect();
        for (i, f) in found.into_iter().enumerate() {
            if let Some(p) = f { size[i] = p.len(); desc[i] = Some(describe_payload(&p, n, &[])); choice[i] = Choice::Rule { payload: p, deps: vec![] }; }
        }
    }

    // cross-column rules: greedy by saving; a column is derived at most once and the derivation graph
    // stays acyclic (a derived column may be the source of another one)
    if opts.cross && ncols >= 2 && n > 0 {
        let cols: Vec<&ColumnData> = block.columns.iter().map(|c| &c.data).collect();
        let mut cands: Vec<(usize, cross::Cand, Vec<u8>)> = cross::search(&cols, lv).into_iter()
            .filter(|c| eligible[c.target])
            .map(|c| { let p = rule_payload(c.family, digests[c.target], &c.deps, &c.body); (p.len(), c, p) })
            .filter(|(len, c, _)| *len < size[c.target])
            .collect();
        cands.sort_by_key(|(len, c, _)| (std::cmp::Reverse(size[c.target] - len), c.target, *len));
        let mut edges: Vec<Vec<usize>> = vec![vec![]; ncols];
        // true if `to` is reachable from `from` over the accepted derivations
        let reaches = |edges: &Vec<Vec<usize>>, from: usize, to: usize| -> bool {
            let mut seen = vec![false; edges.len()];
            let mut st = vec![from];
            while let Some(x) = st.pop() {
                if x == to { return true; }
                if seen[x] { continue; }
                seen[x] = true;
                st.extend(edges[x].iter().copied());
            }
            false
        };
        for (len, c, p) in cands {
            if !edges[c.target].is_empty() || c.deps.iter().any(|&d| reaches(&edges, d, c.target)) { continue; }
            if len >= size[c.target] { continue; }
            let deps: Vec<&ColumnData> = c.deps.iter().map(|&d| &block.columns[d].data).collect();
            match decode_rule(&p, codec::dtype_of(cols[c.target]), n, &deps, ncols) {
                Ok(d) if same(&d, cols[c.target]) => {}
                _ => { debug_assert!(false, "cross rule failed its own check on column {}", block.columns[c.target].name); continue; }
            }
            edges[c.target] = c.deps.clone();
            size[c.target] = len;
            let names: Vec<&str> = c.deps.iter().map(|&d| block.columns[d].name.as_str()).collect();
            desc[c.target] = Some(describe_payload(&p, n, &names));
            choice[c.target] = Choice::Rule { payload: p, deps: c.deps.clone() };
        }
    }

    let report: Vec<ColumnChoice> = block.columns.iter().enumerate().map(|(i, c)| ColumnChoice {
        name: c.name.clone(), v4_bytes: base[i].3.len(),
        rule: match &choice[i] { Choice::V4 => None, Choice::Rule { payload, .. } => Some((desc[i].clone().unwrap_or_default(), payload.len())) },
    }).collect();
    let any_rule = choice.iter().any(|c| matches!(c, Choice::Rule { .. }));
    let _ = choice.iter().map(|c| if let Choice::Rule { deps, .. } = c { deps.len() } else { 0 }).sum::<usize>();
    let entries: Vec<(u8, u8, bool, &[u8])> = (0..ncols).map(|i| match &choice[i] {
        Choice::V4 => (base[i].0, base[i].1, base[i].2, base[i].3.as_slice()),
        Choice::Rule { payload, .. } => (TRANSFORM_RULE, 0u8, false, payload.as_slice()),
    }).collect();
    Ok((assemble(if any_rule { VERSION_V5 } else { crate::v4::VERSION_V4 }, block, o, &entries)?, report))
}

/// The version-4 file layout (with `version` 4 this is exactly what `v4::write` produces).
fn assemble(version: u16, block: &DataBlock, o: &CompactOptions, enc: &[(u8, u8, bool, &[u8])]) -> Result<Vec<u8>, KoreError> {
    let mut used: Vec<SharedDict> = vec![];
    let mut dict_idx = vec![NO_DICT; block.columns.len()];
    for (i, (col, e)) in block.columns.iter().zip(enc).enumerate() {
        if !e.2 { continue; }
        let d = dict_for(o, &col.name).expect("dictionary");
        let k = match used.iter().position(|u| u.id() == d.id()) { Some(k) => k, None => { used.push(d.clone()); used.len() - 1 } };
        if k >= NO_DICT as usize { return Err(bad("too many dictionaries in one file")); }
        dict_idx[i] = k as u8;
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(block.columns.len() as u32).to_le_bytes());
    out.extend_from_slice(&(block.num_rows as u64).to_le_bytes());
    out.extend_from_slice(&(used.len() as u16).to_le_bytes());
    for d in &used {
        out.extend_from_slice(&d.id());
        out.extend_from_slice(&(d.content().len() as u32).to_le_bytes());
    }
    for (i, (col, (t, s, _, payload))) in block.columns.iter().zip(enc).enumerate() {
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
    for (_, _, _, payload) in enc { out.extend_from_slice(payload); }
    Ok(out)
}

fn describe_payload(p: &[u8], n: usize, names: &[&str]) -> String {
    let Ok((family, _, _, off)) = rule_header(p, usize::MAX) else { return "?".into() };
    let mut rd = Rd::new(&p[off..]);
    if family <= 6 { within::describe(family, &mut rd, n) } else { cross::describe(family, &mut rd, n, names) }
}

// ── reader ───────────────────────────────────────────────────────────────────

struct Raw { name: String, dtype: DType, transform: u8, stage: u8, dict: u8, len: u64, crc: u32 }

fn take<'a>(d: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], KoreError> {
    let end = pos.checked_add(n).filter(|&e| e <= d.len()).ok_or_else(|| bad("truncated .kore data"))?;
    let s = &d[*pos..end];
    *pos = end;
    Ok(s)
}

fn parse_header(data: &[u8]) -> Result<(usize, Vec<([u8; 32], u32)>, Vec<Raw>, usize), KoreError> {
    let mut pos = 0usize;
    if take(data, &mut pos, 4)? != MAGIC { return Err(bad("invalid KORE magic bytes")); }
    let version = u16::from_le_bytes(take(data, &mut pos, 2)?.try_into().unwrap());
    if version != VERSION_V5 { return Err(bad(format!("unsupported version {version}"))); }
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
    let num_rows = usize::try_from(num_rows).ok().filter(|&r| r <= MAX_ROWS).ok_or_else(|| bad("implausible row count"))?;
    let mut referenced = vec![false; nd];
    for r in &dir {
        if r.transform == TRANSFORM_RULE {
            if r.stage != 0 || r.dict != NO_DICT { return Err(bad("rule column with a stage or dictionary")); }
            continue;
        }
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

/// Per column of a version-5 file: name, dtype, payload bytes and the rule (None = version-4 payload).
/// Checks the header and payload checksums, decodes nothing.
pub fn describe(data: &[u8]) -> Result<Vec<(String, DType, usize, Option<String>)>, KoreError> {
    let (n, _, dir, mut pos) = parse_header(data)?;
    let names: Vec<String> = dir.iter().map(|r| r.name.clone()).collect();
    let mut out = vec![];
    for r in &dir {
        let p = take(data, &mut pos, usize::try_from(r.len).map_err(|_| bad("overflow"))?)?;
        if compress::crc32(p) != r.crc { return Err(bad(format!("checksum mismatch in column '{}'", r.name))); }
        let rule = if r.transform == TRANSFORM_RULE {
            let (_, _, deps, _) = rule_header(p, dir.len())?;
            let dn: Vec<&str> = deps.iter().map(|&d| names[d].as_str()).collect();
            Some(describe_payload(p, n, &dn))
        } else { None };
        out.push((r.name.clone(), r.dtype, p.len(), rule));
    }
    Ok(out)
}

pub fn read(data: &[u8], wanted: Option<&[&str]>, dicts: &DictSet) -> Result<DataBlock, KoreError> {
    match std::panic::catch_unwind(|| read_inner(data, wanted, dicts)) {
        Ok(r) => r,
        Err(_) => Err(bad("corrupt .kore data")),
    }
}

fn read_inner(data: &[u8], wanted: Option<&[&str]>, dicts: &DictSet) -> Result<DataBlock, KoreError> {
    use rayon::prelude::*;
    let (n, dict_list, dir, mut pos) = parse_header(data)?;
    let ncols = dir.len();
    let mut payloads: Vec<&[u8]> = Vec::with_capacity(ncols);
    for r in &dir {
        let len = usize::try_from(r.len).map_err(|_| bad("column length overflow"))?;
        payloads.push(take(data, &mut pos, len)?);
    }
    if pos != data.len() { return Err(bad("trailing bytes after last column")); }
    if let Some(w) = wanted {
        for name in w { if !dir.iter().any(|c| c.name == *name) { return Err(bad(format!("column not found: {name}"))); } }
    }
    let keep: Vec<bool> = dir.iter().map(|r| wanted.map_or(true, |w| w.iter().any(|x| *x == r.name))).collect();
    // the kept columns and, transitively, their rule sources (a rule header is read only after its CRC)
    let mut deps: Vec<Vec<usize>> = vec![vec![]; ncols];
    let mut need = vec![false; ncols];
    let mut stack: Vec<usize> = (0..ncols).filter(|&i| keep[i]).collect();
    while let Some(i) = stack.pop() {
        if need[i] { continue; }
        need[i] = true;
        if dir[i].transform != TRANSFORM_RULE { continue; }
        if compress::crc32(payloads[i]) != dir[i].crc { return Err(bad(format!("checksum mismatch in column '{}'", dir[i].name))); }
        let (_, _, d, _) = rule_header(payloads[i], ncols).map_err(|e| bad(format!("column '{}': {e}", dir[i].name)))?;
        if d.contains(&i) { return Err(bad("rule column refers to itself")); }
        stack.extend(d.iter().copied());
        deps[i] = d;
    }

    let mut resolved: Vec<Option<&SharedDict>> = vec![None; dict_list.len()];
    for i in (0..ncols).filter(|&i| need[i] && dir[i].dict != NO_DICT) {
        let (id, len) = dict_list[dir[i].dict as usize];
        let d = dicts.get(&id).ok_or_else(|| bad(format!(
            "column '{}' needs shared dictionary {} (not supplied; use KoreReader::from_bytes_with_dicts)", dir[i].name, crate::dict::hex(&id))))?;
        if d.content().len() != len as usize { return Err(bad("dictionary length does not match the file")); }
        resolved[dir[i].dict as usize] = Some(d);
    }
    let decode_one = |i: usize, srcs: &[&ColumnData]| -> Result<ColumnData, KoreError> {
        let r = &dir[i];
        if compress::crc32(payloads[i]) != r.crc { return Err(bad(format!("checksum mismatch in column '{}'", r.name))); }
        let d = if r.transform == TRANSFORM_RULE {
            decode_rule(payloads[i], r.dtype, n, srcs, ncols)
        } else {
            let dict = if r.dict == NO_DICT { None } else { resolved[r.dict as usize] };
            crate::v4::decode_payload(payloads[i], r.dtype, r.transform, r.stage, dict, n)
        }.map_err(|e| bad(format!("column '{}': {e}", r.name)))?;
        if d.len() != n { return Err(bad(format!("column '{}' has {} values, expected {n}", r.name, d.len()))); }
        Ok(d)
    };
    // decode in waves: every column whose sources are decoded; no progress = a dependency cycle
    let total: usize = (0..ncols).filter(|&i| need[i]).map(|i| payloads[i].len()).sum();
    let mut cols: Vec<Option<ColumnData>> = (0..ncols).map(|_| None).collect();
    loop {
        let ready: Vec<usize> = (0..ncols).filter(|&i| need[i] && cols[i].is_none() && deps[i].iter().all(|&d| cols[d].is_some())).collect();
        if ready.is_empty() {
            if (0..ncols).any(|i| need[i] && cols[i].is_none()) { return Err(bad("rule columns depend on each other in a cycle")); }
            break;
        }
        let run = |&i: &usize| -> Result<ColumnData, KoreError> {
            let srcs: Vec<&ColumnData> = deps[i].iter().map(|&d| cols[d].as_ref().expect("source decoded")).collect();
            decode_one(i, &srcs)
        };
        let done: Vec<ColumnData> = if ready.len() <= 1 || total < PAR_MIN_BYTES {
            ready.iter().map(run).collect::<Result<_, _>>()?
        } else {
            ready.par_iter().map(run).collect::<Result<_, _>>()?
        };
        for (i, d) in ready.iter().zip(done) { cols[*i] = Some(d); }
    }
    let mut columns: Vec<Column> = vec![];
    match wanted {
        None => for (i, c) in cols.into_iter().enumerate() { columns.push(Column { name: dir[i].name.clone(), data: c.expect("decoded") }); },
        Some(w) => {
            let mut seen: Vec<&str> = vec![];
            for name in w {
                if seen.contains(name) { continue; }
                seen.push(name);
                let i = dir.iter().position(|r| r.name == *name).expect("checked");
                columns.push(Column { name: dir[i].name.clone(), data: cols[i].take().expect("decoded") });
            }
        }
    }
    Ok(DataBlock { columns, num_rows: n })
}
