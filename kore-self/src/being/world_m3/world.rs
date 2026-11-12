//! World M3: a World T-like world whose values come from one real file (the January 2024 NYC yellow-taxi
//! records, `source.rs`). The pool of files, the task kinds, the schedule, the visible task view and the
//! exact checkers. A pure function of (source SHA-256, `M3Cfg`, `M3_VERSION`) and of (step, world clock).
//! The slices, templates, quirks, rewards and energies are author-written; only the values and their
//! anomalies are real. See docs/m3_design.md.

use super::source::*;
use crate::being::world::{Rng, SECS_PER_ENERGY};
use crate::being::world_t::data::{delim_name, mix, money_canon, rows_for};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Source of every log row of a World M3 being.
pub const SOURCE_T: &str = "being:worldm3";
pub const M3_VERSION: u32 = 1;
pub const SKIP_COST: i64 = 1;
pub const DEFAULT_BUDGET: i64 = 6000;
/// The engine (copied from World T) names the world config `WTCfg`.
pub type WTCfg = M3Cfg;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    ColStat,
    Filter,
    Group,
    Quantile,
    Lookup,
    Anomaly,
    FixRow,
    Format,
    Ts,
}

pub const KINDS: [Kind; 9] = [Kind::ColStat, Kind::Filter, Kind::Group, Kind::Quantile, Kind::Lookup, Kind::Anomaly, Kind::FixRow, Kind::Format, Kind::Ts];

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::ColStat => "colstat",
            Kind::Filter => "filter",
            Kind::Group => "group",
            Kind::Quantile => "quantile",
            Kind::Lookup => "lookup",
            Kind::Anomaly => "anomaly",
            Kind::FixRow => "fix_row",
            Kind::Format => "format",
            Kind::Ts => "ts",
        }
    }
    pub fn idx(self) -> usize {
        KINDS.iter().position(|k| *k == self).unwrap()
    }
    pub fn tmpls(self) -> &'static [Tmpl] {
        use Tmpl::*;
        match self {
            Kind::ColStat => &[ColSum, ColMax, ColCount, ColMean, ColDistinct],
            Kind::Filter => &[FltCount, FltSum],
            Kind::Group => &[GrpTop, GrpTop3],
            Kind::Quantile => &[QMedian, QP90],
            Kind::Lookup => &[Lookup],
            Kind::Anomaly => &[AnFareNeg, AnDistZero, AnTotalMismatch, AnOutDay, AnOutMonth, AnFirstBack],
            Kind::FixRow => &[FixLine],
            Kind::Format => &[FormatOf],
            Kind::Ts => &[TsNext],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tmpl {
    ColSum,
    ColMax,
    ColCount,
    ColMean,
    ColDistinct,
    FltCount,
    FltSum,
    GrpTop,
    GrpTop3,
    QMedian,
    QP90,
    Lookup,
    AnFareNeg,
    AnDistZero,
    AnTotalMismatch,
    AnOutDay,
    AnOutMonth,
    AnFirstBack,
    FixLine,
    FormatOf,
    TsNext,
}

pub const TMPLS: [Tmpl; 21] = [
    Tmpl::ColSum, Tmpl::ColMax, Tmpl::ColCount, Tmpl::ColMean, Tmpl::ColDistinct, Tmpl::FltCount, Tmpl::FltSum, Tmpl::GrpTop, Tmpl::GrpTop3, Tmpl::QMedian, Tmpl::QP90, Tmpl::Lookup, Tmpl::AnFareNeg,
    Tmpl::AnDistZero, Tmpl::AnTotalMismatch, Tmpl::AnOutDay, Tmpl::AnOutMonth, Tmpl::AnFirstBack, Tmpl::FixLine, Tmpl::FormatOf, Tmpl::TsNext,
];

impl Tmpl {
    pub fn idx(self) -> usize {
        TMPLS.iter().position(|t| *t == self).unwrap()
    }
    pub fn name(self) -> &'static str {
        match self {
            Tmpl::ColSum => "col_sum",
            Tmpl::ColMax => "col_max",
            Tmpl::ColCount => "col_count",
            Tmpl::ColMean => "col_mean",
            Tmpl::ColDistinct => "col_distinct",
            Tmpl::FltCount => "flt_count",
            Tmpl::FltSum => "flt_sum",
            Tmpl::GrpTop => "grp_top",
            Tmpl::GrpTop3 => "grp_top3",
            Tmpl::QMedian => "q_median",
            Tmpl::QP90 => "q_p90",
            Tmpl::Lookup => "lookup",
            Tmpl::AnFareNeg => "an_fare_neg",
            Tmpl::AnDistZero => "an_dist_zero",
            Tmpl::AnTotalMismatch => "an_total_mismatch",
            Tmpl::AnOutDay => "an_out_day",
            Tmpl::AnOutMonth => "an_out_month",
            Tmpl::AnFirstBack => "an_first_back",
            Tmpl::FixLine => "fix_line",
            Tmpl::FormatOf => "format_of",
            Tmpl::TsNext => "ts_next",
        }
    }
    pub fn parse(s: &str) -> Option<Tmpl> {
        TMPLS.iter().copied().find(|t| t.name() == s)
    }
    pub fn kind(self) -> Kind {
        *KINDS.iter().find(|k| k.tmpls().contains(&self)).unwrap()
    }
    pub fn base_reward(self) -> i64 {
        match self.kind() {
            Kind::ColStat | Kind::Lookup => 6,
            Kind::Filter | Kind::Group | Kind::Anomaly | Kind::FixRow => 9,
            Kind::Quantile => 8,
            Kind::Ts => 7,
            Kind::Format => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Plan {
    NaiveNative,
    SniffNative,
    NaiveSql,
    SniffSql,
    NativeFilter,
    TsLast,
    TsDrift,
    TsSeason7,
    TsSeason24,
    TsMean,
    TsRule,
    FixFields,
    FixTypes,
    FmtExt,
    FmtSniff,
}

pub const PLANS: [Plan; 15] = [
    Plan::NaiveNative, Plan::SniffNative, Plan::NaiveSql, Plan::SniffSql, Plan::NativeFilter, Plan::TsLast, Plan::TsDrift, Plan::TsSeason7, Plan::TsSeason24, Plan::TsMean, Plan::TsRule, Plan::FixFields, Plan::FixTypes,
    Plan::FmtExt, Plan::FmtSniff,
];

impl Plan {
    pub fn idx(self) -> usize {
        PLANS.iter().position(|p| *p == self).unwrap()
    }
    pub fn name(self) -> &'static str {
        match self {
            Plan::NaiveNative => "naive+native",
            Plan::SniffNative => "sniff+native",
            Plan::NaiveSql => "naive+sql",
            Plan::SniffSql => "sniff+sql",
            Plan::NativeFilter => "native-filter",
            Plan::TsLast => "ts-last",
            Plan::TsDrift => "ts-drift",
            Plan::TsSeason7 => "ts-season7",
            Plan::TsSeason24 => "ts-season24",
            Plan::TsMean => "ts-mean7",
            Plan::TsRule => "ts-rule",
            Plan::FixFields => "fix-fieldcount",
            Plan::FixTypes => "fix-typecheck",
            Plan::FmtExt => "fmt-extension",
            Plan::FmtSniff => "fmt-sniff",
        }
    }
    pub fn parse(s: &str) -> Option<Plan> {
        PLANS.iter().copied().find(|p| p.name() == s)
    }
    pub fn uses_sql(self) -> bool {
        matches!(self, Plan::NaiveSql | Plan::SniffSql)
    }
    pub fn listed(self, v: &ViewT) -> i64 {
        scale_energy(self.energy(v.rows), v.cost_pm)
    }
    /// World T's energy formulas by rows; `native-filter` is priced as the sniffing loader plus a scan.
    pub fn energy(self, rows: usize) -> i64 {
        let r = rows as i64;
        match self {
            Plan::NaiveNative => 1 + r / 300,
            Plan::SniffNative | Plan::NativeFilter => 2 + r / 200,
            Plan::NaiveSql => 2 + r / 200,
            Plan::SniffSql => 3 + r / 150,
            Plan::TsLast | Plan::TsDrift | Plan::TsSeason7 | Plan::TsSeason24 | Plan::TsMean | Plan::TsRule => 1,
            Plan::FixFields => 1 + r / 300,
            Plan::FixTypes => 2 + r / 120,
            Plan::FmtExt => 1,
            Plan::FmtSniff => 2,
        }
    }
}

/// Plans that apply to a template (public). `TsRule` only when the being holds a rule for the signature.
pub fn plans_for(t: Tmpl) -> &'static [Plan] {
    use Plan::*;
    match t.kind() {
        Kind::ColStat | Kind::Lookup => &[NaiveNative, SniffNative, NaiveSql, SniffSql],
        Kind::Filter | Kind::Anomaly => &[NaiveSql, SniffSql, NativeFilter],
        Kind::Group => &[NaiveSql, SniffSql],
        Kind::Quantile => &[SniffNative, SniffSql],
        Kind::FixRow => &[FixFields, FixTypes],
        Kind::Format => &[FmtExt, FmtSniff],
        Kind::Ts => &[TsLast, TsDrift, TsSeason7, TsSeason24, TsMean, TsRule],
    }
}

/// Contexts (visible in the file name and the task).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ctx {
    Day,
    Vendor,
    Zone,
    Night,
    Airport,
    Misc,
    HourCity,
    HourAirport,
    HourZones,
    DailyV1,
    DailyV2,
}

pub const CTXS: [Ctx; 11] = [Ctx::Day, Ctx::Vendor, Ctx::Zone, Ctx::Night, Ctx::Airport, Ctx::Misc, Ctx::HourCity, Ctx::HourAirport, Ctx::HourZones, Ctx::DailyV1, Ctx::DailyV2];
pub const TABLE_CTXS: [Ctx; 6] = [Ctx::Day, Ctx::Vendor, Ctx::Zone, Ctx::Night, Ctx::Airport, Ctx::Misc];
pub const TS_CTXS: [Ctx; 5] = [Ctx::HourCity, Ctx::HourAirport, Ctx::HourZones, Ctx::DailyV1, Ctx::DailyV2];

impl Ctx {
    pub fn name(self) -> &'static str {
        match self {
            Ctx::Day => "day",
            Ctx::Vendor => "vendor",
            Ctx::Zone => "zone",
            Ctx::Night => "night",
            Ctx::Airport => "airport",
            Ctx::Misc => "misc",
            Ctx::HourCity => "hourly_city",
            Ctx::HourAirport => "hourly_airport",
            Ctx::HourZones => "hourly_zones",
            Ctx::DailyV1 => "daily_vendor1",
            Ctx::DailyV2 => "daily_vendor2",
        }
    }
    pub fn idx(self) -> usize {
        CTXS.iter().position(|c| *c == self).unwrap()
    }
    pub fn parse(s: &str) -> Option<Ctx> {
        CTXS.iter().copied().find(|c| c.name() == s)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct M3Cfg {
    pub seed: u64,
    pub budget: i64,
    pub start_kinds: usize,
    pub intro_every: i64,
    /// Files per context and pool (the data pool has three times as many).
    pub files: u64,
    /// Out-of-distribution world (never tuned): size class 3 (1000 to 2500 rows), columns reversed.
    pub ood: bool,
    pub penalty_pm: u32,
    pub cost_pm: u32,
}

impl M3Cfg {
    pub fn new(seed: u64, budget: i64) -> M3Cfg {
        M3Cfg { seed, budget, start_kinds: 3, intro_every: (budget / 10).max(1), files: 6, ood: false, penalty_pm: 0, cost_pm: 1000 }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=1_000_000).contains(&self.budget) {
            return Err("budget must be 1..=1000000".into());
        }
        if !(1..=9).contains(&self.start_kinds) || self.intro_every < 1 || !(1..=20).contains(&self.files) {
            return Err("start_kinds 1..=9, intro_every >= 1, files 1..=20".into());
        }
        if self.penalty_pm > 20_000 || !(100..=10_000).contains(&self.cost_pm) {
            return Err("penalty_pm must be 0..=20000 and cost_pm 100..=10000".into());
        }
        Ok(())
    }
    pub fn tags(&self) -> Vec<String> {
        vec![
            format!("m3_version:{M3_VERSION}"),
            format!("m3_source:{}", &SOURCE_SHA256[..16]),
            format!("m3_seed:{}", self.seed),
            format!("m3_budget:{}", self.budget),
            format!("m3_start:{}", self.start_kinds),
            format!("m3_intro:{}", self.intro_every),
            format!("m3_files:{}", self.files),
            format!("m3_ood:{}", self.ood as u8),
            format!("m3_penalty_pm:{}", self.penalty_pm),
            format!("m3_cost_pm:{}", self.cost_pm),
        ]
    }
    pub fn from_tags(tags: &str) -> Option<M3Cfg> {
        let g = |k: &str| crate::being::state::tag(tags, k).and_then(|x| x.parse::<i64>().ok());
        if g("m3_version")? != M3_VERSION as i64 || crate::being::state::tag(tags, "m3_source")? != &SOURCE_SHA256[..16] {
            return None;
        }
        let c = M3Cfg { seed: g("m3_seed")? as u64, budget: g("m3_budget")?, start_kinds: g("m3_start")? as usize, intro_every: g("m3_intro")?, files: g("m3_files")? as u64, ood: g("m3_ood")? == 1, penalty_pm: g("m3_penalty_pm")? as u32, cost_pm: g("m3_cost_pm")? as u32 };
        c.validate().ok().map(|_| c)
    }
}

/// How one file is rendered (author-defined quirks; values are never altered).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Render {
    pub delim: u8,
    pub decimal_comma: bool,
    pub null_tok: &'static str,
    pub crlf: bool,
    pub pad: bool,
    pub bom: bool,
    pub header: bool,
    /// Timestamps as `MM/DD/YYYY hh:mm:ss AM` instead of ISO.
    pub us_ts: bool,
}

pub const NULL_TOKS: [&str; 4] = ["", "NA", "NULL", "-"];

#[derive(Debug, Clone, Copy, Default)]
struct Profile {
    alt_delim: u32,
    decimal_comma: u32,
    crlf: u32,
    pad: u32,
    bom: u32,
    us_ts: u32,
    no_header: u32,
    null_pref: usize,
    bad_kinds: [u32; 4],
}

fn profile(seed: u64, ctx: Ctx) -> Profile {
    let mut r = Rng::new(mix(&[seed, 0x9F0F, 0x3300 + ctx.idx() as u64]));
    let mut p = |lo: i64, hi: i64| r.range(lo, hi) as u32;
    let mut pr = match ctx {
        Ctx::Day => Profile { alt_delim: p(0, 150), crlf: p(0, 200), bom: p(0, 100), us_ts: p(0, 150), pad: p(0, 100), null_pref: 0, ..Default::default() },
        Ctx::Vendor => Profile { alt_delim: p(300, 700), decimal_comma: p(400, 800), us_ts: p(200, 500), null_pref: 2, ..Default::default() },
        Ctx::Zone => Profile { alt_delim: p(100, 400), pad: p(200, 500), bom: p(100, 300), null_pref: 1, ..Default::default() },
        Ctx::Night => Profile { crlf: p(300, 700), us_ts: p(400, 800), null_pref: 3, ..Default::default() },
        Ctx::Airport => Profile { alt_delim: p(200, 500), decimal_comma: p(300, 600), pad: p(0, 200), bom: p(200, 400), null_pref: 0, ..Default::default() },
        _ => Profile { alt_delim: p(0, 300), crlf: p(0, 300), us_ts: p(100, 400), pad: p(100, 300), null_pref: 1, ..Default::default() },
    };
    pr.no_header = p(150, 350);
    pr.bad_kinds = match ctx {
        Ctx::Vendor | Ctx::Zone => [p(30, 60), p(10, 30), p(10, 30), p(0, 20)],
        Ctx::Day | Ctx::Night => [p(0, 10), p(10, 30), p(10, 30), p(30, 60)],
        _ => [p(0, 10), p(20, 40), p(30, 60), p(10, 30)],
    };
    pr
}

fn draw_render(r: &mut Rng, p: &Profile, allow_no_header: bool) -> Render {
    let mut ch = |pm: u32| (r.below(1000) as u32) < pm;
    let alt = ch(p.alt_delim);
    let decimal_comma_draw = ch(p.decimal_comma);
    let crlf = ch(p.crlf);
    let pad = ch(p.pad);
    let bom = ch(p.bom);
    let us_ts = ch(p.us_ts);
    let header = !(allow_no_header && ch(p.no_header));
    let main_tok = ch(800);
    let delim = if alt { [b';', b'\t', b'|'][r.below(3) as usize] } else { b',' };
    let null_tok = if main_tok { NULL_TOKS[p.null_pref] } else { NULL_TOKS[r.below(4) as usize] };
    Render { delim, decimal_comma: delim == b';' && decimal_comma_draw, null_tok, crlf, pad, bom, header, us_ts }
}

/// The extension a delimiter is written with (`;` files are legacy `.csv`).
pub fn ext_of(d: u8) -> &'static str {
    match d {
        b'\t' => "tsv",
        b'|' => "txt",
        _ => "csv",
    }
}

/// A table file, logically: its rows (trip ids) and columns (base column indexes, `C_TRIP` included).
#[derive(Debug, Clone, PartialEq)]
pub struct TableSpec {
    /// The January day of the slice.
    pub day: i64,
    pub rows: Vec<u32>,
    pub cols: Vec<usize>,
    pub rd: Render,
    /// 1-based physical line of the injected malformed line (fix pool), and its kind.
    pub bad: Option<(usize, u8)>,
    /// What the slice is (for the cross-check and the reports; never shown to the being).
    pub slice: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Logical {
    Table(TableSpec),
    /// Counts per hour (744) or per day (31), from the base table.
    Series { values: Vec<i64>, hourly: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileT {
    pub name: String,
    pub ctx: Ctx,
    pub kind_pool: &'static str,
    pub size: u8,
    pub bytes: Vec<u8>,
    pub logical: Logical,
}

/// What the being sees of a task (same shape as World T's view).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewT {
    pub step: u64,
    pub id: String,
    pub tmpl: Tmpl,
    pub ctx: Ctx,
    pub file: String,
    pub rows: usize,
    pub size: u8,
    pub reward: i64,
    pub param: String,
    /// (value column, category column, key column, operator / second column)
    pub cols: [String; 4],
    pub prefix: usize,
    pub tol: i64,
    pub prompt: String,
    pub penalty_pm: u32,
    pub cost_pm: u32,
}

impl ViewT {
    pub fn penalty(&self) -> f64 {
        self.reward as f64 * self.penalty_pm as f64 / 1000.0
    }
}

pub fn scale_energy(e: i64, cost_pm: u32) -> i64 {
    if e <= 1 {
        return e.max(0);
    }
    1 + ((e - 1) * cost_pm as i64 + 500) / 1000
}

#[derive(Debug, Clone, PartialEq)]
pub enum Truth {
    Exact(String),
    /// A value in cents: any correct rounding of sum / n (a mean, or a median of an even count with n = 2).
    Mean { sum: i64, n: i64 },
    Tol { value: i64, tol: i64 },
}

impl Truth {
    /// A canonical text of the truth (cross-check export).
    pub fn text(&self) -> String {
        match self {
            Truth::Exact(s) => format!("exact:{s}"),
            Truth::Mean { sum, n } => format!("mean:{sum}/{n}"),
            Truth::Tol { value, tol } => format!("tol:{value}/{tol}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskT {
    pub view: ViewT,
    pub truth: Truth,
    pub file_idx: usize,
}

impl TaskT {
    pub fn check(&self, ans: &str) -> bool {
        match &self.truth {
            Truth::Exact(s) => ans == s,
            Truth::Mean { sum, n } => crate::being::world_t::world::parse_cents(ans).map_or(false, |c| 2 * (c as i128 * *n as i128 - *sum as i128).abs() <= *n as i128),
            Truth::Tol { value, tol } => ans.parse::<i64>().map_or(false, |a| (a - value).abs() <= *tol),
        }
    }
    pub fn revealed(&self) -> Option<i64> {
        match &self.truth {
            Truth::Tol { value, .. } => Some(*value),
            _ => None,
        }
    }
}

/// Render one value of column `c`.
pub fn field(c: usize, v: Option<i64>, rd: &Render) -> String {
    let Some(v) = v else { return rd.null_tok.to_string() };
    match col_type(c) {
        CT::Int => v.to_string(),
        CT::Flag => if v == 1 { "Y" } else { "N" }.to_string(),
        CT::Cents => crate::being::world_t::data::money_str(v, false, rd.decimal_comma),
        CT::Ts => if rd.us_ts { us_ts(v) } else { iso_ts(v) },
    }
}

fn render_table(base: &Base, t: &TableSpec, bad_kind: Option<(usize, u8)>) -> (Vec<u8>, Option<usize>) {
    let rd = &t.rd;
    let d = rd.delim as char;
    let sep = if rd.pad { format!("{d} ") } else { d.to_string() };
    let nl = if rd.crlf { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = vec![];
    if rd.header {
        lines.push(t.cols.iter().map(|c| col_name(*c)).collect::<Vec<_>>().join(&sep));
    }
    let mut bad_line = None;
    for (i, r) in t.rows.iter().enumerate() {
        let mut fs: Vec<String> = t.cols.iter().map(|c| field(*c, base.get(*c, *r), rd)).collect();
        if let Some((row, kind)) = bad_kind.filter(|b| b.0 == i) {
            let pos = |c: usize| t.cols.iter().position(|x| *x == c).unwrap_or(1);
            match kind {
                0 => {
                    let j = pos(C_PU);
                    fs[j] = format!("{}{d}x", fs[j]);
                }
                1 => {
                    fs.pop();
                }
                2 => {
                    let j = pos(C_FARE);
                    fs[j] = format!("{}O", fs[j]);
                }
                _ => {
                    // an impossible calendar date in the pickup timestamp (day 32), time kept
                    let j = pos(C_PICKUP);
                    let v = base.get(C_PICKUP, t.rows[row]).unwrap_or(JAN1);
                    let tm = iso_ts(v)[11..].to_string();
                    fs[j] = if rd.us_ts { format!("01/32/2024 {}", &us_ts(v)[11..]) } else { format!("2024-01-32 {tm}") };
                }
            }
            bad_line = Some(lines.len() + 1);
        }
        lines.push(fs.join(&sep));
    }
    let mut s = lines.join(nl);
    s.push_str(nl);
    let mut out = vec![];
    if rd.bom {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    out.extend_from_slice(s.as_bytes());
    (out, bad_line)
}

fn render_series(values: &[i64], hourly: bool) -> Vec<u8> {
    let mut s = String::from(if hourly { "hour,trips\n" } else { "day,trips\n" });
    for (i, x) in values.iter().enumerate() {
        let k = if hourly { iso_ts(JAN1 + i as i64 * 3600) } else { jan_date(i as i64 + 1) };
        s.push_str(&format!("{k},{x}\n"));
    }
    s.into_bytes()
}

/// Columns that hold hundredths (money, distance) and the integer category columns.
pub fn is_value_col(c: usize) -> bool {
    c != C_TRIP && col_type(c) == CT::Cents
}
pub fn is_cat_col(c: usize) -> bool {
    c != C_TRIP && col_type(c) == CT::Int
}

pub struct WorldT {
    pub cfg: M3Cfg,
    pub base: Arc<Base>,
    pub files: Vec<FileT>,
    pub order: Vec<Kind>,
    /// The world's seeded set of 20 pickup zones (context `zone` and the series `hourly_zones`).
    pub zones: Vec<i64>,
    by_kind: Vec<Vec<usize>>,
}

impl WorldT {
    pub fn new(cfg: M3Cfg, base: Arc<Base>) -> WorldT {
        let seed = cfg.seed;
        let mut zr = Rng::new(mix(&[seed, 0x20E5]));
        let mut zones: Vec<i64> = vec![];
        while zones.len() < 20 {
            let z = zr.range(1, 263);
            if !zones.contains(&z) && !AIRPORT_ZONES.contains(&z) {
                zones.push(z);
            }
        }
        zones.sort();
        let mut w = WorldT { cfg, base, files: vec![], order: vec![], zones, by_kind: vec![vec![]; KINDS.len()] };
        let table_kinds = [Kind::ColStat, Kind::Filter, Kind::Group, Kind::Quantile, Kind::Lookup, Kind::Anomaly];
        for ctx in TABLE_CTXS {
            let p = profile(seed, ctx);
            for i in 0..w.cfg.files * 3 {
                w.add_table(ctx, &p, "t", i, &table_kinds);
            }
            for i in 0..w.cfg.files {
                w.add_table(ctx, &p, "fix", i, &[Kind::FixRow]);
            }
            for i in 0..w.cfg.files {
                w.add_table(ctx, &p, "raw", i, &[Kind::Format]);
            }
        }
        // the series, computed from the base table
        let b = w.base.clone();
        let mut hc = vec![0i64; 744];
        let mut ha = vec![0i64; 744];
        let mut hz = vec![0i64; 744];
        let mut dv = [vec![0i64; 31], vec![0i64; 31]];
        let zs: BTreeSet<i64> = w.zones.iter().copied().collect();
        for d in 1..=31usize {
            for &r in &b.by_day[d] {
                let t = b.get(C_PICKUP, r).unwrap();
                let h = ((t - JAN1) / 3600) as usize;
                hc[h] += 1;
                let pu = b.get(C_PU, r).unwrap_or(-1);
                if AIRPORT_ZONES.contains(&pu) {
                    ha[h] += 1;
                }
                if zs.contains(&pu) {
                    hz[h] += 1;
                }
                match b.get(C_VENDOR, r) {
                    Some(1) => dv[0][d - 1] += 1,
                    Some(2) => dv[1][d - 1] += 1,
                    _ => {}
                }
            }
        }
        for (ctx, values, hourly) in [(Ctx::HourCity, hc, true), (Ctx::HourAirport, ha, true), (Ctx::HourZones, hz, true), (Ctx::DailyV1, dv[0].clone(), false), (Ctx::DailyV2, dv[1].clone(), false)] {
            let bytes = render_series(&values, hourly);
            w.by_kind[Kind::Ts.idx()].push(w.files.len());
            w.files.push(FileT { name: format!("{}.txt", ctx.name()), ctx, kind_pool: "series", size: if hourly { 1 } else { 0 }, bytes, logical: Logical::Series { values, hourly } });
        }
        let mut order = KINDS.to_vec();
        let mut r = Rng::new(mix(&[seed, 0x0DE5, 0x33]));
        for i in (1..order.len()).rev() {
            let j = r.below(i as u64 + 1) as usize;
            order.swap(i, j);
        }
        w.order = order;
        w
    }

    fn slice_rows(&self, ctx: Ctx, r: &mut Rng) -> (i64, Vec<u32>, String) {
        let b = &self.base;
        let mut best: Option<(i64, Vec<u32>, String)> = None;
        for _ in 0..300 {
            let d = r.range(1, 31);
            let day = &b.by_day[d as usize];
            let (rows, what): (Vec<u32>, String) = match ctx {
                Ctx::Day => (day.clone(), format!("day {d}")),
                Ctx::Vendor => {
                    let v = 1 + r.below(2);
                    (day.iter().copied().filter(|x| b.get(C_VENDOR, *x) == Some(v)).collect(), format!("day {d}, vendor {v}"))
                }
                Ctx::Zone => (day.iter().copied().filter(|x| b.get(C_PU, *x).map_or(false, |z| self.zones.contains(&z))).collect(), format!("day {d}, zone set")),
                Ctx::Night => (day.iter().copied().filter(|x| b.get(C_PICKUP, *x).map_or(false, |t| (t - JAN1).rem_euclid(DAY) < 6 * 3600)).collect(), format!("day {d}, 00:00-05:59")),
                Ctx::Airport => (day.iter().copied().filter(|x| b.get(C_PU, *x).map_or(false, |z| AIRPORT_ZONES.contains(&z))).collect(), format!("day {d}, airports")),
                _ => (day.clone(), format!("day {d} sample plus every out-of-month and dropoff-before-pickup row")),
            };
            if rows.len() >= 40 {
                return (d, rows, what);
            }
            if best.as_ref().map_or(true, |x| rows.len() > x.1.len()) {
                best = Some((d, rows, what));
            }
        }
        // (only a small hand-made table gets here) the largest slice seen
        best.unwrap()
    }

    fn add_table(&mut self, ctx: Ctx, p: &Profile, pool: &'static str, i: u64, kinds: &[Kind]) {
        let seed = self.cfg.seed;
        let pool_code = match pool {
            "t" => 0u64,
            "fix" => 1,
            _ => 2,
        };
        let code = 0x1000 * ctx.idx() as u64 + 0x100 * pool_code + i;
        let mut r = Rng::new(mix(&[seed, 0xF11E, code]));
        let size = if self.cfg.ood { 3 } else if pool == "raw" { 0 } else if pool == "fix" { r.below(2) as u8 } else { r.below(3) as u8 };
        let (day, cand, what) = self.slice_rows(ctx, &mut r);
        let n = rows_for(&mut r, size).min(cand.len());
        let mut keyed: Vec<(u64, u32)> = cand.iter().map(|t| (mix(&[seed, code, *t as u64]), *t)).collect();
        if n < keyed.len() {
            keyed.select_nth_unstable(n);
            keyed.truncate(n);
        }
        let mut rows: Vec<u32> = keyed.into_iter().map(|x| x.1).collect();
        if ctx == Ctx::Misc {
            for x in self.base.out_month.iter().chain(self.base.backwards.iter()) {
                if !rows.contains(x) {
                    rows.push(*x);
                }
            }
        }
        let b = &self.base;
        rows.sort_by_key(|x| (b.get(C_PICKUP, *x).unwrap_or(i64::MIN), *x));
        // columns: trip_id first, then a seeded subset of 7 to 12 others in file order
        let mut set: BTreeSet<usize> = [C_PICKUP, C_PU, C_FARE].into_iter().collect();
        let full_money = r.below(3) == 0;
        let k = if full_money {
            set.extend(COMPONENTS);
            set.insert(C_TOTAL);
            r.range(11, 12) as usize
        } else {
            r.range(7, 12) as usize
        };
        while set.len() < k {
            set.insert(r.below(19) as usize);
        }
        let mut cols: Vec<usize> = vec![C_TRIP];
        cols.extend(set);
        if self.cfg.ood {
            cols.reverse();
        }
        let rd = draw_render(&mut r, p, pool == "raw");
        let mut spec = TableSpec { day, rows, cols, rd, bad: None, slice: what };
        let bad = if pool == "fix" {
            let w = p.bad_kinds;
            let tot: u32 = w.iter().sum::<u32>().max(1);
            let mut x = r.below(tot as u64) as u32;
            let mut kind = 0u8;
            for (j, wj) in w.iter().enumerate() {
                if x < *wj {
                    kind = j as u8;
                    break;
                }
                x -= wj;
            }
            let row = r.range(spec.rows.len() as i64 / 5, spec.rows.len() as i64 - 1) as usize;
            Some((row, kind))
        } else {
            None
        };
        let (bytes, line) = render_table(&self.base, &spec, bad);
        spec.bad = line.zip(bad.map(|b| b.1));
        let ext = ext_of(spec.rd.delim);
        let name = format!("{}_{}{:02}.{ext}", ctx.name(), pool, i);
        for k in kinds {
            self.by_kind[k.idx()].push(self.files.len());
        }
        self.files.push(FileT { name, ctx, kind_pool: pool, size, bytes, logical: Logical::Table(spec) });
    }

    pub fn open_kinds(&self, clock: i64) -> Vec<Kind> {
        let energy = clock / SECS_PER_ENERGY;
        let n = (self.cfg.start_kinds as i64 + energy / self.cfg.intro_every).min(KINDS.len() as i64) as usize;
        self.order[..n].to_vec()
    }

    pub fn intro_energy(&self, i: usize) -> i64 {
        if i < self.cfg.start_kinds { 0 } else { (i - self.cfg.start_kinds + 1) as i64 * self.cfg.intro_every }
    }

    pub fn file(&self, name: &str) -> Option<&FileT> {
        self.files.iter().find(|f| f.name == name)
    }

    /// sha256 over every file name and its bytes, in pool order (the reproducibility digest).
    pub fn pool_digest(&self) -> String {
        let mut parts: Vec<&[u8]> = vec![];
        for f in &self.files {
            parts.push(f.name.as_bytes());
            parts.push(&f.bytes);
        }
        crate::versioned::sha256_hex(&parts)
    }

    /// The task of a step: the kind is drawn from the open kinds; the task of (step, kind) does not depend on
    /// the clock (so the cross-check can enumerate every task a life can meet).
    pub fn task(&self, step: u64, clock: i64) -> TaskT {
        let open = self.open_kinds(clock);
        let mut r = Rng::new(mix(&[self.cfg.seed, 0x7A5C, step]));
        let kind = open[r.below(open.len() as u64) as usize];
        self.task_of(step, kind)
    }

    pub fn task_of(&self, step: u64, kind: Kind) -> TaskT {
        let mut r = Rng::new(mix(&[self.cfg.seed, 0x3A5D, step, kind.idx() as u64]));
        let ts = kind.tmpls();
        let tmpl = ts[r.below(ts.len() as u64) as usize];
        let pool = &self.by_kind[kind.idx()];
        for _ in 0..40 {
            let fi = pool[r.below(pool.len() as u64) as usize];
            if let Some(t) = self.make_task(step, tmpl, fi, &mut r) {
                return t;
            }
        }
        // no file supports the template unambiguously: a row count (always defined)
        let fi = pool[r.below(pool.len() as u64) as usize];
        self.make_task(step, Tmpl::ColCount, fi, &mut r).unwrap()
    }

    fn make_task(&self, step: u64, tmpl: Tmpl, fi: usize, r: &mut Rng) -> Option<TaskT> {
        let f = &self.files[fi];
        let b = &self.base;
        let mut cols: [String; 4] = Default::default();
        let mut param = String::new();
        let (mut prefix, mut tol) = (0usize, 0i64);
        let rows;
        let truth = match &f.logical {
            Logical::Series { values, hourly } => {
                prefix = if *hourly { r.range(168, 743) } else { r.range(14, 30) } as usize;
                rows = prefix;
                let v = values[prefix];
                tol = 3.max((v as f64 * 0.05).round() as i64);
                cols[0] = "trips".into();
                Truth::Tol { value: v, tol }
            }
            Logical::Table(t) => {
                rows = t.rows.len();
                let has = |c: usize| t.cols.contains(&c);
                let vals = |c: usize| -> Vec<Option<i64>> { t.rows.iter().map(|x| b.get(c, *x)).collect() };
                let nn = |c: usize| t.rows.iter().any(|x| b.get(c, *x).is_some());
                let pick_col = |r: &mut Rng, p: &dyn Fn(usize) -> bool| -> Option<usize> {
                    let cs: Vec<usize> = t.cols.iter().copied().filter(|c| p(*c) && nn(*c)).collect();
                    (!cs.is_empty()).then(|| cs[r.below(cs.len() as u64) as usize])
                };
                let row_with = |r: &mut Rng, c: usize| -> Option<u32> {
                    let rs: Vec<u32> = t.rows.iter().copied().filter(|x| b.get(c, *x).is_some()).collect();
                    (!rs.is_empty()).then(|| rs[r.below(rs.len() as u64) as usize])
                };
                let name = |c: usize| col_name(c).to_string();
                let count = |p: &dyn Fn(u32) -> bool| Truth::Exact(t.rows.iter().filter(|x| p(**x)).count().to_string());
                match tmpl {
                    Tmpl::ColSum | Tmpl::ColMax | Tmpl::ColMean | Tmpl::QMedian | Tmpl::QP90 => {
                        let c = pick_col(r, &is_value_col)?;
                        cols[0] = name(c);
                        let mut xs: Vec<i64> = vals(c).into_iter().flatten().collect();
                        match tmpl {
                            Tmpl::ColSum => Truth::Exact(money_canon(xs.iter().sum())),
                            Tmpl::ColMax => Truth::Exact(money_canon(*xs.iter().max()?)),
                            Tmpl::ColMean => Truth::Mean { sum: xs.iter().sum(), n: xs.len() as i64 },
                            Tmpl::QMedian => {
                                xs.sort();
                                let n = xs.len();
                                if n % 2 == 1 { Truth::Mean { sum: xs[n / 2], n: 1 } } else { Truth::Mean { sum: xs[n / 2 - 1] + xs[n / 2], n: 2 } }
                            }
                            _ => {
                                xs.sort();
                                let k = (xs.len() * 9 + 9) / 10;
                                Truth::Exact(money_canon(xs[k.max(1) - 1]))
                            }
                        }
                    }
                    Tmpl::ColCount => Truth::Exact(t.rows.len().to_string()),
                    Tmpl::ColDistinct => {
                        let c = pick_col(r, &|c| is_cat_col(c) || col_type(c) == CT::Flag)?;
                        cols[1] = name(c);
                        let s: BTreeSet<i64> = vals(c).into_iter().flatten().collect();
                        Truth::Exact(s.len().to_string())
                    }
                    Tmpl::FltCount => {
                        if r.below(2) == 0 {
                            let c = pick_col(r, &is_value_col)?;
                            let k = b.get(c, row_with(r, c)?)?;
                            cols[0] = name(c);
                            cols[3] = ">".into();
                            param = money_canon(k);
                            count(&|x| b.get(c, x).map_or(false, |v| v > k))
                        } else {
                            let c = pick_col(r, &is_cat_col)?;
                            let k = b.get(c, row_with(r, c)?)?;
                            cols[0] = name(c);
                            cols[3] = "=".into();
                            param = k.to_string();
                            count(&|x| b.get(c, x) == Some(k))
                        }
                    }
                    Tmpl::FltSum | Tmpl::GrpTop => {
                        let m = pick_col(r, &|c| is_value_col(c) && c != C_DIST)?;
                        let c = pick_col(r, &is_cat_col)?;
                        cols[0] = name(m);
                        cols[1] = name(c);
                        if tmpl == Tmpl::FltSum {
                            let rows_both: Vec<u32> = t.rows.iter().copied().filter(|x| b.get(m, *x).is_some() && b.get(c, *x).is_some()).collect();
                            if rows_both.is_empty() {
                                return None;
                            }
                            let v = b.get(c, rows_both[r.below(rows_both.len() as u64) as usize])?;
                            param = v.to_string();
                            Truth::Exact(money_canon(t.rows.iter().filter(|x| b.get(c, **x) == Some(v)).filter_map(|x| b.get(m, *x)).sum()))
                        } else {
                            let mut tot: BTreeMap<i64, i64> = BTreeMap::new();
                            for x in &t.rows {
                                if let (Some(k), Some(v)) = (b.get(c, *x), b.get(m, *x)) {
                                    *tot.entry(k).or_default() += v;
                                }
                            }
                            let best = *tot.values().max()?;
                            let tops: Vec<i64> = tot.iter().filter(|(_, v)| **v == best).map(|(k, _)| *k).collect();
                            if tops.len() != 1 || tot.len() < 2 {
                                return None;
                            }
                            Truth::Exact(tops[0].to_string())
                        }
                    }
                    Tmpl::GrpTop3 => {
                        cols[1] = name(C_PU);
                        let mut n: BTreeMap<i64, i64> = BTreeMap::new();
                        for v in vals(C_PU).into_iter().flatten() {
                            *n.entry(v).or_default() += 1;
                        }
                        if n.len() < 3 {
                            return None;
                        }
                        let mut v: Vec<(i64, i64)> = n.into_iter().collect();
                        v.sort_by_key(|(id, c)| (-c, *id));
                        Truth::Exact(v[..3].iter().map(|x| x.0.to_string()).collect::<Vec<_>>().join(","))
                    }
                    Tmpl::Lookup => {
                        let c = pick_col(r, &|c| is_value_col(c) || is_cat_col(c))?;
                        let x = row_with(r, c)?;
                        cols[0] = name(c);
                        cols[2] = "trip_id".into();
                        param = x.to_string();
                        let v = b.get(c, x)?;
                        Truth::Exact(if col_type(c) == CT::Cents { money_canon(v) } else { v.to_string() })
                    }
                    Tmpl::AnFareNeg => {
                        cols[0] = name(C_FARE);
                        count(&|x| b.get(C_FARE, x).map_or(false, |v| v < 0))
                    }
                    Tmpl::AnDistZero => {
                        if !has(C_DIST) {
                            return None;
                        }
                        cols[0] = name(C_DIST);
                        count(&|x| b.get(C_DIST, x) == Some(0))
                    }
                    Tmpl::AnTotalMismatch => {
                        if !has(C_TOTAL) || !COMPONENTS.iter().all(|c| has(*c)) {
                            return None;
                        }
                        cols[0] = name(C_TOTAL);
                        count(&|x| {
                            let comp: Option<i64> = COMPONENTS.iter().map(|c| b.get(*c, x)).sum();
                            matches!((b.get(C_TOTAL, x), comp), (Some(tt), Some(s)) if tt != s)
                        })
                    }
                    Tmpl::AnOutDay => {
                        cols[0] = name(C_PICKUP);
                        param = jan_date(t.day);
                        let lo = JAN1 + (t.day - 1) * DAY;
                        count(&|x| b.get(C_PICKUP, x).map_or(false, |v| v < lo || v >= lo + DAY))
                    }
                    Tmpl::AnOutMonth => {
                        cols[0] = name(C_PICKUP);
                        count(&|x| b.get(C_PICKUP, x).map_or(false, |v| v < JAN1 || v >= JAN1 + 31 * DAY))
                    }
                    Tmpl::AnFirstBack => {
                        if !has(C_DROPOFF) {
                            return None;
                        }
                        cols[0] = name(C_PICKUP);
                        cols[1] = name(C_DROPOFF);
                        cols[2] = "trip_id".into();
                        let first = t.rows.iter().copied().filter(|x| matches!((b.get(C_PICKUP, *x), b.get(C_DROPOFF, *x)), (Some(p), Some(d)) if d < p)).min()?;
                        Truth::Exact(first.to_string())
                    }
                    Tmpl::FixLine => Truth::Exact(t.bad?.0.to_string()),
                    Tmpl::FormatOf => Truth::Exact(format!("{}/{}", delim_name(t.rd.delim), if t.rd.header { "header" } else { "noheader" })),
                    Tmpl::TsNext => return None,
                }
            }
        };
        let reward = tmpl.base_reward() + 2 * f.size as i64;
        let prompt = prompt_of(tmpl, &f.name, &param, &cols, prefix, tol);
        let id = format!("m{step}-{}-{}", tmpl.name(), f.name.split('.').next().unwrap_or(""));
        Some(TaskT { view: ViewT { step, id, tmpl, ctx: f.ctx, file: f.name.clone(), rows, size: f.size, reward, param, cols, prefix, tol, prompt, penalty_pm: self.cfg.penalty_pm, cost_pm: self.cfg.cost_pm }, truth, file_idx: fi })
    }

    pub fn view_bytes<'a>(&self, v: &ViewT, bytes: &'a [u8]) -> &'a [u8] {
        if v.prefix == 0 {
            return bytes;
        }
        let mut lines = 0usize;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'\n' {
                lines += 1;
                if lines == v.prefix + 1 {
                    return &bytes[..=i];
                }
            }
        }
        bytes
    }
}

pub fn prompt_of(t: Tmpl, file: &str, param: &str, c: &[String; 4], prefix: usize, tol: i64) -> String {
    let conv = "NULLs ignored, values as recorded (negatives included), money as 1234.50";
    match t {
        Tmpl::ColSum => format!("{file}: what is the total of column {} ({conv})?", c[0]),
        Tmpl::ColMax => format!("{file}: what is the largest value of column {} ({conv})?", c[0]),
        Tmpl::ColCount => format!("{file}: how many data rows does the file have?"),
        Tmpl::ColMean => format!("{file}: what is the mean of column {} ({conv}; two decimals)?", c[0]),
        Tmpl::ColDistinct => format!("{file}: how many distinct non-NULL values does column {} have?", c[1]),
        Tmpl::FltCount => format!("{file}: how many rows have {} {} {param} (NULL never satisfies)?", c[0], c[3]),
        Tmpl::FltSum => format!("{file}: what is the total of {} where {} = {param} ({conv})?", c[0], c[1]),
        Tmpl::GrpTop => format!("{file}: which {} has the largest total {} ({conv})?", c[1], c[0]),
        Tmpl::GrpTop3 => format!("{file}: which three {} have the most trips (most first, ties: smallest id; answer like 132,161,237)?", c[1]),
        Tmpl::QMedian => format!("{file}: what is the median of {} ({conv}; an even count takes the mean of the two middle values)?", c[0]),
        Tmpl::QP90 => format!("{file}: what is the 90th percentile of {} ({conv}; nearest rank: the value at rank ceil(0.9 n) ascending)?", c[0]),
        Tmpl::Lookup => format!("{file}: what is {} of the row with {} = {param}?", c[0], c[2]),
        Tmpl::AnFareNeg => format!("{file}: how many rows have {} below 0?", c[0]),
        Tmpl::AnDistZero => format!("{file}: how many rows have {} exactly 0?", c[0]),
        Tmpl::AnTotalMismatch => format!("{file}: on how many rows does total_amount differ from the sum of fare_amount, extra, mta_tax, tip_amount, tolls_amount, improvement_surcharge, congestion_surcharge and Airport_fee (exact cents; rows with a NULL among them are not counted)?"),
        Tmpl::AnOutDay => format!("{file}: how many rows have {} outside the day {param}?", c[0]),
        Tmpl::AnOutMonth => format!("{file}: how many rows have {} outside January 2024?", c[0]),
        Tmpl::AnFirstBack => format!("{file}: what is the smallest trip_id of a row whose {} is before its {}?", c[1], c[0]),
        Tmpl::FixLine => format!("{file}: which line (1-based, the header is line 1) is malformed (extra or missing field, a bad number or an impossible date)?"),
        Tmpl::FormatOf => format!("{file}: what is the delimiter, and is there a header row?"),
        Tmpl::TsNext => format!("{file}: from its first {prefix} values, predict the next count within +-{tol}"),
    }
}
