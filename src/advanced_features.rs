/// Advanced query features: subqueries and window functions
///
/// Provides:
/// - Subquery parsing and execution
/// - Window function support
/// - Frame specification
/// - Aggregate window functions

use std::collections::HashMap;

/// Window function type
#[derive(Clone, Debug, PartialEq)]
pub enum WindowFunction {
    RowNumber,
    Rank,
    DenseRank,
    Lag,
    Lead,
    FirstValue,
    LastValue,
    Sum,
    Avg,
    Count,
    Min,
    Max,
}

impl WindowFunction {
    pub fn as_str(&self) -> &str {
        match self {
            WindowFunction::RowNumber => "ROW_NUMBER",
            WindowFunction::Rank => "RANK",
            WindowFunction::DenseRank => "DENSE_RANK",
            WindowFunction::Lag => "LAG",
            WindowFunction::Lead => "LEAD",
            WindowFunction::FirstValue => "FIRST_VALUE",
            WindowFunction::LastValue => "LAST_VALUE",
            WindowFunction::Sum => "SUM",
            WindowFunction::Avg => "AVG",
            WindowFunction::Count => "COUNT",
            WindowFunction::Min => "MIN",
            WindowFunction::Max => "MAX",
        }
    }

    pub fn is_ranking(&self) -> bool {
        matches!(
            self,
            WindowFunction::RowNumber
                | WindowFunction::Rank
                | WindowFunction::DenseRank
        )
    }

    pub fn is_aggregate(&self) -> bool {
        matches!(
            self,
            WindowFunction::Sum
                | WindowFunction::Avg
                | WindowFunction::Count
                | WindowFunction::Min
                | WindowFunction::Max
        )
    }
}

/// Frame bound type
#[derive(Clone, Debug, PartialEq)]
pub enum FrameBound {
    UnboundedPreceding,
    Preceding(u64),
    CurrentRow,
    Following(u64),
    UnboundedFollowing,
}

impl FrameBound {
    pub fn as_str(&self) -> &str {
        match self {
            FrameBound::UnboundedPreceding => "UNBOUNDED PRECEDING",
            FrameBound::Preceding(_) => "PRECEDING",
            FrameBound::CurrentRow => "CURRENT ROW",
            FrameBound::Following(_) => "FOLLOWING",
            FrameBound::UnboundedFollowing => "UNBOUNDED FOLLOWING",
        }
    }
}

/// Frame specification
#[derive(Clone, Debug)]
pub struct FrameSpec {
    pub frame_type: FrameType,
    pub start_bound: FrameBound,
    pub end_bound: FrameBound,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FrameType {
    Rows,
    Range,
}

impl FrameSpec {
    pub fn rows_unbounded() -> Self {
        Self {
            frame_type: FrameType::Rows,
            start_bound: FrameBound::UnboundedPreceding,
            end_bound: FrameBound::UnboundedFollowing,
        }
    }

    pub fn rows_between(start: u64, end: u64) -> Self {
        Self {
            frame_type: FrameType::Rows,
            start_bound: FrameBound::Preceding(start),
            end_bound: FrameBound::Following(end),
        }
    }

    pub fn range_unbounded() -> Self {
        Self {
            frame_type: FrameType::Range,
            start_bound: FrameBound::UnboundedPreceding,
            end_bound: FrameBound::CurrentRow,
        }
    }
}

/// Window specification
#[derive(Clone, Debug)]
pub struct WindowSpec {
    pub partition_columns: Vec<String>,
    pub order_columns: Vec<(String, bool)>, // (column, is_asc)
    pub frame: FrameSpec,
}

impl WindowSpec {
    pub fn new(partition: Vec<String>, order: Vec<(String, bool)>) -> Self {
        Self {
            partition_columns: partition,
            order_columns: order,
            frame: FrameSpec::rows_unbounded(),
        }
    }

    pub fn with_frame(mut self, frame: FrameSpec) -> Self {
        self.frame = frame;
        self
    }
}

/// Window function clause
#[derive(Clone, Debug)]
pub struct WindowFunctionClause {
    pub function: WindowFunction,
    pub column: Option<String>,
    pub alias: String,
    pub window: WindowSpec,
}

impl WindowFunctionClause {
    pub fn new(
        function: WindowFunction,
        column: Option<&str>,
        alias: &str,
        window: WindowSpec,
    ) -> Self {
        Self {
            function,
            column: column.map(|s| s.to_string()),
            alias: alias.to_string(),
            window,
        }
    }
}

/// Subquery
#[derive(Clone, Debug)]
pub struct Subquery {
    pub alias: String,
    pub select_columns: Vec<String>,
    pub from_table: String,
    pub where_condition: Option<String>,
    pub order_by: Option<String>,
    pub limit: Option<usize>,
}

impl Subquery {
    pub fn new(alias: &str, table: &str) -> Self {
        Self {
            alias: alias.to_string(),
            select_columns: vec!["*".to_string()],
            from_table: table.to_string(),
            where_condition: None,
            order_by: None,
            limit: None,
        }
    }

    pub fn with_columns(mut self, columns: Vec<&str>) -> Self {
        self.select_columns = columns.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_where(mut self, condition: &str) -> Self {
        self.where_condition = Some(condition.to_string());
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn sql_string(&self) -> String {
        let mut sql = format!(
            "SELECT {} FROM {}",
            self.select_columns.join(", "),
            self.from_table
        );

        if let Some(where_clause) = &self.where_condition {
            sql.push_str(&format!(" WHERE {}", where_clause));
        }

        if let Some(order_clause) = &self.order_by {
            sql.push_str(&format!(" ORDER BY {}", order_clause));
        }

        if let Some(limit) = self.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        sql
    }
}

/// Advanced query builder
pub struct AdvancedQueryBuilder {
    base_query: String,
    subqueries: HashMap<String, Subquery>,
    window_functions: Vec<WindowFunctionClause>,
}

impl AdvancedQueryBuilder {
    pub fn new(base_query: &str) -> Self {
        Self {
            base_query: base_query.to_string(),
            subqueries: HashMap::new(),
            window_functions: Vec::new(),
        }
    }

    pub fn add_subquery(mut self, subquery: Subquery) -> Self {
        self.subqueries.insert(subquery.alias.clone(), subquery);
        self
    }

    pub fn add_window_function(
        mut self,
        window_func: WindowFunctionClause,
    ) -> Self {
        self.window_functions.push(window_func);
        self
    }

    pub fn build(&self) -> String {
        let mut result = self.base_query.clone();

        // Add subqueries as CTEs if present
        if !self.subqueries.is_empty() {
            let mut ctes = Vec::new();
            for (_, subquery) in &self.subqueries {
                ctes.push(format!(
                    "{} AS ({})",
                    subquery.alias,
                    subquery.sql_string()
                ));
            }
            result = format!("WITH {} {}", ctes.join(", "), result);
        }

        result
    }

    pub fn build_with_window_functions(&self) -> String {
        let mut result = self.build();

        for window_func in &self.window_functions {
            let func_str = match &window_func.column {
                Some(col) => format!(
                    "{}({}) OVER (PARTITION BY {} ORDER BY {}) AS {}",
                    window_func.function.as_str(),
                    col,
                    window_func.window.partition_columns.join(", "),
                    window_func
                        .window
                        .order_columns
                        .iter()
                        .map(|(c, asc)| {
                            format!(
                                "{} {}",
                                c,
                                if *asc { "ASC" } else { "DESC" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    window_func.alias
                ),
                None => format!(
                    "{}() OVER (PARTITION BY {} ORDER BY {}) AS {}",
                    window_func.function.as_str(),
                    window_func.window.partition_columns.join(", "),
                    window_func
                        .window
                        .order_columns
                        .iter()
                        .map(|(c, asc)| {
                            format!(
                                "{} {}",
                                c,
                                if *asc { "ASC" } else { "DESC" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    window_func.alias
                ),
            };

            // Insert window function into SELECT clause
            result = result.replace("SELECT", &format!("SELECT {}, ", func_str));
        }

        result
    }
}

/// Window function result row
#[derive(Clone, Debug)]
pub struct WindowFunctionResult {
    pub row_values: HashMap<String, String>,
    pub window_values: HashMap<String, String>,
}

impl WindowFunctionResult {
    pub fn new() -> Self {
        Self {
            row_values: HashMap::new(),
            window_values: HashMap::new(),
        }
    }

    pub fn set_value(&mut self, column: &str, value: &str) {
        self.row_values.insert(column.to_string(), value.to_string());
    }

    pub fn set_window_value(&mut self, function: &str, value: &str) {
        self.window_values
            .insert(function.to_string(), value.to_string());
    }

    pub fn get_value(&self, column: &str) -> Option<&String> {
        self.row_values.get(column)
    }

    pub fn get_window_value(&self, function: &str) -> Option<&String> {
        self.window_values.get(function)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_function_str() {
        assert_eq!(WindowFunction::RowNumber.as_str(), "ROW_NUMBER");
        assert_eq!(WindowFunction::Rank.as_str(), "RANK");
    }

    #[test]
    fn test_window_function_is_ranking() {
        assert!(WindowFunction::RowNumber.is_ranking());
        assert!(WindowFunction::Rank.is_ranking());
        assert!(!WindowFunction::Sum.is_ranking());
    }

    #[test]
    fn test_window_function_is_aggregate() {
        assert!(WindowFunction::Sum.is_aggregate());
        assert!(WindowFunction::Avg.is_aggregate());
        assert!(!WindowFunction::RowNumber.is_aggregate());
    }

    #[test]
    fn test_frame_spec_rows() {
        let frame = FrameSpec::rows_between(1, 1);
        assert_eq!(frame.frame_type, FrameType::Rows);
    }

    #[test]
    fn test_frame_spec_range() {
        let frame = FrameSpec::range_unbounded();
        assert_eq!(frame.frame_type, FrameType::Range);
    }

    #[test]
    fn test_window_spec() {
        let spec = WindowSpec::new(
            vec!["department".to_string()],
            vec![("salary".to_string(), false)],
        );
        assert_eq!(spec.partition_columns.len(), 1);
    }

    #[test]
    fn test_subquery_sql() {
        let subquery = Subquery::new("sales_sub", "sales")
            .with_columns(vec!["id", "amount"])
            .with_where("amount > 100")
            .with_limit(1000);

        let sql = subquery.sql_string();
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("LIMIT"));
    }

    #[test]
    fn test_advanced_query_builder() {
        let builder = AdvancedQueryBuilder::new(
            "SELECT * FROM employees",
        )
        .add_subquery(Subquery::new("high_salary", "employees"));

        let query = builder.build();
        assert!(query.contains("WITH"));
    }

    #[test]
    fn test_window_function_clause() {
        let window = WindowSpec::new(
            vec!["dept".to_string()],
            vec![("salary".to_string(), false)],
        );
        let clause = WindowFunctionClause::new(
            WindowFunction::RowNumber,
            None,
            "row_num",
            window,
        );

        assert_eq!(clause.alias, "row_num");
    }

    #[test]
    fn test_window_function_result() {
        let mut result = WindowFunctionResult::new();
        result.set_value("name", "John");
        result.set_window_value("row_num", "1");

        assert_eq!(result.get_value("name"), Some(&"John".to_string()));
        assert_eq!(
            result.get_window_value("row_num"),
            Some(&"1".to_string())
        );
    }

    #[test]
    fn test_frame_bound_str() {
        assert_eq!(
            FrameBound::UnboundedPreceding.as_str(),
            "UNBOUNDED PRECEDING"
        );
        assert_eq!(FrameBound::CurrentRow.as_str(), "CURRENT ROW");
    }
}
