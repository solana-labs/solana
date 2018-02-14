//! The `tick` crate provides the foundational data structures for Proof of History

pub struct Tick {
    pub hash: u64,
    pub n: u64,
    pub data: Option<u64>,
}

impl Tick {
    /// Creates a Tick from by hashing 'seed' 'n' times.
    ///
    /// ```
    /// use loomination::tick::Tick;
    /// assert_eq!(Tick::new(0, 1).n, 1)
    /// ```
    pub fn new(seed: u64, n: u64) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let data = None;
        let mut hash = seed;
        let mut hasher = DefaultHasher::new();
        for _ in 0..n {
            hash.hash(&mut hasher);
            hash = hasher.finish();
        }
        Tick { hash, n, data }
    }
    /// Verifies self.hash is the result of hashing a 'seed' 'self.n' times.
    ///
    /// ```
    /// use loomination::tick::Tick;
    /// assert!(Tick::new(0, 0).verify(0)); // base case
    /// assert!(!Tick::new(0, 0).verify(1)); // base case, bad
    /// assert!(Tick::new(0, 1).verify(0)); // inductive case
    /// assert!(!Tick::new(0, 1).verify(1)); // inductive case, bad
    /// ```
    pub fn verify(self: &Self, seed: u64) -> bool {
        self.hash == Self::new(seed, self.n).hash
    }
}

/// Verifies the hashes and counts of a slice of ticks are all consistent.
///
/// ```
/// use loomination::tick::{verify_slice, Tick};
/// assert!(verify_slice(&vec![], 0)); // base case
/// assert!(verify_slice(&vec![Tick::new(0, 0)], 0)); // singleton case 1
/// assert!(!verify_slice(&vec![Tick::new(0, 0)], 1)); // singleton case 2, bad
/// assert!(verify_slice(&vec![Tick::new(0, 0), Tick::new(0, 0)], 0)); // lazy inductive case
/// assert!(!verify_slice(&vec![Tick::new(0, 0), Tick::new(1, 0)], 0)); // lazy inductive case, bad
/// ```
pub fn verify_slice(ticks: &[Tick], seed: u64) -> bool {
    use rayon::prelude::*;
    let genesis = [Tick::new(seed, 0)];
    let tick_pairs = genesis.par_iter().chain(ticks).zip(ticks);
    tick_pairs.all(|(x, x1)| x1.verify(x.hash))
}

/// Verifies the hashes and ticks serially. Exists only for reference.
pub fn verify_slice_seq(ticks: &[Tick], seed: u64) -> bool {
    let genesis = [Tick::new(seed, 0)];
    let tick_pairs = genesis.iter().chain(ticks).zip(ticks);
    tick_pairs.into_iter().all(|(x, x1)| x1.verify(x.hash))
}

/// Create a vector of Ticks of length 'len' from 'seed' hash and 'hashes_per_tick'.
pub fn create_ticks(seed: u64, hashes_per_tick: u64, len: usize) -> Vec<Tick> {
    use itertools::unfold;
    let mut ticks_iter = unfold(seed, |state| {
        let tick = Tick::new(*state, hashes_per_tick);
        *state = tick.hash;
        return Some(tick);
    });
    ticks_iter.by_ref().take(len).collect()
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use tick;

    #[bench]
    fn tick_bench(bencher: &mut Bencher) {
        let seed = 0;
        let ticks = tick::create_ticks(seed, 100_000, 4);
        bencher.iter(|| {
            assert!(tick::verify_slice(&ticks, seed));
        });
    }
}
