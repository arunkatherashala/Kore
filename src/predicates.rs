//! Predicate Pushdown and Query Filtering
//!
//! Provides abstractions for predicates (WHERE clauses) and column selection
//! to enable query optimization and early data filtering.

use std::collections::HashSet;

/// A comparison operator for predicates
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ComparisonOp {
    Equals,
    NotEquals,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

/// A value that can be compared in predicates
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum PredicateValue {
    Int64(i64),
    Float64(f64),
    String(String),
    Boolean(bool),
}

impl PredicateValue {
    /// Check if this value matches the comparison
    pub fn matches(&self, op: &ComparisonOp, other: &PredicateValue) -> bool {
        match (self, other) {
            (PredicateValue::Int64(a), PredicateValue::Int64(b)) => match op {
                ComparisonOp::Equals => a == b,
                ComparisonOp::NotEquals => a != b,
                ComparisonOp::LessThan => a < b,
                ComparisonOp::LessThanOrEqual => a <= b,
                ComparisonOp::GreaterThan => a > b,
                ComparisonOp::GreaterThanOrEqual => a >= b,
            },
            (PredicateValue::Float64(a), PredicateValue::Float64(b)) => match op {
                ComparisonOp::Equals => (a - b).abs() < f64::EPSILON,
                ComparisonOp::NotEquals => (a - b).abs() >= f64::EPSILON,
                ComparisonOp::LessThan => a < b,
                ComparisonOp::LessThanOrEqual => a <= b,
                ComparisonOp::GreaterThan => a > b,
                ComparisonOp::GreaterThanOrEqual => a >= b,
            },
            (PredicateValue::String(a), PredicateValue::String(b)) => match op {
                ComparisonOp::Equals => a == b,
                ComparisonOp::NotEquals => a != b,
                ComparisonOp::LessThan => a < b,
                ComparisonOp::LessThanOrEqual => a <= b,
                ComparisonOp::GreaterThan => a > b,
                ComparisonOp::GreaterThanOrEqual => a >= b,
            },
            (PredicateValue::Boolean(a), PredicateValue::Boolean(b)) => match op {
                ComparisonOp::Equals => a == b,
                ComparisonOp::NotEquals => a != b,
                _ => false, // Boolean comparisons only make sense with Equals/NotEquals
            },
            _ => false, // Type mismatch
        }
    }
}

/// A single column predicate (filter condition)
#[derive(Debug, Clone)]
pub struct ColumnPredicate {
    pub column_name: String,
    pub op: ComparisonOp,
    pub value: PredicateValue,
}

impl ColumnPredicate {
    /// Create a new predicate
    pub fn new(column_name: String, op: ComparisonOp, value: PredicateValue) -> Self {
        ColumnPredicate {
            column_name,
            op,
            value,
        }
    }

    /// Check if a value matches this predicate
    pub fn matches(&self, actual_value: &PredicateValue) -> bool {
        actual_value.matches(&self.op, &self.value)
    }
}

/// Range predicates for min/max filtering (used for block skipping)
#[derive(Debug, Clone)]
pub struct RangePredicate {
    pub column_name: String,
    pub min_value: PredicateValue,
    pub max_value: PredicateValue,
}

impl RangePredicate {
    /// Create a new range predicate
    pub fn new(column_name: String, min_value: PredicateValue, max_value: PredicateValue) -> Self {
        RangePredicate {
            column_name,
            min_value,
            max_value,
        }
    }

    /// Check if a value is in this range
    pub fn contains(&self, value: &PredicateValue) -> bool {
        value >= &self.min_value && value <= &self.max_value
    }
}

/// A combined predicate expression (multiple conditions combined with AND)
#[derive(Debug, Clone)]
pub struct PredicateExpression {
    predicates: Vec<ColumnPredicate>,
}

impl PredicateExpression {
    /// Create a new empty predicate expression
    pub fn new() -> Self {
        PredicateExpression {
            predicates: Vec::new(),
        }
    }

    /// Add a predicate to the expression
    pub fn add(&mut self, predicate: ColumnPredicate) {
        self.predicates.push(predicate);
    }

    /// Check if a set of values matches all predicates
    pub fn matches(&self, values: &[(String, PredicateValue)]) -> bool {
        for predicate in &self.predicates {
            let matches = values
                .iter()
                .find(|(name, _)| name == &predicate.column_name)
                .map(|(_, value)| predicate.matches(value))
                .unwrap_or(false);

            if !matches {
                return false;
            }
        }
        true
    }

    /// Get the number of predicates
    pub fn len(&self) -> usize {
        self.predicates.len()
    }

    /// Check if expression is empty
    pub fn is_empty(&self) -> bool {
        self.predicates.is_empty()
    }

    /// Get all column names referenced in predicates
    pub fn referenced_columns(&self) -> HashSet<String> {
        self.predicates
            .iter()
            .map(|p| p.column_name.clone())
            .collect()
    }
}

impl Default for PredicateExpression {
    fn default() -> Self {
        Self::new()
    }
}

/// Column selection for pruning
#[derive(Debug, Clone)]
pub struct ColumnSelection {
    /// Specific columns to select. If empty, all columns are selected
    selected_columns: HashSet<String>,
}

impl ColumnSelection {
    /// Create a new column selection that selects all columns
    pub fn all() -> Self {
        ColumnSelection {
            selected_columns: HashSet::new(),
        }
    }

    /// Create a new column selection with specific columns
    pub fn new(columns: Vec<String>) -> Self {
        ColumnSelection {
            selected_columns: columns.into_iter().collect(),
        }
    }

    /// Check if a column is selected
    pub fn is_selected(&self, column_name: &str) -> bool {
        // If selection is empty, all columns are selected
        if self.selected_columns.is_empty() {
            return true;
        }
        self.selected_columns.contains(column_name)
    }

    /// Get selected column names
    pub fn columns(&self) -> Vec<&str> {
        self.selected_columns.iter().map(|s| s.as_str()).collect()
    }

    /// Check if all columns are selected
    pub fn is_all(&self) -> bool {
        self.selected_columns.is_empty()
    }

    /// Add a column to selection
    pub fn add(&mut self, column_name: String) {
        self.selected_columns.insert(column_name);
    }
}

impl Default for ColumnSelection {
    fn default() -> Self {
        Self::all()
    }
}

/// Query filter combining predicates and column selection
#[derive(Debug, Clone)]
pub struct QueryFilter {
    pub predicates: PredicateExpression,
    pub column_selection: ColumnSelection,
}

impl QueryFilter {
    /// Create a new query filter
    pub fn new(predicates: PredicateExpression, column_selection: ColumnSelection) -> Self {
        QueryFilter {
            predicates,
            column_selection,
        }
    }

    /// Create an empty filter (no predicates, all columns)
    pub fn empty() -> Self {
        QueryFilter {
            predicates: PredicateExpression::new(),
            column_selection: ColumnSelection::all(),
        }
    }

    /// Check if filter is empty (no predicates and all columns selected)
    pub fn is_empty(&self) -> bool {
        self.predicates.is_empty() && self.column_selection.is_all()
    }
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predicate_value_int_equals() {
        let a = PredicateValue::Int64(42);
        let b = PredicateValue::Int64(42);
        assert!(a.matches(&ComparisonOp::Equals, &b));
    }

    #[test]
    fn test_predicate_value_int_not_equals() {
        let a = PredicateValue::Int64(42);
        let b = PredicateValue::Int64(10);
        assert!(a.matches(&ComparisonOp::NotEquals, &b));
    }

    #[test]
    fn test_predicate_value_int_greater_than() {
        let a = PredicateValue::Int64(42);
        let b = PredicateValue::Int64(10);
        assert!(a.matches(&ComparisonOp::GreaterThan, &b));
    }

    #[test]
    fn test_predicate_value_float_equals() {
        let a = PredicateValue::Float64(3.14);
        let b = PredicateValue::Float64(3.14);
        assert!(a.matches(&ComparisonOp::Equals, &b));
    }

    #[test]
    fn test_predicate_value_string_equals() {
        let a = PredicateValue::String("hello".to_string());
        let b = PredicateValue::String("hello".to_string());
        assert!(a.matches(&ComparisonOp::Equals, &b));
    }

    #[test]
    fn test_column_predicate_matches() {
        let pred = ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        );

        assert!(pred.matches(&PredicateValue::Int64(35)));
        assert!(!pred.matches(&PredicateValue::Int64(25)));
    }

    #[test]
    fn test_predicate_expression_empty() {
        let expr = PredicateExpression::new();
        assert!(expr.is_empty());
        assert_eq!(expr.len(), 0);
    }

    #[test]
    fn test_predicate_expression_single() {
        let mut expr = PredicateExpression::new();
        expr.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));

        assert!(!expr.is_empty());
        assert_eq!(expr.len(), 1);

        let matches = expr.matches(&[("age".to_string(), PredicateValue::Int64(35))]);
        assert!(matches);
    }

    #[test]
    fn test_predicate_expression_multiple() {
        let mut expr = PredicateExpression::new();
        expr.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));
        expr.add(ColumnPredicate::new(
            "score".to_string(),
            ComparisonOp::LessThan,
            PredicateValue::Int64(100),
        ));

        let matches = expr.matches(&[
            ("age".to_string(), PredicateValue::Int64(35)),
            ("score".to_string(), PredicateValue::Int64(90)),
        ]);
        assert!(matches);

        let no_match = expr.matches(&[
            ("age".to_string(), PredicateValue::Int64(35)),
            ("score".to_string(), PredicateValue::Int64(105)),
        ]);
        assert!(!no_match);
    }

    #[test]
    fn test_column_selection_all() {
        let selection = ColumnSelection::all();
        assert!(selection.is_selected("col1"));
        assert!(selection.is_selected("col2"));
        assert!(selection.is_all());
    }

    #[test]
    fn test_column_selection_specific() {
        let selection = ColumnSelection::new(vec!["col1".to_string(), "col2".to_string()]);
        assert!(selection.is_selected("col1"));
        assert!(selection.is_selected("col2"));
        assert!(!selection.is_selected("col3"));
        assert!(!selection.is_all());
    }

    #[test]
    fn test_column_selection_add() {
        let mut selection = ColumnSelection::all();
        selection.add("col1".to_string());
        assert!(selection.is_selected("col1"));
    }

    #[test]
    fn test_query_filter_empty() {
        let filter = QueryFilter::empty();
        assert!(filter.is_empty());
    }

    #[test]
    fn test_query_filter_with_predicates() {
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));

        let filter = QueryFilter::new(predicates, ColumnSelection::all());
        assert!(!filter.is_empty());
    }

    #[test]
    fn test_query_filter_with_column_selection() {
        let filter = QueryFilter::new(
            PredicateExpression::new(),
            ColumnSelection::new(vec!["col1".to_string()]),
        );
        assert!(!filter.is_empty());
    }

    #[test]
    fn test_range_predicate_contains() {
        let range = RangePredicate::new(
            "age".to_string(),
            PredicateValue::Int64(20),
            PredicateValue::Int64(40),
        );

        assert!(range.contains(&PredicateValue::Int64(30)));
        assert!(range.contains(&PredicateValue::Int64(20)));
        assert!(range.contains(&PredicateValue::Int64(40)));
        assert!(!range.contains(&PredicateValue::Int64(50)));
    }
}
