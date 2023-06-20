use crate::zk_token_elgamal::pod::{
    GroupedElGamalCiphertext2Handles, GroupedElGamalCiphertext3Handles, Pod, PodU16, PodU64,
    Zeroable,
};
#[cfg(not(target_os = "solana"))]
use crate::{errors::ProofError, instruction::transfer as decoded};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferAmountCiphertext(pub GroupedElGamalCiphertext3Handles);

#[cfg(not(target_os = "solana"))]
impl From<decoded::TransferAmountCiphertext> for TransferAmountCiphertext {
    fn from(decoded_ciphertext: decoded::TransferAmountCiphertext) -> Self {
        Self(decoded_ciphertext.0.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<TransferAmountCiphertext> for decoded::TransferAmountCiphertext {
    type Error = ProofError;

    fn try_from(pod_ciphertext: TransferAmountCiphertext) -> Result<Self, Self::Error> {
        Ok(Self(pod_ciphertext.0.try_into()?))
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeEncryption(pub GroupedElGamalCiphertext2Handles);

#[cfg(not(target_os = "solana"))]
impl From<decoded::FeeEncryption> for FeeEncryption {
    fn from(decoded_ciphertext: decoded::FeeEncryption) -> Self {
        Self(decoded_ciphertext.0.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<FeeEncryption> for decoded::FeeEncryption {
    type Error = ProofError;

    fn try_from(pod_ciphertext: FeeEncryption) -> Result<Self, Self::Error> {
        Ok(Self(pod_ciphertext.0.try_into()?))
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeParameters {
    /// Fee rate expressed as basis points of the transfer amount, i.e. increments of 0.01%
    pub fee_rate_basis_points: PodU16,
    /// Maximum fee assessed on transfers, expressed as an amount of tokens
    pub maximum_fee: PodU64,
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::FeeParameters> for FeeParameters {
    fn from(decoded_fee_parameters: decoded::FeeParameters) -> Self {
        FeeParameters {
            fee_rate_basis_points: decoded_fee_parameters.fee_rate_basis_points.into(),
            maximum_fee: decoded_fee_parameters.maximum_fee.into(),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl From<FeeParameters> for decoded::FeeParameters {
    fn from(pod_fee_parameters: FeeParameters) -> Self {
        decoded::FeeParameters {
            fee_rate_basis_points: pod_fee_parameters.fee_rate_basis_points.into(),
            maximum_fee: pod_fee_parameters.maximum_fee.into(),
        }
    }
}
