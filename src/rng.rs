//! splitmix64 and the per-round schedule.

/// splitmix64: the zero-dependency PRNG. Output is a hash of the state, so
/// nearby seeds give unrelated streams.
#[derive(Clone, Debug)]
pub struct SplitMix(pub u64);

impl SplitMix {
    pub fn new(seed: u64) -> SplitMix {
        SplitMix(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value in `0..n`. `n` must be nonzero.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// The cell order for one round: a Fisher-Yates permutation of `0..cells`
/// driven by `SplitMix(seed ^ round)`. A pure function of its arguments, so
/// round k never depends on round j or on how many rounds the run has.
pub fn schedule(seed: u64, round: u32, cells: usize) -> Vec<usize> {
    let mut rng = SplitMix::new(seed ^ round as u64);
    let mut order: Vec<usize> = (0..cells).collect();
    for i in (1..cells).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        order.swap(i, j);
    }
    order
}
