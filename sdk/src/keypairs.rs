use crate::pubkey::Pubkey;
use crate::signature::{KeypairUtil, Signature};

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

impl<T: KeypairUtil, U: KeypairUtil> Keypairs for (T, U) {
    fn pubkeys(&self) -> Vec<Pubkey> {
        vec![self.0.pubkey(), self.1.pubkey()]
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        vec![self.0.sign_message(message), self.1.sign_message(message)]
    }
}

impl<T: KeypairUtil, U: KeypairUtil, V: KeypairUtil> Keypairs for (T, U, V) {
    fn pubkeys(&self) -> Vec<Pubkey> {
        vec![self.0.pubkey(), self.1.pubkey(), self.2.pubkey()]
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        vec![
            self.0.sign_message(message),
            self.1.sign_message(message),
            self.2.sign_message(message),
        ]
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
