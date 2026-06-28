//! KorePress — Layer 10: LZ77 + RLE file compression (pure Rust)
//! Header: KORPRS01(8) | algo(1) | orig_size_le(8) | payload

use std::fs;

const MAGIC: &[u8; 8] = b"KORPRS01";

pub struct CompressResult { pub algo: String, pub original_size: u64, pub compressed_size: u64, pub ratio: f64 }
pub struct CompressInfo   { pub algo: String, pub original_size: u64, pub compressed_size: u64, pub ratio: f64 }

pub struct KorePress;
impl KorePress {
    /// Compress src -> dst. algo: "rle" | "lz77" | "auto"
    pub fn compress(src: &str, dst: &str, algo: &str) -> Result<CompressResult, String> {
        let data = fs::read(src).map_err(|e| e.to_string())?;
        let orig = data.len() as u64;
        let (ab, payload) = match algo {
            "rle"  => (1u8, Self::rle_enc(&data)),
            "lz77" => (2u8, Self::lz77_enc(&data)),
            _      => {
                let r = Self::rle_enc(&data);
                let l = Self::lz77_enc(&data);
                if r.len() <= l.len() { (1u8, r) } else { (2u8, l) }
            }
        };
        let comp_size = payload.len() as u64;
        let ratio = if orig > 0 { comp_size as f64 / orig as f64 } else { 1.0 };
        let algo_name = if ab == 1 { "rle" } else { "lz77" }.to_string();
        let mut out = Vec::with_capacity(17 + payload.len());
        out.extend_from_slice(MAGIC); out.push(ab);
        out.extend_from_slice(&orig.to_le_bytes());
        out.extend_from_slice(&payload);
        fs::write(dst, &out).map_err(|e| e.to_string())?;
        Ok(CompressResult { algo: algo_name, original_size: orig, compressed_size: comp_size, ratio })
    }

    /// Decompress src -> dst.
    pub fn decompress(src: &str, dst: &str) -> Result<(), String> {
        let data = fs::read(src).map_err(|e| e.to_string())?;
        if data.len() < 17 || &data[..8] != MAGIC { return Err("Not a KorePress file".into()); }
        let ab = data[8];
        let orig = u64::from_le_bytes(data[9..17].try_into().unwrap()) as usize;
        let payload = &data[17..];
        let out = match ab {
            1 => Self::rle_dec(payload, orig),
            2 => Self::lz77_dec(payload, orig),
            _ => return Err(format!("Unknown algo {}", ab)),
        }?;
        fs::write(dst, &out).map_err(|e| e.to_string())
    }

    /// Return info about a compressed file without decompressing.
    pub fn info(path: &str) -> Result<CompressInfo, String> {
        let data = fs::read(path).map_err(|e| e.to_string())?;
        if data.len() < 17 || &data[..8] != MAGIC { return Err("Not a KorePress file".into()); }
        let ab = data[8];
        let orig = u64::from_le_bytes(data[9..17].try_into().unwrap());
        let comp = (data.len() as u64).saturating_sub(17);
        let ratio = if orig > 0 { comp as f64 / orig as f64 } else { 1.0 };
        let algo = match ab { 1 => "rle", 2 => "lz77", _ => "unknown" }.to_string();
        Ok(CompressInfo { algo, original_size: orig, compressed_size: comp, ratio })
    }

    // ── RLE: (count:u8, byte:u8) pairs ───────────────────────────────────
    fn rle_enc(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        while i < data.len() {
            let b = data[i]; let mut n = 1usize;
            while i + n < data.len() && data[i+n] == b && n < 255 { n += 1; }
            out.push(n as u8); out.push(b); i += n;
        }
        out
    }
    fn rle_dec(data: &[u8], _orig: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::new(); let mut i = 0;
        while i + 1 < data.len() {
            for _ in 0..data[i] as usize { out.push(data[i+1]); }
            i += 2;
        }
        Ok(out)
    }

    // ── LZ77 with hash table — O(n) average ──────────────────────────────
    // Tokens: 0x00,byte = literal; 0x01,off_le16,len_u8 = back-ref
    fn lz77_enc(data: &[u8]) -> Vec<u8> {
        const HS: usize = 1 << 14; const HM: usize = HS - 1;
        const WIN: usize = 65535; const MIN_M: usize = 4; const MAX_M: usize = 255;
        let mut ht = vec![0usize; HS]; let mut out = Vec::new(); let mut i = 0;
        while i < data.len() {
            if i + MIN_M > data.len() { out.push(0u8); out.push(data[i]); i += 1; continue; }
            let h = Self::h4(&data[i..i+4]) & HM;
            let j = ht[h]; ht[h] = i;
            let off = i.saturating_sub(j);
            if off > 0 && off <= WIN && j + MIN_M <= i &&
               data.get(j..j+MIN_M) == data.get(i..i+MIN_M) {
                let mut len = MIN_M;
                while len < MAX_M && i+len < data.len() && j+len < i &&
                      data[j+len] == data[i+len] { len += 1; }
                out.push(1u8); out.extend_from_slice(&(off as u16).to_le_bytes()); out.push(len as u8);
                i += len;
            } else { out.push(0u8); out.push(data[i]); i += 1; }
        }
        out
    }
    fn lz77_dec(data: &[u8], _orig: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::new(); let mut i = 0;
        while i < data.len() {
            match data[i] {
                0 => { if i+1 < data.len() { out.push(data[i+1]); } i += 2; }
                1 => {
                    if i+3 >= data.len() { break; }
                    let off = u16::from_le_bytes([data[i+1], data[i+2]]) as usize;
                    let len = data[i+3] as usize; i += 4;
                    if off == 0 || off > out.len() { continue; }
                    let base = out.len() - off;
                    for k in 0..len { let b = out[base + k % off]; out.push(b); }
                }
                _ => { i += 1; }
            }
        }
        Ok(out)
    }
    fn h4(d: &[u8]) -> usize {
        let v = u32::from_le_bytes([d[0],d[1],d[2],d[3]]);
        (v.wrapping_mul(0x9E3779B1) >> 18) as usize
    }
}
