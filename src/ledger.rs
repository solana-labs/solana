//! The `ledger` module provides functions for parallel verification of the
//! Proof of History ledger.

use entry::{next_tick, Entry};
use hash::Hash;
use rayon::prelude::*;

pub trait Block {
    /// Verifies the hashes and counts of a slice of events are all consistent.
    fn verify(&self, start_hash: &Hash) -> bool;
}

impl Block for [Entry] {
    fn verify(&self, start_hash: &Hash) -> bool {
        let genesis = [Entry::new_tick(0, start_hash)];
        let entry_pairs = genesis.par_iter().chain(self).zip(self);
        entry_pairs.all(|(x0, x1)| x1.verify(&x0.id))
    }
}

/// Create a vector of Ticks of length `len` from `start_hash` hash and `num_hashes`.
pub fn next_ticks(start_hash: &Hash, num_hashes: u64, len: usize) -> Vec<Entry> {
    let mut id = *start_hash;
    let mut ticks = vec![];
    for _ in 0..len {
        let entry = next_tick(&id, num_hashes);
        id = entry.id;
        ticks.push(entry);
    }
    ticks
}

#[cfg(test)]
mod tests {
    use super::*;
    use hash::hash;

    #[test]
    fn test_verify_slice() {
        let zero = Hash::default();
        let one = hash(&zero);
        assert!(vec![][..].verify(&zero)); // base case
        assert!(vec![Entry::new_tick(0, &zero)][..].verify(&zero)); // singleton case 1
        assert!(!vec![Entry::new_tick(0, &zero)][..].verify(&one)); // singleton case 2, bad
        assert!(next_ticks(&zero, 0, 2)[..].verify(&zero)); // inductive step

        let mut bad_ticks = next_ticks(&zero, 0, 2);
        bad_ticks[1].id = one;
        assert!(!bad_ticks.verify(&zero)); // inductive step, bad
    }
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use ledger::*;

    #[bench]
    fn event_bench(bencher: &mut Bencher) {
        let start_hash = Hash::default();
        let entries = next_ticks(&start_hash, 10_000, 8);
        bencher.iter(|| {
            assert!(entries.verify(&start_hash));
        });
    }
}
