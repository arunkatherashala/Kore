//! World T data: a deterministic generator of realistic-looking data files (tables with quirks, time
//! series, log files). A pure function of (seed, context, file index); no real data, no personal data,
//! nothing read from outside. Every value, name and quirk below is author-defined.
//!
//! A file is generated as a LOGICAL object (the rows and values, used by the checkers) and then
//! RENDERED to bytes with its quirks (delimiter, quoting, thousands separators, decimal commas, null
//! tokens, byte-order mark, CRLF line ends, padding, a missing header, one malformed line). The being
//! only ever sees the rendered bytes.

use crate::being::world::Rng;

pub fn mix(parts: &[u64]) -> u64 {
    let mut acc = 0x7E57_DA7A_u64;
    for p in parts {
        let mut r = Rng::new(acc ^ p.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        acc = r.next_u64();
    }
    acc
}

/// Integer weights for a skewed (Zipf-like) draw over `n` categories: 60, 30, 20, 15, 12, ...
pub fn zipf_pick(r: &mut Rng, n: usize) -> usize {
    let w: Vec<u64> = (0..n).map(|i| 120 / (i as u64 + 2)).collect();
    let tot: u64 = w.iter().sum();
    let mut x = r.below(tot) as u64;
    for (i, wi) in w.iter().enumerate() {
        if x < *wi {
            return i;
        }
        x -= wi;
    }
    n - 1
}

/// A standard normal draw (Box-Muller), deterministic.
pub fn normal(r: &mut Rng) -> f64 {
    let u1 = r.f64().max(1e-12);
    let u2 = r.f64();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Data contexts: a domain and a file type. Visible to the being (in the file name and the task).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ctx {
    Sales,
    Orders,
    Payments,
    Customers,
    Inventory,
    Shipments,
    Invoices,
    Payroll,
    Meter,
    CumSales,
    Shifts,
    Sensor,
    Visits,
    AppLog,
    WorkerLog,
}

pub const CTXS: [Ctx; 15] = [
    Ctx::Sales, Ctx::Orders, Ctx::Payments, Ctx::Customers, Ctx::Inventory, Ctx::Shipments, Ctx::Invoices, Ctx::Payroll, Ctx::Meter, Ctx::CumSales, Ctx::Shifts, Ctx::Sensor, Ctx::Visits,
    Ctx::AppLog, Ctx::WorkerLog,
];
pub const TABLE_CTXS: [Ctx; 6] = [Ctx::Sales, Ctx::Orders, Ctx::Payments, Ctx::Customers, Ctx::Inventory, Ctx::Shipments];
pub const DERIVED_CTXS: [Ctx; 2] = [Ctx::Invoices, Ctx::Payroll];
pub const EXACT_TS_CTXS: [Ctx; 3] = [Ctx::Meter, Ctx::CumSales, Ctx::Shifts];
pub const NOISY_TS_CTXS: [Ctx; 2] = [Ctx::Sensor, Ctx::Visits];
pub const LOG_CTXS: [Ctx; 2] = [Ctx::AppLog, Ctx::WorkerLog];

impl Ctx {
    pub fn idx(self) -> usize {
        CTXS.iter().position(|c| *c == self).unwrap()
    }
    pub fn name(self) -> &'static str {
        match self {
            Ctx::Sales => "sales",
            Ctx::Orders => "orders",
            Ctx::Payments => "payments",
            Ctx::Customers => "customers",
            Ctx::Inventory => "inventory",
            Ctx::Shipments => "shipments",
            Ctx::Invoices => "invoices",
            Ctx::Payroll => "payroll",
            Ctx::Meter => "meter",
            Ctx::CumSales => "cumsales",
            Ctx::Shifts => "shifts",
            Ctx::Sensor => "sensor",
            Ctx::Visits => "visits",
            Ctx::AppLog => "applog",
            Ctx::WorkerLog => "workerlog",
        }
    }
    pub fn parse(s: &str) -> Option<Ctx> {
        CTXS.iter().copied().find(|c| c.name() == s)
    }
    pub fn ext(self) -> &'static str {
        match self {
            Ctx::Inventory => "tsv",
            Ctx::Shipments => "txt",
            Ctx::AppLog | Ctx::WorkerLog => "log",
            _ => "csv",
        }
    }
    /// The delimiter the file type suggests (what a reader guessing from the extension assumes).
    pub fn ext_delim(self) -> u8 {
        match self.ext() {
            "tsv" => b'\t',
            "txt" => b'|',
            _ => b',',
        }
    }
}

pub fn delim_name(d: u8) -> &'static str {
    match d {
        b',' => "comma",
        b';' => "semicolon",
        b'\t' => "tab",
        b'|' => "pipe",
        _ => "other",
    }
}

// ---- tables ----

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Null,
    Int(i64),
    /// Money in cents.
    Money(i64),
    Str(String),
    /// (year, month, day); may be invalid only in a malformed line.
    Date(i64, i64, i64),
}

/// The role of each column (the checkers and the being's question templates refer to roles by the
/// column's header name, which is visible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Key,
    Text,
    CatA,
    CatB,
    Int,
    Money,
    Date,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub ctx: Ctx,
    pub header: Vec<String>,
    pub roles: Vec<Role>,
    pub rows: Vec<Vec<Val>>,
}

impl Table {
    pub fn col(&self, role: Role) -> Option<usize> {
        self.roles.iter().position(|r| *r == role)
    }
}

const REGIONS: [&str; 5] = ["North", "South", "East", "West", "Central"];
const PRODUCTS: [&str; 6] = ["widget", "gadget", "gizmo", "sprocket", "bracket", "valve"];
const STATUS: [&str; 5] = ["shipped", "pending", "returned", "cancelled", "on_hold"];
const METHODS: [&str; 5] = ["card", "transfer", "cash", "voucher", "wallet"];
const SEGMENTS: [&str; 4] = ["retail", "wholesale", "online", "partner"];
const CITIES: [&str; 6] = ["Northport", "Easton", "Westfield", "Southbay", "Midvale", "Lakeside"];
const WAREHOUSES: [&str; 4] = ["WH-A", "WH-B", "WH-C", "WH-D"];
const CATEGORIES: [&str; 5] = ["tools", "paint", "garden", "plumbing", "electrical"];
const CARRIERS: [&str; 4] = ["FastShip", "BlueLine", "CargoCo", "Parcelo"];
const LAST: [&str; 10] = ["Smith", "Garcia", "Chen", "Okafor", "Novak", "Silva", "Kim", "Moreau", "Rossi", "Haddad"];
const FIRST: [&str; 8] = ["A.", "B.", "C.", "D.", "E.", "F.", "G.", "H."];
const COMPANIES: [&str; 6] = ["Acme", "Globex", "Initech", "Umbrella", "Hooli", "Vandelay"];

/// (header, role) of each table context, in file column order.
pub fn schema(ctx: Ctx) -> Vec<(&'static str, Role)> {
    use Role::*;
    match ctx {
        Ctx::Sales => vec![("sale_id", Key), ("date", Date), ("region", CatA), ("product", CatB), ("qty", Int), ("amount", Money)],
        Ctx::Orders => vec![("order_id", Key), ("customer", Text), ("status", CatB), ("region", CatA), ("items", Int), ("amount", Money), ("date", Date)],
        Ctx::Payments => vec![("pay_id", Key), ("method", CatA), ("amount", Money), ("channel", CatB), ("tries", Int)],
        Ctx::Customers => vec![("cust_id", Key), ("name", Text), ("city", CatA), ("segment", CatB), ("visits", Int), ("balance", Money)],
        Ctx::Inventory => vec![("sku", Key), ("warehouse", CatA), ("category", CatB), ("stock", Int), ("unit_cost", Money)],
        Ctx::Shipments => vec![("ship_id", Key), ("carrier", CatA), ("region", CatB), ("date", Date), ("parcels", Int), ("cost", Money)],
        _ => vec![],
    }
}

fn cats(ctx: Ctx, role: Role) -> &'static [&'static str] {
    match (ctx, role) {
        (Ctx::Sales, Role::CatA) | (Ctx::Orders, Role::CatA) | (Ctx::Shipments, Role::CatB) => &REGIONS,
        (Ctx::Sales, Role::CatB) => &PRODUCTS,
        (Ctx::Orders, Role::CatB) => &STATUS,
        (Ctx::Payments, Role::CatA) => &METHODS,
        (Ctx::Payments, Role::CatB) => &SEGMENTS,
        (Ctx::Customers, Role::CatA) => &CITIES,
        (Ctx::Customers, Role::CatB) => &SEGMENTS,
        (Ctx::Inventory, Role::CatA) => &WAREHOUSES,
        (Ctx::Inventory, Role::CatB) => &CATEGORIES,
        (Ctx::Shipments, Role::CatA) => &CARRIERS,
        _ => &REGIONS,
    }
}

fn key_prefix(ctx: Ctx) -> &'static str {
    match ctx {
        Ctx::Sales => "S",
        Ctx::Orders => "O",
        Ctx::Payments => "P",
        Ctx::Customers => "C",
        Ctx::Inventory => "SKU",
        Ctx::Shipments => "SH",
        Ctx::Invoices => "INV",
        Ctx::Payroll => "E",
        _ => "X",
    }
}

/// Rows of a file by size class 0 (small), 1 (medium), 2 (large).
pub fn rows_for(r: &mut Rng, size: u8) -> usize {
    match size {
        0 => r.range(40, 120) as usize,
        3 => r.range(1000, 2500) as usize,
        1 => r.range(150, 350) as usize,
        2 => r.range(400, 700) as usize,
        _ => r.range(1000, 2500) as usize,
    }
}

/// The hidden quirk profile of a context in one world (per mille chances), drawn from the seed inside
/// author-defined ranges. Never shown to the being.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Profile {
    pub alt_delim: u32,
    pub decimal_comma: u32,
    pub thousands: u32,
    pub nulls: u32,
    pub crlf: u32,
    pub pad: u32,
    pub bom: u32,
    pub text_comma: u32,
    pub no_header: u32,
    /// Malformed-line kinds for fix tasks: extra field, missing field, bad number, bad date (weights).
    pub bad_kinds: [u32; 4],
}

pub fn profile(seed: u64, ctx: Ctx) -> Profile {
    let mut r = Rng::new(mix(&[seed, 0x9F0F, ctx.idx() as u64]));
    let mut p = |lo: i64, hi: i64| r.range(lo, hi) as u32;
    let mut pr = match ctx {
        Ctx::Sales => Profile { alt_delim: p(0, 80), nulls: p(30, 150), crlf: p(0, 150), bom: p(0, 100), ..Default::default() },
        Ctx::Orders => Profile { alt_delim: p(400, 800), decimal_comma: p(500, 900), nulls: p(20, 100), text_comma: p(300, 600), ..Default::default() },
        Ctx::Payments => Profile { thousands: p(300, 700), nulls: p(0, 60), pad: p(0, 150), ..Default::default() },
        Ctx::Customers => Profile { text_comma: p(700, 1000), pad: p(300, 600), nulls: p(0, 80), ..Default::default() },
        Ctx::Inventory => Profile { nulls: p(200, 400), bom: p(0, 100), ..Default::default() },
        Ctx::Shipments => Profile { crlf: p(300, 700), bom: p(200, 500), nulls: p(0, 100), ..Default::default() },
        _ => Profile::default(),
    };
    pr.no_header = p(150, 350);
    pr.bad_kinds = match ctx {
        Ctx::Orders | Ctx::Customers => [p(30, 60), p(10, 30), p(10, 30), p(0, 20)],
        Ctx::Sales | Ctx::Shipments => [p(0, 10), p(10, 30), p(10, 30), p(30, 60)],
        _ => [p(0, 10), p(20, 40), p(30, 60), 0],
    };
    pr
}

/// How one file is rendered (drawn per file from the context's profile).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Render {
    pub delim: u8,
    pub decimal_comma: bool,
    pub thousands: bool,
    pub null_tok: &'static str,
    pub crlf: bool,
    pub pad: bool,
    pub bom: bool,
    pub header: bool,
}

const NULL_TOKS: [&str; 4] = ["", "NA", "NULL", "-"];

pub fn draw_render(r: &mut Rng, ctx: Ctx, p: &Profile, allow_no_header: bool) -> Render {
    let mut ch = |pm: u32| (r.below(1000) as u32) < pm;
    let base = ctx.ext_delim();
    let delim = if base == b',' && ch(p.alt_delim) { b';' } else { base };
    let decimal_comma = delim == b';' && ch(p.decimal_comma);
    let thousands = ch(p.thousands);
    let crlf = ch(p.crlf);
    let pad = ch(p.pad);
    let bom = ch(p.bom);
    let header = !(allow_no_header && ch(p.no_header));
    // each context's exports mostly use one null token (author-defined), sometimes another
    let pref = match ctx {
        Ctx::Sales | Ctx::Payments | Ctx::Customers => 0,
        Ctx::Orders => 2,
        Ctx::Inventory => 3,
        _ => 1,
    };
    let null_tok = if ch(800) { NULL_TOKS[pref] } else { NULL_TOKS[r.below(4) as usize] };
    Render { delim, decimal_comma, thousands, null_tok, crlf, pad, bom, header }
}

fn person(r: &mut Rng, comma: bool) -> String {
    let l = LAST[r.below(LAST.len() as u64) as usize];
    let f = FIRST[r.below(FIRST.len() as u64) as usize];
    if comma { format!("{l}, {f}") } else { format!("{f} {l}") }
}

fn company(r: &mut Rng, comma: bool) -> String {
    let c = COMPANIES[r.below(COMPANIES.len() as u64) as usize];
    if comma { format!("{c}, Inc.") } else { format!("{c} Ltd") }
}

fn money(r: &mut Rng, ctx: Ctx) -> i64 {
    let (mu, sd) = match ctx {
        Ctx::Payments => (6.0, 1.1),
        Ctx::Customers => (6.5, 1.3),
        Ctx::Inventory => (2.5, 0.8),
        Ctx::Shipments => (3.2, 0.6),
        _ => (4.2, 1.0),
    };
    let v = (mu + sd * normal(r)).exp();
    ((v * 100.0).round() as i64).clamp(1, 900_000_000)
}

/// A logical table of a context. Values with a `nulls` per mille chance of NULL in the money column.
pub fn gen_table(seed: u64, ctx: Ctx, file: u64, size: u8, p: &Profile) -> Table {
    let mut r = Rng::new(mix(&[seed, 0x7AB1, ctx.idx() as u64, file]));
    let sc = schema(ctx);
    let n = rows_for(&mut r, size);
    let text_comma = (r.below(1000) as u32) < p.text_comma;
    let mut rows = vec![];
    let base_key = r.range(1000, 9000);
    for i in 0..n {
        let mut row = vec![];
        for (_, role) in &sc {
            let v = match role {
                Role::Key => Val::Str(format!("{}-{}", key_prefix(ctx), base_key + i as i64 * 3 + r.below(3))),
                Role::Text => {
                    let half = r.below(2) == 0;
                    Val::Str(if ctx == Ctx::Orders { company(&mut r, text_comma && half) } else { person(&mut r, text_comma) })
                }
                Role::CatA | Role::CatB => {
                    let c = cats(ctx, *role);
                    Val::Str(c[zipf_pick(&mut r, c.len())].to_string())
                }
                Role::Int => Val::Int(1 + (r.f64().powi(2) * 40.0) as i64),
                Role::Money => {
                    if (r.below(1000) as u32) < p.nulls {
                        Val::Null
                    } else {
                        Val::Money(money(&mut r, ctx))
                    }
                }
                Role::Date => Val::Date(2024, 1 + r.below(12), 1 + r.below(28)),
            };
            row.push(v);
        }
        rows.push(row);
    }
    Table { ctx, header: sc.iter().map(|(h, _)| h.to_string()).collect(), roles: sc.iter().map(|(_, r)| *r).collect(), rows }
}

pub fn money_str(c: i64, thousands: bool, decimal_comma: bool) -> String {
    let neg = c < 0;
    let a = c.abs();
    let (int, frac) = (a / 100, a % 100);
    let mut is = int.to_string();
    if thousands && int >= 1000 {
        let b: Vec<char> = is.chars().collect();
        let mut o = String::new();
        for (i, ch) in b.iter().enumerate() {
            if i > 0 && (b.len() - i) % 3 == 0 {
                o.push(',');
            }
            o.push(*ch);
        }
        is = o;
    }
    format!("{}{}{}{:02}", if neg { "-" } else { "" }, is, if decimal_comma { ',' } else { '.' }, frac)
}

/// Canonical money text of an answer (no separators, a point, two decimals).
pub fn money_canon(c: i64) -> String {
    money_str(c, false, false)
}

fn quote_if(s: String, delim: u8) -> String {
    if s.as_bytes().contains(&delim) || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// Render one value as a field of a file.
pub fn field(v: &Val, rd: &Render) -> String {
    match v {
        Val::Null => rd.null_tok.to_string(),
        Val::Int(i) => i.to_string(),
        Val::Money(c) => quote_if(money_str(*c, rd.thousands, rd.decimal_comma), rd.delim),
        Val::Str(s) => quote_if(s.clone(), rd.delim),
        Val::Date(y, m, d) => format!("{y:04}-{m:02}-{d:02}"),
    }
}

/// The malformed line of a fix task (kind 0 extra field, 1 missing field, 2 bad number, 3 bad date).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bad {
    pub row: usize,
    pub kind: u8,
}

/// Render a table to bytes. Returns the bytes and, if `bad` is given, the 1-based physical line of the
/// malformed row.
pub fn render_table(t: &Table, rd: &Render, bad: Option<Bad>) -> (Vec<u8>, Option<usize>) {
    let d = rd.delim as char;
    let sep = if rd.pad { format!("{d} ") } else { d.to_string() };
    let nl = if rd.crlf { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = vec![];
    if rd.header {
        lines.push(t.header.join(&sep));
    }
    let mut bad_line = None;
    for (i, row) in t.rows.iter().enumerate() {
        let mut fs: Vec<String> = row.iter().map(|v| field(v, rd)).collect();
        if let Some(b) = bad.filter(|b| b.row == i) {
            match b.kind {
                0 => {
                    // an unquoted field that contains the delimiter (a broken export)
                    let j = t.col(Role::Text).or(t.col(Role::CatB)).unwrap_or(1);
                    fs[j] = format!("{}{d}{}", fs[j].trim_matches('"'), "x");
                }
                1 => {
                    fs.pop();
                }
                2 => {
                    let j = t.col(Role::Int).unwrap_or(0);
                    fs[j] = format!("{}O", fs[j]);
                }
                _ => {
                    let j = t.col(Role::Date).or(t.col(Role::Int)).unwrap_or(0);
                    fs[j] = if t.roles[j] == Role::Date { "2024-02-31".to_string() } else { format!("{}O", fs[j]) };
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

// ---- derived columns ----

/// Hidden rule of a derived column (all exact in cents). `Tiered` is outside the being's grammar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DRule {
    Product,
    PlusFee(i64),
    Discount,
    Rate(i64),
    DiscountFee(i64),
    DiscountRate(i64),
    Tiered,
}

/// Round num/den half up (den > 0, num >= 0).
pub fn round_div(num: i128, den: i128) -> i128 {
    (2 * num + den).div_euclid(2 * den)
}

impl DRule {
    pub fn apply(self, a: i64, b: i64, d: i64) -> i64 {
        let ab = (a as i128) * (b as i128);
        let v = match self {
            DRule::Product => ab,
            DRule::PlusFee(k) => ab + k as i128,
            DRule::Discount => round_div(ab * (100 - d as i128), 100),
            DRule::Rate(r) => round_div(ab * r as i128, 100),
            DRule::DiscountFee(k) => round_div(ab * (100 - d as i128), 100) + k as i128,
            DRule::DiscountRate(r) => round_div(round_div(ab * (100 - d as i128), 100) * r as i128, 100),
            DRule::Tiered => {
                if a > 40 {
                    (40 * b as i128) + round_div((a as i128 - 40) * b as i128 * 3, 2)
                } else {
                    ab
                }
            }
        };
        v as i64
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    pub ctx: Ctx,
    pub header: [String; 5],
    /// (key, a, b, d, target)
    pub rows: Vec<(String, i64, i64, i64, i64)>,
    pub rule: DRule,
    pub render: Render,
}

pub fn gen_derived(seed: u64, ctx: Ctx, file: u64, size: u8, ood: bool) -> Derived {
    let mut r = Rng::new(mix(&[seed, 0xDE21, ctx.idx() as u64, file]));
    let n = rows_for(&mut r, size).min(200);
    let pick = r.below(1000);
    let fee = if ood { [1234, 777][r.below(2) as usize] } else { [499, 999, 1500, 250][r.below(4) as usize] };
    let rate = if ood { [113, 115, 93][r.below(3) as usize] } else { [107, 110, 119, 120, 125][r.below(5) as usize] };
    // author-defined mix per context
    let rule = match ctx {
        // out of distribution: discounts on payroll, fees on everything, unseen constants
        _ if ood => match pick {
            0..=249 => DRule::DiscountFee(fee),
            250..=499 => DRule::Rate(rate),
            500..=749 => DRule::DiscountRate(rate),
            _ => DRule::Tiered,
        },
        Ctx::Invoices => match pick {
            0..=249 => DRule::Product,
            250..=449 => DRule::Discount,
            450..=599 => DRule::PlusFee(fee),
            600..=749 => DRule::Rate(rate),
            750..=899 => DRule::DiscountRate(rate),
            _ => DRule::DiscountFee(fee),
        },
        _ => match pick {
            0..=299 => DRule::Product,
            300..=549 => DRule::Tiered,
            550..=749 => DRule::Rate(rate),
            _ => DRule::PlusFee(fee),
        },
    };
    let header = if ctx == Ctx::Invoices { ["inv_id", "qty", "unit_price", "discount_pct", "total"] } else { ["emp_id", "hours", "rate", "bonus_pct", "gross"] }.map(String::from);
    let base = r.range(100, 900);
    let mut rows = vec![];
    for i in 0..n {
        let a = if ctx == Ctx::Payroll { r.range(20, 60) } else { 1 + (r.f64().powi(2) * 30.0) as i64 };
        let b = if ctx == Ctx::Payroll { r.range(1500, 6000) } else { money(&mut r, Ctx::Sales).min(500_000) };
        let d = [0, 0, 5, 10, 15, 20][r.below(6) as usize];
        let t = rule.apply(a, b, d);
        rows.push((format!("{}-{}", key_prefix(ctx), base + i as i64), a, b, d, t));
    }
    let render = Render { delim: b',', decimal_comma: false, thousands: false, null_tok: "", crlf: r.below(3) == 0, pad: false, bom: false, header: true };
    Derived { ctx, header, rows, rule, render }
}

/// Render a derived table with the target of `blank` left empty.
pub fn render_derived(dv: &Derived, blank: usize) -> Vec<u8> {
    let nl = if dv.render.crlf { "\r\n" } else { "\n" };
    let mut s = dv.header.join(",");
    s.push_str(nl);
    for (i, (k, a, b, d, t)) in dv.rows.iter().enumerate() {
        let tt = if i == blank { String::new() } else { money_canon(*t) };
        s.push_str(&format!("{k},{a},{},{d},{tt}{nl}", money_canon(*b)));
    }
    s.into_bytes()
}

// ---- time series ----

/// Hidden generating rule of an exact (integer) series. `Weekly` is outside the being's rule grammar
/// (lag 7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SRule {
    Linear,
    Quadratic,
    Cubic,
    /// Out of distribution only: x(n) = x(n-1) + x(n-2) (in the grammar) and x(n) = 2 x(n-1) + c (in the grammar).
    Fib,
    Double,
    Alternating,
    Weekly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub ctx: Ctx,
    pub values: Vec<i64>,
    pub rule: Option<SRule>,
    pub step_secs: i64,
}

pub const EXACT_LEN: usize = 30;
pub const NOISY_LEN: usize = 120;

pub fn gen_exact(seed: u64, ctx: Ctx, file: u64, ood: bool) -> Series {
    let mut r = Rng::new(mix(&[seed, 0x5E21, ctx.idx() as u64, file]));
    let pick = r.below(1000);
    let rule = match ctx {
        _ if ood => match pick {
            0..=399 => SRule::Fib,
            400..=799 => SRule::Double,
            _ => SRule::Weekly,
        },
        Ctx::Meter => match pick {
            0..=599 => SRule::Linear,
            600..=849 => SRule::Quadratic,
            _ => SRule::Weekly,
        },
        Ctx::CumSales => match pick {
            0..=299 => SRule::Linear,
            300..=749 => SRule::Quadratic,
            _ => SRule::Cubic,
        },
        _ => match pick {
            0..=449 => SRule::Alternating,
            450..=799 => SRule::Weekly,
            _ => SRule::Linear,
        },
    };
    let x0 = r.range(100, 5000);
    let c = r.range(3, 60);
    let dd = r.range(1, 6);
    let mut v: Vec<i64> = vec![];
    let week: Vec<i64> = (0..7).map(|_| r.range(20, 200)).collect();
    let (a0, a1) = (r.range(30, 300), r.range(30, 300));
    for n in 0..EXACT_LEN as i64 {
        let x = match rule {
            SRule::Linear => x0 + c * n,
            SRule::Quadratic => x0 + c * n + dd * n * (n - 1) / 2 * 2,
            SRule::Cubic => x0 + c * n + dd * n * n * n,
            SRule::Fib | SRule::Double => 0,
            SRule::Alternating => (if n % 2 == 0 { a0 } else { a1 }) + c * (n / 2),
            SRule::Weekly => week[(n % 7) as usize] + c * (n / 7),
        };
        v.push(x);
    }
    if rule == SRule::Fib {
        v = vec![a0 % 50 + 1, a1 % 50 + 1];
        while v.len() < EXACT_LEN {
            let k = v.len();
            v.push(v[k - 1] + v[k - 2]);
        }
    } else if rule == SRule::Double {
        v = vec![x0 % 200 + 1];
        while v.len() < EXACT_LEN {
            let k = v.len();
            v.push(2 * v[k - 1] + c);
        }
    }
    Series { ctx, values: v, rule: Some(rule), step_secs: if ctx == Ctx::Shifts { 43_200 } else { 86_400 } }
}

pub fn gen_noisy(seed: u64, ctx: Ctx, file: u64) -> Series {
    let mut r = Rng::new(mix(&[seed, 0x0153, ctx.idx() as u64, file]));
    let (period, level) = if ctx == Ctx::Sensor { (24usize, r.range(150, 300) as f64) } else { (7usize, r.range(800, 3000) as f64) };
    let shape: Vec<f64> = (0..period).map(|i| {
        let ph = 2.0 * std::f64::consts::PI * i as f64 / period as f64;
        ph.sin() * 0.25 + (2.0 * ph).cos() * 0.08
    }).collect();
    let noise = [0.003, 0.006, 0.010, 0.016][r.below(4) as usize];
    let trend = (r.f64() - 0.5) * 0.002;
    let mut v = vec![];
    for n in 0..NOISY_LEN {
        let x = level * (1.0 + shape[n % period] + trend * n as f64 + noise * normal(&mut r));
        v.push(x.round() as i64);
    }
    Series { ctx, values: v, rule: None, step_secs: if ctx == Ctx::Sensor { 3600 } else { 86_400 } }
}

/// Render the first `k` points of a series as `timestamp,value` lines (the sandbox shows no more).
pub fn render_series(s: &Series, k: usize) -> Vec<u8> {
    let mut out = String::from(if s.ctx == Ctx::Sensor { "ts,celsius_x10\n" } else { "date,value\n" });
    for (i, x) in s.values.iter().take(k).enumerate() {
        out.push_str(&format!("{},{x}\n", crate::being::world::iso_at(i as i64 * s.step_secs)));
    }
    out.into_bytes()
}

// ---- logs ----

#[derive(Debug, Clone, PartialEq)]
pub struct LogFile {
    pub ctx: Ctx,
    pub bytes: Vec<u8>,
    /// (service, is_error) of every entry (not of continuation lines).
    pub entries: Vec<(String, bool)>,
    pub services: Vec<String>,
}

const SERVICES: [&str; 6] = ["checkout", "auth", "search", "billing", "catalog", "notify"];
const INFO_MSGS: [&str; 5] = ["request served in 12 ms", "cache refreshed", "user session started", "job queued", "health check ok"];
const ERR_MSGS: [&str; 4] = ["upstream timeout", "database connection refused", "payment declined by gateway", "null pointer in handler"];

pub fn gen_log(seed: u64, ctx: Ctx, file: u64, size: u8) -> LogFile {
    let mut r = Rng::new(mix(&[seed, 0x1065, ctx.idx() as u64, file]));
    let n = rows_for(&mut r, size);
    let mut pr = Rng::new(mix(&[seed, 0x10F1, ctx.idx() as u64]));
    // hidden per-context chances (per mille, per line): lower-case level, an "ERROR" word inside an INFO
    // message, a stack trace after an error
    let (lower, inside, trace) = match ctx {
        Ctx::AppLog => (pr.range(0, 100) as u64, pr.range(400, 700) as u64, pr.range(0, 100) as u64),
        _ => (pr.range(200, 400) as u64, pr.range(0, 150) as u64, pr.range(400, 800) as u64),
    };
    let nsvc = 4 + r.below(3) as usize;
    let services: Vec<String> = SERVICES[..nsvc].iter().map(|s| s.to_string()).collect();
    let file_lower = (r.below(1000) as u64) < lower;
    let file_inside = (r.below(1000) as u64) < inside;
    let mut s = String::new();
    let mut entries = vec![];
    for i in 0..n {
        let ts = crate::being::world::iso_at(i as i64 * 7 + r.below(5));
        let svc = &services[zipf_pick(&mut r, nsvc)];
        let x = r.below(100);
        let (lvl, msg, err) = if x < 10 {
            ("ERROR", ERR_MSGS[r.below(4) as usize].to_string(), true)
        } else if x < 25 {
            ("WARN", "slow response from dependency".to_string(), false)
        } else if x < 30 {
            ("DEBUG", "payload size 2048".to_string(), false)
        } else if file_inside && r.below(8) == 0 {
            ("INFO", "retry succeeded after ERROR response from upstream".to_string(), false)
        } else {
            ("INFO", INFO_MSGS[r.below(5) as usize].to_string(), false)
        };
        let lv = if file_lower && r.below(3) == 0 { lvl.to_lowercase() } else { lvl.to_string() };
        s.push_str(&format!("{ts} {lv} [{svc}] {msg}\n"));
        if err && (r.below(1000) as u64) < trace {
            s.push_str(&format!("    at {svc}.Handler.run(Handler.java:{})\n", 10 + r.below(90)));
            s.push_str("    Caused by: java.io.IOException: ERROR reading socket\n");
        }
        entries.push((svc.clone(), err));
    }
    LogFile { ctx, bytes: s.into_bytes(), entries, services }
}
