/// TRACK D: Time-Series Optimized Codecs
/// FOR codec tuning + time-range indexes for InfluxDB/Prometheus integration
/// Target: 20-30% compression improvement on metrics data

use std::collections::HashMap;

/// Time-series optimized Frame-of-Reference codec
pub struct TimeSeriesForCodec {
    frame_size: usize,
    detect_monotonic: bool,
}

impl TimeSeriesForCodec {
    pub fn new(frame_size: usize) -> Self {
        Self {
            frame_size,
            detect_monotonic: true,
        }
    }

    /// Encode timestamps using monotonic sequence detection
    pub fn encode_timestamps(&self, timestamps: &[i64]) -> Vec<u8> {
        let mut result = Vec::new();

        // Check if monotonically increasing (common in time-series)
        let is_monotonic = timestamps.windows(2).all(|w| w[0] < w[1]);

        if is_monotonic {
            // Use delta-of-delta encoding for timestamps
            result.push(1u8); // Flag: monotonic
            result.extend_from_slice(&self.encode_delta_of_delta(timestamps));
        } else {
            // Fallback to standard FOR
            result.push(0u8); // Flag: not monotonic
            result.extend_from_slice(&self.encode_standard_for(timestamps));
        }

        result
    }

    /// Delta-of-delta encoding: optimal for monotonic time-series
    fn encode_delta_of_delta(&self, values: &[i64]) -> Vec<u8> {
        let mut result = Vec::new();

        if values.is_empty() {
            return result;
        }

        // Store first value
        result.extend_from_slice(&values[0].to_le_bytes());

        if values.len() < 2 {
            return result;
        }

        // Store first delta
        let first_delta = values[1] - values[0];
        result.extend_from_slice(&first_delta.to_le_bytes());

        // Store delta-of-delta
        for i in 2..values.len() {
            let delta = values[i] - values[i - 1];
            let delta_of_delta = delta - (values[i - 1] - values[i - 2]);
            let compact = delta_of_delta as i16; // Fits in 2 bytes
            result.extend_from_slice(&compact.to_le_bytes());
        }

        result
    }

    /// Standard FOR encoding
    fn encode_standard_for(&self, values: &[i64]) -> Vec<u8> {
        let mut result = Vec::new();

        if let Some(&base) = values.first() {
            result.extend_from_slice(&base.to_le_bytes());

            for &val in values {
                let delta = (val - base) as u8;
                result.push(delta);
            }
        }

        result
    }

    /// Decode timestamps
    pub fn decode_timestamps(&self, data: &[u8]) -> Vec<i64> {
        if data.is_empty() {
            return vec![];
        }

        let is_monotonic = data[0] == 1;

        if is_monotonic {
            self.decode_delta_of_delta(&data[1..])
        } else {
            self.decode_standard_for(&data[1..])
        }
    }

    fn decode_delta_of_delta(&self, data: &[u8]) -> Vec<i64> {
        let mut result = Vec::new();

        if data.len() < 8 {
            return result;
        }

        let first = i64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);
        result.push(first);

        if data.len() < 16 {
            return result;
        }

        let first_delta = i64::from_le_bytes([
            data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
        ]);
        result.push(first + first_delta);

        let mut prev_delta = first_delta;
        let mut i = 16;

        while i + 1 < data.len() {
            let delta_of_delta =
                i16::from_le_bytes([data[i], data[i + 1]]) as i64;
            let delta = prev_delta + delta_of_delta;
            let val = result.last().unwrap() + delta;
            result.push(val);
            prev_delta = delta;
            i += 2;
        }

        result
    }

    fn decode_standard_for(&self, data: &[u8]) -> Vec<i64> {
        let mut result = Vec::new();

        if data.len() >= 8 {
            let base = i64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]);
            result.push(base);

            for i in 8..data.len() {
                let delta = data[i] as i64;
                result.push(base + delta);
            }
        }

        result
    }
}

/// Time-range index for efficient range queries
pub struct TimeRangeIndex {
    blocks: HashMap<u64, TimeBlock>,
}

pub struct TimeBlock {
    pub min_timestamp: i64,
    pub max_timestamp: i64,
    pub row_count: u64,
    pub block_id: u64,
}

impl TimeRangeIndex {
    pub fn new() -> Self {
        Self {
            blocks: HashMap::new(),
        }
    }

    /// Add a time block to the index
    pub fn add_block(
        &mut self,
        block_id: u64,
        min_ts: i64,
        max_ts: i64,
        row_count: u64,
    ) {
        self.blocks.insert(
            block_id,
            TimeBlock {
                min_timestamp: min_ts,
                max_timestamp: max_ts,
                row_count,
                block_id,
            },
        );
    }

    /// Find blocks in time range (for predicate pushdown)
    pub fn query_range(&self, start_ts: i64, end_ts: i64) -> Vec<u64> {
        self.blocks
            .values()
            .filter(|block| {
                block.min_timestamp <= end_ts && block.max_timestamp >= start_ts
            })
            .map(|block| block.block_id)
            .collect()
    }

    /// Skip blocks outside range (manifest-level optimization)
    pub fn can_skip_range(&self, block_id: u64, start_ts: i64, end_ts: i64) -> bool {
        if let Some(block) = self.blocks.get(&block_id) {
            block.max_timestamp < start_ts || block.min_timestamp > end_ts
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monotonic_timestamps() {
        let codec = TimeSeriesForCodec::new(4);
        let timestamps = vec![1000i64, 2000, 3000, 4000, 5000];
        let encoded = codec.encode_timestamps(&timestamps);
        let decoded = codec.decode_timestamps(&encoded);
        assert_eq!(decoded, timestamps);
    }

    #[test]
    fn test_time_range_index() {
        let mut index = TimeRangeIndex::new();
        index.add_block(1, 1000, 2000, 100);
        index.add_block(2, 2001, 3000, 100);
        index.add_block(3, 3001, 4000, 100);

        let results = index.query_range(1500, 2500);
        assert!(results.contains(&1));
        assert!(results.contains(&2));
        assert!(!results.contains(&3));
    }
}
