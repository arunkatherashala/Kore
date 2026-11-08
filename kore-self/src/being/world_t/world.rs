//! World T: the sandbox pool of data files, the task kinds, the schedule, the visible task view and the
//! exact checkers. A pure function of `WTCfg` (all integers, logged in the genesis event) and of (step,
//! world clock). Author-defined simulation; see docs/world_t.md.

use super::data::*;
use crate::being::world::{Rng, SECS_PER_ENERGY};

/// Source of every log row of a World T being.
pub const SOURCE_T: &str = "being:worldt";
pub const SKIP_COST: i64 = 1;

/// Task kinds. Opened over the life in a seed-shuffled order (the being is told when a kind opens).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    ColStat,
    Group,
    Lookup,
    TsExact,
    TsNoisy,
    Derived,
    FixRow,
    Format,
    Logs,
}

pub const KINDS: [Kind; 9] = [Kind::ColStat, Kind::Group, Kind::Lookup, Kind::TsExact, Kind::TsNoisy, Kind::Derived, Kind::FixRow, Kind::Format, Kind::Logs];

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::ColStat => "colstat",
            Kind::Group => "group",
            Kind::Lookup => "lookup",
            Kind::TsExact => "ts_exact",
            Kind::TsNoisy => "ts_noisy",
            Kind::Derived => "derived",
            Kind::FixRow => "fix_row",
            Kind::Format => "format",
            Kind::Logs => "logs",
        }
    }
    pub fn idx(self) -> usize {
        KINDS.iter().position(|k| *k == self).unwrap()
    }
    pub fn tmpls(self) -> &'static [Tmpl] {
        use Tmpl::*;
        match self {
            Kind::ColStat => &[ColSum, ColMax, ColCount, ColMean, ColDistinct],
            Kind::Group => &[GrpSumWhere, GrpTop, GrpCountGt],
            Kind::Lookup => &[Lookup],
            Kind::TsExact => &[TsNext],
            Kind::TsNoisy => &[TsNear],
            Kind::Derived => &[DerivedVal],
            Kind::FixRow => &[FixLine],
            Kind::Format => &[FormatOf],
            Kind::Logs => &[LogCount, LogTop],
        }
    }
    fn ctxs(self) -> &'static [Ctx] {
        match self {
            Kind::ColStat | Kind::Group | Kind::Lookup | Kind::FixRow | Kind::Format => &TABLE_CTXS,
            Kind::TsExact => &EXACT_TS_CTXS,
            Kind::TsNoisy => &NOISY_TS_CTXS,
            Kind::Derived => &DERIVED_CTXS,
            Kind::Logs => &LOG_CTXS,
        }
    }
}

/// Question templates (visible: the task text names its template).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tmpl {
    ColSum,
    ColMax,
    ColCount,
    ColMean,
    ColDistinct,
    GrpSumWhere,
    GrpTop,
    GrpCountGt,
    Lookup,
    TsNext,
    TsNear,
    DerivedVal,
    FixLine,
    FormatOf,
    LogCount,
    LogTop,
}

pub const TMPLS: [Tmpl; 16] = [
    Tmpl::ColSum, Tmpl::ColMax, Tmpl::ColCount, Tmpl::ColMean, Tmpl::ColDistinct, Tmpl::GrpSumWhere, Tmpl::GrpTop, Tmpl::GrpCountGt, Tmpl::Lookup, Tmpl::TsNext, Tmpl::TsNear, Tmpl::DerivedVal,
    Tmpl::FixLine, Tmpl::FormatOf, Tmpl::LogCount, Tmpl::LogTop,
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
            Tmpl::GrpSumWhere => "grp_sum_where",
            Tmpl::GrpTop => "grp_top",
            Tmpl::GrpCountGt => "grp_count_gt",
            Tmpl::Lookup => "lookup",
            Tmpl::TsNext => "ts_next",
            Tmpl::TsNear => "ts_near",
            Tmpl::DerivedVal => "derived_val",
            Tmpl::FixLine => "fix_line",
            Tmpl::FormatOf => "format_of",
            Tmpl::LogCount => "log_count",
            Tmpl::LogTop => "log_top",
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
            Kind::Group => 9,
            Kind::TsExact | Kind::TsNoisy => 7,
            Kind::Derived | Kind::FixRow => 9,
            Kind::Format => 4,
            Kind::Logs => 8,
        }
    }
}

/// The being's plans (tools and tool chains). Which plans apply to a template is public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Plan {
    NaiveNative,
    SniffNative,
    NaiveSql,
    SniffSql,
    TsLast,
    TsDrift,
    TsSeason7,
    TsSeason24,
    TsMean,
    TsRule,
    ColProduct,
    ColRatio,
    ColSearch,
    FixFields,
    FixTypes,
    FmtExt,
    FmtSniff,
    LogGrep,
    LogParse,
    LogSql,
}

pub const PLANS: [Plan; 20] = [
    Plan::NaiveNative, Plan::SniffNative, Plan::NaiveSql, Plan::SniffSql, Plan::TsLast, Plan::TsDrift, Plan::TsSeason7, Plan::TsSeason24, Plan::TsMean, Plan::TsRule, Plan::ColProduct, Plan::ColRatio,
    Plan::ColSearch, Plan::FixFields, Plan::FixTypes, Plan::FmtExt, Plan::FmtSniff, Plan::LogGrep, Plan::LogParse, Plan::LogSql,
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
            Plan::TsLast => "ts-last",
            Plan::TsDrift => "ts-drift",
            Plan::TsSeason7 => "ts-season7",
            Plan::TsSeason24 => "ts-season24",
            Plan::TsMean => "ts-mean7",
            Plan::TsRule => "ts-rule",
            Plan::ColProduct => "col-product",
            Plan::ColRatio => "col-ratio",
            Plan::ColSearch => "col-search",
            Plan::FixFields => "fix-fieldcount",
            Plan::FixTypes => "fix-typecheck",
            Plan::FmtExt => "fmt-extension",
            Plan::FmtSniff => "fmt-sniff",
            Plan::LogGrep => "log-grep",
            Plan::LogParse => "log-parse",
            Plan::LogSql => "log-sql",
        }
    }
    pub fn parse(s: &str) -> Option<Plan> {
        PLANS.iter().copied().find(|p| p.name() == s)
    }
    pub fn uses_sql(self) -> bool {
        matches!(self, Plan::NaiveSql | Plan::SniffSql | Plan::LogSql)
    }
    /// Listed energy for a file with `rows` data lines (ColSearch: the price of the first two candidates;
    /// its real cost depends on how many forms it tries).
    pub fn energy(self, rows: usize) -> i64 {
        let r = rows as i64;
        match self {
            Plan::NaiveNative => 1 + r / 300,
            Plan::SniffNative => 2 + r / 200,
            Plan::NaiveSql => 2 + r / 200,
            Plan::SniffSql => 3 + r / 150,
            Plan::TsLast | Plan::TsDrift | Plan::TsSeason7 | Plan::TsSeason24 | Plan::TsMean | Plan::TsRule => 1,
            Plan::ColProduct => 1,
            Plan::ColRatio => 2,
            Plan::ColSearch => 3,
            Plan::FixFields => 1 + r / 300,
            Plan::FixTypes => 2 + r / 120,
            Plan::FmtExt => 1,
            Plan::FmtSniff => 2,
            Plan::LogGrep => 1 + r / 400,
            Plan::LogParse => 2 + r / 250,
            Plan::LogSql => 3 + r / 200,
        }
    }
}

/// Plans that apply to a template (public knowledge). `TsRule` applies only when the being holds a
/// rule for the series' signature (the learner checks that).
pub fn plans_for(t: Tmpl) -> &'static [Plan] {
    use Plan::*;
    match t {
        Tmpl::ColSum | Tmpl::ColMax | Tmpl::ColCount | Tmpl::ColMean | Tmpl::ColDistinct | Tmpl::Lookup => &[NaiveNative, SniffNative, NaiveSql, SniffSql],
        Tmpl::GrpSumWhere | Tmpl::GrpTop | Tmpl::GrpCountGt => &[NaiveSql, SniffSql],
        Tmpl::TsNext => &[TsLast, TsDrift, TsSeason7, TsRule],
        Tmpl::TsNear => &[TsLast, TsDrift, TsSeason7, TsSeason24, TsMean, TsRule],
        Tmpl::DerivedVal => &[ColProduct, ColRatio, ColSearch],
        Tmpl::FixLine => &[FixFields, FixTypes],
        Tmpl::FormatOf => &[FmtExt, FmtSniff],
        Tmpl::LogCount => &[LogGrep, LogParse, LogSql],
        Tmpl::LogTop => &[LogGrep, LogSql],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WTCfg {
    pub seed: u64,
    /// Energy units of the life (no refills).
    pub budget: i64,
    /// Kinds open at energy 0.
    pub start_kinds: usize,
    /// Energy units between two newly opened kinds.
    pub intro_every: i64,
    /// Files per context and pool.
    pub files: u64,
    /// Out-of-distribution world (never used in tuning): tables of 1000 to 2500 rows with their columns in
    /// reverse order, series and column rules the default world never uses. Off by default (no tag).
    pub ood: bool,
}

pub const DEFAULT_BUDGET: i64 = 6000;

impl WTCfg {
    pub fn new(seed: u64, budget: i64) -> WTCfg {
        WTCfg { seed, budget, start_kinds: 3, intro_every: (budget / 10).max(1), files: 6, ood: false }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=1_000_000).contains(&self.budget) {
            return Err("budget must be 1..=1000000".into());
        }
        if !(1..=9).contains(&self.start_kinds) {
            return Err("start_kinds must be 1..=9".into());
        }
        if self.intro_every < 1 {
            return Err("intro_every must be at least 1".into());
        }
        if !(1..=20).contains(&self.files) {
            return Err("files must be 1..=20".into());
        }
        Ok(())
    }
    pub fn tags(&self) -> Vec<String> {
        let mut t = vec![format!("wt_seed:{}", self.seed), format!("wt_budget:{}", self.budget), format!("wt_start:{}", self.start_kinds), format!("wt_intro:{}", self.intro_every), format!("wt_files:{}", self.files)];
        if self.ood {
            t.push("wt_ood:1".into());
        }
        t
    }
    pub fn from_tags(tags: &str) -> Option<WTCfg> {
        let g = |k: &str| crate::being::state::tag(tags, k).and_then(|x| x.parse::<i64>().ok());
        let c = WTCfg { seed: g("wt_seed")? as u64, budget: g("wt_budget")?, start_kinds: g("wt_start")? as usize, intro_every: g("wt_intro")?, files: g("wt_files")? as u64, ood: g("wt_ood") == Some(1) };
        c.validate().ok().map(|_| c)
    }
}

/// What a file is, logically (the checkers' ground truth). Never shown to the being.
#[derive(Debug, Clone, PartialEq)]
pub enum Logical {
    Table(Table, Render, Option<usize>),
    Derived(Derived, Vec<usize>),
    Exact(Series),
    Noisy(Series),
    Log(LogFile),
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

/// What the being sees of a task.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewT {
    pub step: u64,
    pub id: String,
    pub tmpl: Tmpl,
    pub ctx: Ctx,
    pub file: String,
    /// Data lines the tool will read (for a series task: the visible prefix).
    pub rows: usize,
    pub size: u8,
    pub reward: i64,
    /// The question's parameter (a category value, a key, a threshold, a service, a row id).
    pub param: String,
    /// Column names the question refers to: (value column, category column, key column, count column).
    pub cols: [String; 4],
    /// Series tasks: lines of the file visible to the tools (header excluded); 0 otherwise.
    pub prefix: usize,
    /// Tolerance of a TsNear task (absolute), 0 otherwise.
    pub tol: i64,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Truth {
    Exact(String),
    /// A mean in cents: any correct rounding of sum / n.
    Mean { sum: i64, n: i64 },
    Tol { value: i64, tol: i64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskT {
    pub view: ViewT,
    pub truth: Truth,
    /// Index of the file in the pool.
    pub file_idx: usize,
}

impl TaskT {
    /// The exact checker.
    pub fn check(&self, ans: &str) -> bool {
        match &self.truth {
            Truth::Exact(s) => ans == s,
            Truth::Mean { sum, n } => parse_cents(ans).map_or(false, |c| 2 * (c as i128 * *n as i128 - *sum as i128).abs() <= *n as i128),
            Truth::Tol { value, tol } => ans.parse::<i64>().map_or(false, |a| (a - value).abs() <= *tol),
        }
    }
    /// The answer revealed after an attempt (series tasks: the exact next value).
    pub fn revealed(&self) -> Option<i64> {
        match &self.truth {
            Truth::Tol { value, .. } => Some(*value),
            Truth::Exact(s) if self.view.tmpl == Tmpl::TsNext => s.parse().ok(),
            _ => None,
        }
    }
}

/// "1234.50" -> 123450 (canonical money text only).
pub fn parse_cents(s: &str) -> Option<i64> {
    let (i, f) = s.split_once('.')?;
    if f.len() != 2 || i.is_empty() {
        return None;
    }
    let neg = i.starts_with('-');
    let iv: i64 = i.trim_start_matches('-').parse().ok()?;
    let fv: i64 = f.parse().ok()?;
    let v = iv * 100 + fv;
    Some(if neg { -v } else { v })
}

pub struct WorldT {
    pub cfg: WTCfg,
    pub files: Vec<FileT>,
    /// Kinds in order of opening.
    pub order: Vec<Kind>,
    by_kind: Vec<Vec<usize>>,
}

impl WorldT {
    pub fn new(cfg: WTCfg) -> WorldT {
        let seed = cfg.seed;
        let mut files = vec![];
        let mut by_kind: Vec<Vec<usize>> = vec![vec![]; KINDS.len()];
        let mut add = |f: FileT, kinds: &[Kind], files: &mut Vec<FileT>| {
            for k in kinds {
                by_kind[k.idx()].push(files.len());
            }
            files.push(f);
        };
        for ctx in TABLE_CTXS {
            let p = profile(seed, ctx);
            for i in 0..cfg.files {
                let mut r = Rng::new(mix(&[seed, 0xF11E, ctx.idx() as u64, i]));
                // the data pool (tables for statistics, groups and lookups)
                let size = if cfg.ood { 3 } else { r.below(3) as u8 };
                let t = ood_shape(gen_table(seed, ctx, i, size, &p), cfg.ood);
                let rd = draw_render(&mut r, ctx, &p, false);
                let (bytes, _) = render_table(&t, &rd, None);
                add(FileT { name: format!("{}_t{:02}.{}", ctx.name(), i, ctx.ext()), ctx, kind_pool: "t", size, bytes, logical: Logical::Table(t, rd, None) }, &[Kind::ColStat, Kind::Group, Kind::Lookup], &mut files);
                // the fix pool: one malformed line each
                let size = if cfg.ood { 3 } else { r.below(2) as u8 };
                let t = ood_shape(gen_table(seed, ctx, 100 + i, size, &p), cfg.ood);
                let rd = draw_render(&mut r, ctx, &p, false);
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
                let row = r.range(t.rows.len() as i64 / 5, t.rows.len() as i64 - 1) as usize;
                let (bytes, bad) = render_table(&t, &rd, Some(Bad { row, kind }));
                add(FileT { name: format!("{}_fix{:02}.{}", ctx.name(), i, ctx.ext()), ctx, kind_pool: "fix", size, bytes, logical: Logical::Table(t, rd, bad) }, &[Kind::FixRow], &mut files);
                // the raw pool (format questions; some files have no header)
                let t = ood_shape(gen_table(seed, ctx, 200 + i, 0, &p), cfg.ood);
                let rd = draw_render(&mut r, ctx, &p, true);
                let (bytes, _) = render_table(&t, &rd, None);
                add(FileT { name: format!("{}_raw{:02}.{}", ctx.name(), i, ctx.ext()), ctx, kind_pool: "raw", size: 0, bytes, logical: Logical::Table(t, rd, None) }, &[Kind::Format], &mut files);
            }
        }
        for ctx in DERIVED_CTXS {
            for i in 0..cfg.files {
                let mut r = Rng::new(mix(&[seed, 0xF12E, ctx.idx() as u64, i]));
                let size = r.below(2) as u8;
                let dv = gen_derived(seed, ctx, i, size, cfg.ood);
                let n = dv.rows.len();
                let mut blanks: Vec<usize> = vec![];
                while blanks.len() < 4 {
                    let b = r.range(n as i64 / 3, n as i64 - 1) as usize;
                    if !blanks.contains(&b) {
                        blanks.push(b);
                    }
                }
                blanks.sort();
                let bytes = render_derived_blanks(&dv, &blanks);
                add(FileT { name: format!("{}_{:02}.csv", ctx.name(), i), ctx, kind_pool: "derived", size, bytes, logical: Logical::Derived(dv, blanks) }, &[Kind::Derived], &mut files);
            }
        }
        for ctx in EXACT_TS_CTXS {
            for i in 0..cfg.files * 2 {
                let s = gen_exact(seed, ctx, i, cfg.ood);
                let bytes = render_series(&s, s.values.len());
                add(FileT { name: format!("{}_{:02}.csv", ctx.name(), i), ctx, kind_pool: "series", size: 0, bytes, logical: Logical::Exact(s) }, &[Kind::TsExact], &mut files);
            }
        }
        for ctx in NOISY_TS_CTXS {
            for i in 0..cfg.files * 2 {
                let s = gen_noisy(seed, ctx, i);
                let bytes = render_series(&s, s.values.len());
                add(FileT { name: format!("{}_{:02}.csv", ctx.name(), i), ctx, kind_pool: "series", size: 1, bytes, logical: Logical::Noisy(s) }, &[Kind::TsNoisy], &mut files);
            }
        }
        for ctx in LOG_CTXS {
            for i in 0..cfg.files {
                let mut r = Rng::new(mix(&[seed, 0xF13E, ctx.idx() as u64, i]));
                let size = r.below(3) as u8;
                let lf = gen_log(seed, ctx, i, size);
                add(FileT { name: format!("{}_{:02}.log", ctx.name(), i), ctx, kind_pool: "log", size, bytes: lf.bytes.clone(), logical: Logical::Log(lf) }, &[Kind::Logs], &mut files);
            }
        }
        let mut order = KINDS.to_vec();
        let mut r = Rng::new(mix(&[seed, 0x0DE5]));
        for i in (1..order.len()).rev() {
            let j = r.below(i as u64 + 1) as usize;
            order.swap(i, j);
        }
        WorldT { cfg, files, order, by_kind }
    }

    /// Kinds open at a world clock (seconds; 30 s per energy unit).
    pub fn open_kinds(&self, clock: i64) -> Vec<Kind> {
        let energy = clock / SECS_PER_ENERGY;
        let n = (self.cfg.start_kinds as i64 + energy / self.cfg.intro_every).min(KINDS.len() as i64) as usize;
        self.order[..n].to_vec()
    }

    /// The energy at which the i-th kind (in order of opening) opens.
    pub fn intro_energy(&self, i: usize) -> i64 {
        if i < self.cfg.start_kinds { 0 } else { (i - self.cfg.start_kinds + 1) as i64 * self.cfg.intro_every }
    }

    pub fn file(&self, name: &str) -> Option<&FileT> {
        self.files.iter().find(|f| f.name == name)
    }

    /// The task of a step (one task per step).
    pub fn task(&self, step: u64, clock: i64) -> TaskT {
        let open = self.open_kinds(clock);
        let mut r = Rng::new(mix(&[self.cfg.seed, 0x7A5C, step]));
        let kind = open[r.below(open.len() as u64) as usize];
        let ts = kind.tmpls();
        let tmpl = ts[r.below(ts.len() as u64) as usize];
        let pool = &self.by_kind[kind.idx()];
        let fi = pool[r.below(pool.len() as u64) as usize];
        self.make_task(step, tmpl, fi, &mut r)
    }

    fn make_task(&self, step: u64, tmpl: Tmpl, fi: usize, r: &mut Rng) -> TaskT {
        let f = &self.files[fi];
        let mut cols: [String; 4] = Default::default();
        let mut param = String::new();
        let (mut prefix, mut tol) = (0usize, 0i64);
        let rows;
        let truth = match &f.logical {
            Logical::Table(t, rd, bad) => {
                let h = |role: Role| t.col(role).map(|j| t.header[j].clone()).unwrap_or_default();
                cols = [h(Role::Money), h(Role::CatA), h(Role::Key), h(Role::Int)];
                rows = t.rows.len();
                let mi = t.col(Role::Money).unwrap();
                let ci = t.col(Role::CatA).unwrap();
                let money: Vec<i64> = t.rows.iter().filter_map(|row| if let Val::Money(c) = row[mi] { Some(c) } else { None }).collect();
                let cat = |row: &Vec<Val>| if let Val::Str(s) = &row[ci] { s.clone() } else { String::new() };
                match tmpl {
                    Tmpl::ColSum => Truth::Exact(money_canon(money.iter().sum())),
                    Tmpl::ColMax => Truth::Exact(money_canon(*money.iter().max().unwrap_or(&0))),
                    Tmpl::ColCount => Truth::Exact(t.rows.len().to_string()),
                    Tmpl::ColMean => Truth::Mean { sum: money.iter().sum(), n: money.len().max(1) as i64 },
                    Tmpl::ColDistinct => {
                        let s: std::collections::BTreeSet<String> = t.rows.iter().map(cat).collect();
                        Truth::Exact(s.len().to_string())
                    }
                    Tmpl::GrpSumWhere => {
                        let v = cat(&t.rows[r.below(t.rows.len() as u64) as usize]);
                        let s: i64 = t.rows.iter().filter(|row| cat(row) == v).filter_map(|row| if let Val::Money(c) = row[mi] { Some(c) } else { None }).sum();
                        param = v;
                        Truth::Exact(money_canon(s))
                    }
                    Tmpl::GrpTop => {
                        let mut m: std::collections::BTreeMap<String, i64> = Default::default();
                        for row in &t.rows {
                            if let Val::Money(c) = row[mi] {
                                *m.entry(cat(row)).or_default() += c;
                            }
                        }
                        let best = m.values().max().copied().unwrap_or(0);
                        let tops: Vec<&String> = m.iter().filter(|(_, v)| **v == best).map(|(k, _)| k).collect();
                        if tops.len() != 1 {
                            return self.make_task(step, Tmpl::ColCount, fi, r);
                        }
                        Truth::Exact(tops[0].clone())
                    }
                    Tmpl::GrpCountGt => {
                        let ii = t.col(Role::Int).unwrap();
                        let k = r.range(3, 25);
                        param = k.to_string();
                        Truth::Exact(t.rows.iter().filter(|row| matches!(row[ii], Val::Int(x) if x > k)).count().to_string())
                    }
                    Tmpl::Lookup => {
                        let ki = t.col(Role::Key).unwrap();
                        let cand: Vec<&Vec<Val>> = t.rows.iter().filter(|row| matches!(row[mi], Val::Money(_))).collect();
                        let row = cand[r.below(cand.len().max(1) as u64) as usize];
                        param = if let Val::Str(s) = &row[ki] { s.clone() } else { String::new() };
                        let c = if let Val::Money(c) = row[mi] { c } else { 0 };
                        // a key may repeat (author-defined keys are not unique by construction): the first row counts
                        let first = t.rows.iter().find(|x| matches!(&x[ki], Val::Str(s) if *s == param)).unwrap();
                        let c = if let Val::Money(c0) = first[mi] { c0 } else { c };
                        Truth::Exact(money_canon(c))
                    }
                    Tmpl::FixLine => Truth::Exact(bad.unwrap_or(0).to_string()),
                    Tmpl::FormatOf => Truth::Exact(format!("{}/{}", delim_name(rd.delim), if rd.header { "header" } else { "noheader" })),
                    _ => unreachable!("table template"),
                }
            }
            Logical::Derived(dv, blanks) => {
                rows = dv.rows.len();
                let b = blanks[r.below(blanks.len() as u64) as usize];
                param = dv.rows[b].0.clone();
                cols = [dv.header[4].clone(), dv.header[1].clone(), dv.header[0].clone(), dv.header[2].clone()];
                Truth::Exact(money_canon(dv.rows[b].4))
            }
            Logical::Exact(s) => {
                prefix = r.range(16, s.values.len() as i64 - 1) as usize;
                rows = prefix;
                cols = ["value".into(), String::new(), String::new(), String::new()];
                Truth::Exact(s.values[prefix].to_string())
            }
            Logical::Noisy(s) => {
                prefix = r.range(50, s.values.len() as i64 - 1) as usize;
                rows = prefix;
                let last = s.values[prefix - 1];
                tol = (last.abs() * 3 / 100).max(2);
                cols = [if s.ctx == Ctx::Sensor { "celsius_x10" } else { "value" }.into(), String::new(), String::new(), String::new()];
                Truth::Tol { value: s.values[prefix], tol }
            }
            Logical::Log(lf) => {
                rows = lf.bytes.iter().filter(|b| **b == b'\n').count();
                match tmpl {
                    Tmpl::LogCount => {
                        let svc = lf.services[r.below(lf.services.len() as u64) as usize].clone();
                        let n = lf.entries.iter().filter(|(s, e)| *e && *s == svc).count();
                        param = svc;
                        Truth::Exact(n.to_string())
                    }
                    _ => {
                        let mut m: std::collections::BTreeMap<&str, usize> = Default::default();
                        for (s, e) in &lf.entries {
                            if *e {
                                *m.entry(s.as_str()).or_default() += 1;
                            }
                        }
                        let best = m.values().max().copied().unwrap_or(0);
                        let tops: Vec<&&str> = m.iter().filter(|(_, v)| **v == best).map(|(k, _)| k).collect();
                        if tops.len() != 1 {
                            return self.make_task(step, Tmpl::LogCount, fi, r);
                        }
                        Truth::Exact(tops[0].to_string())
                    }
                }
            }
        };
        let size = f.size;
        let reward = tmpl.base_reward() + 2 * size as i64;
        let prompt = prompt_of(tmpl, &f.name, &param, &cols, prefix, tol);
        let id = format!("t{step}-{}-{}", tmpl.name(), f.name.split('.').next().unwrap_or(""));
        TaskT { view: ViewT { step, id, tmpl, ctx: f.ctx, file: f.name.clone(), rows, size, reward, param, cols, prefix, tol, prompt }, truth, file_idx: fi }
    }

    /// The bytes a tool may read for a task: the file, or for a series task its visible prefix.
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

/// Out-of-distribution shape: the same table with its columns in reverse order.
fn ood_shape(mut t: Table, ood: bool) -> Table {
    if ood {
        t.header.reverse();
        t.roles.reverse();
        for r in t.rows.iter_mut() {
            r.reverse();
        }
    }
    t
}

fn render_derived_blanks(dv: &Derived, blanks: &[usize]) -> Vec<u8> {
    let nl = if dv.render.crlf { "\r\n" } else { "\n" };
    let mut s = dv.header.join(",");
    s.push_str(nl);
    for (i, (k, a, b, d, t)) in dv.rows.iter().enumerate() {
        let tt = if blanks.contains(&i) { String::new() } else { money_canon(*t) };
        s.push_str(&format!("{k},{a},{},{d},{tt}{nl}", money_canon(*b)));
    }
    s.into_bytes()
}

pub fn prompt_of(t: Tmpl, file: &str, param: &str, c: &[String; 4], prefix: usize, tol: i64) -> String {
    match t {
        Tmpl::ColSum => format!("{file}: what is the total of column {} (nulls ignored)?", c[0]),
        Tmpl::ColMax => format!("{file}: what is the largest value of column {}?", c[0]),
        Tmpl::ColCount => format!("{file}: how many data rows does the file have?"),
        Tmpl::ColMean => format!("{file}: what is the mean of column {} (nulls ignored, two decimals)?", c[0]),
        Tmpl::ColDistinct => format!("{file}: how many distinct values does column {} have?", c[1]),
        Tmpl::GrpSumWhere => format!("{file}: what is the total of {} where {} = {param}?", c[0], c[1]),
        Tmpl::GrpTop => format!("{file}: which {} has the largest total {}?", c[1], c[0]),
        Tmpl::GrpCountGt => format!("{file}: how many rows have {} greater than {param}?", c[3]),
        Tmpl::Lookup => format!("{file}: what is {} of the first row with {} = {param}?", c[0], c[2]),
        Tmpl::TsNext => format!("{file}: from its first {prefix} values, what is the next value (exact)?"),
        Tmpl::TsNear => format!("{file}: from its first {prefix} values, predict the next value within +-{tol}"),
        Tmpl::DerivedVal => format!("{file}: column {} is missing for {} = {param}; what is it?", c[0], c[2]),
        Tmpl::FixLine => format!("{file}: which line (1-based, the header is line 1) is malformed?"),
        Tmpl::FormatOf => format!("{file}: what is the delimiter, and is there a header row?"),
        Tmpl::LogCount => format!("{file}: how many ERROR entries does service {param} have?"),
        Tmpl::LogTop => format!("{file}: which service has the most ERROR entries?"),
    }
}
