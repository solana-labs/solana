//! The `transaction` crate provides functionality for creating log transactions.

use signature::{get_pubkey, verify_signature, PublicKey, Signature};
use ring::signature::Ed25519KeyPair;
use serde::Serialize;
use bincode::serialize;
use log::Sha256Hash;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction<T> {
    pub from: PublicKey,
    pub to: PublicKey,
    pub asset: T,
    pub last_id: Sha256Hash,
    pub sig: Signature,
}

impl<T: Serialize> Transaction<T> {
    pub fn new(
        from_keypair: &Ed25519KeyPair,
        to: PublicKey,
        asset: T,
        last_id: Sha256Hash,
    ) -> Self {
        let mut tr = Transaction {
            from: get_pubkey(&from_keypair),
            to,
            asset,
            last_id,
            sig: Signature::default(),
        };
        tr.sign(from_keypair);
        tr
    }

    fn get_sign_data(&self) -> Vec<u8> {
        serialize(&(&self.from, &self.to, &self.asset, &self.last_id)).unwrap()
    }

    pub fn sign(&mut self, keypair: &Ed25519KeyPair) {
        let sign_data = self.get_sign_data();
        self.sig = Signature::clone_from_slice(keypair.sign(&sign_data).as_ref());
    }

    pub fn verify(&self) -> bool {
        verify_signature(&self.from, &self.get_sign_data(), &self.sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};
    use log::*;
    use signature::*;

    #[test]
    fn test_claim() {
        let keypair = generate_keypair();
        let asset = hash(b"hello, world");
        let zero = Sha256Hash::default();
        let tr0 = Transaction::new(&keypair, get_pubkey(&keypair), asset, zero);
        assert!(tr0.verify());
    }

    #[test]
    fn test_transfer() {
        let zero = Sha256Hash::default();
        let keypair0 = generate_keypair();
        let keypair1 = generate_keypair();
        let pubkey1 = get_pubkey(&keypair1);
        let asset = hash(b"hello, world");
        let tr0 = Transaction::new(&keypair0, pubkey1, asset, zero);
        assert!(tr0.verify());
    }

    #[test]
    fn test_serialize_claim() {
        let claim0 = Transaction {
            from: Default::default(),
            to: Default::default(),
            asset: 0u8,
            last_id: Default::default(),
            sig: Default::default(),
        };
        let buf = serialize(&claim0).unwrap();
        let claim1: Transaction<u8> = deserialize(&buf).unwrap();
        assert_eq!(claim1, claim0);
    }

    #[test]
    fn test_bad_event_signature() {
        let zero = Sha256Hash::default();
        let keypair = generate_keypair();
        let pubkey = get_pubkey(&keypair);
        let mut tr = Transaction::new(&keypair, pubkey, hash(b"hello, world"), zero);
        tr.sign(&keypair);
        tr.asset = hash(b"goodbye cruel world"); // <-- attack!
        assert!(!tr.verify());
    }

    #[test]
    fn test_hijack_attack() {
        let keypair0 = generate_keypair();
        let keypair1 = generate_keypair();
        let thief_keypair = generate_keypair();
        let pubkey1 = get_pubkey(&keypair1);
        let zero = Sha256Hash::default();
        let mut tr = Transaction::new(&keypair0, pubkey1, hash(b"hello, world"), zero);
        tr.sign(&keypair0);
        tr.to = get_pubkey(&thief_keypair); // <-- attack!
        assert!(!tr.verify());
    }
}
