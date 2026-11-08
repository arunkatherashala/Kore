//! Rule families inside one column.
//!
//! ```text
//! 1 int sequence      nulls | intseq body over the non-null values
//! 2 float decimal     nulls | scale:u8 | n_exc | exception positions (delta varints) | exception bits (u64 planes)
//!                     | intseq body over k, where value = k / 10^scale (bit-exact) for every other non-null value
//! 3 float predicted   nulls | model (0 prev | 1 affine a:u64 b:u64 bits, value = a + i*b) | xor residual (u64 planes)
//! 4 timestamp text    nulls | intseq body over epoch seconds of canonical YYYY-MM-DDTHH:MM:SSZ text
//! 5 text template     nulls | skeletons (embedded v4 Str column; each digit run that is a canonical integer of
//!                     at most 18 digits replaced by 0x00) | n_slots | slot modes (1 = delta to the previous value of
//!                     the same template slot) | n_numbers | numbers (zigzag varints)
//! 6 categorical Markov nulls | D | dictionary (embedded v4 Str column, first-appearance order) | ranks: per value
//!                     its rank in the adaptive frequency list of the previous value's context
//! ```

use kore_core::{ColumnData, KoreError};
use super::intseq;
use super::stream::{self, bad, put_varint, Levels, Rd};
use crate::DType;

pub const INT_SEQ: u8 = 1;
pub const FLOAT_DEC: u8 = 2;
pub const FLOAT_PRED: u8 = 3;
pub const TS_TEXT: u8 = 4;
pub const TEMPLATE: u8 = 5;
pub const MARKOV: u8 = 6;

const POW10: [f64; 16] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15];
const MARKOV_MAX: usize = 1024;

fn nulls_of<T>(v: &[Option<T>]) -> Vec<bool> { v.iter().map(|x| x.is_none()).collect() }

/// Every applicable within-column candidate: `(family, body)`.
pub fn encode(data: &ColumnData, lv: Levels) -> Vec<(u8, Vec<u8>)> {
    let mut out = vec![];
    match data {
        ColumnData::Int64(v) => {
            let vals: Vec<i64> = v.iter().flatten().copied().collect();
            let mut b = vec![];
            stream::put_nulls(&mut b, &nulls_of(v), lv);
            b.extend_from_slice(&intseq::encode(&vals, lv).1);
            out.push((INT_SEQ, b));
        }
        ColumnData::Float64(v) => {
            let vals: Vec<f64> = v.iter().flatten().copied().collect();
            if let Some(b) = float_decimal(&vals, &nulls_of(v), lv) { out.push((FLOAT_DEC, b)); }
            out.push((FLOAT_PRED, float_pred(&vals, &nulls_of(v), lv)));
        }
        ColumnData::Str(v) => {
            let vals: Vec<&str> = v.iter().flatten().map(|s| s.as_str()).collect();
            if !vals.is_empty() {
                let secs: Option<Vec<i64>> = vals.iter().map(|s| crate::codec::parse_ts(s)).collect();
                if let Some(secs) = secs {
                    let mut b = vec![];
                    stream::put_nulls(&mut b, &nulls_of(v), lv);
                    b.extend_from_slice(&intseq::encode(&secs, lv).1);
                    out.push((TS_TEXT, b));
                }
            }
            if let Some(b) = template(&vals, &nulls_of(v), lv) { out.push((TEMPLATE, b)); }
            if let Some(b) = markov(&vals, &nulls_of(v), lv) { out.push((MARKOV, b)); }
        }
        _ => {}
    }
    out
}

fn decode_nulls_values<T>(nulls: &[bool], vals: Vec<T>) -> Result<Vec<Option<T>>, KoreError> {
    let m = nulls.iter().filter(|&&b| !b).count();
    if vals.len() != m { return Err(bad("value count does not match the null bitmap")); }
    let mut it = vals.into_iter();
    Ok(nulls.iter().map(|&b| if b { None } else { it.next() }).collect())
}

pub fn decode(family: u8, rd: &mut Rd, dtype: DType, n: usize) -> Result<ColumnData, KoreError> {
    let nulls = stream::get_nulls(rd, n)?;
    let m = nulls.iter().filter(|&&b| !b).count();
    let need = |want: DType| if dtype == want { Ok(()) } else { Err(bad("rule family does not match the column type")) };
    Ok(match family {
        INT_SEQ => { need(DType::I64)?; ColumnData::Int64(decode_nulls_values(&nulls, intseq::decode(rd, m)?)?) }
        FLOAT_DEC => { need(DType::F64)?; ColumnData::Float64(decode_nulls_values(&nulls, float_decimal_decode(rd, m)?)?) }
        FLOAT_PRED => { need(DType::F64)?; ColumnData::Float64(decode_nulls_values(&nulls, float_pred_decode(rd, m)?)?) }
        TS_TEXT => {
            need(DType::Str)?;
            let secs = intseq::decode(rd, m)?;
            let s: Option<Vec<String>> = secs.into_iter().map(crate::codec::fmt_ts).collect();
            ColumnData::Str(decode_nulls_values(&nulls, s.ok_or_else(|| bad("timestamp out of range"))?)?)
        }
        TEMPLATE => { need(DType::Str)?; ColumnData::Str(decode_nulls_values(&nulls, template_decode(rd, m)?)?) }
        MARKOV => { need(DType::Str)?; ColumnData::Str(decode_nulls_values(&nulls, markov_decode(rd, m)?)?) }
        f => return Err(bad(format!("unknown rule family {f}"))),
    })
}

pub fn describe(family: u8, rd: &mut Rd, n: usize) -> String {
    let nulls = match stream::get_nulls(rd, n) { Ok(x) => x, Err(_) => return "?".into() };
    let m = nulls.iter().filter(|&&b| !b).count();
    match family {
        INT_SEQ => format!("int: {}", intseq::describe(rd, m)),
        FLOAT_DEC => {
            let s = rd.u8().unwrap_or(0);
            let ne = rd.varint().unwrap_or(0);
            let mut skip = || -> Result<(), KoreError> { stream::get_stream(rd, usize::MAX)?; stream::get_stream(rd, usize::MAX)?; Ok(()) };
            let inner = if ne > 0 { skip().ok().map(|_| intseq::describe(rd, m)) } else { Some(intseq::describe(rd, m)) };
            format!("float: k/10^{s}, {ne} exceptions, k: {}", inner.unwrap_or_else(|| "?".into()))
        }
        FLOAT_PRED => format!("float: xor residual vs {}", match rd.u8() { Ok(0) => "previous value", Ok(1) => "affine a+i*b", _ => "?" }),
        TS_TEXT => format!("timestamp text: {}", intseq::describe(rd, m)),
        TEMPLATE => "text template + numeric slots".into(),
        MARKOV => "categorical Markov ranks".into(),
        _ => "?".into(),
    }
}

// ── float: decimal scale ─────────────────────────────────────────────────────

fn dec_k(x: f64, p: f64) -> Option<i64> {
    if !x.is_finite() { return None; }
    let y = x * p;
    if !(y.abs() < 9.0e15) { return None; }
    let k = y.round() as i64;
    if ((k as f64) / p).to_bits() == x.to_bits() { Some(k) } else { None }
}

/// Smallest scale at which at least 7/8 of a sample is exactly k / 10^s.
fn find_scale(vals: &[f64]) -> Option<usize> {
    let step = (vals.len() / 4096).max(1);
    let sample: Vec<f64> = vals.iter().step_by(step).copied().collect();
    if sample.is_empty() { return None; }
    (0..POW10.len()).find(|&s| sample.iter().filter(|&&x| dec_k(x, POW10[s]).is_some()).count() * 8 >= sample.len() * 7)
}

fn float_decimal(vals: &[f64], nulls: &[bool], lv: Levels) -> Option<Vec<u8>> {
    let s = find_scale(vals)?;
    let p = POW10[s];
    let (mut ks, mut exc_pos, mut exc_bits) = (Vec::with_capacity(vals.len()), vec![], vec![]);
    for (i, &x) in vals.iter().enumerate() {
        match dec_k(x, p) { Some(k) => ks.push(k), None => { exc_pos.push(i); exc_bits.push(x.to_bits()); } }
    }
    let mut b = vec![];
    stream::put_nulls(&mut b, nulls, lv);
    b.push(s as u8);
    put_varint(&mut b, exc_pos.len() as u64);
    if !exc_pos.is_empty() {
        let mut pb = vec![];
        let mut prev = 0usize;
        for (j, &i) in exc_pos.iter().enumerate() { put_varint(&mut pb, (if j == 0 { i } else { i - prev - 1 }) as u64); prev = i; }
        stream::put_stream(&mut b, &pb, lv);
        stream::put_u64_stream(&mut b, &exc_bits, lv);
    }
    b.extend_from_slice(&intseq::encode(&ks, lv).1);
    Some(b)
}

fn float_decimal_decode(rd: &mut Rd, m: usize) -> Result<Vec<f64>, KoreError> {
    let s = rd.u8()? as usize;
    if s >= POW10.len() { return Err(bad("decimal scale out of range")); }
    let p = POW10[s];
    let ne = rd.count(m)?;
    let (mut pos, mut bits) = (vec![], vec![]);
    if ne > 0 {
        let gaps = stream::get_uint_stream(rd, ne, m as u64)?;
        let mut prev: Option<usize> = None;
        for g in gaps {
            let i = match prev { None => g as usize, Some(q) => q.checked_add(1 + g as usize).ok_or_else(|| bad("overflow"))? };
            if i >= m { return Err(bad("exception position out of range")); }
            pos.push(i);
            prev = Some(i);
        }
        bits = stream::get_u64_stream(rd, ne)?;
    }
    let ks = intseq::decode(rd, m - ne)?;
    let mut out = Vec::with_capacity(m);
    let (mut e, mut k) = (0, 0);
    for i in 0..m {
        if e < pos.len() && pos[e] == i { out.push(f64::from_bits(bits[e])); e += 1; } else { out.push((ks[k] as f64) / p); k += 1; }
    }
    Ok(out)
}

// ── float: predicted, xor residual ───────────────────────────────────────────

#[derive(Clone, Copy)]
enum FModel { Prev, Affine(f64, f64) }

fn fpred(m: FModel, v: &[f64], i: usize) -> u64 {
    match m {
        FModel::Prev => if i > 0 { v[i - 1].to_bits() } else { 0 },
        FModel::Affine(a, b) => (a + (i as f64) * b).to_bits(),
    }
}

fn float_pred(vals: &[f64], nulls: &[bool], lv: Levels) -> Vec<u8> {
    let m = vals.len();
    let mut models = vec![FModel::Prev];
    if m >= 2 {
        models.push(FModel::Affine(vals[0], (vals[m - 1] - vals[0]) / ((m - 1) as f64)));
        models.push(FModel::Affine(vals[0], vals[1] - vals[0]));
    }
    let resid = |md: FModel, r: std::ops::Range<usize>| -> Vec<u64> { r.map(|i| vals[i].to_bits() ^ fpred(md, vals, i)).collect() };
    let mut best = (usize::MAX, 0usize);
    for (j, &md) in models.iter().enumerate() {
        let sc: usize = stream::windows(m, 8192).into_iter().map(|w| stream::score(&stream::shuffle8(&resid(md, w)))).sum();
        if sc < best.0 { best = (sc, j); }
    }
    let md = models[best.1];
    let mut b = vec![];
    stream::put_nulls(&mut b, nulls, lv);
    match md {
        FModel::Prev => b.push(0),
        FModel::Affine(a, c) => { b.push(1); b.extend_from_slice(&a.to_bits().to_le_bytes()); b.extend_from_slice(&c.to_bits().to_le_bytes()); }
    }
    stream::put_u64_stream(&mut b, &resid(md, 0..m), lv);
    b
}

fn float_pred_decode(rd: &mut Rd, m: usize) -> Result<Vec<f64>, KoreError> {
    let md = match rd.u8()? {
        0 => FModel::Prev,
        1 => FModel::Affine(f64::from_bits(rd.u64le()?), f64::from_bits(rd.u64le()?)),
        _ => return Err(bad("unknown float model")),
    };
    let r = stream::get_u64_stream(rd, m)?;
    let mut v: Vec<f64> = Vec::with_capacity(m);
    for (i, x) in r.into_iter().enumerate() { let p = fpred(md, &v, i); v.push(f64::from_bits(p ^ x)); }
    Ok(v)
}

// ── text template ────────────────────────────────────────────────────────────

/// Skeleton (0x00 per numeric slot) and the slot values; None if `s` contains 0x00.
fn split_template(s: &str, nums: &mut Vec<i64>) -> Option<String> {
    let b = s.as_bytes();
    if b.contains(&0) { return None; }
    let mut sk = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() { j += 1; }
            let run = &b[i..j];
            if run.len() <= 18 && (run.len() == 1 || run[0] != b'0') {
                nums.push(run.iter().fold(0i64, |a, &c| a * 10 + (c - b'0') as i64));
                sk.push(0);
            } else {
                sk.extend_from_slice(run);
            }
            i = j;
        } else { sk.push(b[i]); i += 1; }
    }
    Some(String::from_utf8(sk).expect("ascii digits replaced by 0x00 keep UTF-8"))
}

fn template(vals: &[&str], nulls: &[bool], lv: Levels) -> Option<Vec<u8>> {
    if vals.is_empty() { return None; }
    let mut nums_row: Vec<Vec<i64>> = Vec::with_capacity(vals.len());
    let mut sks: Vec<Option<String>> = Vec::with_capacity(vals.len());
    let mut ids: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut tid = Vec::with_capacity(vals.len());
    let mut slots_of: Vec<usize> = vec![];
    for s in vals {
        let mut nums = vec![];
        let sk = split_template(s, &mut nums)?;
        let n_ids = ids.len();
        let id = *ids.entry(sk.clone()).or_insert(n_ids);
        if id == slots_of.len() { slots_of.push(nums.len()); }
        if ids.len() > vals.len() / 2 + 16 { return None; }
        tid.push(id);
        sks.push(Some(sk));
        nums_row.push(nums);
    }
    let total: usize = nums_row.iter().map(|x| x.len()).sum();
    if total == 0 { return None; }
    // per template slot: delta to the previous value of the same slot, or plain (smaller varint total wins)
    let base: Vec<usize> = slots_of.iter().scan(0, |a, &k| { let b = *a; *a += k; Some(b) }).collect();
    let n_slots: usize = slots_of.iter().sum();
    let (mut cost_plain, mut cost_delta, mut prev) = (vec![0usize; n_slots], vec![0usize; n_slots], vec![0i64; n_slots]);
    for (r, nums) in nums_row.iter().enumerate() {
        for (j, &x) in nums.iter().enumerate() {
            let k = base[tid[r]] + j;
            cost_plain[k] += stream::varint_len(stream::zz(x));
            cost_delta[k] += stream::varint_len(stream::zz(x.wrapping_sub(prev[k])));
            prev[k] = x;
        }
    }
    let modes: Vec<u8> = (0..n_slots).map(|k| (cost_delta[k] < cost_plain[k]) as u8).collect();
    let mut prev = vec![0i64; n_slots];
    let mut nb = Vec::with_capacity(total * 2);
    for (r, nums) in nums_row.iter().enumerate() {
        for (j, &x) in nums.iter().enumerate() {
            let k = base[tid[r]] + j;
            stream::put_zz(&mut nb, if modes[k] == 1 { x.wrapping_sub(prev[k]) } else { x });
            prev[k] = x;
        }
    }
    let mut b = vec![];
    stream::put_nulls(&mut b, nulls, lv);
    stream::put_v4col(&mut b, &ColumnData::Str(sks), lv).ok()?;
    put_varint(&mut b, n_slots as u64);
    stream::put_stream(&mut b, &modes, lv);
    put_varint(&mut b, total as u64);
    stream::put_stream(&mut b, &nb, lv);
    Some(b)
}

fn template_decode(rd: &mut Rd, m: usize) -> Result<Vec<String>, KoreError> {
    let sks = match stream::get_v4col(rd, DType::Str, m)? { ColumnData::Str(v) => v, _ => return Err(bad("skeleton column type")) };
    let zeros: usize = sks.iter().flatten().map(|s| s.bytes().filter(|&c| c == 0).count()).sum();
    let n_slots = rd.count(zeros)?;
    let modes = stream::get_stream(rd, n_slots)?;
    if modes.len() != n_slots || modes.iter().any(|&x| x > 1) { return Err(bad("bad template slot modes")); }
    let total = rd.count(zeros)?;
    let nums = stream::get_zz_stream(rd, total)?;
    let mut ids: std::collections::HashMap<&str, (usize, usize)> = std::collections::HashMap::new();
    let mut next_base = 0usize;
    let mut prev = vec![0i64; n_slots];
    let mut q = 0usize;
    let mut out = Vec::with_capacity(m);
    for sk in &sks {
        let sk = sk.as_deref().ok_or_else(|| bad("null skeleton"))?;
        let k = sk.bytes().filter(|&c| c == 0).count();
        let (base, _) = match ids.get(sk) {
            Some(&e) => e,
            None => {
                let e = (next_base, k);
                next_base = next_base.checked_add(k).ok_or_else(|| bad("overflow"))?;
                if next_base > n_slots { return Err(bad("more template slots than declared")); }
                ids.insert(sk, e);
                e
            }
        };
        let mut s = String::with_capacity(sk.len() + 8 * k);
        let mut j = 0;
        for part in sk.split('\0') {
            if j > 0 {
                let slot = base + j - 1;
                let r = *nums.get(q).ok_or_else(|| bad("too few template numbers"))?;
                q += 1;
                let x = if modes[slot] == 1 { prev[slot].wrapping_add(r) } else { r };
                if !(0..=999_999_999_999_999_999).contains(&x) { return Err(bad("template number out of range")); }
                prev[slot] = x;
                s.push_str(&x.to_string());
            }
            s.push_str(part);
            j += 1;
        }
        out.push(s);
    }
    if q != nums.len() || next_base != n_slots { return Err(bad("template numbers do not match the skeletons")); }
    Ok(out)
}

// ── categorical: adaptive Markov ranks ───────────────────────────────────────

/// Per context (previous symbol, or D at the start) a list of all symbols ordered by how often they
/// followed that context (ties: earlier in the list); a symbol's count is incremented after use and it
/// moves up past symbols with a smaller count.
struct Markov { d: usize, list: Vec<Vec<u16>>, pos: Vec<Vec<u16>>, cnt: Vec<Vec<u32>> }

impl Markov {
    fn new(d: usize) -> Self { Markov { d, list: vec![vec![]; d + 1], pos: vec![vec![]; d + 1], cnt: vec![vec![]; d + 1] } }
    fn ctx(&mut self, c: usize) {
        if self.list[c].is_empty() {
            self.list[c] = (0..self.d as u16).collect();
            self.pos[c] = (0..self.d as u16).collect();
            self.cnt[c] = vec![0; self.d];
        }
    }
    fn update(&mut self, c: usize, sym: usize) {
        let (list, pos, cnt) = (&mut self.list[c], &mut self.pos[c], &mut self.cnt[c]);
        cnt[sym] += 1;
        let mut p = pos[sym] as usize;
        while p > 0 && cnt[list[p - 1] as usize] < cnt[sym] {
            let other = list[p - 1];
            list[p] = other;
            pos[other as usize] = p as u16;
            p -= 1;
        }
        list[p] = sym as u16;
        pos[sym] = p as u16;
    }
}

fn markov(vals: &[&str], nulls: &[bool], lv: Levels) -> Option<Vec<u8>> {
    if vals.len() < 64 { return None; }
    let mut ids: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut dict: Vec<Option<String>> = vec![];
    let mut codes = Vec::with_capacity(vals.len());
    for s in vals {
        let n = ids.len();
        let c = *ids.entry(s).or_insert(n);
        if c == dict.len() { dict.push(Some(s.to_string())); if dict.len() > MARKOV_MAX { return None; } }
        codes.push(c);
    }
    let d = dict.len();
    let mut mk = Markov::new(d);
    let mut ranks = Vec::with_capacity(vals.len());
    let mut ctx = d;
    for &c in &codes {
        mk.ctx(ctx);
        put_varint(&mut ranks, mk.pos[ctx][c] as u64);
        mk.update(ctx, c);
        ctx = c;
    }
    let mut b = vec![];
    stream::put_nulls(&mut b, nulls, lv);
    put_varint(&mut b, d as u64);
    stream::put_v4col(&mut b, &ColumnData::Str(dict), lv).ok()?;
    stream::put_stream(&mut b, &ranks, lv);
    Some(b)
}

fn markov_decode(rd: &mut Rd, m: usize) -> Result<Vec<String>, KoreError> {
    let d = rd.count(MARKOV_MAX)?;
    let dict = match stream::get_v4col(rd, DType::Str, d)? { ColumnData::Str(v) => v, _ => return Err(bad("dictionary type")) };
    let dict: Vec<String> = dict.into_iter().map(|x| x.ok_or_else(|| bad("null in dictionary"))).collect::<Result<_, _>>()?;
    let ranks = stream::get_uint_stream(rd, m, d.saturating_sub(1) as u64)?;
    if m > 0 && d == 0 { return Err(bad("empty dictionary")); }
    let mut mk = Markov::new(d);
    let mut out = Vec::with_capacity(m);
    let mut ctx = d;
    for r in ranks {
        mk.ctx(ctx);
        let c = mk.list[ctx][r as usize] as usize;
        out.push(dict[c].clone());
        mk.update(ctx, c);
        ctx = c;
    }
    Ok(out)
}
