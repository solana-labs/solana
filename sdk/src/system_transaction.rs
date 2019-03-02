//! The `system_transaction` module provides functionality for creating system transactions.

use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::Keypair;
use crate::system_instruction::SystemInstruction;
use crate::system_program;
use crate::transaction::{Instruction, Transaction};

pub struct SystemTransaction {}

impl SystemTransaction {
    /// Create and sign new SystemInstruction::CreateAccount transaction
    pub fn new_program_account(
        from_keypair: &Keypair,
        to: Pubkey,
        recent_block_hash: Hash,
        tokens: u64,
        space: u64,
        program_id: Pubkey,
        fee: u64,
    ) -> Transaction {
        let create = SystemInstruction::CreateAccount {
            tokens, //TODO, the tokens to allocate might need to be higher then 0 in the future
            space,
            program_id,
        };
        Transaction::new(
            from_keypair,
            &[to],
            system_program::id(),
            &create,
            recent_block_hash,
            fee,
        )
    }

    /// Create and sign a transaction to create a system account
    pub fn new_account(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: u64,
        recent_block_hash: Hash,
        fee: u64,
    ) -> Transaction {
        let program_id = system_program::id();
        Self::new_program_account(
            from_keypair,
            to,
            recent_block_hash,
            tokens,
            0,
            program_id,
            fee,
        )
    }
    /// Create and sign new SystemInstruction::Assign transaction
    pub fn new_assign(
        from_keypair: &Keypair,
        recent_block_hash: Hash,
        program_id: Pubkey,
        fee: u64,
    ) -> Transaction {
        let assign = SystemInstruction::Assign { program_id };
        Transaction::new(
            from_keypair,
            &[],
            system_program::id(),
            &assign,
            recent_block_hash,
            fee,
        )
    }
    /// Create and sign new SystemInstruction::Move transaction
    pub fn new_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: u64,
        recent_block_hash: Hash,
        fee: u64,
    ) -> Transaction {
        let move_tokens = SystemInstruction::Move { tokens };
        Transaction::new(
            from_keypair,
            &[to],
            system_program::id(),
            &move_tokens,
            recent_block_hash,
            fee,
        )
    }
    /// Create and sign new SystemInstruction::Move transaction to many destinations
    pub fn new_move_many(
        from: &Keypair,
        moves: &[(Pubkey, u64)],
        recent_block_hash: Hash,
        fee: u64,
    ) -> Transaction {
        let instructions: Vec<_> = moves
            .iter()
            .enumerate()
            .map(|(i, (_, amount))| {
                let spend = SystemInstruction::Move { tokens: *amount };
                Instruction::new(0, &spend, vec![0, i as u8 + 1])
            })
            .collect();
        let to_keys: Vec<_> = moves.iter().map(|(to_key, _)| *to_key).collect();

        Transaction::new_with_instructions(
            &[from],
            &to_keys,
            recent_block_hash,
            fee,
            vec![system_program::id()],
            instructions,
        )
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
