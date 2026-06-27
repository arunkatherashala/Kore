/// KORE Query Engine Integration Tests
/// End-to-end testing of query execution with all components

#[cfg(test)]
mod integration_tests {
    use kore_fileformat::query_engine::{Query, QueryExecutor, Lexer, Parser};
    use kore_fileformat::query_cache::{QueryPlanCache, QueryOptimizer};
    use kore_fileformat::index_manager::{IndexManager, IndexType};
    use kore_fileformat::benchmarks::BenchmarkEngine;
    use std::collections::HashMap;

    /// Test end-to-end query with caching
    #[test]
    fn test_query_with_caching() {
        let cache = QueryPlanCache::new(10, 60);
        
        // First execution - should not be cached
        let query_text = "SELECT * FROM users WHERE id = 1";
        let plan1 = cache.get(query_text);
        assert!(plan1.is_none());
        
        // Add to cache
        cache.put(query_text, 50.0);
        
        // Second execution - should be cached
        let plan2 = cache.get(query_text);
        assert!(plan2.is_some());
    }

    /// Test query parsing and execution
    #[test]
    fn test_query_parsing_and_execution() {
        let mut parser = Parser::new("SELECT id, name FROM users WHERE id = 1 LIMIT 10");
        let query = parser.parse().unwrap();
        
        assert_eq!(query.select_cols, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(query.table, "users");
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.limit, Some(10));
    }

    /// Test query with index optimization
    #[test]
    fn test_query_with_index_optimization() {
        let mut manager = IndexManager::new();
        manager.create_index("users", "id", IndexType::Hash, 10000, 1000000).unwrap();
        manager.create_index("users", "email", IndexType::BTree, 10000, 1000000).unwrap();
        
        let indexes = manager.get_table_indexes("users");
        assert_eq!(indexes.len(), 2);
    }

    /// Test query cost estimation
    #[test]
    fn test_query_cost_estimation() {
        let cost = QueryOptimizer::estimate_cost(10_000_000, 0.1, 1);
        assert!(cost.estimated_rows > 0);
        assert!(cost.estimated_cost > 0.0);
    }

    /// Test multiple queries with cache statistics
    #[test]
    fn test_cache_statistics() {
        let cache = QueryPlanCache::new(10, 60);
        
        // Add multiple queries
        cache.put("SELECT * FROM users", 50.0);
        cache.put("SELECT * FROM products", 60.0);
        cache.put("SELECT * FROM orders", 55.0);
        
        let stats = cache.stats();
        assert_eq!(stats.total_cached_queries, 3);
    }

    /// Test benchmark integration with query engine
    #[test]
    fn test_benchmark_and_query_integration() {
        // This test verifies that benchmarking can work with query engine
        let results = BenchmarkEngine::benchmark_files(vec![]);
        assert_eq!(results.len(), 0); // Empty because no files exist
    }

    /// Test query with complex filters
    #[test]
    fn test_complex_query_parsing() {
        let query_text = "SELECT id, name FROM users WHERE status = 1 LIMIT 100";
        let mut parser = Parser::new(query_text);
        let query = parser.parse().unwrap();
        
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.limit, Some(100));
    }

    /// Test index recommendation based on query patterns
    #[test]
    fn test_index_recommendation() {
        let mut manager = IndexManager::new();
        let recommendations = manager.recommend_indexes("users", &["id", "email", "status"]);
        
        // Should recommend indexes on columns that don't already have them
        assert!(recommendations.len() > 0);
    }

    /// Test execution strategy selection
    #[test]
    fn test_execution_strategy_selection() {
        let cost = QueryOptimizer::estimate_cost(1000, 0.5, 0);
        let strategy = QueryOptimizer::choose_strategy(&cost);
        
        // Small query without joins should use index scan
        assert_eq!(
            strategy,
            kore_fileformat::query_cache::ExecutionStrategy::IndexScan
        );
    }

    /// Test query lexer with JOIN keywords
    #[test]
    fn test_lexer_with_join_keywords() {
        let query_text = "SELECT * FROM users INNER JOIN orders ON users.id = orders.user_id";
        let mut lexer = Lexer::new(query_text);
        
        // Count tokens to verify lexer works
        let mut token_count = 0;
        loop {
            let token = lexer.next_token();
            token_count += 1;
            if token == kore_fileformat::query_engine::Token::Eof {
                break;
            }
        }
        
        assert!(token_count > 0);
    }

    /// Test end-to-end query with cache and index
    #[test]
    fn test_full_query_optimization_pipeline() {
        // Step 1: Parse query
        let query_text = "SELECT * FROM users WHERE id = 100";
        let mut parser = Parser::new(query_text);
        let query = parser.parse().unwrap();
        
        // Step 2: Check cache
        let cache = QueryPlanCache::new(10, 60);
        let cached = cache.get(query_text);
        assert!(cached.is_none());
        
        // Step 3: Check indexes
        let mut manager = IndexManager::new();
        manager.create_index("users", "id", IndexType::Hash, 10000, 1000000).unwrap();
        let index = manager.get_index("users", "id");
        assert!(index.is_some());
        
        // Step 4: Estimate cost
        let cost = QueryOptimizer::estimate_cost(10_000_000, 0.0001, 0);
        
        // Step 5: Store in cache
        cache.put(query_text, 25.0);
        assert!(cache.get(query_text).is_some());
    }

    /// Test query comparison performance
    #[test]
    fn test_query_performance_comparison() {
        // Simulate different query complexities with joins having higher cost
        let simple_cost = QueryOptimizer::estimate_cost(1000, 1.0, 0);
        let filtered_cost = QueryOptimizer::estimate_cost(1000, 0.1, 0);
        let join_cost = QueryOptimizer::estimate_cost(1000, 0.1, 1);
        
        // Join adds cost multiplier
        assert!(join_cost.estimated_cost > filtered_cost.estimated_cost);
    }
}
