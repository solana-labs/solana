#![cfg(not(target_arch = "bpf"))]

use {
    curve25519_dalek::{ristretto::RistrettoPoint, scalar::Scalar, traits::Identity},
    serde::{Deserialize, Serialize},
    std::collections::HashMap,
};

const TWO15: u32 = 32768;
const TWO14: u32 = 16384; // 2^14
                          // const TWO16: u32 = 65536; // 2^16
const TWO18: u32 = 262144; // 2^18

/// Type that captures a discrete log challenge.
///
/// The goal of discrete log is to find x such that x * generator = target.
#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
pub struct DiscreteLog {
    /// Generator point for discrete log
    pub generator: RistrettoPoint,
    /// Target point for discrete log
    pub target: RistrettoPoint,
}

#[derive(Serialize, Deserialize, Default)]
pub struct DecodeU32Precomputation(HashMap<[u8; 32], u32>);

/// Builds a HashMap of 2^18 elements
fn decode_u32_precomputation(generator: RistrettoPoint) -> DecodeU32Precomputation {
    let mut hashmap = HashMap::new();

    let two12_scalar = Scalar::from(TWO14);
    let identity = RistrettoPoint::identity(); // 0 * G
    let generator = two12_scalar * generator; // 2^12 * G

    // iterator for 2^12*0G , 2^12*1G, 2^12*2G, ...
    let ristretto_iter = RistrettoIterator::new(identity, generator);
    let mut steps_for_breakpoint = 0;
    ristretto_iter.zip(0..TWO18).for_each(|(elem, x_hi)| {
        let key = elem.compress().to_bytes();
        hashmap.insert(key, x_hi);

        // unclean way to print status update; will clean up later
        if x_hi % TWO15 == 0 {
            println!("     [{:?}/8] completed", steps_for_breakpoint);
            steps_for_breakpoint += 1;
        }
    });
    println!("     [8/8] completed");

    DecodeU32Precomputation(hashmap)
}

lazy_static::lazy_static! {
    /// Pre-computed HashMap needed for decryption. The HashMap is independent of (works for) any key.
    pub static ref DECODE_U32_PRECOMPUTATION_FOR_G: DecodeU32Precomputation = {
        static DECODE_U32_PRECOMPUTATION_FOR_G_BINCODE: &[u8] =
            include_bytes!("decode_u32_precomputation_for_G.bincode");
        bincode::deserialize(DECODE_U32_PRECOMPUTATION_FOR_G_BINCODE).unwrap_or_default()
    };
}

/// Solves the discrete log instance using a 18/14 bit offline/online split
impl DiscreteLog {
    /// Solves the discrete log problem under the assumption that the solution
    /// is a 32-bit number.
    pub(crate) fn decode_u32(self) -> Option<u32> {
        self.decode_u32_online(&decode_u32_precomputation(self.generator))
    }

    /// Solves the discrete log instance using the pre-computed HashMap by enumerating through 2^14
    /// possible solutions
    pub fn decode_u32_online(self, hashmap: &DecodeU32Precomputation) -> Option<u32> {
        // iterator for 0G, -1G, -2G, ...
        let ristretto_iter = RistrettoIterator::new(self.target, -self.generator);

        let mut decoded = None;
        ristretto_iter.zip(0..TWO14).for_each(|(elem, x_lo)| {
            let key = elem.compress().to_bytes();
            if hashmap.0.contains_key(&key) {
                let x_hi = hashmap.0[&key];
                decoded = Some(x_lo + TWO14 * x_hi);
            }
        });
        decoded
    }
}

/// HashableRistretto iterator.
///
/// Given an initial point X and a stepping point P, the iterator iterates through
/// X + 0*P, X + 1*P, X + 2*P, X + 3*P, ...
struct RistrettoIterator {
    pub curr: RistrettoPoint,
    pub step: RistrettoPoint,
}

impl RistrettoIterator {
    fn new(curr: RistrettoPoint, step: RistrettoPoint) -> Self {
        RistrettoIterator { curr, step }
    }
}

impl Iterator for RistrettoIterator {
    type Item = RistrettoPoint;

    fn next(&mut self) -> Option<Self::Item> {
        let r = self.curr;
        self.curr += self.step;
        Some(r)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT as G, std::time::Instant,
    };

    #[test]
    #[allow(non_snake_case)]
    fn test_serialize_decode_u32_precomputation_for_G() {
        let decode_u32_precomputation_for_G = decode_u32_precomputation(G);

        if decode_u32_precomputation_for_G.0 != DECODE_U32_PRECOMPUTATION_FOR_G.0 {
            use std::{fs::File, io::Write, path::PathBuf};
            let mut f = File::create(PathBuf::from(
                "src/encryption/decode_u32_precomputation_for_G.bincode",
            ))
            .unwrap();
            f.write_all(&bincode::serialize(&decode_u32_precomputation_for_G).unwrap())
                .unwrap();
            panic!("Rebuild and run this test again");
        }
    }

    /// Discrete log test for 16/16 split
    ///
    /// Very informal measurements on my machine:
    ///   - 8 sec for precomputation
    ///   - 3 sec for online computation
    #[test]
    fn test_decode_correctness() {
        let amount: u32 = 65545;

        let instance = DiscreteLog {
            generator: G,
            target: Scalar::from(amount) * G,
        };

        // Very informal measurements for now
        let start_precomputation = Instant::now();
        let precomputed_hashmap = decode_u32_precomputation(G);
        let precomputation_secs = start_precomputation.elapsed().as_secs_f64();

        let start_online = Instant::now();
        let computed_amount = instance.decode_u32_online(&precomputed_hashmap).unwrap();
        let online_secs = start_online.elapsed().as_secs_f64();

        assert_eq!(amount, computed_amount);

        println!("16/16 Split precomputation: {:?} sec", precomputation_secs);
        println!("16/16 Split online computation: {:?} sec", online_secs);
    }
}
