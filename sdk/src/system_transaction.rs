//! The `system_transaction` module provides functionality for creating system transactions.

use hash::Hash;
use pubkey::Pubkey;
use signature::Keypair;
use system_instruction::SystemInstruction;
use system_program;
use transaction::{Instruction, Transaction};

pub trait SystemTransaction {
    fn system_create(
        from_keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
        tokens: u64,
        space: u64,
        program_id: Pubkey,
        fee: u64,
    ) -> Self;

    fn system_assign(from_keypair: &Keypair, last_id: Hash, program_id: Pubkey, fee: u64) -> Self;

    fn system_new(from_keypair: &Keypair, to: Pubkey, tokens: u64, last_id: Hash) -> Self;

    fn system_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: u64,
        last_id: Hash,
        fee: u64,
    ) -> Self;

    fn system_move_many(
        from_keypair: &Keypair,
        moves: &[(Pubkey, u64)],
        last_id: Hash,
        fee: u64,
    ) -> Self;

    fn system_spawn(from_keypair: &Keypair, last_id: Hash, fee: u64) -> Self;
}

impl SystemTransaction for Transaction {
    /// Create and sign new SystemInstruction::CreateAccount transaction
    fn system_create(
        from_keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
        tokens: u64,
        space: u64,
        program_id: Pubkey,
        fee: u64,
    ) -> Self {
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
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemInstruction::Assign transaction
    fn system_assign(from_keypair: &Keypair, last_id: Hash, program_id: Pubkey, fee: u64) -> Self {
        let assign = SystemInstruction::Assign { program_id };
        Transaction::new(
            from_keypair,
            &[],
            system_program::id(),
            &assign,
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemInstruction::CreateAccount transaction with some defaults
    fn system_new(from_keypair: &Keypair, to: Pubkey, tokens: u64, last_id: Hash) -> Self {
        Transaction::system_create(from_keypair, to, last_id, tokens, 0, Pubkey::default(), 0)
    }
    /// Create and sign new SystemInstruction::Move transaction
    fn system_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: u64,
        last_id: Hash,
        fee: u64,
    ) -> Self {
        let move_tokens = SystemInstruction::Move { tokens };
        Transaction::new(
            from_keypair,
            &[to],
            system_program::id(),
            &move_tokens,
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemInstruction::Move transaction to many destinations
    fn system_move_many(from: &Keypair, moves: &[(Pubkey, u64)], last_id: Hash, fee: u64) -> Self {
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
            last_id,
            fee,
            vec![system_program::id()],
            instructions,
        )
    }
    /// Create and sign new SystemInstruction::Spawn transaction
    fn system_spawn(from_keypair: &Keypair, last_id: Hash, fee: u64) -> Self {
        let spawn = SystemInstruction::Spawn;
        Transaction::new(
            from_keypair,
            &[],
            system_program::id(),
            &spawn,
            last_id,
            fee,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::KeypairUtil;

    #[test]
    fn test_move_many() {
        let from = Keypair::new();
        let t1 = Keypair::new();
        let t2 = Keypair::new();
        let moves = vec![(t1.pubkey(), 1), (t2.pubkey(), 2)];

        let tx = Transaction::system_move_many(&from, &moves, Default::default(), 0);
        assert_eq!(tx.account_keys[0], from.pubkey());
        assert_eq!(tx.account_keys[1], t1.pubkey());
        assert_eq!(tx.account_keys[2], t2.pubkey());
        assert_eq!(tx.instructions.len(), 2);
        assert_eq!(tx.instructions[0].accounts, vec![0, 1]);
        assert_eq!(tx.instructions[1].accounts, vec![0, 2]);
    }
}
