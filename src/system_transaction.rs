//! The `system_transaction` module provides functionality for creating system transactions.

use bincode::serialize;
use hash::Hash;
use signature::{Keypair, KeypairUtil, Pubkey};
use system_program::SystemProgram;
use transaction::Transaction;

pub trait SystemTransaction {
    fn system_create(
        from_keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
        tokens: i64,
        space: u64,
        program_id: Pubkey,
        fee: i64,
    ) -> Self;

    fn system_assign(from_keypair: &Keypair, last_id: Hash, program_id: Pubkey, fee: i64) -> Self;

    fn system_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self;

    fn system_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    fn system_load(
        from_keypair: &Keypair,
        last_id: Hash,
        fee: i64,
        program_id: Pubkey,
        name: String,
    ) -> Self;
}

impl SystemTransaction for Transaction {
    /// Create and sign new SystemProgram::CreateAccount transaction
    fn system_create(
        from_keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
        tokens: i64,
        space: u64,
        program_id: Pubkey,
        fee: i64,
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
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemProgram::CreateAccount transaction
    fn system_assign(from_keypair: &Keypair, last_id: Hash, program_id: Pubkey, fee: i64) -> Self {
        let create = SystemProgram::Assign { program_id };
        Transaction::new(
            from_keypair,
            &[],
            SystemProgram::id(),
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemProgram::CreateAccount transaction with some defaults
    fn system_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self {
        Transaction::system_create(from_keypair, to, last_id, tokens, 0, Pubkey::default(), 0)
    }
    /// Create and sign new SystemProgram::Move transaction
    fn system_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        let create = SystemProgram::Move { tokens };
        Transaction::new(
            from_keypair,
            &[to],
            SystemProgram::id(),
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemProgram::Load transaction
    fn system_load(
        from_keypair: &Keypair,
        last_id: Hash,
        fee: i64,
        program_id: Pubkey,
        name: String,
    ) -> Self {
        let load = SystemProgram::Load { program_id, name };
        Transaction::new(
            from_keypair,
            &[],
            SystemProgram::id(),
            serialize(&load).unwrap(),
            last_id,
            fee,
        )
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
            memfind(&tx_bytes, &tx.from().as_ref()),
            Some(PUB_KEY_OFFSET)
        );
        assert!(tx.verify_signature());
    }

    #[test]
    fn test_userdata_layout() {
        let mut tx0 = test_tx();
        tx0.userdata = vec![1, 2, 3];
        let sign_data0a = tx0.get_sign_data();
        let tx_bytes = serialize(&tx0).unwrap();
        assert!(tx_bytes.len() < PACKET_DATA_SIZE);
        assert_eq!(memfind(&tx_bytes, &sign_data0a), Some(SIGNED_DATA_OFFSET));
        assert_eq!(
            memfind(&tx_bytes, &tx0.signature.as_ref()),
            Some(SIG_OFFSET)
        );
        assert_eq!(
            memfind(&tx_bytes, &tx0.from().as_ref()),
            Some(PUB_KEY_OFFSET)
        );
        let tx1 = deserialize(&tx_bytes).unwrap();
        assert_eq!(tx0, tx1);
        assert_eq!(tx1.userdata, vec![1, 2, 3]);

        tx0.userdata = vec![1, 2, 4];
        let sign_data0b = tx0.get_sign_data();
        assert_ne!(sign_data0a, sign_data0b);
    }
}
