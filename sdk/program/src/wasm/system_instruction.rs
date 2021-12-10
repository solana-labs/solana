//! `SystemInstruction` Javascript interface
#![cfg(target_arch = "wasm32")]
#![allow(non_snake_case)]
use {
    crate::{instruction::Instruction, pubkey::Pubkey, system_instruction::*},
    wasm_bindgen::prelude::*,
};

#[wasm_bindgen]
impl SystemInstruction {
    pub fn createAccount(
        from_pubkey: &Pubkey,
        to_pubkey: &Pubkey,
        lamports: u64,
        space: u64,
        owner: &Pubkey,
    ) -> Instruction {
        create_account(from_pubkey, to_pubkey, lamports, space, owner)
    }

    pub fn createAccountWithSeed(
        from_pubkey: &Pubkey,
        to_pubkey: &Pubkey,
        base: &Pubkey,
        seed: &str,
        lamports: u64,
        space: u64,
        owner: &Pubkey,
    ) -> Instruction {
        create_account_with_seed(from_pubkey, to_pubkey, base, seed, lamports, space, owner)
    }

    pub fn assign(pubkey: &Pubkey, owner: &Pubkey) -> Instruction {
        assign(pubkey, owner)
    }

    pub fn assignWithSeed(
        pubkey: &Pubkey,
        base: &Pubkey,
        seed: &str,
        owner: &Pubkey,
    ) -> Instruction {
        assign_with_seed(pubkey, base, seed, owner)
    }

    pub fn transfer(from_pubkey: &Pubkey, to_pubkey: &Pubkey, lamports: u64) -> Instruction {
        transfer(from_pubkey, to_pubkey, lamports)
    }

    pub fn transferWithSeed(
        from_pubkey: &Pubkey,
        from_base: &Pubkey,
        from_seed: String,
        from_owner: &Pubkey,
        to_pubkey: &Pubkey,
        lamports: u64,
    ) -> Instruction {
        transfer_with_seed(
            from_pubkey,
            from_base,
            from_seed,
            from_owner,
            to_pubkey,
            lamports,
        )
    }

    pub fn allocate(pubkey: &Pubkey, space: u64) -> Instruction {
        allocate(pubkey, space)
    }

    pub fn allocateWithSeed(
        address: &Pubkey,
        base: &Pubkey,
        seed: &str,
        space: u64,
        owner: &Pubkey,
    ) -> Instruction {
        allocate_with_seed(address, base, seed, space, owner)
    }

    pub fn createNonceAccount(
        from_pubkey: &Pubkey,
        nonce_pubkey: &Pubkey,
        authority: &Pubkey,
        lamports: u64,
    ) -> js_sys::Array {
        let instructions = create_nonce_account(from_pubkey, nonce_pubkey, authority, lamports);
        instructions.into_iter().map(JsValue::from).collect()
    }

    pub fn advanceNonceAccount(nonce_pubkey: &Pubkey, authorized_pubkey: &Pubkey) -> Instruction {
        advance_nonce_account(nonce_pubkey, authorized_pubkey)
    }

    pub fn withdrawNonceAccount(
        nonce_pubkey: &Pubkey,
        authorized_pubkey: &Pubkey,
        to_pubkey: &Pubkey,
        lamports: u64,
    ) -> Instruction {
        withdraw_nonce_account(nonce_pubkey, authorized_pubkey, to_pubkey, lamports)
    }

    pub fn authorizeNonceAccount(
        nonce_pubkey: &Pubkey,
        authorized_pubkey: &Pubkey,
        new_authority: &Pubkey,
    ) -> Instruction {
        authorize_nonce_account(nonce_pubkey, authorized_pubkey, new_authority)
    }
}
