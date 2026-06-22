pub struct RleCodec;

impl RleCodec {
    /// Simple RLE decompressor demo.
    /// Format: series of (value_byte, count_u32_le) pairs. Emits `value_byte` repeated `count` times.
    pub fn decompress(input: &[u8]) -> std::io::Result<Vec<u8>> {
        use std::convert::TryInto;
        let mut out = Vec::new();
        let mut i = 0;
        while i + 5 <= input.len() {
            let val = input[i];
            let cnt = u32::from_le_bytes(input[i+1..i+5].try_into().unwrap()) as usize;
            for _ in 0..cnt { out.push(val); }
            i += 5;
        }
        Ok(out)
    }
}
