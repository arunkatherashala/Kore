pub struct PackedCodec;

impl PackedCodec {
    /// Demo packed codec: assumes input is raw bytes representing ASCII lines; returns as-is.
    pub fn decompress(input: &[u8]) -> std::io::Result<Vec<u8>> {
        Ok(input.to_vec())
    }
}
