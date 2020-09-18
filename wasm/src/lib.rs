extern crate ed25519_dalek;

mod utils;

use wasm_bindgen::prelude::*;
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signature, Signer as _, Verifier as _, SECRET_KEY_LENGTH, PUBLIC_KEY_LENGTH};
use web_sys::console;

#[wasm_bindgen]
pub fn setup() {
	console::log_1(&"Setting up @solana/wasm".into());
	utils::set_panic_hook();
}

/// Sign a message using ED25519 from ed25519_dalek
/// * pubkey: UIntArray with 32 element
/// * private: UIntArray with 64 element
/// * message: Arbitrary length UIntArray
///
/// * returned vector is the signature consisting of 64 bytes.
#[wasm_bindgen]
pub fn ed25519_sign(pubkey: &[u8], seckey: &[u8], message: &[u8]) -> Vec<u8> {
	let secret: SecretKey = SecretKey::from_bytes(&seckey[..SECRET_KEY_LENGTH]).unwrap();
	let public: PublicKey = PublicKey::from_bytes(&pubkey[..PUBLIC_KEY_LENGTH]).unwrap();
	let keypair: Keypair  = Keypair{ secret: secret, public: public };

    keypair
		.sign(message)
		.to_bytes()
		.to_vec()
}
