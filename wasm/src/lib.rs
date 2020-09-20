extern crate ed25519_dalek;
extern crate rand;
mod utils;

use wasm_bindgen::prelude::*;
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer as _, SECRET_KEY_LENGTH, PUBLIC_KEY_LENGTH};
use serde::{Deserialize, Serialize};
use rand::rngs::OsRng;
use web_sys::console;

#[wasm_bindgen]
pub fn setup() {
	console::log_1(&"@solana/wasm Initialized. WASM ready for calls.".into());
	utils::set_panic_hook();
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize, Debug)]
pub struct Pair{
	public: Vec<u8>,
	secret: Vec<u8>
}

/// Generates key pair for use with ED25519 from ed25519_dalek
///
/// * returned struct has two fields public and secret.
#[wasm_bindgen(js_name = GenerateKeyPair)]
pub fn generate_keypair() -> Result<JsValue, JsValue> {
	let mut csprng = OsRng{};
	let keypair = Keypair::generate(&mut csprng);
	let secret = keypair.secret.as_bytes().to_vec();
	let public = keypair.public.as_bytes().to_vec();

	let result = Pair{ public, secret};
	Ok(JsValue::from_serde(&result).unwrap())
}

/// Sign a message using ED25519 from ed25519_dalek
/// * pubkey: UIntArray with 32 element
/// * private: UIntArray with 64 element
/// * message: Arbitrary length UIntArray
///
/// * returned vector is the signature consisting of 64 bytes.
#[wasm_bindgen(js_name = SignED25519)]
pub fn ed25519_sign(pubkey: &[u8], seckey: &[u8], message: &[u8]) -> Vec<u8> {
	let secret: SecretKey = SecretKey::from_bytes(&seckey[..SECRET_KEY_LENGTH]).unwrap();
	let public: PublicKey = PublicKey::from_bytes(&pubkey[..PUBLIC_KEY_LENGTH]).unwrap();
	let keypair: Keypair  = Keypair{ secret: secret, public: public };

    keypair
		.sign(message)
		.to_bytes()
		.to_vec()
}

// TODO: add verification
