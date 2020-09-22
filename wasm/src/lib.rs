extern crate ed25519_dalek;
extern crate rand;
mod utils;

use wasm_bindgen::prelude::*;
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer as _, SECRET_KEY_LENGTH, PUBLIC_KEY_LENGTH};
use serde::{Deserialize, Serialize};
use rand::rngs::OsRng;
use web_sys::console;

cfg_if::cfg_if! {
	if #[cfg(consolelog)] {
		fn console_log(info: &str) {
			console::log_1(&info.into());
		}
	} else {
		fn console_log(info: &str) {}
	}
}

#[wasm_bindgen]
pub fn setup() {
	console_log("@solana/wasm Initialized. WASM ready for calls.");
	utils::set_panic_hook();
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize, Debug)]
pub struct Pair{
	publicKey: Vec<u8>,
	secretKey: Vec<u8>
}

/// Generates key pair for use with ED25519 from ed25519_dalek
///
/// * returned struct has two fields public and secret.
#[wasm_bindgen(js_name = generateKeyPair)]
pub fn generate_keypair() -> Result<JsValue, JsValue> {
	let mut csprng = OsRng{};
	let keypair = Keypair::generate(&mut csprng);
	// use all 64bit for secret key to maintain compatibility with tweetnacl
	let secretKey = keypair.to_bytes().to_vec();
	let publicKey = keypair.public.as_bytes().to_vec();

	let result = Pair{ publicKey, secretKey};
	Ok(JsValue::from_serde(&result).unwrap())
}

/// Sign a message using ED25519 from ed25519_dalek
/// * pubkey: UIntArray with 32 element
/// * private: UIntArray with 64 element
/// * message: Arbitrary length UIntArray
///
/// * returned vector is the signature consisting of 64 bytes.
#[wasm_bindgen(js_name = signED25519)]
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
