//! Calendar helpers for the SQL date/time functions. Dates are `YYYY-MM-DD` strings and timestamps
//! `YYYY-MM-DD HH:MM:SS[.fff]` strings (UTC, no time zones); everything here works on civil days and
//! seconds so that month/year arithmetic clips to month ends the way Spark does.

use std::cmp::Ordering;

/// A calendar instant: whole days since 1970-01-01 plus seconds of day plus nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dt {
    pub days: i64,
    pub secs: i64,
    pub nanos: i64,
    /// the text had a time-of-day part
    pub has_time: bool,
}

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn is_leap(y: i64) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }

pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m { 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, 4 | 6 | 9 | 11 => 30, _ => if is_leap(y) { 29 } else { 28 } }
}

pub fn valid_ymd(y: i64, m: i64, d: i64) -> bool {
    (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m) && (0..=9999).contains(&y)
}

/// Parse `YYYY-MM-DD`, `YYYY-M-D`, optionally followed by `[ T]HH:MM[:SS[.fff]]`.
pub fn parse(s: &str) -> Option<Dt> {
    let s = s.trim();
    let (date_part, time_part) = match s.find(|c| c == ' ' || c == 'T') {
        Some(i) => (&s[..i], Some(s[i + 1..].trim())),
        None => (s, None),
    };
    let mut it = date_part.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next().map(|x| x.parse().ok()).unwrap_or(Some(1))?;
    let d: i64 = it.next().map(|x| x.parse().ok()).unwrap_or(Some(1))?;
    if it.next().is_some() || !valid_ymd(y, m, d) { return None; }
    let (mut secs, mut nanos, mut has_time) = (0, 0, false);
    if let Some(t) = time_part {
        if !t.is_empty() {
            let t = t.trim_end_matches('Z');
            let mut parts = t.split(':');
            let h: i64 = parts.next()?.parse().ok()?;
            let mi: i64 = parts.next().map(|x| x.parse().ok()).unwrap_or(Some(0))?;
            let sec_str = parts.next().unwrap_or("0");
            let (sec, frac) = match sec_str.split_once('.') {
                Some((a, b)) => (a.parse::<i64>().ok()?, b),
                None => (sec_str.parse::<i64>().ok()?, ""),
            };
            if !(0..24).contains(&h) || !(0..60).contains(&mi) || !(0..60).contains(&sec) { return None; }
            let mut fr = frac.chars().take(9).collect::<String>();
            while fr.len() < 9 { fr.push('0'); }
            nanos = if frac.is_empty() { 0 } else { fr.parse().ok()? };
            secs = h * 3600 + mi * 60 + sec;
            has_time = true;
        }
    }
    Some(Dt { days: days_from_civil(y, m, d), secs, nanos, has_time })
}

/// Legacy support: integers shaped like YYYYMMDD are dates.
pub fn from_yyyymmdd(i: i64) -> Option<Dt> {
    if !(10000101..=99991231).contains(&i) { return None; }
    let (y, m, d) = (i / 10000, (i / 100) % 100, i % 100);
    if !valid_ymd(y, m, d) { return None; }
    Some(Dt { days: days_from_civil(y, m, d), secs: 0, nanos: 0, has_time: false })
}

pub fn fmt_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

pub fn fmt_ts(days: i64, secs: i64, nanos: i64) -> String {
    let base = format!("{} {:02}:{:02}:{:02}", fmt_date(days), secs / 3600, (secs / 60) % 60, secs % 60);
    if nanos == 0 { base } else {
        let mut f = format!("{:09}", nanos);
        while f.ends_with('0') { f.pop(); }
        format!("{base}.{f}")
    }
}

pub fn fmt(dt: &Dt) -> String {
    if dt.has_time { fmt_ts(dt.days, dt.secs, dt.nanos) } else { fmt_date(dt.days) }
}

impl Dt {
    pub fn ymd(&self) -> (i64, i64, i64) { civil_from_days(self.days) }
    pub fn from_epoch_secs(s: i64) -> Dt {
        Dt { days: s.div_euclid(86400), secs: s.rem_euclid(86400), nanos: 0, has_time: true }
    }
    pub fn epoch_secs(&self) -> i64 { self.days * 86400 + self.secs }
    /// Monday = 0 .. Sunday = 6
    pub fn weekday(&self) -> i64 { (self.days + 3).rem_euclid(7) }
    pub fn day_of_year(&self) -> i64 { let (y, _, _) = self.ymd(); self.days - days_from_civil(y, 1, 1) + 1 }
    pub fn add_months(&self, n: i64) -> Dt {
        let (y, m, d) = self.ymd();
        let total = y * 12 + (m - 1) + n;
        let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
        let nd = d.min(days_in_month(ny, nm));
        Dt { days: days_from_civil(ny, nm, nd), ..*self }
    }
    pub fn add_days(&self, n: i64) -> Dt { Dt { days: self.days + n, ..*self } }
    pub fn add_secs(&self, n: i64) -> Dt {
        let t = self.epoch_secs() + n;
        Dt { days: t.div_euclid(86400), secs: t.rem_euclid(86400), nanos: self.nanos, has_time: true }
    }
    /// ISO-8601 week number (weeks start on Monday, week 1 contains the first Thursday).
    pub fn iso_week(&self) -> i64 {
        let thursday = self.days - self.weekday() + 3;
        let (ty, _, _) = civil_from_days(thursday);
        (thursday - days_from_civil(ty, 1, 1)) / 7 + 1
    }
}

pub fn cmp(a: &Dt, b: &Dt) -> Ordering {
    (a.days, a.secs, a.nanos).cmp(&(b.days, b.secs, b.nanos))
}

pub const MONTH_NAMES: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
pub const DAY_NAMES: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// Unit names accepted by DATEADD / DATEDIFF / DATE_TRUNC / INTERVAL, normalised.
pub fn norm_unit(u: &str) -> Option<&'static str> {
    Some(match u.trim().to_ascii_lowercase().trim_end_matches('s') {
        "year" | "yyyy" | "yy" | "y" => "year",
        "quarter" | "qq" | "q" => "quarter",
        "month" | "mon" | "mm" | "m" => "month",
        "week" | "wk" | "ww" => "week",
        "day" | "dd" | "d" | "dayofmonth" => "day",
        "hour" | "hh" | "h" => "hour",
        "minute" | "mi" | "n" => "minute",
        "second" | "ss" | "s" | "sec" => "second",
        _ => return None,
    })
}

/// Truncate to a unit (`year`, `quarter`, `month`, `week` (Monday), `day`, `hour`, `minute`, `second`).
pub fn trunc(dt: &Dt, unit: &str) -> Option<Dt> {
    let (y, m, _) = dt.ymd();
    let date = |days: i64| Dt { days, secs: 0, nanos: 0, has_time: dt.has_time };
    Some(match unit {
        "year" => date(days_from_civil(y, 1, 1)),
        "quarter" => date(days_from_civil(y, ((m - 1) / 3) * 3 + 1, 1)),
        "month" => date(days_from_civil(y, m, 1)),
        "week" => date(dt.days - dt.weekday()),
        "day" => date(dt.days),
        "hour" => Dt { secs: dt.secs / 3600 * 3600, nanos: 0, has_time: true, ..*dt },
        "minute" => Dt { secs: dt.secs / 60 * 60, nanos: 0, has_time: true, ..*dt },
        "second" => Dt { nanos: 0, has_time: true, ..*dt },
        _ => return None,
    })
}

/// Whole units from `a` to `b` (truncated toward zero), like TIMESTAMPDIFF.
pub fn diff_units(unit: &str, a: &Dt, b: &Dt) -> Option<i64> {
    Some(match unit {
        "year" | "quarter" | "month" => {
            let (ay, am, ad) = a.ymd();
            let (by, bm, bd) = b.ymd();
            let mut months = (by * 12 + bm) - (ay * 12 + am);
            // not a whole month yet when the day/time of `b` is before that of `a`
            let a_tail = (ad, a.secs, a.nanos);
            let b_tail = (bd, b.secs, b.nanos);
            if months > 0 && b_tail < a_tail { months -= 1; }
            if months < 0 && b_tail > a_tail { months += 1; }
            match unit { "year" => months / 12, "quarter" => months / 3, _ => months }
        }
        "week" => (b.epoch_secs() - a.epoch_secs()) / (7 * 86400),
        "day" => (b.epoch_secs() - a.epoch_secs()) / 86400,
        "hour" => (b.epoch_secs() - a.epoch_secs()) / 3600,
        "minute" => (b.epoch_secs() - a.epoch_secs()) / 60,
        "second" => b.epoch_secs() - a.epoch_secs(),
        _ => return None,
    })
}

/// `dt + n units`.
pub fn add_unit(dt: &Dt, unit: &str, n: i64) -> Option<Dt> {
    Some(match unit {
        "year" => dt.add_months(n * 12),
        "quarter" => dt.add_months(n * 3),
        "month" => dt.add_months(n),
        "week" => dt.add_days(n * 7),
        "day" => dt.add_days(n),
        "hour" => dt.add_secs(n * 3600),
        "minute" => dt.add_secs(n * 60),
        "second" => dt.add_secs(n),
        _ => return None,
    })
}

/// Spark `months_between`.
pub fn months_between(a: &Dt, b: &Dt, round: bool) -> f64 {
    let (ay, am, ad) = a.ymd();
    let (by, bm, bd) = b.ymd();
    let months = ((ay - by) * 12 + (am - bm)) as f64;
    let a_last = ad == days_in_month(ay, am);
    let b_last = bd == days_in_month(by, bm);
    if ad == bd || (a_last && b_last) { return months; }
    let sec_a = (ad - 1) as f64 * 86400.0 + a.secs as f64;
    let sec_b = (bd - 1) as f64 * 86400.0 + b.secs as f64;
    let r = months + (sec_a - sec_b) / (31.0 * 86400.0);
    if round { (r * 1e8).round() / 1e8 } else { r }
}

/// Date to the next given weekday (`Mon`, `Tuesday`, ...), strictly after `dt`.
pub fn next_day(dt: &Dt, name: &str) -> Option<Dt> {
    let n = name.trim().to_ascii_lowercase();
    let target = DAY_NAMES.iter().position(|d| { let dl = d.to_ascii_lowercase(); n == dl || (n.len() == 2 && dl.starts_with(&n)) || (n.len() == 3 && dl.starts_with(&n)) })? as i64;
    let delta = (target - dt.weekday() + 6) % 7 + 1;
    Some(Dt { days: dt.days + delta, secs: 0, nanos: 0, has_time: false })
}

/// Format with a Java `DateTimeFormatter` style pattern (the commonly used letters).
pub fn format_pattern(dt: &Dt, pat: &str) -> String {
    let chars: Vec<char> = pat.chars().collect();
    let (y, m, d) = dt.ymd();
    let (h, mi, s) = (dt.secs / 3600, (dt.secs / 60) % 60, dt.secs % 60);
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i += 1;
            if i < chars.len() && chars[i] == '\'' { out.push('\''); i += 1; continue; }
            while i < chars.len() && chars[i] != '\'' { out.push(chars[i]); i += 1; }
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() { out.push(c); i += 1; continue; }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c { n += 1; }
        match c {
            'y' | 'u' => if n == 2 { out.push_str(&format!("{:02}", y % 100)) } else { out.push_str(&format!("{:0w$}", y, w = n.max(1))) },
            'M' | 'L' => match n {
                1 => out.push_str(&m.to_string()),
                2 => out.push_str(&format!("{:02}", m)),
                3 => out.push_str(&MONTH_NAMES[(m - 1) as usize][..3]),
                _ => out.push_str(MONTH_NAMES[(m - 1) as usize]),
            },
            'd' => out.push_str(&format!("{:0w$}", d, w = n)),
            'D' => out.push_str(&format!("{:0w$}", dt.day_of_year(), w = n)),
            'H' => out.push_str(&format!("{:0w$}", h, w = n)),
            'h' => out.push_str(&format!("{:0w$}", if h % 12 == 0 { 12 } else { h % 12 }, w = n)),
            'm' => out.push_str(&format!("{:0w$}", mi, w = n)),
            's' => out.push_str(&format!("{:0w$}", s, w = n)),
            'S' => { let f = format!("{:09}", dt.nanos); out.push_str(&f[..n.min(9)]); }
            'E' => { let name = DAY_NAMES[dt.weekday() as usize]; if n >= 4 { out.push_str(name) } else { out.push_str(&name[..3]) } }
            'e' => out.push_str(&(((dt.weekday() + 1) % 7) + 1).to_string()),
            'a' => out.push_str(if h < 12 { "AM" } else { "PM" }),
            'Q' | 'q' => out.push_str(&((m - 1) / 3 + 1).to_string()),
            _ => { for _ in 0..n { out.push(c); } }
        }
        i += n;
    }
    out
}

/// Parse text with a Java style pattern (numeric fields, month names, literals).
pub fn parse_pattern(s: &str, pat: &str) -> Option<Dt> {
    let sc: Vec<char> = s.chars().collect();
    let pc: Vec<char> = pat.chars().collect();
    let (mut y, mut mo, mut d, mut h, mut mi, mut sec, mut pm) = (1970i64, 1i64, 1i64, 0i64, 0i64, 0i64, None::<bool>);
    let mut has_time = false;
    let (mut i, mut j) = (0usize, 0usize);
    while j < pc.len() {
        let c = pc[j];
        if c == '\'' {
            j += 1;
            while j < pc.len() && pc[j] != '\'' { if sc.get(i) != Some(&pc[j]) { return None; } i += 1; j += 1; }
            j += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            if sc.get(i) != Some(&c) { return None; }
            i += 1; j += 1;
            continue;
        }
        let mut n = 1;
        while j + n < pc.len() && pc[j + n] == c { n += 1; }
        j += n;
        let read_num = |i: &mut usize, min: usize, max: usize| -> Option<i64> {
            let st = *i;
            while *i < sc.len() && *i - st < max && sc[*i].is_ascii_digit() { *i += 1; }
            if *i - st < min { return None; }
            sc[st..*i].iter().collect::<String>().parse().ok()
        };
        // a numeric field followed directly by another letter field has a fixed width
        let fixed = j < pc.len() && pc[j].is_ascii_alphabetic();
        match c {
            'y' | 'u' => { y = read_num(&mut i, 1, if n == 2 { 2 } else if fixed { n } else { 9 })?; if n == 2 { y += 2000; } }
            'M' | 'L' if n >= 3 => {
                let rest: String = sc[i..].iter().collect::<String>().to_ascii_lowercase();
                let idx = MONTH_NAMES.iter().position(|mn| { let l = mn.to_ascii_lowercase(); if n == 3 { rest.starts_with(&l[..3]) } else { rest.starts_with(&l) } })?;
                mo = idx as i64 + 1;
                i += if n == 3 { 3 } else { MONTH_NAMES[idx].len() };
            }
            'M' | 'L' => mo = read_num(&mut i, 1, if n == 2 || fixed { 2 } else { 2 })?,
            'd' => d = read_num(&mut i, 1, 2)?,
            'H' => { h = read_num(&mut i, 1, 2)?; has_time = true; }
            'h' => { h = read_num(&mut i, 1, 2)?; has_time = true; }
            'm' => { mi = read_num(&mut i, 1, 2)?; has_time = true; }
            's' => { sec = read_num(&mut i, 1, 2)?; has_time = true; }
            'S' => { read_num(&mut i, 1, 9)?; }
            'a' => {
                let t: String = sc.get(i..i + 2)?.iter().collect::<String>().to_ascii_uppercase();
                pm = Some(match t.as_str() { "AM" => false, "PM" => true, _ => return None });
                i += 2;
            }
            _ => return None,
        }
    }
    if i != sc.len() { return None; }
    if let Some(p) = pm { h = h % 12 + if p { 12 } else { 0 }; }
    if !valid_ymd(y, mo, d) || h > 23 || mi > 59 || sec > 59 { return None; }
    Some(Dt { days: days_from_civil(y, mo, d), secs: h * 3600 + mi * 60 + sec, nanos: 0, has_time })
}

pub fn now() -> Dt {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let mut dt = Dt::from_epoch_secs(d.as_secs() as i64);
    dt.nanos = d.subsec_nanos() as i64;
    dt
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civil_roundtrip() {
        for z in [-800000, -1, 0, 1, 19782, 2932896] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(fmt_date(days_from_civil(2024, 2, 29)), "2024-02-29");
    }
    #[test]
    fn month_clipping() {
        let d = parse("2024-01-31").unwrap();
        assert_eq!(fmt(&d.add_months(1)), "2024-02-29");
        assert_eq!(fmt(&parse("2024-02-29").unwrap().add_months(12)), "2025-02-28");
    }
    #[test]
    fn iso_week_and_weekday() {
        let d = parse("2024-03-05").unwrap();
        assert_eq!(d.iso_week(), 10);
        assert_eq!(d.weekday(), 1); // Tuesday
        assert_eq!(parse("2021-01-03").unwrap().iso_week(), 53);
    }
}
