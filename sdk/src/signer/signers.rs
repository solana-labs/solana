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

/// Do not make any blanket impls for [impl Signer] or Deref<Target = [impl Signer]> for now because
/// fixed size arrays [T; N] do not implement it yet, but std lib will still
/// throw conflicting impl bec it might impl it in the future.
/// This SignerSlice helper trait helps to unify across [T], [T; N], Vec<T> etc
///
/// TODO: replace this helper trait
/// with blanket over [SlicePattern](https://doc.rust-lang.org/stable/core/slice/trait.SlicePattern.html)
/// or [core::str::Pattern](https://doc.rust-lang.org/stable/core/str/pattern/trait.Pattern.html) once stabilized
trait SignerSlice {
    type Item: Signer;

    fn as_signer_slice(&self) -> &[Self::Item];
}

impl<T: SignerSlice + ?Sized> Signers for T {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.as_signer_slice()
            .iter()
            .map(|keypair| keypair.pubkey())
            .collect()
    }

    fn try_pubkeys(&self) -> Result<Vec<Pubkey>, SignerError> {
        let mut pubkeys = Vec::new();
        for keypair in self.as_signer_slice().iter() {
            pubkeys.push(keypair.try_pubkey()?);
        }
        Ok(pubkeys)
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.as_signer_slice()
            .iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Vec<Signature>, SignerError> {
        let mut signatures = Vec::new();
        for keypair in self.as_signer_slice().iter() {
            signatures.push(keypair.try_sign_message(message)?);
        }
        Ok(signatures)
    }

    fn is_interactive(&self) -> bool {
        self.as_signer_slice().iter().any(|s| s.is_interactive())
    }
}

impl<T: Signer> SignerSlice for [T] {
    type Item = T;

    fn as_signer_slice(&self) -> &[Self::Item] {
        self
    }
}

impl<T: Signer, const N: usize> SignerSlice for [T; N] {
    type Item = T;

    fn as_signer_slice(&self) -> &[Self::Item] {
        self
    }
}

impl<T: Signer> SignerSlice for Vec<T> {
    type Item = T;

    fn as_signer_slice(&self) -> &[Self::Item] {
        self
    }
}

impl<S: SignerSlice + ?Sized> SignerSlice for &S {
    type Item = S::Item;

    fn as_signer_slice(&self) -> &[Self::Item] {
        (**self).as_signer_slice()
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
