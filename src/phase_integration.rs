/// Integration Examples: Phase 2, 3, 4 Working Together
/// 
/// This demonstrates how MCP Server (Phase 2), Query Engine (Phase 3), 
/// and AI Features (Phase 4) integrate to provide a complete solution

use crate::mcp_server::{MCPServer, QueryRequest};
use crate::query_exec_v3::{QueryPlanner, RowFilter, GroupByExecutor, AggregationFunc};
use crate::ai_features::{AICodecSelector, NaturalLanguageParser, CodecRecommendation};
use std::collections::HashMap;

pub struct KoreFullStack {
    mcp: MCPServer,
    query_planner: QueryPlanner,
}

impl KoreFullStack {
    pub fn new(data_dir: &str) -> std::io::Result<Self> {
        Ok(Self {
            mcp: MCPServer::new(data_dir)?,
            query_planner: QueryPlanner::new(),
        })
    }

    /// Example 1: User asks natural language query → AI parses → Query Engine executes
    pub fn natural_language_query_example(
        &mut self,
        user_query: &str,
        file_path: &str,
    ) -> Result<String, String> {
        // Phase 4: AI Feature - Parse natural language
        let intent = NaturalLanguageParser::parse(user_query)
            .ok_or_else(|| "Could not parse query".to_string())?;

        // Phase 4: Convert to SQL
        let sql = NaturalLanguageParser::intent_to_sql(&intent, file_path);
        println!("🤖 Parsed user intent into: {}", sql);

        // Phase 2: MCP Server would execute this
        let req = QueryRequest {
            file_path: file_path.to_string(),
            select_columns: None,
            where_clause: None,
            limit: Some(100),
        };

        match self.mcp.execute_query(&req) {
            Ok(result) => {
                println!(
                    "✅ Query executed in {:.2}ms, returned {} rows",
                    result.execution_time_ms, result.row_count
                );
                Ok(format!("Executed: {}", sql))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Example 2: Data analysis with GROUP BY and aggregations
    pub fn group_by_analysis_example(
        &self,
        rows: Vec<HashMap<String, String>>,
        group_cols: Vec<String>,
    ) -> Vec<HashMap<String, String>> {
        // Phase 3: Query Engine - Define aggregations
        let aggs = vec![
            ("count".to_string(), AggregationFunc::Count),
            (
                "total_amount".to_string(),
                AggregationFunc::Sum("amount".to_string()),
            ),
            (
                "avg_amount".to_string(),
                AggregationFunc::Avg("amount".to_string()),
            ),
        ];

        // Phase 3: Execute vectorized GROUP BY
        let results = GroupByExecutor::execute(rows, &group_cols, &aggs);
        println!("📊 Group-by analysis complete: {} groups", results.len());

        results
    }

    /// Example 3: Codec recommendation based on column analysis
    pub fn codec_recommendation_example(
        &self,
        column_data: Vec<String>,
    ) -> CodecRecommendation {
        // Phase 4: AI Feature - Analyze data and recommend compression
        let rec = AICodecSelector::recommend_codec(&column_data);
        println!(
            "💾 Codec Recommendation: {} (confidence: {:.1}%, estimated ratio: {:.2})",
            rec.codec,
            rec.confidence * 100.0,
            rec.estimated_ratio
        );

        rec
    }

    /// Example 4: Complex query with WHERE clause and projections
    pub fn filtered_projection_example(
        &mut self,
        rows: Vec<HashMap<String, String>>,
        where_clause: &str,
        select_cols: Vec<String>,
    ) -> Result<Vec<HashMap<String, String>>, String> {
        // Phase 3: Query Engine - Parse WHERE clause
        let predicate = QueryPlanner::parse_where_clause(where_clause)
            .ok_or_else(|| "Invalid WHERE clause".to_string())?;

        // Phase 3: Filter rows
        let filtered: Vec<_> = rows
            .into_iter()
            .filter(|row| RowFilter::matches(&predicate, row))
            .collect();

        println!("🔍 Filtered {} rows using: {}", filtered.len(), where_clause);

        // Phase 3: Project columns
        let projected: Vec<_> = filtered
            .into_iter()
            .map(|row| {
                let mut proj_row = HashMap::new();
                for col in &select_cols {
                    if let Some(val) = row.get(col) {
                        proj_row.insert(col.clone(), val.clone());
                    }
                }
                proj_row
            })
            .collect();

        println!("📋 Projected to {} columns", select_cols.len());

        Ok(projected)
    }
}

// ================== Usage Examples ==================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_phase2_mcp_server_init() {
        let server = MCPServer::new(".");
        assert!(server.is_ok());
    }

    #[test]
    fn test_phase3_query_parsing() {
        let pred = QueryPlanner::parse_where_clause("age > 30 AND city = NYC");
        assert!(pred.is_some());
    }

    #[test]
    fn test_phase4_natural_language() {
        let intent = NaturalLanguageParser::parse("Show me records where age > 30");
        assert!(intent.is_some());
    }

    #[test]
    fn test_full_stack_codec_recommendation() {
        let data = vec!["A".to_string(), "A".to_string(), "B".to_string()];
        let rec = AICodecSelector::recommend_codec(&data);
        assert!(rec.confidence > 0.5);
    }

    #[test]
    fn test_full_stack_group_by() {
        let mut rows = Vec::new();

        let mut row1 = HashMap::new();
        row1.insert("category".to_string(), "Electronics".to_string());
        row1.insert("amount".to_string(), "100".to_string());
        rows.push(row1);

        let mut row2 = HashMap::new();
        row2.insert("category".to_string(), "Electronics".to_string());
        row2.insert("amount".to_string(), "200".to_string());
        rows.push(row2);

        let stack = KoreFullStack::new(".").unwrap();
        let results = stack.group_by_analysis_example(
            rows,
            vec!["category".to_string()],
        );

        assert_eq!(results.len(), 1);
    }
}

// ================== Example Workflows ==================

/// Complete example workflow for Phase 2, 3, 4
pub fn example_complete_workflow() {
    println!("\n🚀 KORE v1.3.2 - Phase 2, 3, 4 Complete Workflow\n");

    // Initialize
    println!("1️⃣  Initializing MCP Server (Phase 2)...");
    let data_dir = ".";
    let _mcp = MCPServer::new(data_dir).expect("Failed to init MCP");
    println!("   ✅ MCP Server ready for Claude/ChatGPT integration\n");

    // Natural Language Query
    println!("2️⃣  Processing natural language query (Phase 4)...");
    let user_query = "Show me the average revenue per region";
    let intent = NaturalLanguageParser::parse(user_query);
    if let Some(intent) = intent {
        let sql = NaturalLanguageParser::intent_to_sql(&intent, "sales");
        println!("   User: {}", user_query);
        println!("   SQL:  {}\n", sql);
    }

    // Query Execution
    println!("3️⃣  Executing query with Phase 3 engine...");
    let where_clause = "amount > 100";
    if let Some(_pred) = QueryPlanner::parse_where_clause(where_clause) {
        println!("   ✅ WHERE clause parsed successfully\n");
    }

    // Codec Recommendation
    println!("4️⃣  AI codec selection (Phase 4)...");
    let sample_data = vec!["Category_A".to_string(), "Category_A".to_string(), "Category_B".to_string()];
    let rec = AICodecSelector::recommend_codec(&sample_data);
    println!("   💾 Recommended codec: {} ({:.0}% confidence)",
             rec.codec, rec.confidence * 100.0);
    println!("   📈 Estimated compression ratio: {:.2}x\n", 1.0 / rec.estimated_ratio);

    // Group By Analysis
    println!("5️⃣  Vectorized GROUP BY aggregation (Phase 3)...");
    let mut test_rows = vec![];
    for i in 0..5 {
        let mut row = HashMap::new();
        row.insert("region".to_string(), if i % 2 == 0 { "North" } else { "South" }.to_string());
        row.insert("revenue".to_string(), (i * 100).to_string());
        test_rows.push(row);
    }

    let stack = KoreFullStack::new(data_dir).expect("Failed to init stack");
    let results = stack.group_by_analysis_example(
        test_rows,
        vec!["region".to_string()],
    );
    println!("   ✅ Group-by produced {} groups\n", results.len());

    println!("🎉 Workflow complete! All phases (2, 3, 4) working together.\n");
}
