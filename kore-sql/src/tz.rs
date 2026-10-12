//! Time zones for `from_utc_timestamp` / `to_utc_timestamp`. No tz database is bundled. Supported: UTC / GMT / Z,
//! fixed offsets (`+05:30`, `UTC+2`), and a built-in table of common zones with their current rules (US 2007+,
//! EU 1996+, AU 2008+, fixed-offset zones from the year their offset last changed). Dates outside those years are
//! an error rather than a guess.

use crate::datetime::{civil_from_days, days_from_civil, days_in_month};

#[derive(Clone, Copy)]
enum Rule { None, Us, Eu, Au }

struct Zone { std: i64, rule: Rule, from_year: i64 }

fn parse_offset(s: &str) -> Option<i64> {
    let (sign, rest) = match s.chars().next()? { '+' => (1, &s[1..]), '-' => (-1, &s[1..]), _ => return None };
    let mut it = rest.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next().map(|x| x.parse().ok()).unwrap_or(Some(0))?;
    let sec: i64 = it.next().map(|x| x.parse().ok()).unwrap_or(Some(0))?;
    if h > 18 || m > 59 || sec > 59 { return None; }
    Some(sign * (h * 3600 + m * 60 + sec))
}

fn zone_of(name: &str) -> Option<Zone> {
    let n = name.trim();
    let up = n.to_ascii_uppercase();
    if matches!(up.as_str(), "UTC" | "GMT" | "Z" | "UT" | "ETC/UTC" | "ETC/GMT" | "UCT" | "ZULU") { return Some(Zone { std: 0, rule: Rule::None, from_year: 0 }); }
    if let Some(off) = parse_offset(n) { return Some(Zone { std: off, rule: Rule::None, from_year: 0 }); }
    for p in ["UTC", "GMT", "UT"] {
        if let Some(r) = up.strip_prefix(p) { if let Some(off) = parse_offset(r) { return Some(Zone { std: off, rule: Rule::None, from_year: 0 }); } }
    }
    let h = 3600;
    let (std, rule, from) = match n {
        "America/New_York" | "US/Eastern" | "America/Toronto" | "America/Detroit" | "EST5EDT" => (-5 * h, Rule::Us, 2007),
        "America/Chicago" | "US/Central" | "CST6CDT" => (-6 * h, Rule::Us, 2007),
        "America/Denver" | "US/Mountain" | "MST7MDT" => (-7 * h, Rule::Us, 2007),
        "America/Los_Angeles" | "US/Pacific" | "America/Vancouver" | "PST8PDT" => (-8 * h, Rule::Us, 2007),
        "America/Anchorage" | "US/Alaska" => (-9 * h, Rule::Us, 2007),
        "America/Phoenix" | "US/Arizona" => (-7 * h, Rule::None, 1968),
        "Pacific/Honolulu" | "US/Hawaii" => (-10 * h, Rule::None, 1947),
        "Europe/London" | "Europe/Dublin" | "Europe/Lisbon" => (0, Rule::Eu, 1996),
        "Europe/Paris" | "Europe/Berlin" | "Europe/Madrid" | "Europe/Rome" | "Europe/Amsterdam" | "Europe/Brussels" | "Europe/Vienna"
        | "Europe/Zurich" | "Europe/Stockholm" | "Europe/Oslo" | "Europe/Copenhagen" | "Europe/Prague" | "Europe/Warsaw" | "Europe/Budapest"
        | "Europe/Belgrade" | "Europe/Zagreb" | "Europe/Luxembourg" | "Europe/Monaco" => (h, Rule::Eu, 1996),
        "Europe/Athens" | "Europe/Helsinki" | "Europe/Kiev" | "Europe/Kyiv" | "Europe/Bucharest" | "Europe/Sofia" | "Europe/Riga"
        | "Europe/Tallinn" | "Europe/Vilnius" => (2 * h, Rule::Eu, 1996),
        "Australia/Sydney" | "Australia/Melbourne" | "Australia/Canberra" => (10 * h, Rule::Au, 2008),
        "Australia/Brisbane" => (10 * h, Rule::None, 1992),
        "Asia/Kolkata" | "Asia/Calcutta" => (5 * h + 1800, Rule::None, 1946),
        "Asia/Kathmandu" => (5 * h + 2700, Rule::None, 1986),
        "Asia/Tokyo" | "Asia/Seoul" => (9 * h, Rule::None, 1962),
        "Asia/Shanghai" | "Asia/Hong_Kong" | "Asia/Singapore" | "Asia/Manila" | "Asia/Taipei" => (8 * h, Rule::None, 1992),
        "Asia/Dubai" => (4 * h, Rule::None, 1920),
        "Asia/Karachi" => (5 * h, Rule::None, 2009),
        "Asia/Dhaka" => (6 * h, Rule::None, 2010),
        "Asia/Bangkok" | "Asia/Jakarta" | "Asia/Ho_Chi_Minh" => (7 * h, Rule::None, 1983),
        "Asia/Riyadh" | "Europe/Istanbul" | "Europe/Moscow" | "Africa/Nairobi" => (3 * h, Rule::None, 2016),
        "Africa/Johannesburg" => (2 * h, Rule::None, 1944),
        "Africa/Lagos" => (h, Rule::None, 1920),
        "America/Sao_Paulo" | "America/Argentina/Buenos_Aires" | "America/Buenos_Aires" => (-3 * h, Rule::None, 2019),
        "America/Bogota" | "America/Lima" => (-5 * h, Rule::None, 1993),
        _ => return None,
    };
    Some(Zone { std, rule, from_year: from })
}

/// n-th (1-based) given weekday (0 = Sunday) of a month as days since the epoch; n = 0 means the last one.
fn nth_dow(y: i64, m: i64, dow: i64, n: i64) -> i64 {
    let wd = |d: i64| (d + 4).rem_euclid(7);
    if n == 0 {
        let last = days_from_civil(y, m, days_in_month(y, m));
        last - (wd(last) - dow).rem_euclid(7)
    } else {
        let first = days_from_civil(y, m, 1);
        first + (dow - wd(first)).rem_euclid(7) + (n - 1) * 7
    }
}

/// Is daylight saving time in force at this UTC instant?
fn dst_active(z: &Zone, utc: i64) -> bool {
    let y = civil_from_days(utc.div_euclid(86400)).0;
    match z.rule {
        Rule::None => false,
        Rule::Us => {
            let start = nth_dow(y, 3, 0, 2) * 86400 + 2 * 3600 - z.std;
            let end = nth_dow(y, 11, 0, 1) * 86400 + 2 * 3600 - (z.std + 3600);
            utc >= start && utc < end
        }
        Rule::Eu => {
            let start = nth_dow(y, 3, 0, 0) * 86400 + 3600;
            let end = nth_dow(y, 10, 0, 0) * 86400 + 3600;
            utc >= start && utc < end
        }
        Rule::Au => {
            let end = nth_dow(y, 4, 0, 1) * 86400 + 3 * 3600 - (z.std + 3600);
            let start = nth_dow(y, 10, 0, 1) * 86400 + 2 * 3600 - z.std;
            utc < end || utc >= start
        }
    }
}

fn check_year(z: &Zone, name: &str, t: i64) -> Result<(), String> {
    let y = civil_from_days(t.div_euclid(86400)).0;
    if y < z.from_year || y > 2200 {
        Err(format!("time zone '{name}' is only supported for years {}..2200 (no tz database is bundled)", z.from_year))
    } else { Ok(()) }
}

/// Offset (seconds east of UTC) of zone `name` at the UTC instant `utc` (epoch seconds).
pub fn offset_at_utc(name: &str, utc: i64) -> Result<i64, String> {
    let z = zone_of(name).ok_or_else(|| format!("unknown time zone '{name}'"))?;
    check_year(&z, name, utc)?;
    Ok(z.std + if dst_active(&z, utc) { 3600 } else { 0 })
}

/// UTC instant of the local wall-clock time `local` (epoch seconds as if local were UTC) in zone `name`.
/// Overlaps resolve to the earlier instant (daylight offset); gaps shift forward, as java.time does.
pub fn local_to_utc(name: &str, local: i64) -> Result<i64, String> {
    let z = zone_of(name).ok_or_else(|| format!("unknown time zone '{name}'"))?;
    check_year(&z, name, local)?;
    let u_dst = local - (z.std + 3600);
    let u_std = local - z.std;
    Ok(if dst_active(&z, u_dst) { u_dst } else { u_std })
}
