mod encryption;
mod transfer_with_fee;
mod transfer_without_fee;

pub use {
    encryption::{FeeEncryption, TransferAmountEncryption},
    transfer_with_fee::{TransferWithFeeData, TransferWithFeeProofContext, TransferWithFeePubkeys},
    transfer_without_fee::{TransferData, TransferProofContext, TransferPubkeys},
};

#[cfg(not(target_os = "solana"))]
use arrayref::{array_ref, array_refs};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FeeParameters {
    /// Fee rate expressed as basis points of the transfer amount, i.e. increments of 0.01%
    pub fee_rate_basis_points: u16,
    /// Maximum fee assessed on transfers, expressed as an amount of tokens
    pub maximum_fee: u64,
}

#[cfg(not(target_os = "solana"))]
impl FeeParameters {
    pub fn to_bytes(&self) -> [u8; 10] {
        let mut bytes = [0u8; 10];
        bytes[..2].copy_from_slice(&self.fee_rate_basis_points.to_le_bytes());
        bytes[2..10].copy_from_slice(&self.maximum_fee.to_le_bytes());

        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let bytes = array_ref![bytes, 0, 10];
        let (fee_rate_basis_points, maximum_fee) = array_refs![bytes, 2, 8];

        Self {
            fee_rate_basis_points: u16::from_le_bytes(*fee_rate_basis_points),
            maximum_fee: u64::from_le_bytes(*maximum_fee),
        }
    }
}
