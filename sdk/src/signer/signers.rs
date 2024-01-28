#![cfg(feature = "full")]

use crate::{
    pubkey::Pubkey,
    signature::{Signature, Signer, SignerError},
};

/// Convenience trait for working with mixed collections of `Signer`s
pub trait Signers {
    fn pubkeys(&self) -> Vec<Pubkey>;
    fn try_pubkeys(&self) -> Result<Vec<Pubkey>, SignerError>;
    fn sign_message(&self, message: &[u8]) -> Vec<Signature>;
    fn try_sign_message(&self, message: &[u8]) -> Result<Vec<Signature>, SignerError>;
    fn is_interactive(&self) -> bool;
}

/// Any `T` where `&T` impls `IntoIterator` yielding
/// references to `Signer`s implements `Signers`.
///
/// This includes [&dyn Signer], &[Box<dyn Signer>],
/// [&dyn Signer; N], Vec<dyn Signer>, Vec<Keypair>, HashSet<Box<dyn Signer>> etc
impl<T: ?Sized, S: Signer + ?Sized> Signers for T
where
    for<'a> &'a T: IntoIterator<Item = &'a S>,
{
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.into_iter().map(|keypair| keypair.pubkey()).collect()
    }

    fn try_pubkeys(&self) -> Result<Vec<Pubkey>, SignerError> {
        let mut pubkeys = Vec::new();
        for keypair in self.into_iter() {
            pubkeys.push(keypair.try_pubkey()?);
        }
        Ok(pubkeys)
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.into_iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Vec<Signature>, SignerError> {
        let mut signatures = Vec::new();
        for keypair in self.into_iter() {
            signatures.push(keypair.try_sign_message(message)?);
        }
        Ok(signatures)
    }

    fn is_interactive(&self) -> bool {
        self.into_iter().any(|s| s.is_interactive())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Foo;
    impl Signer for Foo {
        fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
            Ok(Pubkey::default())
        }
        fn try_sign_message(&self, _message: &[u8]) -> Result<Signature, SignerError> {
            Ok(Signature::default())
        }
        fn is_interactive(&self) -> bool {
            false
        }
    }

    struct Bar;
    impl Signer for Bar {
        fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
            Ok(Pubkey::default())
        }
        fn try_sign_message(&self, _message: &[u8]) -> Result<Signature, SignerError> {
            Ok(Signature::default())
        }
        fn is_interactive(&self) -> bool {
            false
        }
    }

    #[test]
    fn test_dyn_keypairs_compile() {
        let xs: Vec<Box<dyn Signer>> = vec![Box::new(Foo {}), Box::new(Bar {})];
        assert_eq!(
            xs.sign_message(b""),
            vec![Signature::default(), Signature::default()],
        );

        // Same as above, but less compiler magic.
        let xs_ref: &[Box<dyn Signer>] = &xs;
        assert_eq!(
            Signers::sign_message(xs_ref, b""),
            vec![Signature::default(), Signature::default()],
        );
    }

    #[test]
    fn test_dyn_keypairs_by_ref_compile() {
        let foo = Foo {};
        let bar = Bar {};
        let xs: Vec<&dyn Signer> = vec![&foo, &bar];
        assert_eq!(
            xs.sign_message(b""),
            vec![Signature::default(), Signature::default()],
        );

        // Same as above, but less compiler magic.
        let xs_ref: &[&dyn Signer] = &xs;
        assert_eq!(
            Signers::sign_message(xs_ref, b""),
            vec![Signature::default(), Signature::default()],
        );
    }
}
