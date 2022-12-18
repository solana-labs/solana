pub mod prelude {
    pub use super::{consts::*, target_arch::*};
}

use {consts::*, thiserror::Error};

mod consts {
    pub const POSEIDON_HASH_BYTES: usize = 32;
}

#[derive(Error, Debug)]
pub enum PoseidonSyscallError {
    #[error("Failed to compute the Poseidon hash")]
    Poseidon,
    #[error("Failed to convert a vector of bytes into an array")]
    VecToArray,
    #[error("Unexpected error")]
    Unexpected,
}

impl From<u64> for PoseidonSyscallError {
    fn from(error: u64) -> Self {
        match error {
            1 => PoseidonSyscallError::Poseidon,
            2 => PoseidonSyscallError::VecToArray,
            _ => PoseidonSyscallError::Unexpected,
        }
    }
}

impl From<PoseidonSyscallError> for u64 {
    fn from(error: PoseidonSyscallError) -> Self {
        match error {
            PoseidonSyscallError::Poseidon => 1,
            PoseidonSyscallError::VecToArray => 2,
            PoseidonSyscallError::Unexpected => 3,
        }
    }
}

#[repr(transparent)]
pub struct PoseidonHash(pub [u8; POSEIDON_HASH_BYTES]);

impl PoseidonHash {
    pub fn new(hash_array: [u8; POSEIDON_HASH_BYTES]) -> Self {
        Self(hash_array)
    }

    pub fn to_bytes(&self) -> [u8; POSEIDON_HASH_BYTES] {
        self.0
    }
}

#[cfg(not(target_os = "solana"))]
mod target_arch {
    use super::*;
    use ark_bn254::fq::Fq;
    use ark_ff::{BigInteger, PrimeField};
    use light_poseidon::{parameters::bn254_x5_3::poseidon_parameters, PoseidonHasher};

    pub fn poseidon_hash(vals: &[&[u8]]) -> Result<PoseidonHash, PoseidonSyscallError> {
        let params = poseidon_parameters();
        let mut poseidon = PoseidonHasher::<Fq>::new(params);

        let mut inputs = Vec::with_capacity(vals.len());
        for val in vals {
            inputs.push(Fq::from_be_bytes_mod_order(&val));
        }

        let res = poseidon
            .hash(&inputs)
            .map_err(|_| PoseidonSyscallError::Poseidon)?
            .into_repr()
            .to_bytes_be();

        Ok(PoseidonHash(
            res.try_into()
                .map_err(|_| PoseidonSyscallError::VecToArray)?,
        ))
    }
}

#[cfg(target_os = "solana")]
mod target_arch {
    use super::*;

    pub fn poseidon_hash(vals: &[&[u8]]) -> Result<PoseidonHash, PoseidonSyscallError> {
        let mut hash_result = [0; POSEIDON_HASH_BYTES];
        let result = unsafe {
            crate::syscalls::sol_poseidon(
                vals as *const _ as *const u8,
                vals.len() as u64,
                &mut hash_result as *mut _ as *mut u8,
            )
        };

        match result {
            0 => Ok(PoseidonHash::new(hash_result)),
            e => Err(PoseidonSyscallError::from(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::prelude::*;

    #[test]
    fn test_poseidon_input_ones_twos() {
        let input1 = [1u8; 32];
        let input2 = [2u8; 32];

        let hash = poseidon_hash(&[&input1, &input2]).expect("Failed to compute the Poseidon hash");
        assert_eq!(
            hash.to_bytes(),
            [
                40, 7, 251, 60, 51, 30, 115, 141, 251, 200, 13, 46, 134, 91, 113, 170, 131, 90, 53,
                175, 9, 61, 242, 164, 127, 33, 249, 65, 253, 131, 35, 116
            ]
        );
    }

    #[test]
    fn test_poseidon_input_one_two() {
        let input1 = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 1,
        ];
        let input2 = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 2,
        ];

        let hash = poseidon_hash(&[&input1, &input2]).expect("Failed to compute the Poseidon hash");
        assert_eq!(
            hash.to_bytes(),
            [
                25, 11, 182, 121, 54, 48, 205, 9, 39, 164, 111, 44, 108, 203, 20, 95, 112, 101, 97,
                130, 151, 54, 169, 215, 37, 104, 12, 83, 176, 236, 253, 54
            ]
        );
    }

    #[test]
    fn test_poseidon_input_random() {
        let input1 = [
            0x06, 0x9c, 0x63, 0x81, 0xac, 0x0b, 0x96, 0x8e, 0x88, 0x1c, 0x91, 0x3c, 0x17, 0xd8,
            0x36, 0x06, 0x7f, 0xd1, 0x5f, 0x2c, 0xc7, 0x9f, 0x90, 0x2c, 0x80, 0x70, 0xb3, 0x6d,
            0x28, 0x66, 0x17, 0xdd,
        ];
        let input2 = [
            0xc3, 0x3b, 0x60, 0x04, 0x2f, 0x76, 0xc7, 0xfb, 0xd0, 0x5d, 0xb7, 0x76, 0x23, 0xcb,
            0x17, 0xb8, 0x1d, 0x49, 0x41, 0x4b, 0x82, 0xe5, 0x6a, 0x2e, 0xc0, 0x18, 0xf7, 0xa5,
            0x5c, 0x3f, 0x30, 0x0b,
        ];

        let hash = poseidon_hash(&[&input1, &input2]).expect("Failed to compute the Poseidon hash");
        assert_eq!(
            hash.to_bytes(),
            [
                43, 94, 133, 6, 86, 161, 42, 237, 224, 252, 105, 131, 134, 176, 141, 84, 159, 162,
                172, 12, 155, 131, 123, 94, 218, 217, 178, 239, 100, 87, 4, 238
            ]
        )
    }
}
