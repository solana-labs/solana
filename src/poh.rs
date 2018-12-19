//! The `Poh` module provides an object for generating a Proof of History.
//! It records Hashes items on behalf of its users.
use solana_sdk::hash::{hash, hashv, Hash};

pub struct Poh {
    id: Hash,
    num_hashes: u64,
    pub tick_height: u64,
}

#[derive(Debug)]
pub struct PohEntry {
    pub tick_height: u64,
    pub num_hashes: u64,
    pub id: Hash,
    pub mixin: Option<Hash>,
}

impl Poh {
    pub fn new(id: Hash, tick_height: u64) -> Self {
        Poh {
            num_hashes: 0,
            id,
            tick_height,
        }
    }

    pub fn hash(&mut self) {
        self.id = hash(&self.id.as_ref());
        self.num_hashes += 1;
    }

    pub fn record(&mut self, mixin: Hash) -> PohEntry {
        self.id = hashv(&[&self.id.as_ref(), &mixin.as_ref()]);

        let num_hashes = self.num_hashes + 1;
        self.num_hashes = 0;

        PohEntry {
            tick_height: self.tick_height,
            num_hashes,
            id: self.id,
            mixin: Some(mixin),
        }
    }

    // emissions of Ticks (i.e. PohEntries without a mixin) allows
    //  validators to parallelize the work of catching up
    pub fn tick(&mut self) -> PohEntry {
        self.hash();

        let num_hashes = self.num_hashes;
        self.num_hashes = 0;
        self.tick_height += 1;

        PohEntry {
            tick_height: self.tick_height,
            num_hashes,
            id: self.id,
            mixin: None,
        }
    }
}

#[cfg(test)]
pub fn verify(initial: Hash, entries: &[PohEntry]) -> bool {
    let mut id = initial;

    for entry in entries {
        assert!(entry.num_hashes != 0);

        for _ in 1..entry.num_hashes {
            id = hash(&id.as_ref());
        }
        id = match entry.mixin {
            Some(mixin) => hashv(&[&id.as_ref(), &mixin.as_ref()]),
            None => hash(&id.as_ref()),
        };
        if id != entry.id {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use crate::poh::{self, PohEntry};
    use solana_sdk::hash::{hash, hashv, Hash};
    #[test]
    fn test_poh_verify() {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        let two = hash(&one.as_ref());
        let one_with_zero = hashv(&[&zero.as_ref(), &zero.as_ref()]);

        assert_eq!(
            poh::verify(
                zero,
                &[PohEntry {
                    tick_height: 0,
                    num_hashes: 1,
                    id: one,
                    mixin: None,
                }],
            ),
            true
        );
        assert_eq!(
            poh::verify(
                zero,
                &[PohEntry {
                    tick_height: 0,
                    num_hashes: 2,
                    id: two,
                    mixin: None,
                }]
            ),
            true
        );

        assert_eq!(
            poh::verify(
                zero,
                &[PohEntry {
                    tick_height: 0,
                    num_hashes: 1,
                    id: one_with_zero,
                    mixin: Some(zero),
                }]
            ),
            true
        );
        assert_eq!(
            poh::verify(
                zero,
                &[PohEntry {
                    tick_height: 0,
                    num_hashes: 1,
                    id: zero,
                    mixin: None
                }]
            ),
            false
        );

        assert_eq!(
            poh::verify(
                zero,
                &[
                    PohEntry {
                        tick_height: 0,
                        num_hashes: 1,
                        id: one_with_zero,
                        mixin: Some(zero),
                    },
                    PohEntry {
                        tick_height: 0,
                        num_hashes: 1,
                        id: hash(&one_with_zero.as_ref()),
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
        poh::verify(
            Hash::default(),
            &[PohEntry {
                tick_height: 0,
                num_hashes: 0,
                id: Hash::default(),
                mixin: None,
            }],
        );
    }

}
