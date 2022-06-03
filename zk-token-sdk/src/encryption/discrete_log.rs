#![cfg(not(target_os = "solana"))]

use {
    crate::errors::ProofError,
    curve25519_dalek::{
        constants::RISTRETTO_BASEPOINT_POINT as G, ristretto::RistrettoPoint, scalar::Scalar,
        traits::Identity,
    },
    serde::{Deserialize, Serialize},
    std::{collections::HashMap, thread},
};

const TWO16: u64 = 65536; // 2^16

/// Type that captures a discrete log challenge.
///
/// The goal of discrete log is to find x such that x * generator = target.
#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
pub struct DiscreteLog {
    /// Generator point for discrete log
    pub generator: RistrettoPoint,
    /// Target point for discrete log
    pub target: RistrettoPoint,
    /// Number of threads used for discrete log computation
    num_threads: usize,
    /// Range bound for discrete log search derived from the max value to search for and
    /// `num_threads`
    range_bound: usize,
    /// Ristretto point representing each step of the discrete log search
    step_point: RistrettoPoint,
}

#[derive(Serialize, Deserialize, Default)]
pub struct DecodePrecomputation(HashMap<[u8; 32], u16>);

/// Builds a HashMap of 2^16 elements
#[allow(dead_code)]
fn decode_u32_precomputation(generator: RistrettoPoint) -> DecodePrecomputation {
    let mut hashmap = HashMap::new();

    let two16_scalar = Scalar::from(TWO16);
    let identity = RistrettoPoint::identity(); // 0 * G
    let generator = two16_scalar * generator; // 2^16 * G

    // iterator for 2^12*0G , 2^12*1G, 2^12*2G, ...
    let ristretto_iter = RistrettoIterator::new((identity, 0), (generator, 1));
    for (point, x_hi) in ristretto_iter.take(TWO16 as usize) {
        let key = point.compress().to_bytes();
        hashmap.insert(key, x_hi as u16);
    }

    DecodePrecomputation(hashmap)
}

lazy_static::lazy_static! {
    /// Pre-computed HashMap needed for decryption. The HashMap is independent of (works for) any key.
    pub static ref DECODE_PRECOMPUTATION_FOR_G: DecodePrecomputation = {
        static DECODE_PRECOMPUTATION_FOR_G_BINCODE: &[u8] =
            include_bytes!("decode_u32_precomputation_for_G.bincode");
        bincode::deserialize(DECODE_PRECOMPUTATION_FOR_G_BINCODE).unwrap_or_default()
    };
}

/// Solves the discrete log instance using a 16/16 bit offline/online split
impl DiscreteLog {
    /// Discrete log instance constructor.
    ///
    /// Default number of threads set to 1.
    pub fn new(generator: RistrettoPoint, target: RistrettoPoint) -> Self {
        Self {
            generator,
            target,
            num_threads: 1,
            range_bound: TWO16 as usize,
            step_point: G,
        }
    }

    /// Adjusts number of threads in a discrete log instance.
    pub fn num_threads(&mut self, num_threads: usize) -> Result<(), ProofError> {
        // number of threads must be a positive power-of-two integer
        if num_threads == 0 || (num_threads & (num_threads - 1)) != 0 {
            return Err(ProofError::DiscreteLogThreads);
        }

        self.num_threads = num_threads;
        self.range_bound = (TWO16 as usize).checked_div(num_threads).unwrap();
        self.step_point = Scalar::from(num_threads as u64) * G;

        Ok(())
    }

    /// Solves the discrete log problem under the assumption that the solution
    /// is a 32-bit number.
    pub fn decode_u32(self) -> Option<u64> {
        let mut starting_point = self.target;
        let handles = (0..self.num_threads)
            .into_iter()
            .map(|i| {
                let ristretto_iterator = RistrettoIterator::new(
                    (starting_point, i as u64),
                    (-(&self.step_point), self.num_threads as u64),
                );

                let handle =
                    thread::spawn(move || Self::decode_range(ristretto_iterator, self.range_bound));

                starting_point -= G;
                handle
            })
            .collect::<Vec<_>>();

        let mut solution = None;
        for handle in handles {
            let discrete_log = handle.join().unwrap();
            if discrete_log.is_some() {
                solution = discrete_log;
            }
        }
        solution
    }

    fn decode_range(ristretto_iterator: RistrettoIterator, range_bound: usize) -> Option<u64> {
        let hashmap = &DECODE_PRECOMPUTATION_FOR_G;
        let mut decoded = None;
        for (point, x_lo) in ristretto_iterator.take(range_bound) {
            let key = point.compress().to_bytes();
            if hashmap.0.contains_key(&key) {
                let x_hi = hashmap.0[&key];
                decoded = Some(x_lo + TWO16 * x_hi as u64);
            }
        }
        decoded
    }
}

/// HashableRistretto iterator.
///
/// Given an initial point X and a stepping point P, the iterator iterates through
/// X + 0*P, X + 1*P, X + 2*P, X + 3*P, ...
struct RistrettoIterator {
    pub current: (RistrettoPoint, u64),
    pub step: (RistrettoPoint, u64),
}

impl RistrettoIterator {
    fn new(current: (RistrettoPoint, u64), step: (RistrettoPoint, u64)) -> Self {
        RistrettoIterator { current, step }
    }
}

impl Iterator for RistrettoIterator {
    type Item = (RistrettoPoint, u64);

    fn next(&mut self) -> Option<Self::Item> {
        let r = self.current;
        self.current = (self.current.0 + self.step.0, self.current.1 + self.step.1);
        Some(r)
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::time::Instant};

    #[test]
    #[allow(non_snake_case)]
    fn test_serialize_decode_u32_precomputation_for_G() {
        let decode_u32_precomputation_for_G = decode_u32_precomputation(G);

        if decode_u32_precomputation_for_G.0 != DECODE_PRECOMPUTATION_FOR_G.0 {
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

    #[test]
    fn test_decode_correctness() {
        // general case
        let amount: u64 = 55;

        let instance = DiscreteLog::new(G, Scalar::from(amount) * G);

        // Very informal measurements for now
        let start_computation = Instant::now();
        let decoded = instance.decode_u32();
        let computation_secs = start_computation.elapsed().as_secs_f64();

        assert_eq!(amount, decoded.unwrap());

        println!(
            "single thread discrete log computation secs: {:?} sec",
            computation_secs
        );
    }

    #[test]
    fn test_decode_correctness_threaded() {
        // general case
        let amount: u64 = 55;

        let mut instance = DiscreteLog::new(G, Scalar::from(amount) * G);
        instance.num_threads(4).unwrap();

        // Very informal measurements for now
        let start_computation = Instant::now();
        let decoded = instance.decode_u32();
        let computation_secs = start_computation.elapsed().as_secs_f64();

        assert_eq!(amount, decoded.unwrap());

        println!(
            "4 thread discrete log computation: {:?} sec",
            computation_secs
        );

        // amount 0
        let amount: u64 = 0;

        let instance = DiscreteLog::new(G, Scalar::from(amount) * G);

        let decoded = instance.decode_u32();
        assert_eq!(amount, decoded.unwrap());

        // amount 1
        let amount: u64 = 1;

        let instance = DiscreteLog::new(G, Scalar::from(amount) * G);

        let decoded = instance.decode_u32();
        assert_eq!(amount, decoded.unwrap());

        // amount 2
        let amount: u64 = 2;

        let instance = DiscreteLog::new(G, Scalar::from(amount) * G);

        let decoded = instance.decode_u32();
        assert_eq!(amount, decoded.unwrap());

        // amount 3
        let amount: u64 = 3;

        let instance = DiscreteLog::new(G, Scalar::from(amount) * G);

        let decoded = instance.decode_u32();
        assert_eq!(amount, decoded.unwrap());

        // max amount
        let amount: u64 = ((1_u64 << 32) - 1) as u64;

        let instance = DiscreteLog::new(G, Scalar::from(amount) * G);

        let decoded = instance.decode_u32();
        assert_eq!(amount, decoded.unwrap());
    }
}
