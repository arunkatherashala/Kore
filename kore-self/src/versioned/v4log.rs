//! Log format 4: segments are kore-store version-4 ("compact") files; the `text` column of `events`
//! segments may be coded with a shared dictionary, a separate content-addressed object
//! (`dicts/<sha256>.kdic`, kore-store `KDIC`) that is listed in the MANIFEST by the commit that first
//! uses it. See `kore-self/docs/versioned_log.md` ("Log format 4").
//!
//! Dictionary content is a sequence of WHOLE event texts separated by the byte 0xFF (never part of
//! UTF-8), so the texts a dictionary was built from can be listed exactly (`members`); `forget` uses
//! that to rebuild every dictionary that holds a forgotten text.

use super::manifest::{DictRef, Entry};
use super::*;
use kore_store::{CompactOptions, DictSet, KoreReader, KoreWriter, SharedDict};

pub const DICT_DIR: &str = "dicts";
pub const DICT_SEP: u8 = 0xFF;
/// Segment encoding id (`SegRef.enc`) of format 4 segments.
pub const ENC_V4: u8 = 4;

/// When dictionaries are built. Not stored in the log: it only affects future writes (readers need
/// nothing but the listed objects).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DictPolicy {
    /// The first dictionary is built by the first commit after which the log holds at least this many
    /// events (the commit's own events included, and coded with it). 0 = never build one.
    pub train_events: u64,
    /// Maximum dictionary content: the newest whole event texts that fit.
    pub max_bytes: usize,
    /// Drift check: after this many events since the current dictionary was listed, the next commit
    /// with at least `check_min_text` bytes of event text is also coded with a candidate dictionary
    /// built from the events stored before it; the candidate replaces the current dictionary (a new
    /// object, the old one stays for the segments that use it) if it makes that commit's events
    /// segment at least `min_gain_pct` percent smaller. 0 = no drift checks.
    pub check_events: u64,
    pub check_min_text: usize,
    pub min_gain_pct: u32,
}

impl Default for DictPolicy {
    fn default() -> Self {
        DictPolicy { train_events: 1000, max_bytes: 128 << 10, check_events: 100_000, check_min_text: 16 << 10, min_gain_pct: 5 }
    }
}

pub fn dict_file(id_hex: &str) -> String {
    format!("{DICT_DIR}/{id_hex}.kdic")
}

/// Dictionary from texts given NEWEST FIRST: whole texts only, as many as fit in `max_bytes`
/// (a text longer than that is skipped), stored oldest first. None if nothing fits.
pub fn dict_from_texts<'a>(newest_first: impl Iterator<Item = &'a str>, max_bytes: usize) -> Option<SharedDict> {
    let mut picked: Vec<&str> = vec![];
    let mut total = 0usize;
    for t in newest_first {
        if t.is_empty() || t.len() + 1 > max_bytes {
            continue;
        }
        if total + t.len() + 1 > max_bytes {
            break;
        }
        total += t.len() + 1;
        picked.push(t);
    }
    if picked.is_empty() {
        return None;
    }
    let mut c = Vec::with_capacity(total);
    for (i, t) in picked.iter().rev().enumerate() {
        if i > 0 {
            c.push(DICT_SEP);
        }
        c.extend_from_slice(t.as_bytes());
    }
    SharedDict::from_content(&c).ok()
}

/// The event texts a dictionary was built from (exact, see the module docs).
pub fn members(d: &SharedDict) -> Vec<&[u8]> {
    let c = d.content();
    let c = if c.first() == Some(&0) { &c[1..] } else { c };
    c.split(|&b| b == DICT_SEP).filter(|m| !m.is_empty()).collect()
}

/// `d` without the given texts (every occurrence). None if no text is left.
pub fn without_texts(d: &SharedDict, gone: &HashSet<&[u8]>) -> Option<SharedDict> {
    let keep: Vec<&[u8]> = members(d).into_iter().filter(|m| !gone.contains(m)).collect();
    if keep.is_empty() {
        return None;
    }
    let mut c = vec![];
    for (i, m) in keep.iter().enumerate() {
        if i > 0 {
            c.push(DICT_SEP);
        }
        c.extend_from_slice(m);
    }
    SharedDict::from_content(&c).ok()
}

pub fn dict_ref(d: &SharedDict) -> DictRef {
    DictRef { id: d.id_hex(), file: dict_file(&d.id_hex()), bytes: d.to_bytes().len() as u64 }
}

/// Encode a format-4 segment. Only `events.text` may use the dictionary.
pub fn encode(t: Table, rows: &[Vec<Cell>], dict: Option<&SharedDict>) -> Vec<u8> {
    let block = rows_to_block(t, rows);
    let mut o = CompactOptions::new();
    if let (Table::Events, Some(d)) = (t, dict) {
        o = o.with_dict("text", d.clone());
    }
    KoreWriter::to_bytes_compact(&block, &o).expect("format-4 segment encodes")
}

pub fn decode(t: Table, bytes: &[u8], dicts: &DictSet) -> Result<Vec<Vec<Cell>>, String> {
    if !kore_store::v4::is_v4(bytes) {
        return Err("not a version-4 segment".into());
    }
    let block = KoreReader::from_bytes_with_dicts(bytes, dicts).map_err(|e| e.to_string())?;
    block_to_rows(t, &block)
}

/// The id column (column 0) only; needs no dictionary.
pub fn decode_ids(t: Table, bytes: &[u8]) -> Result<Vec<IdKey>, String> {
    if !kore_store::v4::is_v4(bytes) {
        return Err("not a version-4 segment".into());
    }
    let name = t.columns()[0].0;
    let block = KoreReader::from_bytes_columns_with_dicts(bytes, &[name], &DictSet::new()).map_err(|e| e.to_string())?;
    match &block.columns[0].data {
        kore_core::ColumnData::Str(v) => Ok(v.iter().flatten().map(|s| IdKey::from_str(s)).collect()),
        _ => Err(format!("{}: id column is not text", t.name())),
    }
}

/// Ids (hex) of the dictionaries a format-4 segment references (checksummed header only).
pub fn seg_dict_ids(bytes: &[u8]) -> Result<Vec<String>, String> {
    let info = kore_store::v4::info(bytes).map_err(|e| e.to_string())?;
    Ok(info.dict_ids.iter().map(|id| hex(id)).collect())
}

/// Read and verify one listed dictionary object (length, sha256 = id).
pub fn read_dict(dir: &Path, d: &DictRef) -> Result<SharedDict, VerifyError> {
    let bytes = match fs::read(dir.join(&d.file)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(VerifyError::MissingSegment { file: d.file.clone() }),
        Err(e) => return Err(VerifyError::Io(format!("{}: {e}", d.file))),
    };
    if bytes.len() as u64 != d.bytes {
        return Err(VerifyError::ChecksumMismatch { file: d.file.clone() });
    }
    let sd = SharedDict::from_bytes(&bytes).map_err(|_| VerifyError::ChecksumMismatch { file: d.file.clone() })?;
    if sd.id_hex() != d.id {
        return Err(VerifyError::ChecksumMismatch { file: d.file.clone() });
    }
    Ok(sd)
}

/// Every dictionary listed in the MANIFEST, each verified.
pub fn load_dicts(dir: &Path, entries: &[Entry]) -> Result<DictSet, VerifyError> {
    let mut s = DictSet::new();
    for e in entries {
        for d in &e.dicts {
            s.add(read_dict(dir, d)?);
        }
    }
    Ok(s)
}

/// Chain-level rules for dictionary listings: canonical file name, 64-char lowercase hex id,
/// each id listed once.
pub fn check_listings(entries: &[Entry]) -> Result<(), VerifyError> {
    let mut seen = HashSet::new();
    for e in entries {
        for d in &e.dicts {
            let ok_hex = d.id.len() == 64 && d.id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !ok_hex || d.file != dict_file(&d.id) {
                return Err(VerifyError::ChainBroken { version: e.v, detail: format!("bad dictionary listing {}", d.file) });
            }
            if !seen.insert(d.id.as_str()) {
                return Err(VerifyError::ChainBroken { version: e.v, detail: format!("dictionary {} listed twice", d.id) });
            }
        }
    }
    Ok(())
}

/// id -> index of the entry that lists it.
pub fn listed_at(entries: &[Entry]) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        for d in &e.dicts {
            m.insert(d.id.clone(), i);
        }
    }
    m
}

/// The newest listed dictionary and the number of event rows stored up to (and including) the
/// commit that listed it.
pub fn current(entries: &[Entry], dicts: &DictSet) -> (Option<SharedDict>, u64) {
    let mut cum = 0u64;
    let mut out = (None, 0u64);
    for e in entries {
        cum += e.segments.iter().filter(|g| g.table == "events").map(|g| g.rows).sum::<u64>();
        if let Some(d) = e.dicts.last() {
            let id = codec::hex64_to_bytes(&d.id);
            out = (id.and_then(|b| dicts.get(&b).cloned()), cum);
        }
    }
    out
}

impl VersionedLog {
    /// Dictionary policy for future format-4 writes of this handle (default: `DictPolicy::default()`).
    pub fn set_dict_policy(&mut self, p: DictPolicy) {
        self.dict_policy = p;
    }

    pub fn dict_policy(&self) -> DictPolicy {
        self.dict_policy
    }

    /// Ids of the dictionaries listed in the MANIFEST, oldest first.
    pub fn dictionary_ids(&self) -> Vec<String> {
        self.entries.iter().flat_map(|e| e.dicts.iter().map(|d| d.id.clone())).collect()
    }

    /// Up to `max_bytes` of whole event texts stored in the log, newest first.
    fn recent_texts(&self, max_bytes: usize) -> Result<Vec<String>, LogError> {
        let mut out: Vec<String> = vec![];
        let mut total = 0usize;
        {
            let g = self.head.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(s) = g.as_ref() {
                for r in s.tables[Table::Events.idx()].iter().rev() {
                    let t = r[5].s();
                    total += t.len() + 1;
                    out.push(t);
                    if total >= max_bytes {
                        break;
                    }
                }
                return Ok(out);
            }
        }
        for &(_, ei, si) in self.ev_segs.iter().rev() {
            let rows = load_seg(&self.dir, &self.entries[ei].segments[si], &self.dicts)?;
            for r in rows.iter().rev() {
                let t = r[5].s();
                total += t.len() + 1;
                out.push(t);
            }
            if total >= max_bytes {
                break;
            }
        }
        Ok(out)
    }

    /// The dictionary for this commit's events segment, and whether it is new (to be listed).
    /// `new_events` are the rows being committed (already counted in the index).
    pub(super) fn choose_dict(&mut self, new_events: &[Vec<Cell>]) -> Result<(Option<SharedDict>, bool), LogError> {
        let p = self.dict_policy;
        let count = self.index.next_ord as u64;
        let fresh_text = |rows: &[Vec<Cell>]| -> Vec<String> { rows.iter().rev().map(|r| r[5].s()).collect() };
        match self.cur_dict.clone() {
            None => {
                if p.train_events == 0 || count < p.train_events {
                    return Ok((None, false));
                }
                let mut texts = fresh_text(new_events);
                let have: usize = texts.iter().map(|t| t.len() + 1).sum();
                if have < p.max_bytes {
                    texts.extend(self.recent_texts(p.max_bytes - have)?);
                }
                let d = dict_from_texts(texts.iter().map(|s| s.as_str()), p.max_bytes);
                Ok((d.clone(), d.is_some()))
            }
            Some(cur) => {
                let text_now: usize = new_events.iter().map(|r| r[5].s().len()).sum();
                if p.check_events == 0 || count.saturating_sub(self.events_at_dict) < p.check_events || text_now < p.check_min_text {
                    return Ok((Some(cur), false));
                }
                let texts = self.recent_texts(p.max_bytes)?;
                let cand = match dict_from_texts(texts.iter().map(|s| s.as_str()), p.max_bytes) {
                    Some(c) if c.id() != cur.id() && !self.dicts.get(&c.id()).is_some() => c,
                    _ => {
                        self.events_at_dict = count;
                        return Ok((Some(cur), false));
                    }
                };
                let a = encode(Table::Events, new_events, Some(&cur)).len();
                let b = encode(Table::Events, new_events, Some(&cand)).len();
                if (b as u64) * 100 <= (a as u64) * (100 - p.min_gain_pct.min(100) as u64) {
                    Ok((Some(cand), true))
                } else {
                    self.events_at_dict = count;
                    Ok((Some(cur), false))
                }
            }
        }
    }

    /// Explicit, one-way upgrade to format 4 (from 1, 2 or 3): one new commit records
    /// `schema_meta.format_version = 4` and, in the same atomic MANIFEST replacement, switches the
    /// header to `KORE-VLOG-MANIFEST 3` (all earlier entry lines byte for byte, so every old version
    /// hash is unchanged). Later commits append (journal) and write format-4 segments; dictionaries
    /// follow `DictPolicy`. Older binaries refuse the result. False if already format 4.
    pub fn upgrade_to_v4(&mut self) -> Result<bool, LogError> {
        if self.format == LogFormat::V4 {
            return Ok(false);
        }
        self.check_manifest_unchanged()?;
        let last_at = self.entries.last().map(|e| e.recorded_at.clone()).unwrap_or_default();
        let mut now = (self.clock)();
        if now < last_at {
            now = last_at;
        }
        let meta = MetaRow { format_version: FORMAT_V4, created_at: now.clone() };
        self.write_commit(vec![(Table::SchemaMeta, vec![meta.to_cells()])], now, LogFormat::V4, None)?;
        self.format = LogFormat::V4;
        self.index.content_idem = true;
        Ok(true)
    }
}
