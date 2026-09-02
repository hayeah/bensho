//! splitmix64 and the two per-round schedules: groups within the round,
//! cells within a group.

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

    /// A Fisher-Yates permutation of `0..n` drawn from this stream.
    pub fn permutation(&mut self, n: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = self.below(i as u64 + 1) as usize;
            order.swap(i, j);
        }
        order
    }
}

/// The group order for one round: a permutation of `0..groups` driven by
/// `SplitMix(seed ^ round)`. A pure function of its arguments, so round k
/// never depends on round j or on how many rounds the run has.
pub fn schedule(seed: u64, round: u32, groups: usize) -> Vec<usize> {
    SplitMix::new(seed ^ round as u64).permutation(groups)
}

/// The cell order within one group for one round: a permutation of
/// `0..cells` driven by `SplitMix(seed ^ round ^ ((group_index + 1) << 32))`,
/// `group_index` the group's registration index. Adding a group at the end
/// changes no earlier group's order; the `+ 1` keeps the first group's stream
/// distinct from the group-level one.
pub fn schedule_in_group(seed: u64, round: u32, group_index: usize, cells: usize) -> Vec<usize> {
    SplitMix::new(seed ^ round as u64 ^ ((group_index as u64 + 1) << 32)).permutation(cells)
}
