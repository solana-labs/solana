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
pub struct DiscreteLogInstance {
    /// Generator point for discrete log
    pub generator: RistrettoPoint,
    /// Target point for discrete log
    pub target: RistrettoPoint,
}

/// Builds a HashMap of 2^18 elements
pub fn decode_u32_precomputation(generator: RistrettoPoint) -> HashMap<[u8; 32], u32> {
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

    hashmap
}

/// Solves the discrete log instance using a 18/14 bit offline/online split
impl DiscreteLogInstance {
    /// Solves the discrete log problem under the assumption that the solution
    /// is a 32-bit number.
    pub fn decode_u32(self) -> Option<u32> {
        let hashmap = decode_u32_precomputation(self.generator);
        self.decode_u32_online(&hashmap)
    }

    /// Solves the discrete log instance using the pre-computed HashMap by enumerating through 2^14
    /// possible solutions
    pub fn decode_u32_online(self, hashmap: &HashMap<[u8; 32], u32>) -> Option<u32> {
        // iterator for 0G, -1G, -2G, ...
        let ristretto_iter = RistrettoIterator::new(self.target, -self.generator);

        let mut decoded = None;
        ristretto_iter.zip(0..TWO14).for_each(|(elem, x_lo)| {
            let key = elem.compress().to_bytes();
            if hashmap.contains_key(&key) {
                let x_hi = hashmap[&key];
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
    use {super::*, curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT as G};

    /// Discrete log test for 16/16 split
    ///
    /// Very informal measurements on my machine:
    ///   - 8 sec for precomputation
    ///   - 3 sec for online computation
    #[test]
    fn test_decode_correctness() {
        let amount: u32 = 65545;

        let instance = DiscreteLogInstance {
            generator: G,
            target: Scalar::from(amount) * G,
        };

        // Very informal measurements for now
        let start_precomputation = time::precise_time_s();
        let precomputed_hashmap = decode_u32_precomputation(G);
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
}
