//! `hz`: a small deterministic LZ77 + canonical Huffman coder (deflate-like,
//! but its own byte format, not RFC 1951 compatible). Written for the v2
//! kore-store column stage (ported from the Digital Life log layer, kore-self hz.rs); no external crate. Encoding is a pure function of the
//! input (same bytes in, same bytes out), which the hash chain relies on.
//!
//! Stream: varint(original length), then blocks. Each block: 1 bit `last`,
//! code-length tables for the literal/length alphabet (286 symbols) and the
//! distance alphabet (40 symbols), then Huffman-coded tokens up to the
//! end-of-block symbol (256). Matches: length 4..=258, distance 1..=2^20.

const NLIT: usize = 286;
const NDIST: usize = 40;
const MAX_BITS: u8 = 15;
const MIN_MATCH: usize = 4;
const MAX_MATCH: usize = 258;
const MAX_DIST: usize = 1 << 20;
const BLOCK_TOKENS: usize = 1 << 18;
const CHAIN: usize = 32;

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];

#[derive(Clone, Copy)]
struct Tok {
    /// literal byte (len == 0) or distance (len >= MIN_MATCH)
    a: u32,
    len: u16,
}

// ── LZ77 ─────────────────────────────────────────────────────────────────────

fn hash4(d: &[u8], i: usize) -> usize {
    let v = u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
    (v.wrapping_mul(2_654_435_761) >> 16) as usize
}

fn lz(data: &[u8]) -> Vec<Tok> {
    let n = data.len();
    let mut toks = Vec::with_capacity(n / 3 + 16);
    let mut head = vec![u32::MAX; 1 << 16];
    let mut prev = vec![u32::MAX; n];
    let find = |head: &Vec<u32>, prev: &Vec<u32>, i: usize| -> (usize, usize) {
        if i + MIN_MATCH > n {
            return (0, 0);
        }
        let mut best = (0usize, 0usize);
        let mut cand = head[hash4(data, i)];
        let maxl = MAX_MATCH.min(n - i);
        let mut chain = 0;
        while cand != u32::MAX && chain < CHAIN {
            let c = cand as usize;
            let dist = i - c;
            if dist > MAX_DIST {
                break;
            }
            if data[c + best.0.min(maxl - 1)] == data[i + best.0.min(maxl - 1)] {
                let mut l = 0;
                while l < maxl && data[c + l] == data[i + l] {
                    l += 1;
                }
                if l > best.0 {
                    best = (l, dist);
                    if l == maxl {
                        break;
                    }
                }
            }
            cand = prev[c];
            chain += 1;
        }
        if best.0 >= MIN_MATCH {
            best
        } else {
            (0, 0)
        }
    };
    let insert = |head: &mut Vec<u32>, prev: &mut Vec<u32>, i: usize| {
        if i + MIN_MATCH <= n {
            let h = hash4(data, i);
            prev[i] = head[h];
            head[h] = i as u32;
        }
    };
    let mut i = 0;
    while i < n {
        let (l, d) = find(&head, &prev, i);
        if l >= MIN_MATCH {
            // lazy: is the next position clearly better?
            insert(&mut head, &mut prev, i);
            let (l2, _) = find(&head, &prev, i + 1);
            if l2 > l + 1 {
                toks.push(Tok { a: data[i] as u32, len: 0 });
                i += 1;
                continue;
            }
            toks.push(Tok { a: d as u32, len: l as u16 });
            for k in 1..l {
                insert(&mut head, &mut prev, i + k);
            }
            i += l;
        } else {
            insert(&mut head, &mut prev, i);
            toks.push(Tok { a: data[i] as u32, len: 0 });
            i += 1;
        }
    }
    toks
}

// ── symbol mapping ───────────────────────────────────────────────────────────

fn len_sym(l: usize) -> (usize, u32, u8) {
    let mut k = 28;
    while LBASE[k] as usize > l {
        k -= 1;
    }
    (257 + k, (l - LBASE[k] as usize) as u32, LEXTRA[k])
}

fn dist_sym(d: usize) -> (usize, u32, u8) {
    let dd = d - 1;
    if dd < 4 {
        return (dd, 0, 0);
    }
    let nb = (usize::BITS - 1 - dd.leading_zeros()) as usize; // floor(log2)
    let b = (dd >> (nb - 1)) & 1;
    let code = 2 * nb + b;
    let extra = (dd & ((1usize << (nb - 1)) - 1)) as u32;
    (code, extra, (nb - 1) as u8)
}

// ── Huffman ──────────────────────────────────────────────────────────────────

fn code_lengths(freq: &[u32]) -> Vec<u8> {
    let mut f: Vec<u32> = freq.to_vec();
    loop {
        let syms: Vec<usize> = (0..f.len()).filter(|&s| f[s] > 0).collect();
        let mut lens = vec![0u8; f.len()];
        if syms.is_empty() {
            return lens;
        }
        if syms.len() == 1 {
            lens[syms[0]] = 1;
            return lens;
        }
        // nodes: (freq, left, right); leaves first
        let mut nodes: Vec<(u64, i32, i32)> = syms.iter().map(|&s| (f[s] as u64, -1 - s as i32, 0)).collect();
        let mut heap = std::collections::BinaryHeap::new();
        for (i, nd) in nodes.iter().enumerate() {
            heap.push(std::cmp::Reverse((nd.0, i)));
        }
        while heap.len() > 1 {
            let std::cmp::Reverse((fa, a)) = heap.pop().unwrap();
            let std::cmp::Reverse((fb, b)) = heap.pop().unwrap();
            nodes.push((fa + fb, a as i32, b as i32));
            heap.push(std::cmp::Reverse((fa + fb, nodes.len() - 1)));
        }
        // depth by walking from the root
        let root = nodes.len() - 1;
        let mut stack = vec![(root, 0u32)];
        let mut max = 0u32;
        while let Some((ni, d)) = stack.pop() {
            let (_, l, r) = nodes[ni];
            if ni < syms.len() {
                let s = (-1 - l) as usize;
                lens[s] = d as u8;
                max = max.max(d);
            } else {
                stack.push((l as usize, d + 1));
                stack.push((r as usize, d + 1));
            }
        }
        if max <= MAX_BITS as u32 {
            return lens;
        }
        for x in f.iter_mut() {
            if *x > 0 {
                *x = (*x + 1) / 2;
            }
        }
    }
}

/// Canonical codes, returned bit-reversed for an LSB-first stream.
fn codes(lens: &[u8]) -> Vec<u32> {
    let mut count = [0u32; 16];
    for &l in lens {
        count[l as usize] += 1;
    }
    count[0] = 0;
    let mut next = [0u32; 16];
    let mut c = 0u32;
    for b in 1..16 {
        c = (c + count[b - 1]) << 1;
        next[b] = c;
    }
    let mut out = vec![0u32; lens.len()];
    for (s, &l) in lens.iter().enumerate() {
        if l > 0 {
            let code = next[l as usize];
            next[l as usize] += 1;
            out[s] = code.reverse_bits() >> (32 - l as u32);
        }
    }
    out
}

struct BitW {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}
impl BitW {
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn write_lengths(w: &mut BitW, lens: &[u8]) {
    let mut i = 0;
    while i < lens.len() {
        if lens[i] == 0 {
            let mut run = 1;
            while i + run < lens.len() && lens[i + run] == 0 && run < 16 {
                run += 1;
            }
            w.put(0, 4);
            w.put((run - 1) as u32, 4);
            i += run;
        } else {
            w.put(lens[i] as u32, 4);
            i += 1;
        }
    }
}

pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut head = Vec::new();
    put_varint(&mut head, data.len() as u64);
    if data.is_empty() {
        return head;
    }
    let toks = lz(data);
    let mut w = BitW { out: head, acc: 0, n: 0 };
    let nblocks = toks.len().div_ceil(BLOCK_TOKENS);
    for (bi, blk) in toks.chunks(BLOCK_TOKENS).enumerate() {
        let mut lf = vec![0u32; NLIT];
        let mut df = vec![0u32; NDIST];
        lf[256] = 1;
        for t in blk {
            if t.len == 0 {
                lf[t.a as usize] += 1;
            } else {
                lf[len_sym(t.len as usize).0] += 1;
                df[dist_sym(t.a as usize).0] += 1;
            }
        }
        let ll = code_lengths(&lf);
        let dl = code_lengths(&df);
        let lc = codes(&ll);
        let dc = codes(&dl);
        w.put((bi + 1 == nblocks) as u32, 1);
        write_lengths(&mut w, &ll);
        write_lengths(&mut w, &dl);
        for t in blk {
            if t.len == 0 {
                w.put(lc[t.a as usize], ll[t.a as usize] as u32);
            } else {
                let (s, e, eb) = len_sym(t.len as usize);
                w.put(lc[s], ll[s] as u32);
                w.put(e, eb as u32);
                let (s, e, eb) = dist_sym(t.a as usize);
                w.put(dc[s], dl[s] as u32);
                w.put(e, eb as u32);
            }
        }
        w.put(lc[256], ll[256] as u32);
    }
    w.finish()
}


// ── segmented framing used by the kore-store stage ───────────────────────────

/// Uncompressed bytes per independently coded segment (bounds memory; the match window is 1 MiB anyway).
pub const SEGMENT: usize = 8 << 20;

/// Frame: repeated `varint(compressed_len) | hz stream`, one per `SEGMENT` bytes of input.
/// Segments are coded in parallel; output order and bytes do not depend on scheduling.
pub fn compress_segmented(data: &[u8]) -> Vec<u8> {
    use rayon::prelude::*;
    let parts: Vec<Vec<u8>> = data.par_chunks(SEGMENT).map(compress).collect();
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len() + 4).sum());
    for p in parts {
        put_varint(&mut out, p.len() as u64);
        out.extend_from_slice(&p);
    }
    out
}

/// Inverse of `compress_segmented`; the caller supplies the expected total length.
pub fn decompress_segmented(inp: &[u8], total: usize) -> Result<Vec<u8>, String> {
    let mut segs: Vec<&[u8]> = Vec::new();
    let mut p = 0usize;
    while p < inp.len() {
        let (mut l, mut shift) = (0u64, 0u32);
        loop {
            let b = *inp.get(p).ok_or("hz: truncated frame")?;
            p += 1;
            l |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 { break; }
            shift += 7;
            if shift > 56 { return Err("hz: bad frame length".into()); }
        }
        let end = usize::try_from(l).ok().and_then(|l| p.checked_add(l)).filter(|&e| e <= inp.len())
            .ok_or("hz: truncated segment")?;
        segs.push(&inp[p..end]);
        p = end;
        if segs.len() > total + 1 { return Err("hz: too many segments".into()); }
    }
    if segs.len() != total.div_ceil(SEGMENT) { return Err("hz: segment count mismatch".into()); }
    use rayon::prelude::*;
    let outs: Result<Vec<Vec<u8>>, String> = segs.par_iter().map(|s| decompress_bounded(s, SEGMENT)).collect();
    let mut out = Vec::with_capacity(total);
    for o in outs? { out.extend_from_slice(&o); }
    if out.len() != total { return Err("hz: total length mismatch".into()); }
    Ok(out)
}

// ── decode ───────────────────────────────────────────────────────────────────

struct BitR<'a> {
    b: &'a [u8],
    pos: usize,
    acc: u64,
    n: u32,
}
impl<'a> BitR<'a> {
    #[inline]
    fn refill(&mut self) {
        if self.pos + 8 <= self.b.len() {
            // branch-free 64-bit refill: bits above `n` are copies of the next bytes, which stay consistent
            let v = u64::from_le_bytes(self.b[self.pos..self.pos + 8].try_into().unwrap());
            self.acc |= v << self.n;
            self.pos += ((63 - self.n) >> 3) as usize;
            self.n |= 56;
            return;
        }
        while self.n <= 56 {
            let byte = if self.pos < self.b.len() { self.b[self.pos] } else { 0 };
            self.pos += 1;
            self.acc |= (byte as u64) << self.n;
            self.n += 8;
        }
    }
    #[inline]
    fn peek(&self, bits: u32) -> u32 {
        (self.acc & ((1u64 << bits) - 1)) as u32
    }
    #[inline]
    fn eat(&mut self, bits: u32) {
        self.acc >>= bits;
        self.n -= bits;
    }
    #[inline]
    fn get(&mut self, bits: u32) -> u32 {
        self.refill();
        let v = self.peek(bits);
        self.eat(bits);
        v
    }
    /// True if we consumed bits beyond the real input.
    fn overrun(&self) -> bool {
        (self.pos * 8) as i64 - self.n as i64 > (self.b.len() * 8) as i64
    }
}

fn read_lengths(r: &mut BitR, count: usize) -> Result<Vec<u8>, String> {
    let mut lens = Vec::with_capacity(count);
    while lens.len() < count {
        let v = r.get(4) as u8;
        if v == 0 {
            let run = r.get(4) as usize + 1;
            if lens.len() + run > count {
                return Err("hz: bad zero run".into());
            }
            lens.resize(lens.len() + run, 0);
        } else {
            lens.push(v);
        }
        if r.overrun() {
            return Err("hz: truncated".into());
        }
    }
    Ok(lens)
}

fn decode_table(lens: &[u8]) -> Result<Vec<u16>, String> {
    let cs = codes(lens);
    let mut kraft = 0u64;
    for &l in lens {
        if l > 0 {
            kraft += 1u64 << (MAX_BITS - l);
        }
    }
    if kraft > 1u64 << MAX_BITS {
        return Err("hz: oversubscribed code".into());
    }
    let mut t = vec![0u16; 1 << MAX_BITS];
    for (s, &l) in lens.iter().enumerate() {
        if l == 0 {
            continue;
        }
        let step = 1usize << l;
        let mut idx = cs[s] as usize;
        while idx < t.len() {
            t[idx] = ((s as u16) << 4) | l as u16;
            idx += step;
        }
    }
    Ok(t)
}

pub fn decompress(inp: &[u8]) -> Result<Vec<u8>, String> {
    decompress_bounded(inp, 1 << 31)
}

/// Like `decompress` but rejects streams that declare more than `max_len` output bytes, and streams
/// whose declared size is beyond what the coder can express for the input size (about 1032:1).
pub fn decompress_bounded(inp: &[u8], max_len: usize) -> Result<Vec<u8>, String> {
    let (mut olen, mut shift, mut p) = (0u64, 0u32, 0usize);
    loop {
        let b = *inp.get(p).ok_or("hz: truncated header")?;
        p += 1;
        olen |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 56 {
            return Err("hz: bad length".into());
        }
    }
    let olen = olen as usize;
    if olen > max_len || olen > inp.len().saturating_mul(1100).saturating_add(64) {
        return Err("hz: implausible length".into());
    }
    let mut out: Vec<u8> = Vec::with_capacity(olen.min(1 << 24));
    if olen == 0 {
        return Ok(out);
    }
    let mut r = BitR { b: &inp[p..], pos: 0, acc: 0, n: 0 };
    loop {
        let last = r.get(1);
        let ll = read_lengths(&mut r, NLIT)?;
        let dl = read_lengths(&mut r, NDIST)?;
        let lt = decode_table(&ll)?;
        let dt = decode_table(&dl)?;
        loop {
            r.refill();
            let e = lt[r.peek(MAX_BITS as u32) as usize];
            let l = (e & 15) as u32;
            if l == 0 {
                return Err("hz: invalid literal code".into());
            }
            r.eat(l);
            let s = (e >> 4) as usize;
            if s < 256 {
                out.push(s as u8);
            } else if s == 256 {
                break;
            } else {
                let k = s - 257;
                if k >= 29 {
                    return Err("hz: bad length symbol".into());
                }
                let len = LBASE[k] as usize + r.get(LEXTRA[k] as u32) as usize;
                r.refill();
                let de = dt[r.peek(MAX_BITS as u32) as usize];
                let dlen = (de & 15) as u32;
                if dlen == 0 {
                    return Err("hz: invalid distance code".into());
                }
                r.eat(dlen);
                let code = (de >> 4) as usize;
                let dd = if code < 4 {
                    code
                } else {
                    let nb = code / 2;
                    let b = code & 1;
                    (1usize << nb) + (b << (nb - 1)) + r.get((nb - 1) as u32) as usize
                };
                let dist = dd + 1;
                if dist > out.len() {
                    return Err("hz: distance beyond output".into());
                }
                let start = out.len() - dist;
                if dist >= len {
                    out.extend_from_within(start..start + len);
                } else {
                    for j in 0..len {
                        let b = out[start + j];
                        out.push(b);
                    }
                }
            }
            if out.len() > olen {
                return Err("hz: output longer than declared".into());
            }
            if r.overrun() {
                return Err("hz: truncated stream".into());
            }
        }
        if last == 1 {
            break;
        }
    }
    if out.len() != olen {
        return Err("hz: output length mismatch".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(d: &[u8]) {
        let c = compress(d);
        assert_eq!(decompress(&c).unwrap(), d);
        assert_eq!(compress(d), c, "deterministic");
    }

    #[test]
    fn roundtrips() {
        rt(b"");
        rt(b"a");
        rt(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        rt(b"the quick brown fox jumps over the lazy dog the quick brown fox");
        let mut x = 12345u64;
        let rnd: Vec<u8> = (0..70_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        rt(&rnd);
        let mut mixed = Vec::new();
        for i in 0..40_000u32 {
            mixed.extend_from_slice(format!("line {} of the log, value {}\n", i % 977, i * 7 % 13).as_bytes());
        }
        rt(&mixed);
        let c = compress(&mixed);
        assert!(c.len() < mixed.len() / 4);
        // more than one block
        let big: Vec<u8> = (0..1_500_000u32).map(|i| (i.wrapping_mul(2654435761u32) >> 24) as u8 & 0x3f).collect();
        rt(&big);
    }

    #[test]
    fn segmented_roundtrip() {
        let d: Vec<u8> = (0..(SEGMENT as u32 + 12345)).map(|i| (i % 251) as u8 ^ (i >> 12) as u8).collect();
        let c = compress_segmented(&d);
        assert_eq!(decompress_segmented(&c, d.len()).unwrap(), d);
        assert_eq!(compress_segmented(&d), c);
        assert!(decompress_segmented(&c, d.len() + 1).is_err());
    }

    #[test]
    fn corrupt_input_errors_not_panics() {
        let mut d = Vec::new();
        for i in 0..5000u32 {
            d.extend_from_slice(format!("row {i} text\n").as_bytes());
        }
        let c = compress(&d);
        for cut in [0, 1, 3, c.len() / 2, c.len() - 1] {
            let _ = decompress(&c[..cut]);
        }
        for i in (0..c.len()).step_by(37) {
            let mut b = c.clone();
            b[i] ^= 0x55;
            let _ = decompress(&b);
        }
    }
}
