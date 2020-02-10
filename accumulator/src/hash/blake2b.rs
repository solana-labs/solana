//! `GeneralHasher` interface for `blake2_rfc`.
use super::GeneralHasher;
use blake2_rfc::blake2b::Blake2b as Blake2b_;
use std::hash::Hasher;

/// Thin wrapper around `Blake2b` from `blake2_rfc`.
pub struct Blake2b(pub Blake2b_);

impl Default for Blake2b {
    fn default() -> Self {
        // 32 bytes = 256 bits
        Self(Blake2b_::new(32))
    }
}

impl Hasher for Blake2b {
    /// We could return a truncated hash but it's easier just to not use this fn for now.
    fn finish(&self) -> u64 {
        panic!("Don't use! Prefer finalize(self).")
    }
    fn write(&mut self, bytes: &[u8]) {
        Blake2b_::update(&mut self.0, bytes)
    }
}

impl GeneralHasher for Blake2b {
    type Output = [u8; 32];
    fn finalize(self) -> Self::Output {
        let res = self.0.finalize();
        *array_ref![res.as_bytes(), 0, 32]
    }
}
