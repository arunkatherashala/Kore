//! MANIFEST: one JSON line per commit, hash-chained.
//!
//! File layout: header line, then one entry per line, each terminated by `\n`.
//! Entry hash = sha256(prev_hash_hex || canonical(entry)). Entry `v` is the
//! state of the log after commit `v`; version `v` = entries `0..=v`.

use super::model::sha256_hex;
use serde::{Deserialize, Serialize};

pub const HEADER: &str = "KORE-VLOG-MANIFEST 1";
/// Journal manifest (log format 3): same entry lines and hash chain, but a commit APPENDS its
/// line (then fsyncs) instead of rewriting the file; an unterminated last line is an uncommitted
/// tail (a crash during the append) and is ignored by `open`, reported by `verify`.
pub const HEADER_J: &str = "KORE-VLOG-MANIFEST 2";
/// Log format 4 MANIFEST: the journal discipline of header 2, plus entries may list shared
/// dictionary objects (`dicts`). Older binaries refuse the header, so they never misread a format-4 log.
pub const HEADER_V4: &str = "KORE-VLOG-MANIFEST 3";
pub const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegRef {
    pub table: String,
    /// Path relative to the log dir, e.g. `events/0000000003.kore`.
    pub file: String,
    pub sha256: String,
    pub rows: u64,
    /// Segment encoding: absent = v1 (kore-store columns, as written by every
    /// version before log format 2), `Some(2)` = v2 (`codec.rs`). Part of the
    /// hashed entry text, so it cannot be changed without breaking the chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enc: Option<u8>,
}

/// A shared dictionary object (kore-store `KDIC`, content-addressed) listed by the commit that first
/// uses it. `id` = sha256 of the object (hex), `file` = `dicts/<id>.kdic`, `bytes` = object length.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DictRef {
    pub id: String,
    pub file: String,
    pub bytes: u64,
}

/// Written by `forget`: what was removed, by event-id hash only (never content).
/// `removed` holds `sha256("forget1|" || event_id)` per forgotten event.
/// `prior_head` is the head hash before the affected entries were re-sealed;
/// `resealed_from` is the first version whose segments were rewritten.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForgetRecord {
    pub removed: Vec<String>,
    pub prior_head: String,
    pub resealed_from: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub v: u64,
    pub recorded_at: String,
    pub segments: Vec<SegRef>,
    pub prev: String,
    pub hash: String,
    /// Log format 4: dictionary objects first used by this commit. Absent on every other entry, so
    /// the hashed text of older entries is unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dicts: Vec<DictRef>,
    /// Present only on the commit created by `forget`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forget: Option<ForgetRecord>,
}

impl Entry {
    pub fn canonical(v: u64, recorded_at: &str, segments: &[SegRef]) -> String {
        let mut s = format!("v={v}\nat={recorded_at}\n");
        for g in segments {
            s.push_str(&format!("{}\t{}\t{}\t{}\n", g.table, g.file, g.sha256, g.rows));
            if let Some(e) = g.enc {
                // v1 entries keep their original hashed text byte for byte.
                s.push_str(&format!("enc\t{e}\n"));
            }
        }
        s
    }
    pub fn compute_hash(&self) -> String {
        let mut c = Self::canonical(self.v, &self.recorded_at, &self.segments);
        for d in &self.dicts {
            c.push_str(&format!("dict	{}	{}	{}
", d.id, d.file, d.bytes));
        }
        if let Some(f) = &self.forget {
            c.push_str(&format!("forget	{}	{}	{}
", f.removed.join(","), f.prior_head, f.resealed_from));
        }
        sha256_hex(&[self.prev.as_bytes(), c.as_bytes()])
    }
    pub fn new(v: u64, recorded_at: String, segments: Vec<SegRef>, prev: &str) -> Entry {
        let mut e = Entry { v, recorded_at, segments, prev: prev.to_string(), hash: String::new(), dicts: vec![], forget: None };
        e.hash = e.compute_hash();
        e
    }
    /// Recompute `prev` and `hash` (used when `forget` re-seals the chain).
    pub fn reseal(&mut self, prev: &str) {
        self.prev = prev.to_string();
        self.hash = self.compute_hash();
    }
}

/// Header line for a manifest level (1 = rewrite, 2 = journal, 3 = journal + dictionaries).
pub fn header_for(level: u8) -> &'static str {
    match level {
        3 => HEADER_V4,
        2 => HEADER_J,
        _ => HEADER,
    }
}

pub fn render(entries: &[Entry], level: u8) -> String {
    let mut s = String::new();
    s.push_str(header_for(level));
    s.push('\n');
    for e in entries {
        s.push_str(&serde_json::to_string(e).expect("entry serialises"));
        s.push('\n');
    }
    s
}

/// Result of `parse_ex`.
pub struct Parsed {
    pub entries: Vec<Entry>,
    /// Header was a journal header (`HEADER_J` or `HEADER_V4`).
    pub journal: bool,
    /// 1 = `HEADER`, 2 = `HEADER_J`, 3 = `HEADER_V4`.
    pub level: u8,
    /// Length of the committed prefix; bytes after it are an uncommitted partial tail (journal only).
    pub valid_len: usize,
}

/// Parse the raw manifest. Does not check the chain (see `verify`). Journal manifests tolerate an
/// unterminated last line (reported in `valid_len`); format-1 manifests never do.
pub fn parse_ex(raw: &[u8]) -> Result<Parsed, String> {
    if raw.is_empty() {
        return Err("manifest is empty".into());
    }
    let nl = raw.iter().position(|&b| b == b'\n').ok_or_else(|| "manifest is truncated (no trailing newline)".to_string())?;
    let header = &raw[..nl];
    let level: u8 = if header == HEADER.as_bytes() {
        1
    } else if header == HEADER_J.as_bytes() {
        2
    } else if header == HEADER_V4.as_bytes() {
        3
    } else {
        return Err("manifest header missing or wrong".into());
    };
    let journal = level >= 2;
    let last_nl = raw.iter().rposition(|&b| b == b'\n').expect("has a newline");
    let valid_len = last_nl + 1;
    if !journal && valid_len != raw.len() {
        return Err("manifest is truncated (no trailing newline)".into());
    }
    let text = std::str::from_utf8(&raw[..valid_len]).map_err(|_| "manifest is not valid UTF-8".to_string())?;
    let mut out = Vec::new();
    for (i, l) in text.lines().skip(1).enumerate() {
        let e: Entry = serde_json::from_str(l).map_err(|e| format!("manifest line {}: {e}", i + 2))?;
        if level < 3 && !e.dicts.is_empty() {
            return Err(format!("manifest line {}: dictionary listing needs the format-4 header", i + 2));
        }
        out.push(e);
    }
    Ok(Parsed { entries: out, journal, level, valid_len })
}

/// Strict parse (no uncommitted tail allowed), used by `verify`.
pub fn parse(raw: &[u8]) -> Result<Vec<Entry>, String> {
    let p = parse_ex(raw)?;
    if p.valid_len != raw.len() {
        return Err("manifest has an uncommitted partial tail (interrupted append; `recover` removes it)".into());
    }
    Ok(p.entries)
}
