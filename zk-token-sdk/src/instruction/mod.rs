pub mod close_account;
pub mod transfer;
pub mod transfer_with_fee;
pub mod withdraw;
pub mod withdraw_withheld;

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::ElGamalCiphertext,
            pedersen::{PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
    },
    curve25519_dalek::scalar::Scalar,
    subtle::ConstantTimeEq,
};
pub use {
    close_account::CloseAccountData, transfer::TransferData,
    transfer_with_fee::TransferWithFeeData, withdraw::WithdrawData,
    withdraw_withheld::WithdrawWithheldTokensData,
};

#[cfg(not(target_os = "solana"))]
pub trait Verifiable {
    fn verify(&self) -> Result<(), ProofError>;
}

#[cfg(not(target_os = "solana"))]
#[derive(Debug, Copy, Clone)]
pub enum Role {
    Source,
    Dest,
    Auditor,
}

/// Takes in a 64-bit number `amount` and a bit length `bit_length`. It returns:
///  - the `bit_length` low bits of `amount` interpretted as u64
///  - the (64 - `bit_length`) high bits of `amount` interpretted as u64
#[cfg(not(target_os = "solana"))]
pub fn split_u64(
    amount: u64,
    lo_bit_length: usize,
    hi_bit_length: usize,
) -> Result<(u64, u64), ProofError> {
    assert!(lo_bit_length <= 64);
    assert!(hi_bit_length <= 64);

    if !bool::from((amount >> (lo_bit_length + hi_bit_length)).ct_eq(&0u64)) {
        return Err(ProofError::TransferAmount);
    }

    let lo = amount << (64 - lo_bit_length) >> (64 - lo_bit_length);
    let hi = amount >> lo_bit_length;

    Ok((lo, hi))
}

#[cfg(not(target_os = "solana"))]
fn combine_lo_hi_ciphertexts(
    ciphertext_lo: &ElGamalCiphertext,
    ciphertext_hi: &ElGamalCiphertext,
    bit_length: usize,
) -> ElGamalCiphertext {
    let two_power = (1_u64) << bit_length;
    ciphertext_lo + &(ciphertext_hi * &Scalar::from(two_power))
}

#[cfg(not(target_os = "solana"))]
pub fn combine_lo_hi_commitments(
    comm_lo: &PedersenCommitment,
    comm_hi: &PedersenCommitment,
    bit_length: usize,
) -> PedersenCommitment {
    let two_power = (1_u64) << bit_length;
    comm_lo + comm_hi * &Scalar::from(two_power)
}

#[cfg(not(target_os = "solana"))]
pub fn combine_lo_hi_openings(
    opening_lo: &PedersenOpening,
    opening_hi: &PedersenOpening,
    bit_length: usize,
) -> PedersenOpening {
    let two_power = (1_u64) << bit_length;
    opening_lo + opening_hi * &Scalar::from(two_power)
}
