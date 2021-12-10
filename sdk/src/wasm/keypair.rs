//! `Keypair` Javascript interface
#![cfg(target_arch = "wasm32")]
#![allow(non_snake_case)]
use {
    crate::signer::{keypair::Keypair, Signer},
    solana_program::{pubkey::Pubkey, wasm::display_to_jsvalue},
    wasm_bindgen::prelude::*,
};

#[wasm_bindgen]
impl Keypair {
    /// Create a new `Keypair `
    #[wasm_bindgen(constructor)]
    pub fn constructor() -> Keypair {
        Keypair::new()
    }

    /// Convert a `Keypair` to a `Uint8Array`
    pub fn toBytes(&self) -> Box<[u8]> {
        self.to_bytes().into()
    }

    /// Recover a `Keypair` from a `Uint8Array`
    pub fn fromBytes(bytes: &[u8]) -> Result<Keypair, JsValue> {
        Keypair::from_bytes(bytes).map_err(display_to_jsvalue)
    }

    /// Return the `Pubkey` for this `Keypair`
    #[wasm_bindgen(js_name = pubkey)]
    pub fn js_pubkey(&self) -> Pubkey {
        // `wasm_bindgen` does not support traits (`Signer) yet
        self.pubkey()
    }
}
