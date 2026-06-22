/// Simple boolean expression evaluator for line predicates.
/// Supported grammar (small):
/// expr := term (" OR " term)*
/// term := factor (" AND " factor)*
/// factor := predicate | '(' expr ')'
/// predicate := same forms as `predicate::eval_line_predicate` (contains:, equals: or substring)
use crate::predicate as base_pred;

fn eval_term(term: &str, line: &str) -> bool {
    // term: factors joined by AND
    let parts: Vec<&str> = term.split(" AND ").map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    for p in parts { if !eval_factor(p, line) { return false; } }
    true
}

fn eval_factor(f: &str, line: &str) -> bool {
    let f = f.trim();
    if f.starts_with('(') && f.ends_with(')') {
        return eval_expr(&f[1..f.len()-1], line);
    }
    base_pred::eval_line_predicate(f, line)
}

pub fn eval_expr(expr: &str, line: &str) -> bool {
    // split by OR at top level (no nested OR inside parentheses handled separately by factor)
    let parts: Vec<&str> = expr.split(" OR ").map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    for t in parts {
        if eval_term(t, line) { return true; }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_or_and() {
        let line = "a foo b";
        assert!(eval_expr("contains:foo OR contains:bar", line));
        assert!(eval_expr("contains:foo AND contains:a", line));
        assert!(!eval_expr("contains:xxx OR equals:nomatch", line));
        assert!(eval_expr("(contains:foo AND contains:a) OR equals:nomatch", line));
    }
}
