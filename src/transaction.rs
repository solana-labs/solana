//! The `transaction` crate provides functionality for creating log transactions.

use signature::{KeyPair, KeyPairUtil, PublicKey, Signature, SignatureUtil};
use serde::Serialize;
use bincode::serialize;
use hash::Hash;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction<T> {
    pub from: PublicKey,
    pub to: PublicKey,
    pub asset: T,
    pub last_id: Hash,
    pub sig: Signature,
}

impl<T: Serialize> Transaction<T> {
    pub fn new(from_keypair: &KeyPair, to: PublicKey, asset: T, last_id: Hash) -> Self {
        let mut tr = Transaction {
            from: from_keypair.pubkey(),
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

    pub fn sign(&mut self, keypair: &KeyPair) {
        let sign_data = self.get_sign_data();
        self.sig = Signature::clone_from_slice(keypair.sign(&sign_data).as_ref());
    }

    pub fn verify(&self) -> bool {
        self.sig.verify(&self.from, &self.get_sign_data())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};
    use hash::hash;

    #[test]
    fn test_claim() {
        let keypair = KeyPair::new();
        let asset = hash(b"hello, world");
        let zero = Hash::default();
        let tr0 = Transaction::new(&keypair, keypair.pubkey(), asset, zero);
        assert!(tr0.verify());
    }

    #[test]
    fn test_transfer() {
        let zero = Hash::default();
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let pubkey1 = keypair1.pubkey();
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
        let zero = Hash::default();
        let keypair = KeyPair::new();
        let pubkey = keypair.pubkey();
        let mut tr = Transaction::new(&keypair, pubkey, hash(b"hello, world"), zero);
        tr.sign(&keypair);
        tr.asset = hash(b"goodbye cruel world"); // <-- attack!
        assert!(!tr.verify());
    }

    #[test]
    fn test_hijack_attack() {
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let thief_keypair = KeyPair::new();
        let pubkey1 = keypair1.pubkey();
        let zero = Hash::default();
        let mut tr = Transaction::new(&keypair0, pubkey1, hash(b"hello, world"), zero);
        tr.sign(&keypair0);
        tr.to = thief_keypair.pubkey(); // <-- attack!
        assert!(!tr.verify());
    }
}
