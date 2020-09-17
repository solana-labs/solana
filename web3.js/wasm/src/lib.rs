use wasm_bindgen::prelude::*;
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signature, Signer as _, Verifier as _};

#[wasm_bindgen]
extern {
    pub fn alert(s: &str);
}

#[wasm_bindgen]
pub fn greet(name: &str) {
    alert(&format!("Hello, {}!", name));
}

#[wasm_bindgen]
pub fn ext_ed_sign(pubkey: &[u8], seckey: &[u8], message: &[u8]) -> Vec<u8> {
	create_from_parts(pubkey, seckey)
		.sign(message)
		.to_bytes()
		.to_vec()
}
