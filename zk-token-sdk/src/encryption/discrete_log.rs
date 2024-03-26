//! The discrete log implementation for the twisted ElGamal decryption.
//!
//! The implementation uses the baby-step giant-step method, which consists of a precomputation
//! step and an online step. The precomputation step involves computing a hash table of a number
//! of Ristretto points that is independent of a discrete log instance. The online phase computes
//! the final discrete log solution using the discrete log instance and the pre-computed hash
//! table. More details on the baby-step giant-step algorithm and the implementation can be found
//! in the [spl documentation](https://spl.solana.com).
//!
//! The implementation is NOT intended to run in constant-time. There are some measures to prevent
//! straightforward timing attacks. For instance, it does not short-circuit the search when a
//! solution is found. However, the use of hashtables, batching, and threads make the
//! implementation inherently not constant-time. This may theoretically allow an adversary to gain
//! information on a discrete log solution depending on the execution time of the implementation.
//!

#![cfg(not(target_os = "solana"))]

use {
    crate::RISTRETTO_POINT_LEN,
    curve25519_dalek::{
        constants::RISTRETTO_BASEPOINT_POINT as G,
        ristretto::RistrettoPoint,
        scalar::Scalar,
        traits::{Identity, IsIdentity},
    },
    itertools::Itertools,
    serde::{Deserialize, Serialize},
    std::{collections::HashMap, thread},
    thiserror::Error,
};

const TWO16: u64 = 65536; // 2^16
const TWO17: u64 = 131072; // 2^17

/// Maximum number of threads permitted for discrete log computation
const MAX_THREAD: usize = 65536;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum DiscreteLogError {
    #[error("discrete log number of threads not power-of-two")]
    DiscreteLogThreads,
    #[error("discrete log batch size too large")]
    DiscreteLogBatchSize,
}

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
    /// Ristretto point compression batch size
    compression_batch_size: usize,
}

#[derive(Serialize, Deserialize, Default)]
pub struct DecodePrecomputation(HashMap<[u8; RISTRETTO_POINT_LEN], u16>);

/// Builds a HashMap of 2^16 elements
#[allow(dead_code)]
fn decode_u32_precomputation(generator: RistrettoPoint) -> DecodePrecomputation {
    let mut hashmap = HashMap::new();

    let two17_scalar = Scalar::from(TWO17);
    let identity = RistrettoPoint::identity(); // 0 * G
    let generator = two17_scalar * generator; // 2^17 * G

    // iterator for 2^17*0G , 2^17*1G, 2^17*2G, ...
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
            compression_batch_size: 32,
        }
    }

    /// Adjusts number of threads in a discrete log instance.
    pub fn num_threads(&mut self, num_threads: usize) -> Result<(), DiscreteLogError> {
        // number of threads must be a positive power-of-two integer
        if num_threads == 0 || (num_threads & (num_threads - 1)) != 0 || num_threads > MAX_THREAD {
            return Err(DiscreteLogError::DiscreteLogThreads);
        }

        self.num_threads = num_threads;
        self.range_bound = (TWO16 as usize).checked_div(num_threads).unwrap();
        self.step_point = Scalar::from(num_threads as u64) * G;

        Ok(())
    }

    /// Adjusts inversion batch size in a discrete log instance.
    pub fn set_compression_batch_size(
        &mut self,
        compression_batch_size: usize,
    ) -> Result<(), DiscreteLogError> {
        if compression_batch_size >= TWO16 as usize || compression_batch_size == 0 {
            return Err(DiscreteLogError::DiscreteLogBatchSize);
        }
        self.compression_batch_size = compression_batch_size;

        Ok(())
    }

    /// Solves the discrete log problem under the assumption that the solution
    /// is a positive 32-bit number.
    pub fn decode_u32(self) -> Option<u64> {
        let mut starting_point = self.target;
        let handles = (0..self.num_threads)
            .map(|i| {
                let ristretto_iterator = RistrettoIterator::new(
                    (starting_point, i as u64),
                    (-(&self.step_point), self.num_threads as u64),
                );

                let handle = thread::spawn(move || {
                    Self::decode_range(
                        ristretto_iterator,
                        self.range_bound,
                        self.compression_batch_size,
                    )
                });

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

    fn decode_range(
        ristretto_iterator: RistrettoIterator,
        range_bound: usize,
        compression_batch_size: usize,
    ) -> Option<u64> {
        let hashmap = &DECODE_PRECOMPUTATION_FOR_G;
        let mut decoded = None;

        for batch in &ristretto_iterator
            .take(range_bound)
            .chunks(compression_batch_size)
        {
            // batch compression currently errors if any point in the batch is the identity point
            let (batch_points, batch_indices): (Vec<_>, Vec<_>) = batch
                .filter(|(point, index)| {
                    if point.is_identity() {
                        decoded = Some(*index);
                        return false;
                    }
                    true
                })
                .unzip();

            let batch_compressed = RistrettoPoint::double_and_compress_batch(&batch_points);

            for (point, x_lo) in batch_compressed.iter().zip(batch_indices.iter()) {
                let key = point.to_bytes();
                if hashmap.0.contains_key(&key) {
                    let x_hi = hashmap.0[&key];
                    decoded = Some(x_lo + TWO16 * x_hi as u64);
                }
            }
        }

        decoded
    }
}

/// Hashable Ristretto iterator.
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
        // let decode_u32_precomputation_for_G = decode_u32_precomputation(G);

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
        let amount: u64 = 4294967295;

        let instance = DiscreteLog::new(G, Scalar::from(amount) * G);

        // Very informal measurements for now
        let start_computation = Instant::now();
        let decoded = instance.decode_u32();
        let computation_secs = start_computation.elapsed().as_secs_f64();

        assert_eq!(amount, decoded.unwrap());

        println!("single thread discrete log computation secs: {computation_secs:?} sec");
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

        println!("4 thread discrete log computation: {computation_secs:?} sec");

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
        let amount: u64 = (1_u64 << 32) - 1;

        let instance = DiscreteLog::new(G, Scalar::from(amount) * G);

        let decoded = instance.decode_u32();
        assert_eq!(amount, decoded.unwrap());
    }
}
