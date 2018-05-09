//! The `signature` module provides functionality for public, and private keys.

use generic_array::typenum::{U32, U64};
use generic_array::GenericArray;
use ring::signature::Ed25519KeyPair;
use ring::{rand, signature};
use untrusted;

pub type KeyPair = Ed25519KeyPair;
pub type PublicKey = GenericArray<u8, U32>;
pub type Signature = GenericArray<u8, U64>;

pub trait KeyPairUtil {
    fn new() -> Self;
    fn pubkey(&self) -> PublicKey;
}

impl KeyPairUtil for Ed25519KeyPair {
    /// Return a new ED25519 keypair
    fn new() -> Self {
        let rng = rand::SystemRandom::new();
        let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng).expect("failed to generate_pkcs8");
        signature::Ed25519KeyPair::from_pkcs8(untrusted::Input::from(&pkcs8_bytes)).expect("failed to construct Ed25519KeyPair")
    }

    /// Return the public key for the given keypair
    fn pubkey(&self) -> PublicKey {
        GenericArray::clone_from_slice(self.public_key_bytes())
    }
}

pub trait SignatureUtil {
    fn verify(&self, peer_public_key_bytes: &[u8], msg_bytes: &[u8]) -> bool;
}

impl SignatureUtil for GenericArray<u8, U64> {
    fn verify(&self, peer_public_key_bytes: &[u8], msg_bytes: &[u8]) -> bool {
        let peer_public_key = untrusted::Input::from(peer_public_key_bytes);
        let msg = untrusted::Input::from(msg_bytes);
        let sig = untrusted::Input::from(self);
        signature::verify(&signature::ED25519, peer_public_key, msg, sig).is_ok()
    }
}
