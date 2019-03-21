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
    pub fn new_program_account(
        from_keypair: &Keypair,
        to: &Pubkey,
        recent_blockhash: Hash,
        lamports: u64,
        space: u64,
        program_id: &Pubkey,
        fee: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let create_instruction =
            SystemInstruction::new_program_account(&from_pubkey, to, lamports, space, program_id);
        let instructions = vec![create_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, fee)
    }

    /// Create and sign a transaction to create a system account
    pub fn new_account(
        from_keypair: &Keypair,
        to: &Pubkey,
        lamports: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let program_id = system_program::id();
        Self::new_program_account(
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
        fee: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let assign_instruction = SystemInstruction::new_assign(&from_pubkey, program_id);
        let instructions = vec![assign_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, fee)
    }

    /// Create and sign new SystemInstruction::Move transaction
    pub fn new_move(
        from_keypair: &Keypair,
        to: &Pubkey,
        lamports: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let move_instruction = SystemInstruction::new_move(&from_pubkey, to, lamports);
        let instructions = vec![move_instruction];
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, fee)
    }

    /// Create and sign new SystemInstruction::Move transaction to many destinations
    pub fn new_move_many(
        from_keypair: &Keypair,
        to_lamports: &[(Pubkey, u64)],
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let from_pubkey = from_keypair.pubkey();
        let instructions: Vec<_> = to_lamports
            .iter()
            .map(|(pubkey, lamports)| SystemInstruction::new_move(&from_pubkey, pubkey, *lamports))
            .collect();
        Transaction::new_signed_instructions(&[from_keypair], instructions, recent_blockhash, fee)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::KeypairUtil;

    #[test]
    fn test_move_many() {
        let from = Keypair::new();
        let t1 = Keypair::new();
        let t2 = Keypair::new();
        let moves = vec![(t1.pubkey(), 1), (t2.pubkey(), 2)];

        let tx = SystemTransaction::new_move_many(&from, &moves, Hash::default(), 0);
        assert_eq!(tx.account_keys[0], from.pubkey());
        assert_eq!(tx.account_keys[1], t1.pubkey());
        assert_eq!(tx.account_keys[2], t2.pubkey());
        assert_eq!(tx.instructions.len(), 2);
        assert_eq!(tx.instructions[0].accounts, vec![0, 1]);
        assert_eq!(tx.instructions[1].accounts, vec![0, 2]);
    }
}
