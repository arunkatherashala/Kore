pub struct ForCodec;

impl ForCodec {
    /// Simple FOR (frame-of-reference) demo decompressor.
    /// Expects input as: base (i64 little-endian, 8 bytes) followed by a sequence of deltas as i32 LE.
    /// Output: concatenated lines of decimal values with newline.
    pub fn decompress(input: &[u8]) -> std::io::Result<Vec<u8>> {
        use std::convert::TryInto;
        if input.len() < 8 { return Ok(vec![]); }
        let base = i64::from_le_bytes(input[0..8].try_into().unwrap());
        let mut out = Vec::new();
        let mut offset = 8;
        while offset + 4 <= input.len() {
            let d = i32::from_le_bytes(input[offset..offset+4].try_into().unwrap()) as i64;
            let v = base + d;
            out.extend_from_slice(v.to_string().as_bytes());
            out.push(b'\n');
            offset += 4;
        }
        Ok(out)
    }
}
