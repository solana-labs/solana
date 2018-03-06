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
    pub fn new_claim(to: PublicKey, asset: T, last_id: Sha256Hash, sig: Signature) -> Self {
        Transaction {
            from: to,
            to,
            asset,
            last_id,
            sig,
        }
    }

    pub fn verify(&self) -> bool {
        let sign_data = serialize(&(&self.from, &self.to, &self.asset, &self.last_id)).unwrap();
        verify_signature(&self.from, &sign_data, &self.sig)
    }
}

fn sign_serialized<T: Serialize>(asset: &T, keypair: &Ed25519KeyPair) -> Signature {
    let serialized = serialize(asset).unwrap();
    Signature::clone_from_slice(keypair.sign(&serialized).as_ref())
}

/// Return a signature for the given transaction data using the private key from the given keypair.
pub fn sign_transaction_data<T: Serialize>(
    asset: &T,
    keypair: &Ed25519KeyPair,
    to: &PublicKey,
    last_id: &Sha256Hash,
) -> Signature {
    let from = &get_pubkey(keypair);
    sign_serialized(&(from, to, asset, last_id), keypair)
}

/// Return a signature for the given data using the private key from the given keypair.
pub fn sign_claim_data<T: Serialize>(
    asset: &T,
    keypair: &Ed25519KeyPair,
    last_id: &Sha256Hash,
) -> Signature {
    sign_transaction_data(asset, keypair, &get_pubkey(keypair), last_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};

    #[test]
    fn test_serialize_claim() {
        let claim0 = Transaction::new_claim(
            Default::default(),
            0u8,
            Default::default(),
            Default::default(),
        );
        let buf = serialize(&claim0).unwrap();
        let claim1: Transaction<u8> = deserialize(&buf).unwrap();
        assert_eq!(claim1, claim0);
    }
}
