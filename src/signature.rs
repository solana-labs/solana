//! The `signature` crate provides functionality for public and private keys

use generic_array::GenericArray;
use generic_array::typenum::{U32, U64};
use ring::signature::Ed25519KeyPair;
use ring::{rand, signature};
use untrusted;

pub type KeyPair = Ed25519KeyPair;
pub type PublicKey = GenericArray<u8, U32>;
pub type Signature = GenericArray<u8, U64>;

/// Return a new ED25519 keypair
pub fn generate_keypair() -> KeyPair {
    let rng = rand::SystemRandom::new();
    let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    signature::Ed25519KeyPair::from_pkcs8(untrusted::Input::from(&pkcs8_bytes)).unwrap()
}

/// Return the public key for the given keypair
pub fn get_pubkey(keypair: &KeyPair) -> PublicKey {
    GenericArray::clone_from_slice(keypair.public_key_bytes())
}

/// Verify a signed message with the given public key.
pub fn verify_signature(peer_public_key_bytes: &[u8], msg_bytes: &[u8], sig_bytes: &[u8]) -> bool {
    let peer_public_key = untrusted::Input::from(peer_public_key_bytes);
    let msg = untrusted::Input::from(msg_bytes);
    let sig = untrusted::Input::from(sig_bytes);
    signature::verify(&signature::ED25519, peer_public_key, msg, sig).is_ok()
}
