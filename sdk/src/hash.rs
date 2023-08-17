//! Hashing with the [SHA-256] hash function, and a general [`Hash`] type.
//!
//! [SHA-256]: https://en.wikipedia.org/wiki/SHA-2
//! [`Hash`]: struct@Hash

use rand::Rng;
pub use solana_program::hash::*;

/// Random hash value for tests and benchmarks.
#[deprecated(since = "1.17.0", note = "Please use `new_with_thread_rng`")]
#[cfg(feature = "full")]
pub fn new_rand<R: ?Sized>(rng: &mut R) -> Hash
where
    R: rand0_7::Rng,
{
    let mut buf = [0u8; HASH_BYTES];
    rng.fill(&mut buf);
    Hash::new(&buf)
}

/// Random hash value for tests and benchmarks.
#[cfg(feature = "full")]
pub fn new_with_thread_rng() -> Hash {
    let mut rng = rand::thread_rng();
    let mut buf = [0u8; HASH_BYTES];
    rng.fill(&mut buf);
    Hash::new(&buf)
}
