/// Predicate Pushdown for KORE v1.6.0
/// 
/// Push filter predicates down to chunk level for early filtering.
/// Uses statistics (min/max values) to eliminate chunks that can't match the filter.

use std::cmp::Ordering;

/// Represents a filter condition
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// col = value (equality)
    Equals { col: String, value: i64 },
    /// col > value (greater than)
    GreaterThan { col: String, value: i64 },
    /// col < value (less than)
    LessThan { col: String, value: i64 },
    /// col >= value
    GreaterEqual { col: String, value: i64 },
    /// col <= value
    LessEqual { col: String, value: i64 },
    /// col IN (values)
    In { col: String, values: Vec<i64> },
    /// col IS NOT NULL
    IsNotNull { col: String },
    /// col IS NULL
    IsNull { col: String },
    /// AND: Both conditions must be true
    And(Box<Predicate>, Box<Predicate>),
    /// OR: Either condition can be true
    Or(Box<Predicate>, Box<Predicate>),
}

/// Statistics for a chunk (min/max values, null count)
#[derive(Debug, Clone)]
pub struct ChunkStats {
    /// Column name
    pub column: String,
    /// Minimum value in chunk (if known)
    pub min_value: Option<i64>,
    /// Maximum value in chunk (if known)
    pub max_value: Option<i64>,
    /// Number of null values in chunk
    pub null_count: u64,
    /// Total rows in chunk
    pub row_count: u64,
}

impl ChunkStats {
    pub fn new(column: String, min_value: Option<i64>, max_value: Option<i64>, row_count: u64) -> Self {
        ChunkStats {
            column,
            min_value,
            max_value,
            null_count: 0,
            row_count,
        }
    }
}

/// Result of predicate evaluation on chunk
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateResult {
    /// All rows in chunk must match (can skip this chunk in NOT filter)
    MustMatch,
    /// No rows in chunk can match (skip this chunk entirely)
    NoMatch,
    /// Some rows might match (must scan chunk)
    MayMatch,
}

impl Predicate {
    /// Test if a chunk can possibly satisfy this predicate
    /// Returns:
    /// - MustMatch: All rows in chunk satisfy predicate
    /// - NoMatch: No rows in chunk can satisfy predicate
    /// - MayMatch: Some rows might satisfy predicate (need to scan)
    pub fn test_chunk(&self, chunk: &ChunkStats) -> PredicateResult {
        match self {
            Predicate::Equals { col, value } => {
                if col != &chunk.column {
                    return PredicateResult::MayMatch; // Unknown for other columns
                }

                match (chunk.min_value, chunk.max_value) {
                    (Some(min), Some(max)) if *value < min || *value > max => PredicateResult::NoMatch,
                    (Some(min), Some(max)) if *value == min && *value == max => PredicateResult::MustMatch,
                    _ => PredicateResult::MayMatch,
                }
            }

            Predicate::GreaterThan { col, value } => {
                if col != &chunk.column {
                    return PredicateResult::MayMatch;
                }

                match chunk.max_value {
                    Some(max) if max <= *value => PredicateResult::NoMatch,
                    _ => match chunk.min_value {
                        Some(min) if min > *value => PredicateResult::MustMatch,
                        _ => PredicateResult::MayMatch,
                    },
                }
            }

            Predicate::LessThan { col, value } => {
                if col != &chunk.column {
                    return PredicateResult::MayMatch;
                }

                match chunk.min_value {
                    Some(min) if min >= *value => PredicateResult::NoMatch,
                    _ => match chunk.max_value {
                        Some(max) if max < *value => PredicateResult::MustMatch,
                        _ => PredicateResult::MayMatch,
                    },
                }
            }

            Predicate::GreaterEqual { col, value } => {
                if col != &chunk.column {
                    return PredicateResult::MayMatch;
                }

                match chunk.max_value {
                    Some(max) if max < *value => PredicateResult::NoMatch,
                    _ => match chunk.min_value {
                        Some(min) if min >= *value => PredicateResult::MustMatch,
                        _ => PredicateResult::MayMatch,
                    },
                }
            }

            Predicate::LessEqual { col, value } => {
                if col != &chunk.column {
                    return PredicateResult::MayMatch;
                }

                match chunk.min_value {
                    Some(min) if min > *value => PredicateResult::NoMatch,
                    _ => match chunk.max_value {
                        Some(max) if max <= *value => PredicateResult::MustMatch,
                        _ => PredicateResult::MayMatch,
                    },
                }
            }

            Predicate::In { col, values } => {
                if col != &chunk.column {
                    return PredicateResult::MayMatch;
                }

                let min_val = *values.iter().min().unwrap_or(&i64::MIN);
                let max_val = *values.iter().max().unwrap_or(&i64::MAX);

                match (chunk.min_value, chunk.max_value) {
                    (Some(chunk_min), Some(chunk_max)) if chunk_max < min_val || chunk_min > max_val => {
                        PredicateResult::NoMatch
                    }
                    _ => PredicateResult::MayMatch,
                }
            }

            Predicate::IsNotNull { col } => {
                if col != &chunk.column {
                    return PredicateResult::MayMatch;
                }

                if chunk.null_count == chunk.row_count {
                    PredicateResult::NoMatch // All nulls
                } else if chunk.null_count == 0 {
                    PredicateResult::MustMatch // No nulls
                } else {
                    PredicateResult::MayMatch
                }
            }

            Predicate::IsNull { col } => {
                if col != &chunk.column {
                    return PredicateResult::MayMatch;
                }

                if chunk.null_count == 0 {
                    PredicateResult::NoMatch // No nulls
                } else if chunk.null_count == chunk.row_count {
                    PredicateResult::MustMatch // All nulls
                } else {
                    PredicateResult::MayMatch
                }
            }

            Predicate::And(left, right) => {
                let left_result = left.test_chunk(chunk);
                let right_result = right.test_chunk(chunk);

                match (left_result, right_result) {
                    (PredicateResult::NoMatch, _) | (_, PredicateResult::NoMatch) => PredicateResult::NoMatch,
                    (PredicateResult::MustMatch, PredicateResult::MustMatch) => PredicateResult::MustMatch,
                    _ => PredicateResult::MayMatch,
                }
            }

            Predicate::Or(left, right) => {
                let left_result = left.test_chunk(chunk);
                let right_result = right.test_chunk(chunk);

                match (left_result, right_result) {
                    (PredicateResult::MustMatch, _) | (_, PredicateResult::MustMatch) => PredicateResult::MustMatch,
                    (PredicateResult::NoMatch, PredicateResult::NoMatch) => PredicateResult::NoMatch,
                    _ => PredicateResult::MayMatch,
                }
            }
        }
    }

    /// Simplify this predicate (remove redundant conditions)
    pub fn simplify(&self) -> Predicate {
        match self {
            Predicate::And(left, right) => {
                let left_simplified = left.simplify();
                let right_simplified = right.simplify();
                Predicate::And(Box::new(left_simplified), Box::new(right_simplified))
            }
            Predicate::Or(left, right) => {
                let left_simplified = left.simplify();
                let right_simplified = right.simplify();
                Predicate::Or(Box::new(left_simplified), Box::new(right_simplified))
            }
            other => other.clone(),
        }
    }
}

/// Filter chunks based on predicate evaluation
pub fn filter_chunks(chunks: &[ChunkStats], predicate: &Predicate) -> Result<Vec<usize>, String> {
    let mut result = Vec::new();

    for (idx, chunk) in chunks.iter().enumerate() {
        match predicate.test_chunk(chunk) {
            PredicateResult::NoMatch => {
                // Skip this chunk entirely
            }
            PredicateResult::MustMatch | PredicateResult::MayMatch => {
                // Include this chunk
                result.push(idx);
            }
        }
    }

    Ok(result)
}

/// Estimate selectivity (fraction of rows that pass filter) for a chunk
pub fn estimate_selectivity(chunk: &ChunkStats, predicate: &Predicate) -> f64 {
    match predicate.test_chunk(chunk) {
        PredicateResult::NoMatch => 0.0,
        PredicateResult::MustMatch => 1.0,
        PredicateResult::MayMatch => {
            // Conservative estimate: assume 50% pass (could be improved with statistics)
            0.5
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equals_in_range() {
        let chunk = ChunkStats::new("id".to_string(), Some(1), Some(100), 100);
        let pred = Predicate::Equals {
            col: "id".to_string(),
            value: 50,
        };
        assert_eq!(pred.test_chunk(&chunk), PredicateResult::MayMatch);
    }

    #[test]
    fn test_equals_outside_range() {
        let chunk = ChunkStats::new("id".to_string(), Some(1), Some(100), 100);
        let pred = Predicate::Equals {
            col: "id".to_string(),
            value: 500,
        };
        assert_eq!(pred.test_chunk(&chunk), PredicateResult::NoMatch);
    }

    #[test]
    fn test_greater_than() {
        let chunk = ChunkStats::new("age".to_string(), Some(20), Some(80), 100);
        let pred = Predicate::GreaterThan {
            col: "age".to_string(),
            value: 50,
        };
        assert_eq!(pred.test_chunk(&chunk), PredicateResult::MayMatch);
    }

    #[test]
    fn test_greater_than_all_match() {
        let chunk = ChunkStats::new("age".to_string(), Some(60), Some(80), 100);
        let pred = Predicate::GreaterThan {
            col: "age".to_string(),
            value: 50,
        };
        assert_eq!(pred.test_chunk(&chunk), PredicateResult::MustMatch);
    }

    #[test]
    fn test_greater_than_no_match() {
        let chunk = ChunkStats::new("age".to_string(), Some(20), Some(40), 100);
        let pred = Predicate::GreaterThan {
            col: "age".to_string(),
            value: 50,
        };
        assert_eq!(pred.test_chunk(&chunk), PredicateResult::NoMatch);
    }

    #[test]
    fn test_is_not_null_all_non_null() {
        let chunk = ChunkStats::new("name".to_string(), None, None, 100);
        let mut chunk = chunk;
        chunk.null_count = 0;
        let pred = Predicate::IsNotNull {
            col: "name".to_string(),
        };
        assert_eq!(pred.test_chunk(&chunk), PredicateResult::MustMatch);
    }

    #[test]
    fn test_filter_chunks() {
        let chunks = vec![
            ChunkStats::new("id".to_string(), Some(1), Some(100), 100),
            ChunkStats::new("id".to_string(), Some(101), Some(200), 100),
            ChunkStats::new("id".to_string(), Some(201), Some(300), 100),
        ];

        let pred = Predicate::GreaterThan {
            col: "id".to_string(),
            value: 150,
        };

        let result = filter_chunks(&chunks, &pred).unwrap();
        assert_eq!(result.len(), 2); // Second and third chunks
        assert!(result.contains(&1));
        assert!(result.contains(&2));
    }

    #[test]
    fn test_and_predicate() {
        let chunk = ChunkStats::new("id".to_string(), Some(1), Some(100), 100);
        let pred = Predicate::And(
            Box::new(Predicate::GreaterThan {
                col: "id".to_string(),
                value: 0,
            }),
            Box::new(Predicate::LessThan {
                col: "id".to_string(),
                value: 150,
            }),
        );
        assert_eq!(pred.test_chunk(&chunk), PredicateResult::MustMatch);
    }

    #[test]
    fn test_selectivity_must_match() {
        let chunk = ChunkStats::new("id".to_string(), Some(1), Some(100), 100);
        let pred = Predicate::GreaterThan {
            col: "id".to_string(),
            value: 0,
        };
        let sel = estimate_selectivity(&chunk, &pred);
        assert_eq!(sel, 1.0);
    }

    #[test]
    fn test_selectivity_no_match() {
        let chunk = ChunkStats::new("id".to_string(), Some(1), Some(100), 100);
        let pred = Predicate::GreaterThan {
            col: "id".to_string(),
            value: 500,
        };
        let sel = estimate_selectivity(&chunk, &pred);
        assert_eq!(sel, 0.0);
    }
}
