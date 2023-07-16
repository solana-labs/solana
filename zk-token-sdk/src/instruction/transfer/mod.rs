mod encryption;
mod with_fee;
mod without_fee;

#[cfg(not(target_os = "solana"))]
use {
    crate::encryption::{
        elgamal::ElGamalCiphertext,
        pedersen::{PedersenCommitment, PedersenOpening},
    },
    curve25519_dalek::scalar::Scalar,
};
#[cfg(not(target_os = "solana"))]
pub use {
    encryption::{FeeEncryption, TransferAmountCiphertext},
    with_fee::TransferWithFeePubkeys,
    without_fee::TransferPubkeys,
};
pub use {
    with_fee::{TransferWithFeeData, TransferWithFeeProofContext},
    without_fee::{TransferData, TransferProofContext},
};

#[cfg(not(target_os = "solana"))]
#[derive(Debug, Copy, Clone)]
pub enum Role {
    Source,
    Destination,
    Auditor,
    WithdrawWithheldAuthority,
}

/// Takes in a 64-bit number `amount` and a bit length `bit_length`. It returns:
///  - the `bit_length` low bits of `amount` interpretted as u64
///  - the (64 - `bit_length`) high bits of `amount` interpretted as u64
#[cfg(not(target_os = "solana"))]
pub fn split_u64(amount: u64, bit_length: usize) -> (u64, u64) {
    if bit_length == 64 {
        (amount, 0)
    } else {
        let lo = amount << (64 - bit_length) >> (64 - bit_length);
        let hi = amount >> bit_length;
        (lo, hi)
    }
}

#[cfg(not(target_os = "solana"))]
pub fn combine_lo_hi_u64(amount_lo: u64, amount_hi: u64, bit_length: usize) -> u64 {
    if bit_length == 64 {
        amount_lo
    } else {
        amount_lo + (amount_hi << bit_length)
    }
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

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FeeParameters {
    /// Fee rate expressed as basis points of the transfer amount, i.e. increments of 0.01%
    pub fee_rate_basis_points: u16,
    /// Maximum fee assessed on transfers, expressed as an amount of tokens
    pub maximum_fee: u64,
}
