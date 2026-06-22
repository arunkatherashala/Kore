use std::io::{self, Read, Seek, SeekFrom, BufReader};

/// Simple length-prefixed block decoder for prototype purposes.
/// Format: optional 4-byte magic `KORB`. Then sequence of blocks, each:
/// 4-byte LE length (u32) followed by `length` bytes payload. Payload is
/// newline-delimited rows (UTF-8). This is NOT the real KORE binary format —
/// it's a pragmatic prototype to enable block-aware processing.
pub struct BlockDecoder<R: Read + Seek> {
    reader: BufReader<R>,
    finished: bool,
}

impl<R: Read + Seek> BlockDecoder<R> {
    pub fn new(reader: R) -> io::Result<Self> {
        let mut buf = [0u8;4];
        // peek magic (if present)
        let mut br = BufReader::new(reader);
        let pos = br.seek(SeekFrom::Current(0))?;
        if br.read_exact(&mut buf).is_ok() {
            if &buf == b"KORB" {
                // magic consumed
            } else {
                // rewind
                br.seek(SeekFrom::Start(pos))?;
            }
        }
        Ok(BlockDecoder { reader: br, finished: false })
    }

    /// Decode all blocks, invoking `cb` for each decoded row slice.
    pub fn decode_all<F: FnMut(String)>(&mut self, mut cb: F) -> io::Result<()> {
        while !self.finished {
            // read length
            let mut lenb = [0u8;4];
            match self.reader.read_exact(&mut lenb) {
                Ok(_) => {},
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => { self.finished = true; break; }
                Err(e) => return Err(e),
            }
            let len = u32::from_le_bytes(lenb) as usize;
            if len == 0 { continue; }
            let mut payload = vec![0u8; len];
            self.reader.read_exact(&mut payload)?;
            if let Ok(s) = String::from_utf8(payload) {
                for line in s.lines() { cb(line.to_string()); }
            }
        }
        Ok(())
    }
}
