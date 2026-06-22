use std::io::{self, Read};

/// Prototype stateful KORE block/codec decoder for the `kore_fileformat` crate.
pub struct KoreBlockDecoder<R: Read> {
    reader: R,
    state: DecoderState,
}

#[derive(Default)]
struct DecoderState {
    _placeholder: u8,
}

impl<R: Read> KoreBlockDecoder<R> {
    pub fn new(reader: R) -> io::Result<Self> {
        Ok(Self { reader, state: DecoderState::default() })
    }

    /// Reads a length-prefixed block (u32 BE) and appends payload to `out`.
    pub fn decode_next_block(&mut self, out: &mut Vec<u8>) -> io::Result<usize> {
        use byteorder::{BigEndian, ReadBytesExt};
        let len = match self.reader.read_u32::<BigEndian>() {
            Ok(v) => v as usize,
            Err(e) => return Err(e),
        };
        let mut buf = vec![0u8; len];
        self.reader.read_exact(&mut buf)?;
        out.extend_from_slice(&buf);
        Ok(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn decode_length_prefixed_blocks() {
        let mut v = Vec::new();
        v.extend(&3u32.to_be_bytes());
        v.extend(b"abc");
        v.extend(&4u32.to_be_bytes());
        v.extend(b"DEFG");
        let cur = Cursor::new(v);
        let mut dec = KoreBlockDecoder::new(cur).unwrap();
        let mut out = Vec::new();
        let n1 = dec.decode_next_block(&mut out).unwrap();
        assert_eq!(n1, 3);
        let n2 = dec.decode_next_block(&mut out).unwrap();
        assert_eq!(n2, 4);
        assert_eq!(&out, b"abcDEFG");
    }
}
