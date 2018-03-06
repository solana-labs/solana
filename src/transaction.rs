//! The `transaction` crate provides functionality for creating log transactions.

use generic_array::GenericArray;
use generic_array::typenum::{U32, U64};
use ring::signature::Ed25519KeyPair;
use ring::{rand, signature};
use untrusted;
use serde::Serialize;
use bincode::serialize;
use log::Sha256Hash;

pub type PublicKey = GenericArray<u8, U32>;
pub type Signature = GenericArray<u8, U64>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction<T> {
    pub from: PublicKey,
    pub to: PublicKey,
    pub data: T,
    pub last_id: Sha256Hash,
    pub sig: Signature,
}

impl<T: Serialize> Transaction<T> {
    pub fn new_claim(to: PublicKey, data: T, last_id: Sha256Hash, sig: Signature) -> Self {
        Transaction {
            from: to,
            to,
            data,
            last_id,
            sig,
        }
    }

    pub fn verify(&self) -> bool {
        let sign_data = serialize(&(&self.from, &self.to, &self.data, &self.last_id)).unwrap();
        verify_signature(&self.from, &sign_data, &self.sig)
    }
}

/// Return a new ED25519 keypair
pub fn generate_keypair() -> Ed25519KeyPair {
    let rng = rand::SystemRandom::new();
    let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    signature::Ed25519KeyPair::from_pkcs8(untrusted::Input::from(&pkcs8_bytes)).unwrap()
}

/// Return the public key for the given keypair
pub fn get_pubkey(keypair: &Ed25519KeyPair) -> PublicKey {
    GenericArray::clone_from_slice(keypair.public_key_bytes())
}

/// Return a signature for the given data using the private key from the given keypair.
fn sign_serialized<T: Serialize>(data: &T, keypair: &Ed25519KeyPair) -> Signature {
    let serialized = serialize(data).unwrap();
    GenericArray::clone_from_slice(keypair.sign(&serialized).as_ref())
}

/// Return a signature for the given transaction data using the private key from the given keypair.
pub fn sign_transaction_data<T: Serialize>(
    data: &T,
    keypair: &Ed25519KeyPair,
    to: &PublicKey,
    last_id: &Sha256Hash,
) -> Signature {
    let from = &get_pubkey(keypair);
    sign_serialized(&(from, to, data, last_id), keypair)
}

/// Return a signature for the given data using the private key from the given keypair.
pub fn sign_claim_data<T: Serialize>(
    data: &T,
    keypair: &Ed25519KeyPair,
    last_id: &Sha256Hash,
) -> Signature {
    sign_transaction_data(data, keypair, &get_pubkey(keypair), last_id)
}

/// Verify a signed message with the given public key.
pub fn verify_signature(peer_public_key_bytes: &[u8], msg_bytes: &[u8], sig_bytes: &[u8]) -> bool {
    let peer_public_key = untrusted::Input::from(peer_public_key_bytes);
    let msg = untrusted::Input::from(msg_bytes);
    let sig = untrusted::Input::from(sig_bytes);
    signature::verify(&signature::ED25519, peer_public_key, msg, sig).is_ok()
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
