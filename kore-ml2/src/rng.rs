//! Minimal xorshift64 PRNG — no external dependencies.

#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Avoid zero state
        let s = if seed == 0 { 6364136223846793005 } else { seed };
        Rng { state: s }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Uniform [0, max)
    #[inline]
    pub fn next_usize(&mut self, max: usize) -> usize {
        (self.next_u64() % max as u64) as usize
    }

    /// Uniform f64 in [0, 1)
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Bootstrap sample: `k` draws with replacement from `0..n`
    pub fn bootstrap(&mut self, n: usize, k: usize) -> Vec<usize> {
        (0..k).map(|_| self.next_usize(n)).collect()
    }

    /// Sample `k` indices from `0..n` without replacement (Fisher–Yates prefix)
    pub fn sample_without_replacement(&mut self, n: usize, k: usize) -> Vec<usize> {
        let k = k.min(n);
        let mut indices: Vec<usize> = (0..n).collect();
        for i in 0..k {
            let j = i + self.next_usize(n - i);
            indices.swap(i, j);
        }
        indices[..k].to_vec()
    }
}
