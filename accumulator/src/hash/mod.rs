//! This module wraps `blake2b_rfc` into a convenient hashing interface (`GeneralHasher`) and
//! exports the generalized `hash` function. Also exported is `hash_to_prime`, which works by
//! repeatedly `hash`ing a value together with an incrementing nonce until the output is prime.
use crate::uint::u256;
use rug::integer::Order;
use rug::Integer;
use std::hash::{Hash, Hasher};

mod blake2b;
pub use blake2b::Blake2b;
pub mod primality;

/// Like `std::hash::Hasher`, but general over output type.
pub trait GeneralHasher: Hasher {
    /// The associated output type of the Hasher.
    type Output;
    /// Similar to `Hasher::finish`, but consumes `self`.
    fn finalize(self) -> Self::Output;
}

// Note: We explicitly pass in the hasher constructor so we don't have to specify its type via
// generics. Rust has poor support for type applications, so if we wanted to pass `H` at the
// type-level, we'd need to fully specify `T` as well, which is a pain in the ass.
//
// Instead of writing:
// `hash::<Blake2b, (&G::Elem, &BigUint, &G::Elem)>(&(base, exp, result))`
//
// This lets us write:
// `hash(&Blake2b::default, &(base, exp, result))`

/// Hash using the general Hasher.
///
/// This function takes in the hash constructor as an argument for convenience.
pub fn hash<H: GeneralHasher, T: Hash + ?Sized>(new_hasher: &dyn Fn() -> H, t: &T) -> H::Output {
    let mut h = new_hasher();
    t.hash(&mut h);
    h.finalize()
}

/// Calls `hash` with a Blake2b hasher.
pub fn blake2b<T: Hash + ?Sized>(t: &T) -> Integer {
    Integer::from_digits(&hash(&Blake2b::default, t), Order::Msf)
}

/// Hashes `t` to an odd prime.
///
/// Uses `Blake2b` as the hash function, and hashes with a counter until a prime is found via
/// probabilistic primality checking.
///
/// This function is optimized for 256-bit integers.
#[allow(clippy::module_name_repetitions)]
pub fn hash_to_prime<T: Hash + ?Sized>(t: &T) -> Integer {
    let mut counter = 0_u64;
    loop {
        let mut hash = hash(&Blake2b::default, &(t, counter));
        // Make the candidate prime odd. This gives ~7% performance gain on a 2018 Macbook Pro.
        hash[0] |= 1;
        let candidate_prime = u256(hash);
        if primality::is_prob_prime(&candidate_prime) {
            return Integer::from(candidate_prime);
        }
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake2() {
        let data = b"martian cyborg gerbil attack";
        hash(&Blake2b::default, data);
    }

    #[test]
    fn test_() {
        let b_1 = "boom i got ur boyfriend";
        let b_2 = "boom i got ur boyfriene";
        assert_ne!(b_1, b_2);
        let h_1 = hash_to_prime(b_1);
        let h_2 = hash_to_prime(b_2);
        assert_ne!(h_1, h_2);
        let mut digits1 = [0; 4];
        h_1.write_digits(&mut digits1, Order::Lsf);
        assert!(primality::is_prob_prime(&u256(digits1)));
        let mut digits2 = [0; 4];
        h_2.write_digits(&mut digits2, Order::Lsf);
        assert!(primality::is_prob_prime(&u256(digits2)));
    }
}
