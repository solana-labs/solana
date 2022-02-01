pub mod close_account;
pub mod transfer;
pub mod transfer_with_fee;
pub mod withdraw;

#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            elgamal::ElGamalCiphertext,
            pedersen::{PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
    },
    curve25519_dalek::scalar::Scalar,
};
pub use {close_account::CloseAccountData, transfer::TransferData, withdraw::WithdrawData};

/// Constant for 2^32
#[cfg(not(target_arch = "bpf"))]
const TWO_32: u64 = 4294967296;

#[cfg(not(target_arch = "bpf"))]
pub trait Verifiable {
    fn verify(&self) -> Result<(), ProofError>;
}

#[cfg(not(target_arch = "bpf"))]
#[derive(Debug, Copy, Clone)]
pub enum Role {
    Source,
    Dest,
    Auditor,
}

/// Split u64 number into two u32 numbers
#[cfg(not(target_arch = "bpf"))]
pub fn split_u64_into_u32(amount: u64) -> (u32, u32) {
    let lo = amount as u32;
    let hi = (amount >> 32) as u32;

    (lo, hi)
}

#[cfg(not(target_arch = "bpf"))]
fn combine_u32_ciphertexts(
    ciphertext_lo: &ElGamalCiphertext,
    ciphertext_hi: &ElGamalCiphertext,
) -> ElGamalCiphertext {
    ciphertext_lo + &(ciphertext_hi * &Scalar::from(TWO_32))
}

#[cfg(not(target_arch = "bpf"))]
pub fn combine_u32_commitments(
    comm_lo: &PedersenCommitment,
    comm_hi: &PedersenCommitment,
) -> PedersenCommitment {
    comm_lo + comm_hi * &Scalar::from(TWO_32)
}

#[cfg(not(target_arch = "bpf"))]
pub fn combine_u32_openings(
    opening_lo: &PedersenOpening,
    opening_hi: &PedersenOpening,
) -> PedersenOpening {
    opening_lo + opening_hi * &Scalar::from(TWO_32)
}
