//! Phase 3.1 Integration Tests - Predicate Pushdown & Column Pruning
//!
//! Tests the integration of predicate pushdown and column pruning
//! with the KoreReader and DuckDB connector.

#[cfg(test)]
mod phase_3_1_integration_tests {
    use kore_fileformat::predicates::*;
    use kore_fileformat::arrow_converter::*;
    use kore_fileformat::duckdb_connector::*;
    use std::fs;

    // ============================================================================
    // SECTION 1: Filter Creation Tests (5 tests)
    // ============================================================================

    #[test]
    fn test_create_simple_filter() {
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(30),
        ));

        let filter = QueryFilter::new(
            predicates,
            ColumnSelection::all(),
        );

        assert!(!filter.is_empty());
        assert_eq!(filter.predicates.len(), 1);
    }

    #[test]
    fn test_create_column_selection_filter() {
        let selection = ColumnSelection::new(vec![
            "id".to_string(),
            "name".to_string(),
            "age".to_string(),
        ]);

        let filter = QueryFilter::new(
            PredicateExpression::new(),
            selection,
        );

        assert!(!filter.is_empty());
        assert!(filter.column_selection.is_selected("id"));
        assert!(filter.column_selection.is_selected("name"));
        assert!(filter.column_selection.is_selected("age"));
        assert!(!filter.column_selection.is_selected("email"));
    }

    #[test]
    fn test_create_combined_filter() {
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
        assert_eq!(filter.column_selection.columns().len(), 3);
    }

    #[test]
    fn test_filter_matches_predicate_values() {
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "status".to_string(),
            ComparisonOp::Equals,
            PredicateValue::String("active".to_string()),
        ));

        let values = vec![
            ("status".to_string(), PredicateValue::String("active".to_string())),
        ];

        assert!(predicates.matches(&values));
    }

    #[test]
    fn test_filter_rejects_non_matching_values() {
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "status".to_string(),
            ComparisonOp::Equals,
            PredicateValue::String("active".to_string()),
        ));

        let values = vec![
            ("status".to_string(), PredicateValue::String("inactive".to_string())),
        ];

        assert!(!predicates.matches(&values));
    }

    // ============================================================================
    // SECTION 2: Multi-Column Filter Tests (6 tests)
    // ============================================================================

    #[test]
    fn test_multi_column_and_logic() {
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
        predicates.add(ColumnPredicate::new(
            "active".to_string(),
            ComparisonOp::Equals,
            PredicateValue::Boolean(true),
        ));

        let all_match = vec![
            ("age".to_string(), PredicateValue::Int64(35)),
            ("score".to_string(), PredicateValue::Int64(85)),
            ("active".to_string(), PredicateValue::Boolean(true)),
        ];
        assert!(predicates.matches(&all_match));

        let one_fails = vec![
            ("age".to_string(), PredicateValue::Int64(25)),
            ("score".to_string(), PredicateValue::Int64(85)),
            ("active".to_string(), PredicateValue::Boolean(true)),
        ];
        assert!(!predicates.matches(&one_fails));
    }

    #[test]
    fn test_filter_column_subset_selection() {
        let selection = ColumnSelection::new(vec![
            "id".to_string(),
            "name".to_string(),
        ]);

        let all_cols = vec!["id", "name", "age", "email", "score"];
        let selected: Vec<&str> = all_cols
            .iter()
            .filter(|col| selection.is_selected(col))
            .copied()
            .collect();

        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&"id"));
        assert!(selected.contains(&"name"));
        assert!(!selected.contains(&"age"));
    }

    #[test]
    fn test_filter_large_column_subset() {
        let mut cols = Vec::new();
        for i in 0..100 {
            cols.push(format!("col_{}", i));
        }

        let selection = ColumnSelection::new(cols[0..10].to_vec());

        for i in 0..100 {
            let col_name = format!("col_{}", i);
            if i < 10 {
                assert!(selection.is_selected(&col_name));
            } else {
                assert!(!selection.is_selected(&col_name));
            }
        }
    }

    #[test]
    fn test_filter_multiple_predicates_same_column() {
        // This simulates range predicates: age >= 30 AND age <= 50
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThanOrEqual,
            PredicateValue::Int64(30),
        ));
        predicates.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::LessThanOrEqual,
            PredicateValue::Int64(50),
        ));

        let in_range = vec![
            ("age".to_string(), PredicateValue::Int64(40)),
        ];
        assert!(predicates.matches(&in_range));

        let too_low = vec![
            ("age".to_string(), PredicateValue::Int64(25)),
        ];
        assert!(!predicates.matches(&too_low));

        let too_high = vec![
            ("age".to_string(), PredicateValue::Int64(55)),
        ];
        assert!(!predicates.matches(&too_high));
    }

    #[test]
    fn test_filter_string_predicates() {
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "country".to_string(),
            ComparisonOp::Equals,
            PredicateValue::String("USA".to_string()),
        ));

        let matches = vec![
            ("country".to_string(), PredicateValue::String("USA".to_string())),
        ];
        assert!(predicates.matches(&matches));

        let no_match = vec![
            ("country".to_string(), PredicateValue::String("Canada".to_string())),
        ];
        assert!(!predicates.matches(&no_match));
    }

    #[test]
    fn test_filter_float_predicates() {
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "rating".to_string(),
            ComparisonOp::GreaterThanOrEqual,
            PredicateValue::Float64(4.5),
        ));

        let high_rating = vec![
            ("rating".to_string(), PredicateValue::Float64(4.7)),
        ];
        assert!(predicates.matches(&high_rating));

        let low_rating = vec![
            ("rating".to_string(), PredicateValue::Float64(3.2)),
        ];
        assert!(!predicates.matches(&low_rating));
    }

    // ============================================================================
    // SECTION 3: Range Predicate Tests (5 tests)
    // ============================================================================

    #[test]
    fn test_range_predicate_integer_range() {
        let range = RangePredicate::new(
            "age".to_string(),
            PredicateValue::Int64(20),
            PredicateValue::Int64(65),
        );

        assert!(range.contains(&PredicateValue::Int64(30)));
        assert!(range.contains(&PredicateValue::Int64(20)));
        assert!(range.contains(&PredicateValue::Int64(65)));
        assert!(!range.contains(&PredicateValue::Int64(10)));
        assert!(!range.contains(&PredicateValue::Int64(70)));
    }

    #[test]
    fn test_range_predicate_float_range() {
        let range = RangePredicate::new(
            "temperature".to_string(),
            PredicateValue::Float64(0.0),
            PredicateValue::Float64(100.0),
        );

        assert!(range.contains(&PredicateValue::Float64(50.0)));
        assert!(range.contains(&PredicateValue::Float64(0.0)));
        assert!(range.contains(&PredicateValue::Float64(100.0)));
        assert!(!range.contains(&PredicateValue::Float64(-10.0)));
        assert!(!range.contains(&PredicateValue::Float64(150.0)));
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
        assert!(range.contains(&PredicateValue::String("alice_x".to_string())));
        assert!(!range.contains(&PredicateValue::String("charlie".to_string())));
        assert!(!range.contains(&PredicateValue::String("_alice".to_string())));
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
    fn test_range_predicate_negative_range() {
        let range = RangePredicate::new(
            "balance".to_string(),
            PredicateValue::Int64(-1000),
            PredicateValue::Int64(1000),
        );

        assert!(range.contains(&PredicateValue::Int64(-500)));
        assert!(range.contains(&PredicateValue::Int64(0)));
        assert!(range.contains(&PredicateValue::Int64(500)));
        assert!(!range.contains(&PredicateValue::Int64(-2000)));
        assert!(!range.contains(&PredicateValue::Int64(2000)));
    }

    // ============================================================================
    // SECTION 4: Filter Combination Tests (6 tests)
    // ============================================================================

    #[test]
    fn test_filter_combination_predicates_and_columns() {
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
            "id".to_string(),
            "name".to_string(),
            "age".to_string(),
            "score".to_string(),
        ]);

        let filter = QueryFilter::new(predicates, selection);

        assert!(!filter.is_empty());
        assert_eq!(filter.predicates.len(), 2);

        // Verify column selection works
        assert!(filter.column_selection.is_selected("id"));
        assert!(filter.column_selection.is_selected("name"));
        assert!(filter.column_selection.is_selected("age"));
        assert!(filter.column_selection.is_selected("score"));
        assert!(!filter.column_selection.is_selected("email"));
    }

    #[test]
    fn test_filter_complexity_scaling() {
        let mut predicates = PredicateExpression::new();
        for i in 0..10 {
            predicates.add(ColumnPredicate::new(
                format!("col_{}", i),
                ComparisonOp::GreaterThan,
                PredicateValue::Int64(i as i64 * 10),
            ));
        }

        assert_eq!(predicates.len(), 10);

        let mut cols = Vec::new();
        for i in 0..20 {
            cols.push(format!("col_{}", i));
        }
        let selection = ColumnSelection::new(cols);

        let filter = QueryFilter::new(predicates, selection);

        assert!(!filter.is_empty());
        assert_eq!(filter.predicates.len(), 10);
    }

    #[test]
    fn test_empty_filter_passthrough() {
        let filter = QueryFilter::empty();
        assert!(filter.is_empty());
        assert!(filter.predicates.is_empty());
        assert!(filter.column_selection.is_all());
    }

    #[test]
    fn test_filter_referenced_columns() {
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

        let cols = predicates.referenced_columns();
        assert_eq!(cols.len(), 2);
        assert!(cols.contains("age"));
        assert!(cols.contains("score"));
    }

    #[test]
    fn test_filter_all_comparison_operators() {
        let ops = vec![
            ComparisonOp::Equals,
            ComparisonOp::NotEquals,
            ComparisonOp::LessThan,
            ComparisonOp::LessThanOrEqual,
            ComparisonOp::GreaterThan,
            ComparisonOp::GreaterThanOrEqual,
        ];

        for op in ops {
            let pred = ColumnPredicate::new(
                "test".to_string(),
                op,
                PredicateValue::Int64(50),
            );

            let mut expr = PredicateExpression::new();
            expr.add(pred);
            assert_eq!(expr.len(), 1);
        }
    }

    #[test]
    fn test_filter_complex_real_world_scenario() {
        // Scenario: Customer query
        // SELECT customer_id, name, email FROM customers
        // WHERE age > 18 AND status = 'active' AND total_spent >= 100
        
        let mut predicates = PredicateExpression::new();
        predicates.add(ColumnPredicate::new(
            "age".to_string(),
            ComparisonOp::GreaterThan,
            PredicateValue::Int64(18),
        ));
        predicates.add(ColumnPredicate::new(
            "status".to_string(),
            ComparisonOp::Equals,
            PredicateValue::String("active".to_string()),
        ));
        predicates.add(ColumnPredicate::new(
            "total_spent".to_string(),
            ComparisonOp::GreaterThanOrEqual,
            PredicateValue::Float64(100.0),
        ));

        let selection = ColumnSelection::new(vec![
            "customer_id".to_string(),
            "name".to_string(),
            "email".to_string(),
        ]);

        let filter = QueryFilter::new(predicates, selection);

        assert!(!filter.is_empty());
        assert_eq!(filter.predicates.len(), 3);
        assert!(filter.column_selection.is_selected("customer_id"));
        assert!(filter.column_selection.is_selected("name"));
        assert!(filter.column_selection.is_selected("email"));
        assert!(!filter.column_selection.is_selected("age"));
        assert!(!filter.column_selection.is_selected("status"));
        assert!(!filter.column_selection.is_selected("total_spent"));
    }
}
