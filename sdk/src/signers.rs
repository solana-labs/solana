use crate::{
    pubkey::Pubkey,
    signature::{Signature, Signer},
};

pub trait Signers {
    fn pubkeys(&self) -> Vec<Pubkey>;
    fn sign_message(&self, message: &[u8]) -> Vec<Signature>;
}

macro_rules! default_keypairs_impl {
    () => (
            fn pubkeys(&self) -> Vec<Pubkey> {
                self.iter().map(|keypair| keypair.pubkey()).collect()
            }

            fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
                self.iter()
                    .map(|keypair| keypair.sign_message(message))
                    .collect()
            }
    );
}

impl<T: Signer> Signers for [&T] {
    default_keypairs_impl!();
}

impl Signers for [Box<dyn Signer>] {
    default_keypairs_impl!();
}

impl<T: Signer> Signers for [&T; 0] {
    default_keypairs_impl!();
}

impl<T: Signer> Signers for [&T; 1] {
    default_keypairs_impl!();
}

impl<T: Signer> Signers for [&T; 2] {
    default_keypairs_impl!();
}

impl<T: Signer> Signers for [&T; 3] {
    default_keypairs_impl!();
}

impl<T: Signer> Signers for [&T; 4] {
    default_keypairs_impl!();
}

impl<T: Signer> Signers for Vec<&T> {
    default_keypairs_impl!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error;

    struct Foo;
    impl Signer for Foo {
        fn try_pubkey(&self) -> Result<Pubkey, Box<dyn error::Error>> {
            Ok(Pubkey::default())
        }
        fn try_sign_message(&self, _message: &[u8]) -> Result<Signature, Box<dyn error::Error>> {
            Ok(Signature::default())
        }
    }

    struct Bar;
    impl Signer for Bar {
        fn try_pubkey(&self) -> Result<Pubkey, Box<dyn error::Error>> {
            Ok(Pubkey::default())
        }
        fn try_sign_message(&self, _message: &[u8]) -> Result<Signature, Box<dyn error::Error>> {
            Ok(Signature::default())
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
}
