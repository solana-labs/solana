//! The `system_transaction` module provides functionality for creating system transactions.

use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil};
use crate::system_instruction::SystemInstruction;
use crate::system_program;
use crate::transaction::Transaction;

pub struct SystemTransaction {}

impl SystemTransaction {
    /// Create and sign new SystemInstruction::CreateAccount transaction
    pub fn new_account(
        from_keypair: &Keypair,
        to: &Pubkey,
        recent_blockhash: Hash,
        lamports: u64,
        space: u64,
        program_id: &Pubkey,
        _fee: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let create_instruction =
            SystemInstruction::new_account(&from_pubkey, to, lamports, space, program_id);
        let instructions = vec![create_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash)
    }

    /// Create and sign a transaction to create a system account
    pub fn new_user_account(
        from_keypair: &Keypair,
        to: &Pubkey,
        lamports: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let program_id = system_program::id();
        Self::new_account(
            from_keypair,
            to,
            recent_blockhash,
            lamports,
            0,
            &program_id,
            fee,
        )
    }

    /// Create and sign new SystemInstruction::Assign transaction
    pub fn new_assign(
        from_keypair: &Keypair,
        recent_blockhash: Hash,
        program_id: &Pubkey,
        _fee: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let assign_instruction = SystemInstruction::new_assign(&from_pubkey, program_id);
        let instructions = vec![assign_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash)
    }

    /// Create and sign new SystemInstruction::Transfer transaction
    pub fn new_transfer(
        from_keypair: &Keypair,
        to: &Pubkey,
        lamports: u64,
        recent_blockhash: Hash,
        _fee: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let move_instruction = SystemInstruction::new_transfer(&from_pubkey, to, lamports);
        let instructions = vec![move_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash)
    }
}
