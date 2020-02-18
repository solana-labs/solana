//! @brief Example Rust-based BPF program that issues a cross-program-invocation

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

pub fn create_instruction(
    program_id: Pubkey,
    arguments: &[(&Pubkey, bool, bool)],
    data: Vec<u8>,
) -> Instruction {
    let mut accounts = Vec::with_capacity(arguments.len() + 1);
    for (key, is_writable, is_signer) in arguments.iter() {
        if *is_writable {
            accounts.push(AccountMeta::new(**key, *is_signer));
        } else {
            accounts.push(AccountMeta::new_readonly(**key, *is_signer));
        }
    }
    Instruction {
        program_id,
        accounts,
        data,
    }
}
