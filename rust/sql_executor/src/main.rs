use kore_fileformat::{ColumnStats, Footer, KoreReader};
use sql_parser::{SQLParser, ast::Expr};

fn expr_can_match(expr: &Expr, col: &ColumnStats, col_name: &str) -> bool {
    match expr {
        Expr::Binary { left, op, right } => {
            // accept qualified names like `t.col0` as well as `col0`
            let left_col = if let Some(idx) = left.rfind('.') { left[idx+1..].to_string() } else { left.clone() };
            if left_col == col_name {
                // support simple numeric comparisons: >, >=, <, <=, =
                if op.eq_ignore_ascii_case("IS") && right.eq_ignore_ascii_case("NULL") {
                    // predicate: col IS NULL -> only possible if null_count > 0
                    return col.null_count > 0;
                }
                if op.eq_ignore_ascii_case("IS") && right.eq_ignore_ascii_case("NOT") {
                    // parser may produce right=="NOT" for 'IS NOT NULL' depending on tokenizer; conservatively assume may match
                    return true;
                }
                if let (Some(min_s), Some(max_s)) = (col.min.as_ref(), col.max.as_ref()) {
                    // Try numeric comparison first
                    if let (Ok(minv), Ok(maxv), Ok(rv)) = (min_s.parse::<f64>(), max_s.parse::<f64>(), right.parse::<f64>()) {
                        match op.as_str() {
                            ">" => return maxv > rv,
                            ">=" => return maxv >= rv,
                            "<" => return minv < rv,
                            "<=" => return minv <= rv,
                            "=" | "==" => return !(rv < minv || rv > maxv),
                            _ => return true,
                        }
                    }

                    // Handle IN list: right like "(1,2,3)" or "('a','b')"
                    if op.eq_ignore_ascii_case("IN") {
                        let items = right.trim();
                        let items = items.trim_start_matches('(').trim_end_matches(')');
                        for token in items.split(',') {
                            let v = token.trim().trim_matches('"').trim_matches('\'');
                            // numeric check
                            if let (Ok(minv), Ok(maxv), Ok(iv)) = (min_s.parse::<f64>(), max_s.parse::<f64>(), v.parse::<f64>()) {
                                if iv >= minv && iv <= maxv { return true; }
                            } else {
                                // string-ish: check if v within min..max lexicographically
                                if v >= min_s && v <= max_s { return true; }
                            }
                        }
                        return false;
                    }

                    // Handle LIKE (simple prefix optimization) -- pattern 'prefix%'
                    if op.eq_ignore_ascii_case("LIKE") {
                        let pat = right.trim().trim_matches('"').trim_matches('\'');
                        if pat.ends_with('%') {
                            let prefix = &pat[..pat.len()-1];
                            if let (Some(min_s), Some(max_s)) = (col.min.as_ref(), col.max.as_ref()) {
                                if min_s.starts_with(prefix) || max_s.starts_with(prefix) { return true; }
                                // if range covers possible prefix lexicographically
                                if min_s.as_str() <= prefix && max_s.as_str() >= prefix { return true; }
                                return false;
                            }
                            return true;
                        }
                        // For other LIKE patterns, be conservative
                        return true;
                    }

                    // String equality: check if literal overlaps range
                    if op == "=" || op == "==" {
                        if right >= min_s && right <= max_s { return true; }
                        return false;
                    }
                }
                true
            } else {
                // not a predicate on this column; conservatively assume it may match
                true
            }
        }
        Expr::Logical { left, op, right } => {
            match op.as_str() {
                "AND" => expr_can_match(left, col, col_name) && expr_can_match(right, col, col_name),
                "OR" => expr_can_match(left, col, col_name) || expr_can_match(right, col, col_name),
                _ => true,
            }
        }
        Expr::Paren(inner) => expr_can_match(inner, col, col_name),
    }
}

fn main() {
    // Demo footer with one column stat (col0: 0..50)
    let stats = vec![ColumnStats { min: Some("0".into()), max: Some("50".into()), null_count: 0 }];
    let footer = Footer::new(1, stats);
    let reader = KoreReader::from_footer(footer);

    // parse SQL and extract WHERE
    let sql = "SELECT * FROM t WHERE col0 > 100";
    let mut p = SQLParser::new(sql);
    let ast = p.parse().expect("parse");
    let where_expr = ast.select.and_then(|s| s.where_clause);

    let col0 = &reader.column_stats()[0];
    if let Some(expr) = where_expr {
        let should_scan = expr_can_match(&expr, col0, "col0");
        println!("SQL: {}", sql);
        println!("Should scan row-group for predicate? {}", should_scan);
    } else {
        println!("No WHERE clause; must scan.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greater_than_no_match() {
        let stats = ColumnStats { min: Some("0".into()), max: Some("50".into()), null_count: 0 };
        // predicate col0 > 100 -> should be false (no match)
        let expr = Expr::Binary { left: "col0".into(), op: ">".into(), right: "100".into() };
        assert!(!expr_can_match(&expr, &stats, "col0"));
    }

    #[test]
    fn test_greater_than_possible() {
        let stats = ColumnStats { min: Some("0".into()), max: Some("150".into()), null_count: 0 };
        let expr = Expr::Binary { left: "t.col0".into(), op: ">".into(), right: "100".into() };
        assert!(expr_can_match(&expr, &stats, "col0"));
    }

    #[test]
    fn test_is_null_false() {
        let stats = ColumnStats { min: Some("0".into()), max: Some("10".into()), null_count: 0 };
        let expr = Expr::Binary { left: "col0".into(), op: "IS".into(), right: "NULL".into() };
        assert!(!expr_can_match(&expr, &stats, "col0"));
    }

    #[test]
    fn test_is_null_true() {
        let stats = ColumnStats { min: Some("0".into()), max: Some("10".into()), null_count: 5 };
        let expr = Expr::Binary { left: "col0".into(), op: "IS".into(), right: "NULL".into() };
        assert!(expr_can_match(&expr, &stats, "col0"));
    }
}
