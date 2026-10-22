//! Small RFC 4180 CSV reader and writer (no external crate).
//!
//! Reader: comma separated, fields may be quoted with `"`, a doubled `""`
//! inside quotes is one quote, quoted fields may contain commas, CR, LF and
//! CRLF. Records end at LF or CRLF (a bare CR outside quotes also ends a
//! record). A leading UTF-8 byte order mark is dropped. Completely empty lines
//! are skipped. A quote in the middle of an unquoted field is kept literally;
//! text after a closing quote is appended literally. An unterminated quoted
//! field is an error.
//!
//! Writer: fields are quoted when they contain a comma, quote, CR or LF, or
//! start or end with a space. Records end with CRLF. No spreadsheet-formula
//! neutralisation is done (a cell starting with `=` is written as is), so be
//! careful opening exports in a spreadsheet.

/// Parse CSV text into records of fields.
pub fn parse(input: &str) -> Result<Vec<Vec<String>>, String> {
    let text = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut rec: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut field_started = false; // a quoted field, or any char, was seen in this field
    let mut line = 1usize;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                '\n' => {
                    line += 1;
                    field.push('\n');
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !field_started => {
                in_quotes = true;
                field_started = true;
            }
            ',' => {
                rec.push(std::mem::take(&mut field));
                field_started = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                line += 1;
                if !(rec.is_empty() && field.is_empty() && !field_started) {
                    rec.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut rec));
                }
                field_started = false;
            }
            _ => {
                field.push(c);
                field_started = true;
            }
        }
    }
    if in_quotes {
        return Err(format!("unterminated quoted field (started before line {line})"));
    }
    if !(rec.is_empty() && field.is_empty() && !field_started) {
        rec.push(field);
        records.push(rec);
    }
    Ok(records)
}

fn needs_quote(f: &str) -> bool {
    f.contains([',', '"', '\r', '\n']) || f.starts_with(' ') || f.ends_with(' ')
}

pub fn write_field(out: &mut String, f: &str) {
    if needs_quote(f) {
        out.push('"');
        for c in f.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
    } else {
        out.push_str(f);
    }
}

pub fn write_record<S: AsRef<str>>(out: &mut String, fields: &[S]) {
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_field(out, f.as_ref());
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<Vec<String>> {
        parse(s).unwrap()
    }
    fn v(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
    }

    #[test]
    fn plain_and_line_endings() {
        assert_eq!(p("a,b,c\n1,2,3\n"), v(&[&["a", "b", "c"], &["1", "2", "3"]]));
        assert_eq!(p("a,b\r\n1,2\r\n"), v(&[&["a", "b"], &["1", "2"]]));
        assert_eq!(p("a,b\n1,2"), v(&[&["a", "b"], &["1", "2"]]));
        assert_eq!(p("a\rb\r"), v(&[&["a"], &["b"]]));
        assert_eq!(p(""), Vec::<Vec<String>>::new());
        assert_eq!(p("\n\n a \n\n"), v(&[&[" a "]]));
        assert_eq!(p("\u{feff}x,y\n"), v(&[&["x", "y"]]));
    }

    #[test]
    fn empty_fields() {
        assert_eq!(p("a,,c\n,,\n"), v(&[&["a", "", "c"], &["", "", ""]]));
        assert_eq!(p("a,\n"), v(&[&["a", ""]]));
        assert_eq!(p("\"\",x\n"), v(&[&["", "x"]]));
        assert_eq!(p("\"\"\n"), v(&[&[""]]));
    }

    #[test]
    fn quotes_commas_newlines() {
        assert_eq!(p("\"a,b\",c\n"), v(&[&["a,b", "c"]]));
        assert_eq!(p("\"say \"\"hi\"\"\",x\n"), v(&[&["say \"hi\"", "x"]]));
        assert_eq!(p("\"line1\nline2\",x\n"), v(&[&["line1\nline2", "x"]]));
        assert_eq!(p("\"line1\r\nline2\",x\r\n"), v(&[&["line1\r\nline2", "x"]]));
        assert_eq!(p("\"\"\"\"\n"), v(&[&["\""]]));
        assert_eq!(p("\"ünï, 日本 😀\",é\n"), v(&[&["ünï, 日本 😀", "é"]]));
    }

    #[test]
    fn lenient_cases_and_errors() {
        assert_eq!(p("ab\"cd,e\n"), v(&[&["ab\"cd", "e"]]));
        assert_eq!(p("\"ab\"cd,e\n"), v(&[&["abcd", "e"]]));
        assert!(parse("\"unterminated,a\n").is_err());
        assert!(parse("a,\"b\n").is_err());
    }

    #[test]
    fn writer_round_trips() {
        let rows = v(&[
            &["plain", "with,comma", "with \"quote\"", "multi\nline", " lead", "trail ", "", "ünï 😀"],
            &["", "", "", "", "", "", "", ""],
            &["a\r\nb", "=SUM(A1)", "\"", ",", "x", "y", "z", "w"],
        ]);
        let mut s = String::new();
        for r in &rows {
            write_record(&mut s, r);
        }
        assert_eq!(parse(&s).unwrap(), rows);
        let mut f = String::new();
        write_field(&mut f, "a\"b");
        assert_eq!(f, "\"a\"\"b\"");
        let mut f = String::new();
        write_field(&mut f, "simple");
        assert_eq!(f, "simple");
    }
}
