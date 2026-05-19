//! Phase 3.1 - Predicate Pushdown and Column Pruning Tests
//! 
//! Comprehensive test suite for query optimization including:
//! - Column selection and pruning
//! - Predicate filtering
//! - Range optimization
//! - Integration with DuckDB

#[cfg(test)]
mod phase_3_1_tests {
    use kore_fileformat::predicates::*;
    use std::collections::HashSet;

    // ============================================================================
    // SECTION 1: Basic Predicate Tests (5 tests)
    // ============================================================================

    #[test]
    fn test_int_predicate_equality() {
        let pred = ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::Equals,
            PredicateValue::Int64(42),
        );
        assert!(pred.matches(&PredicateValue::Int64(42)));
        assert!(!pred.matches(&PredicateValue::Int64(43)));
    }

    #[test]
    fn test_int_predicate_greater_than() {
        let pred = ColumnPredicate::new(
            "score".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(100),
        );
        assert!(pred.matches(&PredicateValue::Int64(101)));
        assert!(!pred.matches(&PredicateValue::Int64(100)));
        assert!(!pred.matches(&PredicateValue::Int64(99)));
    }

    #[test]
    fn test_float_predicate_range() {
        let pred = ColumnPredicate::new(
            "rating".to_string(),
            ComparisonOp::GreaterThanOrEqual,
            PredicateValue::Float64(3.5),
        );
        assert!(pred.matches(&PredicateValue::Float64(3.5)));
        assert!(pred.matches(&PredicateValue::Float64(4.0)));
        assert!(!pred.matches(&PredicateValue::Float64(3.4)));
    }

    #[test]
    fn test_string_predicate_equality() {
        let pred = ColumnPredicate::new(
            "name".to_string(),
            ComparisonOp::Equals,
            PredicateValue::String("Alice".to_string()),
        );
        assert!(pred.matches(&PredicateValue::String("Alice".to_string())));
        assert!(!pred.matches(&PredicateValue::String("Bob".to_string())));
    }

    #[test]
    fn test_boolean_predicate_equality() {
        let pred = ColumnPredicate::new(
            "active".to_string(),
            ComparisonOp::Equals,
            PredicateValue::Boolean(true),
        );
        assert!(pred.matches(&PredicateValue::Boolean(true)));
        assert!(!pred.matches(&PredicateValue::Boolean(false)));
    }

    // ============================================================================
    // SECTION 2: Predicate Expression Tests (8 tests)
    // ============================================================================

    #[test]
    fn test_predicate_expression_empty() {
        let expr = PredicateExpression::new();
        assert!(expr.is_empty());
        assert_eq!(expr.len(), 0);
    }

    #[test]
    fn test_predicate_expression_single_predicate() {
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
    fn test_predicate_expression_and_logic() {
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
        
        // Both match
        let both = expr.matches(&[
            ("age".to_string(), PredicateValue::Int64(35)),
            ("score".to_string(), PredicateValue::Int64(90)),
        ]);
        assert!(both);
        
        // Age fails
        let age_fail = expr.matches(&[
            ("age".to_string(), PredicateValue::Int64(25)),
            ("score".to_string(), PredicateValue::Int64(90)),
        ]);
        assert!(!age_fail);
        
        // Score fails
        let score_fail = expr.matches(&[
            ("age".to_string(), PredicateValue::Int64(35)),
            ("score".to_string(), PredicateValue::Int64(105)),
        ]);
        assert!(!score_fail);
    }

    #[test]
    fn test_predicate_expression_three_conditions() {
        let mut expr = PredicateExpression::new();
        expr.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));
        expr.add(ColumnPredicate::new(
            "score".to_string(),
            ComparisonOp::GreaterThanOrEqual,
            PredicateValue::Int64(80),
        ));
        expr.add(ColumnPredicate::new(
            "active".to_string(),
            ComparisonOp::Equals,
            PredicateValue::Boolean(true),
        ));
        
        assert_eq!(expr.len(), 3);
        
        let matches = expr.matches(&[
            ("age".to_string(), PredicateValue::Int64(35)),
            ("score".to_string(), PredicateValue::Int64(85)),
            ("active".to_string(), PredicateValue::Boolean(true)),
        ]);
        assert!(matches);
    }

    #[test]
    fn test_predicate_expression_referenced_columns() {
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
        
        let cols = expr.referenced_columns();
        assert_eq!(cols.len(), 2);
        assert!(cols.contains("age"));
        assert!(cols.contains("score"));
    }

    #[test]
    fn test_predicate_expression_missing_column() {
        let mut expr = PredicateExpression::new();
        expr.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));
        
        // Value set doesn't include "age"
        let no_match = expr.matches(&[
            ("score".to_string(), PredicateValue::Int64(90))
        ]);
        assert!(!no_match);
    }

    #[test]
    fn test_predicate_expression_comparison_operators() {
        let test_ops = vec![
            (ComparisonOp::LessThan, 5, 10, true),
            (ComparisonOp::LessThan, 10, 5, false),
            (ComparisonOp::LessThanOrEqual, 5, 5, true),
            (ComparisonOp::LessThanOrEqual, 5, 4, false),
            (ComparisonOp::GreaterThan, 10, 5, true),
            (ComparisonOp::GreaterThan, 5, 10, false),
            (ComparisonOp::GreaterThanOrEqual, 5, 5, true),
            (ComparisonOp::GreaterThanOrEqual, 6, 5, true),
            (ComparisonOp::NotEquals, 5, 10, true),
            (ComparisonOp::NotEquals, 5, 5, false),
        ];
        
        for (op, actual, compare_to, expected) in test_ops {
            let pred = ColumnPredicate::new(
                "test".to_string(),
                op.clone(),
                PredicateValue::Int64(compare_to),
            );
            let result = pred.matches(&PredicateValue::Int64(actual));
            assert_eq!(result, expected, "Failed for {:?} with {} vs {}", op, actual, compare_to);
        }
    }

    // ============================================================================
    // SECTION 3: Column Selection Tests (7 tests)
    // ============================================================================

    #[test]
    fn test_column_selection_all() {
        let selection = ColumnSelection::all();
        assert!(selection.is_selected("col1"));
        assert!(selection.is_selected("col2"));
        assert!(selection.is_selected("col_x"));
        assert!(selection.is_all());
    }

    #[test]
    fn test_column_selection_specific() {
        let selection = ColumnSelection::new(vec![
            "col1".to_string(),
            "col2".to_string(),
        ]);
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
    fn test_column_selection_empty_list() {
        let selection = ColumnSelection::new(vec![]);
        // Empty list means "select all"
        assert!(selection.is_all());
    }

    #[test]
    fn test_column_selection_single_column() {
        let selection = ColumnSelection::new(vec!["name".to_string()]);
        assert!(selection.is_selected("name"));
        assert!(!selection.is_selected("age"));
        assert!(!selection.is_selected("email"));
    }

    #[test]
    fn test_column_selection_many_columns() {
        let cols = vec![
            "id".to_string(),
            "name".to_string(),
            "age".to_string(),
            "email".to_string(),
            "score".to_string(),
        ];
        let selection = ColumnSelection::new(cols);
        
        assert!(selection.is_selected("id"));
        assert!(selection.is_selected("name"));
        assert!(selection.is_selected("age"));
        assert!(selection.is_selected("email"));
        assert!(selection.is_selected("score"));
        assert!(!selection.is_selected("phone"));
    }

    #[test]
    fn test_column_selection_columns_list() {
        let cols_vec = vec!["col1".to_string(), "col2".to_string(), "col3".to_string()];
        let selection = ColumnSelection::new(cols_vec.clone());
        
        let cols = selection.columns();
        assert_eq!(cols.len(), 3);
        assert!(cols.contains(&"col1"));
        assert!(cols.contains(&"col2"));
        assert!(cols.contains(&"col3"));
    }

    // ============================================================================
    // SECTION 4: Range Predicate Tests (5 tests)
    // ============================================================================

    #[test]
    fn test_range_predicate_contains_in_range() {
        let range = RangePredicate::new(
            "age".to_string(),
            PredicateValue::Int64(20),
            PredicateValue::Int64(40),
        );
        
        assert!(range.contains(&PredicateValue::Int64(20)));
        assert!(range.contains(&PredicateValue::Int64(30)));
        assert!(range.contains(&PredicateValue::Int64(40)));
    }

    #[test]
    fn test_range_predicate_contains_out_of_range() {
        let range = RangePredicate::new(
            "age".to_string(),
            PredicateValue::Int64(20),
            PredicateValue::Int64(40),
        );
        
        assert!(!range.contains(&PredicateValue::Int64(10)));
        assert!(!range.contains(&PredicateValue::Int64(50)));
    }

    #[test]
    fn test_range_predicate_single_value() {
        let range = RangePredicate::new(
            "id".to_string(),
            PredicateValue::Int64(42),
            PredicateValue::Int64(42),
        );
        
        assert!(range.contains(&PredicateValue::Int64(42)));
        assert!(!range.contains(&PredicateValue::Int64(41)));
        assert!(!range.contains(&PredicateValue::Int64(43)));
    }

    #[test]
    fn test_range_predicate_float_range() {
        let range = RangePredicate::new(
            "temperature".to_string(),
            PredicateValue::Float64(20.0),
            PredicateValue::Float64(30.0),
        );
        
        assert!(range.contains(&PredicateValue::Float64(25.0)));
        assert!(range.contains(&PredicateValue::Float64(20.0)));
        assert!(range.contains(&PredicateValue::Float64(30.0)));
        assert!(!range.contains(&PredicateValue::Float64(15.0)));
    }

    #[test]
    fn test_range_predicate_string_range() {
        let range = RangePredicate::new(
            "name".to_string(),
            PredicateValue::String("alice".to_string()),
            PredicateValue::String("bob".to_string()),
        );
        
        assert!(range.contains(&PredicateValue::String("alice".to_string())));
        assert!(range.contains(&PredicateValue::String("bob".to_string())));
        assert!(range.contains(&PredicateValue::String("alice_2".to_string())));
        assert!(!range.contains(&PredicateValue::String("charlie".to_string())));
        assert!(!range.contains(&PredicateValue::String("_alice".to_string())));
    }

    // ============================================================================
    // SECTION 5: Query Filter Tests (8 tests)
    // ============================================================================

    #[test]
    fn test_query_filter_empty() {
        let filter = QueryFilter::empty();
        assert!(filter.is_empty());
        assert!(filter.predicates.is_empty());
        assert!(filter.column_selection.is_all());
    }

    #[test]
    fn test_query_filter_with_predicates_only() {
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));
        
        let filter = QueryFilter::new(predicates, ColumnSelection::all());
        assert!(!filter.is_empty());
        assert!(!filter.predicates.is_empty());
        assert!(filter.column_selection.is_all());
    }

    #[test]
    fn test_query_filter_with_column_selection_only() {
        let filter = QueryFilter::new(
            PredicateExpression::new(),
            ColumnSelection::new(vec!["col1".to_string(), "col2".to_string()]),
        );
        assert!(!filter.is_empty());
        assert!(filter.predicates.is_empty());
        assert!(!filter.column_selection.is_all());
    }

    #[test]
    fn test_query_filter_with_both() {
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));
        
        let filter = QueryFilter::new(
            predicates,
            ColumnSelection::new(vec!["name".to_string(), "age".to_string()]),
        );
        assert!(!filter.is_empty());
        assert!(!filter.predicates.is_empty());
        assert!(!filter.column_selection.is_all());
    }

    #[test]
    fn test_query_filter_default() {
        let filter = QueryFilter::default();
        assert!(filter.is_empty());
    }

    #[test]
    fn test_query_filter_complex_scenario() {
        // Scenario: SELECT name, age, score WHERE age > 30 AND score >= 80
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));
        predicates.add(ColumnPredicate::new(
            "score".to_string(),
            ComparisonOp::GreaterThanOrEqual,
            PredicateValue::Int64(80),
        ));
        
        let selection = ColumnSelection::new(vec![
            "name".to_string(),
            "age".to_string(),
            "score".to_string(),
        ]);
        
        let filter = QueryFilter::new(predicates, selection);
        
        assert!(!filter.is_empty());
        assert_eq!(filter.predicates.len(), 2);
        assert!(filter.column_selection.is_selected("name"));
        assert!(filter.column_selection.is_selected("age"));
        assert!(filter.column_selection.is_selected("score"));
        assert!(!filter.column_selection.is_selected("email"));
    }

    #[test]
    fn test_query_filter_union_scenario() {
        // Complex query with OR-like patterns (multiple predicates per column)
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "status".to_string(),
            ComparisonOp::Equals,
            PredicateValue::String("active".to_string()),
        ));
        predicates.add(ColumnPredicate::new(
            "verified".to_string(),
            ComparisonOp::Equals,
            PredicateValue::Boolean(true),
        ));
        
        let selection = ColumnSelection::new(vec![
            "id".to_string(),
            "status".to_string(),
            "verified".to_string(),
        ]);
        
        let filter = QueryFilter::new(predicates, selection);
        assert_eq!(filter.predicates.len(), 2);
        assert_eq!(filter.column_selection.columns().len(), 3);
    }
}
