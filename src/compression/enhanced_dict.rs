// src/compression/enhanced_dict.rs
// Multi-level dictionary encoder for improved compression

use crate::compression::CompressionStats;

/// Multi-level dictionary: trade memory for compression efficiency
/// Level 1: Byte indices (fits 256 values)
/// Level 2: Short indices (fits 65K values)
/// Level 3: Fallback (rare values, direct encoding)
pub struct MultiLevelDictionary {
    level1: Vec<Vec<u8>>,          // Top 256 values
    level2: Vec<Vec<u8>>,          // Next 65K values
    level3: Vec<Vec<u8>>,          // Rare values
    _indices_level1: Vec<u8>,       // Index into level1
    _indices_level2: Vec<u16>,      // Index into level2
    _level3_entries: Vec<(Vec<u8>, u32)>, // (value, count)
}

impl MultiLevelDictionary {
    pub fn new() -> Self {
        Self {
            level1: vec![],
            level2: vec![],
            level3: vec![],
            _indices_level1: vec![],
            _indices_level2: vec![],
            _level3_entries: vec![],
        }
    }

    /// Build multi-level dictionary from sorted (value, count) pairs
    pub fn build(&mut self, value_counts: &[(Vec<u8>, u32)]) {
        let mut sorted = value_counts.to_vec();
        sorted.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by count descending

        let mut total_count = 0;
        let total_sum: u32 = sorted.iter().map(|(_, c)| c).sum();

        for (value, count) in sorted {
            total_count += count;
            let percentage = (total_count as f64 / total_sum as f64) * 100.0;

            if self.level1.len() < 256 && percentage < 80.0 {
                self.level1.push(value);
            } else if self.level2.len() < 65536 && percentage < 99.0 {
                self.level2.push(value);
            } else {
                self.level3.push(value);
            }
        }
    }

    /// Encode value to appropriate index level
    pub fn encode_value(&self, value: &[u8]) -> (Vec<u8>, u8) {
        // Check level 1 (1 byte)
        if let Some(idx) = self.level1.iter().position(|v| v == value) {
            return (vec![idx as u8], 1);
        }

        // Check level 2 (2 bytes)
        if let Some(idx) = self.level2.iter().position(|v| v == value) {
            let bytes = (idx as u16).to_le_bytes().to_vec();
            return (bytes, 2);
        }

        // Fallback to level 3 (full encoding)
        (value.to_vec(), 3)
    }

    /// Compress using multi-level indices
    pub fn compress(&self, data: &[&[u8]], stats: &mut CompressionStats) -> Option<Vec<u8>> {
        let mut result = vec![];

        // Header
        result.push(self.level1.len() as u8);
        result.extend((self.level2.len() as u16).to_le_bytes());

        // Level 1 dictionary
        for entry in &self.level1 {
            result.push(entry.len() as u8);
            result.extend_from_slice(entry);
        }

        // Level 2 dictionary
        for entry in &self.level2 {
            result.push((entry.len() >> 8) as u8);
            result.push(entry.len() as u8);
            result.extend_from_slice(entry);
        }

        // Encoded data
        for value in data {
            let (encoded, level) = self.encode_value(value);
            result.push(level);
            result.extend(encoded);
        }

        let original_total: usize = data.iter().map(|v| v.len()).sum();
        stats.original_size += original_total;
        stats.compressed_size += result.len();

        if result.len() >= original_total {
            return None; // Expansion detected
        }

        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_level_dictionary() {
        let mut dict = MultiLevelDictionary::new();
        
        let values = vec![
            (b"active".to_vec(), 450),
            (b"inactive".to_vec(), 350),
            (b"pending".to_vec(), 190),
            (b"archived".to_vec(), 10),
        ];
        
        dict.build(&values);
        
        // Most common should be in level 1 (1 byte)
        let (encoded, level) = dict.encode_value(b"active");
        assert_eq!(level, 1);
        assert_eq!(encoded.len(), 1);
    }
}
