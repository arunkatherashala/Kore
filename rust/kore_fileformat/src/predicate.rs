/// Very small predicate evaluator prototype.
/// Supported forms (string):
/// - "contains:SUBSTR"  → true if input line contains SUBSTR
/// - "equals:VALUE"     → true if input line == VALUE
pub fn eval_line_predicate(pred: &str, line: &str) -> bool {
    if let Some(rest) = pred.strip_prefix("contains:") {
        return line.contains(rest);
    }
    if let Some(rest) = pred.strip_prefix("equals:") {
        return line == rest;
    }
    // default fallback: substring match
    line.contains(pred)
}

pub fn eval_expression(expr: &str, line: &str) -> bool {
    crate::expression::eval_expr(expr, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains() {
        assert!(eval_line_predicate("contains:foo", "this has foo inside"));
        assert!(!eval_line_predicate("contains:bar", "nope"));
    }

    #[test]
    fn test_equals() {
        assert!(eval_line_predicate("equals:row1", "row1"));
        assert!(!eval_line_predicate("equals:row1", "row1x"));
    }
}
