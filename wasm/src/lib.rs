mod utils;

use wasm_bindgen::prelude::*;

use ed25519_dalek::{
    Keypair, PublicKey, SecretKey, Signature, Signer, Verifier, PUBLIC_KEY_LENGTH,
    SECRET_KEY_LENGTH,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::convert::TryFrom;

cfg_if::cfg_if! {
    if #[cfg(consolelog)] {
        use web_sys::console;

        fn console_log(info: &str) {
            console::log_1(&info.into());
        }
    } else {
        fn console_log(_info: &str) {}
    }
}

#[wasm_bindgen]
pub fn setup() {
    console_log("@solana/wasm Initialized. WASM ready for calls.");
    utils::set_panic_hook();
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Pair {
    public_key: Vec<u8>,
    secret_key: Vec<u8>,
}

/// Generate key pair for use with Ed25519 from ed25519_dalek
///
/// * returned struct has two fields public and secret.
#[wasm_bindgen(js_name = generateKeyPair)]
pub fn generate_keypair() -> Result<JsValue, JsValue> {
    // using get random directly instead of OsRng since it's easier to integrate with WASM
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).unwrap();
    let secret = SecretKey::from_bytes(&seed).unwrap();
    let public = PublicKey::from(&secret);
    let keypair = Keypair { secret, public };

    // use all 64bit for secret key to maintain compatibility with tweetnacl
    let secret_key = keypair.to_bytes().to_vec();
    let public_key = keypair.public.as_bytes().to_vec();

    let result = Pair {
        public_key,
        secret_key,
    };
    Ok(JsValue::from_serde(&result).unwrap())
}

/// Create key pair from secret key for use with Ed25519 from ed25519_dalek
/// * secretkey: UIntArray with 64 elements
///
/// * returned struct has two fields public and secret.
#[wasm_bindgen(js_name = keyPairFromSecretKey)]
pub fn from_secret_key(secretkey: &[u8]) -> Result<JsValue, JsValue> {
    // use 64bit for secret key to maintain compatibility with tweetnacl
    let keypair = Keypair::from_bytes(&secretkey).unwrap();
    let secret_key = keypair.to_bytes().to_vec();
    let public_key = keypair.public.as_bytes().to_vec();

    let result = Pair {
        public_key,
        secret_key,
    };
    return Ok(JsValue::from_serde(&result).unwrap());
}

/// Create key pair from secret key for use with Ed25519 from ed25519_dalek
/// * seed: secret key UIntArray with 32 elements
///
/// * returned struct has two fields public and secret.
#[wasm_bindgen(js_name = keyPairFromSeed)]
pub fn keypair_from_seed(seed: &[u8]) -> Result<JsValue, JsValue> {
    // use 64bit for secret key to maintain compatibility with tweetnacl
    let secret = SecretKey::from_bytes(&seed).unwrap();
    let public: PublicKey = (&secret).into();
    let keypair = Keypair { secret, public };
    let public_key = public.as_bytes().to_vec();
    let secret_key = keypair.to_bytes().to_vec();

    let result = Pair {
        public_key,
        secret_key,
    };
    return Ok(JsValue::from_serde(&result).unwrap());
}

/// Sign a message using Ed25519 from ed25519_dalek
/// * pubkey: UIntArray with 32 elements
/// * private: UIntArray with 32 or 64 elements
/// * message: Arbitrary length UIntArray
///
/// * returned vector is the signature consisting of 64 bytes.
#[wasm_bindgen(js_name = signEd25519)]
pub fn ed25519_sign(pubkey: &[u8], seckey: &[u8], message: &[u8]) -> Vec<u8> {
    let secret: SecretKey = SecretKey::from_bytes(&seckey[..SECRET_KEY_LENGTH]).unwrap();
    let public: PublicKey = PublicKey::from_bytes(&pubkey[..PUBLIC_KEY_LENGTH]).unwrap();
    let keypair: Keypair = Keypair { secret, public };

    keypair.sign(message).to_bytes().to_vec()
}

/// Verify signature using Ed25519 from ed25519_dalek
/// * pubkey: UIntArray with 32 elements
/// * signature: UIntArray 64 elements
/// * message: Arbitrary length UIntArray
///
/// * returned vector is the signature consisting of 64 bytes.
#[wasm_bindgen(js_name = verifyEd25519)]
pub fn ed25519_verify(pubkey: &[u8], signature: &[u8], message: &[u8]) -> bool {
    let public = PublicKey::from_bytes(&pubkey[..PUBLIC_KEY_LENGTH]).unwrap();

    let sig = match Signature::try_from(signature) {
        Ok(sig) => sig,
        Err(_) => return false,
    };

    public.verify(message, &sig).is_ok()
}

/// Verify signature using Ed25519 from ed25519_dalek
/// * pubkey: UIntArray with 32 elements
/// * signature: UIntArray 64 elements
/// * message: Arbitrary length UIntArray
///
/// * returned vector is the signature consisting of 64 bytes.
#[wasm_bindgen(js_name = isOnCurveEd25519)]
pub fn ed25519_is_on_curve(pubkey: &[u8]) -> bool {
    return curve25519_dalek::edwards::CompressedEdwardsY::from_slice(&pubkey)
        .decompress()
        .is_some();
}

#[wasm_bindgen(js_name = sha256)]
pub fn hash_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.input(data);
    return hex::encode(hasher.result());
}
