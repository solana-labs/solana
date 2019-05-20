//! The `system_transaction` module provides functionality for creating system transactions.

use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil};
use crate::system_instruction;
use crate::system_program;
use crate::transaction::Transaction;

/// Create and sign new SystemInstruction::CreateAccount transaction
pub fn create_account(
    from_keypair: &Keypair,
    to: &Pubkey,
    recent_blockhash: Hash,
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let create_instruction =
        system_instruction::create_account(&from_pubkey, to, lamports, space, program_id);
    let instructions = vec![create_instruction];
    Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash)
}

/// Create and sign a transaction to create a system account
pub fn create_user_account(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let program_id = system_program::id();
    create_account(from_keypair, to, recent_blockhash, lamports, 0, &program_id)
}

/// Create and sign new system_instruction::Assign transaction
pub fn assign(from_keypair: &Keypair, recent_blockhash: Hash, program_id: &Pubkey) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let assign_instruction = system_instruction::assign(&from_pubkey, program_id);
    let instructions = vec![assign_instruction];
    Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash)
}

/// Create and sign new system_instruction::Transfer transaction
pub fn transfer(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let transfer_instruction = system_instruction::transfer(&from_pubkey, to, lamports);
    let instructions = vec![transfer_instruction];
    Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash)
}
