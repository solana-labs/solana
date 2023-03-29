//! Abstractions and implementations for transaction signers.

#![cfg(feature = "full")]

use {
    crate::{
        pubkey::Pubkey,
        signature::{PresignerError, Signature},
        transaction::TransactionError,
    },
    itertools::Itertools,
    thiserror::Error,
};

pub mod keypair;
pub mod null_signer;
pub mod presigner;
pub mod signers;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignerError {
    #[error("keypair-pubkey mismatch")]
    KeypairPubkeyMismatch,

    #[error("not enough signers")]
    NotEnoughSigners,

    #[error("transaction error")]
    TransactionError(#[from] TransactionError),

    #[error("custom error: {0}")]
    Custom(String),

    // Presigner-specific Errors
    #[error("presigner error")]
    PresignerError(#[from] PresignerError),

    // Remote Keypair-specific Errors
    #[error("connection error: {0}")]
    Connection(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("no device found")]
    NoDeviceFound,

    #[error("{0}")]
    Protocol(String),

    #[error("{0}")]
    UserCancel(String),

    #[error("too many signers")]
    TooManySigners,
}

/// The `Signer` trait declares operations that all digital signature providers
/// must support. It is the primary interface by which signers are specified in
/// `Transaction` signing interfaces
pub trait Signer {
    /// Infallibly gets the implementor's public key. Returns the all-zeros
    /// `Pubkey` if the implementor has none.
    fn pubkey(&self) -> Pubkey {
        self.try_pubkey().unwrap_or_default()
    }
    /// Fallibly gets the implementor's public key
    fn try_pubkey(&self) -> Result<Pubkey, SignerError>;
    /// Infallibly produces an Ed25519 signature over the provided `message`
    /// bytes. Returns the all-zeros `Signature` if signing is not possible.
    fn sign_message(&self, message: &[u8]) -> Signature {
        self.try_sign_message(message).unwrap_or_default()
    }
    /// Fallibly produces an Ed25519 signature over the provided `message` bytes.
    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError>;
    /// Whether the impelmentation requires user interaction to sign
    fn is_interactive(&self) -> bool;
}

impl<T> From<T> for Box<dyn Signer>
where
    T: Signer + 'static,
{
    fn from(signer: T) -> Self {
        Box::new(signer)
    }
}

impl PartialEq for dyn Signer {
    fn eq(&self, other: &dyn Signer) -> bool {
        self.pubkey() == other.pubkey()
    }
}

impl std::fmt::Debug for dyn Signer {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "Signer: {:?}", self.pubkey())
    }
}

/// Removes duplicate signers while preserving order. O(n²)
pub fn unique_signers(signers: Vec<&dyn Signer>) -> Vec<&dyn Signer> {
    signers.into_iter().unique_by(|s| s.pubkey()).collect()
}

#[cfg(test)]
mod tests {
    use {super::*, crate::signer::keypair::Keypair};

    fn pubkeys(signers: &[&dyn Signer]) -> Vec<Pubkey> {
        signers.iter().map(|x| x.pubkey()).collect()
    }

    #[test]
    fn test_unique_signers() {
        let alice = Keypair::new();
        let bob = Keypair::new();
        assert_eq!(
            pubkeys(&unique_signers(vec![&alice, &bob, &alice])),
            pubkeys(&[&alice, &bob])
        );
    }
}
