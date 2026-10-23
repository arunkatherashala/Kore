//! Data model: tables, cells, typed rows, validation helpers, hashing, time.

use kore_core::{Column, DataBlock, Value};
use sha2::{Digest, Sha256};

// ─── Hash / hex ──────────────────────────────────────────────────────────────

pub fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

pub fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    hex(&h.finalize())
}

// ─── Time (ISO-8601 UTC, second resolution) ──────────────────────────────────

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// Normalise `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS[Z]` to `YYYY-MM-DDTHH:MM:SSZ`.
/// Anything else (offsets, fractions, bad ranges) is rejected so that string
/// order equals time order.
pub fn normalize_ts(s: &str) -> Result<String, String> {
    let b = s.trim().as_bytes();
    let bad = || format!("invalid timestamp '{s}' (want YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ, UTC)");
    let digits = |r: std::ops::Range<usize>| -> Option<i64> {
        let sl = b.get(r)?;
        if sl.iter().all(|c| c.is_ascii_digit()) {
            std::str::from_utf8(sl).ok()?.parse().ok()
        } else {
            None
        }
    };
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return Err(bad());
    }
    let (y, mo, d) = (digits(0..4).ok_or_else(bad)?, digits(5..7).ok_or_else(bad)?, digits(8..10).ok_or_else(bad)?);
    let (mut h, mut mi, mut se) = (0, 0, 0);
    if b.len() > 10 {
        let rest_ok = (b.len() == 19 || (b.len() == 20 && b[19] == b'Z')) && b[10] == b'T' && b[13] == b':' && b[16] == b':';
        if !rest_ok {
            return Err(bad());
        }
        h = digits(11..13).ok_or_else(bad)?;
        mi = digits(14..16).ok_or_else(bad)?;
        se = digits(17..19).ok_or_else(bad)?;
    }
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || se > 59 {
        return Err(bad());
    }
    Ok(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z"))
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ` (civil-from-days algorithm).
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

// ─── Tables and cells ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty {
    S,
    I,
    F,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Table {
    SchemaMeta,
    Events,
    Revisions,
    RevisionCauses,
    Predictions,
    Outcomes,
}

pub const ALL_TABLES: [Table; 6] = [
    Table::SchemaMeta,
    Table::Events,
    Table::Revisions,
    Table::RevisionCauses,
    Table::Predictions,
    Table::Outcomes,
];

impl Table {
    pub fn name(self) -> &'static str {
        match self {
            Table::SchemaMeta => "schema_meta",
            Table::Events => "events",
            Table::Revisions => "revisions",
            Table::RevisionCauses => "revision_causes",
            Table::Predictions => "predictions",
            Table::Outcomes => "outcomes",
        }
    }
    pub fn from_name(n: &str) -> Option<Table> {
        ALL_TABLES.iter().copied().find(|t| t.name() == n)
    }
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn columns(self) -> &'static [(&'static str, Ty)] {
        use Ty::*;
        match self {
            Table::SchemaMeta => &[("format_version", I), ("created_at", S)],
            Table::Events => &[
                ("event_id", S), ("occurred_at", S), ("recorded_at", S), ("source", S),
                ("kind", S), ("text", S), ("tags", S), ("redacted", B),
            ],
            Table::Revisions => &[
                ("rev_id", S), ("entity_type", S), ("key", S), ("stance", S), ("confidence", F),
                ("rationale", S), ("supersedes", S), ("valid_from", S), ("author", S), ("recorded_at", S),
            ],
            Table::RevisionCauses => &[("rev_id", S), ("event_id", S)],
            Table::Predictions => &[
                ("pred_id", S), ("made_at", S), ("question_id", S), ("target", S), ("options", S),
                ("probs", S), ("model_id", S), ("model_version", S), ("features_cite", S), ("resolves_by", S),
            ],
            Table::Outcomes => &[("pred_id", S), ("resolved_at", S), ("actual", S), ("outcome_event_id", S)],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    S(String),
    I(i64),
    F(f64),
    B(bool),
    Null,
}

impl Cell {
    pub fn s(&self) -> String {
        match self {
            Cell::S(x) => x.clone(),
            _ => String::new(),
        }
    }
    pub fn opt_s(&self) -> Option<String> {
        match self {
            Cell::S(x) => Some(x.clone()),
            _ => None,
        }
    }
    fn canon(&self, out: &mut Vec<u8>) {
        match self {
            Cell::S(x) => {
                out.push(b's');
                out.extend((x.len() as u64).to_le_bytes());
                out.extend(x.as_bytes());
            }
            Cell::I(x) => {
                out.push(b'i');
                out.extend(x.to_le_bytes());
            }
            Cell::F(x) => {
                out.push(b'f');
                out.extend(x.to_bits().to_le_bytes());
            }
            Cell::B(x) => {
                out.push(b'b');
                out.push(*x as u8);
            }
            Cell::Null => out.push(b'n'),
        }
    }
}

pub fn canon_rows(rows: &[Vec<Cell>]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in rows {
        for c in r {
            c.canon(&mut out);
        }
        out.push(b'\n');
    }
    out
}

pub fn rows_to_block(table: Table, rows: &[Vec<Cell>]) -> DataBlock {
    let cols = table
        .columns()
        .iter()
        .enumerate()
        .map(|(i, (name, ty))| match ty {
            Ty::S => Column::str_col(name, rows.iter().map(|r| r[i].opt_s()).collect()),
            Ty::I => Column::int64(name, rows.iter().map(|r| if let Cell::I(x) = r[i] { Some(x) } else { None }).collect()),
            Ty::F => Column::float64(name, rows.iter().map(|r| if let Cell::F(x) = r[i] { Some(x) } else { None }).collect()),
            Ty::B => Column::bool_col(name, rows.iter().map(|r| if let Cell::B(x) = r[i] { Some(x) } else { None }).collect()),
        })
        .collect();
    DataBlock::new(cols).expect("equal-length columns")
}

/// Convert a decoded block back to cells, checking the schema exactly.
pub fn block_to_rows(table: Table, block: &DataBlock) -> Result<Vec<Vec<Cell>>, String> {
    let spec = table.columns();
    if block.columns.len() != spec.len() {
        return Err(format!("{}: expected {} columns, found {}", table.name(), spec.len(), block.columns.len()));
    }
    for (c, (name, _)) in block.columns.iter().zip(spec) {
        if c.name != *name {
            return Err(format!("{}: expected column '{}', found '{}'", table.name(), name, c.name));
        }
    }
    let mut rows = Vec::with_capacity(block.num_rows);
    for r in 0..block.num_rows {
        let mut row = Vec::with_capacity(spec.len());
        for (c, (_, ty)) in block.columns.iter().zip(spec) {
            row.push(match (c.data.get_value(r), ty) {
                (Value::Str(s), Ty::S) => Cell::S(s),
                (Value::Int(i), Ty::I) => Cell::I(i),
                (Value::Float(f), Ty::F) => Cell::F(f),
                (Value::Bool(b), Ty::B) => Cell::B(b),
                (Value::Null, _) => Cell::Null,
                (v, t) => return Err(format!("{}: unexpected value {v:?} for {t:?}", table.name())),
            });
        }
        rows.push(row);
    }
    Ok(rows)
}

// ─── Typed rows ──────────────────────────────────────────────────────────────

pub trait Row: Sized {
    const TABLE: Table;
    fn to_cells(&self) -> Vec<Cell>;
    fn from_cells(c: &[Cell]) -> Self;
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventRow {
    pub event_id: String,
    pub occurred_at: String,
    pub recorded_at: String,
    pub source: String,
    pub kind: String,
    pub text: String,
    pub tags: String,
    pub redacted: bool,
}
impl Row for EventRow {
    const TABLE: Table = Table::Events;
    fn to_cells(&self) -> Vec<Cell> {
        vec![
            Cell::S(self.event_id.clone()), Cell::S(self.occurred_at.clone()), Cell::S(self.recorded_at.clone()),
            Cell::S(self.source.clone()), Cell::S(self.kind.clone()), Cell::S(self.text.clone()),
            Cell::S(self.tags.clone()), Cell::B(self.redacted),
        ]
    }
    fn from_cells(c: &[Cell]) -> Self {
        EventRow {
            event_id: c[0].s(), occurred_at: c[1].s(), recorded_at: c[2].s(), source: c[3].s(),
            kind: c[4].s(), text: c[5].s(), tags: c[6].s(), redacted: c[7] == Cell::B(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RevisionRow {
    pub rev_id: String,
    pub entity_type: String,
    pub key: String,
    pub stance: String,
    pub confidence: f64,
    pub rationale: String,
    pub supersedes: Option<String>,
    pub valid_from: String,
    pub author: String,
    pub recorded_at: String,
}
impl Row for RevisionRow {
    const TABLE: Table = Table::Revisions;
    fn to_cells(&self) -> Vec<Cell> {
        vec![
            Cell::S(self.rev_id.clone()), Cell::S(self.entity_type.clone()), Cell::S(self.key.clone()),
            Cell::S(self.stance.clone()), Cell::F(self.confidence), Cell::S(self.rationale.clone()),
            self.supersedes.clone().map(Cell::S).unwrap_or(Cell::Null), Cell::S(self.valid_from.clone()),
            Cell::S(self.author.clone()), Cell::S(self.recorded_at.clone()),
        ]
    }
    fn from_cells(c: &[Cell]) -> Self {
        RevisionRow {
            rev_id: c[0].s(), entity_type: c[1].s(), key: c[2].s(), stance: c[3].s(),
            confidence: if let Cell::F(f) = c[4] { f } else { 0.0 },
            rationale: c[5].s(), supersedes: c[6].opt_s(), valid_from: c[7].s(), author: c[8].s(), recorded_at: c[9].s(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CauseRow {
    pub rev_id: String,
    pub event_id: String,
}
impl Row for CauseRow {
    const TABLE: Table = Table::RevisionCauses;
    fn to_cells(&self) -> Vec<Cell> {
        vec![Cell::S(self.rev_id.clone()), Cell::S(self.event_id.clone())]
    }
    fn from_cells(c: &[Cell]) -> Self {
        CauseRow { rev_id: c[0].s(), event_id: c[1].s() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PredictionRow {
    pub pred_id: String,
    pub made_at: String,
    pub question_id: String,
    pub target: String,
    /// JSON array of strings.
    pub options: String,
    /// JSON array of numbers.
    pub probs: String,
    pub model_id: String,
    pub model_version: String,
    /// JSON array of event ids.
    pub features_cite: String,
    pub resolves_by: Option<String>,
}
impl Row for PredictionRow {
    const TABLE: Table = Table::Predictions;
    fn to_cells(&self) -> Vec<Cell> {
        vec![
            Cell::S(self.pred_id.clone()), Cell::S(self.made_at.clone()), Cell::S(self.question_id.clone()),
            Cell::S(self.target.clone()), Cell::S(self.options.clone()), Cell::S(self.probs.clone()),
            Cell::S(self.model_id.clone()), Cell::S(self.model_version.clone()), Cell::S(self.features_cite.clone()),
            self.resolves_by.clone().map(Cell::S).unwrap_or(Cell::Null),
        ]
    }
    fn from_cells(c: &[Cell]) -> Self {
        PredictionRow {
            pred_id: c[0].s(), made_at: c[1].s(), question_id: c[2].s(), target: c[3].s(), options: c[4].s(),
            probs: c[5].s(), model_id: c[6].s(), model_version: c[7].s(), features_cite: c[8].s(), resolves_by: c[9].opt_s(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeRow {
    pub pred_id: String,
    pub resolved_at: String,
    pub actual: String,
    pub outcome_event_id: Option<String>,
}
impl Row for OutcomeRow {
    const TABLE: Table = Table::Outcomes;
    fn to_cells(&self) -> Vec<Cell> {
        vec![
            Cell::S(self.pred_id.clone()), Cell::S(self.resolved_at.clone()), Cell::S(self.actual.clone()),
            self.outcome_event_id.clone().map(Cell::S).unwrap_or(Cell::Null),
        ]
    }
    fn from_cells(c: &[Cell]) -> Self {
        OutcomeRow { pred_id: c[0].s(), resolved_at: c[1].s(), actual: c[2].s(), outcome_event_id: c[3].opt_s() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaRow {
    pub format_version: i64,
    pub created_at: String,
}
impl Row for MetaRow {
    const TABLE: Table = Table::SchemaMeta;
    fn to_cells(&self) -> Vec<Cell> {
        vec![Cell::I(self.format_version), Cell::S(self.created_at.clone())]
    }
    fn from_cells(c: &[Cell]) -> Self {
        MetaRow { format_version: if let Cell::I(i) = c[0] { i } else { 0 }, created_at: c[1].s() }
    }
}

// ─── Caller inputs ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct NewEvent {
    pub source: String,
    /// Caller-supplied time the event happened (ISO-8601 UTC). Never defaulted to "now".
    pub occurred_at: String,
    /// note | action | decision | outcome | statement
    pub kind: String,
    pub text: String,
    pub tags: Vec<String>,
    pub redacted: bool,
}

#[derive(Debug, Clone, Default)]
pub struct NewRevision {
    /// belief | habit | decision
    pub entity_type: String,
    pub key: String,
    pub stance: String,
    pub confidence: f64,
    pub rationale: String,
    /// None = supersede the current head for (entity_type, key), if any.
    pub supersedes: Option<String>,
    pub valid_from: String,
    /// "user" or "rule:<id>"
    pub author: String,
    pub caused_by: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NewPrediction {
    pub made_at: String,
    pub question_id: String,
    pub target: String,
    pub options: Vec<String>,
    pub probs: Vec<f64>,
    pub model_id: String,
    pub model_version: String,
    pub features_cite: Vec<String>,
    pub resolves_by: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NewOutcome {
    pub pred_id: String,
    pub resolved_at: String,
    pub actual: String,
    pub outcome_event_id: Option<String>,
}

/// Everything committed atomically as one new version.
#[derive(Debug, Clone, Default)]
pub struct Batch {
    pub events: Vec<NewEvent>,
    pub revisions: Vec<NewRevision>,
    pub predictions: Vec<NewPrediction>,
    pub outcomes: Vec<NewOutcome>,
}

pub const EVENT_KINDS: [&str; 5] = ["note", "action", "decision", "outcome", "statement"];
/// `rule` (learner parameter sets and evolution records) is library-only: the
/// tools refuse it, see `log_tools::belief_revise`.
pub const ENTITY_TYPES: [&str; 4] = ["belief", "habit", "decision", "rule"];

/// Event id: hex sha256 of `source|occurred_at|normalised text`
/// (text trimmed, internal whitespace runs collapsed to one space).
pub fn event_id(source: &str, occurred_at: &str, text: &str) -> String {
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    sha256_hex(&[source.as_bytes(), b"|", occurred_at.as_bytes(), b"|", norm.as_bytes()])
}
