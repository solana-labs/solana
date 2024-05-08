use {
    crate::elgamal_program::errors::ProofVerificationError,
    bytemuck::Pod,
    num_derive::{FromPrimitive, ToPrimitive},
};

pub mod errors;
pub mod pod;

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofType {
    /// Empty proof type used to distinguish if a proof context account is initialized
    Uninitialized,
}

pub trait ZkProofData<T: Pod> {
    const PROOF_TYPE: ProofType;

    fn context_data(&self) -> &T;

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofVerificationError>;
}
