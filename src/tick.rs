//! The `tick` crate provides the foundational data structures for Proof of History

pub struct Tick {
    pub hash: u64,
    pub n: u64,
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

        let mut hash = seed;
        let mut hasher = DefaultHasher::new();
        for _ in 0..n {
            hash.hash(&mut hasher);
            hash = hasher.finish();
        }
        Tick { hash, n }
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
    // Verify the first item against the seed.
    match ticks.first() {
        None => return true,
        Some(x) if !x.verify(seed) => return false,
        Some(_) => (),
    }
    // Verify all follow items using the hash in the item before it.
    let mut tick_pairs = ticks.iter().zip(ticks.iter().skip(1));
    tick_pairs.all(|(x, x1)| x1.verify(x.hash))
}
