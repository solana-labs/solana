//! The `system_transaction` module provides functionality for creating system transactions.

use hash::Hash;
use signature::{Keypair, KeypairUtil};
use solana_sdk::pubkey::Pubkey;
use system_program::SystemProgram;
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
    /// Create and sign new SystemProgram::CreateAccount transaction
    fn system_create(
        from_keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
        tokens: u64,
        space: u64,
        program_id: Pubkey,
        fee: u64,
    ) -> Self {
        let create = SystemProgram::CreateAccount {
            tokens, //TODO, the tokens to allocate might need to be higher then 0 in the future
            space,
            program_id,
        };
        Transaction::new(
            from_keypair,
            &[to],
            SystemProgram::id(),
            &create,
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemProgram::Assign transaction
    fn system_assign(from_keypair: &Keypair, last_id: Hash, program_id: Pubkey, fee: u64) -> Self {
        let assign = SystemProgram::Assign { program_id };
        Transaction::new(
            from_keypair,
            &[],
            SystemProgram::id(),
            &assign,
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemProgram::CreateAccount transaction with some defaults
    fn system_new(from_keypair: &Keypair, to: Pubkey, tokens: u64, last_id: Hash) -> Self {
        Transaction::system_create(from_keypair, to, last_id, tokens, 0, Pubkey::default(), 0)
    }
    /// Create and sign new SystemProgram::Move transaction
    fn system_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: u64,
        last_id: Hash,
        fee: u64,
    ) -> Self {
        let move_tokens = SystemProgram::Move { tokens };
        Transaction::new(
            from_keypair,
            &[to],
            SystemProgram::id(),
            &move_tokens,
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemProgram::Move transaction to many destinations
    fn system_move_many(from: &Keypair, moves: &[(Pubkey, u64)], last_id: Hash, fee: u64) -> Self {
        let instructions: Vec<_> = moves
            .iter()
            .enumerate()
            .map(|(i, (_, amount))| {
                let spend = SystemProgram::Move { tokens: *amount };
                Instruction::new(0, &spend, vec![0, i as u8 + 1])
            }).collect();
        let to_keys: Vec<_> = moves.iter().map(|(to_key, _)| *to_key).collect();

        Transaction::new_with_instructions(
            from,
            &to_keys,
            last_id,
            fee,
            vec![SystemProgram::id()],
            instructions,
        )
    }
    /// Create and sign new SystemProgram::Spawn transaction
    fn system_spawn(from_keypair: &Keypair, last_id: Hash, fee: u64) -> Self {
        let spawn = SystemProgram::Spawn;
        Transaction::new(from_keypair, &[], SystemProgram::id(), &spawn, last_id, fee)
    }
}

pub fn test_tx() -> Transaction {
    let keypair1 = Keypair::new();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    Transaction::system_new(&keypair1, pubkey1, 42, zero)
}

#[cfg(test)]
pub fn memfind<A: Eq>(a: &[A], b: &[A]) -> Option<usize> {
    assert!(a.len() >= b.len());
    let end = a.len() - b.len() + 1;
    for i in 0..end {
        if a[i..i + b.len()] == b[..] {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};
    use packet::PACKET_DATA_SIZE;
    use transaction::{PUB_KEY_OFFSET, SIGNED_DATA_OFFSET, SIG_OFFSET};

    #[test]
    fn test_layout() {
        let tx = test_tx();
        let sign_data = tx.get_sign_data();
        let tx_bytes = serialize(&tx).unwrap();
        assert_eq!(memfind(&tx_bytes, &sign_data), Some(SIGNED_DATA_OFFSET));
        assert_eq!(memfind(&tx_bytes, &tx.signature.as_ref()), Some(SIG_OFFSET));
        assert_eq!(
            memfind(&tx_bytes, &tx.account_keys[0].as_ref()),
            Some(PUB_KEY_OFFSET)
        );
        assert!(tx.verify_signature());
    }

    #[test]
    fn test_userdata_layout() {
        let mut tx0 = test_tx();
        tx0.instructions[0].userdata = vec![1, 2, 3];
        let sign_data0a = tx0.get_sign_data();
        let tx_bytes = serialize(&tx0).unwrap();
        assert!(tx_bytes.len() < PACKET_DATA_SIZE);
        assert_eq!(memfind(&tx_bytes, &sign_data0a), Some(SIGNED_DATA_OFFSET));
        assert_eq!(
            memfind(&tx_bytes, &tx0.signature.as_ref()),
            Some(SIG_OFFSET)
        );
        assert_eq!(
            memfind(&tx_bytes, &tx0.account_keys[0].as_ref()),
            Some(PUB_KEY_OFFSET)
        );
        let tx1 = deserialize(&tx_bytes).unwrap();
        assert_eq!(tx0, tx1);
        assert_eq!(tx1.instructions[0].userdata, vec![1, 2, 3]);

        tx0.instructions[0].userdata = vec![1, 2, 4];
        let sign_data0b = tx0.get_sign_data();
        assert_ne!(sign_data0a, sign_data0b);
    }
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
