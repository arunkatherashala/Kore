//! Best-effort redaction of obvious personal identifiers before event text is
//! stored. Hand-written matchers (no regex crate is available offline).
//!
//! What it recognises:
//! * e-mail addresses (`local@domain.tld`, unicode letters allowed in the local part),
//! * phone-like numbers (10-15 digits that contain a separator, a leading `+` or
//!   parentheses; each digit group of at most 4 digits unless the number starts
//!   with `+`; dotted IP addresses, ISO dates and decimals are left alone),
//! * card-like numbers: 13-19 digits (optionally grouped with single spaces or
//!   hyphens) that pass the Luhn check,
//! * US social security numbers in the form `123-45-6789` (area 000/666/9xx,
//!   group 00 and serial 0000 are not matched),
//! * user patterns from `redact_patterns.txt` in the owner directory: one plain
//!   substring per line, case-insensitive, `#` starts a comment, patterns shorter
//!   than 3 characters are ignored.
//!
//! Each match is replaced by `[REDACTED:<kind>]` with kind one of `email`,
//! `phone`, `card`, `ssn`, `custom`.
//!
//! This is best-effort, not a guarantee. It misses names, addresses, account
//! numbers in unusual layouts, numbers split by other characters, and anything
//! written in words. It can also redact harmless text (a 13-digit number that
//! happens to pass Luhn, a long phone-shaped serial). A pattern is only
//! recognised when the characters on both sides are not letters or digits, so
//! `x4111111111111111` is left alone. Do not rely on it for compliance.
//!
//! Redaction is deterministic, so the same input always yields the same stored
//! text (and therefore the same event id). Changing `redact_patterns.txt`
//! later changes the text of future events; already stored events are not
//! rewritten.

use std::path::Path;

pub const PATTERN_FILE: &str = "redact_patterns.txt";
const MAX_PATTERNS: usize = 1000;
const MIN_PATTERN_CHARS: usize = 3;

#[derive(Debug, Clone, Default)]
pub struct Redactor {
    patterns: Vec<Vec<char>>,
    /// Pattern lines that were ignored (too short or beyond the cap).
    pub ignored_patterns: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Redaction {
    pub text: String,
    /// Distinct kinds that were replaced, sorted.
    pub kinds: Vec<&'static str>,
    /// Number of replaced spans.
    pub count: usize,
}

impl Redaction {
    pub fn changed(&self) -> bool {
        self.count > 0
    }
}

#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    kind: &'static str,
    prio: u8,
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl Redactor {
    /// Built-in matchers only.
    pub fn builtin() -> Redactor {
        Redactor::default()
    }

    /// Built-in matchers plus the substrings listed in `text` (file format above).
    pub fn with_patterns(text: &str) -> Redactor {
        let mut r = Redactor::default();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            if l.chars().count() < MIN_PATTERN_CHARS || r.patterns.len() >= MAX_PATTERNS {
                r.ignored_patterns += 1;
                continue;
            }
            r.patterns.push(l.chars().map(lower).collect());
        }
        r
    }

    /// Load `<owner_dir>/redact_patterns.txt` if it exists (a missing or
    /// unreadable file means built-in matchers only).
    pub fn load(owner_dir: &Path) -> Redactor {
        match std::fs::read_to_string(owner_dir.join(PATTERN_FILE)) {
            Ok(t) => Redactor::with_patterns(&t),
            Err(_) => Redactor::builtin(),
        }
    }

    pub fn redact(&self, text: &str) -> Redaction {
        let chars: Vec<char> = text.chars().collect();
        let mut spans: Vec<Span> = Vec::new();
        self.custom_spans(&chars, &mut spans);
        email_spans(&chars, &mut spans);
        ssn_spans(&chars, &mut spans);
        card_spans(&chars, &mut spans);
        phone_spans(&chars, &mut spans);
        spans.sort_by(|a, b| a.start.cmp(&b.start).then(a.prio.cmp(&b.prio)).then((b.end - b.start).cmp(&(a.end - a.start))));
        let mut out = String::with_capacity(text.len());
        let mut pos = 0usize;
        let mut kinds: Vec<&'static str> = Vec::new();
        let mut count = 0usize;
        for s in spans {
            if s.start < pos {
                continue;
            }
            out.extend(&chars[pos..s.start]);
            out.push_str("[REDACTED:");
            out.push_str(s.kind);
            out.push(']');
            if !kinds.contains(&s.kind) {
                kinds.push(s.kind);
            }
            count += 1;
            pos = s.end;
        }
        out.extend(&chars[pos..]);
        kinds.sort();
        Redaction { text: out, kinds, count }
    }

    fn custom_spans(&self, chars: &[char], spans: &mut Vec<Span>) {
        if self.patterns.is_empty() {
            return;
        }
        let low: Vec<char> = chars.iter().map(|c| lower(*c)).collect();
        for p in &self.patterns {
            let mut i = 0;
            while i + p.len() <= low.len() {
                if low[i..i + p.len()] == p[..] {
                    spans.push(Span { start: i, end: i + p.len(), kind: "custom", prio: 0 });
                    i += p.len();
                } else {
                    i += 1;
                }
            }
        }
    }
}

// ─── e-mail ──────────────────────────────────────────────────────────────────

fn is_local(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

fn email_spans(chars: &[char], spans: &mut Vec<Span>) {
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        let mut j = i;
        while j > 0 && is_local(chars[j - 1]) {
            j -= 1;
        }
        while j < i && matches!(chars[j], '.' | '_' | '%' | '+' | '-') {
            j += 1;
        }
        let mut k = i + 1;
        while k < n && (chars[k].is_alphanumeric() || chars[k] == '-' || chars[k] == '.') {
            k += 1;
        }
        while k > i + 1 && matches!(chars[k - 1], '.' | '-') {
            k -= 1;
        }
        let domain = &chars[i + 1..k];
        let labels: Vec<&[char]> = domain.split(|c| *c == '.').collect();
        let tld_ok = labels.last().map_or(false, |t| t.len() >= 2 && t.iter().all(|c| c.is_alphabetic()));
        let ok = j < i && labels.len() >= 2 && labels.iter().all(|l| !l.is_empty() && l[0] != '-' && l[l.len() - 1] != '-') && tld_ok;
        if ok {
            spans.push(Span { start: j, end: k, kind: "email", prio: 1 });
            i = k;
        } else {
            i += 1;
        }
    }
}

// ─── SSN ─────────────────────────────────────────────────────────────────────

fn d(chars: &[char], i: usize) -> bool {
    chars.get(i).map_or(false, |c| c.is_ascii_digit())
}

fn ssn_spans(chars: &[char], spans: &mut Vec<Span>) {
    let n = chars.len();
    let mut i = 0;
    while i + 11 <= n {
        let shape = (0..3).all(|k| d(chars, i + k)) && chars[i + 3] == '-' && (4..6).all(|k| d(chars, i + k)) && chars[i + 6] == '-' && (7..11).all(|k| d(chars, i + k));
        let before_ok = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '-');
        let after_ok = i + 11 == n || !(chars[i + 11].is_alphanumeric() || chars[i + 11] == '-');
        if shape && before_ok && after_ok {
            let area: String = chars[i..i + 3].iter().collect();
            let group: String = chars[i + 4..i + 6].iter().collect();
            let serial: String = chars[i + 7..i + 11].iter().collect();
            if area != "000" && area != "666" && !area.starts_with('9') && group != "00" && serial != "0000" {
                spans.push(Span { start: i, end: i + 11, kind: "ssn", prio: 2 });
                i += 11;
                continue;
            }
        }
        i += 1;
    }
}

// ─── card-like numbers (Luhn) ────────────────────────────────────────────────

pub fn luhn_ok(digits: &[u8]) -> bool {
    let mut sum = 0u32;
    for (i, dg) in digits.iter().rev().enumerate() {
        let mut v = *dg as u32;
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    !digits.is_empty() && sum % 10 == 0
}

/// Maximal digit runs joined by single `' '` or `'-'` separators, starting at
/// a digit whose predecessor is not a letter or digit.
fn digit_chain(chars: &[char], start: usize) -> Vec<(usize, usize)> {
    let n = chars.len();
    let mut groups = Vec::new();
    let mut p = start;
    loop {
        let s = p;
        while p < n && chars[p].is_ascii_digit() {
            p += 1;
        }
        groups.push((s, p));
        if groups.len() >= 10 {
            break;
        }
        if p + 1 < n && (chars[p] == ' ' || chars[p] == '-') && chars[p + 1].is_ascii_digit() {
            p += 1;
        } else {
            break;
        }
    }
    groups
}

fn card_spans(chars: &[char], spans: &mut Vec<Span>) {
    let n = chars.len();
    let mut p = 0;
    while p < n {
        if !chars[p].is_ascii_digit() || (p > 0 && (chars[p - 1].is_alphanumeric())) {
            p += 1;
            continue;
        }
        let groups = digit_chain(chars, p);
        let chain_end = groups.last().map(|g| g.1).unwrap_or(p + 1);
        let mut hit = None;
        'outer: for a in 0..groups.len() {
            for b in (a..groups.len()).rev() {
                let digits: Vec<u8> = (groups[a].0..groups[b].1).filter(|&k| chars[k].is_ascii_digit()).map(|k| chars[k] as u8 - b'0').collect();
                let end = groups[b].1;
                let after_ok = end == n || !chars[end].is_alphanumeric();
                let before_ok = groups[a].0 == 0 || !chars[groups[a].0 - 1].is_alphanumeric();
                if (13..=19).contains(&digits.len()) && after_ok && before_ok && luhn_ok(&digits) {
                    hit = Some((groups[a].0, end));
                    break 'outer;
                }
            }
        }
        match hit {
            Some((s, e)) => {
                spans.push(Span { start: s, end: e, kind: "card", prio: 3 });
                p = e;
            }
            None => p = chain_end.max(p + 1),
        }
    }
}

// ─── phone-like numbers ──────────────────────────────────────────────────────

fn iso_date_at(chars: &[char], i: usize) -> bool {
    i + 10 <= chars.len()
        && (0..4).all(|k| d(chars, i + k))
        && chars[i + 4] == '-'
        && (5..7).all(|k| d(chars, i + k))
        && chars[i + 7] == '-'
        && (8..10).all(|k| d(chars, i + k))
        && !d(chars, i + 10)
}

fn phone_spans(chars: &[char], spans: &mut Vec<Span>) {
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let starts = c.is_ascii_digit() || c == '+' || c == '(';
        if !starts || (i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '+')) {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() && iso_date_at(chars, i) {
            i += 10;
            continue;
        }
        // parse greedily
        let plus = c == '+';
        let mut p = i + usize::from(plus);
        let mut digits = 0usize;
        let mut end = p;
        let mut seps = 0usize;
        let mut dot_seps = 0usize;
        let mut groups: Vec<usize> = Vec::new();
        let mut cur = 0usize;
        let mut prev_digit = false;
        let mut open_paren = false;
        while p < n {
            let ch = chars[p];
            if ch.is_ascii_digit() {
                digits += 1;
                cur += 1;
                prev_digit = true;
                end = p + 1;
            } else if matches!(ch, ' ' | '-' | '.') && prev_digit && (d(chars, p + 1) || (chars[p..].get(1) == Some(&'('))) {
                seps += 1;
                if ch == '.' {
                    dot_seps += 1;
                }
                groups.push(cur);
                cur = 0;
                prev_digit = false;
            } else if ch == '(' && !prev_digit && !open_paren && d(chars, p + 1) {
                open_paren = true;
                seps += 1;
                prev_digit = false;
            } else if ch == ')' && open_paren && prev_digit {
                open_paren = false;
                groups.push(cur);
                cur = 0;
                prev_digit = false;
                end = p + 1;
                // a ')' may be followed by a space or digit
                if p + 1 < n && chars[p + 1] == ' ' && d(chars, p + 2) {
                    p += 1;
                } else if !(p + 1 < n && chars[p + 1].is_ascii_digit()) {
                    p += 1;
                    break;
                }
            } else {
                break;
            }
            p += 1;
        }
        if cur > 0 {
            groups.push(cur);
        }
        let is_ip = dot_seps == 3 && seps == 3 && groups.len() == 4 && groups.iter().all(|g| *g <= 3);
        let group_ok = plus || groups.iter().all(|g| *g <= 4);
        let boundary_ok = end == n || !chars[end].is_alphanumeric();
        let shape_ok = plus || seps > 0;
        if (10..=15).contains(&digits) && shape_ok && group_ok && boundary_ok && !is_ip && !open_paren {
            spans.push(Span { start: i, end, kind: "phone", prio: 4 });
            i = end;
        } else {
            i = if c.is_ascii_digit() { skip_digits(chars, i) } else { i + 1 };
        }
    }
}

fn skip_digits(chars: &[char], i: usize) -> usize {
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    j.max(i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> String {
        Redactor::builtin().redact(s).text
    }

    #[test]
    fn emails() {
        assert_eq!(r("write to jane.doe+tag@example.com now"), "write to [REDACTED:email] now");
        assert_eq!(r("a@b.co, c@d.org."), "[REDACTED:email], [REDACTED:email].");
        assert_eq!(r("(bob@mail.example.com)"), "([REDACTED:email])");
        assert_eq!(r("józef.müller@exämple.de wrote"), "[REDACTED:email] wrote");
        assert_eq!(r("日本@例え.jp"), "[REDACTED:email]");
        // not emails
        for s in ["@handle", "user@", "user@localhost", "a@b", "a @ b.com", "x@y.c", "name@.com", "@@", "mail me @ noon"] {
            assert_eq!(r(s), s, "{s}");
        }
    }

    #[test]
    fn ssn_boundaries() {
        assert_eq!(r("ssn 123-45-6789."), "ssn [REDACTED:ssn].");
        assert_eq!(r("123-45-6789"), "[REDACTED:ssn]");
        for s in ["000-12-3456", "666-12-3456", "900-12-3456", "123-00-4567", "123-45-0000", "1123-45-6789", "123-45-67890", "a123-45-6789", "123-45-6789-1"] {
            assert!(!r(s).contains("[REDACTED:ssn]"), "{s} -> {}", r(s));
        }
    }

    #[test]
    fn cards_use_luhn() {
        assert_eq!(r("card 4111 1111 1111 1111 expires"), "card [REDACTED:card] expires");
        assert_eq!(r("4111-1111-1111-1111"), "[REDACTED:card]");
        assert_eq!(r("4111111111111111"), "[REDACTED:card]");
        assert_eq!(r("amex 378282246310005!"), "amex [REDACTED:card]!");
        assert_eq!(r("2024-03-01 4111111111111111"), "2024-03-01 [REDACTED:card]");
        assert_eq!(r("→4111 1111 1111 1111←"), "→[REDACTED:card]←");
        // fails Luhn: not a card (its tail may still look phone-like, which is fine)
        assert!(!r("4111 1111 1111 1112").contains("[REDACTED:card]"));
        assert_eq!(r("4111111111111112"), "4111111111111112");
        // letters touching the digits: untouched (documented)
        assert_eq!(r("x4111111111111111"), "x4111111111111111");
        assert_eq!(r("4111111111111111x"), "4111111111111111x");
        // too short / too long
        assert_eq!(r("411111111111"), "411111111111");
        assert_eq!(r("41111111111111111111"), "41111111111111111111");
    }

    #[test]
    fn luhn_function() {
        assert!(luhn_ok(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]));
        assert!(!luhn_ok(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2]));
        assert!(!luhn_ok(&[]));
    }

    #[test]
    fn phones() {
        for s in ["call +1 555 123 4567 please", "call (555) 123-4567 please", "call 555-123-4567 please", "call 555.123.4567 please", "+442071838750 now"] {
            assert!(r(s).contains("[REDACTED:phone]"), "{s} -> {}", r(s));
            assert!(!r(s).contains("555") && !r(s).contains("2071838750"), "{s} -> {}", r(s));
        }
        assert_eq!(r("call 555-123-4567."), "call [REDACTED:phone].");
        // not phones
        for s in [
            "on 2024-03-01 12 people came",
            "version 1.2.3.4",
            "ip 192.168.100.100 up",
            "pi is 3.14159265358979",
            "order 1234567890",
            "room 12-34",
            "at 12:30 on 2024-03-01",
            "id abc1234567890",
            "1 2 3 4 5 6 7 8 9",
        ] {
            assert_eq!(r(s), s, "{s}");
        }
    }

    #[test]
    fn custom_patterns_case_insensitive_and_unicode() {
        let red = Redactor::with_patterns("# comment\nProject Falcon\n\nMüller\nab\n");
        assert_eq!(red.ignored_patterns, 1);
        let out = red.redact("we met MÜLLER about project falcon, ab stays");
        assert_eq!(out.text, "we met [REDACTED:custom] about [REDACTED:custom], ab stays");
        assert_eq!(out.kinds, vec!["custom"]);
        assert_eq!(out.count, 2);
        // overlapping with builtin: earliest/highest priority wins, no double replace
        let red = Redactor::with_patterns("jane.doe\n");
        let t = red.redact("jane.doe@example.com").text;
        assert!(t == "[REDACTED:custom]@example.com" || t == "[REDACTED:email]", "{t}");
    }

    #[test]
    fn multiple_kinds_reported() {
        let out = Redactor::builtin().redact("mail a@b.com, ssn 123-45-6789, card 4111111111111111, tel 555-123-4567");
        assert_eq!(out.kinds, vec!["card", "email", "phone", "ssn"]);
        assert_eq!(out.count, 4);
        assert_eq!(out.text, "mail [REDACTED:email], ssn [REDACTED:ssn], card [REDACTED:card], tel [REDACTED:phone]");
    }

    #[test]
    fn idempotent_and_clean_text_untouched() {
        let red = Redactor::with_patterns("secret thing\n");
        for s in ["a@b.com and 4111111111111111 and 555-123-4567 and secret thing", "plain text, nothing here", "", "ünïcödé ✓ 日本語 😀"] {
            let once = red.redact(s);
            let twice = red.redact(&once.text);
            assert_eq!(once.text, twice.text, "{s}");
            assert!(!twice.changed(), "second pass must find nothing: {}", twice.text);
        }
        assert!(!red.redact("plain text, nothing here").changed());
    }

    #[test]
    fn never_panics_on_odd_input() {
        let alphabet: Vec<char> = "0123456789 -+.()@aZ_é日😀\n\t:".chars().collect();
        let mut x = 0x9E3779B97F4A7C15u64;
        let red = Redactor::with_patterns("abc\n日本\n");
        for _ in 0..3000 {
            let mut s = String::new();
            for _ in 0..(x % 40) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                s.push(alphabet[(x % alphabet.len() as u64) as usize]);
            }
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let out = red.redact(&s);
            assert!(out.text.chars().count() <= s.chars().count() * 20 + 20);
        }
        // truncated multi-byte boundaries via char iteration cannot split
        assert_eq!(r("é@"), "é@");
    }

    #[test]
    fn load_missing_file_is_builtin() {
        let d = tempfile::tempdir().unwrap();
        assert!(Redactor::load(d.path()).patterns.is_empty());
        std::fs::write(d.path().join(PATTERN_FILE), "hidden word\n").unwrap();
        assert_eq!(Redactor::load(d.path()).redact("a Hidden Word b").text, "a [REDACTED:custom] b");
    }
}
