#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_decompress_roundtrip() {
        // base = 100, deltas = [1,2,3]
        let mut data = Vec::new();
        data.extend_from_slice(&100i64.to_le_bytes());
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&3i32.to_le_bytes());
        let out = crate::codecs::ForCodec::decompress(&data).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("101"));
        assert!(s.contains("102"));
        assert!(s.contains("103"));
    }

    #[test]
    fn rle_decompress_roundtrip() {
        // value 'A' x3, value 'B' x2
        let mut data = Vec::new();
        data.push(b'A'); data.extend_from_slice(&3u32.to_le_bytes());
        data.push(b'B'); data.extend_from_slice(&2u32.to_le_bytes());
        let out = crate::codecs::RleCodec::decompress(&data).unwrap();
        assert_eq!(out, vec![b'A',b'A',b'A',b'B',b'B']);
    }
}
