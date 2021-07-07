#![cfg(feature = "full")]

use crate::{
    pubkey::Pubkey,
    signature::Signature,
    signer::{Signer, SignerError},
};

/// NullSigner - A `Signer` implementation that always produces `Signature::default()`.
/// Used as a placeholder for absentee signers whose 'Pubkey` is required to construct
/// the transaction
#[derive(Clone, Debug, Default)]
pub struct NullSigner {
    pubkey: Pubkey,
}

impl NullSigner {
    pub fn new(pubkey: &Pubkey) -> Self {
        Self { pubkey: *pubkey }
    }
}

impl Signer for NullSigner {
    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        Ok(self.pubkey)
    }

    fn try_sign_message(&self, _message: &[u8]) -> Result<Signature, SignerError> {
        Ok(Signature::default())
    }

    fn is_interactive(&self) -> bool {
        false
    }
}

impl<T> PartialEq<T> for NullSigner
where
    T: Signer,
{
    fn eq(&self, other: &T) -> bool {
        self.pubkey == other.pubkey()
    }
}
