//! The `system_transaction` module provides functionality for creating system transactions.

use crate::{
    hash::Hash,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_instruction,
    transaction::Transaction,
};

fn create_account_instructions(
    from_keypair: &Keypair,
    to_keypair: &Keypair,
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Vec<Instruction> {
    let from_pubkey = from_keypair.pubkey();
    let to_pubkey = to_keypair.pubkey();
    return vec![system_instruction::create_account(
        &from_pubkey,
        &to_pubkey,
        lamports,
        space,
        program_id,
    )];
}

pub fn create_account(
    from_keypair: &Keypair,
    to_keypair: &Keypair,
    recent_blockhash: Hash,
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Transaction {
    Transaction::new_signed_instructions(
        &[from_keypair, to_keypair],
        create_account_instructions(from_keypair, to_keypair, lamports, space, program_id),
        recent_blockhash,
    )
}

pub fn create_account_with_dummy_sig(
    from_keypair: &Keypair,
    to_keypair: &Keypair,
    recent_blockhash: Hash,
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Transaction {
    Transaction::new_signed_instructions_random(
        create_account_instructions(from_keypair, to_keypair, lamports, space, program_id),
        recent_blockhash,
    )
}

pub fn assign_instructions(from_keypair: &Keypair, program_id: &Pubkey) -> Vec<Instruction> {
    let from_pubkey = from_keypair.pubkey();
    let assign_instruction = system_instruction::assign(&from_pubkey, program_id);
    return vec![assign_instruction];
}

pub fn assign(from_keypair: &Keypair, recent_blockhash: Hash, program_id: &Pubkey) -> Transaction {
    Transaction::new_signed_instructions(
        &[from_keypair],
        assign_instructions(from_keypair, program_id),
        recent_blockhash,
    )
}

pub fn assign_with_dummy_sig(
    from_keypair: &Keypair,
    recent_blockhash: Hash,
    program_id: &Pubkey,
) -> Transaction {
    Transaction::new_signed_instructions_random(
        assign_instructions(from_keypair, program_id),
        recent_blockhash,
    )
}

fn transfer_instructions(from_keypair: &Keypair, to: &Pubkey, lamports: u64) -> Vec<Instruction> {
    let from_pubkey = from_keypair.pubkey();
    return vec![system_instruction::transfer(&from_pubkey, to, lamports)];
}

pub fn transfer(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    Transaction::new_signed_instructions(
        &[from_keypair],
        transfer_instructions(from_keypair, to, lamports),
        recent_blockhash,
    )
}

pub fn transfer_with_dummy_sig(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    Transaction::new_signed_instructions_random(
        transfer_instructions(from_keypair, to, lamports),
        recent_blockhash,
    )
}

/// Create and sign new nonced system_instruction::Transfer transaction
pub fn nonced_transfer(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    nonce_account: &Pubkey,
    nonce_authority: &Keypair,
    nonce_hash: Hash,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let transfer_instruction = system_instruction::transfer(&from_pubkey, to, lamports);
    let instructions = vec![transfer_instruction];
    Transaction::new_signed_with_nonce(
        instructions,
        Some(&from_pubkey),
        &[from_keypair, nonce_authority],
        nonce_account,
        &nonce_authority.pubkey(),
        nonce_hash,
    )
}
