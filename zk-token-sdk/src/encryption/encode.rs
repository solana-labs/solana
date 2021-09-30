use core::ops::{Add, Neg, Sub};

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT as G;
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::Identity;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

const TWO14: u32 = 16384; // 2^14
const TWO16: u32 = 65536; // 2^16
const TWO18: u32 = 262144; // 2^18

/// Type that captures a discrete log challenge.
///
/// The goal of discrete log is to find x such that x * generator = target.
#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
pub struct DiscreteLogInstance {
    /// Generator point for discrete log
    pub generator: RistrettoPoint,
    /// Target point for discrete log
    pub target: RistrettoPoint,
}

/// Solves the discrete log instance using a 18/14 bit offline/online split
impl DiscreteLogInstance {
    /// Solves the discrete log problem under the assumption that the solution
    /// is a 32-bit number.
    pub fn decode_u32(self) -> Option<u32> {
        let hashmap = DiscreteLogInstance::decode_u32_precomputation(self.generator);
        self.decode_u32_online(&hashmap)
    }

    pub fn decode_u32_precomputation(generator: RistrettoPoint) -> HashMap<HashableRistretto, u32> {
        let mut hashmap = HashMap::new();

        let two16_scalar = Scalar::from(TWO16);
        let identity = HashableRistretto(RistrettoPoint::identity()); // 0 * G
        let generator = HashableRistretto(two16_scalar * generator); // 2^16 * G

        // iterator for 2^16*0G , 2^16*1G, 2^16*2G, ...
        let ristretto_iter = RistrettoIterator::new(identity, generator);
        ristretto_iter.zip(0..TWO16).for_each(|(elem, x_hi)| {
            hashmap.insert(elem, x_hi);
        });

        hashmap
    }

    pub fn decode_u32_online(self, hashmap: &HashMap<HashableRistretto, u32>) -> Option<u32> {
        // iterator for 0G, -1G, -2G, ...
        let ristretto_iter = RistrettoIterator::new(
            HashableRistretto(self.target),
            HashableRistretto(-self.generator),
        );

        let mut decoded = None;
        ristretto_iter.zip(0..TWO16).for_each(|(elem, x_lo)| {
            if hashmap.contains_key(&elem) {
                let x_hi = hashmap[&elem];
                decoded = Some(x_lo + TWO16 * x_hi);
            }
        });
        decoded
    }
}

/// Solves the discrete log instance using a 18/14 bit offline/online split
impl DiscreteLogInstance {
    /// Solves the discrete log problem under the assumption that the solution
    /// is a 32-bit number.
    pub fn decode_u32_alt(self) -> Option<u32> {
        let hashmap = DiscreteLogInstance::decode_u32_precomputation_alt(self.generator);
        self.decode_u32_online_alt(&hashmap)
    }

    pub fn decode_u32_precomputation_alt(
        generator: RistrettoPoint,
    ) -> HashMap<HashableRistretto, u32> {
        let mut hashmap = HashMap::new();

        let two12_scalar = Scalar::from(TWO14);
        let identity = HashableRistretto(RistrettoPoint::identity()); // 0 * G
        let generator = HashableRistretto(two12_scalar * generator); // 2^12 * G

        // iterator for 2^12*0G , 2^12*1G, 2^12*2G, ...
        let ristretto_iter = RistrettoIterator::new(identity, generator);
        ristretto_iter.zip(0..TWO18).for_each(|(elem, x_hi)| {
            hashmap.insert(elem, x_hi);
        });

        hashmap
    }

    pub fn decode_u32_online_alt(self, hashmap: &HashMap<HashableRistretto, u32>) -> Option<u32> {
        // iterator for 0G, -1G, -2G, ...
        let ristretto_iter = RistrettoIterator::new(
            HashableRistretto(self.target),
            HashableRistretto(-self.generator),
        );

        let mut decoded = None;
        ristretto_iter.zip(0..TWO14).for_each(|(elem, x_lo)| {
            if hashmap.contains_key(&elem) {
                let x_hi = hashmap[&elem];
                decoded = Some(x_lo + TWO14 * x_hi);
            }
        });
        decoded
    }
}

/// Type wrapper for RistrettoPoint that implements the Hash trait
#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq)]
pub struct HashableRistretto(pub RistrettoPoint);

impl HashableRistretto {
    pub fn encode<T: Into<Scalar>>(amount: T) -> Self {
        HashableRistretto(amount.into() * G)
    }
}

impl Hash for HashableRistretto {
    fn hash<H: Hasher>(&self, state: &mut H) {
        bincode::serialize(self).unwrap().hash(state);
    }
}

impl PartialEq for HashableRistretto {
    fn eq(&self, other: &Self) -> bool {
        self == other
    }
}

/// HashableRistretto iterator.
///
/// Given an initial point X and a stepping point P, the iterator iterates through
/// X + 0*P, X + 1*P, X + 2*P, X + 3*P, ...
struct RistrettoIterator {
    pub curr: HashableRistretto,
    pub step: HashableRistretto,
}

impl RistrettoIterator {
    fn new(curr: HashableRistretto, step: HashableRistretto) -> Self {
        RistrettoIterator { curr, step }
    }
}

impl Iterator for RistrettoIterator {
    type Item = HashableRistretto;

    fn next(&mut self) -> Option<Self::Item> {
        let r = self.curr;
        self.curr = self.curr + self.step;
        Some(r)
    }
}

impl<'a, 'b> Add<&'b HashableRistretto> for &'a HashableRistretto {
    type Output = HashableRistretto;

    fn add(self, other: &HashableRistretto) -> HashableRistretto {
        HashableRistretto(self.0 + other.0)
    }
}

define_add_variants!(
    LHS = HashableRistretto,
    RHS = HashableRistretto,
    Output = HashableRistretto
);

impl<'a, 'b> Sub<&'b HashableRistretto> for &'a HashableRistretto {
    type Output = HashableRistretto;

    fn sub(self, other: &HashableRistretto) -> HashableRistretto {
        HashableRistretto(self.0 - other.0)
    }
}

define_sub_variants!(
    LHS = HashableRistretto,
    RHS = HashableRistretto,
    Output = HashableRistretto
);

impl Neg for HashableRistretto {
    type Output = HashableRistretto;

    fn neg(self) -> HashableRistretto {
        HashableRistretto(-self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Discrete log test for 16/16 split
    ///
    /// Very informal measurements on my machine:
    ///   - 8 sec for precomputation
    ///   - 3 sec for online computation
    #[test]
    #[ignore]
    fn test_decode_correctness() {
        let amount: u32 = 65545;

        let instance = DiscreteLogInstance {
            generator: G,
            target: Scalar::from(amount) * G,
        };

        // Very informal measurements for now
        let start_precomputation = time::precise_time_s();
        let precomputed_hashmap = DiscreteLogInstance::decode_u32_precomputation(G);
        let end_precomputation = time::precise_time_s();

        let start_online = time::precise_time_s();
        let computed_amount = instance.decode_u32_online(&precomputed_hashmap).unwrap();
        let end_online = time::precise_time_s();

        assert_eq!(amount, computed_amount);

        println!(
            "16/16 Split precomputation: {:?} sec",
            end_precomputation - start_precomputation
        );
        println!(
            "16/16 Split online computation: {:?} sec",
            end_online - start_online
        );
    }

    /// Discrete log test for 18/14 split
    ///
    /// Very informal measurements on my machine:
    ///   - 33 sec for precomputation
    ///   - 0.8 sec for online computation
    #[test]
    #[ignore]
    fn test_decode_alt_correctness() {
        let amount: u32 = 65545;

        let instance = DiscreteLogInstance {
            generator: G,
            target: Scalar::from(amount) * G,
        };

        // Very informal measurements for now
        let start_precomputation = time::precise_time_s();
        let precomputed_hashmap = DiscreteLogInstance::decode_u32_precomputation_alt(G);
        let end_precomputation = time::precise_time_s();

        let start_online = time::precise_time_s();
        let computed_amount = instance
            .decode_u32_online_alt(&precomputed_hashmap)
            .unwrap();
        let end_online = time::precise_time_s();

        assert_eq!(amount, computed_amount);

        println!(
            "18/14 Split precomputation: {:?} sec",
            end_precomputation - start_precomputation
        );
        println!(
            "18/14 Split online computation: {:?} sec",
            end_online - start_online
        );
    }
}
