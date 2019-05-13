//! The `Poh` module provhashes an object for generating a Proof of History.
//! It records Hashes items on behalf of its users.
use solana_sdk::hash::{hash, hashv, Hash};

pub struct Poh {
    pub hash: Hash,
    num_hashes: u64,
    pub tick_height: u64,
}

#[derive(Debug)]
pub struct PohEntry {
    pub tick_height: u64,
    pub num_hashes: u64,
    pub hash: Hash,
    pub mixin: Option<Hash>,
}

impl Poh {
    pub fn new(hash: Hash, tick_height: u64) -> Self {
        Poh {
            num_hashes: 0,
            hash,
            tick_height,
        }
    }

    pub fn hash(&mut self) {
        self.hash = hash(&self.hash.as_ref());
        self.num_hashes += 1;
    }

    pub fn record(&mut self, mixin: Hash) -> PohEntry {
        self.hash = hashv(&[&self.hash.as_ref(), &mixin.as_ref()]);

        let num_hashes = self.num_hashes + 1;
        self.num_hashes = 0;

        PohEntry {
            tick_height: self.tick_height,
            num_hashes,
            hash: self.hash,
            mixin: Some(mixin),
        }
    }

    // emissions of Ticks (i.e. PohEntries without a mixin) allows
    // validators to parallelize the work of catching up
    pub fn tick(&mut self) -> PohEntry {
        self.hash();

        let num_hashes = self.num_hashes;
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
pub fn verify(initial_hash: Hash, entries: &[PohEntry]) -> bool {
    let mut current_hash = initial_hash;

    for entry in entries {
        assert!(entry.num_hashes != 0);

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

#[cfg(test)]
mod tests {
    use crate::poh::{verify, Poh, PohEntry};
    use solana_sdk::hash::{hash, hashv, Hash};

    #[test]
    fn test_poh_verify() {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        let two = hash(&one.as_ref());
        let one_with_zero = hashv(&[&zero.as_ref(), &zero.as_ref()]);

        let mut poh = Poh::new(zero, 0);
        assert_eq!(
            verify(
                zero,
                &[poh.tick(), poh.record(zero), poh.record(zero), poh.tick(),],
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

}
