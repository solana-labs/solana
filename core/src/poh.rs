//! The `Poh` module provides an object for generating a Proof of History.
use solana_sdk::hash::{hash, hashv, Hash};

pub struct Poh {
    pub hash: Hash,
    num_hashes: u64,
    pub tick_height: u64,
    hashes_per_tick: u64,
    pub remaining_hashes: u64,
}

#[derive(Debug)]
pub struct PohEntry {
    pub tick_height: u64,
    pub num_hashes: u64,
    pub hash: Hash,
    pub mixin: Option<Hash>,
}

impl Poh {
    pub fn new(hash: Hash, tick_height: u64, hashes_per_tick: Option<u64>) -> Self {
        let hashes_per_tick = hashes_per_tick.unwrap_or(std::u64::MAX);
        Poh {
            num_hashes: 0,
            remaining_hashes: hashes_per_tick,
            hashes_per_tick,
            hash,
            tick_height,
        }
    }

    pub fn hash(&mut self, num_hashes: u64) {
        for _ in 0..num_hashes {
            self.hash = hash(&self.hash.as_ref());
        }
        self.num_hashes += num_hashes;
        self.remaining_hashes -= num_hashes;
    }

    pub fn record(&mut self, mixin: Hash) -> Option<PohEntry> {
        let num_hashes = self.num_hashes + 1;
        if self.remaining_hashes <= 1 {
            return None;
        }
        self.remaining_hashes -= 1;
        self.hash = hashv(&[&self.hash.as_ref(), &mixin.as_ref()]);
        self.num_hashes = 0;

        Some(PohEntry {
            tick_height: self.tick_height,
            num_hashes,
            hash: self.hash,
            mixin: Some(mixin),
        })
    }

    // emissions of Ticks (i.e. PohEntries without a mixin) allows
    // validators to parallelize the work of catching up
    pub fn tick(&mut self) -> PohEntry {
        self.hash(1);

        let num_hashes = self.num_hashes;
        if self.hashes_per_tick < std::u64::MAX {
            assert_eq!(self.remaining_hashes, 0);
        }
        self.remaining_hashes = self.hashes_per_tick;
        self.num_hashes = 0;
        self.tick_height += 1;

        PohEntry {
            tick_height: self.tick_height,
            num_hashes,
            hash: self.hash,
            mixin: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::poh::{Poh, PohEntry};
    use solana_sdk::hash::{hash, hashv, Hash};

    fn verify(initial_hash: Hash, entries: &[PohEntry]) -> bool {
        let mut current_hash = initial_hash;

        for entry in entries {
            assert_ne!(entry.num_hashes, 0);

            for _ in 1..entry.num_hashes {
                current_hash = hash(&current_hash.as_ref());
            }
            current_hash = match entry.mixin {
                Some(mixin) => hashv(&[&current_hash.as_ref(), &mixin.as_ref()]),
                None => hash(&current_hash.as_ref()),
            };
            if current_hash != entry.hash {
                return false;
            }
        }

        true
    }

    #[test]
    fn test_poh_verify() {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        let two = hash(&one.as_ref());
        let one_with_zero = hashv(&[&zero.as_ref(), &zero.as_ref()]);

        let mut poh = Poh::new(zero, 0, None);
        assert_eq!(
            verify(
                zero,
                &[
                    poh.tick(),
                    poh.record(zero).unwrap(),
                    poh.record(zero).unwrap(),
                    poh.tick(),
                ],
            ),
            true
        );

        assert_eq!(
            verify(
                zero,
                &[PohEntry {
                    tick_height: 0,
                    num_hashes: 1,
                    hash: one,
                    mixin: None,
                }],
            ),
            true
        );
        assert_eq!(
            verify(
                zero,
                &[PohEntry {
                    tick_height: 0,
                    num_hashes: 2,
                    hash: two,
                    mixin: None,
                }]
            ),
            true
        );

        assert_eq!(
            verify(
                zero,
                &[PohEntry {
                    tick_height: 0,
                    num_hashes: 1,
                    hash: one_with_zero,
                    mixin: Some(zero),
                }]
            ),
            true
        );
        assert_eq!(
            verify(
                zero,
                &[PohEntry {
                    tick_height: 0,
                    num_hashes: 1,
                    hash: zero,
                    mixin: None
                }]
            ),
            false
        );

        assert_eq!(
            verify(
                zero,
                &[
                    PohEntry {
                        tick_height: 0,
                        num_hashes: 1,
                        hash: one_with_zero,
                        mixin: Some(zero),
                    },
                    PohEntry {
                        tick_height: 0,
                        num_hashes: 1,
                        hash: hash(&one_with_zero.as_ref()),
                        mixin: None
                    },
                ]
            ),
            true
        );
    }

    #[test]
    #[should_panic]
    fn test_poh_verify_assert() {
        verify(
            Hash::default(),
            &[PohEntry {
                tick_height: 0,
                num_hashes: 0,
                hash: Hash::default(),
                mixin: None,
            }],
        );
    }

    #[test]
    #[should_panic]
    fn test_poh_too_many_hashes() {
        let mut poh = Poh::new(Hash::default(), 0, Some(1));
        poh.hash(1);
        assert_eq!(poh.remaining_hashes, 0);
        poh.tick(); // <-- This adds a second hash, exceeding hashes_per_tick.  Panic
    }

    #[test]
    #[should_panic]
    fn test_poh_too_few_hashes() {
        let mut poh = Poh::new(Hash::default(), 0, Some(2));
        assert_eq!(poh.remaining_hashes, 2);
        poh.tick(); // <-- 2 hashes_per_tick are required, but there's only one.  Panic
    }

    #[test]
    fn test_poh_record_not_permitted() {
        let mut poh = Poh::new(Hash::default(), 0, Some(10));
        poh.hash(9);
        assert_eq!(poh.remaining_hashes, 1);
        assert!(poh.record(Hash::default()).is_none()); // <-- record() rejected to avoid exceeding hashes_per_tick
        poh.tick();
    }
}
