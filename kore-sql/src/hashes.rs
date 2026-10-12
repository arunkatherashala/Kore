//! MD5, SHA-1, SHA-2 and CRC32 for the `md5`, `sha1`, `sha2` and `crc32` SQL functions.

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{:02x}", b)).collect() }

fn pad_message(data: &[u8], block: usize, big_endian_len: bool) -> Vec<u8> {
    // block = 64 (MD5, SHA-1, SHA-256: 8-byte length) or 128 (SHA-512: 16-byte length)
    let len_bytes = if block == 64 { 8 } else { 16 };
    let mut m = data.to_vec();
    m.push(0x80);
    while m.len() % block != block - len_bytes { m.push(0); }
    let bits = (data.len() as u128) * 8;
    if block == 64 {
        let b = bits as u64;
        m.extend_from_slice(&if big_endian_len { b.to_be_bytes() } else { b.to_le_bytes() });
    } else {
        m.extend_from_slice(&bits.to_be_bytes());
    }
    m
}

pub fn md5(data: &[u8]) -> String {
    const S: [u32; 64] = [7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    for chunk in pad_message(data, 64, false).chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (mut f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d; d = c; c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a); b0 = b0.wrapping_add(b); c0 = c0.wrapping_add(c); d0 = d0.wrapping_add(d);
    }
    let mut out = Vec::new();
    for v in [a0, b0, c0, d0] { out.extend_from_slice(&v.to_le_bytes()); }
    hex(&out)
}

pub fn sha1(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    for chunk in pad_message(data, 64, true).chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]); }
        for i in 16..80 { w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1); }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5A827999u32),
                1 => (b ^ c ^ d, 0x6ED9EBA1),
                2 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d; d = c; c = b.rotate_left(30); b = a; a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) { *x = x.wrapping_add(y); }
    }
    hex(&h.iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<u8>>())
}

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_core(data: &[u8], init: [u32; 8], out_words: usize) -> String {
    let mut h = init;
    for chunk in pad_message(data, 64, true).chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]); }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6]; v[6] = v[5]; v[5] = v[4]; v[4] = v[3].wrapping_add(t1);
            v[3] = v[2]; v[2] = v[1]; v[1] = v[0]; v[0] = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip(v) { *x = x.wrapping_add(y); }
    }
    hex(&h[..out_words].iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<u8>>())
}

const K512: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc, 0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2, 0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65, 0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4, 0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df, 0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30, 0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8, 0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec, 0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178, 0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c, 0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

fn sha512_core(data: &[u8], init: [u64; 8], out_bytes: usize) -> String {
    let mut h = init;
    for chunk in pad_message(data, 128, true).chunks(128) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&chunk[8 * i..8 * i + 8]);
            w[i] = u64::from_be_bytes(b);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..80 {
            let s1 = v[4].rotate_right(14) ^ v[4].rotate_right(18) ^ v[4].rotate_right(41);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K512[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(28) ^ v[0].rotate_right(34) ^ v[0].rotate_right(39);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6]; v[6] = v[5]; v[5] = v[4]; v[4] = v[3].wrapping_add(t1);
            v[3] = v[2]; v[2] = v[1]; v[1] = v[0]; v[0] = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip(v) { *x = x.wrapping_add(y); }
    }
    let bytes: Vec<u8> = h.iter().flat_map(|v| v.to_be_bytes()).collect();
    hex(&bytes[..out_bytes])
}

/// `sha2(data, bits)` for 224, 256 (also 0), 384 and 512; None for any other width.
pub fn sha2(data: &[u8], bits: i64) -> Option<String> {
    Some(match bits {
        0 | 256 => sha256_core(data, [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19], 8),
        224 => sha256_core(data, [0xc1059ed8, 0x367cd507, 0x3070dd17, 0xf70e5939, 0xffc00b31, 0x68581511, 0x64f98fa7, 0xbefa4fa4], 7),
        512 => sha512_core(data, [0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1, 0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179], 64),
        384 => sha512_core(data, [0xcbbb9d5dc1059ed8, 0x629a292a367cd507, 0x9159015a3070dd17, 0x152fecd8f70e5939, 0x67332667ffc00b31, 0x8eb44a8768581511, 0xdb0c2e0d64f98fa7, 0x47b5481dbefa4fa4], 48),
        _ => return None,
    })
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 { crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 }; }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_digests() {
        assert_eq!(md5(b"a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(md5(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(sha1(b"a"), "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8");
        assert_eq!(sha2(b"a", 256).unwrap(), "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb");
        assert_eq!(sha2(b"abc", 224).unwrap(), "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7");
        assert_eq!(sha2(b"abc", 512).unwrap(), "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
        assert_eq!(sha2(b"abc", 384).unwrap(), "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
        assert_eq!(crc32(b"a"), 3904355907);
        // multi-block input
        assert_eq!(md5(&[b'a'; 1000]), "cabe45dcc9ae5b66ba86600cca6b8ba8");
    }
}

// ─── Spark's hash() (Murmur3 x86_32, seed 42) and xxhash64() ────────────────────

/// A value as `hash()` / `xxhash64()` see it: Spark hashes INT and BIGINT differently.
#[derive(Debug, Clone)]
pub enum HashVal { Null, Int32(i32), Int64(i64), Double(f64), Bool(bool), Bytes(Vec<u8>), Seq(Vec<HashVal>) }
// A DECIMAL literal (`2.5`) is hashed by Spark from its unscaled value: `Int64` of 25 is what the caller passes for it.

fn mix_k1(k1: i32) -> i32 { (k1.wrapping_mul(0xcc9e2d51u32 as i32)).rotate_left(15).wrapping_mul(0x1b873593) }
fn mix_h1(h1: i32, k1: i32) -> i32 { (h1 ^ k1).rotate_left(13).wrapping_mul(5).wrapping_add(0xe6546b64u32 as i32) }
fn fmix32(mut h1: i32, len: i32) -> i32 {
    h1 ^= len;
    h1 ^= ((h1 as u32) >> 16) as i32;
    h1 = h1.wrapping_mul(0x85ebca6bu32 as i32);
    h1 ^= ((h1 as u32) >> 13) as i32;
    h1 = h1.wrapping_mul(0xc2b2ae35u32 as i32);
    h1 ^= ((h1 as u32) >> 16) as i32;
    h1
}
fn m3_int(i: i32, seed: i32) -> i32 { fmix32(mix_h1(seed, mix_k1(i)), 4) }
fn m3_long(l: i64, seed: i32) -> i32 {
    let h1 = mix_h1(seed, mix_k1(l as i32));
    let h1 = mix_h1(h1, mix_k1((l >> 32) as i32));
    fmix32(h1, 8)
}
fn m3_bytes(b: &[u8], seed: i32) -> i32 {
    let aligned = b.len() - b.len() % 4;
    let mut h1 = seed;
    for c in b[..aligned].chunks(4) { h1 = mix_h1(h1, mix_k1(i32::from_le_bytes([c[0], c[1], c[2], c[3]]))); }
    // Spark mixes the remaining bytes one at a time (sign-extended), unlike reference Murmur3
    for &x in &b[aligned..] { h1 = mix_h1(h1, mix_k1(x as i8 as i32)); }
    fmix32(h1, b.len() as i32)
}

const P1: u64 = 11400714785074694791;
const P2: u64 = 14029467366897019727;
const P3: u64 = 1609587929392839161;
const P4: u64 = 9650029242287828579;
const P5: u64 = 2870177450012600261;
fn xx_fmix(mut h: u64) -> u64 { h ^= h >> 33; h = h.wrapping_mul(P2); h ^= h >> 29; h = h.wrapping_mul(P3); h ^= h >> 32; h }
fn xx_int(i: i32, seed: i64) -> i64 {
    let mut h = (seed as u64).wrapping_add(P5).wrapping_add(4);
    h ^= (i as u32 as u64).wrapping_mul(P1);
    h = h.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
    xx_fmix(h) as i64
}
fn xx_long(l: i64, seed: i64) -> i64 {
    let mut h = (seed as u64).wrapping_add(P5).wrapping_add(8);
    h ^= (l as u64).wrapping_mul(P2).rotate_left(31).wrapping_mul(P1);
    h = h.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
    xx_fmix(h) as i64
}
fn xx_bytes(b: &[u8], seed: i64) -> i64 {
    let seed = seed as u64;
    let len = b.len();
    let rd64 = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
    let rd32 = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as u64;
    let round = |acc: u64, w: u64| acc.wrapping_add(w.wrapping_mul(P2)).rotate_left(31).wrapping_mul(P1);
    let mut off = 0;
    let mut h: u64;
    if len >= 32 {
        let (mut v1, mut v2, mut v3, mut v4) = (seed.wrapping_add(P1).wrapping_add(P2), seed.wrapping_add(P2), seed, seed.wrapping_sub(P1));
        while off + 32 <= len {
            v1 = round(v1, rd64(off)); v2 = round(v2, rd64(off + 8)); v3 = round(v3, rd64(off + 16)); v4 = round(v4, rd64(off + 24));
            off += 32;
        }
        h = v1.rotate_left(1).wrapping_add(v2.rotate_left(7)).wrapping_add(v3.rotate_left(12)).wrapping_add(v4.rotate_left(18));
        for v in [v1, v2, v3, v4] { h ^= round(0, v); h = h.wrapping_mul(P1).wrapping_add(P4); }
    } else {
        h = seed.wrapping_add(P5);
    }
    h = h.wrapping_add(len as u64);
    while off + 8 <= len { h ^= round(0, rd64(off)); h = h.rotate_left(27).wrapping_mul(P1).wrapping_add(P4); off += 8; }
    if off + 4 <= len { h ^= rd32(off).wrapping_mul(P1); h = h.rotate_left(23).wrapping_mul(P2).wrapping_add(P3); off += 4; }
    while off < len { h ^= (b[off] as u64).wrapping_mul(P5); h = h.rotate_left(11).wrapping_mul(P1); off += 1; }
    xx_fmix(h) as i64
}

fn double_bits(d: f64) -> i64 { (if d == 0.0 { 0.0f64 } else { d }).to_bits() as i64 }

/// `hash(a, b, ..)`: Murmur3 chained over the arguments starting from seed 42; NULLs leave the running hash alone.
pub fn spark_hash(vals: &[HashVal]) -> i32 {
    fn step(v: &HashVal, seed: i32) -> i32 {
        match v {
            HashVal::Null => seed,
            HashVal::Int32(i) => m3_int(*i, seed),
            HashVal::Int64(l) => m3_long(*l, seed),
            HashVal::Double(d) => m3_long(double_bits(*d), seed),
            HashVal::Bool(b) => m3_int(*b as i32, seed),
            HashVal::Bytes(b) => m3_bytes(b, seed),
            HashVal::Seq(items) => items.iter().fold(seed, |s, x| step(x, s)),
        }
    }
    vals.iter().fold(42, |s, v| step(v, s))
}

/// `xxhash64(a, b, ..)`: XXH64 chained over the arguments starting from seed 42.
pub fn spark_xxhash64(vals: &[HashVal]) -> i64 {
    fn step(v: &HashVal, seed: i64) -> i64 {
        match v {
            HashVal::Null => seed,
            HashVal::Int32(i) => xx_int(*i, seed),
            HashVal::Int64(l) => xx_long(*l, seed),
            HashVal::Double(d) => xx_long(double_bits(*d), seed),
            HashVal::Bool(b) => xx_int(*b as i32, seed),
            HashVal::Bytes(b) => xx_bytes(b, seed),
            HashVal::Seq(items) => items.iter().fold(seed, |s, x| step(x, s)),
        }
    }
    vals.iter().fold(42, |s, v| step(v, s))
}

/// Spark's `soundex`: a non-letter first character returns the input unchanged.
pub fn soundex(s: &str) -> String {
    const MAP: &[u8; 26] = b"01230127022455012623017202";
    let b = s.as_bytes();
    if b.is_empty() { return String::new(); }
    let up = |c: u8| if c.is_ascii_lowercase() { c - 32 } else { c };
    let first = up(b[0]);
    if !first.is_ascii_uppercase() { return s.to_string(); }
    let mut sx = [b'0'; 4];
    sx[0] = first;
    let mut n = 1;
    let mut last = MAP[(first - b'A') as usize];
    for &c in &b[1..] {
        let c = up(c);
        if !c.is_ascii_uppercase() { last = b'0'; continue; }
        let code = MAP[(c - b'A') as usize];
        if code == b'7' { continue; }
        if code != b'0' && code != last { sx[n] = code; n += 1; if n > 3 { break; } }
        last = code;
    }
    String::from_utf8_lossy(&sx).to_string()
}
