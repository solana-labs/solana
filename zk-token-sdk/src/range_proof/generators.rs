use {
    crate::range_proof::errors::RangeProofGeneratorError,
    curve25519_dalek::{
        digest::{ExtendableOutput, Update, XofReader},
        ristretto::RistrettoPoint,
    },
    sha3::{Sha3XofReader, Shake256},
};

#[cfg(not(target_os = "solana"))]
const MAX_GENERATOR_LENGTH: usize = u32::MAX as usize;

/// Generators for Pedersen vector commitments.
///
/// The code is copied from https://github.com/dalek-cryptography/bulletproofs for now...
struct GeneratorsChain {
    reader: Sha3XofReader,
}

impl GeneratorsChain {
    /// Creates a chain of generators, determined by the hash of `label`.
    fn new(label: &[u8]) -> Self {
        let mut shake = Shake256::default();
        shake.update(b"GeneratorsChain");
        shake.update(label);

        GeneratorsChain {
            reader: shake.finalize_xof(),
        }
    }

    /// Advances the reader n times, squeezing and discarding
    /// the result.
    fn fast_forward(mut self, n: usize) -> Self {
        for _ in 0..n {
            let mut buf = [0u8; 64];
            self.reader.read(&mut buf);
        }
        self
    }
}

impl Default for GeneratorsChain {
    fn default() -> Self {
        Self::new(&[])
    }
}

impl Iterator for GeneratorsChain {
    type Item = RistrettoPoint;

    fn next(&mut self) -> Option<Self::Item> {
        let mut uniform_bytes = [0u8; 64];
        self.reader.read(&mut uniform_bytes);

        Some(RistrettoPoint::from_uniform_bytes(&uniform_bytes))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::max_value(), None)
    }
}

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct BulletproofGens {
    /// The maximum number of usable generators.
    pub gens_capacity: usize,
    /// Precomputed \\(\mathbf G\\) generators.
    G_vec: Vec<RistrettoPoint>,
    /// Precomputed \\(\mathbf H\\) generators.
    H_vec: Vec<RistrettoPoint>,
}

impl BulletproofGens {
    pub fn new(gens_capacity: usize) -> Result<Self, RangeProofGeneratorError> {
        let mut gens = BulletproofGens {
            gens_capacity: 0,
            G_vec: Vec::new(),
            H_vec: Vec::new(),
        };
        gens.increase_capacity(gens_capacity)?;
        Ok(gens)
    }

    // pub fn new_aggregate(gens_capacities: Vec<usize>) -> Vec<BulletproofGens> {
    //     let mut gens_vector = Vec::new();
    //     for (capacity, i) in gens_capacities.iter().enumerate() {
    //         gens_vector.push(BulletproofGens::new(capacity, &i.to_le_bytes()));
    //     }
    //     gens_vector
    // }

    /// Increases the generators' capacity to the amount specified.
    /// If less than or equal to the current capacity, does nothing.
    pub fn increase_capacity(
        &mut self,
        new_capacity: usize,
    ) -> Result<(), RangeProofGeneratorError> {
        if self.gens_capacity >= new_capacity {
            return Ok(());
        }

        if new_capacity > MAX_GENERATOR_LENGTH {
            return Err(RangeProofGeneratorError::MaximumGeneratorLengthExceeded);
        }

        self.G_vec.extend(
            &mut GeneratorsChain::new(&[b'G'])
                .fast_forward(self.gens_capacity)
                .take(new_capacity - self.gens_capacity),
        );

        self.H_vec.extend(
            &mut GeneratorsChain::new(&[b'H'])
                .fast_forward(self.gens_capacity)
                .take(new_capacity - self.gens_capacity),
        );

        self.gens_capacity = new_capacity;
        Ok(())
    }

    #[allow(non_snake_case)]
    pub(crate) fn G(&self, n: usize) -> impl Iterator<Item = &RistrettoPoint> {
        GensIter {
            array: &self.G_vec,
            n,
            gen_idx: 0,
        }
    }

    #[allow(non_snake_case)]
    pub(crate) fn H(&self, n: usize) -> impl Iterator<Item = &RistrettoPoint> {
        GensIter {
            array: &self.H_vec,
            n,
            gen_idx: 0,
        }
    }
}

struct GensIter<'a> {
    array: &'a Vec<RistrettoPoint>,
    n: usize,
    gen_idx: usize,
}

impl<'a> Iterator for GensIter<'a> {
    type Item = &'a RistrettoPoint;

    fn next(&mut self) -> Option<Self::Item> {
        if self.gen_idx >= self.n {
            None
        } else {
            let cur_gen = self.gen_idx;
            self.gen_idx += 1;
            Some(&self.array[cur_gen])
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.n - self.gen_idx;
        (size, Some(size))
    }
}
