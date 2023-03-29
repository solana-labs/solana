//! Hashing with the [SHA-256] hash function, and a general [`Hash`] type.
//!
//! [SHA-256]: https://en.wikipedia.org/wiki/SHA-2
//! [`Hash`]: struct@Hash

pub use solana_program::hash::*;

/// Random hash value for tests and benchmarks.
#[cfg(feature = "full")]
pub fn new_rand<R: ?Sized>(rng: &mut R) -> Hash
where
    R: rand::Rng,
{
    let mut buf = [0u8; HASH_BYTES];
    rng.fill(&mut buf);
    Hash::new(&buf)
}
