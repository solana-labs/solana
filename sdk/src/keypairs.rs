use crate::{
    pubkey::Pubkey,
    signature::{KeypairUtil, Signature},
};

pub trait Keypairs {
    fn pubkeys(&self) -> Vec<Pubkey>;
    fn sign_message(&self, message: &[u8]) -> Vec<Signature>;
}

impl<T: KeypairUtil> Keypairs for [&T] {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|keypair| keypair.pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }
}

impl Keypairs for [Box<dyn KeypairUtil>] {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|keypair| keypair.pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }
}

impl<T: KeypairUtil> Keypairs for [&T; 0] {
    fn pubkeys(&self) -> Vec<Pubkey> {
        vec![]
    }

    fn sign_message(&self, _message: &[u8]) -> Vec<Signature> {
        vec![]
    }
}

impl<T: KeypairUtil> Keypairs for [&T; 1] {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|keypair| keypair.pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }
}

impl<T: KeypairUtil> Keypairs for [&T; 2] {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|keypair| keypair.pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }
}

impl<T: KeypairUtil> Keypairs for [&T; 3] {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|keypair| keypair.pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }
}

impl<T: KeypairUtil> Keypairs for [&T; 4] {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|keypair| keypair.pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }
}

impl<T: KeypairUtil> Keypairs for Vec<&T> {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|keypair| keypair.pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter()
            .map(|keypair| keypair.sign_message(message))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error;

    struct Foo;
    impl KeypairUtil for Foo {
        fn try_pubkey(&self) -> Result<Pubkey, Box<dyn error::Error>> {
            Ok(Pubkey::default())
        }
        fn try_sign_message(&self, _message: &[u8]) -> Result<Signature, Box<dyn error::Error>> {
            Ok(Signature::default())
        }
    }

    struct Bar;
    impl KeypairUtil for Bar {
        fn try_pubkey(&self) -> Result<Pubkey, Box<dyn error::Error>> {
            Ok(Pubkey::default())
        }
        fn try_sign_message(&self, _message: &[u8]) -> Result<Signature, Box<dyn error::Error>> {
            Ok(Signature::default())
        }
    }

    #[test]
    fn test_dyn_keypairs_compile() {
        let xs: Vec<Box<dyn KeypairUtil>> = vec![Box::new(Foo {}), Box::new(Bar {})];
        assert_eq!(
            xs.sign_message(b""),
            vec![Signature::default(), Signature::default()],
        );

        // Same as above, but less compiler magic.
        let xs_ref: &[Box<dyn KeypairUtil>] = &xs;
        assert_eq!(
            Keypairs::sign_message(xs_ref, b""),
            vec![Signature::default(), Signature::default()],
        );
    }
}
