//! Errors related to proving and verifying sigma proofs.
use {
    crate::errors::{ProofVerificationError, TranscriptError},
    thiserror::Error,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
#[error("equality proof verification failed: {0}")]
pub struct EqualityProofError(#[from] pub(crate) ProofVerificationError);
impl_from_transcript_error!(EqualityProofError);

#[derive(Error, Clone, Debug, Eq, PartialEq)]
#[error("validity proof verification failed: {0}")]
pub struct ValidityProofError(#[from] pub(crate) ProofVerificationError);
impl_from_transcript_error!(ValidityProofError);

#[derive(Error, Clone, Debug, Eq, PartialEq)]
#[error("zero-balance proof verification failed: {0}")]
pub struct ZeroBalanceProofError(#[from] pub(crate) ProofVerificationError);
impl_from_transcript_error!(ZeroBalanceProofError);

#[derive(Error, Clone, Debug, Eq, PartialEq)]
#[error("fee sigma proof verification failed: {0}")]
pub struct FeeSigmaProofError(#[from] pub(crate) ProofVerificationError);
impl_from_transcript_error!(FeeSigmaProofError);

#[derive(Error, Clone, Debug, Eq, PartialEq)]
#[error("public key validity proof verification failed: {0}")]
pub struct PubkeyValidityProofError(#[from] pub(crate) ProofVerificationError);
impl_from_transcript_error!(PubkeyValidityProofError);
