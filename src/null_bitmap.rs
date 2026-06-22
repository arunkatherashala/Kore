pub fn pack_presence_bits(presence: &[u8]) -> Vec<u8> {
    let mut packed = Vec::new();
    let mut byte = 0u8;
    for (i, &bit) in presence.iter().enumerate() {
        if bit == 1 { byte |= 1 << (i % 8); }
        if i % 8 == 7 { packed.push(byte); byte = 0; }
    }
    if presence.len() % 8 != 0 { packed.push(byte); }
    packed
}

pub fn build_presence_from_bools(bools: &[bool], row_count: usize) -> Vec<u8> {
    if bools.len() == row_count {
        // 1 = present, 0 = null
        bools.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect()
    } else {
        // Fallback: assume all present
        vec![1u8; row_count]
    }
}

/// Count null rows from a packed presence bitmap (1 = present, 0 = null).
/// Only the first `row_count` bits are considered; padding bits in the last byte are ignored.
pub fn count_nulls_in_packed_bitmap(bitmap: &[u8], row_count: u64) -> u64 {
    let mut null_count = 0u64;
    for i in 0..row_count as usize {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        let present = bitmap
            .get(byte_idx)
            .map(|byte| (byte >> bit_idx) & 1)
            .unwrap_or(0);
        if present == 0 {
            null_count += 1;
        }
    }
    null_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_nulls_ignores_padding_bits() {
        // 10 rows, nulls at positions 1, 3, 7
        let presence = vec![1u8, 0, 1, 0, 1, 1, 1, 0, 1, 1];
        let packed = pack_presence_bits(&presence);
        assert_eq!(count_nulls_in_packed_bitmap(&packed, 10), 3);
    }
}
