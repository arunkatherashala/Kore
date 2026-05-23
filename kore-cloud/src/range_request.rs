//! HTTP Range request support for efficient partial reads

use std::fmt;

/// Represents an HTTP Range request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeRequest {
    pub start: u64,
    pub end: u64,
}

impl RangeRequest {
    /// Create a new range request (inclusive on both ends)
    pub fn new(start: u64, end: u64) -> Result<Self, String> {
        if start > end {
            return Err(format!("Invalid range: start ({}) > end ({})", start, end));
        }
        Ok(RangeRequest { start, end })
    }

    /// Create range for reading first N bytes
    pub fn first(n: u64) -> Self {
        RangeRequest {
            start: 0,
            end: n - 1,
        }
    }

    /// Create range for reading last N bytes
    pub fn last(total_size: u64, n: u64) -> Self {
        RangeRequest {
            start: total_size.saturating_sub(n),
            end: total_size - 1,
        }
    }

    /// Size of this range in bytes (inclusive)
    pub fn size(&self) -> u64 {
        self.end - self.start + 1
    }

    /// HTTP Range header value
    pub fn to_header(&self) -> String {
        format!("bytes={}-{}", self.start, self.end)
    }

    /// Parse HTTP Content-Range header
    /// Format: "bytes start-end/total"
    pub fn from_content_range(header: &str) -> Result<(Self, u64), String> {
        let parts: Vec<&str> = header.split('/').collect();
        if parts.len() != 2 {
            return Err("Invalid Content-Range header".to_string());
        }

        let total_size: u64 = parts[1]
            .parse()
            .map_err(|_| "Invalid total size".to_string())?;

        let range_part = parts[0].trim_start_matches("bytes ");
        let range_parts: Vec<&str> = range_part.split('-').collect();
        if range_parts.len() != 2 {
            return Err("Invalid range format".to_string());
        }

        let start: u64 = range_parts[0]
            .parse()
            .map_err(|_| "Invalid start".to_string())?;
        let end: u64 = range_parts[1]
            .parse()
            .map_err(|_| "Invalid end".to_string())?;

        Ok((RangeRequest::new(start, end)?, total_size))
    }
}

impl fmt::Display for RangeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Range({}-{})", self.start, self.end)
    }
}

/// Strategy for reading multiple ranges efficiently
pub struct RangeStrategy {
    ranges: Vec<RangeRequest>,
}

impl RangeStrategy {
    /// Create new range strategy
    pub fn new() -> Self {
        RangeStrategy {
            ranges: Vec::new(),
        }
    }

    /// Add a range to read
    pub fn add_range(mut self, range: RangeRequest) -> Self {
        self.ranges.push(range);
        self
    }

    /// Get all ranges
    pub fn ranges(&self) -> &[RangeRequest] {
        &self.ranges
    }

    /// Optimize ranges by merging adjacent ones
    pub fn optimize(&mut self) {
        if self.ranges.len() <= 1 {
            return;
        }

        self.ranges.sort_by_key(|r| r.start);

        let mut optimized = Vec::new();
        let mut current = self.ranges[0];

        for range in &self.ranges[1..] {
            if range.start <= current.end + 1 {
                // Merge ranges
                current.end = current.end.max(range.end);
            } else {
                // Gap too large, keep separate
                optimized.push(current);
                current = *range;
            }
        }
        optimized.push(current);

        self.ranges = optimized;
    }

    /// Estimate total bytes to read
    pub fn total_bytes(&self) -> u64 {
        self.ranges.iter().map(|r| r.size()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_request_new() {
        let range = RangeRequest::new(0, 99).unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 99);
        assert_eq!(range.size(), 100);
    }

    #[test]
    fn test_range_request_invalid() {
        let result = RangeRequest::new(100, 50);
        assert!(result.is_err());
    }

    #[test]
    fn test_range_request_to_header() {
        let range = RangeRequest::new(0, 99).unwrap();
        assert_eq!(range.to_header(), "bytes=0-99");
    }

    #[test]
    fn test_range_request_first() {
        let range = RangeRequest::first(1024);
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 1023);
        assert_eq!(range.size(), 1024);
    }

    #[test]
    fn test_range_request_last() {
        let range = RangeRequest::last(10000, 1024);
        assert_eq!(range.size(), 1024);
    }

    #[test]
    fn test_range_strategy_optimize() {
        let mut strategy = RangeStrategy::new();
        strategy = strategy.add_range(RangeRequest::new(0, 99).unwrap());
        strategy = strategy.add_range(RangeRequest::new(100, 199).unwrap());
        strategy = strategy.add_range(RangeRequest::new(200, 299).unwrap());
        strategy = strategy.add_range(RangeRequest::new(500, 599).unwrap());

        strategy.optimize();

        // Should merge first 3 ranges
        assert!(strategy.ranges.len() <= 3);
    }

    #[test]
    fn test_content_range_parsing() {
        let (range, total) = RangeRequest::from_content_range("bytes 0-99/1000").unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 99);
        assert_eq!(total, 1000);
    }
}
