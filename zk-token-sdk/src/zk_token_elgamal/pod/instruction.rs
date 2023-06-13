use crate::zk_token_elgamal::pod::{
    DecryptHandle, ElGamalPubkey, GroupedElGamalCiphertext3Handles, PedersenCommitment, Pod,
    PodU16, PodU64, Zeroable,
};
#[cfg(not(target_os = "solana"))]
use crate::{errors::ProofError, instruction::transfer as decoded};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferPubkeys {
    pub source_pubkey: ElGamalPubkey,
    pub destination_pubkey: ElGamalPubkey,
    pub auditor_pubkey: ElGamalPubkey,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferWithFeePubkeys {
    pub source_pubkey: ElGamalPubkey,
    pub destination_pubkey: ElGamalPubkey,
    pub auditor_pubkey: ElGamalPubkey,
    pub withdraw_withheld_authority_pubkey: ElGamalPubkey,
}

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
pub struct FeeEncryption {
    pub commitment: PedersenCommitment,
    pub destination_handle: DecryptHandle,
    pub withdraw_withheld_authority_handle: DecryptHandle,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeParameters {
    /// Fee rate expressed as basis points of the transfer amount, i.e. increments of 0.01%
    pub fee_rate_basis_points: PodU16,
    /// Maximum fee assessed on transfers, expressed as an amount of tokens
    pub maximum_fee: PodU64,
}
