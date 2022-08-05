//! keccak permutation function used for the sponge construction SHA-3 and Keccak256.
//! Not to be confused with keccak-256, the hash function used by ethereum.
//! If you're looking for the hash function, see the [`keccak`] module of this crate.
//!
//! [keccak]: https://keccak.team/keccak.html

pub const KECCAK_F1600_STATE_WIDTH: usize = 25;

/// A single invocation of the keccak permutation, performed in-place.
pub fn keccak_permutation(state: &mut [u64; KECCAK_F1600_STATE_WIDTH]) {
    #[cfg(not(target_os = "solana"))]
    {
        use keccak::f1600 as keccak;
        keccak(state);
    }

    #[cfg(target_os = "solana")]
    {
        unsafe {
            crate::syscalls::sol_keccak_permutation(state as *mut _ as *mut u64);
        }
    }
}
