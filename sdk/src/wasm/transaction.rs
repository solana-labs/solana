//! `Transaction` Javascript interface
#![cfg(target_arch = "wasm32")]
#![allow(non_snake_case)]
use {
    crate::{hash::Hash, message::Message, signer::keypair::Keypair, transaction::Transaction},
    solana_program::{
        pubkey::Pubkey,
        wasm::{display_to_jsvalue, instructions::Instructions},
    },
    wasm_bindgen::prelude::*,
};

#[wasm_bindgen]
impl Transaction {
    /// Create a new `Transaction`
    #[wasm_bindgen(constructor)]
    pub fn constructor(instructions: Instructions, payer: Option<Pubkey>) -> Transaction {
        let instructions: Vec<_> = instructions.into();
        Transaction::new_with_payer(&instructions, payer.as_ref())
    }

    /// Return a message containing all data that should be signed.
    #[wasm_bindgen(js_name = message)]
    pub fn js_message(&self) -> Message {
        self.message.clone()
    }

    /// Return the serialized message data to sign.
    pub fn messageData(&self) -> Box<[u8]> {
        self.message_data().into()
    }

    /// Verify the transaction
    #[wasm_bindgen(js_name = verify)]
    pub fn js_verify(&self) -> Result<(), JsValue> {
        self.verify().map_err(display_to_jsvalue)
    }

    pub fn partialSign(&mut self, keypair: &Keypair, recent_blockhash: &Hash) {
        self.partial_sign(&[keypair], *recent_blockhash);
    }

    pub fn isSigned(&self) -> bool {
        self.is_signed()
    }

    pub fn toBytes(&self) -> Box<[u8]> {
        bincode::serialize(self).unwrap().into()
    }

    pub fn fromBytes(bytes: &[u8]) -> Result<Transaction, JsValue> {
        bincode::deserialize(bytes).map_err(display_to_jsvalue)
    }
}
