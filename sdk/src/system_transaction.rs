//! The `system_transaction` module provides functionality for creating system transactions.
#![cfg(feature = "full")]

use crate::{
    hash::Hash,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};

/// Create and sign new SystemInstruction::CreateAccount transaction
pub fn create_account(
    from_keypair: &Keypair,
    to_keypair: &Keypair,
    recent_blockhash: Hash,
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let to_pubkey = to_keypair.pubkey();
    let instruction =
        system_instruction::create_account(&from_pubkey, &to_pubkey, lamports, space, program_id);
    let message = Message::new(&[instruction], Some(&from_pubkey));
    Transaction::new(&[from_keypair, to_keypair], message, recent_blockhash)
}

/// Create and sign new SystemInstruction::Allocate transaction
pub fn allocate(
    payer_keypair: &Keypair,
    account_keypair: &Keypair,
    recent_blockhash: Hash,
    space: u64,
) -> Transaction {
    let payer_pubkey = payer_keypair.pubkey();
    let account_pubkey = account_keypair.pubkey();
    let instruction = system_instruction::allocate(&account_pubkey, space);
    let message = Message::new(&[instruction], Some(&payer_pubkey));
    Transaction::new(&[payer_keypair, account_keypair], message, recent_blockhash)
}

/// Create and sign new system_instruction::Assign transaction
pub fn assign(from_keypair: &Keypair, recent_blockhash: Hash, program_id: &Pubkey) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let instruction = system_instruction::assign(&from_pubkey, program_id);
    let message = Message::new(&[instruction], Some(&from_pubkey));
    Transaction::new(&[from_keypair], message, recent_blockhash)
}

/// Create and sign new system_instruction::Transfer transaction
pub fn transfer(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let instruction = system_instruction::transfer(&from_pubkey, to, lamports);
    let message = Message::new(&[instruction], Some(&from_pubkey));
    Transaction::new(&[from_keypair], message, recent_blockhash)
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
    let instruction = system_instruction::transfer(&from_pubkey, to, lamports);
    let message = Message::new_with_nonce(
        vec![instruction],
        Some(&from_pubkey),
        nonce_account,
        &nonce_authority.pubkey(),
    );
    Transaction::new(&[from_keypair, nonce_authority], message, nonce_hash)
}
